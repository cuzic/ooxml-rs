//! Cross-run find/replace (ADR-0008 §4).
//!
//! A paragraph's text is the concatenation of its items as in Phase 0
//! ([`crate::ParagraphRef::text`]): `a:r` text, `a:br` = `\v`, `a:fld` =
//! its cached text. Matches are literal, left to right, non-overlapping and
//! never cross paragraphs. One match is edited by these rules:
//!
//! 1. a match covering any part of an `a:fld` is refused
//!    ([`Error::FieldInMatch`]), of an `a:br` ([`Error::BreakInMatch`]);
//! 2. the **start run** (where the match starts) keeps its `a:rPr` and
//!    becomes `prefix + replacement (+ suffix if the match ends in it)`;
//! 3. runs fully covered are removed, a partly covered last run is trimmed;
//! 4. a touched run left empty is removed unless no `a:r` would remain
//!    (then the first one stays with an empty `a:t`);
//! 5. a `\v` in the replacement becomes `a:br` + a new run whose `a:rPr`
//!    copies the start run's (the Phase 0 format-source copy); the suffix
//!    goes into the last new run, which is refused when the start run has a
//!    link ([`Error::LinkSplitByBreak`]);
//! 6. [`MixedFormat::Refuse`] refuses a match across runs whose `a:rPr`
//!    differ beyond the proofing attributes ([`Error::MixedFormatInMatch`]);
//! 7. ADR-0005 §6 locks: character-animated or comment-anchored text,
//!    shapes in `mc:AlternateContent`, merged cells, unsupported paragraph
//!    content are refused.
//!
//! `replace_all` plans every match of the scope before the first change;
//! a refused match changes nothing ([`OnRefusal`]).

use std::ops::Range;

use ooxml_core::{ElementSpec, QName, RawItem, RawNode};
use ooxml_schema::dml::{Break, Field, Paragraph, Run, RunProperties, Text};

use crate::error::{Error, Result};
use crate::read::{self, ShapeKind, ShapePath};
use crate::text;
use crate::write::{Fmt, Guard, SlideMut, copy_attrs, node_at, para_shape, set_attrs};

/// Which slides `find`/`replace_all` look at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Scope {
    /// Every slide in `p:sldIdLst` order (a dangling slide is an error).
    #[default]
    Slides,
    /// One slide (0-based index).
    Slide(usize),
}

/// Options of [`crate::Presentation::find`].
#[derive(Clone, Debug, Default)]
pub struct FindOptions {
    /// The slides searched.
    pub scope: Scope,
    /// Fold ASCII letters only (byte offsets stay valid); otherwise exact.
    pub ascii_case_insensitive: bool,
}

/// What a match across visibly different runs does (ADR-0008 §4 rule 6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MixedFormat {
    /// The replacement takes the start run's format (typing over a selection).
    #[default]
    Allow,
    /// Refuse a match whose covered runs differ in `a:rPr` beyond the
    /// proofing attributes, or in their links.
    Refuse,
}

/// What `replace_all` does when some match is refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OnRefusal {
    /// Apply the other matches; list the refused ones in the report.
    #[default]
    Skip,
    /// Change nothing if any match is refused.
    Abort,
}

/// Options of [`crate::Presentation::replace_all`].
#[derive(Clone, Debug, Default)]
pub struct ReplaceOptions {
    /// Scope and matching.
    pub find: FindOptions,
    /// Rule 6.
    pub mixed_format: MixedFormat,
    /// Rule for refusals.
    pub on_refusal: OnRefusal,
}

/// Where a paragraph is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLocation {
    /// Slide index.
    pub slide: usize,
    /// The shape (for a shape in `mc:AlternateContent`: the AC entry).
    pub shape: ShapePath,
    /// `(row, col)` for a table cell (`col` counts `a:tc` in the row).
    pub cell: Option<(usize, usize)>,
    /// Paragraph index in the text body.
    pub paragraph: usize,
}

/// One match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// The paragraph.
    pub location: TextLocation,
    /// Byte range in the paragraph text **before** any replacement.
    pub range: Range<usize>,
}

/// Result of [`crate::Presentation::replace_all`].
#[derive(Debug, Default)]
pub struct ReplaceReport {
    /// Matches replaced (none if `aborted`).
    pub replaced: Vec<Match>,
    /// Refused matches with the reason.
    pub skipped: Vec<(Match, Error)>,
    /// [`OnRefusal::Abort`] and something was refused: nothing changed.
    pub aborted: bool,
}

/// Attributes ignored by [`MixedFormat::Refuse`] (proofing state only).
pub const PROOFING_ATTRIBUTES: &[&str] = &["lang", "altLang", "err", "dirty", "smtClean", "smtId", "noProof", "bmk"];

fn a(local: &'static str) -> QName {
    QName::new(crate::names::A_NS, local)
}

pub(crate) fn check_pattern(pattern: &str) -> Result<()> {
    if pattern.is_empty() || pattern.contains(['\n', '\r', '\u{b}']) {
        return Err(Error::InvalidPattern);
    }
    Ok(())
}

/// Non-overlapping matches of `pattern` in `text`, left to right.
pub(crate) fn find_in(text: &str, pattern: &str, ascii_ci: bool) -> Vec<Range<usize>> {
    if ascii_ci {
        let (h, n) = (text.to_ascii_lowercase(), pattern.to_ascii_lowercase());
        h.match_indices(&n).map(|(i, m)| i..i + m.len()).collect()
    } else {
        text.match_indices(pattern).map(|(i, m)| i..i + m.len()).collect()
    }
}

// ---------------------------------------------------------------------------
// Paragraph model

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Run,
    Break,
    Field,
}

#[derive(Clone, Debug)]
struct Item<'d> {
    /// Child index in the original `a:p` (`None` for a created item).
    orig: Option<usize>,
    kind: Kind,
    text: String,
    /// The original `a:rPr` (runs), for formats and links.
    rpr: Option<&'d RawNode>,
    /// Format of a created run.
    fmt: Fmt,
    touched: bool,
}

fn item_text(n: &RawNode) -> String {
    n.elements().find(|c| c.is(Text::QNAME)).map(|t| t.text().into_owned()).unwrap_or_default()
}

fn rpr_of(n: &RawNode) -> Option<&RawNode> {
    n.elements().find(|c| c.is(RunProperties::QNAME))
}

fn has_link(rpr: Option<&RawNode>) -> bool {
    rpr.is_some_and(|r| r.elements().any(|c| c.is(a("hlinkClick")) || c.is(a("hlinkMouseOver"))))
}

/// A canonical string of an `a:rPr` for rule 6: attributes sorted (proofing
/// ones dropped at the top level), children in order.
fn canon(n: Option<&RawNode>, top: bool) -> String {
    let Some(n) = n else { return String::new() };
    let mut attrs: Vec<String> = n
        .attrs()
        .iter()
        .filter(|at| !(top && at.name().ns().is_empty() && PROOFING_ATTRIBUTES.contains(&at.name().local())))
        .map(|at| format!("{{{}}}{}={:?}", at.name().ns(), at.name().local(), at.value()))
        .collect();
    attrs.sort();
    let mut s = format!("<{{{}}}{} {}>", n.name().ns(), n.name().local(), attrs.join(" "));
    for c in n.children() {
        match c {
            RawItem::Element(e) => s.push_str(&canon(Some(e), false)),
            RawItem::Text(t) => s.push_str(&format!("{:?}", t.value())),
            RawItem::Markup(_) => {}
        }
    }
    s
}

/// The paragraph's text, as [`crate::ParagraphRef::text`].
pub(crate) fn paragraph_text(p: &RawNode) -> String {
    crate::ParagraphRef { node: p }.text()
}

/// One planned paragraph edit: the accepted matches, the refused ones, and
/// the primitive operations, all decided before any change.
#[derive(Debug, Default)]
pub(crate) struct ParaPlan {
    pub accepted: Vec<Range<usize>>,
    pub refused: Vec<(Range<usize>, Error)>,
    /// `(child index, new a:t text)` for kept runs whose text changes.
    texts: Vec<(usize, String)>,
    /// Child indices removed (descending order when applied).
    removed: Vec<usize>,
    /// `(slot position in the final run slot, item)` created, ascending.
    created: Vec<(usize, Kind, String, Fmt)>,
}

impl ParaPlan {
    pub(crate) fn changes(&self) -> bool {
        !self.texts.is_empty() || !self.removed.is_empty() || !self.created.is_empty()
    }
}

/// Context of the paragraph's text body.
pub(crate) struct BodyCtx<'g> {
    /// `Err` producer when the whole body is not editable (guard, AC, merged cell…).
    pub blocked: Option<&'g dyn Fn() -> Error>,
    pub guard: Option<&'g Guard>,
}

/// Plans `pattern` → `segments` (the replacement split on `\v`) in one
/// paragraph. Never changes anything.
pub(crate) fn plan_paragraph(
    p: &RawNode,
    pattern: &str,
    segments: &[&str],
    opts: &ReplaceOptions,
    ctx: &BodyCtx<'_>,
) -> ParaPlan {
    let text = paragraph_text(p);
    let found = find_in(&text, pattern, opts.find.ascii_case_insensitive);
    let mut plan = ParaPlan::default();
    if found.is_empty() {
        return plan;
    }
    let body_error = || -> Option<Error> {
        if let Some(f) = ctx.blocked {
            return Some(f());
        }
        if let Some(g) = ctx.guard {
            g.text().err()
        } else {
            None
        }
    };
    if body_error().is_some() {
        plan.refused = found.into_iter().map(|r| (r, body_error().expect("checked"))).collect();
        return plan;
    }
    if let Err(e) = para_shape(p) {
        let msg = e.to_string();
        plan.refused = found.into_iter().map(|r| (r, Error::UnsupportedContent(msg.clone()))).collect();
        return plan;
    }
    // The model: the run slot (a:r, a:br, a:fld) in document order.
    let mut items: Vec<Item<'_>> = p
        .indexed_elements()
        .filter_map(|(i, c)| {
            let kind = if c.is(Run::QNAME) {
                Kind::Run
            } else if c.is(Break::QNAME) {
                Kind::Break
            } else if c.is(Field::QNAME) {
                Kind::Field
            } else {
                return None;
            };
            let text = if kind == Kind::Break { "\u{b}".into() } else { item_text(c) };
            let rpr = if kind == Kind::Run { rpr_of(c) } else { None };
            Some(Item { orig: Some(i), kind, text, rpr, fmt: None, touched: false })
        })
        .collect();
    let spans = |items: &[Item<'_>]| {
        let mut off = 0;
        items
            .iter()
            .map(|it| {
                let r = off..off + it.text.len();
                off = r.end;
                r
            })
            .collect::<Vec<_>>()
    };
    let covered = |sp: &[Range<usize>], m: &Range<usize>| -> Vec<usize> {
        sp.iter().enumerate().filter(|(_, r)| r.start < m.end && r.end > m.start).map(|(i, _)| i).collect()
    };
    // Checks per match, on the original model.
    let sp = spans(&items);
    let mut accepted = Vec::new();
    for m in found {
        let cov = covered(&sp, &m);
        let refuse = |e: Error| Some(e);
        let err = if cov.iter().any(|&i| items[i].kind == Kind::Field) {
            refuse(Error::FieldInMatch)
        } else if cov.iter().any(|&i| items[i].kind == Kind::Break) {
            refuse(Error::BreakInMatch)
        } else if opts.mixed_format == MixedFormat::Refuse
            && cov.windows(2).any(|w| canon(items[w[0]].rpr, true) != canon(items[w[1]].rpr, true))
        {
            refuse(Error::MixedFormatInMatch)
        } else if segments.len() > 1 {
            let start = &items[cov[0]];
            let ends_inside = m.end < sp[cov[0]].end;
            if has_link(start.rpr) && ends_inside {
                refuse(Error::LinkSplitByBreak)
            } else {
                // No depth check: a created `a:r`/`a:rPr`/`a:t` sits at the
                // depth of the start run's own `a:t`, which the parser
                // already accepted (ADR-0002 note "parser depth limit").
                copy_attrs::<RunProperties>(start.rpr, "a:rPr").err()
            }
        } else {
            None
        };
        match err {
            Some(e) => plan.refused.push((m, e)),
            None => accepted.push(m),
        }
    }
    // Apply accepted matches to the model, right to left.
    for m in accepted.iter().rev() {
        let sp = spans(&items);
        let cov = covered(&sp, m);
        let (first, last) = (cov[0], *cov.last().expect("non-empty match"));
        let a0 = sp[first].start;
        let al = sp[last].start;
        let tail = items[last].text[m.end - al..].to_owned();
        let suffix = if first == last { tail.clone() } else { String::new() };
        let prefix = items[first].text[..m.start - a0].to_owned();
        items[first].text = if segments.len() == 1 { format!("{prefix}{}{suffix}", segments[0]) } else { format!("{prefix}{}", segments[0]) };
        items[first].touched = true;
        let mut remove = Vec::new();
        if first != last {
            remove.extend(cov[1..cov.len() - 1].iter().copied());
            if tail.is_empty() {
                remove.push(last);
            } else {
                items[last].text = tail;
                items[last].touched = true;
            }
        }
        let mut added = Vec::new();
        if segments.len() > 1 {
            let fmt = copy_attrs::<RunProperties>(items[first].rpr, "a:rPr").expect("checked in planning");
            for (k, seg) in segments.iter().enumerate().skip(1) {
                added.push(Item { orig: None, kind: Kind::Break, text: "\u{b}".into(), rpr: None, fmt: None, touched: false });
                let t = if k == segments.len() - 1 { format!("{seg}{suffix}") } else { (*seg).to_owned() };
                if !t.is_empty() {
                    added.push(Item { orig: None, kind: Kind::Run, text: t, rpr: items[first].rpr, fmt: fmt.clone(), touched: false });
                }
            }
        }
        for &i in remove.iter().rev() {
            items.remove(i);
        }
        let at = first + 1;
        items.splice(at..at, added);
    }
    // Rule 4: touched runs left empty are removed unless no run would remain.
    let empty_touched = |it: &Item<'_>| it.kind == Kind::Run && it.touched && it.text.is_empty();
    let runs_left = items.iter().filter(|it| it.kind == Kind::Run && !empty_touched(it)).count();
    if runs_left > 0 {
        items.retain(|it| !empty_touched(it));
    } else if let Some(keep) = items.iter().position(empty_touched) {
        let mut k = 0;
        items.retain(|it| {
            let r = !empty_touched(it) || k == keep;
            k += 1;
            r
        });
    }
    // Derive the primitive operations.
    let kept: Vec<usize> = items.iter().filter_map(|it| it.orig).collect();
    for (i, c) in p.indexed_elements() {
        if (c.is(Run::QNAME) || c.is(Break::QNAME) || c.is(Field::QNAME)) && !kept.contains(&i) {
            plan.removed.push(i);
        }
    }
    for (pos, it) in items.iter().enumerate() {
        match it.orig {
            Some(i) => {
                if it.kind == Kind::Run {
                    let n = node_at(p, &[i]).expect("child of the paragraph");
                    if item_text(n) != it.text || n.elements().all(|c| !c.is(Text::QNAME)) {
                        plan.texts.push((i, it.text.clone()));
                    }
                }
            }
            None => plan.created.push((pos, it.kind, it.text.clone(), it.fmt.clone())),
        }
    }
    plan.accepted = accepted;
    plan
}

/// Applies a plan to the paragraph at `para` (a node path from the slide root).
pub(crate) fn apply_plan(slide: &mut SlideMut<'_>, body: &[usize], para_child: usize, plan: &ParaPlan) -> Result<()> {
    if !plan.changes() {
        return Ok(());
    }
    fn lens<'s>(slide: &'s mut SlideMut<'_>, body: &[usize], para_child: usize) -> Result<ooxml_core::LensMut<'s, Paragraph>> {
        crate::write::paragraph_lens(slide.doc, body, para_child)
    }
    for (child, t) in &plan.texts {
        let p = lens(slide, body, para_child)?;
        let mut r = p.into_child_at::<Run>(*child).ok_or(Error::RunNotFound(*child))?;
        r.get_or_insert::<Text>()?.set_text(t)?;
    }
    let mut removed = plan.removed.clone();
    removed.sort_unstable();
    for &child in removed.iter().rev() {
        lens(slide, body, para_child)?.remove_at(child);
    }
    for (pos, kind, t, fmt) in &plan.created {
        let mut p = lens(slide, body, para_child)?;
        match kind {
            Kind::Break => {
                p.insert_by::<Break>(|k, _| k == *pos)?;
            }
            Kind::Run => {
                let mut r = p.insert_by::<Run>(|k, _| k == *pos)?;
                if let Some(f) = fmt {
                    set_attrs(&mut r.insert::<RunProperties>()?, f)?;
                }
                r.insert::<Text>()?.set_text(t)?;
            }
            Kind::Field => unreachable!("fields are never created"),
        }
    }
    Ok(())
}

/// A text body found on a slide.
pub(crate) struct Target {
    pub shape: ShapePath,
    pub cell: Option<(usize, usize)>,
    /// Path of the text body element from the slide root.
    pub body: Vec<usize>,
}

/// Every text body of a slide, in shape order: `p:sp` bodies (groups
/// included), table cells, and the representative shape of an
/// `mc:AlternateContent`.
pub(crate) fn targets(doc: &ooxml_core::XmlDocument) -> Vec<Target> {
    let mut out = Vec::new();
    for s in read::shapes_of(doc) {
        let base = s.path().as_slice().to_vec();
        match s.kind() {
            ShapeKind::Shape => {
                if let Some((i, _)) = s.raw().indexed_elements().find(|(_, c)| c.is(ooxml_schema::pml::TextBody::QNAME)) {
                    out.push(Target { shape: s.path().clone(), cell: None, body: [base, vec![i]].concat() });
                }
            }
            ShapeKind::Table => {
                let frame = s.raw();
                let mut path = base.clone();
                let mut n = frame;
                for q in [ooxml_schema::dml::Graphic::QNAME, ooxml_schema::dml::GraphicData::QNAME, ooxml_schema::dml::Table::QNAME] {
                    let Some((i, c)) = n.indexed_elements().find(|(_, c)| c.is(q)) else { break };
                    path.push(i);
                    n = c;
                }
                for (r, (ri, tr)) in n.indexed_elements().filter(|(_, c)| c.is(ooxml_schema::dml::TableRow::QNAME)).enumerate() {
                    for (c, (ci, tc)) in tr.indexed_elements().filter(|(_, c)| c.is(ooxml_schema::dml::TableCell::QNAME)).enumerate() {
                        if let Some((bi, _)) = tc.indexed_elements().find(|(_, x)| x.is(ooxml_schema::dml::TextBody::QNAME)) {
                            out.push(Target {
                                shape: s.path().clone(),
                                cell: Some((r, c)),
                                body: [path.clone(), vec![ri, ci, bi]].concat(),
                            });
                        }
                    }
                }
            }
            ShapeKind::AlternateContent => {
                let ac = s.raw();
                let Some((bi, branch)) = ac
                    .indexed_elements()
                    .find(|(_, b)| b.name().local() == "Fallback")
                    .or_else(|| ac.indexed_elements().find(|(_, b)| b.name().local() == "Choice"))
                else {
                    continue;
                };
                let Some((si, sp)) = branch.indexed_elements().next() else { continue };
                if !sp.is(ooxml_schema::pml::Shape::QNAME) {
                    continue;
                }
                if let Some((ti, _)) = sp.indexed_elements().find(|(_, c)| c.is(ooxml_schema::pml::TextBody::QNAME)) {
                    out.push(Target { shape: s.path().clone(), cell: None, body: [base, vec![bi, si, ti]].concat() });
                }
            }
            _ => {}
        }
    }
    out
}

/// Paragraph child indices of a body element.
pub(crate) fn paragraphs(body: &RawNode) -> Vec<usize> {
    body.indexed_elements().filter(|(_, c)| c.is(Paragraph::QNAME)).map(|(i, _)| i).collect()
}

/// The replacement's `\v` segments, with Phase 0's character rules.
pub(crate) fn segments(replacement: &str) -> Result<Vec<&str>> {
    text::paragraph_segments(replacement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching() {
        assert_eq!(find_in("aXbxXc", "x", false), vec![3..4]);
        assert_eq!(find_in("aXbxXc", "x", true), [1..2, 3..4, 4..5]);
        assert_eq!(find_in("aaaa", "aa", false), [0..2, 2..4]);
        assert!(check_pattern("").is_err() && check_pattern("a\u{b}").is_err() && check_pattern("a\n").is_err());
        assert!(check_pattern("a b").is_ok());
    }
}
