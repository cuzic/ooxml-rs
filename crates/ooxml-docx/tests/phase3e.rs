mod common;

use std::io::{Cursor, Write};

use ooxml_docx::{Document, Error, FieldSpec, ParaRef, RunPosition, RunRef, StoryIndex};
use zip::write::SimpleFileOptions;

const MAIN: &str = "word/document.xml";
const PLAIN: &str = "<w:p><w:r><w:t>Plain</w:t></w:r></w:p>";
const REFRESH: &str = "<w:updateFields w:val='true'/>";

fn end(index: usize) -> RunPosition {
    RunPosition::EndOf(ParaRef::new(StoryIndex::MAIN, index))
}

fn reference(name: &str) -> FieldSpec {
    FieldSpec::Ref {
        bookmark: name.into(),
        hyperlink: false,
    }
}

fn sequence(name: &str) -> FieldSpec {
    FieldSpec::Seq {
        identifier: name.into(),
    }
}

fn assert_refusal(
    input: &[u8],
    opt_in: bool,
    at: RunPosition,
    spec: FieldSpec,
    check: impl FnOnce(Error),
) {
    let mut doc = Document::from_bytes(input.to_vec()).unwrap();
    if opt_in {
        doc.enable_update_fields_on_open().unwrap();
    }
    let before = if opt_in {
        doc.to_bytes().unwrap()
    } else {
        input.to_vec()
    };
    check(doc.insert_field(at, spec).unwrap_err());
    assert_eq!(doc.to_bytes().unwrap(), before);
}

#[test]
fn refuses_unsafe_fields_without_mutating_the_package() {
    let plain = common::docx(PLAIN, Some(""), None);
    for picture in ["yyyy\"mm", "yyyy\\mm", "yyyy“mm"] {
        assert_refusal(
            &plain,
            false,
            end(0),
            FieldSpec::Date {
                format: picture.into(),
            },
            |error| {
                std::assert_matches!(
                    error,
                    Error::InvalidValue {
                        what: "field argument",
                        ..
                    }
                );
            },
        );
    }
    assert_refusal(
        &plain,
        false,
        end(0),
        FieldSpec::Date {
            format: "dddd".into(),
        },
        |error| {
            std::assert_matches!(error, Error::UnsupportedDateFormat { token } if token == "dddd");
        },
    );
    assert_refusal(&plain, true, end(0), reference("Missing"), |error| {
        std::assert_matches!(error, Error::UnknownBookmark { name } if name == "Missing");
    });
    assert_refusal(
        &plain,
        true,
        end(0),
        FieldSpec::PageRef {
            bookmark: "Missing".into(),
            hyperlink: false,
        },
        |error| {
            std::assert_matches!(error, Error::UnknownBookmark { name } if name == "Missing");
        },
    );
    assert_refusal(
        &plain,
        true,
        end(0),
        FieldSpec::Toc {
            levels: (3, 1),
            hyperlinks: false,
        },
        |error| {
            std::assert_matches!(
                error,
                Error::InvalidValue {
                    what: "TOC heading levels",
                    ..
                }
            );
        },
    );
    for name in ["Words", "Unknown Property"] {
        assert_refusal(
            &plain,
            true,
            end(0),
            FieldSpec::DocProperty { name: name.into() },
            |error| {
                std::assert_matches!(error, Error::StaleFieldRisk { codes } if codes == [format!("DOCPROPERTY {}", name.to_uppercase())]);
            },
        );
    }
    for settings in [None, Some("<w:updateFields w:val='false'/>"), Some(REFRESH)] {
        let input = common::docx(PLAIN, settings, None);
        for (spec, code) in [
            (reference("Missing"), "REF"),
            (
                FieldSpec::PageRef {
                    bookmark: "Missing".into(),
                    hyperlink: false,
                },
                "PAGEREF",
            ),
            (sequence("Figure"), "SEQ"),
            (
                FieldSpec::Toc {
                    levels: (1, 3),
                    hyperlinks: false,
                },
                "TOC",
            ),
        ] {
            assert_refusal(&input, false, end(0), spec, |error| {
                std::assert_matches!(error, Error::StaleFieldRisk { codes } if codes == [code]);
            });
        }
    }
    let tracking = common::docx(PLAIN, Some("<w:trackRevisions/>"), None);
    assert_refusal(&tracking, false, end(0), FieldSpec::Page, |error| {
        std::assert_matches!(error, Error::TrackRevisionsOn);
    });
    for edit in ["readOnly", "forms", "trackedChanges", "comments"] {
        let protected = common::docx(
            PLAIN,
            Some(&format!(
                "<w:documentProtection w:enforcement='true' w:edit='{edit}'/>"
            )),
            None,
        );
        assert_refusal(&protected, false, end(0), FieldSpec::Page, |error| {
            std::assert_matches!(error, Error::DocumentProtected { edit: actual } if actual == edit);
        });
    }
    let field = common::docx(
        "<w:p><w:r><w:fldChar w:fldCharType='begin'/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType='end'/></w:r></w:p>",
        None,
        None,
    );
    assert_refusal(
        &field,
        false,
        RunPosition::After(RunRef::new(ParaRef::new(StoryIndex::MAIN, 0), 1)),
        FieldSpec::Page,
        |error| {
            std::assert_matches!(error, Error::InsideField);
        },
    );
}

#[test]
fn field_insertion_uses_the_text_gate_and_rejects_a_non_pagination_override() {
    let input = common::docx(
        "<w:p><w:fldSimple w:instr=' SEQ Figure '><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p><w:p/>",
        None,
        None,
    );
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let error = doc
        .insert_field_with_page_cache(
            end(1),
            FieldSpec::Time {
                format: "HH:mm".into(),
            },
            3,
        )
        .unwrap_err();
    std::assert_matches!(
        error,
        Error::InvalidValue {
            what: "pagination field",
            ..
        }
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    doc.insert_field(end(1), FieldSpec::Page).unwrap();
    assert_eq!(doc.paragraphs().nth(1).unwrap().text(), "1");
}

#[test]
fn pagination_cache_copies_format_and_changes_only_the_target_paragraph() {
    let first = "<w:p><w:r><w:t>Unchanged before</w:t></w:r></w:p>";
    let last = "<w:p><w:r><w:t>Unchanged after</w:t></w:r></w:p>";
    let input = common::docx(
        &format!(
            "{first}<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Anchor</w:t></w:r><w:r><w:t>Suffix</w:t></w:r></w:p>{last}"
        ),
        Some("<w:documentProtection w:enforcement='true' w:edit='none'/>"),
        Some(PLAIN),
    );
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let at = RunPosition::After(RunRef::new(ParaRef::new(StoryIndex::MAIN, 1), 0));
    doc.insert_field_with_page_cache(at, FieldSpec::NumPages, 42)
        .unwrap();
    let out = doc.to_bytes().unwrap();
    let orig_entries = common::entries(&input);
    let out_entries = common::entries(&out);
    assert_eq!(orig_entries.len(), out_entries.len());
    for (original, output) in orig_entries.iter().zip(&out_entries) {
        if original.0 != MAIN {
            assert_eq!(original, output);
        }
    }
    let before = String::from_utf8(common::part(&input, MAIN)).unwrap();
    let after = String::from_utf8(common::part(&out, MAIN)).unwrap();
    let prefix = before.split_once(first).unwrap().0.len() + first.len();
    let suffix = before.split_once(last).unwrap().1.len() + last.len();
    assert!(after.starts_with(&before[..prefix]));
    assert!(after.ends_with(&before[before.len() - suffix..]));
    let xml = roxmltree::Document::parse(&after).unwrap();
    let paragraph = xml
        .descendants()
        .filter(|node| node.has_tag_name((common::W_NS, "p")))
        .nth(1)
        .unwrap();
    let runs: Vec<_> = paragraph
        .children()
        .filter(|node| node.has_tag_name((common::W_NS, "r")))
        .collect();
    assert_eq!(runs.len(), 7);
    let properties_xml = |run: roxmltree::Node<'_, '_>| {
        let properties = run.first_element_child().unwrap();
        assert!(properties.has_tag_name((common::W_NS, "rPr")));
        &after[properties.range()]
    };
    for run in &runs[1..6] {
        assert_eq!(properties_xml(*run), properties_xml(runs[0]));
    }
    assert_eq!(
        Document::from_bytes(out)
            .unwrap()
            .paragraphs()
            .nth(1)
            .unwrap()
            .text(),
        "Anchor42Suffix"
    );
}

#[test]
fn sequence_counts_only_earlier_matching_fields_in_the_story() {
    let seed = |name| {
        format!("<w:fldSimple w:instr=' SEQ {name} '><w:r><w:t>77</w:t></w:r></w:fldSimple>")
    };
    let body = format!(
        "<w:p>{}{}<w:r><w:t>Anchor</w:t></w:r>{}</w:p><w:p>{}</w:p>",
        seed("fIGure"),
        seed("Other"),
        seed("Figure"),
        seed("Figure")
    );
    let header = format!("<w:p>{}</w:p>", seed("Figure"));
    let input = common::docx(&body, Some(""), Some(&header));
    let mut doc = Document::from_bytes(input).unwrap();
    doc.enable_update_fields_on_open().unwrap();
    let anchor = doc
        .paragraphs()
        .next()
        .unwrap()
        .runs()
        .find(|run| run.text() == "Anchor")
        .unwrap()
        .run_ref();
    doc.insert_field(RunPosition::After(anchor), sequence("Figure"))
        .unwrap();
    doc.insert_field(end(0), sequence("Figure")).unwrap();
    let output = Document::from_bytes(doc.to_bytes().unwrap()).unwrap();
    assert_eq!(output.paragraphs().next().unwrap().text(), "7777Anchor2774");
}

#[test]
fn sequence_identifier_requires_an_ascii_letter_then_ascii_word_characters() {
    let input = common::docx(PLAIN, Some(""), None);
    for name in ["", "1Fig", "_Fig", "My Fig", "Fig-1", "図表"] {
        assert_refusal(&input, true, end(0), sequence(name), |error| {
            std::assert_matches!(error, Error::InvalidValue { what: "SEQ identifier", value } if value == name);
        });
    }
    let mut doc = Document::from_bytes(input).unwrap();
    doc.enable_update_fields_on_open().unwrap();
    doc.insert_field(end(0), sequence("Figure_12")).unwrap();
    assert_eq!(doc.paragraphs().next().unwrap().text(), "Plain1");
}

#[test]
fn ref_refuses_insertion_inside_the_bookmark_range() {
    let input = common::docx(
        concat!(
            "<w:p><w:r><w:t>Before</w:t></w:r><w:bookmarkStart w:id='1' w:name='Target'/>",
            "<w:r><w:t>First</w:t></w:r></w:p>",
            "<w:p><w:r><w:t>Last</w:t></w:r><w:bookmarkEnd w:id='1'/></w:p>",
        ),
        Some(REFRESH),
        None,
    );
    let after = |paragraph, run| {
        RunPosition::After(RunRef::new(ParaRef::new(StoryIndex::MAIN, paragraph), run))
    };
    for at in [after(0, 1), end(0), after(1, 0)] {
        assert_refusal(&input, true, at, reference("target"), |error| {
            std::assert_matches!(error, Error::InvalidValue { what: "self-referencing REF bookmark", value } if value == "target");
        });
    }
    let mut doc = Document::from_bytes(input).unwrap();
    doc.enable_update_fields_on_open().unwrap();
    doc.insert_field(after(0, 0), reference("Target")).unwrap();
    doc.insert_field(end(1), reference("Target")).unwrap();
    doc.insert_field(
        end(0),
        FieldSpec::PageRef {
            bookmark: "Target".into(),
            hyperlink: false,
        },
    )
    .unwrap();
}

#[test]
fn bookmark_result_uses_current_text_and_pageref_uses_a_placeholder() {
    let input = common::docx(
        concat!(
            "<w:p><w:r><w:t>Before</w:t></w:r><w:bookmarkStart w:id='04' w:name='Target'/>",
            "<w:r><w:t>One</w:t><w:tab/></w:r><w:del><w:r><w:delText>Deleted</w:delText></w:r></w:del>",
            "<w:ins><w:r><w:t>Two</w:t></w:r></w:ins></w:p>",
            "<w:p><w:r><w:t>Three</w:t></w:r><w:bookmarkEnd w:id='04'/><w:r><w:t>After</w:t></w:r></w:p>",
            "<w:p/><w:p/>"
        ),
        Some(""),
        None,
    );
    let mut doc = Document::from_bytes(input).unwrap();
    doc.enable_update_fields_on_open().unwrap();
    doc.insert_field(end(2), reference("target")).unwrap();
    doc.insert_field(
        end(3),
        FieldSpec::PageRef {
            bookmark: "Target".into(),
            hyperlink: true,
        },
    )
    .unwrap();
    assert_eq!(doc.paragraphs().nth(2).unwrap().text(), "One\tTwo\nThree");
    assert_eq!(doc.paragraphs().nth(3).unwrap().text(), "?");
}

fn with_properties(body: &str) -> Vec<u8> {
    let input = common::docx(PLAIN, Some(""), None);
    let rels = common::part(&input, "_rels/.rels");
    let rels = std::str::from_utf8(&rels).unwrap().replace(
        "</Relationships>",
        concat!(
            "<Relationship Id='rId2' Type='http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties' Target='metadata/core.xml'/>",
            "<Relationship Id='rId3' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties' Target='metadata/custom.xml'/>",
            "</Relationships>"
        ),
    );
    let input = common::with_part(&input, "_rels/.rels", rels.as_bytes());
    let mut archive = zip::ZipArchive::new(Cursor::new(input)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        zip.raw_copy_file(archive.by_index_raw(index).unwrap())
            .unwrap();
    }
    let custom = format!(
        "<Properties xmlns='http://schemas.openxmlformats.org/officeDocument/2006/custom-properties' xmlns:vt='http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes'>{body}</Properties>"
    );
    for (part, xml) in [
        (
            "metadata/core.xml",
            "<coreProperties xmlns='http://schemas.openxmlformats.org/package/2006/metadata/core-properties' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:dcterms='http://purl.org/dc/terms/'><dc:title>Core &amp; title</dc:title><dcterms:created>2024-01-01T00:00:00Z</dcterms:created><dcterms:modified>2024-02-01T00:00:00Z</dcterms:modified><lastPrinted>2024-03-01T00:00:00Z</lastPrinted></coreProperties>",
        ),
        ("metadata/custom.xml", &custom),
    ] {
        zip.start_file(part, SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn docproperty_reads_relationship_targets_and_respects_linked_shadowing() {
    let property = |name: &str| FieldSpec::DocProperty { name: name.into() };
    let input =
        with_properties("<property name='Client Name'><vt:lpwstr>A &amp; B</vt:lpwstr></property>");
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    doc.insert_field(end(0), property("Title")).unwrap();
    doc.insert_field(end(0), property("Client Name")).unwrap();
    let output = doc.to_bytes().unwrap();
    assert_eq!(
        Document::from_bytes(output.clone())
            .unwrap()
            .paragraphs()
            .next()
            .unwrap()
            .text(),
        "PlainCore & titleA & B"
    );
    for part in ["metadata/core.xml", "metadata/custom.xml", "_rels/.rels"] {
        assert_eq!(common::part(&input, part), common::part(&output, part));
    }
    let linked = with_properties(
        "<property name='Title' linkTarget='Bookmark'><vt:lpwstr>Linked</vt:lpwstr></property>",
    );
    assert_refusal(&linked, true, end(0), property("Title"), |error| {
        std::assert_matches!(error, Error::StaleFieldRisk { codes } if codes == ["DOCPROPERTY TITLE"]);
    });
}

#[test]
fn date_properties_have_an_empty_dirty_cache_without_opt_in() {
    let input = with_properties(concat!(
        "<property name='Deadline'><vt:filetime>2024-04-01T00:00:00Z</vt:filetime></property>",
        "<property name='Title'><vt:filetime>2024-05-01T00:00:00Z</vt:filetime></property>",
        "<property name='Author'><vt:lpwstr>2024-06-01T00:00:00Z</vt:lpwstr></property>",
    ));
    let mut doc = Document::from_bytes(input).unwrap();
    for name in [
        "CreateTime",
        "LastSavedTime",
        "LastPrinted",
        "Deadline",
        "Title",
        "Author",
    ] {
        doc.insert_field(end(0), FieldSpec::DocProperty { name: name.into() })
            .unwrap();
    }
    let output = doc.to_bytes().unwrap();
    let part = common::part(&output, MAIN);
    let xml = roxmltree::Document::parse(std::str::from_utf8(&part).unwrap()).unwrap();
    let markers: Vec<_> = xml
        .descendants()
        .filter(|node| node.has_tag_name((common::W_NS, "fldChar")))
        .map(|node| node.attribute((common::W_NS, "dirty")))
        .collect();
    assert_eq!(
        markers,
        [[Some("true"), None, None]; 5]
            .into_iter()
            .flatten()
            .chain([None; 3])
            .collect::<Vec<_>>()
    );
    let mut reopened = Document::from_bytes(output).unwrap();
    assert_eq!(
        reopened.paragraphs().next().unwrap().text(),
        "Plain2024-06-01T00:00:00Z"
    );
    reopened.insert_field(end(0), FieldSpec::Page).unwrap();
}
