//! Cross-run find/replace (ADR-0008 §4): the spike's invariants
//! (`spikes/pptx-phase1/findreplace.py`) as Rust tests, named rule tests,
//! refusals with byte-identical packages, and an opt-in corpus sweep.
//!
//! Invariants, for every paragraph of the slide after `replace_all`:
//! - **T** its text equals the old text with exactly the reported matches
//!   replaced (`str::replace` restricted to the accepted matches);
//! - **F** every kept character keeps its old `a:rPr` (canonical subtree),
//!   every inserted character before the first `\v` has the start run's
//!   `a:rPr`, each inserted `\v` is an `a:br` without `a:rPr`, and every
//!   inserted character after it has the start run's `a:rPr` without links;
//! - paragraphs without an accepted match are unchanged, and every part
//!   but the slide is byte-identical.

#[path = "oracle/cases.rs"]
mod cases;

use std::assert_matches;
use std::collections::BTreeMap;

use ooxml_core::{Package, RawItem, RawNode};
use ooxml_pptx::{
    Error, FindOptions, MixedFormat, OnRefusal, Presentation, ReplaceOptions, Scope, ShapeKind, TextItem,
    TextLocation,
};

const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

// ---------------------------------------------------------------------------
// Paragraph model: one (char, format) pair per character of the text.

fn canon(n: &RawNode, skip_links: bool) -> String {
    let mut attrs: Vec<String> = n.attrs().iter().map(|a| format!("{{{}}}{}={:?}", a.name().ns(), a.name().local(), a.value())).collect();
    attrs.sort();
    let mut s = format!("<{{{}}}{} {}>", n.name().ns(), n.name().local(), attrs.join(" "));
    for c in n.children() {
        match c {
            RawItem::Element(e) => {
                if skip_links && e.name().ns() == A_NS && matches!(e.name().local(), "hlinkClick" | "hlinkMouseOver") {
                    continue;
                }
                s.push_str(&canon(e, false));
            }
            // whitespace between child elements is not content (a:rPr has
            // element-only content; pretty-printed files indent it)
            RawItem::Text(t) if t.value().trim().is_empty() => {}
            RawItem::Text(t) => s.push_str(&format!("{:?}", t.value())),
            RawItem::Markup(_) => {}
        }
    }
    s
}

fn rpr(n: &RawNode, skip_links: bool) -> String {
    n.elements().find(|c| c.name().ns() == A_NS && c.name().local() == "rPr").map(|r| canon(r, skip_links)).unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
struct Model {
    chars: Vec<(char, String)>,
    /// Per character: the run's `a:rPr` without links (for `\v` copies).
    nolink: Vec<String>,
}

fn model(p: ooxml_pptx::ParagraphRef<'_>) -> Model {
    let mut m = Model { chars: Vec::new(), nolink: Vec::new() };
    for it in p.items() {
        match it {
            TextItem::Run(r) => {
                for c in r.text().chars() {
                    m.chars.push((c, rpr(r.raw(), false)));
                    m.nolink.push(rpr(r.raw(), true));
                }
            }
            TextItem::Break => {
                m.chars.push(('\u{b}', "BR".into()));
                m.nolink.push(String::new());
            }
            TextItem::Field(f) => {
                for c in f.text().chars() {
                    m.chars.push((c, format!("FLD{}", rpr(f.raw(), false))));
                    m.nolink.push(String::new());
                }
            }
            TextItem::Other(_) => {}
        }
    }
    m
}

fn text_of(m: &Model) -> String {
    m.chars.iter().map(|(c, _)| *c).collect()
}

/// Expected model after replacing `ranges` (byte ranges of the old text).
fn expected(old: &Model, ranges: &[std::ops::Range<usize>], replacement: &str) -> Model {
    let text = text_of(old);
    let byte_to_char: BTreeMap<usize, usize> = text.char_indices().enumerate().map(|(ci, (b, _))| (b, ci)).chain([(text.len(), old.chars.len())]).collect();
    let mut out = Model { chars: Vec::new(), nolink: Vec::new() };
    let mut pos = 0;
    for r in ranges {
        let (s, e) = (byte_to_char[&r.start], byte_to_char[&r.end]);
        out.chars.extend_from_slice(&old.chars[pos..s]);
        out.nolink.extend_from_slice(&old.nolink[pos..s]);
        let (start_fmt, start_nolink) = (old.chars[s].1.clone(), old.nolink[s].clone());
        let mut after_break = false;
        for c in replacement.chars() {
            if c == '\u{b}' {
                after_break = true;
                out.chars.push(('\u{b}', "BR".into()));
                out.nolink.push(String::new());
            } else {
                out.chars.push((c, if after_break { start_nolink.clone() } else { start_fmt.clone() }));
                out.nolink.push(start_nolink.clone());
            }
        }
        pos = e;
    }
    out.chars.extend_from_slice(&old.chars[pos..]);
    out.nolink.extend_from_slice(&old.nolink[pos..]);
    out
}

/// Every searchable paragraph of a slide, keyed by location (no AC: its
/// matches are always refused).
fn paragraphs(pres: &mut Presentation, slide: usize) -> BTreeMap<String, Model> {
    let s = pres.slide(slide).unwrap();
    let mut out = BTreeMap::new();
    for sh in s.shapes() {
        let key = |cell: Option<(usize, usize)>, k: usize| format!("{:?}/{cell:?}/{k}", sh.path().as_slice());
        if let Some(b) = sh.text_body() {
            for (k, p) in b.paragraphs().into_iter().enumerate() {
                out.insert(key(None, k), model(p));
            }
        }
        if let Some(t) = sh.table() {
            for (r, row) in t.rows().into_iter().enumerate() {
                for (c, cell) in row.into_iter().enumerate() {
                    if let Some(b) = cell.text_body() {
                        for (k, p) in b.paragraphs().into_iter().enumerate() {
                            out.insert(key(Some((r, c)), k), model(p));
                        }
                    }
                }
            }
        }
    }
    out
}

fn loc_key(l: &TextLocation) -> String {
    format!("{:?}/{:?}/{}", l.shape.as_slice(), l.cell, l.paragraph)
}

fn opts(slide: usize) -> ReplaceOptions {
    ReplaceOptions { find: FindOptions { scope: Scope::Slide(slide), ..FindOptions::default() }, ..ReplaceOptions::default() }
}

#[derive(Default, Debug)]
struct Stats {
    calls: usize,
    replaced: usize,
    skipped: BTreeMap<String, usize>,
}

/// Runs one replacement on a fresh copy and checks T, F, unchanged
/// paragraphs and untouched parts.
fn check(bytes: &[u8], slide: usize, pattern: &str, replacement: &str, o: &ReplaceOptions, st: &mut Stats) {
    let mut pres = Presentation::from_bytes(bytes.to_vec()).unwrap();
    let before = paragraphs(&mut pres, slide);
    let part = pres.slides().nth(slide).unwrap().unwrap().part_name;
    let report = pres.replace_all(pattern, replacement, o).unwrap();
    st.calls += 1;
    st.replaced += report.replaced.len();
    for (_, e) in &report.skipped {
        *st.skipped.entry(format!("{e:?}").split([' ', '(', '{']).next().unwrap().to_owned()).or_default() += 1;
    }
    let out = pres.to_bytes().unwrap();
    let mut after_pres = Presentation::from_bytes(out.clone()).unwrap();
    let after = paragraphs(&mut after_pres, slide);
    let mut accepted: BTreeMap<String, Vec<std::ops::Range<usize>>> = BTreeMap::new();
    for m in &report.replaced {
        assert_eq!(m.location.slide, slide);
        accepted.entry(loc_key(&m.location)).or_default().push(m.range.clone());
    }
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>(), "{pattern:?}: paragraph set changed");
    for (k, old) in &before {
        let mut ranges = accepted.get(k).cloned().unwrap_or_default();
        ranges.sort_by_key(|r| r.start);
        let exp = expected(old, &ranges, replacement);
        let got = &after[k];
        assert_eq!(text_of(got), text_of(&exp), "T: {k} {pattern:?} -> {replacement:?}");
        assert_eq!(got.chars, exp.chars, "F: {k} {pattern:?} -> {replacement:?}");
    }
    // Only the slide part may differ.
    let (a, b) = (Package::from_bytes(bytes.to_vec()).unwrap(), Package::from_bytes(out).unwrap());
    for name in a.part_names() {
        if name != part {
            assert_eq!(a.read_part(name).unwrap(), b.read_part(name).unwrap(), "{name} changed");
        }
    }
    if report.replaced.is_empty() {
        assert_eq!(a.read_part(&part).unwrap(), b.read_part(&part).unwrap(), "nothing replaced but {part} changed");
    }
}

/// Probes for a paragraph: up to `max` 2–3-word phrases that cross a run
/// boundary, plus its first word.
fn probes(m: &Model, max: usize) -> Vec<String> {
    let text = text_of(m);
    let mut out = Vec::new();
    let words: Vec<(usize, &str)> = text.split_whitespace().map(|w| (w.as_ptr() as usize - text.as_ptr() as usize, w)).collect();
    if let Some((_, w)) = words.first() {
        out.push((*w).to_owned());
    }
    let fmt_at = |byte: usize| text[..byte].chars().count();
    for n in [2, 3] {
        for win in words.windows(n) {
            let (s, e) = (win[0].0, win[n - 1].0 + win[n - 1].1.len());
            let pat = &text[s..e];
            let (cs, ce) = (fmt_at(s), fmt_at(e));
            let crosses = m.chars[cs..ce].windows(2).any(|w| w[0].1 != w[1].1);
            if crosses && !pat.contains(['\n', '\r', '\u{b}']) && out.len() <= max {
                out.push(pat.to_owned());
            }
        }
    }
    out
}

fn sweep(bytes: &[u8], probes_per_para: usize, st: &mut Stats) {
    let Ok(mut pres) = Presentation::from_bytes(bytes.to_vec()) else { return };
    for slide in 0..pres.slide_count() {
        if pres.slide(slide).is_err() {
            continue;
        }
        let paras = paragraphs(&mut pres, slide);
        let mut pats: Vec<String> = paras.values().flat_map(|m| probes(m, probes_per_para)).collect();
        pats.sort();
        pats.dedup();
        for p in &pats {
            for r in ["\u{1E8C}", "", "Q\u{b}Z", "\u{b}"] {
                check(bytes, slide, p, r, &opts(slide), st);
            }
        }
    }
}

#[test]
fn invariants_on_every_fixture() {
    let mut st = Stats::default();
    for f in cases::FIXTURES {
        sweep(&cases::read_fixture(f), 2, &mut st);
    }
    eprintln!("{st:?}");
    assert!(st.calls > 500 && st.replaced > 500, "{st:?}");
    // The fixtures exercise every refusal class the rules define.
    // (BreakInMatch is defensive: a pattern cannot contain `\v`, so a
    // match never covers an `a:br`; AC text is tested on a synthetic deck.)
    for k in ["FieldInMatch", "LinkSplitByBreak", "FormatNotCopyable"] {
        assert!(st.skipped.contains_key(k), "no {k} in the sweep: {st:?}");
    }
}

/// Opt-in: `OOXML_PPTX_CORPUS=dir1:dir2 cargo test -p ooxml-pptx --test replace -- --ignored`
/// runs the same invariants over a third-party corpus (untrusted files are
/// only parsed). Prints the counts.
#[test]
#[ignore = "needs OOXML_PPTX_CORPUS"]
fn invariants_on_a_corpus() {
    let Ok(dirs) = std::env::var("OOXML_PPTX_CORPUS") else { return };
    let mut st = Stats::default();
    let mut files = 0;
    for d in dirs.split(':') {
        let mut entries: Vec<_> = std::fs::read_dir(d).unwrap().filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for p in entries.into_iter().filter(|p| p.extension().is_some_and(|e| e == "pptx")) {
            files += 1;
            sweep(&std::fs::read(&p).unwrap(), 2, &mut st);
        }
    }
    eprintln!("{files} files: {st:?}");
}

// ---------------------------------------------------------------------------
// Rule tests on synthetic paragraphs (SYNTHETIC: the subtitle paragraph of
// poi/sample.pptx slide 1 replaced by the given XML, in memory).

const SUBTITLE: &str = "<a:r><a:rPr lang=\"en-US\"><a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/></a:rPr><a:t>Nunc at risus vel erat tempus posuere. Aenean non ante.</a:t></a:r>";

fn deck(runs: &str) -> Vec<u8> {
    let mut pkg = Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    let xml = String::from_utf8(pkg.read_part("ppt/slides/slide1.xml").unwrap()).unwrap();
    assert!(xml.contains(SUBTITLE));
    pkg.set_part("ppt/slides/slide1.xml", xml.replacen(SUBTITLE, runs, 1).into_bytes()).unwrap();
    pkg.to_bytes().unwrap()
}

fn subtitle(pres: &mut Presentation) -> (Vec<String>, String) {
    let s = pres.slide(0).unwrap();
    let sh = s.shapes().into_iter().find(|x| x.name() == Some("Rectangle 3")).unwrap();
    let p = sh.text_body().unwrap().paragraphs()[0];
    let items = p
        .items()
        .into_iter()
        .map(|it| match it {
            TextItem::Run(r) => format!("r[{}]{}", rpr(r.raw(), false).len(), r.text()),
            TextItem::Break => "br".into(),
            TextItem::Field(f) => format!("fld:{}", f.text()),
            TextItem::Other(_) => "other".into(),
        })
        .collect();
    (items, p.text())
}

fn replaced(runs: &str, pat: &str, rep: &str, o: &ReplaceOptions) -> (Presentation, ooxml_pptx::ReplaceReport) {
    let mut pres = Presentation::from_bytes(deck(runs)).unwrap();
    let r = pres.replace_all(pat, rep, o).unwrap();
    (pres, r)
}

fn r(attrs: &str, children: &str, t: &str) -> String {
    format!("<a:r><a:rPr lang=\"en-US\"{attrs}>{children}</a:rPr><a:t>{t}</a:t></a:r>")
}

#[test]
fn rules_2_to_4_start_run_keeps_its_format() {
    let runs = format!("{}{}{}", r("", "", "Hello wo"), r(" b=\"1\"", "", "rld and "), r(" i=\"1\"", "", "more"));
    // across three runs: start run gets the replacement, middle removed, last trimmed
    let (mut p, rep) = replaced(&runs, "wo", "WO", &opts(0));
    assert_eq!(rep.replaced.len(), 1);
    assert_eq!(subtitle(&mut p).1, "Hello WOrld and more");
    let (mut p, _) = replaced(&runs, "world and mo", "X", &opts(0));
    let (items, text) = subtitle(&mut p);
    assert_eq!(text, "Hello Xre");
    assert_eq!(items.len(), 2, "{items:?}"); // middle run removed, last trimmed to "re"
    assert!(items[1].ends_with("re"));
    // a deletion that empties a touched run removes it; the last run stays
    let (mut p, _) = replaced(&runs, "rld and ", "", &opts(0));
    assert_eq!(subtitle(&mut p).1, "Hello womore");
    assert_eq!(subtitle(&mut p).0.len(), 2);
    // deleting everything keeps one empty run (rule 4)
    let (mut p, _) = replaced(&runs, "Hello world and more", "", &opts(0));
    let (items, text) = subtitle(&mut p);
    assert_eq!(text, "");
    assert!(items.len() == 1 && items[0].starts_with("r[") && items[0].ends_with(']'), "{items:?}");
}

#[test]
fn rule_4_keeps_one_run_when_every_touched_run_empties() {
    // two matches empty both runs: one (the first) stays with an empty a:t
    let runs = format!("{}{}", r("", "", "x"), r(" b=\"1\"", "", "x"));
    let (mut p, rep) = replaced(&runs, "x", "", &opts(0));
    assert_eq!(rep.replaced.len(), 2);
    let (items, text) = subtitle(&mut p);
    assert_eq!(text, "");
    assert_eq!(items.len(), 1, "{items:?}");
}

#[test]
fn unsupported_paragraph_content_is_refused() {
    // SYNTHETIC: an a14:m (math) item between two runs
    let runs = format!("{}<a14:m xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\"/>{}", r("", "", "left"), r("", "", "right"));
    let input = deck(&runs);
    let mut pres = Presentation::from_bytes(input.clone()).unwrap();
    let rep = pres.replace_all("left", "L", &opts(0)).unwrap();
    assert_matches!(rep.skipped[..], [(_, Error::UnsupportedContent(_))]);
    assert_eq!(pres.to_bytes().unwrap(), input);
}

#[test]
fn rule_1_fields_and_breaks_are_refused_byte_identical() {
    let runs = format!("{}<a:br/>{}<a:fld id=\"{{B6F15528-21DE-4FAA-801E-634DDDAF4B2B}}\" type=\"slidenum\"><a:rPr lang=\"en-US\"/><a:t>7</a:t></a:fld>{}", r("", "", "one"), r("", "", "two "), r("", "", " three"));
    let input = deck(&runs);
    for (pat, err) in [("one\u{b}two", None), ("e", None), ("two 7", Some("FieldInMatch")), ("7 t", Some("FieldInMatch"))] {
        let mut pres = Presentation::from_bytes(input.clone()).unwrap();
        let res = pres.replace_all(pat, "X", &opts(0));
        match err {
            None if pat.contains('\u{b}') => assert_matches!(res, Err(Error::InvalidPattern)),
            None => assert!(res.unwrap().skipped.is_empty()),
            Some(_) => {
                let rep = res.unwrap();
                assert_matches!(rep.skipped[..], [(_, Error::FieldInMatch)]);
                assert!(rep.replaced.is_empty());
                assert_eq!(pres.to_bytes().unwrap(), input, "refused match changed the package");
            }
        }
    }
    // a match covering a break: the break is "\v", so only a pattern with
    // \v could cover it, which is InvalidPattern; BreakInMatch is reached
    // through find's view of the text and checked in the sweep.
}

#[test]
fn rule_5_line_break_in_replacement() {
    let runs = format!("{}{}", r(" b=\"1\"", "", "alpha beta"), r(" i=\"1\"", "", " gamma"));
    let (mut p, _) = replaced(&runs, "beta", "B1\u{b}B2", &opts(0));
    let (items, text) = subtitle(&mut p);
    assert_eq!(text, "alpha B1\u{b}B2 gamma");
    // start run "alpha B1", br, new run "B2" (copy of the start format), the italic run
    assert_eq!(items.len(), 4, "{items:?}");
    assert_eq!(items[1], "br");
    assert_eq!(items[0][..items[0].find(']').unwrap()], items[2][..items[2].find(']').unwrap()]);
    // the suffix inside the start run goes to the last new run
    let (mut p, _) = replaced(&runs, "alpha", "A\u{b}", &opts(0));
    let (items, text) = subtitle(&mut p);
    assert_eq!(text, "A\u{b} beta gamma");
    assert_eq!(items.len(), 4, "{items:?}");
    // an rPr with child elements is not copyable in Phase 0 (attribute-only copy)
    let runs = r("", "<a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill>", "red text");
    let input = deck(&runs);
    let mut pres = Presentation::from_bytes(input.clone()).unwrap();
    let rep = pres.replace_all("red", "R\u{b}D", &opts(0)).unwrap();
    assert_matches!(rep.skipped[..], [(_, Error::FormatNotCopyable("a:rPr"))]);
    assert_eq!(pres.to_bytes().unwrap(), input);
}

#[test]
fn rule_5_link_split_by_break() {
    let link = "<a:hlinkClick r:id=\"rId99\"/>";
    let runs = format!("{}{}", r("", link, "click here now"), r("", "", " after"));
    let input = deck(&runs);
    // ends inside the linked run, with a \v: refused, byte-identical
    let mut pres = Presentation::from_bytes(input.clone()).unwrap();
    let rep = pres.replace_all("here", "H\u{b}", &opts(0)).unwrap();
    assert_matches!(rep.skipped[..], [(_, Error::LinkSplitByBreak)]);
    assert_eq!(pres.to_bytes().unwrap(), input);
    // without \v: the link stays on the whole start run
    let (mut p, _) = replaced(&runs, "here", "THERE", &opts(0));
    assert_eq!(subtitle(&mut p).1, "click THERE now after");
    // Abort: nothing changes even though another match would be fine
    let runs2 = format!("{}{}", r("", link, "here"), r("", "", " x here y"));
    let input2 = deck(&runs2);
    let mut pres = Presentation::from_bytes(input2.clone()).unwrap();
    let o = ReplaceOptions { on_refusal: OnRefusal::Abort, ..opts(0) };
    let rep = pres.replace_all("her", "H\u{b}", &o).unwrap();
    assert!(rep.aborted && rep.replaced.is_empty() && rep.skipped.len() == 1, "{rep:?}");
    assert_eq!(pres.to_bytes().unwrap(), input2);
    // Skip: the other match is applied
    let mut pres = Presentation::from_bytes(input2).unwrap();
    let rep = pres.replace_all("her", "H\u{b}", &opts(0)).unwrap();
    assert_eq!((rep.replaced.len(), rep.skipped.len()), (1, 1));
}

#[test]
fn rule_6_mixed_format() {
    let runs = format!("{}{}{}", r("", "", "ab"), r(" err=\"1\"", "", "cd"), r(" b=\"1\"", "", "ef"));
    let o = ReplaceOptions { mixed_format: MixedFormat::Refuse, ..opts(0) };
    // proofing-only difference: allowed
    let (mut p, rep) = replaced(&runs, "bc", "X", &o);
    assert!(rep.skipped.is_empty());
    assert_eq!(subtitle(&mut p).1, "aXdef");
    // visible difference: refused under Refuse, allowed by default
    let (_, rep) = replaced(&runs, "de", "X", &o);
    assert_matches!(rep.skipped[..], [(_, Error::MixedFormatInMatch)]);
    let (mut p, rep) = replaced(&runs, "de", "X", &opts(0));
    assert!(rep.skipped.is_empty());
    assert_eq!(subtitle(&mut p).1, "abcXf");
    // different links count as a visible difference
    let runs = format!("{}{}", r("", "<a:hlinkClick r:id=\"rId1\"/>", "ab"), r("", "<a:hlinkClick r:id=\"rId2\"/>", "cd"));
    let (_, rep) = replaced(&runs, "bc", "X", &o);
    assert_matches!(rep.skipped[..], [(_, Error::MixedFormatInMatch)]);
}

#[test]
fn arguments_and_scope() {
    let mut pres = Presentation::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    let before = pres.to_bytes().unwrap();
    for (pat, rep) in [("", "x"), ("a\nb", "x"), ("a\u{b}", "x")] {
        assert_matches!(pres.replace_all(pat, rep, &ReplaceOptions::default()), Err(Error::InvalidPattern));
    }
    assert_matches!(pres.replace_all("Lorem", "a\nb", &ReplaceOptions::default()), Err(Error::LineBreakInParagraph));
    assert_matches!(pres.replace_all("Lorem", "a\u{1}", &ReplaceOptions::default()), Err(Error::InvalidXmlChar { .. }));
    let o = ReplaceOptions { find: FindOptions { scope: Scope::Slide(9), ..FindOptions::default() }, ..ReplaceOptions::default() };
    assert_matches!(pres.replace_all("Lorem", "x", &o), Err(Error::SlideIndexOutOfRange { .. }));
    assert_eq!(pres.to_bytes().unwrap(), before);
    // find: case folding is ASCII only and never dirties
    let found = pres.find("lorem", &FindOptions { ascii_case_insensitive: true, ..FindOptions::default() }).unwrap();
    assert_eq!(found.len(), pres.find("Lorem", &FindOptions::default()).unwrap().len());
    assert!(!found.is_empty());
    assert!(pres.find("lorem", &FindOptions::default()).unwrap().is_empty());
    assert_eq!(pres.to_bytes().unwrap(), before);
}

/// Refusals that come from ADR-0005 §6 locks and shape kinds.
#[test]
fn locks_and_containers_are_refused() {
    // SYNTHETIC: the title p:sp of poi/sample.pptx slide 1 wrapped in
    // mc:AlternateContent (Choice + Fallback): found, never replaced.
    let mut pkg = Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    let xml = String::from_utf8(pkg.read_part("ppt/slides/slide1.xml").unwrap()).unwrap();
    let (a0, a1) = (xml.find("<p:sp>").unwrap(), xml.find("</p:sp>").unwrap() + "</p:sp>".len());
    let sp = &xml[a0..a1];
    let ac = format!(
        r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main" Requires="p14">{sp}</mc:Choice><mc:Fallback>{sp}</mc:Fallback></mc:AlternateContent>"#
    );
    pkg.set_part("ppt/slides/slide1.xml", format!("{}{ac}{}", &xml[..a0], &xml[a1..]).into_bytes()).unwrap();
    let input = pkg.to_bytes().unwrap();
    let mut pres = Presentation::from_bytes(input.clone()).unwrap();
    let found = pres.find("Lorem", &FindOptions { scope: Scope::Slide(0), ..FindOptions::default() }).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(pres.slide(0).unwrap().shapes().into_iter().find(|x| x.path() == &found[0].location.shape).unwrap().kind(), &ShapeKind::AlternateContent);
    let rep = pres.replace_all("Lorem", "X", &opts(0)).unwrap();
    assert_matches!(rep.skipped[..], [(_, Error::InsideAlternateContent)]);
    assert_eq!(pres.to_bytes().unwrap(), input);
    // merged table cells (table_test2): refused with MergedCell
    let mut pres = Presentation::from_bytes(cases::read_fixture("poi/table_test2.pptx")).unwrap();
    let rep = pres.replace_all("e", "E", &ReplaceOptions::default()).unwrap();
    let merged = rep.skipped.iter().filter(|(_, e)| matches!(e, Error::MergedCell { .. })).count();
    let found_in_merged = rep.skipped.iter().filter(|(m, _)| m.location.cell.is_some()).count();
    assert_eq!(merged, found_in_merged);
    // SYNTHETIC: p:pRg → p:charRg in 45545_Comment slide 1 locks the shape's characters
    let mut pkg = Package::from_bytes(cases::read_fixture("poi/45545_Comment.pptx")).unwrap();
    let part = "ppt/slides/slide1.xml";
    let xml = String::from_utf8(pkg.read_part(part).unwrap()).unwrap();
    assert!(xml.contains("<p:pRg "));
    pkg.set_part(part, xml.replace("<p:pRg ", "<p:charRg ").into_bytes()).unwrap();
    let input = pkg.to_bytes().unwrap();
    let mut pres = Presentation::from_bytes(input.clone()).unwrap();
    let idx = pres.slides().position(|i| i.unwrap().part_name == part).unwrap();
    let o = ReplaceOptions { on_refusal: OnRefusal::Abort, ..opts(idx) };
    // (shape 584707's only text is three tabs)
    let rep = pres.replace_all("\t", " ", &o).unwrap();
    assert!(rep.aborted && rep.skipped.iter().any(|(_, e)| matches!(e, Error::AnimatedCharacters { .. })), "{rep:?}");
    let locked_shape = rep.skipped.iter().find(|(_, e)| matches!(e, Error::AnimatedCharacters { shape_id: 584707 })).unwrap().0.location.shape.clone();
    assert_eq!(pres.to_bytes().unwrap(), input, "aborted replace changed the package");
    // without the char lock (real file, paragraph lock only): replacing is allowed
    let mut pres = Presentation::from_bytes(cases::read_fixture("poi/45545_Comment.pptx")).unwrap();
    let rep = pres.replace_all("\t", " ", &opts(idx)).unwrap();
    assert!(rep.replaced.iter().any(|m| m.location.shape == locked_shape), "the paragraph-locked shape is edited");
    assert!(!rep.skipped.iter().any(|(_, e)| matches!(e, Error::AnimatedParagraphs { .. })));
}

/// Locality: only the touched paragraph's bytes change.
#[test]
fn replacement_is_local_to_the_paragraph() {
    let input = cases::read_fixture("poi/sample.pptx");
    let mut pres = Presentation::from_bytes(input.clone()).unwrap();
    pres.replace_all("ipsum", "IPSUM", &ReplaceOptions::default()).unwrap();
    let a = String::from_utf8(Package::from_bytes(input).unwrap().read_part("ppt/slides/slide1.xml").unwrap()).unwrap();
    let b = String::from_utf8(Package::from_bytes(pres.to_bytes().unwrap()).unwrap().read_part("ppt/slides/slide1.xml").unwrap()).unwrap();
    let pre = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let post = a.bytes().rev().zip(b.bytes().rev()).take_while(|(x, y)| x == y).count();
    assert_eq!(&a[pre..a.len() - post], "ipsum");
    assert_eq!(&b[pre..b.len() - post], "IPSUM");
}
