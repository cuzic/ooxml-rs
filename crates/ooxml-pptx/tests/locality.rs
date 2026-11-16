//! T2 (locality, ADR-0005 §7) on every text body of every fixture: each
//! edit's byte diff on the slide part must stay inside the span the edit
//! targets (a run's `a:t`, a paragraph, a body), or be exactly an insertion
//! at the slot boundary / exactly the removed element's span. Setting a
//! run to its current text changes no byte at all.

#[path = "oracle/cases.rs"]
mod cases;

use std::collections::BTreeMap;

use ooxml_core::{ElementSpan, RawNode};
use ooxml_pptx::{Error, Presentation, ShapeKind, TextBodyMut, TextBodyRef};

struct Para {
    span: ElementSpan,
    /// (run span, its a:t span, text)
    runs: Vec<(ElementSpan, Option<ElementSpan>, String)>,
    /// where `add_run` inserts: `a:endParaRPr` start or the close tag start
    run_insert_at: Option<usize>,
}

struct Body {
    slide: usize,
    id: u32,
    cell: Option<(usize, usize)>,
    part: String,
    span: ElementSpan,
    paras: Vec<Para>,
}

fn span(n: &RawNode) -> ElementSpan {
    n.span().expect("parsed node")
}

fn collect(b: TextBodyRef<'_>) -> (ElementSpan, Vec<Para>) {
    let paras = b
        .paragraphs()
        .iter()
        .map(|p| {
            let runs = p
                .runs()
                .iter()
                .map(|r| {
                    let t = r.raw().elements().find(|c| c.name().local() == "t").map(span);
                    (span(r.raw()), t, r.text().into_owned())
                })
                .collect();
            let s = span(p.raw());
            let run_insert_at = match p.raw().elements().find(|c| c.name().local() == "endParaRPr") {
                Some(e) => Some(span(e).start),
                None => s.close.map(|(c, _)| c),
            };
            Para { span: s, runs, run_insert_at }
        })
        .collect();
    (span(b.raw()), paras)
}

fn bodies(f: &str) -> Vec<Body> {
    let mut pres = Presentation::from_bytes(cases::read_fixture(f)).unwrap();
    let mut out = Vec::new();
    for si in 0..pres.slide_count() {
        let slide = pres.slide(si).unwrap();
        let part = slide.info().part_name.clone();
        for s in slide.shapes() {
            let Ok(id) = s.id() else { continue };
            if slide.shape_by_id(id).is_err() {
                continue;
            }
            match s.kind() {
                ShapeKind::Shape => {
                    if let Some(b) = s.text_body() {
                        let (span, paras) = collect(b);
                        out.push(Body { slide: si, id, cell: None, part: part.clone(), span, paras });
                    }
                }
                ShapeKind::Table => {
                    for (r, row) in s.table().unwrap().rows().iter().enumerate() {
                        for (c, cell) in row.iter().enumerate() {
                            if let Some(b) = cell.text_body() {
                                let (span, paras) = collect(b);
                                out.push(Body { slide: si, id, cell: Some((r, c)), part: part.clone(), span, paras });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

#[derive(Default)]
struct Stats {
    checked: BTreeMap<&'static str, usize>,
    refused: BTreeMap<String, usize>,
}

/// Runs `edit` on a fresh copy; returns (source, output) of the slide part,
/// or `None` if the edit was refused (then the part must be unchanged).
fn run(f: &str, b: &Body, st: &mut Stats, what: &'static str, edit: impl FnOnce(&mut TextBodyMut<'_>) -> Result<(), Error>) -> Option<(String, String)> {
    let input = cases::read_fixture(f);
    let mut pres = Presentation::from_bytes(input.clone()).unwrap();
    let src = String::from_utf8(pres.package().read_part(&b.part).unwrap()).unwrap();
    let res = {
        let mut slide = pres.slide_mut(b.slide).unwrap();
        let mut shape = slide.shape_mut_by_id(b.id).unwrap();
        match b.cell {
            None => shape.text_body_mut().and_then(|mut body| edit(&mut body)),
            Some((r, c)) => shape.table_cell_mut(r, c).and_then(|mut body| edit(&mut body)),
        }
    };
    let out = pres.slide(b.slide).unwrap().document().serialize();
    match res {
        Ok(()) => {
            *st.checked.entry(what).or_default() += 1;
            Some((src, out))
        }
        Err(e) => {
            assert_eq!(out, src, "{f}: a refused {what} ({e}) changed the slide");
            *st.refused.entry(format!("{what}: {}", format!("{e:?}").split([' ', '(', '{']).next().unwrap())).or_default() += 1;
            None
        }
    }
}

/// The changed region of `src` (common prefix/suffix removed).
fn changed(src: &str, out: &str) -> (usize, usize) {
    let pre = src.bytes().zip(out.bytes()).take_while(|(a, b)| a == b).count();
    let max_suf = src.len().min(out.len()) - pre;
    let suf = src.bytes().rev().zip(out.bytes().rev()).take(max_suf).take_while(|(a, b)| a == b).count();
    (pre, src.len() - suf)
}

fn within(src: &str, out: &str, s: ElementSpan, ctx: &str) {
    let (a, b) = changed(src, out);
    assert!(a >= s.start && b <= s.end(), "{ctx}: diff {a}..{b} outside {}..{}", s.start, s.end());
}

fn pure_insertion(src: &str, out: &str, lo: usize, hi: usize, ctx: &str) {
    let (a, b) = changed(src, out);
    assert!(out.len() > src.len(), "{ctx}: nothing inserted");
    // The insertion point is ambiguous by the common-prefix rule only when
    // the inserted bytes repeat their surroundings; accept any position
    // in [lo, hi] at which src splits into out's prefix and suffix.
    let n = out.len() - src.len();
    let ok = (lo..=hi).any(|p| src.is_char_boundary(p) && out.starts_with(&src[..p]) && out.ends_with(&src[p..]) && out.len() == src.len() + n);
    assert!(ok, "{ctx}: not a pure insertion in {lo}..={hi} (diff {a}..{b})");
}

fn removal(src: &str, out: &str, s: ElementSpan, ctx: &str) {
    let expect = format!("{}{}", &src[..s.start], &src[s.end()..]);
    assert!(out == expect, "{ctx}: not exactly the removed span");
}

#[test]
fn t2_every_edit_stays_local() {
    let mut st = Stats::default();
    for f in cases::FIXTURES {
        for b in bodies(f) {
            let ctx = |w: &str| format!("{f} slide {} shape {} cell {:?}: {w}", b.slide, b.id, b.cell);
            for (pi, p) in b.paras.iter().enumerate() {
                for (ri, (rspan, t, text)) in p.runs.iter().enumerate() {
                    // same value: no byte changes
                    if let Some((src, out)) = run(f, &b, &mut st, "run same", |x| x.paragraph_mut(pi)?.run_mut(ri)?.set_text(text)) {
                        assert_eq!(src, out, "{}", ctx("run same"));
                    }
                    if let Some((src, out)) = run(f, &b, &mut st, "run set", |x| x.paragraph_mut(pi)?.run_mut(ri)?.set_text("Tx & <y> 日本")) {
                        within(&src, &out, t.unwrap_or(*rspan), &ctx("run set"));
                    }
                }
                if let Some((src, out)) = run(f, &b, &mut st, "para set", |x| x.paragraph_mut(pi)?.set_text("P\u{b}Q")) {
                    within(&src, &out, p.span, &ctx("para set"));
                }
                if let Some((src, out)) = run(f, &b, &mut st, "add run", |x| x.paragraph_mut(pi)?.add_run("R")) {
                    match (p.span.is_self_closing(), p.run_insert_at) {
                        (false, Some(at)) => pure_insertion(&src, &out, at, at, &ctx("add run")),
                        _ => within(&src, &out, p.span, &ctx("add run")),
                    }
                }
                if let Some((src, out)) = run(f, &b, &mut st, "remove paragraph", |x| x.remove_paragraph(pi)) {
                    removal(&src, &out, p.span, &ctx("remove paragraph"));
                }
                if let Some(&(rspan, _, _)) = p.runs.first()
                    && let Some((src, out)) = run(f, &b, &mut st, "remove run", |x| x.paragraph_mut(pi)?.remove_run(0))
                {
                    removal(&src, &out, rspan, &ctx("remove run"));
                }
            }
            if let Some((src, out)) = run(f, &b, &mut st, "body set", |x| x.set_text("A\nB\u{b}C")) {
                within(&src, &out, b.span, &ctx("body set"));
            }
            if let Some((src, out)) = run(f, &b, &mut st, "add paragraph", |x| x.add_paragraph("N").map(|_| ())) {
                let last = b.paras.last().unwrap().span.end();
                let close = b.span.close.map_or(b.span.end(), |(c, _)| c);
                pure_insertion(&src, &out, last, close, &ctx("add paragraph"));
            }
        }
    }
    eprintln!("T2 checked: {:?}", st.checked);
    eprintln!("T2 refused (bytes unchanged): {:?}", st.refused);
    assert!(st.checked["run set"] >= 515, "{:?}", st.checked);
}
