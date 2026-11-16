//! `ooxml-pptx`: open → read every slide → text edits → save → reopen.

use std::collections::BTreeSet;

use ooxml_pptx::{Presentation, Result, ShapePath, Slide, TextBodyRef, TextItem};

use crate::fixtures::{PPTX, decode};
use crate::input::Input;
use crate::zipcmp::assert_l1_except;

const MAX_SLIDES: usize = 8;

fn read_body(b: &TextBodyRef<'_>) {
    let _ = b.text();
    for p in b.paragraphs() {
        for it in p.items() {
            match it {
                TextItem::Run(r) => {
                    let _ = r.text();
                }
                TextItem::Field(f) => {
                    let _ = (f.field_type(), f.text());
                }
                TextItem::Break | TextItem::Other(_) => {}
            }
        }
    }
}

/// Reads everything on a slide; returns the shapes with a text body and
/// the table shapes with their size.
fn read_slide(s: &Slide<'_>) -> (Vec<ShapePath>, Vec<(ShapePath, usize, usize)>) {
    let mut bodies = Vec::new();
    let mut tables = Vec::new();
    let _ = s.info();
    let _ = s.placeholders().len();
    for sh in s.shapes() {
        let _ = (sh.id(), sh.name(), sh.kind(), sh.depth(), sh.raw().name());
        if let Some(ph) = sh.placeholder() {
            let _ = (ph.kind(), ph.idx());
        }
        if let Ok(id) = sh.id() {
            let _ = s.shape_by_id(id);
        }
        if let Some(b) = sh.text_body() {
            read_body(&b);
            bodies.push(sh.path().clone());
        }
        if let Some(t) = sh.table() {
            let rows = t.rows();
            for row in &rows {
                for c in row {
                    let _ = (c.is_merged(), c.grid_span(), c.row_span());
                    if let Some(b) = c.text_body() {
                        read_body(&b);
                    }
                }
            }
            let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
            tables.push((sh.path().clone(), rows.len(), cols));
        }
    }
    (bodies, tables)
}

/// `TextBodyMut::set_text`'s documented reading: `\r\n` and `\r` become
/// paragraph breaks.
fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Fuzz entry: a fixture-based input (see [`crate::fixtures`]).
pub fn check_pptx_input(data: &[u8]) {
    if let Some((zip, ops)) = decode(&PPTX, data) {
        check_pptx(&zip, ops);
    }
}

/// Checks on a presentation (if it opens):
///
/// - zero-edit save is Layer-1 identical to the input; reads never panic;
/// - every refused edit leaves the package byte-identical;
/// - a successful `TextBodyMut::set_text` reads back as written;
/// - after the edits, only edited slide parts differ (Layer 1), and the
///   output reopens with the same slide count.
pub fn check_pptx(zip: &[u8], mut ops: Input<'_>) {
    let Ok(mut pres) = Presentation::from_bytes(zip.to_vec()) else { return };
    let Ok(base) = pres.to_bytes() else { return };
    assert_l1_except(zip, &base, &[], "pptx zero-edit save");
    let count = pres.slide_count();
    let _ = pres.slides().count();
    let mut targets = Vec::new();
    for i in 0..count.min(MAX_SLIDES) {
        if let Ok(s) = pres.slide(i) {
            let (b, t) = read_slide(&s);
            targets.push((i, s.info().part_name.clone(), b, t));
        }
    }
    assert_l1_except(zip, &pres.to_bytes().expect("saved before"), &[], "pptx save after reading every slide");
    if targets.is_empty() {
        return;
    }
    let mut edited: BTreeSet<String> = BTreeSet::new();
    let mut steps = 0;
    while !ops.is_empty() && steps < 16 {
        steps += 1;
        let (si, part, bodies, tables) = &targets[usize::from(ops.u8()) % targets.len()];
        let kind = ops.u8() % 10;
        let text = ops.string();
        let (a, b) = (usize::from(ops.u8() % 4), usize::from(ops.u8() % 4));
        let before = pres.to_bytes().expect("saved before");
        let mut check_body: Option<ShapePath> = None;
        let result: Result<()> = (|| {
            let mut slide = pres.slide_mut(*si)?;
            if kind == 9 {
                let Some((path, rows, cols)) = tables.get(a % tables.len().max(1)) else { return Ok(()) };
                let mut sh = slide.shape_mut(path)?;
                return sh.table_cell_mut(b % rows.max(&1), usize::from(text.len() as u8) % cols.max(&1))?.set_text(&text);
            }
            let Some(path) = bodies.get(a % bodies.len().max(1)) else { return Ok(()) };
            let mut sh = slide.shape_mut(path)?;
            let mut body = sh.text_body_mut()?;
            match kind {
                0 => {
                    body.set_text(&text)?;
                    check_body = Some(path.clone());
                    Ok(())
                }
                1 => body.paragraph_mut(b)?.set_text(&text),
                2 => body.paragraph_mut(b)?.run_mut(0)?.set_text(&text),
                3 => body.add_paragraph(&text).map(drop),
                4 => body.paragraph_mut(b)?.add_run(&text),
                5 => body.paragraph_mut(b)?.add_break(),
                6 => body.paragraph_mut(b)?.remove_run(0),
                7 => body.remove_paragraph(b),
                _ => body.paragraph_mut(b)?.remove_field(0),
            }
        })();
        match result {
            Err(e) => {
                let after = pres.to_bytes().expect("to_bytes after a refused edit");
                assert!(after == before, "refused pptx edit kind {kind} ({e}) changed the package");
            }
            Ok(()) => {
                edited.insert(part.clone());
                if let Some(path) = check_body {
                    let s = pres.slide(*si).expect("slide read after edit");
                    let sh = s.shapes().into_iter().find(|x| x.path() == &path).expect("edited shape still there");
                    let got = sh.text_body().expect("text body").text();
                    assert_eq!(got, normalize(&text), "TextBodyMut::set_text does not read back");
                }
            }
        }
    }
    let out = pres.to_bytes().unwrap_or_else(|e| panic!("save after edits failed: {e}"));
    let except: Vec<&str> = edited.iter().map(String::as_str).collect();
    assert_l1_except(&base, &out, &except, "pptx save after edits");
    let mut re = Presentation::from_bytes(out).unwrap_or_else(|e| panic!("edited presentation does not reopen: {e}"));
    assert_eq!(re.slide_count(), count);
    for (si, ..) in &targets {
        let s = re.slide(*si).unwrap_or_else(|e| panic!("edited slide {si} does not read: {e}"));
        let _ = read_slide(&s);
    }
}
