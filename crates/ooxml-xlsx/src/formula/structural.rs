//! Structural reference adjustment (ADR-0018 §§2.3 and 3.2).

use super::lexer::{Area, Comp, LexError, Prefix, TokKind, lex, render_area};
use crate::reference::{MAX_COLS, MAX_ROWS};

#[derive(Clone, Copy)]
pub(crate) enum Axis {
    Row,
    Column,
}

#[derive(Clone, Copy)]
pub(crate) enum Shift {
    Insert { at: u32, count: u32 },
    Delete { at: u32, count: u32 },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AreaOutcome {
    Unchanged,
    Moved(Area),
    Resized(Area),
    Deleted,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ShiftOffSheet;

pub(crate) fn shift_area(
    area: Area,
    axis: Axis,
    shift: Shift,
) -> Result<AreaOutcome, ShiftOffSheet> {
    let (first, last, edge) = match (axis, area) {
        (Axis::Row, Area::Cell { row, .. }) => (row, None, MAX_ROWS - 1),
        (Axis::Column, Area::Cell { col, .. }) => (col, None, MAX_COLS - 1),
        (Axis::Row, Area::Range { r1, r2, .. } | Area::Rows { r1, r2 }) => {
            (r1, Some(r2), MAX_ROWS - 1)
        }
        (Axis::Column, Area::Range { c1, c2, .. } | Area::Cols { c1, c2 }) => {
            (c1, Some(c2), MAX_COLS - 1)
        }
        _ => return Ok(AreaOutcome::Unchanged),
    };
    let reversed = last.is_some_and(|end| end.idx < first.idx);
    let other = last.unwrap_or(first);
    let (lo, hi) = if reversed {
        (other.idx, first.idx)
    } else {
        (first.idx, other.idx)
    };
    let pinned = last.is_some() && hi == edge;
    let (start, end) = match shift {
        Shift::Insert { at, count } => {
            if at > hi || (lo == 0 && pinned) {
                return Ok(AreaOutcome::Unchanged);
            }
            let start = if at <= lo {
                lo.checked_add(count)
                    .filter(|&idx| idx <= edge)
                    .ok_or(ShiftOffSheet)?
            } else {
                lo
            };
            let end = if pinned {
                edge
            } else {
                hi.saturating_add(count).min(edge)
            };
            (start, end)
        }
        Shift::Delete { at, count } => {
            let after = at.saturating_add(count);
            if lo >= at && hi < after {
                return Ok(AreaOutcome::Deleted);
            }
            if at > hi || (lo == 0 && pinned) {
                return Ok(AreaOutcome::Unchanged);
            }
            let start = if lo >= after { lo - count } else { lo.min(at) };
            let end = if pinned {
                edge
            } else if hi >= after {
                hi - count
            } else {
                at - 1
            };
            (start, end)
        }
    };
    if (start, end) == (lo, hi) {
        return Ok(AreaOutcome::Unchanged);
    }
    let (first_idx, last_idx) = if reversed { (end, start) } else { (start, end) };
    let first = Comp {
        idx: first_idx,
        ..first
    };
    let last = Comp {
        idx: last_idx,
        ..other
    };
    let updated = match (axis, area) {
        (Axis::Row, Area::Cell { col, .. }) => Area::Cell { col, row: first },
        (Axis::Column, Area::Cell { row, .. }) => Area::Cell { col: first, row },
        (Axis::Row, Area::Range { c1, c2, .. }) => Area::Range {
            c1,
            r1: first,
            c2,
            r2: last,
        },
        (Axis::Column, Area::Range { r1, r2, .. }) => Area::Range {
            c1: first,
            r1,
            c2: last,
            r2,
        },
        (Axis::Row, Area::Rows { .. }) => Area::Rows {
            r1: first,
            r2: last,
        },
        (Axis::Column, Area::Cols { .. }) => Area::Cols {
            c1: first,
            c2: last,
        },
        _ => unreachable!("the edited axis has reference components"),
    };
    Ok(if end - start == hi - lo {
        AreaOutcome::Moved(updated)
    } else {
        AreaOutcome::Resized(updated)
    })
}

/// A zero-based axis mapping. An inserted position has no predecessor,
/// and a deleted position has no successor.
impl Shift {
    pub(crate) fn before(self, index: u32) -> Option<u32> {
        match self {
            Self::Insert { at, count } if index >= at => {
                index.checked_sub(count).filter(|old| *old >= at)
            }
            Self::Delete { at, count } if index >= at => index.checked_add(count),
            _ => Some(index),
        }
    }

    pub(crate) fn after(self, index: u32, limit: u32) -> Result<Option<u32>, ShiftOffSheet> {
        let next = match self {
            Self::Insert { at, count } if index >= at => {
                index.checked_add(count).ok_or(ShiftOffSheet)?
            }
            Self::Delete { at, count } if index >= at => {
                let Some(next) = index.checked_sub(count).filter(|next| *next >= at) else {
                    return Ok(None);
                };
                next
            }
            _ => index,
        };
        if next >= limit {
            return Err(ShiftOffSheet);
        }
        Ok(Some(next))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Rewrite {
    Keep,
    New(String),
    Remove,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RewriteError {
    Lex(LexError),
    OffSheet,
    InvalidReference,
}

impl From<ShiftOffSheet> for RewriteError {
    fn from(_: ShiftOffSheet) -> Self {
        Self::OffSheet
    }
}

/// Adjust only the reference tokens selected by the operation's sheet
/// and anchor context. Copy the spelling of everything else.
pub(crate) fn rewrite_formula(
    text: &str,
    axis: Axis,
    edit: Shift,
    refers_to_sheet: impl Fn(&Prefix) -> bool,
) -> Result<Rewrite, RewriteError> {
    let tokens = lex(text).map_err(RewriteError::Lex)?;
    let mut rewritten = String::new();
    let mut last = 0;
    for token in tokens {
        let TokKind::Ref(reference) = token.kind else {
            continue;
        };
        if matches!(reference.prefix, Prefix::External) || !refers_to_sheet(&reference.prefix) {
            continue;
        }
        match shift_area(reference.area, axis, edit)? {
            AreaOutcome::Unchanged => continue,
            AreaOutcome::Deleted => {
                rewritten.push_str(&text[last..token.start + reference.area_offset]);
                rewritten.push_str("#REF!");
            }
            AreaOutcome::Moved(area) | AreaOutcome::Resized(area) => {
                rewritten.push_str(&text[last..token.start + reference.area_offset]);
                rewritten.push_str(&render_area(&area));
            }
        }
        last = token.end;
    }
    if last == 0 {
        return Ok(Rewrite::Keep);
    }
    rewritten.push_str(&text[last..]);
    lex(&rewritten).map_err(RewriteError::Lex)?;
    Ok(Rewrite::New(rewritten))
}

fn rewrite_reference(
    text: &str,
    axis: Axis,
    edit: Shift,
    cell: bool,
) -> Result<Rewrite, RewriteError> {
    let tokens = lex(text.trim()).map_err(RewriteError::Lex)?;
    let [token] = tokens.as_slice() else {
        return Err(RewriteError::InvalidReference);
    };
    let TokKind::Ref(reference) = &token.kind else {
        return Err(RewriteError::InvalidReference);
    };
    if reference.prefix != Prefix::None
        || matches!(reference.area, Area::RefError)
        || cell && !matches!(reference.area, Area::Cell { .. })
    {
        return Err(RewriteError::InvalidReference);
    }
    Ok(match shift_area(reference.area, axis, edit)? {
        AreaOutcome::Unchanged => Rewrite::Keep,
        AreaOutcome::Moved(area) | AreaOutcome::Resized(area) => Rewrite::New(render_area(&area)),
        AreaOutcome::Deleted => Rewrite::Remove,
    })
}

pub(crate) fn rewrite_range(text: &str, axis: Axis, edit: Shift) -> Result<Rewrite, RewriteError> {
    rewrite_reference(text, axis, edit, false)
}

pub(crate) fn rewrite_cell(text: &str, axis: Axis, edit: Shift) -> Result<Rewrite, RewriteError> {
    rewrite_reference(text, axis, edit, true)
}

pub(crate) fn rewrite_sqref(text: &str, axis: Axis, edit: Shift) -> Result<Rewrite, RewriteError> {
    let mut changed = false;
    let mut output = Vec::new();
    for item in text.split_ascii_whitespace() {
        match rewrite_range(item, axis, edit)? {
            Rewrite::Keep => output.push(item.to_owned()),
            Rewrite::New(value) => {
                changed = true;
                output.push(value);
            }
            Rewrite::Remove => changed = true,
        }
    }
    Ok(if !changed {
        Rewrite::Keep
    } else if output.is_empty() {
        Rewrite::Remove
    } else {
        Rewrite::New(output.join(" "))
    })
}

/// Renames table and column tokens whose binding is established by the caller.
pub(crate) fn rewrite_structured<'a>(
    text: &str,
    table: impl Fn(&str) -> bool,
    new_table: Option<&str>,
    column: impl Fn(&str) -> Option<&'a str>,
) -> Result<Rewrite, RewriteError> {
    use super::structured::{parse, specifier};

    let mut edits = Vec::new();
    for token in lex(text).map_err(RewriteError::Lex)? {
        if !matches!(token.kind, TokKind::Opaque) {
            continue;
        }
        let raw = &text[token.start..token.end];
        let Some(open) = raw.find('[') else { continue };
        if !table(&raw[..open]) {
            continue;
        }
        let items = parse(raw).ok_or(RewriteError::InvalidReference)?;
        if let Some(new) =
            new_table.filter(|new| !items.table_span.is_empty() && *new != &raw[..open])
        {
            edits.push((token.start..token.start + open, new.to_owned()));
        }
        let flat_start = open + 1 + usize::from(raw[open + 1..].starts_with('@'));
        for (span, old) in items.column_spans.into_iter().zip(&items.columns) {
            if let Some(new) = column(old).filter(|new| *new != old) {
                let spelling = specifier(new, span.start != flat_start);
                edits.push((
                    token.start + span.start..token.start + span.end,
                    spelling[1..spelling.len() - 1].to_owned(),
                ));
            }
        }
    }
    if edits.is_empty() {
        return Ok(Rewrite::Keep);
    }
    let mut rewritten = String::with_capacity(text.len());
    let mut last = 0;
    for (span, replacement) in edits {
        rewritten.push_str(&text[last..span.start]);
        rewritten.push_str(&replacement);
        last = span.end;
    }
    rewritten.push_str(&text[last..]);
    lex(&rewritten).map_err(RewriteError::Lex)?;
    Ok(Rewrite::New(rewritten))
}

/// Rewrite a numeric position. Range ends stay at the edge if the range
/// survives. The caller checks for a wholly deleted range first.
pub(crate) fn rewrite_index(
    index: u32,
    axis: Axis,
    edit: Shift,
    range_end: bool,
) -> Result<Rewrite, RewriteError> {
    let limit = match axis {
        Axis::Row => MAX_ROWS,
        Axis::Column => MAX_COLS,
    };
    if range_end && index == limit - 1 {
        return Ok(Rewrite::Keep);
    }
    let next = match edit.after(index, limit) {
        Err(_) if range_end && index < limit => Some(limit - 1),
        outcome => outcome?,
    };
    Ok(match next {
        Some(next) if next == index => Rewrite::Keep,
        Some(next) => Rewrite::New(next.to_string()),
        None => Rewrite::Remove,
    })
}

/// Shift one coordinate of an anchor and preserve its original offset.
pub(crate) fn rewrite_marker(
    index: u32,
    offset: i64,
    axis: Axis,
    edit: Shift,
) -> Result<Option<(u32, i64)>, ShiftOffSheet> {
    let limit = match axis {
        Axis::Row => MAX_ROWS,
        Axis::Column => MAX_COLS,
    };
    Ok(edit.after(index, limit)?.map(|next| (next, offset)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::lexer::{TokKind, lex, render_area};

    fn adjust(text: &str, axis: Axis, shift: Shift) -> Result<String, ShiftOffSheet> {
        let tokens = lex(text).unwrap();
        let TokKind::Ref(reference) = &tokens[0].kind else {
            panic!("{text}")
        };
        Ok(match shift_area(reference.area, axis, shift)? {
            AreaOutcome::Unchanged => text.to_owned(),
            AreaOutcome::Moved(area) | AreaOutcome::Resized(area) => render_area(&area),
            AreaOutcome::Deleted => "#REF!".to_owned(),
        })
    }

    #[test]
    fn rewrites_only_references_selected_by_the_sheet_context() {
        let edit = Shift::Delete { at: 1, count: 2 };
        let formula =
            r#"SUM($A$2:A5, Other!A4, Data!B2, 'My Data'!B2, [1]Data!B2, "Data!A2", #REF!)"#;
        let rewritten = rewrite_formula(formula, Axis::Row, edit, |prefix| match prefix {
            Prefix::None => true,
            Prefix::Sheet(name) => matches!(name.as_str(), "Data" | "My Data"),
            _ => false,
        })
        .unwrap();
        assert_eq!(
            rewritten,
            Rewrite::New(r#"SUM($A$2:A3, Other!A4, Data!#REF!, 'My Data'!#REF!, [1]Data!B2, "Data!A2", #REF!)"#.into())
        );
        assert_eq!(
            rewrite_sqref("A1 B2 A5:D6", Axis::Row, edit).unwrap(),
            Rewrite::New("A1 A3:D4".into())
        );
        assert_eq!(
            rewrite_cell("$B$2", Axis::Row, edit).unwrap(),
            Rewrite::Remove
        );
    }

    #[test]
    fn structured_rewrites_preserve_selectors_and_unrelated_tokens() {
        let text = "SUM(Sales[[#Headers],[Old]:[Keep]],Sales[Old],Other[Old])";
        let adjusted = rewrite_structured(
            text,
            |table| table == "Sales",
            Some("NewSales"),
            |col| (col == "Old").then_some("Net, USD"),
        )
        .unwrap();
        assert_eq!(
            adjusted,
            Rewrite::New(
                "SUM(NewSales[[#Headers],[Net, USD]:[Keep]],NewSales[[Net, USD]],Other[Old])"
                    .into()
            )
        );
    }

    #[test]
    fn inserted_and_deleted_positions_have_no_counterpart() {
        let insert = Shift::Insert { at: 3, count: 2 };
        assert_eq!(insert.before(2), Some(2));
        assert_eq!(insert.before(3), None);
        assert_eq!(insert.before(4), None);
        assert_eq!(insert.before(5), Some(3));
        assert_eq!(insert.after(3, 10), Ok(Some(5)));
        assert_eq!(insert.after(9, 10), Err(ShiftOffSheet));
        let delete = Shift::Delete { at: 3, count: 2 };
        assert_eq!(delete.after(3, 10), Ok(None));
        assert_eq!(delete.after(4, 10), Ok(None));
        assert_eq!(delete.after(5, 10), Ok(Some(3)));
        assert_eq!(
            rewrite_marker(3, 77, Axis::Column, insert),
            Ok(Some((5, 77)))
        );
        assert_eq!(rewrite_marker(3, 77, Axis::Column, delete), Ok(None));
        assert_eq!(
            rewrite_index(5, Axis::Column, delete, false),
            Ok(Rewrite::New("3".into()))
        );
        assert_eq!(delete.before(3), Some(5));
    }

    #[test]
    fn numeric_range_ends_pin_and_clip() {
        let insert = Shift::Insert { at: 2, count: 2 };
        let delete = Shift::Delete { at: 2, count: 2 };
        assert_eq!(
            rewrite_index(MAX_COLS - 1, Axis::Column, insert, true),
            Ok(Rewrite::Keep)
        );
        assert_eq!(
            rewrite_index(MAX_COLS - 1, Axis::Column, delete, true),
            Ok(Rewrite::Keep)
        );
        assert_eq!(
            rewrite_index(MAX_COLS - 2, Axis::Column, insert, true),
            Ok(Rewrite::New("16383".into()))
        );
        assert_eq!(
            rewrite_index(MAX_COLS - 1, Axis::Column, insert, false),
            Err(RewriteError::OffSheet)
        );
    }

    #[test]
    fn adjusts_boundaries_and_sticky_ends() {
        for (from, at, count, to) in [
            ("A2:A4", 0, 2, "A4:A6"),
            ("A2:A4", 1, 2, "A4:A6"),
            ("A2:A4", 2, 2, "A2:A6"),
            ("A2:A4", 4, 2, "A2:A4"),
            ("A2:A1048576", 0, 2, "A4:A1048576"),
            ("A2:A1048575", 1, 2, "A4:A1048576"),
            ("A1:A1048576", 0, 2, "A1:A1048576"),
            ("A:C", 0, 2, "A:C"),
            ("A$4:$A2", 2, 2, "A$6:$A2"),
        ] {
            assert_eq!(
                adjust(from, Axis::Row, Shift::Insert { at, count }).unwrap(),
                to
            );
        }
        assert_eq!(
            adjust("C:XFD", Axis::Column, Shift::Insert { at: 1, count: 2 }).unwrap(),
            "E:XFD"
        );
        assert_eq!(
            adjust("XFD1", Axis::Column, Shift::Insert { at: 1, count: 2 }),
            Err(ShiftOffSheet)
        );
        assert_eq!(
            adjust("XFC1:XFD5", Axis::Column, Shift::Insert { at: 1, count: 2 }),
            Err(ShiftOffSheet)
        );

        for (from, at, count, to) in [
            ("A2:A4", 0, 1, "A1:A3"),
            ("A2:A4", 1, 2, "A2:A2"),
            ("A2:A4", 2, 1, "A2:A3"),
            ("A2:A4", 3, 2, "A2:A3"),
            ("A2:A4", 4, 2, "A2:A4"),
            ("A2:A1048576", 0, 1, "A1:A1048576"),
            ("A1048575:A1048576", 1048574, 2, "#REF!"),
        ] {
            assert_eq!(
                adjust(from, Axis::Row, Shift::Delete { at, count }).unwrap(),
                to
            );
        }
    }
}
