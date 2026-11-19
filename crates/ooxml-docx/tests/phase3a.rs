mod common;

use common::{W_NS, document_xml, docx, package, part, story_xml, with_part};
use ooxml_docx::{Document, Error, NoteId, Provenance, RunPosition, StoryKind};
use std::assert_matches;

fn document(body: &str) -> String {
    format!(r#"<w:document xmlns:w="{W_NS}"><w:body>{body}</w:body></w:document>"#)
}

#[test]
fn note_pair_ids_settings_and_locality() {
    let body = "<w:p><w:pPr/><w:r><w:t>left</w:t></w:r><!-- preserved --><w:r><w:t>right</w:t></w:r></w:p><w:p />";
    let input = docx(
        body,
        Some(""),
        Some("<w:p><w:r><w:footnoteReference w:id=\"42\"/></w:r></w:p>"),
    );
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let p = doc.paragraph(0).unwrap();
    let before = p.run(0).unwrap().run_ref();
    let at = RunPosition::EndOf(p.para_ref());
    let id = doc
        .add_footnote(
            RunPosition::After(before),
            vec!["body".into(), "second".into()],
        )
        .unwrap();
    assert_eq!(id, NoteId(43));
    let first = doc.to_bytes().unwrap();
    let companion = String::from_utf8(part(&first, "word/endnotes.xml")).unwrap();
    assert!(companion.contains("continuationSeparator"));
    assert!(!companion.contains("endnoteRef"));
    let settings = String::from_utf8(part(&first, "word/settings.xml")).unwrap();
    assert!(settings.contains(
        "<w:footnotePr><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr>"
    ));
    assert!(
        settings
            .contains("<w:endnotePr><w:endnote w:id=\"-1\"/><w:endnote w:id=\"0\"/></w:endnotePr>")
    );
    let main = String::from_utf8(part(&first, "word/document.xml")).unwrap();
    let new_run = "<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteReference w:id=\"43\"/></w:r>";
    assert!(main.find("left").unwrap() < main.find(new_run).unwrap());
    assert!(main.find("right").unwrap() > main.find(new_run).unwrap());
    assert_eq!(
        main.replace(new_run, "").as_bytes(),
        part(&input, "word/document.xml")
    );
    assert_eq!(
        part(&first, "word/header1.xml"),
        part(&input, "word/header1.xml")
    );

    let id = doc.add_endnote(at, vec!["end".into()]).unwrap();
    assert_eq!(id, NoteId(1));
    let id = doc.add_footnote(at, vec!["more".into()]).unwrap();
    assert_eq!(id, NoteId(44));
    let bytes = doc.to_bytes().unwrap();
    assert!(
        String::from_utf8(part(&bytes, "word/footnotes.xml"))
            .unwrap()
            .contains("<w:footnoteRef/>")
    );
    let reopened = Document::from_bytes(bytes).unwrap();
    assert_eq!(reopened.stories().count(), 4);
}

#[test]
fn note_refusals_leave_package_byte_identical() {
    let body = "<w:p><w:r><w:t>Anchor</w:t></w:r></w:p>";
    for (input, expected) in [
        (docx(body, Some("<w:trackRevisions/>"), None), "track"),
        (
            docx(
                body,
                Some("<w:documentProtection w:edit=\"readOnly\" w:enforcement=\"1\"/>"),
                None,
            ),
            "protection",
        ),
        (
            docx(
                body,
                None,
                Some("<w:p><w:r><w:footnoteReference w:id=\"2147483647\"/></w:r></w:p>"),
            ),
            "overflow",
        ),
        (
            docx(
                body,
                None,
                Some("<w:p><w:r><w:footnoteReference w:id=\"bad\"/></w:r></w:p>"),
            ),
            "malformed",
        ),
        (docx(body, None, None), "empty"),
    ] {
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let at = RunPosition::EndOf(doc.paragraph(0).unwrap().para_ref());
        let paragraphs = if expected == "empty" {
            vec![]
        } else {
            vec!["body".into()]
        };
        let error = doc.add_footnote(at, paragraphs).unwrap_err();
        match (expected, error) {
            ("track", Error::TrackRevisionsOn)
            | ("protection", Error::DocumentProtected { .. })
            | ("overflow", Error::NoteIdsExhausted)
            | ("malformed", Error::InvalidNotePart)
            | ("empty", Error::InvalidValue { .. }) => {}
            (expected, err) => panic!("{expected}: {err:?}"),
        }
        assert_eq!(doc.to_bytes().unwrap(), input, "{expected}");
    }
}

#[test]
fn note_anchors_are_limited_to_the_document_body() {
    let p = "<w:p><w:r><w:t>Anchor</w:t></w:r></w:p>";
    let body = document(&format!("<w:tbl><w:tr><w:tc>{p}</w:tc></w:tr></w:tbl>"));
    let header = story_xml("hdr", p);
    let footer = story_xml("ftr", p);
    let footnotes = story_xml(
        "footnotes",
        &format!(r#"<w:footnote w:id="1">{p}</w:footnote>"#),
    );
    let endnotes = story_xml(
        "endnotes",
        &format!(r#"<w:endnote w:id="1">{p}</w:endnote>"#),
    );
    let comments = story_xml(
        "comments",
        &format!(r#"<w:comment w:id="1" w:author="A">{p}</w:comment>"#),
    );
    let input = package(
        &body,
        &[
            ("header", "header.xml", &header),
            ("footer", "footer.xml", &footer),
            ("footnotes", "footnotes.xml", &footnotes),
            ("endnotes", "endnotes.xml", &endnotes),
            ("comments", "comments.xml", &comments),
        ],
    );
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    for kind in [
        StoryKind::Header,
        StoryKind::Footer,
        StoryKind::Footnotes,
        StoryKind::Endnotes,
        StoryKind::Comments,
    ] {
        let para = doc
            .stories()
            .find(|story| story.id().kind() == kind)
            .unwrap()
            .paragraphs()
            .next()
            .unwrap()
            .para_ref();
        let at = RunPosition::EndOf(para);
        assert_matches!(
            doc.add_footnote(at, vec!["foot".into()]),
            Err(Error::UnsupportedContext { .. })
        );
        assert_eq!(doc.to_bytes().unwrap(), input, "{kind:?}");
        assert_matches!(
            doc.add_endnote(at, vec!["end".into()]),
            Err(Error::UnsupportedContext { .. })
        );
        assert_eq!(doc.to_bytes().unwrap(), input, "{kind:?}");
    }
    let at = RunPosition::EndOf(doc.paragraph(0).unwrap().para_ref());
    assert_eq!(
        doc.add_footnote(at, vec!["foot".into()]).unwrap(),
        NoteId(2)
    );
    assert_eq!(doc.add_endnote(at, vec!["end".into()]).unwrap(), NoteId(2));
    let output = doc.to_bytes().unwrap();
    let styles = String::from_utf8(part(&output, "word/styles.xml")).unwrap();
    for id in ["FootnoteReference", "EndnoteReference"] {
        let (_, style) = styles.split_once(&format!(r#"w:styleId="{id}""#)).unwrap();
        let (style, _) = style.split_once("</w:style>").unwrap();
        assert!(style.contains(r#"<w:rPr><w:vertAlign w:val="superscript"/></w:rPr>"#));
    }
    assert_eq!(
        styles
            .matches(r#"<w:rPr><w:vertAlign w:val="superscript"/></w:rPr>"#)
            .count(),
        2
    );
}

#[test]
fn note_registry_refusals_leave_package_byte_identical() {
    let body = document("<w:p/>");
    let notes = story_xml("footnotes", "");
    let settings = story_xml("settings", "");
    let wrong_root = story_xml("endnotes", "");
    let styles = story_xml(
        "styles",
        concat!(
            r#"<w:style w:type="character" w:styleId="A"><w:name w:val="footnote reference"/></w:style>"#,
            r#"<w:style w:type="character" w:styleId="B"><w:name w:val="footnote reference"/></w:style>"#,
        ),
    );
    let single = package(&body, &[("footnotes", "footnotes.xml", &notes)]);
    let dangling = String::from_utf8(part(&single, "word/_rels/document.xml.rels"))
        .unwrap()
        .replace("footnotes.xml", "missing.xml");
    let dangling = with_part(&single, "word/_rels/document.xml.rels", dangling.as_bytes());
    for (input, case) in [
        (
            package(
                &body,
                &[
                    ("footnotes", "a.xml", &notes),
                    ("footnotes", "b.xml", &notes),
                ],
            ),
            "duplicate",
        ),
        (dangling, "dangling"),
        (
            package(&body, &[("footnotes", "footnotes.xml", &wrong_root)]),
            "wrong root",
        ),
        (
            package(&body, &[("styles", "styles.xml", &styles)]),
            "style name",
        ),
        (
            package(
                &body,
                &[("styles", "a.xml", &styles), ("styles", "b.xml", &styles)],
            ),
            "styles part",
        ),
        (
            package(
                &body,
                &[
                    ("settings", "a.xml", &settings),
                    ("settings", "b.xml", &settings),
                ],
            ),
            "settings",
        ),
    ] {
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let at = RunPosition::EndOf(doc.paragraph(0).unwrap().para_ref());
        let result = doc.add_footnote(at, vec!["note".into()]);
        let Err(error) = result else {
            panic!("{case}: {result:?}")
        };
        match (case, error) {
            ("duplicate" | "dangling" | "wrong root", Error::InvalidNotePart)
            | ("style name", Error::AmbiguousStyle { .. })
            | ("styles part", Error::AmbiguousStylesPart)
            | ("settings", Error::AmbiguousSettings) => {}
            (case, error) => panic!("{case}: {error:?}"),
        }
        assert_eq!(doc.to_bytes().unwrap(), input, "{case}");
    }
}

#[test]
fn adding_notes_does_not_adopt_a_dangling_relationship_id() {
    let body = document_xml(r#"<w:p><w:hyperlink r:id="rId1"/></w:p>"#);
    let input = package(&body, &[]);
    let mut doc = Document::from_bytes(input).unwrap();
    let at = RunPosition::EndOf(doc.paragraph(0).unwrap().para_ref());
    doc.add_footnote(at, vec!["note".into()]).unwrap();
    let output = doc.to_bytes().unwrap();
    let rels = String::from_utf8(part(&output, "word/_rels/document.xml.rels")).unwrap();
    assert!(!rels.contains(r#"Id="rId1""#));
    let main = String::from_utf8(part(&output, "word/document.xml")).unwrap();
    assert!(main.contains(r#"<w:hyperlink r:id="rId1"/>"#));
}

#[test]
fn effective_on_off_properties_and_style_fallback() {
    let paragraph_flags = [
        "keepNext",
        "keepLines",
        "pageBreakBefore",
        "widowControl",
        "suppressLineNumbers",
        "suppressAutoHyphens",
        "kinsoku",
        "wordWrap",
        "overflowPunct",
        "topLinePunct",
        "autoSpaceDE",
        "autoSpaceDN",
        "bidi",
        "adjustRightInd",
        "snapToGrid",
        "contextualSpacing",
        "mirrorIndents",
        "suppressOverlap",
    ];
    let character_flags = [
        "noProof",
        "snapToGrid",
        "webHidden",
        "rtl",
        "cs",
        "specVanish",
        "oMath",
    ];
    let elements = |flags: &[&str]| {
        flags
            .iter()
            .map(|name| format!("<w:{name}/>"))
            .collect::<String>()
    };
    let ppr = elements(&paragraph_flags);
    let rpr = elements(&character_flags);
    let styles = story_xml(
        "styles",
        &format!(
            r#"
        <w:style w:type="paragraph" w:styleId="Normal" w:default="1"><w:pPr>{ppr}</w:pPr></w:style>
        <w:style w:type="character" w:styleId="Default" w:default="1"><w:rPr>{rpr}</w:rPr></w:style>
    "#
        ),
    );
    let body = document(
        r#"<w:p><w:pPr><w:pStyle w:val="MissingParagraph"/><w:contextualSpacing w:val="off"/></w:pPr>
        <w:r><w:rPr><w:rStyle w:val="MissingCharacter"/></w:rPr><w:t>Anchor</w:t></w:r></w:p>
        <w:p><w:pPr><w:pStyle w:val="MissingParagraph"/></w:pPr></w:p>"#,
    );
    let doc = Document::from_bytes(package(&body, &[("styles", "styles.xml", &styles)])).unwrap();
    let p = doc.paragraph(0).unwrap();
    let pp = p.effective().unwrap().properties;
    for name in paragraph_flags {
        let value = pp.value(name).unwrap();
        if name == "contextualSpacing" {
            assert_eq!(
                (value.value.as_str(), value.provenance),
                ("0", Provenance::Direct)
            );
        } else {
            assert_eq!(
                (value.value.as_str(), value.provenance),
                ("1", Provenance::ParagraphStyle),
                "{name}"
            );
        }
    }
    let inherited = doc.paragraph(1).unwrap().effective().unwrap();
    let spacing = inherited.properties.value("contextualSpacing").unwrap();
    assert_eq!(
        (spacing.value.as_str(), spacing.provenance),
        ("1", Provenance::ParagraphStyle)
    );
    let rp = p.run(0).unwrap().effective().unwrap().properties;
    for name in character_flags {
        let value = rp.value(name).unwrap();
        assert_eq!(
            (value.value.as_str(), value.provenance),
            ("1", Provenance::CharacterStyle),
            "{name}"
        );
    }
}

#[test]
fn paragraph_mark_omits_revision_and_container_properties() {
    let input = docx(
        r#"<w:p><w:pPr>
        <w:tabs w:val="ignored"><w:tab w:val="left" w:pos="720"/></w:tabs>
        <w:rPr><w:ins w:author="A"/><w:del w:author="A"/><w:moveFrom w:author="A"/>
        <w:moveTo w:author="A"/><w:rPrChange w:author="A"/><w:oMath/><w:noProof/></w:rPr>
    </w:pPr></w:p>"#,
        None,
        None,
    );
    let doc = Document::from_bytes(input).unwrap();
    let p = doc.paragraph(0).unwrap().effective().unwrap();
    assert!(p.properties.value("tabs").is_none());
    for name in ["ins", "del", "moveFrom", "moveTo", "rPrChange"] {
        assert!(p.mark.properties.attribute(name, "author").is_none());
    }
    assert!(p.mark.properties.value("oMath").is_none());
    assert_eq!(p.mark.properties.value("noProof").unwrap().value, "1");
}

#[test]
fn numbering_follows_style_links_and_stops_at_cycles() {
    let styles = story_xml(
        "styles",
        r#"
        <w:style w:type="numbering" w:styleId="L"><w:pPr><w:numPr><w:numId w:val="2"/></w:numPr></w:pPr></w:style>
        <w:style w:type="numbering" w:styleId="Loop"><w:pPr><w:numPr><w:numId w:val="3"/></w:numPr></w:pPr></w:style>
    "#,
    );
    let numbering = story_xml(
        "numbering",
        r#"
        <w:abstractNum w:abstractNumId="1"><w:numStyleLink w:val="L"/></w:abstractNum>
        <w:abstractNum w:abstractNumId="2"><w:lvl w:ilvl="1"><w:pPr><w:ind w:left="720"/></w:pPr></w:lvl></w:abstractNum>
        <w:abstractNum w:abstractNumId="3"><w:numStyleLink w:val="Loop"/></w:abstractNum>
        <w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>
        <w:num w:numId="2"><w:abstractNumId w:val="2"/></w:num>
        <w:num w:numId="3"><w:abstractNumId w:val="3"/></w:num>
    "#,
    );
    let body = document(
        r#"
        <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr></w:p>
        <w:p><w:pPr><w:numPr><w:numId w:val="3"/></w:numPr></w:pPr></w:p>
    "#,
    );
    let doc = Document::from_bytes(package(
        &body,
        &[
            ("styles", "styles.xml", &styles),
            ("numbering", "numbering.xml", &numbering),
        ],
    ))
    .unwrap();
    let p = doc.paragraph(0).unwrap().effective().unwrap();
    let indent = p.properties.attribute("ind", "left").unwrap();
    assert_eq!(
        (indent.value.as_str(), indent.provenance),
        ("720", Provenance::Numbering)
    );
    let p = doc.paragraph(1).unwrap().effective().unwrap();
    assert!(p.properties.attribute("ind", "left").is_none());
}

#[test]
fn effective_layer_order_and_toggle_parity() {
    let styles = format!(
        r#"<w:styles xmlns:w="{W_NS}">
      <w:docDefaults><w:rPrDefault><w:rPr><w:i/></w:rPr></w:rPrDefault></w:docDefaults>
      <w:style w:type="paragraph" w:styleId="Base"><w:rPr><w:b/></w:rPr></w:style>
      <w:style w:type="paragraph" w:styleId="P" w:default="1"><w:basedOn w:val="Base"/><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr><w:ind w:left="100"/></w:pPr><w:rPr><w:b/><w:i w:val="0"/></w:rPr></w:style>
      <w:style w:type="character" w:styleId="C"><w:rPr><w:b/><w:i w:val="0"/></w:rPr></w:style>
      <w:style w:type="table" w:styleId="T"><w:rPr><w:b w:val="0"/></w:rPr>
        <w:tblStylePr w:type="firstRow"><w:rPr><w:color w:val="112233"/></w:rPr></w:tblStylePr>
      </w:style></w:styles>"#
    );
    let numbering = format!(
        r#"<w:numbering xmlns:w="{W_NS}"><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:pPr><w:ind w:left="200"/></w:pPr><w:rPr><w:sz w:val="40"/></w:rPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#
    );
    let body = r#"<w:tbl><w:tblPr><w:tblStyle w:val="T"/><w:tblLook w:val="0620"/></w:tblPr>
    <w:tr><w:tc><w:p><w:pPr><w:pStyle w:val="P"/><w:rPr><w:sz w:val="24"/></w:rPr></w:pPr>
    <w:r><w:t>Inherited</w:t></w:r><w:r><w:rPr><w:rStyle w:val="C"/></w:rPr><w:t>Toggle</w:t></w:r>
    <w:r><w:rPr><w:rStyle w:val="C"/><w:b/><w:i w:val="0"/></w:rPr><w:t>Direct</w:t></w:r>
    </w:p></w:tc></w:tr></w:tbl>"#;
    let input = package(
        &document(body),
        &[
            ("styles", "styles.xml", &styles),
            ("numbering", "numbering.xml", &numbering),
        ],
    );
    let doc = Document::from_bytes(input.clone()).unwrap();
    let p = doc.paragraph(0).unwrap();
    let effective = p.effective().unwrap();
    let indent = effective.properties.attribute("ind", "left").unwrap();
    assert_eq!(
        (indent.value.as_str(), indent.provenance),
        ("200", Provenance::Numbering)
    );
    assert_eq!(effective.mark.size_half_points().unwrap().value, 24);
    let runs: Vec<_> = p.runs().map(|r| r.effective().unwrap()).collect();
    assert_eq!(
        runs[0].toggle("b").unwrap().provenance,
        Provenance::ParagraphStyle
    );
    assert_eq!(
        runs.iter()
            .map(|r| r.toggle("b").unwrap().value)
            .collect::<Vec<_>>(),
        [true, false, true]
    );
    assert_eq!(
        runs.iter()
            .map(|r| r.toggle("i").unwrap().value)
            .collect::<Vec<_>>(),
        [false, true, false]
    );
    assert_eq!(runs[0].properties.value("color").unwrap().value, "112233");
    assert_eq!(runs[0].size_half_points().unwrap().value, 20);
    assert_eq!(doc.to_bytes().unwrap(), input);
}

#[test]
fn effective_style_cycle_is_read_only() {
    let styles = format!(
        r#"<w:styles xmlns:w="{W_NS}"><w:style w:type="paragraph" w:styleId="A" w:default="1"><w:basedOn w:val="A"/></w:style></w:styles>"#
    );
    let input = package(&document("<w:p/>"), &[("styles", "styles.xml", &styles)]);
    let doc = Document::from_bytes(input.clone()).unwrap();
    std::assert_matches!(
        doc.paragraph(0).unwrap().effective(),
        Err(Error::StyleCycle)
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
}

/// Run against the fixture directory or one of the downloaded ADR-0015 corpora.
#[test]
#[ignore = "set DOCX_CORPUS to a directory of DOCX inputs"]
fn corpus_notes_and_formatting() {
    use ooxml_core::{Package, QName};
    use ooxml_docx::StoryKind;
    use std::{collections::BTreeMap, path::Path};

    fn walk(dir: &Path, paths: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(&path, paths);
            } else if path.extension().is_some_and(|ext| {
                ["docx", "docm", "dotx", "dotm"]
                    .iter()
                    .any(|suffix| ext.eq_ignore_ascii_case(suffix))
            }) {
                paths.push(path);
            }
        }
    }

    let mut paths = Vec::new();
    walk(
        Path::new(&std::env::var_os("DOCX_CORPUS").expect("DOCX_CORPUS")),
        &mut paths,
    );
    paths.sort();
    let (mut opened, mut edited, mut paragraphs, mut runs) = (0, 0, 0, 0);
    let (mut skipped, mut refused) = (BTreeMap::new(), BTreeMap::new());
    let mut format_refusals = 0;
    let mut reopen_failures = 0;
    for path in paths {
        let input = std::fs::read(&path).unwrap();
        let mut doc = match Document::from_bytes(input.clone()) {
            Ok(doc) => doc,
            Err(error) => {
                *skipped.entry(format!("{error}")).or_insert(0) += 1;
                continue;
            }
        };
        opened += 1;
        for story in doc.stories() {
            for p in story.paragraphs() {
                paragraphs += 1;
                format_refusals += usize::from(p.effective().is_err());
                for run in p.runs() {
                    runs += 1;
                    format_refusals += usize::from(run.effective().is_err());
                }
            }
        }
        let mut added = Vec::new();
        for kind in [StoryKind::Footnotes, StoryKind::Endnotes] {
            let anchors: Vec<_> = doc
                .paragraphs()
                .map(|p| RunPosition::EndOf(p.para_ref()))
                .collect();
            let before = doc.to_bytes().unwrap();
            let mut result: ooxml_docx::Result<NoteId> = Err(Error::NoSuchParagraph(0));
            for anchor in anchors {
                let body = vec!["Corpus check".into()];
                result = if kind == StoryKind::Footnotes {
                    doc.add_footnote(anchor, body)
                } else {
                    doc.add_endnote(anchor, body)
                };
                if !matches!(
                    &result,
                    Err(Error::InsideField
                        | Error::InsideRevision { .. }
                        | Error::UnsupportedContext { .. }
                        | Error::TrackedPropertyChange { .. })
                ) {
                    break;
                }
            }
            match result {
                Ok(id) => added.push((kind, id)),
                Err(error) => {
                    assert_eq!(doc.to_bytes().unwrap(), before, "{path:?}");
                    *refused.entry(format!("{error}")).or_insert(0) += 1;
                }
            }
        }
        if added.is_empty() {
            continue;
        }
        edited += usize::from(added.len() == 2);
        let output = doc.to_bytes().unwrap();
        let reopened = match Document::from_bytes(output.clone()) {
            Ok(doc) => doc,
            Err(error) => {
                reopen_failures += 1;
                eprintln!("reopen failed for {path:?}: {error}");
                continue;
            }
        };
        let mut package = Package::from_bytes(output).unwrap();
        for (kind, id) in added {
            let local = if kind == StoryKind::Footnotes {
                "footnote"
            } else {
                "endnote"
            };
            let story = reopened
                .stories()
                .find(|story| story.id().kind() == kind)
                .unwrap();
            let xml = package.xml_part(story.id().part()).unwrap();
            let matches = xml
                .root()
                .elements()
                .filter(|node| {
                    node.is(QName::new(W_NS, local))
                        && node
                            .attr(QName::new(W_NS, "id"))
                            .and_then(|id| id.parse().ok())
                            == Some(id.0)
                })
                .count();
            assert_eq!(matches, 1, "{path:?} ({local})");
        }
    }
    println!(
        "opened={opened} edited_both={edited} paragraphs={paragraphs} runs={runs} format_refusals={format_refusals}"
    );
    println!("skipped={skipped:?}");
    println!("refused={refused:?}");
    assert_eq!(reopen_failures, 0);
}

#[test]
fn after_run_applies_r1_to_r4_before_staging() {
    let field = r#"<w:fldSimple w:instr="TC"><w:r><w:t>result</w:t></w:r></w:fldSimple>"#;
    let revision = "<w:ins w:id=\"1\"><w:r><w:t>text</w:t></w:r></w:ins>";
    let wrapped = "<w:customXml><w:r><w:t>text</w:t></w:r></w:customXml>";
    let non_text = "<w:r><w:drawing/></w:r>";
    for (run, expected) in [
        (field, "field"),
        (revision, "revision"),
        (wrapped, "wrapper"),
        (non_text, "non-text"),
        (
            r#"<w:r><w:rPr><w:rPrChange w:id="1" w:author="A"/></w:rPr><w:t>text</w:t></w:r>"#,
            "tracked",
        ),
        (
            r#"<w:hyperlink><w:r><w:t>text</w:t></w:r></w:hyperlink>"#,
            "hyperlink",
        ),
    ] {
        let input = docx(&format!("<w:p>{run}</w:p>"), None, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let run = doc.paragraph(0).unwrap().run(0).unwrap().run_ref();
        let error = doc
            .add_endnote(RunPosition::After(run), vec!["text".into()])
            .unwrap_err();
        match (expected, error) {
            ("field", Error::InsideField)
            | ("revision", Error::InsideRevision { .. })
            | ("wrapper", Error::UnsupportedContext { .. })
            | ("non-text", Error::RunNotTextOnly { .. })
            | (
                "tracked",
                Error::TrackedPropertyChange {
                    element: "w:rPrChange",
                },
            )
            | ("hyperlink", Error::NestedHyperlink) => {}
            (expected, err) => panic!("{expected}: {err:?}"),
        }
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
}

#[test]
fn existing_note_styles_are_selected_by_name_and_dangling_styles_are_reserved() {
    let body = document(
        r#"<w:p><w:pPr><w:pStyle w:val="FootnoteText"/></w:pPr><w:r><w:t>Anchor</w:t></w:r></w:p>"#,
    );
    let styles = format!(
        r#"<w:styles xmlns:w="{W_NS}"><w:style w:type="character" w:styleId="a5"><w:name w:val="Footnote Reference"/></w:style></w:styles>"#
    );
    let input = package(&body, &[("styles", "styles.xml", &styles)]);
    let mut doc = Document::from_bytes(input).unwrap();
    let at = RunPosition::EndOf(doc.paragraph(0).unwrap().para_ref());
    doc.add_footnote(at, vec!["body".into()]).unwrap();
    let output = doc.to_bytes().unwrap();
    let footnotes = String::from_utf8(part(&output, "word/footnotes.xml")).unwrap();
    assert!(footnotes.contains("<w:pStyle w:val=\"FootnoteText1\"/>"));
    assert!(footnotes.contains("<w:rStyle w:val=\"a5\"/>"));
    let source = String::from_utf8(part(&output, "word/document.xml")).unwrap();
    assert!(source.contains("<w:rStyle w:val=\"a5\"/>"));
    let output_styles = String::from_utf8(part(&output, "word/styles.xml")).unwrap();
    let existing = styles
        .split_once('>')
        .unwrap()
        .1
        .strip_suffix("</w:styles>")
        .unwrap();
    assert!(output_styles.contains(existing));
}

#[test]
fn computed_theme_fonts_and_colors_have_provenance() {
    let input = document(
        r#"<w:p><w:r><w:rPr><w:rFonts w:asciiTheme="majorAscii" w:eastAsiaTheme="majorEastAsia" w:eastAsia="Fallback"/><w:color w:themeColor="accent1" w:themeTint="7F" w:val="111111"/></w:rPr><w:t>text</w:t></w:r></w:p>"#,
    );
    let settings = format!(
        r#"<w:settings xmlns:w="{W_NS}"><w:themeFontLang w:eastAsia="ja-JP"/><w:clrSchemeMapping w:accent1="accent2"/></w:settings>"#
    );
    let theme = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
      <a:themeElements>
        <a:clrScheme><a:accent1><a:srgbClr val="112233"/></a:accent1><a:accent2><a:srgbClr val="000000"/></a:accent2></a:clrScheme>
        <a:fontScheme><a:majorFont><a:latin typeface="Major"/><a:ea typeface=""/><a:font script="Jpan" typeface="Japanese"/></a:majorFont></a:fontScheme>
      </a:themeElements>
    </a:theme>"#;
    let doc = Document::from_bytes(package(
        &input,
        &[
            ("theme", "theme.xml", theme),
            ("settings", "settings.xml", &settings),
        ],
    ))
    .unwrap();
    let run = doc
        .paragraph(0)
        .unwrap()
        .run(0)
        .unwrap()
        .effective()
        .unwrap();
    let color = run.properties.value("color").unwrap();
    assert_eq!(
        (color.value.as_str(), color.provenance),
        ("808080", Provenance::Direct)
    );
    assert_eq!(
        run.properties.attribute("rFonts", "ascii").unwrap().value,
        "Major"
    );
    assert_eq!(
        run.properties
            .attribute("rFonts", "eastAsia")
            .unwrap()
            .value,
        "Japanese"
    );
    let doc = Document::from_bytes(package(&input, &[])).unwrap();
    let run = doc
        .paragraph(0)
        .unwrap()
        .run(0)
        .unwrap()
        .effective()
        .unwrap();
    assert_eq!(
        run.properties
            .attribute("rFonts", "eastAsia")
            .unwrap()
            .value,
        "Fallback"
    );
    assert_eq!(run.properties.value("color").unwrap().value, "111111");
}

#[test]
fn table_banding_uses_grid_columns_and_vertical_merge_start() {
    let styles = format!(
        r#"<w:styles xmlns:w="{W_NS}">
        <w:style w:type="table" w:styleId="T">
          <w:tblStylePr w:type="band1Vert"><w:rPr><w:color w:val="112233"/></w:rPr></w:tblStylePr>
          <w:tblStylePr w:type="band2Vert"><w:rPr><w:color w:val="445566"/></w:rPr></w:tblStylePr>
          <w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr></w:tblStylePr>
        </w:style></w:styles>"#
    );
    let body = r#"<w:tbl><w:tblPr><w:tblStyle w:val="T"/><w:tblLook w:val="0220"/></w:tblPr>
       <w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid>
       <w:tr>
         <w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>merged start</w:t></w:r></w:p></w:tc>
         <w:tc><w:p><w:r><w:t>third grid column</w:t></w:r></w:p></w:tc>
       </w:tr>
       <w:tr>
         <w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge/></w:tcPr><w:p><w:r><w:t>merged continuation</w:t></w:r></w:p></w:tc>
         <w:tc><w:p><w:pPr><w:cnfStyle w:val="100001000000"/></w:pPr><w:r><w:t>cached flags ignored</w:t></w:r></w:p></w:tc>
       </w:tr></w:tbl>"#;
    let doc = Document::from_bytes(package(
        &document(body),
        &[("styles", "styles.xml", &styles)],
    ))
    .unwrap();
    for (p, expected_bold) in doc.paragraphs().zip([true, true, true, false]) {
        let run = p.run(0).unwrap().effective().unwrap();
        assert_eq!(run.properties.value("color").unwrap().value, "112233");
        assert_eq!(
            run.toggle("b").unwrap().value,
            expected_bold,
            "{}",
            p.text()
        );
    }
}

#[test]
fn creating_a_missing_companion_keeps_existing_separator_ids() {
    let footnotes = format!(
        r#"<w:footnotes xmlns:w="{W_NS}">
      <w:footnote w:type="separator" w:id="0"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
      <w:footnote w:type="continuationSeparator" w:id="1"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>
    </w:footnotes>"#
    );
    let input = package(
        &document("<w:p><w:r><w:t>Anchor</w:t></w:r></w:p>"),
        &[("footnotes", "footnotes.xml", &footnotes)],
    );
    let mut doc = Document::from_bytes(input).unwrap();
    let at = RunPosition::EndOf(doc.paragraph(0).unwrap().para_ref());
    assert_eq!(doc.add_endnote(at, vec!["end".into()]).unwrap(), NoteId(1));
    let bytes = doc.to_bytes().unwrap();
    assert_eq!(part(&bytes, "word/footnotes.xml"), footnotes.as_bytes());
    let settings = String::from_utf8(part(&bytes, "word/settings.xml")).unwrap();
    assert!(
        settings.contains(
            "<w:footnotePr><w:footnote w:id=\"0\"/><w:footnote w:id=\"1\"/></w:footnotePr>"
        )
    );
    assert!(
        settings
            .contains("<w:endnotePr><w:endnote w:id=\"-1\"/><w:endnote w:id=\"0\"/></w:endnotePr>")
    );
}
