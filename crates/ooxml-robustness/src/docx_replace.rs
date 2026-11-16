//! `ooxml-docx` Phase1 cross-run find and replace (ADR-0007, PR #52):
//! `find`, `replace`, `replace_at` and `replace_matches` over every story
//! (main body, headers, footers, footnotes, endnotes, comments).
//!
//! Needles are cut from the documents' own paragraph texts at random
//! character boundaries, so they cross runs, tabs, breaks and hyphen
//! characters; replacements mix printable text (with CJK, tab, newline) and
//! arbitrary strings. Checks:
//!
//! - a refused call (`Err`) leaves the package byte-identical;
//! - after `Ok`, every paragraph's text is its old text with exactly the
//!   applied matches replaced (all needle occurrences minus the report's
//!   skipped ones; `\r\n`/`\r` in a replacement read back as `\n`), and
//!   `replaced` counts them; paragraphs without an applied match keep their
//!   text and their generation, changed paragraphs get a new generation;
//! - per story part, the bytes change only inside the spans of the touched
//!   paragraphs; parts without an applied match stay Layer-1 identical;
//! - every `w:rPr` of a touched paragraph after the edit equals one it had
//!   before (the replacement takes an existing run's formatting; kept runs
//!   keep theirs);
//! - a `Match` whose paragraph changed since `find` is `StaleMatch`, and
//!   changes nothing; one from an unchanged paragraph is still accepted;
//! - the output reopens with the same paragraph texts.

use std::collections::BTreeMap;

use ooxml_core::{RawNode, XmlDocument};
use ooxml_docx::{Document, Error, Match, OnRefused, ParaRef, ReplaceOptions, ReplaceReport};

use crate::fixtures::{DOCX, decode};
use crate::input::Input;
use crate::zipcmp::raw_entries;

/// Oracle counters: calls with applied matches, applied matches, skipped
/// matches, refused calls, stale matches refused, changed paragraphs.
pub static STATS: [std::sync::atomic::AtomicUsize; 6] = [const { std::sync::atomic::AtomicUsize::new(0) }; 6];

fn bump(i: usize, n: usize) {
    STATS[i].fetch_add(n, std::sync::atomic::Ordering::Relaxed);
}

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Every paragraph of every story: (ref, text).
fn texts(doc: &Document) -> BTreeMap<ParaRef, String> {
    doc.stories()
        .flat_map(|s| {
            let idx = s.index();
            s.paragraphs().enumerate().map(move |(i, p)| (ParaRef::new(idx, i), p.text())).collect::<Vec<_>>()
        })
        .collect()
}

fn story_part(doc: &Document, p: ParaRef) -> String {
    doc.story_at(p.story()).map(|s| s.id().part().to_owned()).unwrap_or_default()
}

fn rprs(n: &RawNode, out: &mut Vec<RawNode>) {
    for c in n.elements() {
        if c.name().ns() == W_NS && c.name().local() == "rPr" {
            out.push(c.clone());
        } else {
            rprs(c, out);
        }
    }
}

/// A needle cut from a random non-empty paragraph text (1–8 characters).
fn needle(all: &BTreeMap<ParaRef, String>, ops: &mut Input<'_>) -> Option<String> {
    let non_empty: Vec<&String> = all.values().filter(|t| !t.is_empty()).collect();
    if non_empty.is_empty() {
        return None;
    }
    let t = non_empty[usize::from(ops.u16()) % non_empty.len()];
    let chars: Vec<(usize, char)> = t.char_indices().collect();
    let start = usize::from(ops.u16()) % chars.len();
    let len = 1 + usize::from(ops.u8()) % 8;
    let end = chars.get(start + len).map_or(t.len(), |(i, _)| *i);
    Some(t[chars[start].0..end].to_owned())
}

fn replacement(ops: &mut Input<'_>) -> String {
    match ops.u8() % 4 {
        0 => ops.string(),
        1 => String::new(),
        _ => ops.printable(),
    }
}

/// Fuzz entry: a fixture-based input (see [`crate::fixtures`]); the fixtures
/// are the docx fixtures plus [`targeted_docs`].
pub fn check_docx_replace_input(data: &[u8]) {
    if let Some((zip, ops)) = decode(&REPLACE_FIXTURES, data) {
        check_docx_replace(&zip, ops);
    }
}

static REPLACE_FIXTURES: std::sync::LazyLock<Vec<crate::fixtures::Fixture>> = std::sync::LazyLock::new(|| {
    let mut v: Vec<crate::fixtures::Fixture> =
        DOCX.iter().map(|f| crate::fixtures::Fixture { name: f.name.clone(), parts: f.parts.clone() }).collect();
    v.extend(targeted_docs());
    v
});

/// The fixture list of this target (index = an input's first byte).
pub fn replace_fixtures() -> &'static [crate::fixtures::Fixture] {
    &REPLACE_FIXTURES
}

/// Checks one document; see the module doc.
pub fn check_docx_replace(zip: &[u8], mut ops: Input<'_>) {
    let Ok(mut doc) = Document::from_bytes(zip.to_vec()) else { return };
    let mut old_matches: Vec<Match> = Vec::new();
    let mut steps = 0;
    while !ops.is_empty() && steps < 6 {
        steps += 1;
        let before = doc.to_bytes().expect("saves");
        let before_texts = texts(&doc);
        let before_gen: BTreeMap<ParaRef, u64> = before_texts.keys().map(|p| (*p, doc.generation(*p))).collect();
        let before_doc = Document::from_bytes(before.clone()).expect("reopens");
        let Some(needle) = needle(&before_texts, &mut ops) else { return };
        let kind = ops.u8() % 5;
        let on_refused = if ops.u8().is_multiple_of(3) { OnRefused::Refuse } else { OnRefused::Skip };
        let opts = ReplaceOptions::default().on_refused(on_refused);
        let found = doc.find(&needle, &opts);
        let found = match found {
            Ok(f) => f,
            Err(e) => {
                assert!(matches!(e, Error::InvalidXmlChar { .. } | Error::EmptyNeedle), "find({needle:?}) failed: {e:?}");
                continue;
            }
        };
        // what each call applies: (match, replacement)
        let rep = replacement(&mut ops);
        let (result, attempted): (Result<ReplaceReport, Error>, Vec<(Match, String)>) = match kind {
            0 | 1 => (doc.replace(&needle, &rep, &opts), found.iter().map(|m| (m.clone(), rep.clone())).collect()),
            2 if !found.is_empty() => {
                let m = found[usize::from(ops.u8()) % found.len()].clone();
                (doc.replace_at(&m, &rep, &opts), vec![(m, rep.clone())])
            }
            3 if !found.is_empty() => {
                let mut picked: Vec<(Match, String)> = Vec::new();
                for m in &found {
                    if ops.u8().is_multiple_of(2) {
                        picked.push((m.clone(), replacement(&mut ops)));
                    }
                }
                let edits: Vec<(&Match, &str)> = picked.iter().map(|(m, r)| (m, r.as_str())).collect();
                (doc.replace_matches(&edits, &opts), picked)
            }
            _ => {
                // a match from an earlier find: StaleMatch exactly when its
                // paragraph changed since
                let Some(m) = old_matches.get(usize::from(ops.u8()) % old_matches.len().max(1)).cloned() else { continue };
                let stale = doc.generation(m.para) != m.generation;
                let r = doc.replace_at(&m, &rep, &opts);
                if stale {
                    assert!(matches!(r, Err(Error::StaleMatch)), "a match from a changed paragraph gave {r:?}");
                    bump(4, 1);
                }
                (r, vec![(m, rep.clone())])
            }
        };
        old_matches.extend(found.iter().cloned());
        let after = doc.to_bytes().expect("saves");
        let report = match result {
            Err(e) => {
                bump(3, 1);
                assert!(after == before, "refused replace (kind {kind}, {e:?}) changed the package");
                continue;
            }
            Ok(r) => r,
        };
        assert!(on_refused == OnRefused::Skip || report.skipped.is_empty(), "OnRefused::Refuse returned skipped matches");
        // expected texts
        let skipped: Vec<(ParaRef, std::ops::Range<usize>)> = report.skipped.iter().map(|s| (s.para, s.range.clone())).collect();
        let applied: Vec<&(Match, String)> =
            attempted.iter().filter(|(m, _)| !skipped.iter().any(|(p, r)| *p == m.para && *r == m.range)).collect();
        assert_eq!(report.replaced, applied.len(), "replaced count (kind {kind}, needle {needle:?})");
        bump(0, usize::from(!applied.is_empty()));
        bump(1, applied.len());
        bump(2, report.skipped.len());
        let mut expected = before_texts.clone();
        let mut by_para: BTreeMap<ParaRef, Vec<&(Match, String)>> = BTreeMap::new();
        for a in &applied {
            by_para.entry(a.0.para).or_default().push(a);
        }
        for (p, list) in &by_para {
            let old = &before_texts[p];
            let mut s = String::new();
            let mut at = 0;
            let mut list = list.clone();
            list.sort_by_key(|(m, _)| m.range.start);
            for (m, r) in list {
                s.push_str(&old[at..m.range.start]);
                s.push_str(&normalize(r));
                at = m.range.end;
            }
            s.push_str(&old[at..]);
            expected.insert(*p, s);
        }
        let now = texts(&doc);
        for (p, want) in &expected {
            assert_eq!(now.get(p), Some(want), "paragraph {p:?} after replacing {needle:?} (kind {kind})");
        }
        // locality per part, and rPr provenance
        let changed_paras: Vec<ParaRef> = by_para.keys().copied().filter(|p| now[p] != before_texts[p]).collect();
        bump(5, changed_paras.len());
        // generations: changed paragraphs get a new one, others keep theirs
        for (p, g) in &before_gen {
            let changed = now[p] != before_texts[p];
            assert_eq!(doc.generation(*p) != *g, changed, "generation of {p:?} (text changed: {changed})");
        }
        // formatting rule (ADR-0007 §1.3, independently): the runs an
        // applied match spans have the same formatting up to proofing and
        // font hints
        for (m, r) in &applied {
            if *r != m.text {
                assert_uniform_formatting(&before_doc, m);
            }
        }
        let parts_touched: std::collections::BTreeSet<String> = changed_paras.iter().map(|p| story_part(&doc, *p)).collect();
        check_parts(&before, &after, &parts_touched, &doc, &changed_paras);
        let re = Document::from_bytes(after.clone()).unwrap_or_else(|e| panic!("edited document does not reopen: {e:?}"));
        assert_eq!(texts(&re), now, "paragraph texts after reopen");
    }
}

/// Parts other than the touched story parts are L1-identical; inside a
/// touched part the bytes change only within the touched paragraphs'
/// spans (measured on the reopened output), and every `w:rPr` there
/// existed before.
fn check_parts(before: &[u8], after: &[u8], touched: &std::collections::BTreeSet<String>, doc: &Document, changed: &[ParaRef]) {
    let a = raw_entries(before).expect("raw");
    let b = raw_entries(after).expect("raw");
    assert_eq!(a.iter().map(|e| &e.name).collect::<Vec<_>>(), b.iter().map(|e| &e.name).collect::<Vec<_>>(), "entry names changed");
    let pa = ooxml_core::Package::from_bytes(before.to_vec()).expect("reopens");
    let pb = ooxml_core::Package::from_bytes(after.to_vec()).expect("reopens");
    let old_doc = Document::from_bytes(before.to_vec()).expect("reopens");
    for (x, y) in a.iter().zip(&b) {
        if x.compressed == y.compressed && x.crc32 == y.crc32 {
            continue;
        }
        assert!(touched.contains(&x.name), "{} changed but holds no replaced match", x.name);
        let (sa, sb) = (pa.read_part(&x.name).expect("part"), pb.read_part(&x.name).expect("part"));
        // the touched paragraphs' spans in the old part
        let spans: Vec<(usize, usize)> = changed
            .iter()
            .filter(|p| story_part(doc, **p) == x.name)
            .filter_map(|p| old_doc.paragraph_at(*p).and_then(|q| q.raw().span()).map(|s| (s.start, s.end())))
            .collect();
        if spans.is_empty() {
            continue;
        }
        let (lo, hi) = (spans.iter().map(|s| s.0).min().expect("some"), spans.iter().map(|s| s.1).max().expect("some"));
        let prefix = sa.iter().zip(&sb).take_while(|(p, q)| p == q).count();
        let suffix = sa.iter().rev().zip(sb.iter().rev()).take_while(|(p, q)| p == q).count();
        assert!(prefix >= lo && suffix >= sa.len() - hi, "{}: bytes changed outside the touched paragraphs ({lo}..{hi})", x.name);
        // rPr provenance
        let (xa, xb) = (XmlDocument::parse(&sa).expect("parses"), XmlDocument::parse(&sb).expect("parses"));
        let (mut before_rpr, mut after_rpr) = (Vec::new(), Vec::new());
        rprs(xa.root(), &mut before_rpr);
        rprs(xb.root(), &mut after_rpr);
        for r in &after_rpr {
            assert!(before_rpr.iter().any(|o| o.structurally_eq(r)), "{}: a w:rPr that did not exist before", x.name);
        }
    }
}

/// Small documents for the shapes ADR-0007 is about: a placeholder split
/// across runs with identical formatting (rsid splits), proofing-only
/// differences, visible differences, a field, a hyperlink edge, a
/// bookmark inside, tabs and breaks.
pub fn targeted_docs() -> Vec<crate::fixtures::Fixture> {
    let bodies: &[(&str, &str)] = &[
        (
            "split",
            r#"<w:p><w:r w:rsidR="1"><w:t>Dear {{</w:t></w:r><w:r w:rsidR="2"><w:t>na</w:t></w:r><w:r><w:t xml:space="preserve">me}}, hi </w:t></w:r><w:r><w:tab/><w:t>x</w:t><w:br/><w:t>y</w:t></w:r></w:p>"#,
        ),
        (
            "proofing",
            r#"<w:p><w:r><w:rPr><w:noProof/><w:lang w:val="en-US"/></w:rPr><w:t>{{a</w:t></w:r><w:r><w:rPr><w:lang w:val="ja-JP"/></w:rPr><w:t>b}} 日本</w:t></w:r></w:p>"#,
        ),
        ("mixed", r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r><w:r><w:t>plain</w:t></w:r></w:p>"#),
        (
            "field",
            r#"<w:p><w:r><w:t>see </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t> end</w:t></w:r></w:p>"#,
        ),
        (
            "hyperlink-bookmark",
            r#"<w:p><w:r><w:t>go </w:t></w:r><w:hyperlink w:anchor="x"><w:r><w:t>here now</w:t></w:r></w:hyperlink><w:r><w:t>ab</w:t></w:r><w:bookmarkStart w:id="0" w:name="m"/><w:r><w:t>cd</w:t></w:r><w:bookmarkEnd w:id="0"/></w:p>"#,
        ),
    ];
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#;
    bodies
        .iter()
        .map(|(name, body)| {
            let document = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W_NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>{body}<w:sectPr><w:headerReference w:type="default" r:id="rId1"/></w:sectPr></w:body></w:document>"#
            );
            let header = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="{W_NS}">{body}</w:hdr>"#);
            let parts = vec![
                ("[Content_Types].xml".to_owned(), ct.as_bytes().to_vec()),
                ("_rels/.rels".to_owned(), rels.as_bytes().to_vec()),
                ("word/document.xml".to_owned(), document.into_bytes()),
                ("word/_rels/document.xml.rels".to_owned(), doc_rels.as_bytes().to_vec()),
                ("word/header1.xml".to_owned(), header.into_bytes()),
            ];
            crate::fixtures::Fixture { name: format!("replace/{name}.docx"), parts }
        })
        .collect()
}


/// `w:rPr` without what `RunFormatting::IgnoreInvisible` may ignore (a
/// superset of it: `w:noProof`, `w:lang`, `w:rFonts/@w:hint`), so the
/// check never refuses what the library may accept.
fn visible_rpr(run: &RawNode) -> String {
    let Some(rpr) = run.elements().find(|e| e.name().ns() == W_NS && e.name().local() == "rPr") else { return String::new() };
    let mut out = String::new();
    for c in rpr.elements() {
        let l = c.name().local();
        if c.name().ns() == W_NS && (l == "noProof" || l == "lang") {
            continue;
        }
        if l == "rFonts" && c.attrs().iter().all(|a| a.name().local() == "hint") && c.elements().next().is_none() {
            continue; // nothing left once the hint is dropped
        }
        out.push_str(l);
        for a in c.attrs() {
            if l == "rFonts" && a.name().local() == "hint" {
                continue;
            }
            out.push_str(&format!(" {}={:?}", a.name().local(), a.value()));
        }
        // nested children (rare): compare structurally by text
        if c.elements().next().is_some() {
            out.push_str(&format!("{:?}", c.elements().map(|e| e.name().local().to_owned()).collect::<Vec<_>>()));
        }
        out.push(';');
    }
    out
}

fn assert_uniform_formatting(doc: &Document, m: &Match) {
    let Some(p) = doc.paragraph_at(m.para) else { return };
    let runs: Vec<_> = p.runs().collect();
    // offsets are reliable only when every run's text is in Paragraph::text
    if runs.iter().any(|r| matches!(r.context(), ooxml_docx::RunContext::Deleted | ooxml_docx::RunContext::MovedFrom)) {
        return;
    }
    let total: usize = runs.iter().map(|r| r.text().len()).sum();
    if total != p.text().len() {
        return;
    }
    let mut at = 0;
    let mut seen: Vec<String> = Vec::new();
    for r in &runs {
        let len = r.text().len();
        if len > 0 && at < m.range.end && at + len > m.range.start {
            seen.push(visible_rpr(r.raw()));
        }
        at += len;
    }
    assert!(seen.windows(2).all(|w| w[0] == w[1]), "an applied match {:?} spans runs with visibly different formatting: {seen:?}", m.text);
}

#[cfg(test)]
mod tests {
    #[test]
    fn targeted_docs_open() {
        for f in super::targeted_docs() {
            let zip = crate::fixtures::build_zip(f.parts.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
            let d = ooxml_docx::Document::from_bytes(zip).unwrap_or_else(|e| panic!("{}: {e:?}", f.name));
            assert_eq!(d.stories().count(), 2, "{}: main + header", f.name);
        }
    }
}
