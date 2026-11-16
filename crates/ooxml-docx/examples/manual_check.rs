//! Produces a small named set of edited fixtures plus `CHECKLIST.md` for
//! the project owner's manual check in Microsoft 365 Word (ADR-0004 §7,
//! "Manual"). Nothing here runs or renders the documents.
//!
//! ```sh
//! cargo run -p ooxml-docx --example manual_check -- <outdir>
//! ```

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use ooxml_docx::{Document, Paragraph, ReplaceOptions, Run, RunContext, StoryKind};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/docx");

struct Case {
    file: &'static str,
    fixture: &'static str,
    /// What was done, filled in by the edit.
    done: String,
    check: &'static str,
}

type Pick = fn(&Paragraph<'_>, &Run<'_>) -> bool;

/// Replaces the text of the first settable run matching `pick`; returns a
/// description.
fn set_first(doc: &mut Document, pick: Pick, text: &str) -> String {
    let (p, r, old, ptext) = doc
        .paragraphs()
        .find_map(|p| {
            p.runs()
                .find(|r| r.check_settable().is_ok() && !r.text().trim().is_empty() && pick(&p, r))
                .map(|r| (p.index(), r.index(), r.text(), p.text()))
        })
        .expect("the fixture has a matching run");
    doc.set_run_text(p, r, text).expect("settable");
    format!("Paragraph {p} (\"{}\"), run {r}: \"{}\" → \"{}\"", short(&ptext), short(&old), text.escape_debug())
}

fn short(s: &str) -> String {
    let s: String = s.chars().take(60).collect();
    s.escape_debug().to_string()
}

fn has_child(r: &Run<'_>, local: &str) -> bool {
    r.raw().elements().any(|c| c.name().local() == local)
}

fn has_text_id(p: &Paragraph<'_>) -> bool {
    p.raw().attrs().iter().any(|a| a.name().local() == "textId")
}

fn run(out: &Path) -> Result<Vec<Case>, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(out)?;
    let mut cases = Vec::new();
    let mut make = |file: &'static str, fixture: &'static str, check: &'static str, f: &dyn Fn(&mut Document) -> String| -> Result<(), Box<dyn std::error::Error>> {
        let mut doc = Document::open(PathBuf::from(FIXTURES).join(format!("{fixture}.docx")))?;
        doc.write_gate()?;
        let done = f(&mut doc);
        doc.save(out.join(file))?;
        Document::open(out.join(file))?; // the output reopens through the same gates
        cases.push(Case { file, fixture, done, check });
        Ok(())
    };

    make(
        "01-table-cell-run.docx",
        "poi-table-alignment",
        "The cell shows the new text; table layout, alignment and the other cells are unchanged.",
        &|d| set_first(d, |_, _| true, "EDITED cell text"),
    )?;
    make(
        "02-hyperlink-run.docx",
        "poi-bug59058",
        "The link text is the new text and the link still works (Ctrl+click); its style is unchanged.",
        &|d| set_first(d, |_, r| r.context() == RunContext::Hyperlink, "EDITED link text"),
    )?;
    make(
        "03-run-with-lastRenderedPageBreak.docx",
        "poi-bug59058",
        "lastRenderedPageBreak effect: the edited run held a w:lastRenderedPageBreak, which Phase0 drops. \
         Check that pagination looks normal (no blank page, no missing page break) around the edited text.",
        &|d| set_first(d, |_, r| has_child(r, "lastRenderedPageBreak"), "EDITED (lastRenderedPageBreak dropped)"),
    )?;
    make(
        "04-paragraph-with-textId.docx",
        "poi-testComment",
        "textId question: the edited paragraph keeps its w14:textId although its text changed. \
         Check for no repair dialog, that the comment is still attached, and (optional) save in Word and \
         see whether Word rewrites w14:textId of that paragraph.",
        &|d| set_first(d, |p, _| has_text_id(p), "EDITED, w14:textId kept"),
    )?;
    make(
        "05-append-without-sectPr.docx",
        "poi-55733",
        "The body has no w:sectPr: the new paragraph is the last thing in the body. It shows after \"TEST\".",
        &|d| {
            d.append_paragraph("APPENDED at the end of a body without sectPr").unwrap();
            "Appended one paragraph".into()
        },
    )?;
    make(
        "06-append-multi-section.docx",
        "poi-Headers",
        "The new paragraph is at the end of the last section, with that section's header; the three sections and \
         their headers are unchanged.",
        &|d| {
            d.append_paragraph("APPENDED to the last section").unwrap();
            "Appended one paragraph".into()
        },
    )?;
    make(
        "07-whitespace-tab-break.docx",
        "poi-checkboxes",
        "Leading/trailing and double spaces are kept (xml:space=\"preserve\"), the tab is a tab, the line break is a \
         line break (not a new paragraph). The form fields still work.",
        &|d| {
            let s = set_first(d, |_, _| true, "  two leading spaces,  a double space,\ta tab and\na line break ");
            d.append_paragraph("Appended:\tTab\nBreak").unwrap();
            format!("{s}; appended \"Appended:\\tTab\\nBreak\"")
        },
    )?;
    make(
        "08-document-with-fields.docx",
        "sdk-Field-more-field",
        "Only the edited text changed. The DATE, AUTHOR and PAGE fields keep their results (Alt+F9 shows the codes \
         unchanged).",
        &|d| set_first(d, |_, _| true, "EDITED in a document with fields"),
    )?;
    make(
        "09-headers-footers-with-fields.docx",
        "poi-PageSpecificHeadFoot",
        "Headers/footers (first, even, default) and their PAGE fields are unchanged; content controls are intact.",
        &|d| {
            let s = set_first(d, |_, _| true, "EDITED body text");
            d.append_paragraph("APPENDED paragraph").unwrap();
            format!("{s}; appended one paragraph")
        },
    )?;
    make(
        "10-footnotes-endnotes.docx",
        "sdk-ftEdNote",
        "Footnotes and endnotes and their reference marks are unchanged.",
        &|d| set_first(d, |_, _| true, "EDITED text near notes"),
    )?;
    curly_quotes(out)?;
    cases.push(Case {
        file: CURLY_FILE,
        fixture: "",
        done: "Not edited: ooxml-docx refuses it (W2). Three DOCPROPERTY/INFO fields with straight and typographic quotes, each \
               with the cached result 0"
            .into(),
        check: "Typographic quotes question: select all (Ctrl+A) and update fields (F9). Note for each line whether the \
                field now shows the word count, an error (\"Error! Unknown document property name.\"), or stays 0. If the \
                lines with “Words” and “NumWords” show the word count, Word reads typographic quotes as quotes (ADR-0004 \
                rev 4, m9; ooxml-docx already treats them so).",
    });
    phase1(out, &mut cases)?;
    Ok(cases)
}

/// Word Phase1 items (ADR-0007 §9, "Manual").
fn phase1(out: &Path, cases: &mut Vec<Case>) -> Result<(), Box<dyn std::error::Error>> {
    let mut push = |file: &'static str, fixture: &'static str, done: String, check: &'static str| {
        cases.push(Case { file, fixture, done, check });
    };
    // 12: cross-run placeholder with a collapsed _GoBack bookmark inside
    let body = r#"<w:p><w:r><w:rPr><w:b/><w:color w:val="C00000"/></w:rPr><w:t xml:space="preserve">Dear {{</w:t></w:r><w:proofErr w:type="spellStart"/><w:r><w:rPr><w:b/><w:color w:val="C00000"/></w:rPr><w:t>cli</w:t></w:r><w:bookmarkStart w:id="0" w:name="_GoBack"/><w:bookmarkEnd w:id="0"/><w:r><w:rPr><w:b/><w:color w:val="C00000"/></w:rPr><w:t>ent}}</w:t></w:r><w:proofErr w:type="spellEnd"/><w:r><w:t>, thank you.</w:t></w:r></w:p>"#;
    let mut doc = synthetic(body)?;
    doc.replace("{{client}}", "Ada Lovelace", &ReplaceOptions::default())?;
    save(&doc, out, "12-cross-run-placeholder.docx")?;
    push(
        "12-cross-run-placeholder.docx",
        "",
        "Synthetic: `{{client}}` split over three runs with a `proofErr` and a collapsed `_GoBack` bookmark inside, replaced by \"Ada Lovelace\"".into(),
        "The line reads \"Dear Ada Lovelace, thank you.\" with \"Dear Ada Lovelace\" bold dark red. Shift+F5 (go back) still works.",
    );
    // 13: a header and a footer edit
    let mut doc = Document::open(PathBuf::from(FIXTURES).join("poi-PageSpecificHeadFoot.docx"))?;
    let mut done = Vec::new();
    for kind in [StoryKind::Header, StoryKind::Footer] {
        let target = doc.stories().filter(|s| s.id().kind() == kind).find_map(|s| {
            s.paragraphs().find_map(|p| {
                p.runs().find(|r| r.check_settable().is_ok() && !r.text().trim().is_empty()).map(|r| (r.run_ref(), r.text(), s.id().part().to_owned()))
            })
        });
        if let Some((r, old, part)) = target {
            doc.set_run_text_at(r, &format!("EDITED {kind:?}"))?;
            done.push(format!("{part}: \"{}\" → \"EDITED {kind:?}\"", short(&old)));
        }
    }
    save(&doc, out, "13-header-footer.docx")?;
    push(
        "13-header-footer.docx",
        "poi-PageSpecificHeadFoot",
        done.join("; "),
        "The edited header and footer text shows on every page whose section uses that part; the other headers/footers and their PAGE fields are unchanged.",
    );
    // 14: a footnote and a comment edit
    let mut done = Vec::new();
    for (file, fixture, kind) in [("14a-footnote.docx", "sdk-ftEdNote", StoryKind::Footnotes), ("14b-comment.docx", "poi-testComment", StoryKind::Comments)] {
        let mut doc = Document::open(PathBuf::from(FIXTURES).join(format!("{fixture}.docx")))?;
        let target = doc.stories().filter(|s| s.id().kind() == kind).find_map(|s| {
            s.paragraphs().find_map(|p| p.runs().find(|r| r.check_settable().is_ok() && !r.text().trim().is_empty()).map(|r| (r.run_ref(), r.text())))
        });
        let (r, old) = target.ok_or("a settable note/comment run")?;
        doc.set_run_text_at(r, &format!(" EDITED {kind:?} text"))?;
        save(&doc, out, file)?;
        done.push((file, fixture, format!("{kind:?}: \"{}\" → \" EDITED {kind:?} text\"", short(&old))));
    }
    for (file, fixture, d) in done {
        push(file, fixture, d, "The note (14a) or comment (14b) shows the new text after its mark; numbering, the reference mark in the body and the comment's anchor are unchanged.");
    }
    // 15: replace next to a non-breaking hyphen
    let body = r#"<w:p><w:r><w:t>{{code}}</w:t><w:noBreakHyphen/><w:t>2026</w:t></w:r></w:p>"#;
    let mut doc = synthetic(body)?;
    doc.replace("{{code}}", "ISO", &ReplaceOptions::default())?;
    save(&doc, out, "15-no-break-hyphen.docx")?;
    push(
        "15-no-break-hyphen.docx",
        "",
        "Synthetic: `{{code}}<w:noBreakHyphen/>2026` with `{{code}}` replaced by \"ISO\"".into(),
        "Shows \"ISO‑2026\" with a non-breaking hyphen: narrow the window or add text so that the line would wrap there; it must not break at the hyphen.",
    );
    // 16: unpreserved edge whitespace (open question 2)
    let body = r#"<w:p><w:r><w:t>[</w:t></w:r><w:r><w:t> unpreserved edges </w:t></w:r><w:r><w:t>]</w:t></w:r></w:p><w:p><w:r><w:t>[</w:t></w:r><w:r><w:t xml:space="preserve"> preserved edges </w:t></w:r><w:r><w:t>]</w:t></w:r></w:p><w:p><w:r><w:t>[</w:t></w:r><w:r><w:t> set_run_text </w:t></w:r><w:r><w:t>]</w:t></w:r></w:p>"#;
    let mut doc = synthetic(body)?;
    doc.set_run_text(2, 1, " set_run_text ")?;
    save(&doc, out, "16-unpreserved-edge-whitespace.docx")?;
    push(
        "16-unpreserved-edge-whitespace.docx",
        "",
        "Synthetic: line 1 `<w:t> unpreserved edges </w:t>` (not edited), line 2 the same with xml:space=\"preserve\", line 3 edited by set_run_text (which adds preserve)".into(),
        "Open question 2: does line 1 show spaces inside the brackets (\"[ unpreserved edges ]\") like line 2, or not (\"[unpreserved edges]\")? Line 3 must look like line 2.",
    );
    Ok(())
}

fn save(doc: &Document, out: &Path, file: &str) -> Result<(), Box<dyn std::error::Error>> {
    doc.save(out.join(file))?;
    Document::open(out.join(file))?;
    Ok(())
}

/// A minimal package with `body` as the body's inner XML.
fn synthetic(body: &str) -> Result<Document, Box<dyn std::error::Error>> {
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let parts = [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_owned(),
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_owned(),
        ),
        (
            "word/document.xml",
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{body}<w:sectPr/></w:body></w:document>"#),
        ),
    ];
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in parts {
        z.start_file(name, zip::write::SimpleFileOptions::default())?;
        z.write_all(content.as_bytes())?;
    }
    Ok(Document::from_bytes(z.finish()?.into_inner())?)
}

const CURLY_FILE: &str = "11-curly-quoted-docproperty.docx";

/// Writes a minimal package with `DOCPROPERTY "Words"`, `DOCPROPERTY “Words”`
/// and `INFO “NumWords”` (review round 3, m9), and checks that ooxml-docx
/// refuses it.
fn curly_quotes(out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let field = |label: &str, code: &str| {
        format!(
            r#"<w:p><w:r><w:t xml:space="preserve">{label}: </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> {code} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>0</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#
        )
    };
    let body = [
        "<w:p><w:r><w:t>One two three four five six seven eight nine ten.</w:t></w:r></w:p>".to_owned(),
        field("Straight quotes", "DOCPROPERTY \"Words\""),
        field("Typographic quotes", "DOCPROPERTY \u{201C}Words\u{201D}"),
        field("INFO, typographic quotes", "INFO \u{201C}NumWords\u{201D}"),
    ]
    .concat();
    let parts = [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_owned(),
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_owned(),
        ),
        (
            "word/document.xml",
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{body}<w:sectPr/></w:body></w:document>"#),
        ),
    ];
    let mut z = zip::ZipWriter::new(std::fs::File::create(out.join(CURLY_FILE))?);
    for (name, content) in parts {
        z.start_file(name, zip::write::SimpleFileOptions::default())?;
        z.write_all(content.as_bytes())?;
    }
    z.finish()?;
    let doc = Document::open(out.join(CURLY_FILE))?;
    assert!(doc.write_gate().is_err(), "W2 refuses statistics fields, typographic quotes included");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("usage: manual_check <outdir>")?);
    let cases = run(&out)?;
    let mut md = String::new();
    writeln!(md, "# ooxml-docx Phase0/Phase1: manual check in Microsoft 365 Word\n")?;
    writeln!(md, "Generated by `cargo run -p ooxml-docx --example manual_check -- {}`.\n", out.display())?;
    writeln!(md, "For every file:\n")?;
    writeln!(md, "- [ ] Opens with **no repair dialog** (\"Word found unreadable content…\").")?;
    writeln!(md, "- [ ] The edited or appended text is shown as described; nothing else changed (compare with the original fixture in `reference/fixtures/docx/`).")?;
    writeln!(md, "- [ ] Comments, fields, headers/footers, notes and formatting look as in the original.")?;
    writeln!(md, "- [ ] Optional: save in Word, reopen: still no dialog.\n")?;
    for c in &cases {
        writeln!(md, "## `{}`\n", c.file)?;
        if c.fixture.is_empty() {
            writeln!(md, "- Source: synthetic, written by this example")?;
        } else {
            writeln!(md, "- Source: `reference/fixtures/docx/{}.docx`", c.fixture)?;
        }
        writeln!(md, "- Edit: {}", c.done)?;
        writeln!(md, "- [ ] No repair dialog")?;
        writeln!(md, "- [ ] {}\n", c.check)?;
    }
    writeln!(md, "## Questions to answer\n")?;
    writeln!(md, "1. **w14:textId** (`04`): is keeping the old textId on an edited paragraph harmless (no dialog, comment intact)? If Word rewrites it on save, note that.")?;
    writeln!(md, "2. **lastRenderedPageBreak** (`03`): any visible effect of dropping it from the edited run?")?;
    writeln!(md, "3. **Typographic quotes** (`11`): after F9, do the fields written with “…” show the word count (Word reads them as quotes) or an error?")?;
    writeln!(md, "4. **Unpreserved edge whitespace** (`16`, ADR-0007 open question 2): does Word show the edge spaces of a `w:t` without `xml:space=\"preserve\"`? If yes, `UnpreservedWhitespace` can be lifted.")?;
    std::fs::write(out.join("CHECKLIST.md"), md)?;
    for c in &cases {
        println!("{}  ({})", out.join(c.file).display(), if c.fixture.is_empty() { "synthetic" } else { c.fixture });
    }
    println!("{}", out.join("CHECKLIST.md").display());
    Ok(())
}
