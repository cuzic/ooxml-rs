//! Prototype of ADR-0010's lazy `sheetData`: a row-span index built by a
//! quick-xml scan (no tree), rows materialized on demand from their source
//! slice, and a save that splices re-rendered rows into the original bytes.

use std::time::Instant;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::compact::{self, Item, Node, SStr};

/// One `row` element of `sheetData`.
pub struct RowSpan {
    pub r: u32,
    pub start: u32,
    pub end: u32,
}

pub struct LazySheet<'s> {
    pub src: &'s str,
    /// Namespace declarations of the root start tag, verbatim, to wrap a
    /// row slice so that its prefixes resolve.
    pub decls: String,
    pub rows: Vec<RowSpan>,
}

impl<'s> LazySheet<'s> {
    pub fn index(src: &'s str) -> Result<Self, String> {
        let mut reader = Reader::from_str(src);
        reader.config_mut().trim_text(false);
        let mut depth = 0usize;
        let mut sheet_data_depth = None;
        let mut decls = String::new();
        let mut rows = Vec::new();
        let mut open_row: Option<(u32, u32)> = None;
        loop {
            let before = reader.buffer_position() as u32;
            let ev = reader.read_event().map_err(|e| e.to_string())?;
            let after = reader.buffer_position() as u32;
            match ev {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) if depth == 0 && decls.is_empty() => {
                    for a in e.attributes().flatten() {
                        let k = a.key.into_inner();
                        if k == "xmlns" || k.starts_with("xmlns:") {
                            decls.push_str(&format!(" {k}=\"{}\"", &*a.value));
                        }
                    }
                    depth += 1;
                }
                Event::Start(e) => {
                    depth += 1;
                    let local = e.local_name();
                    if local.as_ref() == "sheetData" {
                        sheet_data_depth = Some(depth);
                    } else if local.as_ref() == "row" && sheet_data_depth == Some(depth - 1) {
                        open_row = Some((row_number(&e)?, before));
                    }
                }
                Event::Empty(e) => {
                    if e.local_name().as_ref() == "row" && sheet_data_depth == Some(depth) {
                        rows.push(RowSpan { r: row_number(&e)?, start: before, end: after });
                    }
                }
                Event::End(_) => {
                    if sheet_data_depth == Some(depth - 1)
                        && let Some((r, start)) = open_row.take()
                    {
                        rows.push(RowSpan { r, start, end: after });
                    }
                    if sheet_data_depth == Some(depth) {
                        sheet_data_depth = None;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        Ok(LazySheet { src, decls, rows })
    }

    /// Binary search (rows are in ascending order in a valid sheet; the
    /// real design validates that while indexing).
    pub fn find(&self, r: u32) -> Option<usize> {
        self.rows.binary_search_by_key(&r, |x| x.r).ok()
    }

    /// Parses one row from its slice, wrapped with the root's namespace
    /// declarations. Spans are relative to the returned fragment.
    pub fn materialize(&self, i: usize) -> Result<(String, Node), String> {
        let s = &self.rows[i];
        let frag = format!("<w{}>{}</w>", self.decls, &self.src[s.start as usize..s.end as usize]);
        let doc = compact::parse(frag.as_bytes(), 256)?;
        let row = doc.root.children.into_iter().find_map(|c| match c {
            Item::Element(n) => Some(n),
            _ => None,
        });
        Ok((frag, row.ok_or("empty row")?))
    }
}

fn row_number(e: &quick_xml::events::BytesStart<'_>) -> Result<u32, String> {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == "r" {
            return a.value.parse().ok().ok_or("bad row r".into());
        }
    }
    Err("row without r".into())
}

/// Sets `<v>` of the cell in column `col` (0-based) of a materialized row,
/// marking the dirty path as ADR-0001 requires.
pub fn set_value(row: &mut Node, col: usize, value: &str) -> bool {
    let Some(Item::Element(c)) = row.children.iter_mut().filter(|c| matches!(c, Item::Element(_))).nth(col) else { return false };
    let Some(Item::Element(v)) = c.children.iter_mut().find(|i| matches!(i, Item::Element(n) if n.name.local() == "v")) else { return false };
    v.children.clear();
    v.children.push(Item::Text(compact::Text { value: SStr::new_owned(value), span: (u32::MAX, u32::MAX), cdata: false }));
    v.subtree_dirty = true;
    c.subtree_dirty = true;
    row.subtree_dirty = true;
    true
}

/// Output = source with each edited row's span replaced by its re-rendered
/// fragment (untouched rows and everything outside them are copied).
pub fn splice(src: &str, edited: &mut [(usize, u32, u32, String)]) -> String {
    edited.sort_by_key(|e| e.1);
    let mut out = String::with_capacity(src.len() + 64);
    let mut at = 0usize;
    for (_, start, end, text) in edited.iter() {
        out.push_str(&src[at..*start as usize]);
        out.push_str(text);
        at = *end as usize;
    }
    out.push_str(&src[at..]);
    out
}

/// Timing helper.
pub fn time<T>(f: impl FnOnce() -> T) -> (T, std::time::Duration) {
    let t = Instant::now();
    let v = f();
    (v, t.elapsed())
}
