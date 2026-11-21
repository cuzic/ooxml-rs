//! Locality and atomic refusals for ADR-0016 §§9–10.

use std::assert_matches;
use std::path::Path;

use ooxml_core::package::RegistryRefusal;
use ooxml_core::{Package, PartNameRequest, QName, XmlDocument};
use ooxml_pptx::{
    Error, Presentation, RunFill, SystemColor, TableStyleDef, ThemeColor, ThemeColorSlot,
    ThemeFonts, ThemeOptions,
};

#[expect(dead_code, reason = "output names are used by the two example targets")]
#[path = "oracle/styles_theme_case.rs"]
mod styles_theme_case;

const MAIN: &str = "ppt/presentation.xml";
const MAIN_RELS: &str = "ppt/_rels/presentation.xml.rels";
const STYLES: &str = "ppt/tableStyles.xml";
const THEME: &str = "ppt/theme/theme1.xml";
const STYLES_CT: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml";
const STYLES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

fn fixture(name: &str) -> Package {
    Package::open(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../reference/fixtures/pptx")
            .join(name),
    )
    .unwrap()
}

fn xml(pkg: &Package, part: &str) -> String {
    String::from_utf8(pkg.read_part(part).unwrap()).unwrap()
}

fn replace(pkg: &mut Package, part: &str, old: &str, new: &str) {
    let source = xml(pkg, part);
    assert!(source.contains(old), "{part}: {old}");
    pkg.set_part(part, source.replacen(old, new, 1).into_bytes())
        .unwrap();
}

fn assert_other_parts(before: &Package, after: &Package, changed: &[&str]) {
    let untouched = |pkg: &Package| -> Vec<String> {
        pkg.part_names()
            .filter(|part| !changed.contains(part))
            .map(str::to_owned)
            .collect()
    };
    let names = untouched(before);
    assert_eq!(names, untouched(after));
    for part in names {
        assert_eq!(
            before.read_part(&part).unwrap(),
            after.read_part(&part).unwrap(),
            "{part}"
        );
    }
}

fn style_target(pkg: &Package) -> String {
    let rels = pkg.relationships(MAIN).unwrap();
    let mut edges = rels.by_type(STYLES_REL);
    let target = rels.resolve(edges.next().unwrap()).unwrap();
    assert!(edges.next().is_none());
    target
}

#[test]
fn appends_to_related_part_and_preserves_existing_style_bytes() {
    let mut pkg = fixture(styles_theme_case::TABLE_FIXTURE);
    let renamed = "ppt/customStyles.xml";
    let mut original = xml(&pkg, STYLES);
    let last = r#"<a:tblStyle styleId="{01234567-89AB-8CDE-8F00-0123456789AB}" styleName="Keep"><a:tblBg/><a:seCell/></a:tblStyle>"#;
    if original.ends_with("/>") {
        original.truncate(original.len() - 2);
        original.push_str(&format!(">{last}<!-- keep --></a:tblStyleLst>"));
    } else {
        original = original.replace(
            "</a:tblStyleLst>",
            &format!("{last}<!-- keep --></a:tblStyleLst>"),
        );
    }
    pkg.remove_part(STYLES, &[STYLES_REL]).unwrap();
    pkg.add_part(
        PartNameRequest::Exact(renamed.into()),
        STYLES_CT,
        original.as_bytes(),
    )
    .unwrap();
    pkg.add_relationship(
        Some(MAIN),
        STYLES_REL,
        ooxml_core::Target::Part(renamed.into()),
    )
    .unwrap();
    let mut deck = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let id = deck
        .add_table_style(styles_theme_case::definition())
        .unwrap();
    let after = Package::from_bytes(deck.to_bytes().unwrap()).unwrap();
    assert_eq!(style_target(&after), renamed);
    assert_other_parts(&pkg, &after, &[renamed]);

    let output = xml(&after, renamed);
    let doc = XmlDocument::parse(output.as_bytes()).unwrap();
    let entry = doc.root().elements().last().unwrap();
    assert!(entry.is(QName::new(A, "tblStyle")));
    assert_eq!(entry.attr(QName::new("", "styleId")), Some(id.as_str()));
    let span = entry.span().unwrap();
    assert_eq!(
        format!("{}{}", &output[..span.start], &output[span.end()..]),
        original
    );
    let regions = entry
        .elements()
        .map(|node| node.name().local())
        .collect::<Vec<_>>();
    assert_eq!(regions, ["wholeTbl", "firstRow"]);
    assert!(output.contains("<a:insideH><a:ln w=\"12700\""));
    deck.slide_mut(0)
        .unwrap()
        .shape_mut_by_id(4)
        .unwrap()
        .table_mut()
        .unwrap()
        .set_style(Some(&id))
        .unwrap();

    // Exercise the exact input/output pair used by the external oracles.
    let mut sample = Presentation::from_bytes(
        fixture(styles_theme_case::TABLE_FIXTURE)
            .to_bytes()
            .unwrap(),
    )
    .unwrap();
    styles_theme_case::edit_table(&mut sample).unwrap();
}

#[test]
fn missing_style_part_registers_exact_name_and_keeps_default() {
    let mut pkg = fixture(styles_theme_case::TABLE_FIXTURE);
    pkg.remove_part(STYLES, &[STYLES_REL]).unwrap();
    let mut deck = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let id = deck
        .add_table_style(styles_theme_case::definition())
        .unwrap();
    let saved = Package::from_bytes(deck.to_bytes().unwrap()).unwrap();
    assert_eq!(style_target(&saved), STYLES);
    assert_eq!(saved.content_type(STYLES), Some(STYLES_CT));
    let style_xml = xml(&saved, STYLES);
    assert!(
        style_xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n")
    );
    let doc = XmlDocument::parse(style_xml.as_bytes()).unwrap();
    assert_eq!(
        doc.root().attr(QName::new("", "def")),
        Some("{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}")
    );
    assert_eq!(
        doc.root()
            .elements()
            .next()
            .unwrap()
            .attr(QName::new("", "styleId")),
        Some(id.as_str())
    );
    assert_other_parts(&pkg, &saved, &[STYLES, MAIN_RELS, "[Content_Types].xml"]);
    for (part, attr, suffix) in [
        (MAIN_RELS, "Type", STYLES_REL),
        ("[Content_Types].xml", "PartName", "/ppt/tableStyles.xml"),
    ] {
        let before = xml(&pkg, part);
        let after = xml(&saved, part);
        let doc = XmlDocument::parse(after.as_bytes()).unwrap();
        let added = doc.root().elements().last().unwrap();
        assert_eq!(added.attr(QName::new("", attr)), Some(suffix));
        let span = added.span().unwrap();
        assert_eq!(
            format!("{}{}", &after[..span.start], &after[span.end()..]),
            before
        );
    }
}

#[test]
fn style_refusals_preserve_package_bytes() {
    let mut pkg = fixture(styles_theme_case::TABLE_FIXTURE);
    let original = pkg.to_bytes().unwrap();
    let mut deck = Presentation::from_bytes(original.clone()).unwrap();
    assert_matches!(
        deck.slide_mut(0)
            .unwrap()
            .shape_mut_by_id(4)
            .unwrap()
            .table_mut()
            .unwrap()
            .set_style(Some("{11111111-1111-1111-1111-111111111111}")),
        Err(Error::TableStyleNotDefined(_))
    );
    assert_eq!(deck.to_bytes().unwrap(), original);

    for keep_rel in [true, false] {
        // Leave either the original relationship or its content type entry
        // referring to a now absent part.
        let mut dangling = Package::from_bytes(original.clone()).unwrap();
        dangling.remove_part(STYLES, &[STYLES_REL]).unwrap();
        let registry = if keep_rel {
            MAIN_RELS
        } else {
            "[Content_Types].xml"
        };
        dangling
            .set_part(registry, pkg.read_part(registry).unwrap())
            .unwrap();
        let before = dangling.to_bytes().unwrap();
        let mut deck = Presentation::from_bytes(before.clone()).unwrap();
        assert_matches!(
            deck.add_table_style(TableStyleDef::default()),
            Err(Error::Core(ooxml_core::Error::RegistryRefused {
                reason: RegistryRefusal::NameInUse { .. },
                ..
            }))
        );
        assert_eq!(deck.to_bytes().unwrap(), before);
    }

    // A valid region was already built on the clone when validation fails.
    let mut def = styles_theme_case::definition();
    def.parts
        .first_row
        .as_mut()
        .unwrap()
        .cell
        .as_mut()
        .unwrap()
        .fill = Some(RunFill::Solid(
        ooxml_pptx::WriteColor::Scheme(ooxml_pptx::SchemeColor::PhColor).into(),
    ));
    assert_matches!(deck.add_table_style(def), Err(Error::InvalidSchemeColor));
    assert_eq!(deck.to_bytes().unwrap(), original);

    // Leave the existing theme part alone while constructing an incomplete one.
    replace(&mut pkg, THEME, "<a:ea typeface=\"\"/>", "");
    let before = pkg.to_bytes().unwrap();
    let mut deck = Presentation::from_bytes(before.clone()).unwrap();
    assert_matches!(
        deck.master_theme_mut(0)
            .unwrap()
            .set_major_font(ThemeFonts {
                latin: "Changed",
                ..ThemeFonts::default()
            }),
        Err(Error::UnsupportedContent(_))
    );
    assert_eq!(deck.to_bytes().unwrap(), before);
}

#[test]
fn theme_changes_only_requested_colour_and_font_attributes() {
    let pkg = fixture(styles_theme_case::THEME_FIXTURE);
    let original = xml(&pkg, THEME);
    let mut deck = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    styles_theme_case::edit_theme(&mut deck).unwrap();
    let after = Package::from_bytes(deck.to_bytes().unwrap()).unwrap();
    assert_other_parts(&pkg, &after, &[THEME]);
    let output = xml(&after, THEME);
    let read = |xml: &str| XmlDocument::parse(xml.as_bytes()).unwrap();
    let orig_doc = read(&original);
    let output_doc = read(&output);
    fn spans(doc: &XmlDocument, keys: &[&str]) -> Vec<std::ops::Range<usize>> {
        let mut node = doc.root();
        for key in keys {
            node = node
                .elements()
                .find(|node| node.name().local() == *key)
                .unwrap();
        }
        node.elements()
            .take(if keys.ends_with(&["majorFont"]) { 3 } else { 1 })
            .map(|node| {
                let span = node.span().unwrap();
                span.start..span.end()
            })
            .collect()
    }
    let mask = |doc: &XmlDocument| {
        let mut ranges = spans(doc, &["themeElements", "clrScheme", "accent1"]);
        ranges.extend(spans(doc, &["themeElements", "fontScheme", "majorFont"]));
        let mut xml = doc.source().to_owned();
        for range in ranges.into_iter().rev() {
            xml.replace_range(range, "");
        }
        xml
    };
    assert!(original.contains("<a:font script="));
    assert_eq!(mask(&orig_doc), mask(&output_doc));
    assert!(output.contains("val=\"1C8494\""));
    assert!(output.contains("typeface=\"Georgia\""));

    let mut theme = deck.master_theme_mut(0).unwrap();
    theme
        .set_minor_font(ThemeFonts {
            latin: "Verdana",
            east_asian: "Yu Gothic",
            complex_script: "Arial",
        })
        .unwrap();
    theme
        .set_color(
            ThemeColorSlot::Dk1,
            ThemeColor::System(SystemColor::WindowText, Some([2, 3, 4])),
        )
        .unwrap();
    let output = xml(deck.package(), THEME);
    assert!(output.contains("lastClr=\"020304\""));
    for name in ["Verdana", "Yu Gothic", "Arial"] {
        assert!(output.contains(&format!("typeface=\"{name}\"")));
    }
}

#[test]
fn theme_shared_with_notes_master_requires_opt_in() {
    assert_shared_theme(
        "python-pptx/sld-notes.pptx",
        "ppt/notesMasters/_rels/notesMaster1.xml.rels",
        "../theme/theme2.xml",
    );
}

#[test]
fn theme_shared_with_handout_master_requires_opt_in() {
    assert_shared_theme(
        "poi/45545_Comment.pptx",
        "ppt/handoutMasters/_rels/handoutMaster1.xml.rels",
        "../theme/theme3.xml",
    );
}

#[test]
fn theme_shared_with_another_slide_master_requires_opt_in() {
    assert_shared_theme(
        "poi/bug68703.pptx",
        "ppt/slideMasters/_rels/slideMaster2.xml.rels",
        "../theme/theme2.xml",
    );
}

fn assert_shared_theme(fixture_name: &str, rels: &str, old_target: &str) {
    let mut pkg = fixture(fixture_name);
    replace(&mut pkg, rels, old_target, "../theme/theme1.xml");
    let before = pkg.to_bytes().unwrap();
    let mut deck = Presentation::from_bytes(before.clone()).unwrap();
    assert_matches!(deck.master_theme_mut(0), Err(Error::SharedTheme));
    assert_eq!(deck.to_bytes().unwrap(), before);

    deck.master_theme_mut_with(0, ThemeOptions { allow_shared: true })
        .unwrap()
        .set_color(ThemeColorSlot::Accent1, ThemeColor::Rgb([1, 2, 3]))
        .unwrap();
    let after = Package::from_bytes(deck.to_bytes().unwrap()).unwrap();
    assert_other_parts(&pkg, &after, &[THEME]);
    assert!(xml(&after, THEME).contains("val=\"010203\""));
}

fn alternate_content(contents: &str) -> String {
    format!(
        r#"<compat:AlternateContent xmlns:compat="{}"><compat:Choice Requires="a">{contents}</compat:Choice><compat:Fallback>{contents}</compat:Fallback></compat:AlternateContent>"#,
        ooxml_core::name::MC_NS
    )
}

#[test]
fn table_style_list_with_alternate_content_is_refused_without_changes() {
    let mut pkg = fixture(styles_theme_case::TABLE_FIXTURE);
    let style_xml = xml(&pkg, STYLES);
    let doc = XmlDocument::parse(style_xml.as_bytes()).unwrap();
    let span = doc.root().elements().next().unwrap().span().unwrap();
    let entry = &style_xml[span.start..span.end()];
    replace(&mut pkg, STYLES, entry, &alternate_content(entry));
    let before = pkg.to_bytes().unwrap();
    let mut deck = Presentation::from_bytes(before.clone()).unwrap();
    assert_matches!(
        deck.add_table_style(TableStyleDef::default()),
        Err(Error::UnsupportedContent(message))
            if message.contains("a:tblStyleLst") && message.contains("mc:AlternateContent")
    );
    assert_eq!(deck.to_bytes().unwrap(), before);
}

#[test]
fn alternate_theme_color_is_refused_without_changes() {
    let color = r#"<a:srgbClr val="C0504D"/>"#;
    let slot = format!("<a:accent2>{color}</a:accent2>");
    for replacement in [
        alternate_content(&slot),
        format!("<a:accent2>{}</a:accent2>", alternate_content(color)),
    ] {
        let mut pkg = fixture(styles_theme_case::THEME_FIXTURE);
        replace(&mut pkg, THEME, &slot, &replacement);
        let before = pkg.to_bytes().unwrap();
        let mut deck = Presentation::from_bytes(before.clone()).unwrap();
        assert_matches!(
            deck.master_theme_mut(0)
                .unwrap()
                .set_color(ThemeColorSlot::Accent2, ThemeColor::Rgb([1, 2, 3])),
            Err(Error::UnsupportedContent(message))
                if message.contains("theme colour") && message.contains("mc:AlternateContent")
        );
        assert_eq!(deck.to_bytes().unwrap(), before);
    }
}

#[test]
fn theme_typeface_references_are_refused_without_changes() {
    let before = fixture(styles_theme_case::THEME_FIXTURE)
        .to_bytes()
        .unwrap();
    let mut deck = Presentation::from_bytes(before.clone()).unwrap();
    assert_matches!(
        deck.master_theme_mut(0)
            .unwrap()
            .set_major_font(ThemeFonts {
                latin: "Georgia",
                east_asian: "+mj-ea",
                complex_script: "Arial",
            }),
        Err(Error::ValueOutOfRange { what: "typeface" })
    );
    assert_eq!(deck.to_bytes().unwrap(), before);
    assert_matches!(
        deck.master_theme_mut(0)
            .unwrap()
            .set_minor_font(ThemeFonts {
                latin: "+mn-lt",
                ..ThemeFonts::default()
            }),
        Err(Error::ValueOutOfRange { what: "typeface" })
    );
    assert_eq!(deck.to_bytes().unwrap(), before);
}
