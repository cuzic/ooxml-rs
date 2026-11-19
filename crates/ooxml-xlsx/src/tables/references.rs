//! Structured-reference specifiers and reference checks for table edits.

use super::{attr, descendants};
use crate::formula::lexer::{Prefix, TokKind, lex};
use crate::formula::precedents::fold;
use crate::workbook::TableHeader;
use crate::{CellIndex, Error, Result, Workbook};
use ooxml_core::{RawNode, XmlDocument};

pub(super) fn specifier(name: &str, nested: bool) -> String {
    let double = !nested
        && (name.trim() != name
            || name.contains([
                '\t', '\n', '\r', ',', ':', '.', '[', ']', '#', '\'', '"', '{', '}', '$', '^', '&',
                '*', '+', '=', '-', '>', '<', '/', '@',
            ]));
    let mut result = String::from(if double { "[[" } else { "[" });
    for c in name.chars() {
        if matches!(c, '\'' | '#' | '[' | ']' | '@') {
            result.push('\'');
        }
        result.push(c);
    }
    result.push_str(if double { "]]" } else { "]" });
    result
}

#[derive(Default)]
struct Items {
    columns: Vec<String>,
    row: bool,
}

impl Items {
    fn item(&mut self, item: &str) -> Option<()> {
        match fold(item).as_str() {
            "#this row" | "@" => self.row = true,
            "#all" | "#data" | "#headers" | "#totals" => {}
            _ if item.starts_with('#') || item.is_empty() => return None,
            _ => {
                let mut decoded = String::new();
                let mut chars = item.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '\'' => decoded.push(
                            chars
                                .next()
                                .filter(|c| matches!(c, '\'' | '#' | '[' | ']' | '@'))?,
                        ),
                        '[' | ']' => return None,
                        _ => decoded.push(c),
                    }
                }
                self.columns.push(decoded);
            }
        }
        Some(())
    }
}

/// Takes a bracketed item, skipping closing brackets that are escaped.
fn take_item(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('[')?;
    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\'' => {
                chars.next()?;
            }
            ']' => return Some((&text[..i], &text[i + 1..])),
            '[' => return None,
            _ => {}
        }
    }
    None
}

fn parse(spec: &str) -> Option<Items> {
    let mut rest = spec.strip_circumfix('[', ']')?;
    let mut result = Items::default();
    if let Some(row) = rest.strip_prefix('@') {
        result.row = true;
        rest = row;
        if rest.is_empty() {
            return Some(result);
        }
    }
    if !rest.starts_with('[') {
        result.item(rest)?;
        return Some(result);
    }
    loop {
        let (item, tail) = take_item(rest)?;
        let start = result.columns.len();
        result.item(item)?;
        let mut tail = tail.trim_start();
        if let Some(range) = tail.strip_prefix(':') {
            let (end, remaining) = take_item(range.trim_start())?;
            result.item(end)?;
            if result.columns.len() != start + 2 {
                return None;
            }
            tail = remaining.trim_start();
        }
        if tail.is_empty() {
            return Some(result);
        }
        rest = tail.strip_prefix(',')?.trim_start();
    }
}

pub(super) fn calculated(text: &str, name: &str) -> Result<String> {
    let text = text.strip_prefix('=').unwrap_or(text);
    let invalid = |reason: &str| Error::InvalidFormula {
        text: text.to_owned(),
        reason: reason.to_owned(),
    };
    let tokens = lex(text).map_err(|e| invalid(e.message))?;
    let mut output = String::new();
    let mut pos = 0;
    for token in tokens.iter().filter(|t| matches!(t.kind, TokKind::Opaque)) {
        let raw = &text[token.start..token.end];
        if !raw.starts_with("[@") {
            continue;
        }
        let items = parse(raw).ok_or_else(|| invalid("invalid structured reference"))?;
        let [col] = items.columns.as_slice() else {
            return Err(invalid("expected one column in local row reference"));
        };
        output.push_str(&text[pos..token.start]);
        output.push_str(&format!("{name}[[#This Row],{}]", specifier(col, true)));
        pos = token.end;
    }
    output.push_str(&text[pos..]);
    Ok(output)
}

#[derive(Clone, Copy)]
pub(super) enum Use<'a> {
    Source,
    Column(&'a str),
    Table,
}

fn has_reference(text: &str, own: bool, names: &[String], usage: Use<'_>) -> bool {
    if !text.contains('[') && !matches!(usage, Use::Table) {
        return false;
    }
    let Ok(tokens) = lex(text.strip_prefix('=').unwrap_or(text)) else {
        return matches!(usage, Use::Column(_))
            || names.iter().any(|n| fold(text).contains(&format!("{n}[")))
            || own;
    };
    let text = text.strip_prefix('=').unwrap_or(text);
    tokens.iter().any(|t| {
        if matches!(usage, Use::Table)
            && let TokKind::Name {
                prefix: Prefix::None,
                name,
            } = &t.kind
        {
            return names.contains(&fold(name));
        }
        if !matches!(t.kind, TokKind::Opaque) {
            return false;
        }
        let raw = &text[t.start..t.end];
        let Some(i) = raw.find('[') else { return false };
        let table = &raw[..i];
        let relevant = if table.is_empty() {
            own
        } else {
            names.contains(&fold(table))
        };
        match usage {
            Use::Table => relevant,
            Use::Column(col) => match parse(&raw[i..]) {
                None => true,
                Some(items) => relevant && items.columns.iter().any(|c| fold(c) == fold(col)),
            },
            Use::Source => false,
        }
    })
}

/// Formula-bearing elements from ADR-0009 §6, including extension
/// formulas, plus the reference-bearing attributes in chart/pivot parts.
fn formula_node(node: &RawNode) -> bool {
    matches!(
        node.name().local(),
        "f" | "formula"
            | "formula1"
            | "formula2"
            | "definedName"
            | "calculatedColumnFormula"
            | "totalsRowFormula"
            | "fmla"
            | "sqref"
            | "fullRef"
            | "formulaRef"
            | "datalabelsRange"
            | "filteredSeriesTitle"
            | "filteredCategoryTitle"
            | "FmlaLink"
            | "FmlaRange"
            | "FmlaTxbx"
            | "FmlaPict"
    )
}

impl Workbook {
    pub(super) fn table_referenced(
        &self,
        table: &TableHeader,
        names: &[String],
        id: u32,
        usage: Use<'_>,
    ) -> Result<bool> {
        let worksheet = self
            .contexts
            .iter()
            .find(|(_, context)| context.tables.iter().any(|t| t.part == table.part))
            .and_then(|(sheet, _)| self.sheets[*sheet].part.as_deref());
        for part in self.pkg.part_names() {
            let ct = self
                .pkg
                .content_type(part)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !(ct.ends_with("xml") || ct.ends_with("vmldrawing") || part.ends_with(".rels"))
                || ct.contains("sharedstrings")
                || ct.contains("comments")
                || part.starts_with("docProps/")
            {
                continue;
            }
            if matches!(usage, Use::Source)
                && !(ct.ends_with("pivotcachedefinition+xml") || ct.ends_with("slicercache+xml"))
            {
                continue;
            }
            let parsed;
            let doc = if let Some(doc) = self.pkg.parsed_part(part) {
                doc
            } else {
                let bytes = self.pkg.read_part(part)?;
                let Ok(doc) = XmlDocument::parse_with(&bytes, &self.pkg.parse_options()) else {
                    // A damaged formula-bearing part cannot reliably be checked.
                    // Inspect its bytes for explicit table mentions as a fallback.
                    if names
                        .iter()
                        .any(|n| fold(&String::from_utf8_lossy(&bytes)).contains(&format!("{n}[")))
                        || ct.ends_with("pivotcachedefinition+xml")
                        || ct.ends_with("slicercache+xml")
                    {
                        return Ok(true);
                    }
                    continue;
                };
                parsed = doc;
                &parsed
            };
            for node in descendants(doc.root()) {
                match node.name().local() {
                    "worksheetSource"
                        if node
                            .attr(attr("name"))
                            .is_some_and(|n| names.contains(&fold(n))) =>
                    {
                        return Ok(true);
                    }
                    "tableSlicerCache"
                        if node.attr(attr("tableId")).and_then(|s| s.parse().ok()) == Some(id) =>
                    {
                        return Ok(true);
                    }
                    _ => {}
                }
                if matches!(usage, Use::Source) {
                    continue;
                }
                let own_part = part == table.part;
                if part == worksheet.unwrap_or_default() && node.name().local() == "c" {
                    let own = node
                        .attr(attr("r"))
                        .and_then(|r| CellIndex::parse(r).ok())
                        .is_some_and(|at| table.range.contains(at));
                    if own
                        && node
                            .elements()
                            .filter(|n| formula_node(n))
                            .any(|f| has_reference(&f.text(), true, names, usage))
                    {
                        return Ok(true);
                    }
                }
                if formula_node(node) && has_reference(&node.text(), own_part, names, usage) {
                    return Ok(true);
                }
                if node.name().local() == "cfvo"
                    && node.attr(attr("type")) == Some("formula")
                    && node
                        .attr(attr("val"))
                        .is_some_and(|val| has_reference(val, false, names, usage))
                {
                    return Ok(true);
                }
                for a in node.attrs().iter().filter(|a| {
                    matches!(
                        a.name().local(),
                        "ref"
                            | "formula"
                            | "formula1"
                            | "formula2"
                            | "location"
                            | "range"
                            | "cell"
                            | "fmlaLink"
                            | "fmlaRange"
                            | "fmlaTxbx"
                            | "fmlaGroup"
                            | "linkedCell"
                            | "listFillRange"
                            | "textlink"
                            | "Target"
                    )
                }) {
                    let value = if a.name().local() == "Target" {
                        if node.attr(attr("TargetMode")) == Some("External") {
                            continue;
                        }
                        let Some(fragment) = a.value().strip_prefix('#') else {
                            continue;
                        };
                        fragment
                    } else {
                        a.value()
                    };
                    if has_reference(value, false, names, usage) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
}
