//! Structured-reference specifiers and grammar (ADR-0017 §6.3).

use std::ops::Range;

use super::precedents::fold;

pub(crate) fn specifier(name: &str, nested: bool) -> String {
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
pub(crate) struct Items {
    pub(crate) selectors: Vec<&'static str>,
    pub(crate) columns: Vec<String>,
    pub(crate) column_spans: Vec<Range<usize>>,
    pub(crate) table_span: Range<usize>,
}

impl Items {
    fn item(&mut self, text: &str, item: &str) -> Option<()> {
        match fold(item).as_str() {
            "#this row" | "@" => self.selectors.push("#This Row"),
            "#all" => self.selectors.push("#All"),
            "#data" => self.selectors.push("#Data"),
            "#headers" => self.selectors.push("#Headers"),
            "#totals" => self.selectors.push("#Totals"),
            _ if item.starts_with(['#', '@']) || item.is_empty() => return None,
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
                self.column_spans.push(text.substr_range(item)?.into());
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

pub(crate) fn parse(text: &str) -> Option<Items> {
    let start = text.find('[')?;
    let mut rest = text[start..].strip_circumfix('[', ']')?;
    let mut result = Items {
        table_span: 0..start,
        ..Items::default()
    };
    if let Some(row) = rest.strip_prefix('@') {
        result.selectors.push("#This Row");
        rest = row;
        if rest.is_empty() {
            return Some(result);
        }
    }
    if !rest.trim_start().starts_with('[') {
        let selectors = result.selectors.len();
        result.item(text, rest)?;
        if selectors != 0 && result.columns.is_empty() {
            return None;
        }
        return Some(result);
    }
    rest = rest.trim_start();
    loop {
        let (item, tail) = take_item(rest)?;
        if !result.columns.is_empty() {
            return None;
        }
        result.item(text, item)?;
        let mut tail = tail.trim_start();
        if let Some(range) = tail.strip_prefix(':') {
            let (end, remaining) = take_item(range.trim_start())?;
            result.item(text, end)?;
            if result.columns.len() != 2 {
                return None;
            }
            tail = remaining.trim_start();
        }
        if tail.is_empty() {
            return match result.selectors.as_slice() {
                [] | [_] | ["#Headers", "#Data"] | ["#Data", "#Totals"] => Some(result),
                _ => None,
            };
        }
        rest = tail.strip_prefix(',')?.trim_start();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_spans_cover_the_original_spelling() {
        let text = "État[[#Headers],[First]:[Last']]]";
        let items = parse(text).unwrap();
        assert_eq!(&text[items.table_span], "État");
        let slices: Vec<_> = items
            .column_spans
            .into_iter()
            .map(|span| &text[span])
            .collect();
        assert_eq!(slices, ["First", "Last']"]);
        assert_eq!(items.columns, ["First", "Last]"]);
    }
}
