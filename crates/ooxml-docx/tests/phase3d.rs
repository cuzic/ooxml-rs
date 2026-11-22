mod common;

use std::{assert_matches, collections::BTreeMap, io::Write};

use common::{W_NS, docx, part};
use ooxml_docx::{
    Binding, ControlKind, ControlSpec, Document, Error, NewControlKind, ParaRef, RunRef,
    StoryIndex, TextRange, WrapTarget,
};

const MAIN: &str = "word/document.xml";
const STORE: &str = "customXml/record.xml";
const OLD: &str =
    "<v:root xmlns:v='urn:values'><v:value>old</v:value><v:value>keep</v:value></v:root>";
const ITEM: &str = "{10000000-0000-0000-0000-000000000001}";
const CORE: &str = "{6C3C8BC8-F283-45AE-878A-BAB7291924A1}";
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn sdt(properties: &str, content: &str) -> String {
    format!("<w:sdt><w:sdtPr>{properties}</w:sdtPr><w:sdtContent>{content}</w:sdtContent></w:sdt>")
}

fn para(content: &str) -> String {
    format!("<w:p>{content}</w:p>")
}

fn run(text: &str) -> String {
    format!("<w:r><w:t>{text}</w:t></w:r>")
}

fn bind(item: &str, path: &str, prefixes: &str) -> String {
    format!(
        r#"<w:dataBinding w:storeItemID="{item}" w:xpath="{path}" w:prefixMappings="{prefixes}"/>"#
    )
}

fn with_store(input: &[u8], name: &str, content: &str, core: bool) -> Vec<u8> {
    let mut parts: BTreeMap<_, _> = common::part_names(input)
        .into_iter()
        .map(|name| {
            let bytes = part(input, &name);
            (name, bytes)
        })
        .collect();
    let relationships = "http://schemas.openxmlformats.org/package/2006/relationships";
    let append =
        |parts: &mut BTreeMap<String, Vec<u8>>, file: &str, element: &str, before: &str| {
            let bytes = parts.get_mut(file).unwrap();
            *bytes = std::str::from_utf8(bytes)
                .unwrap()
                .replace(before, &format!("{element}{before}"))
                .into_bytes();
        };
    if core {
        append(
            &mut parts,
            "_rels/.rels",
            &format!(
                "<Relationship Id='core' Type='{relationships}/metadata/core-properties' Target='{name}'/>"
            ),
            "</Relationships>",
        );
    } else {
        append(
            &mut parts,
            "word/_rels/document.xml.rels",
            &format!("<Relationship Id='custom' Type='{REL}/customXml' Target='../{name}'/>"),
            "</Relationships>",
        );
        parts.insert("customXml/_rels/record.xml.rels".into(), format!(
            "<Relationships xmlns='{relationships}'><Relationship Id='props' Type='{REL}/customXmlProps' Target='props.xml'/></Relationships>"
        ).into_bytes());
        parts.insert("customXml/props.xml".into(), format!(
            "<ds:datastoreItem xmlns:ds='http://schemas.openxmlformats.org/officeDocument/2006/customXml' ds:itemID='{ITEM}'/>"
        ).into_bytes());
    }
    parts.insert(name.into(), content.as_bytes().to_vec());
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn bound_write_updates_peers_and_only_the_selected_store_leaf() {
    let first = bind(ITEM, "/a:root[1]/a:value", "xmlns:a='urn:values'");
    let second = bind(ITEM, "/b:root/b:value[1]", "xmlns:b='urn:values'");
    let unrelated = bind(ITEM, "/b:root/b:value[2]", "xmlns:b='urn:values'");
    let body = para(&sdt(&format!("<w:text/>{first}"), &run("old")));
    let header = para(&format!(
        "{}{}",
        sdt(&format!("<w:text/>{second}"), &run("old")),
        sdt(&format!("<w:text/>{unrelated}"), &run("keep")),
    ));
    let input = with_store(&docx(&body, None, Some(&header)), STORE, OLD, false);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let control = doc.content_controls().next().unwrap().location;
    doc.set_text(control, "new").unwrap();
    let output = doc.to_bytes().unwrap();
    assert_eq!(
        part(&output, STORE),
        OLD.replacen(">old<", ">new<", 1).as_bytes()
    );
    for name in common::part_names(&input) {
        let original = part(&input, &name);
        if [MAIN, "word/header1.xml", STORE].contains(&name.as_str()) {
            assert_eq!(
                part(&output, &name),
                String::from_utf8(original)
                    .unwrap()
                    .replacen(">old<", ">new<", 1)
                    .as_bytes(),
                "{name}"
            );
        } else {
            assert_eq!(part(&output, &name), original, "{name}");
        }
    }
}

#[test]
fn placeholders_use_the_control_properties_or_drop_the_placeholder_style() {
    let placeholder_run =
        "<w:r><w:rPr><w:rStyle w:val='PlaceholderText'/><w:i/></w:rPr><w:t>prompt</w:t></w:r>";
    for (rpr, expected, absent) in [("<w:rPr><w:b/></w:rPr>", "b", "i"), ("", "i", "b")] {
        let properties = format!("{rpr}<w:text w:multiLine='1'/><w:temporary/><w:showingPlcHdr/>");
        let input = docx(&para(&sdt(&properties, placeholder_run)), None, None);
        let mut doc = Document::from_bytes(input).unwrap();
        let control = doc.content_controls().next().unwrap().location;
        doc.set_text(control, "left\nright").unwrap();
        let output = String::from_utf8(part(&doc.to_bytes().unwrap(), MAIN)).unwrap();
        let tree = roxmltree::Document::parse(&output).unwrap();
        let content = tree
            .descendants()
            .find(|n| n.has_tag_name((W_NS, "sdtContent")))
            .unwrap();
        assert!(
            content
                .descendants()
                .any(|node| node.has_tag_name((W_NS, expected)))
        );
        for local in [absent, "rStyle"] {
            assert!(
                !content
                    .descendants()
                    .any(|node| node.has_tag_name((W_NS, local)))
            );
        }
        assert!(
            content
                .descendants()
                .any(|node| node.has_tag_name((W_NS, "br")))
        );
        assert!(!output.contains("showingPlcHdr"));
        assert!(output.contains("temporary"));
    }
}

#[test]
fn refusals_are_byte_identical() {
    let missing = bind(ITEM, "/root", "");
    for (properties, settings, expected) in [
        ("<w:picture/>", None, Error::UnsupportedControl),
        (
            "<w:text/><w:lock w:val='contentLocked'/>",
            None,
            Error::ControlLocked,
        ),
        (
            "<w:text/><w:lock w:val='sdtContentLocked'/>",
            None,
            Error::ControlLocked,
        ),
        (&missing, None, Error::UnsupportedBinding),
        (
            "<w:text/>",
            Some("<w:trackRevisions/>"),
            Error::TrackRevisionsOn,
        ),
        (
            "<w:text/>",
            Some("<w:documentProtection w:edit='readOnly' w:enforcement='1'/>"),
            Error::DocumentProtected {
                edit: "readOnly".into(),
            },
        ),
        (
            "<w:text/>",
            None,
            Error::InvalidValue {
                what: "",
                value: String::new(),
            },
        ),
    ] {
        let input = docx(&para(&sdt(properties, &run("old"))), settings, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let control = doc.content_controls().next().unwrap().location;
        let error = doc.set_text(control, "new\nline").unwrap_err();
        assert_eq!(
            std::mem::discriminant(&error),
            std::mem::discriminant(&expected),
            "{error:?}"
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
    let input = docx(
        &para(&sdt(
            "<w:date><w:dateFormat w:val='ddd'/></w:date>",
            &run("old"),
        )),
        None,
        None,
    );
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let control = doc.content_controls().next().unwrap().location;
    let date = "2026-11-21T06:07:08".parse().unwrap();
    assert_matches!(
        doc.set_date(control, date),
        Err(Error::UnsupportedDateFormat { .. })
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    doc.set_date_with_display(control, date, "Saturday")
        .unwrap();
    assert_eq!(doc.paragraph(0).unwrap().text(), "Saturday");
}

#[test]
fn forms_protection_and_sdt_locked_allow_value_edits_only() {
    let input = docx(
        &para(&sdt("<w:text/><w:lock w:val='sdtLocked'/>", &run("old"))),
        Some("<w:documentProtection w:edit='forms' w:enforcement='1'/>"),
        None,
    );
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let p = ParaRef::new(StoryIndex::MAIN, 0);
    assert_matches!(
        doc.wrap_in_control(
            WrapTarget::Paragraphs { first: p, last: p },
            ControlSpec::default()
        ),
        Err(Error::DocumentProtected { .. })
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    let control = doc.content_controls().next().unwrap().location;
    doc.set_text(control, "new").unwrap();
    assert_eq!(doc.paragraph(0).unwrap().text(), "new");
}

#[test]
fn core_properties_follow_package_relationship_and_gate_fields_in_other_stories() {
    const CORE_PART: &str = "metadata/properties.xml";
    let old = "<cp:coreProperties xmlns:cp='http://schemas.openxmlformats.org/package/2006/metadata/core-properties' xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:title>old</dc:title><cp:revision>7</cp:revision></cp:coreProperties>";
    let binding = bind(
        CORE,
        "/p:coreProperties[1]/v:title[1]",
        "xmlns:p='http://schemas.openxmlformats.org/package/2006/metadata/core-properties' xmlns:v='http://purl.org/dc/elements/1.1/'",
    );
    for instruction in ["TITLE", "DOCPROPERTY Title", "INFO Title"] {
        let header = para(&format!(
            "<w:fldSimple w:instr='{instruction}'>{}</w:fldSimple>",
            run("old")
        ));
        let input = with_store(
            &docx(&para(&sdt(&binding, &run("old"))), Some(""), Some(&header)),
            CORE_PART,
            old,
            true,
        );
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let control = doc.content_controls().next().unwrap().location;
        assert_matches!(
            doc.set_text(control, "new"),
            Err(Error::StaleFieldRisk { codes }) if codes == [instruction.to_ascii_uppercase()]
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
        doc.enable_update_fields_on_open().unwrap();
        doc.set_text(control, "new").unwrap();
        assert_eq!(
            part(&doc.to_bytes().unwrap(), CORE_PART),
            old.replace(">old<", ">new<").as_bytes()
        );
    }
}

#[test]
fn wraps_partial_inline_ranges_and_sibling_paragraphs() {
    let body = format!(
        "{}{}",
        para(&format!("{}\n{}", run("before αβ"), run("γδ after"))),
        para(&run("second"))
    );
    let mut doc = Document::from_bytes(docx(&body, None, None)).unwrap();
    let p = ParaRef::new(StoryIndex::MAIN, 0);
    let range = TextRange::with_offsets(RunRef::new(p, 0), 7, RunRef::new(p, 1), 4);
    let wrapped = doc
        .wrap_in_control(
            WrapTarget::Inline(range),
            ControlSpec {
                kind: NewControlKind::Checkbox { checked: false },
                ..ControlSpec::default()
            },
        )
        .unwrap();
    assert_ne!(wrapped.0, 0);
    assert_eq!(doc.paragraph(0).unwrap().text(), "before αβγδ after");
    let control = doc.content_controls().next().unwrap();
    assert_eq!(control.kind, ControlKind::Checkbox);
    doc.set_checked(control.location, true).unwrap();
    assert_eq!(doc.paragraph(0).unwrap().text(), "before ☒ after");
    let output = String::from_utf8(part(&doc.to_bytes().unwrap(), MAIN)).unwrap();
    let tree = roxmltree::Document::parse(&output).unwrap();
    let node = tree
        .descendants()
        .find(|n| n.has_tag_name((W_NS, "sdt")))
        .unwrap();
    assert_eq!(node.lookup_namespace_uri(Some("w14")), Some(W14));
    assert_eq!(
        node.attribute((
            "http://schemas.openxmlformats.org/markup-compatibility/2006",
            "Ignorable"
        )),
        Some("w14")
    );

    let mut doc = Document::from_bytes(docx(&body, None, None)).unwrap();
    let last = ParaRef::new(StoryIndex::MAIN, 1);
    doc.wrap_in_control(
        WrapTarget::Paragraphs { first: p, last },
        ControlSpec::default(),
    )
    .unwrap();
    assert_eq!(
        doc.paragraphs().map(|p| p.text()).collect::<Vec<_>>(),
        ["before αβγδ after", "second"]
    );
}

#[test]
fn wrapping_must_match_existing_binding() {
    let input = with_store(
        &docx(&para(&run("different")), None, None),
        STORE,
        OLD,
        false,
    );
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let p = ParaRef::new(StoryIndex::MAIN, 0);
    let r = RunRef::new(p, 0);
    let spec = ControlSpec {
        binding: Some(Binding {
            store_item_id: ITEM.into(),
            xpath: "/v:root/v:value".into(),
            prefix_mappings: "xmlns:v='urn:values'".into(),
        }),
        ..ControlSpec::default()
    };
    for target in [
        WrapTarget::Inline(TextRange::new(r, r)),
        WrapTarget::Paragraphs { first: p, last: p },
    ] {
        assert_matches!(
            doc.wrap_in_control(target, spec.clone()),
            Err(Error::BindingMismatch)
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
    doc.set_run_text_at(r, "old").unwrap();
    doc.wrap_in_control(WrapTarget::Inline(TextRange::new(r, r)), spec)
        .unwrap();
    assert_eq!(part(&doc.to_bytes().unwrap(), STORE), OLD.as_bytes());
}

/// Run with OOXML_SDK_DOCX_CORPUS set to a directory of SDK Word packages.
#[test]
#[ignore = "requires the upstream SDK Word corpus"]
fn sdk_content_controls_sweep() {
    let folder = std::env::var_os("OOXML_SDK_DOCX_CORPUS").expect("set OOXML_SDK_DOCX_CORPUS");
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("docx" | "docm" | "dotx" | "dotm")
            )
        })
        .collect();
    files.sort();
    assert!(!files.is_empty());
    let mut tried = BTreeMap::<ControlKind, [usize; 2]>::new();
    let mut skipped = 0;
    let mut refusals = BTreeMap::<_, usize>::new();
    let ids = |doc: &Document| {
        let mut counts = BTreeMap::new();
        for id in doc.content_controls().filter_map(|control| control.id) {
            *counts.entry(id.0).or_insert(0) += 1;
        }
        counts
    };
    for path in &files {
        let input = std::fs::read(path).unwrap();
        let Ok(mut doc) = Document::from_bytes(input.clone()) else {
            skipped += 1;
            continue;
        };
        let original_ids = ids(&doc);
        let controls: Vec<_> = doc
            .content_controls()
            .map(|c| {
                (
                    c.location,
                    c.kind,
                    c.list_items().next().map(|(value, _)| value.to_owned()),
                )
            })
            .collect();
        let mut before = doc.to_bytes().unwrap();
        for (control, kind, first_value) in controls {
            let result = match kind {
                ControlKind::PlainText | ControlKind::RichText => {
                    doc.set_text(control, "Corpus value")
                }
                ControlKind::Date => doc.set_date(control, "2026-11-21T12:34:56".parse().unwrap()),
                ControlKind::Checkbox => doc.set_checked(control, true),
                ControlKind::DropDown => {
                    doc.select(control, first_value.as_deref().unwrap_or("Corpus value"))
                }
                ControlKind::ComboBox => doc.select(control, "Corpus value"),
                _ => continue,
            };
            let after = doc
                .to_bytes()
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let counts = tried.entry(kind).or_default();
            match result {
                Ok(()) => {
                    counts[0] += 1;
                    let reopened = Document::from_bytes(after.clone())
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    assert_eq!(ids(&reopened), original_ids, "{}", path.display());
                }
                Err(error) => {
                    counts[1] += 1;
                    *refusals.entry(format!("{kind:?} {error}")).or_default() += 1;
                    assert!(after == before, "{}: {error}", path.display());
                }
            }
            before = after;
        }
    }
    println!(
        "{} packages, {skipped} refused to open. Success/refusal by kind: {tried:?}",
        files.len()
    );
    for (error, count) in refusals {
        println!("{count:4} {error}");
    }
    assert!(tried.values().any(|counts| counts[0] != 0));
}

#[test]
fn rich_control_preserves_hyperlinks_and_markers() {
    let original = sdt(
        "",
        &para(&format!(
            "<w:bookmarkStart w:id='7' w:name='anchor'/><w:hyperlink w:anchor='anchor'>{}</w:hyperlink><w:bookmarkEnd w:id='7'/>{}",
            run("first"),
            run("second"),
        )),
    );
    let input = docx(&original, None, None);
    let mut doc = Document::from_bytes(input).unwrap();
    let control = doc.content_controls().next().unwrap().location;
    doc.set_text(control, "updated").unwrap();
    assert_eq!(doc.paragraph(0).unwrap().text(), "updated");
    let output = String::from_utf8(part(&doc.to_bytes().unwrap(), MAIN)).unwrap();
    assert!(output.contains("<w:bookmarkStart w:id='7' w:name='anchor'/><w:hyperlink w:anchor='anchor'><w:r><w:t>updated</w:t></w:r></w:hyperlink><w:bookmarkEnd w:id='7'/>"));
}

#[test]
fn bound_dates_render_each_picture_and_write_the_selected_mapping() {
    let binding = bind(ITEM, "/v:root/v:value", "xmlns:v='urn:values'");
    for (mapping, expected) in [
        ("dateTime", "2026-11-21T06:07:08Z"),
        ("date", "2026-11-21"),
        ("text", "November 21"),
    ] {
        let primary = format!(
            "{binding}<w:date><w:dateFormat w:val='MMMM d'/><w:storeMappedDataAs w:val='{mapping}'/></w:date>"
        );
        let peer = format!("{binding}<w:date><w:dateFormat w:val='yyyy-MM-dd'/></w:date>");
        let body = format!(
            "{}{}{}",
            para(&sdt(&primary, &run("old"))),
            para(&sdt(&peer, &run("old"))),
            para(&sdt(&format!("{binding}<w:text/>"), &run("old"))),
        );
        let mut doc =
            Document::from_bytes(with_store(&docx(&body, None, None), STORE, OLD, false)).unwrap();
        let control = doc.content_controls().next().unwrap().location;
        doc.set_date(control, "2026-11-21T06:07:08".parse().unwrap())
            .unwrap();
        assert_eq!(
            doc.paragraphs().map(|p| p.text()).collect::<Vec<_>>(),
            ["November 21", "2026-11-21", expected],
        );
        assert_eq!(
            part(&doc.to_bytes().unwrap(), STORE),
            OLD.replacen("old", expected, 1).as_bytes()
        );
    }
}

#[test]
fn checkbox_uses_the_declared_glyph_and_font_over_theme_fonts() {
    let body = para(&sdt(
        &format!(
            "<c:checkbox xmlns:c='{W14}'><c:checked c:val='0'/><c:checkedState c:val='2605' c:font='DejaVu Sans'/></c:checkbox>"
        ),
        "<w:r><w:rPr><w:rFonts w:asciiTheme='majorHAnsi'/></w:rPr><w:t>old</w:t></w:r>",
    ));
    let mut doc = Document::from_bytes(docx(&body, None, None)).unwrap();
    let control = doc.content_controls().next().unwrap().location;
    doc.set_checked(control, true).unwrap();
    assert_eq!(doc.paragraph(0).unwrap().text(), "★");
    let output = String::from_utf8(part(&doc.to_bytes().unwrap(), MAIN)).unwrap();
    let tree = roxmltree::Document::parse(&output).unwrap();
    let font = tree
        .descendants()
        .find(|node| node.has_tag_name((W_NS, "rFonts")))
        .unwrap();
    assert_eq!(font.attribute((W_NS, "ascii")), Some("DejaVu Sans"));
    assert!(font.attribute((W_NS, "asciiTheme")).is_none());
}

#[test]
fn an_uneditable_bound_peer_refuses_the_whole_transaction() {
    let binding = bind(ITEM, "/v:root/v:value", "xmlns:v='urn:values'");
    let primary = para(&sdt(&format!("{binding}<w:text/>"), &run("old")));
    for (peer, expected) in [
        (format!("{binding}<w:text/><w:lock w:val='contentLocked'/>"), Error::ControlLocked),
        (binding.replace("<w:dataBinding ", "<v15:dataBinding xmlns:v15='http://schemas.microsoft.com/office/word/2012/wordml' "), Error::UnsupportedBinding),
        (binding.replace("<w:dataBinding ", "<v15:dataBinding xmlns:v15='http://schemas.microsoft.com/office/word/2010/11/wordml' "), Error::UnsupportedBinding),
    ] {
        let input = with_store(&docx(&primary, None, Some(&para(&sdt(&peer, &run("old"))))), STORE, OLD, false);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let control = doc.content_controls().next().unwrap().location;
        let error = doc.set_text(control, "new").unwrap_err();
        assert_eq!(std::mem::discriminant(&error), std::mem::discriminant(&expected), "{error:?}");
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
}

#[test]
fn non_gregorian_date_edits_require_a_display() {
    let date = "2026-11-21T06:07:08".parse().unwrap();
    for calendar in [
        "hijri", "hebrew", "taiwan", "japan", "thai", "korea", "saka", "none",
    ] {
        let properties = format!(
            "<w:date><w:dateFormat w:val='yyyy/MM/dd'/><w:lid w:val='en-US'/><w:calendar w:val='{calendar}'/></w:date>"
        );
        let input = docx(&para(&sdt(&properties, &run("old"))), None, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let control = doc.content_controls().next().unwrap().location;
        assert_matches!(
            doc.set_date(control, date),
            Err(Error::UnsupportedDateFormat { token }) if token == calendar
        );
        assert_eq!(doc.to_bytes().unwrap(), input, "{calendar}");
        doc.set_date_with_display(control, date, "formatted")
            .unwrap();
        assert_eq!(doc.paragraph(0).unwrap().text(), "formatted");
    }
}

#[test]
fn gregorian_date_pictures_accept_japanese_literals() {
    for calendar in [
        "gregorian",
        "gregorianUs",
        "gregorianMeFrench",
        "gregorianArabic",
        "gregorianXlitEnglish",
        "gregorianXlitFrench",
    ] {
        let properties = format!(
            "<w:date><w:dateFormat w:val='yyyy年M月d日'/><w:lid w:val='ja-JP'/><w:calendar w:val='{calendar}'/></w:date>"
        );
        let mut doc =
            Document::from_bytes(docx(&para(&sdt(&properties, &run("old"))), None, None)).unwrap();
        let control = doc.content_controls().next().unwrap().location;
        doc.set_date(control, "2026-03-02T06:07:08".parse().unwrap())
            .unwrap();
        assert_eq!(doc.paragraph(0).unwrap().text(), "2026年3月2日");
    }
}

#[test]
fn list_selection_updates_existing_last_value() {
    for (list, value, display) in [
        ("dropDownList", "k1", "One"),
        ("comboBox", "k1", "One"),
        ("comboBox", "free & text", "free & text"),
    ] {
        for last_value in [" w:lastValue='k2'", ""] {
            let properties = format!(
                "<w:{list}{last_value}><w:listItem w:value='k1' w:displayText='One'/><w:listItem w:value='k2' w:displayText='Two'/></w:{list}>"
            );
            let input = docx(&para(&sdt(&properties, &run("Two"))), None, None);
            let mut doc = Document::from_bytes(input).unwrap();
            let control = doc.content_controls().next().unwrap().location;
            doc.select(control, value).unwrap();
            assert_eq!(doc.paragraph(0).unwrap().text(), display);
            let output = String::from_utf8(part(&doc.to_bytes().unwrap(), MAIN)).unwrap();
            let tree = roxmltree::Document::parse(&output).unwrap();
            let node = tree
                .descendants()
                .find(|n| n.has_tag_name((W_NS, list)))
                .unwrap();
            assert_eq!(
                node.attribute((W_NS, "lastValue")),
                (!last_value.is_empty()).then_some(value),
            );
        }
    }
}

#[test]
fn inline_wrap_refuses_field_runs_and_enclosing_controls() {
    let begin = "<w:r><w:fldChar w:fldCharType='begin'/></w:r>";
    let instruction = "<w:r><w:instrText> DATE </w:instrText></w:r>";
    let separator = "<w:r><w:fldChar w:fldCharType='separate'/></w:r>";
    let end = "<w:r><w:fldChar w:fldCharType='end'/></w:r>";
    let body = para(&format!(
        "{begin}{instruction}{separator}{}{end}",
        run("result")
    ));
    let input = docx(&body, None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let paragraph = ParaRef::new(StoryIndex::MAIN, 0);
    for index in [1, 3] {
        let run = RunRef::new(paragraph, index);
        assert_matches!(
            doc.wrap_in_control(
                WrapTarget::Inline(TextRange::new(run, run)),
                ControlSpec::default()
            ),
            Err(Error::InsideField)
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
    }

    let input = docx(&para(&sdt("<w:text/>", &run("inside"))), None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let run = RunRef::new(paragraph, 0);
    assert_matches!(
        doc.wrap_in_control(
            WrapTarget::Inline(TextRange::new(run, run)),
            ControlSpec::default()
        ),
        Err(Error::UnsupportedContext { ancestor: "w:sdt" })
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
}

#[test]
fn block_wrap_refuses_fields_and_revisions() {
    let field = format!(
        "{}{}",
        para(
            "<w:r><w:fldChar w:fldCharType='begin'/></w:r><w:r><w:instrText> DATE </w:instrText></w:r><w:r><w:fldChar w:fldCharType='separate'/></w:r>"
        ),
        para(&format!(
            "{}<w:r><w:fldChar w:fldCharType='end'/></w:r>",
            run("result")
        )),
    );
    let move_from = format!(
        "<w:moveFromRangeStart w:id='1' w:name='moved' w:author='A'/>{}<w:moveFromRangeEnd w:id='1'/>",
        para(&run("moved")),
    );
    let revised_mark = para(&format!(
        "<w:pPr><w:rPr><w:ins w:id='1' w:author='A'/></w:rPr></w:pPr>{}",
        run("revised"),
    ));
    for (body, index, inside_field) in [
        (field, 1, true),
        (move_from, 0, false),
        (revised_mark, 0, false),
    ] {
        let input = docx(&body, None, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let paragraph = ParaRef::new(StoryIndex::MAIN, index);
        let result = doc.wrap_in_control(
            WrapTarget::Paragraphs {
                first: paragraph,
                last: paragraph,
            },
            ControlSpec::default(),
        );
        if inside_field {
            assert_matches!(result, Err(Error::InsideField));
        } else {
            assert_matches!(result, Err(Error::InsideRevision { .. }));
        }
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
}

#[test]
fn block_wrap_refuses_invalid_ranges() {
    let p = para(&run("text"));
    for (body, last_index) in [
        (sdt("", &p), 0),
        (
            para("<w:pPr><w:sectPr/></w:pPr><w:r><w:t>section</w:t></w:r>"),
            0,
        ),
        (
            format!("{p}<w:bookmarkStart w:id='1' w:name='gap'/><w:bookmarkEnd w:id='1'/>{p}"),
            1,
        ),
    ] {
        let input = docx(&body, None, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let first = ParaRef::new(StoryIndex::MAIN, 0);
        let last = ParaRef::new(StoryIndex::MAIN, last_index);
        assert_matches!(
            doc.wrap_in_control(
                WrapTarget::Paragraphs { first, last },
                ControlSpec::default()
            ),
            Err(Error::InvalidTextRange)
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
    }

    let input = docx(&p, None, Some(&p));
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let header = doc.stories().nth(1).unwrap().index();
    assert_matches!(
        doc.wrap_in_control(
            WrapTarget::Paragraphs {
                first: ParaRef::new(StoryIndex::MAIN, 0),
                last: ParaRef::new(header, 0),
            },
            ControlSpec::default()
        ),
        Err(Error::InvalidTextRange)
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
}

#[test]
fn control_text_refuses_wrapped_run_without_changes() {
    let ac = format!(
        r#"<mc:AlternateContent xmlns:mc="{}"><mc:Choice Requires="w">{}</mc:Choice></mc:AlternateContent>"#,
        ooxml_core::name::MC_NS,
        run("wrapped"),
    );
    let input = docx(&para(&sdt("<w:text/>", &ac)), None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let control = doc.content_controls().next().unwrap().location;
    assert_matches!(doc.set_text(control, "new"), Err(Error::UnsupportedControl));
    assert_eq!(doc.to_bytes().unwrap(), input);
}
