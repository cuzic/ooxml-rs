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
    phase2_part3(out, &mut cases)?;
    let file = "30-insert-footnote-endnote.docx";
    let fixture = "poi-55733";
    let mut doc = Document::open(PathBuf::from(FIXTURES).join(format!("{fixture}.docx")))?;
    let at = ooxml_docx::RunPosition::EndOf(doc.paragraph(0).ok_or("missing anchor paragraph")?.para_ref());
    doc.add_footnote(at, vec!["D3a inserted footnote.".into()])?;
    doc.add_endnote(at, vec!["D3a inserted endnote.".into()])?;
    doc.save(out.join(file))?;
    Document::open(out.join(file))?;
    cases.push(Case {
        file,
        fixture,
        done: "Added a footnote and an endnote to the first paragraph".into(),
        check: "Follow both reference marks. The footnote reads D3a inserted footnote. The endnote reads D3a inserted endnote.",
    });
    let file = "31-comment-thread.docx";
    let mut doc = synthetic("<w:p><w:r><w:t>Review this phrase today.</w:t></w:r></w:p>")?;
    let run = doc.paragraph(0).and_then(|p| p.run(0)).ok_or("missing comment anchor")?.run_ref();
    let comment = |author: &str, text: &str| ooxml_docx::NewComment {
        author: author.into(),
        initials: None,
        date: None,
        paragraphs: vec![text.into()],
    };
    let id = doc.add_comment(
        ooxml_docx::TextRange::with_offsets(run, 7, run, 18),
        comment("Ada Lovelace", "D3b review comment."),
    )?;
    doc.reply(id, comment("Grace Hopper", "D3b review reply."))?;
    doc.set_resolved(id, true)?;
    save(&doc, out, file)?;
    cases.push(Case {
        file,
        fixture: "",
        done: "Commented on ‘this phrase’, added a reply, and resolved the thread".into(),
        check: "In the Comments pane, show resolved comments. Ada Lovelace’s comment and Grace Hopper’s reply belong to the same resolved thread on ‘this phrase’.",
    });
    let file = "32-comment-local-ignorable.docx";
    let mut doc = synthetic_with_part(
        "<w:p><w:commentRangeStart w:id='1'/><w:r><w:t>Existing anchor.</w:t></w:r><w:commentRangeEnd w:id='1'/><w:r><w:commentReference w:id='1'/></w:r></w:p><w:p><w:r><w:t>New anchor.</w:t></w:r></w:p>",
        Some(("comments", "<w:comment w:id='1' w:author='Existing'><w:p><w:r><w:t>Existing comment.</w:t></w:r></w:p></w:comment>")),
    )?;
    let run = doc.paragraph(1).and_then(|p| p.run(0)).ok_or("missing comment anchor")?.run_ref();
    doc.add_comment(
        ooxml_docx::TextRange::new(run, run),
        comment("Ada Lovelace", "D3b paragraph-local ignorable comment."),
    )?;
    save(&doc, out, file)?;
    cases.push(Case {
        file,
        fixture: "",
        done: "Added a comment to a package whose comments root has no ignorable w14 binding".into(),
        check: "Open the document and verify that both comments are visible on their respective anchors.",
    });
    tracked_cases(out, &mut cases)?;
    control_cases(out, &mut cases)?;
    field_cases(out, &mut cases)?;
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
    phase1_part2(out, &mut push)?;
    phase2_part1(out, &mut push)?;
    phase2_part2(out, &mut push)?;
    Ok(())
}

/// Word Phase1 part 2 items (ADR-0007 §4, §5, §6.3).
fn phase1_part2(out: &Path, push: &mut dyn FnMut(&'static str, &'static str, String, &'static str)) -> Result<(), Box<dyn std::error::Error>> {
    use ooxml_docx::{Alignment, Color, FirstLine, HalfPoints, Highlight, LineSpacing, ParaRef, ParagraphFormat, Prop, RunFormat, RunRef, StoryIndex, Underline, VertAlign};
    let p = |i| ParaRef::new(StoryIndex::MAIN, i);
    // 17: every run setter, one per line
    let lines = ["bold", "bold_cs", "italic", "italic_cs", "strike", "underline double", "color C00000", "size 32", "size_cs 32", "highlight yellow", "superscript", "font Courier New (all slots)"];
    let body: String = lines.iter().map(|l| format!("<w:p><w:r><w:t xml:space=\"preserve\">{l}: Sample עברית 日本語</w:t></w:r></w:p>")).collect();
    let mut doc = synthetic(&body)?;
    let fonts = |v: &str| ooxml_docx::FontSlots { ascii: Prop::Set(v.into()), h_ansi: Prop::Set(v.into()), east_asia: Prop::Set(v.into()), cs: Prop::Set(v.into()) };
    let formats = [
        RunFormat { bold: Prop::Set(true), ..RunFormat::default() },
        RunFormat { bold_cs: Prop::Set(true), ..RunFormat::default() },
        RunFormat { italic: Prop::Set(true), ..RunFormat::default() },
        RunFormat { italic_cs: Prop::Set(true), ..RunFormat::default() },
        RunFormat { strike: Prop::Set(true), ..RunFormat::default() },
        RunFormat { underline: Prop::Set(Underline::Double), ..RunFormat::default() },
        RunFormat { color: Prop::Set(Color::Rgb(0xC0, 0, 0)), ..RunFormat::default() },
        RunFormat { size: Prop::Set(HalfPoints::new(32).ok_or("size")?), ..RunFormat::default() },
        RunFormat { size_cs: Prop::Set(HalfPoints::new(32).ok_or("size")?), ..RunFormat::default() },
        RunFormat { highlight: Prop::Set(Highlight::Yellow), ..RunFormat::default() },
        RunFormat { vert_align: Prop::Set(VertAlign::Superscript), ..RunFormat::default() },
        RunFormat { fonts: fonts("Courier New"), ..RunFormat::default() },
    ];
    for (i, f) in formats.iter().enumerate() {
        doc.format_run(RunRef::new(p(i), 0), f)?;
    }
    let base = lines.len();
    let paras = ["alignment center", "space before 480 / after 480", "line spacing exact 480", "indent left 1440 / right 1440", "hanging 720", "keep with next", "keep lines together"];
    for (k, l) in paras.iter().enumerate() {
        doc.append_paragraph(&format!("{l}: The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog."))?;
        let pf = match k {
            0 => ParagraphFormat { alignment: Prop::Set(Alignment::Center), ..ParagraphFormat::default() },
            1 => ParagraphFormat { space_before: Prop::Set(480), space_after: Prop::Set(480), ..ParagraphFormat::default() },
            2 => ParagraphFormat { line_spacing: Prop::Set(LineSpacing::Exact(480)), ..ParagraphFormat::default() },
            3 => ParagraphFormat { indent_left: Prop::Set(1440), indent_right: Prop::Set(1440), ..ParagraphFormat::default() },
            4 => ParagraphFormat { first_line: Prop::Set(FirstLine::Hanging(720)), indent_left: Prop::Set(720), ..ParagraphFormat::default() },
            5 => ParagraphFormat { keep_next: Prop::Set(true), ..ParagraphFormat::default() },
            _ => ParagraphFormat { keep_lines: Prop::Set(true), ..ParagraphFormat::default() },
        };
        doc.format_paragraph(p(base + k), &pf)?;
    }
    save(&doc, out, "17-formatting-setters.docx")?;
    push(
        "17-formatting-setters.docx",
        "",
        "Synthetic: one line per run setter (Latin, Hebrew and Japanese text), then one paragraph per paragraph setter".into(),
        "Each line shows exactly what its label says (the _cs setters affect only the Hebrew text); the other lines are plain. Paragraph lines: centred, spacing, exact line height, indents, hanging indent, keep with next / keep lines (Paragraph dialog shows the boxes ticked).",
    );
    // 18: a style by id in a localized document (Swedish ids, English names)
    let styles = r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Rubrik1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>"#;
    let mut doc = synthetic_with_styles("<w:p><w:r><w:t>Heading set by style id Rubrik1</w:t></w:r></w:p><w:p><w:r><w:t>Body text</w:t></w:r></w:p>", Some(styles))?;
    let id = doc.style_ids_by_name("Heading 1").first().map(|s| (*s).to_owned()).ok_or("no heading 1")?;
    doc.set_paragraph_style(p(0), Some(&id))?;
    save(&doc, out, "18-localized-style-by-id.docx")?;
    push(
        "18-localized-style-by-id.docx",
        "",
        format!("Synthetic: style id {id:?} found by the built-in name \"heading 1\" and applied to the first paragraph"),
        "The first paragraph is shown as Heading 1 (bold, larger; the Styles pane highlights the heading style, named \"Rubrik 1\" in a Swedish UI); it appears in the Navigation pane.",
    );
    // 19: style-level firstLineChars plus a direct first_line (review m3)
    let styles = r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:pPr><w:ind w:firstLineChars="200"/></w:pPr></w:style>"#;
    let text = "The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog.";
    let mut doc = synthetic_with_styles(&format!("<w:p><w:r><w:t>Style indent (2 characters): {text}</w:t></w:r></w:p><w:p><w:r><w:t>Direct first line 1 inch: {text}</w:t></w:r></w:p>"), Some(styles))?;
    doc.format_paragraph(p(1), &ParagraphFormat { first_line: Prop::Set(FirstLine::Indent(1440)), ..ParagraphFormat::default() })?;
    save(&doc, out, "19-style-firstLineChars-and-first-line.docx")?;
    push(
        "19-style-firstLineChars-and-first-line.docx",
        "",
        "Synthetic: the default style indents the first line by 2 characters (firstLineChars); paragraph 2 gets first_line 1440 twips, written with firstLineChars=\"0\"".into(),
        "Paragraph 1's first line is indented by about two characters; paragraph 2's by one inch (not two characters).",
    );
    // 20: the updateFields opt-in on sdk-TOC2
    let mut doc = Document::open(PathBuf::from(FIXTURES).join("sdk-TOC2.docx"))?;
    doc.enable_update_fields_on_open()?;
    doc.append_paragraph("APPENDED after the opt-in")?;
    save_ungated(&doc, out, "20-update-fields-toc.docx")?;
    push(
        "20-update-fields-toc.docx",
        "sdk-TOC2",
        "enable_update_fields_on_open (settings w:updateFields=true), then a paragraph appended".into(),
        "Open question 5: note the prompt Word shows on open (text), whether the TOC page numbers and fields in headers/footers are updated, and after saving in Word whether settings.xml still has w:updateFields. Repeat in Word for Mac and Word Online if available.",
    );
    Ok(())
}

/// Word Phase2 part 1 items (ADR-0011 §4, §6, §9.1, §12 "Manual").
fn phase2_part1(out: &Path, push: &mut dyn FnMut(&'static str, &'static str, String, &'static str)) -> Result<(), Box<dyn std::error::Error>> {
    use ooxml_docx::{HyperlinkTarget, ParaRef, ParagraphSpec, Place, RunPosition, SectionBreak, StoryIndex};
    let p = |i| ParaRef::new(StoryIndex::MAIN, i);
    let para = |t: &str| format!("<w:p><w:r><w:t>{t}</w:t></w:r></w:p>");
    let body = |n: usize| (1..=n).map(|i| para(&format!("Paragraph {i}. The quick brown fox jumps over the lazy dog."))).collect::<String>();
    // 21/22: nextPage and continuous breaks in a landscape section with page numbering starting at 5
    for (file, kind, label) in [
        ("21-section-break-next-page.docx", SectionBreak::NextPage, "nextPage"),
        ("22-section-break-continuous.docx", SectionBreak::Continuous, "continuous"),
    ] {
        let sect = r#"<w:sectPr><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/><w:pgNumType w:start="5"/><w:cols w:num="2"/></w:sectPr>"#;
        let mut doc = synthetic_with_sect(&body(6), sect)?;
        doc.insert_section_break(p(2), kind)?;
        save(&doc, out, file)?;
        push(
            file,
            "",
            format!("Synthetic: a landscape two-column section with page numbers starting at 5; a {label} break after paragraph 3"),
            "Paragraphs 1–3 and 4–6 are separate sections, both landscape and two columns (Layout > Breaks shows the break kind). Page numbers start at 5 in the first section and continue (not restart) in the second; Word may show the restart only in the first section's Page Number Format dialog.",
        );
    }
    // 23: inserted paragraphs (list continuation, table cell) and an append that moves the last sectPr
    let sect_last = r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="2880" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:pPr><w:r><w:t>Last paragraph, holding the section properties (top margin 2 inches).</w:t></w:r></w:p>"#;
    let list = r#"<w:p><w:pPr><w:ind w:left="720" w:hanging="360"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Item one</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
    let mut doc = synthetic(&format!("{list}{sect_last}"))?;
    doc.insert_paragraph(Place::After(p(0)), "Item two (inserted, same pPr)", &ParagraphSpec::LikeNeighbour)?;
    doc.insert_paragraph(Place::After(p(2)), "Second paragraph in the cell (inserted)", &ParagraphSpec::Plain)?;
    doc.append_paragraph("APPENDED: still in the same section (2-inch top margin)")?;
    save(&doc, out, "23-insert-paragraphs-and-append-moving-sectPr.docx")?;
    push(
        "23-insert-paragraphs-and-append-moving-sectPr.docx",
        "",
        "Synthetic: a paragraph inserted after a list item with the same properties, one in a table cell, and an append after a paragraph holding the last section's sectPr (the sectPr moves to the new paragraph)".into(),
        "The inserted list item shows the same indentation as item one (no numbering definition exists in this file, so no number is shown); the cell has two paragraphs; the appended paragraph is on the same page, in the same section (no section break, 2-inch top margin unchanged).",
    );
    // 24: hyperlinks, external and to a bookmark, in a document whose root lacks xmlns:r
    let body = r#"<w:p><w:r><w:t xml:space="preserve">External link: </w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">Internal link: </w:t></w:r></w:p><w:p><w:bookmarkStart w:id="0" w:name="Target"/><w:r><w:t>Bookmark target</w:t></w:r><w:bookmarkEnd w:id="0"/></w:p>"#;
    let mut doc = synthetic(body)?;
    doc.insert_hyperlink(RunPosition::EndOf(p(0)), "example.com", &HyperlinkTarget::External("https://example.com/".into()))?;
    doc.insert_hyperlink(RunPosition::EndOf(p(1)), "go to the bookmark", &HyperlinkTarget::Bookmark("Target".into()))?;
    save(&doc, out, "24-hyperlinks.docx")?;
    push(
        "24-hyperlinks.docx",
        "",
        "Synthetic (root without xmlns:r): an external hyperlink and one to a bookmark".into(),
        "Ctrl+click opens https://example.com/ and jumps to \"Bookmark target\" respectively; no repair dialog (the first hyperlink declares xmlns:r itself).",
    );
    Ok(())
}

/// Word Phase2 part 2 items (ADR-0011 §3, §7, §12 "Manual").
fn phase2_part2(out: &Path, push: &mut dyn FnMut(&'static str, &'static str, String, &'static str)) -> Result<(), Box<dyn std::error::Error>> {
    use ooxml_docx::{ImageSize, ParaRef, RowTemplate, RunPosition, StoryIndex, StoryKind, TableRef};
    // a 4×2 PNG (red pixels are not needed: Word shows the frame and the size)
    let png = {
        // a real 1×1 red PNG, scaled to 2 × 1 cm
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0\x90wS\xde".to_vec();
        b.extend(b"\0\0\0\x0cIDATx\x9cc\xf8\xcf\xc0\0\0\x03\x01\x01\0\xc9\xfe\x92\xef\0\0\0\0IEND\xaeB`\x82");
        b
    };
    // 25: an image in the body and in a header (header from a fixture)
    let mut doc = Document::open(PathBuf::from(FIXTURES).join("poi-PageSpecificHeadFoot.docx"))?;
    let size = ImageSize::Emu { cx: 720_000, cy: 360_000 };
    let mut done = Vec::new();
    for kind in [StoryKind::Main, StoryKind::Header] {
        let target = doc.stories().filter(|s| s.id().kind() == kind).flat_map(|s| s.paragraphs().map(|p| p.para_ref()).collect::<Vec<_>>()).collect::<Vec<_>>();
        for p in target {
            if doc.insert_image(RunPosition::EndOf(p), &png, size, "red rectangle").is_ok() {
                done.push(format!("{kind:?} paragraph {}", p.index()));
                break;
            }
        }
    }
    save(&doc, out, "25-image-body-and-header.docx")?;
    push(
        "25-image-body-and-header.docx",
        "poi-PageSpecificHeadFoot",
        format!("A 2 × 1 cm red image at the end of: {}", done.join(", ")),
        "The red rectangle shows in the body and in the header, 2 cm wide; Alt Text shows \"red rectangle\". The header image appears on every page that uses that header.",
    );
    // 26: rows and columns
    let tbl = |rows: usize, cols: usize| {
        let grid: String = (0..cols).map(|_| r#"<w:gridCol w:w="2000"/>"#).collect();
        let tr: String = (0..rows)
            .map(|r| {
                let cells: String = (0..cols).map(|c| format!(r#"<w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>r{r}c{c}</w:t></w:r></w:p></w:tc>"#)).collect();
                if r == 0 { format!("<w:tr><w:trPr><w:tblHeader/></w:trPr>{cells}</w:tr>") } else { format!("<w:tr>{cells}</w:tr>") }
            })
            .collect();
        format!(r#"<w:tbl><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4"/><w:left w:val="single" w:sz="4"/><w:bottom w:val="single" w:sz="4"/><w:right w:val="single" w:sz="4"/><w:insideH w:val="single" w:sz="4"/><w:insideV w:val="single" w:sz="4"/></w:tblBorders></w:tblPr><w:tblGrid>{grid}</w:tblGrid>{tr}</w:tbl><w:p/>"#)
    };
    let mut doc = synthetic(&tbl(3, 3))?;
    let t = TableRef::new(StoryIndex::MAIN, 0);
    doc.insert_rows(t, 1, 2, RowTemplate::Above)?;
    doc.insert_column(t, 3, 1500)?;
    save(&doc, out, "26-table-rows-and-column.docx")?;
    push(
        "26-table-rows-and-column.docx",
        "",
        "Synthetic: a 3 × 3 bordered table with a repeating header row; two empty rows inserted after the header (template: the header, without tblHeader), a 1500-twip column appended".into(),
        "The table has 5 rows and 4 columns, all bordered; only the first row repeats as a header (Table Properties > Row); the new cells are empty and editable.",
    );
    // 27: merges
    let mut doc = synthetic(&tbl(4, 4).replace("r1c2</w:t>", "</w:t>").replace("r1c3</w:t>", "</w:t>").replace("r2c2</w:t>", "</w:t>").replace("r2c3</w:t>", "</w:t>").replace("<w:r><w:t></w:t></w:r>", ""))?;
    doc.merge_cells(TableRef::new(StoryIndex::MAIN, 0), 1..3, 2..4)?;
    save(&doc, out, "27-merge-cells.docx")?;
    push(
        "27-merge-cells.docx",
        "",
        "Synthetic: a 4 × 4 table; the 2 × 2 block rows 2–3, columns 3–4 merged (gridSpan + vMerge)".into(),
        "The block shows as one cell spanning two rows and two columns (Table Design: Select > Cell shows it as one cell); the other cells are unchanged.",
    );
    let _ = ParaRef::new(StoryIndex::MAIN, 0);
    Ok(())
}

/// Word Phase2 part 3 items (ADR-0011 §5, §5a, §8).
fn phase2_part3(out: &Path, cases: &mut Vec<Case>) -> Result<(), Box<dyn std::error::Error>> {
    use ooxml_docx::{BookmarkRange, ListTemplate, ParaRef, Prop, RunFormat, StoryIndex, StyleKind, StyleSpec};

    let p = |i| ParaRef::new(StoryIndex::MAIN, i);
    let whole = |i| BookmarkRange::Paragraphs { first: p(i), last: p(i) };
    let body = "<w:p><w:r><w:t>First item, bookmarked</w:t></w:r></w:p><w:p><w:r><w:t>Restart at five</w:t></w:r></w:p><w:p><w:r><w:t>Last line, unnumbered</w:t></w:r></w:p>";
    let mut doc = synthetic_with_styles(body, Some(""))?;
    doc.add_bookmark("Temporary", whole(2))?;
    doc.remove_bookmark("Temporary")?;
    let matched = doc.find("bookmarked", &ReplaceOptions::default())?.remove(0);
    doc.add_bookmark("ManualBookmark", BookmarkRange::Match(&matched))?;
    let id = doc.create_list(ListTemplate::Decimal)?;
    for i in 0..3 {
        doc.apply_list(p(i), id, 0)?;
    }
    doc.restart_list(p(1), 5)?;
    doc.remove_from_list(p(2))?;
    let style = doc.add_style(&StyleSpec {
        kind: StyleKind::Paragraph,
        name: "Manual Custom",
        id: None,
        based_on: None,
        next: None,
        run: RunFormat { bold: Prop::Set(true), ..RunFormat::default() },
        paragraph: Default::default(),
    })?;
    doc.set_paragraph_style(p(2), Some(&style))?;
    let file = "29-bookmark-list-style.docx";
    save(&doc, out, file)?;
    cases.push(Case {
        file,
        fixture: "",
        done: "Added and removed a temporary bookmark, bookmarked part of a run, created a decimal list, restarted it at 5, removed the last paragraph from the list, and styled that paragraph.".into(),
        check: "Go To > Bookmark lists ManualBookmark and selects only “bookmarked”. The lines are numbered 1 and 5, then unnumbered and bold with Manual Custom selected.",
    });
    Ok(())
}

/// [`synthetic`] with a body-level `w:sectPr`.
fn synthetic_with_sect(body: &str, sect: &str) -> Result<Document, Box<dyn std::error::Error>> {
    synthetic(&format!("{body}{sect}"))
}

/// [`save`] without the reopen-through-the-gates check (the opt-in is per
fn tracked_cases(out: &Path, cases: &mut Vec<Case>) -> Result<(), Box<dyn std::error::Error>> {
    use ooxml_docx::{Author, LocalDateTime, ParaRef, ParagraphSpec, Place, Prop, RunFormat, RunRef, StoryIndex};

    let date = LocalDateTime { wall_clock: "2026-11-19T12:00:00".parse()?, utc_offset_minutes: 540 };
    let author = || Author::new("Ada Lovelace");
    let p = |i| ParaRef::new(StoryIndex::MAIN, i);
    let mut doc = synthetic(concat!(
        "<w:p><w:r><w:rPr><w:i/></w:rPr><w:t>Original text.</w:t></w:r></w:p>",
        "<w:p><w:r><w:t>Format this text.</w:t></w:r></w:p>",
    ))?;
    {
        let mut tracked = doc.track_changes(author(), Some(date))?;
        tracked.set_run_text(0, 0, "Tracked replacement.")?;
        tracked.set_run_properties(RunRef::new(p(1), 0), &RunFormat { bold: Prop::Set(true), ..RunFormat::default() })?;
    }
    let file = "33-tracked-replacement-and-format.docx";
    save(&doc, out, file)?;
    cases.push(Case {
        file, fixture: "",
        done: "Recorded a replacement and a bold formatting change with a local date".into(),
        check: "In All Markup, verify the author and displayed time. Accept All shows Tracked replacement and bold text on the second line. Reject All restores Original text and removes the new bold formatting.",
    });

    let mut doc = synthetic_with_styles(
        concat!(
            "<w:p><w:pPr><w:pStyle w:val='Heading1'/></w:pPr><w:r><w:t>Heading before edit</w:t></w:r></w:p>",
            "<w:p><w:pPr><w:sectPr><w:type w:val='continuous'/></w:sectPr></w:pPr><w:r><w:t>Section before edit</w:t></w:r></w:p>",
        ),
        Some(concat!(
            "<w:style w:type='paragraph' w:default='1' w:styleId='Normal'><w:name w:val='Normal'/></w:style>",
            "<w:style w:type='paragraph' w:styleId='Heading1'><w:name w:val='heading 1'/>",
            "<w:basedOn w:val='Normal'/><w:next w:val='Normal'/><w:pPr><w:outlineLvl w:val='0'/></w:pPr></w:style>",
        )),
    )?;
    {
        let mut tracked = doc.track_changes(author(), Some(date))?;
        tracked.insert_paragraph(Place::After(p(0)), "Tracked after heading", &ParagraphSpec::Plain)?;
        tracked.append_paragraph("Tracked after section")?;
    }
    let file = "34-tracked-paragraph-marks.docx";
    save(&doc, out, file)?;
    cases.push(Case {
        file, fixture: "",
        done: "Inserted text and paragraph marks after a heading and before a section boundary".into(),
        check: "Reject All should retain the original heading style and continuous section boundary. Accept All should put the section boundary at the end of Tracked after section.",
    });

    let mut doc = synthetic(concat!(
        "<w:tbl><w:tblGrid><w:gridCol w:w='2500'/></w:tblGrid>",
        "<w:tr><w:tc><w:p><w:r><w:t>Remove this row.</w:t></w:r></w:p></w:tc></w:tr>",
        "<w:tr><w:tc><w:p><w:r><w:t>Keep this row.</w:t></w:r></w:p></w:tc></w:tr>",
        "</w:tbl><w:p/>",
    ))?;
    {
        use ooxml_docx::{RowTemplate, TableRef};
        let mut tracked = doc.track_changes(author(), Some(date))?;
        let table = TableRef::new(StoryIndex::MAIN, 0);
        tracked.insert_rows(table, 2, 1, RowTemplate::Above)?;
        tracked.delete_rows(table, 0..1)?;
    }
    let file = "35-tracked-table-rows.docx";
    save(&doc, out, file)?;
    cases.push(Case {
        file, fixture: "",
        done: "Marked the first row and its text deleted, and inserted an empty last row".into(),
        check: "Accept All keeps the second row and the new empty row. Reject All restores only the two original rows, including Remove this row.",
    });
    let mut doc = synthetic(concat!(
        "<w:p><w:pPr><w:sectPr><w:type w:val='continuous'/></w:sectPr></w:pPr>",
        "<w:r><w:t>First section.</w:t></w:r></w:p>",
        "<w:p><w:r><w:t>Second section.</w:t></w:r></w:p><w:sectPr/>",
    ))?;
    doc.track_changes(author(), Some(date))?.insert_paragraph(
        Place::After(p(0)),
        "New end of first section.",
        &ParagraphSpec::Plain,
    )?;
    let file = "36-tracked-mid-document-section.docx";
    save(&doc, out, file)?;
    cases.push(Case {
        file, fixture: "",
        done: "Inserted a tracked paragraph at the end of the first of two sections".into(),
        check: "Accept All should leave the continuous section break after New end of first section. Reject All should restore the break after First section. Second section should remain in both views.",
    });
    Ok(())
}

fn control_cases(out: &Path, cases: &mut Vec<Case>) -> Result<(), Box<dyn std::error::Error>> {
    use ooxml_docx::{ControlSpec, ParaRef, RunRef, StoryIndex, TextRange, WrapTarget};

    let control = |properties: &str| format!(
        "<w:p><w:sdt><w:sdtPr>{properties}</w:sdtPr><w:sdtContent><w:r><w:t>Before</w:t></w:r></w:sdtContent></w:sdt></w:p>"
    );
    let mut record = |doc: &Document, file, done: &str, check| -> Result<(), Box<dyn std::error::Error>> {
        save(doc, out, file)?;
        cases.push(Case { file, fixture: "", done: done.into(), check });
        Ok(())
    };

    let location = |doc: &Document| doc.content_controls().next().expect("control").location;
    let mut doc = synthetic(&control("<w:text/><w:showingPlcHdr/>"))?;
    doc.set_text(location(&doc), "Plain text edited")?;
    record(&doc, "37-sdt-plain.docx", "Replaced a plain-text placeholder", "The control shows Plain text edited.")?;

    let mut doc = synthetic(&control("<w:date><w:dateFormat w:val='MMMM d, yyyy'/><w:lid w:val='en-US'/></w:date>"))?;
    doc.set_date(location(&doc), "2026-11-21T00:00:00".parse()?)?;
    record(&doc, "38-sdt-date.docx", "Set a date using the stored picture", "The date picker shows November 21, 2026.")?;

    let mut doc = synthetic("<w:p><w:r><w:t>☐</w:t></w:r></w:p>")?;
    let first_run = RunRef::new(ParaRef::new(StoryIndex::MAIN, 0), 0);
    let target = WrapTarget::Inline(TextRange::new(first_run, first_run));
    doc.wrap_in_control(target, ControlSpec {
        kind: ooxml_docx::NewControlKind::Checkbox { checked: false },
        ..ControlSpec::default()
    })?;
    doc.set_checked(location(&doc), true)?;
    record(&doc, "39-sdt-checkbox.docx", "Created a checkbox and checked it", "The checkbox is checked and displays ☒.")?;

    let mut doc = synthetic(&control("<w:dropDownList><w:listItem w:value='blue' w:displayText='Ocean blue'/></w:dropDownList>"))?;
    doc.select(location(&doc), "blue")?;
    record(&doc, "40-sdt-dropdown.docx", "Selected the stored value blue", "The dropdown displays Ocean blue.")?;

    let binding = |prefix: &str| format!(
        r#"<w:dataBinding w:storeItemID="{{10000000-0000-0000-0000-000000000001}}" w:xpath="/{prefix}:record[1]/{prefix}:value" w:prefixMappings="xmlns:{prefix}='urn:values'"/><w:text/>"#
    );
    const PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let mut doc = synthetic_with_extra_parts(
        &(control(&binding("a")) + &control(&binding("b"))),
        None,
        [
            ("word/_rels/document.xml.rels", format!("<Relationships xmlns='{PKG}'><Relationship Id='custom' Type='{REL}/customXml' Target='../customXml/item1.xml'/></Relationships>")),
            ("customXml/item1.xml", "<record xmlns='urn:values'><value>Before</value></record>".into()),
            ("customXml/_rels/item1.xml.rels", format!("<Relationships xmlns='{PKG}'><Relationship Id='props' Type='{REL}/customXmlProps' Target='itemProps1.xml'/></Relationships>")),
            ("customXml/itemProps1.xml", "<ds:datastoreItem xmlns:ds='http://schemas.openxmlformats.org/officeDocument/2006/customXml' ds:itemID='{10000000-0000-0000-0000-000000000001}'/>".into()),
        ],
        &[("/customXml/itemProps1.xml", "application/vnd.openxmlformats-officedocument.customXmlProperties+xml")],
    )?;
    doc.set_text(location(&doc), "Shared value")?;
    record(&doc, "41-sdt-bound.docx", "Updated the custom XML element and both bound controls", "Both controls show Shared value before and after reopening.")?;

    let mut doc = synthetic("<w:p><w:r><w:t>Wrapped paragraph</w:t></w:r></w:p>")?;
    let p = ParaRef::new(StoryIndex::MAIN, 0);
    doc.wrap_in_control(WrapTarget::Paragraphs { first: p, last: p }, ControlSpec {
        alias: Some("Wrapped text".into()),
        ..ControlSpec::default()
    })?;
    record(&doc, "42-sdt-wrapped.docx", "Wrapped an existing paragraph in a rich-text control", "Wrapped paragraph is inside the Wrapped text control.")?;
    Ok(())
}

fn field_cases(out: &Path, cases: &mut Vec<Case>) -> Result<(), Box<dyn std::error::Error>> {
    use ooxml_docx::{FieldSpec, ParaRef, RunPosition, StoryIndex};

    const BOOKMARK: &str = "<w:p><w:bookmarkStart w:id='0' w:name='FieldTarget'/><w:r><w:t>Reference target</w:t></w:r><w:bookmarkEnd w:id='0'/></w:p>";
    const RELS: &str = concat!(
        "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>",
        "<Relationship Id='rId1' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument' Target='word/document.xml'/>",
        "<Relationship Id='rId2' Type='http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties' Target='docProps/core.xml'/>",
        "</Relationships>",
    );
    const CORE: &str = concat!(
        "<cp:coreProperties xmlns:cp='http://schemas.openxmlformats.org/package/2006/metadata/core-properties' ",
        "xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:dcterms='http://purl.org/dc/terms/' ",
        "xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance'>",
        "<dc:title>Field property value</dc:title>",
        "<dcterms:created xsi:type='dcterms:W3CDTF'>2003-11-22T13:14:15Z</dcterms:created>",
        "</cp:coreProperties>",
    );

    const CREATED: &str = "53-field-docproperty-createtime.docx";
    const SEQ_NUMBER_SYMBOLS: &str = "54-field-seq-number-symbols.docx";
    let variants = [
        ("43-field-page.docx", "Page: ", FieldSpec::Page, false),
        ("44-field-numpages.docx", "Pages: ", FieldSpec::NumPages, false),
        ("45-field-sectionpages.docx", "Section pages: ", FieldSpec::SectionPages, false),
        ("46-field-date.docx", "", FieldSpec::Date { format: "(yyyy-MM-dd)".into() }, false),
        ("47-field-time.docx", "", FieldSpec::Time { format: "(HH:mm)".into() }, false),
        ("48-field-docproperty.docx", "", FieldSpec::DocProperty { name: "Title".into() }, false),
        ("49-field-ref.docx", "Reference: ", FieldSpec::Ref { bookmark: "FieldTarget".into(), hyperlink: true }, true),
        ("50-field-pageref.docx", "Reference page: ", FieldSpec::PageRef { bookmark: "FieldTarget".into(), hyperlink: false }, true),
        ("51-field-seq.docx", "Sequence: ", FieldSpec::Seq { identifier: "Figure".into() }, true),
        ("52-field-toc.docx", "", FieldSpec::Toc { levels: (1, 3), hyperlinks: true }, true),
        (CREATED, "Created: ", FieldSpec::DocProperty { name: "CreateTime".into() }, false),
        (SEQ_NUMBER_SYMBOLS, "Sequence with numeric symbols: ", FieldSpec::Seq { identifier: "Fig②½²Ⅻ".into() }, true),
    ];
    for (file, label, spec, opt_in) in variants {
        let paragraph = if label.is_empty() {
            "<w:p/>".into()
        } else {
            format!("<w:p><w:r><w:t xml:space='preserve'>{label}</w:t></w:r></w:p>")
        };
        let bookmark = matches!(spec, FieldSpec::Ref { .. } | FieldSpec::PageRef { .. });
        let body = if bookmark { format!("{BOOKMARK}{paragraph}") } else { paragraph };
        let mut doc = if matches!(spec, FieldSpec::DocProperty { .. }) {
            synthetic_with_extra_parts(
                &body,
                None,
                [("_rels/.rels", RELS.into()), ("docProps/core.xml", CORE.into())],
                &[("/docProps/core.xml", "application/vnd.openxmlformats-package.core-properties+xml")],
            )?
        } else if opt_in {
            synthetic_with_part(&body, Some(("settings", "")))?
        } else {
            synthetic(&body)?
        };
        if opt_in {
            doc.enable_update_fields_on_open()?;
        }
        let at = RunPosition::EndOf(ParaRef::new(StoryIndex::MAIN, usize::from(bookmark)));
        let done = format!("Inserted the complex field {spec:?}");
        doc.insert_field(at, spec)?;
        save(&doc, out, file)?;
        cases.push(Case {
            file,
            fixture: "",
            done,
            check: if file == CREATED {
                concat!(
                    "On first open, record the Word version and locale, any update prompt, and what appears ",
                    "after Created (blank, a date for 22 November 2003, or another result). ",
                    "Use Alt+F9 to show field codes, put the cursor inside DOCPROPERTY CreateTime, ",
                    "and press F9 to update it. Return to the field result and record the new display.",
                )
            } else if file == SEQ_NUMBER_SYMBOLS {
                concat!(
                    "Record the Word version and the displayed result on open. Use Alt+F9 to verify ",
                    "the SEQ identifier is Fig②½²Ⅻ. Put the cursor inside the field and press F9, ",
                    "then return to the result and record whether Word displays 1 or another result.",
                )
            } else {
                "Verify the display, toggle field codes to inspect the instruction, then update the field."
            },
        });
    }
    Ok(())
}

/// `Document`, so a reopened copy is gated again).
fn save_ungated(doc: &Document, out: &Path, file: &str) -> Result<(), Box<dyn std::error::Error>> {
    doc.save(out.join(file))?;
    Ok(())
}

fn save(doc: &Document, out: &Path, file: &str) -> Result<(), Box<dyn std::error::Error>> {
    doc.save(out.join(file))?;
    Document::open(out.join(file))?;
    Ok(())
}

/// A minimal package with `body` as the body's inner XML.
fn synthetic(body: &str) -> Result<Document, Box<dyn std::error::Error>> {
    synthetic_with_part(body, None)
}

/// [`synthetic`] with `word/styles.xml` holding `styles` (inner XML of `w:styles`).
fn synthetic_with_styles(body: &str, styles: Option<&str>) -> Result<Document, Box<dyn std::error::Error>> {
    synthetic_with_part(body, styles.map(|xml| ("styles", xml)))
}

fn synthetic_with_part(body: &str, related: Option<(&str, &str)>) -> Result<Document, Box<dyn std::error::Error>> {
    synthetic_with_extra_parts(body, related, [], &[])
}

fn synthetic_with_extra_parts(
    body: &str,
    related: Option<(&str, &str)>,
    extra: impl IntoIterator<Item = (&'static str, String)>,
    overrides: &[(&str, &str)],
) -> Result<Document, Box<dyn std::error::Error>> {
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let mut parts = vec![
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
            {
                // a body that brings its own section properties gets no default one
                let sect = if body.contains("<w:sectPr") { "" } else { "<w:sectPr/>" };
                format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{body}{sect}</w:body></w:document>"#)
            },
        ),
    ];
    let related_name;
    if let Some((name, inner)) = related {
        related_name = format!("word/{name}.xml");
        parts[0].1 = parts[0].1.replace(
            "</Types>",
            &format!(r#"<Override PartName="/{related_name}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{name}+xml"/></Types>"#),
        );
        parts.push((
            "word/_rels/document.xml.rels",
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{name}" Target="{name}.xml"/></Relationships>"#),
        ));
        parts.push((&related_name, format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{name} xmlns:w="{W}">{inner}</w:{name}>"#)));
    }
    for (part, content_type) in overrides {
        parts[0].1 = parts[0].1.replace(
            "</Types>", &format!("<Override PartName='{part}' ContentType='{content_type}'/></Types>"),
        );
    }
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let extra: Vec<_> = extra.into_iter().collect();
    parts.retain(|(name, _)| !extra.iter().any(|(extra_name, _)| name == extra_name));
    for (name, content) in parts.into_iter().chain(extra) {
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
    writeln!(md, "# ooxml-docx: manual check in Microsoft 365 Word\n")?;
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
    writeln!(md, "5. **updateFields** (`20`, ADR-0007 open question 5): the prompt text; are TOC page numbers and header/footer fields updated; does `w:updateFields` survive Word's save; Word for Mac / Word Online.")?;
    std::fs::write(out.join("CHECKLIST.md"), md)?;
    for c in &cases {
        println!("{}  ({})", out.join(c.file).display(), if c.fixture.is_empty() { "synthetic" } else { c.fixture });
    }
    println!("{}", out.join("CHECKLIST.md").display());
    Ok(())
}
