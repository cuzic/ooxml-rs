use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

use ooxml_core::{Package, QName, RawNode, XmlDocument};

use super::*;
use crate::Workbook;
use crate::images::walk;

const CT: &str = "[Content_Types].xml";
const COMMENTS: &str = "xl/comments1.xml";
const VML: &str = "xl/drawings/vmlDrawing1.vml";
const THREAD: &str = "xl/threadedComments/threadedComment1.xml";
const PERSONS: &str = "xl/persons/person.xml";
const XR: &str = "http://schemas.microsoft.com/office/spreadsheetml/2014/revision";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const XMLNS: &str = "http://www.w3.org/2000/xmlns/";
const SML: &str = crate::SML_NS;
const V: &str = "urn:schemas-microsoft-com:vml";
const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const OFFICE: &str = "urn:schemas-microsoft-com:office:office";
const X: &str = "urn:schemas-microsoft-com:office:excel";
const RID: QName = xml::RID;

fn cell(s: &str) -> CellIndex {
    CellIndex::parse(s).unwrap()
}

fn part(pkg: &Package, name: &str) -> String {
    String::from_utf8(pkg.read_part(name).unwrap()).unwrap()
}

fn doc(pkg: &Package, name: &str) -> XmlDocument {
    XmlDocument::parse(&pkg.read_part(pkg.part_name(name).unwrap()).unwrap()).unwrap()
}

fn deterministic() -> Workbook {
    static NEXT: AtomicU8 = AtomicU8::new(0);
    let mut wb = Workbook::new().unwrap();
    wb.comment_random = Some(|| [NEXT.fetch_add(1, Ordering::Relaxed); 16]);
    wb
}

fn rejected(wb: &mut Workbook, edit: impl FnOnce(&mut Workbook) -> Result<()>) -> Error {
    let before = wb.to_bytes().unwrap();
    let error = edit(wb).unwrap_err();
    assert_eq!(wb.to_bytes().unwrap(), before, "{error:?}");
    error
}

fn with_part(wb: &Workbook, name: &str, bytes: String) -> Workbook {
    let mut pkg = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    pkg.set_part(name, bytes.into_bytes()).unwrap();
    Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap()
}

fn workbook_with_note() -> Workbook {
    let mut wb = deterministic();
    wb.worksheet()
        .unwrap()
        .add_note(cell("C3"), Note::new("Original").author("Ana"))
        .unwrap();
    wb
}

#[test]
fn legacy_notes_share_shape_and_reuse_authors() {
    let mut wb = workbook_with_note();
    let at = cell("D1");
    let notes = [
        Note::new("First").author("Ana"),
        Note::new("Second").author("Bob"),
        Note::new("Third").author("Bob"),
    ];
    let mut sheet = wb.worksheet().unwrap();
    sheet.add_legacy_notes(at, &notes).unwrap();
    assert_eq!(
        sheet.cell_notes().unwrap().into_iter().filter(|entry| entry.0 == at).collect::<Vec<_>>(),
        vec![
            (at, "Ana".into(), "First".into()),
            (at, "Bob".into(), "Second".into()),
            (at, "Bob".into(), "Third".into()),
        ],
    );
    assert!(sheet.add_legacy_notes(at, &notes).is_err());
    let xml = part(wb.package(), COMMENTS);
    assert_eq!(xml.matches(">Ana</author>").count(), 1);
    assert_eq!(xml.matches(">Bob</author>").count(), 1);
    let drawing = part(wb.package(), VML);
    assert_eq!(drawing.matches("<x:Row>0</x:Row><x:Column>3</x:Column>").count(), 1, "{drawing}");
    let mut sheet = wb.worksheet().unwrap();
    sheet.replace_legacy_notes(at, &[Note::new("Edited").author("Bob")]).unwrap();
    assert_eq!(sheet.note_text(at).unwrap().as_deref(), Some("Edited"));
    assert_eq!(part(wb.package(), VML), drawing);
    wb.worksheet().unwrap().replace_legacy_notes(at, &[]).unwrap();
    assert!(!part(wb.package(), VML).contains("<x:Row>0</x:Row><x:Column>3</x:Column>"));

}

#[test]
fn note_sources_report_stored_markup_runs_and_visibility() {
    let mut wb = deterministic();
    let mut sheet = wb.worksheet().unwrap();
    sheet.add_note(cell("C3"), Note::new("plain & <x>").author("Ana").visible(true)).unwrap();
    sheet.add_note(cell("A1"), Note::new("second").author("Bo")).unwrap();
    let comments = part(wb.package(), COMMENTS);
    let start = comments.find("<comment ref=\"A1\"").unwrap();
    let start = start + comments[start..].find("<text>").unwrap();
    let end = start + comments[start..].find("</text>").unwrap() + "</text>".len();
    let rich = r#"<text><r><rPr><b/><outline/></rPr><t>B</t></r><r><t xml:space="preserve"> &amp;r</t></r></text>"#;
    let mut wb = with_part(&wb, COMMENTS, format!("{}{rich}{}", &comments[..start], &comments[end..]));
    let mut sheet = wb.worksheet().unwrap();
    let sources = sheet.note_sources().unwrap();
    let plain = sources.iter().find(|source| source.at == cell("C3")).unwrap();
    assert_eq!((plain.author.as_str(), plain.text.as_str()), ("Ana", "plain & <x>"));
    let stored = plain.text_xml.as_deref().unwrap();
    assert!(stored.contains("plain &amp; &lt;x&gt;") && !stored.starts_with("<text"), "{stored}");
    // This crate writes note text as one run without properties.
    assert_eq!(plain.runs, Some(vec![("plain & <x>".into(), None)]));
    let rich = sources.iter().find(|source| source.at == cell("A1")).unwrap();
    assert_eq!(rich.text, "B &r");
    assert_eq!(rich.text_xml.as_deref(),
        Some(r#"<r><rPr><b/><outline/></rPr><t>B</t></r><r><t xml:space="preserve"> &amp;r</t></r>"#));
    let runs = rich.runs.as_ref().unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].1.as_ref().unwrap().fields & crate::DECLARED_FONT_OUTLINE, crate::DECLARED_FONT_OUTLINE);
    assert_eq!(runs[1], (" &r".into(), None));
    let mut visibility = sheet.note_visibility().unwrap();
    visibility.sort();
    assert_eq!(visibility, vec![(cell("A1"), false), (cell("C3"), true)]);
    assert_eq!(Workbook::new().unwrap().worksheet().unwrap().note_visibility().unwrap(), vec![]);
}

#[test]
fn declared_note_runs_keep_fonts_as_written() {
    let wb = workbook_with_note();
    let comments = part(wb.package(), COMMENTS);
    let start = comments.find("<text>").unwrap();
    let end = comments.find("</text>").unwrap() + "</text>".len();
    let text = concat!(
        r#"<text><r><rPr><b/><sz val="9"/></rPr><t xml:space="preserve">rich </t></r>"#,
        r#"<r><rPr><outline/><rFont val="Arial"/></rPr><t>o</t></r><r><t>note</t></r></text>"#,
    );
    let mut wb = with_part(&wb, COMMENTS, format!("{}{text}{}", &comments[..start], &comments[end..]));
    let sheet = wb.worksheet().unwrap();
    let runs = sheet.declared_note_runs(cell("C3")).unwrap().unwrap();
    assert_eq!(runs.len(), 3);
    let (text, font) = &runs[0];
    let font = font.as_ref().unwrap();
    assert_eq!((text.as_str(), font.fields), ("rich ", crate::DECLARED_FONT_SIZE));
    assert!(font.font.bold);
    assert_eq!(font.font.size.get(), 9.0);
    let font = runs[1].1.as_ref().unwrap();
    assert_eq!(font.fields, crate::DECLARED_FONT_NAME | crate::DECLARED_FONT_OUTLINE);
    assert_eq!(font.font.name, "Arial");
    assert_eq!(runs[2], ("note".into(), None));
    // The plain run reader cannot express the outline run and falls back to text.
    assert_eq!(sheet.note_runs(cell("C3")).unwrap(), Some(vec![("rich onote".into(), None)]));
}

#[test]
fn reordered_legacy_notes_keep_unchanged_rich_markup() {
    let mut wb = workbook_with_note();
    let at = cell("D1");
    let rich = crate::Font { bold: true, ..Default::default() };
    let runs = vec![("First".into(), Some(rich))];
    let mut sheet = wb.worksheet().unwrap();
    sheet.add_legacy_notes(at, &[
        Note::rich_text(runs.clone()).author("Alice"),
        Note::new("Second").author("Bob"),
    ]).unwrap();
    sheet.replace_legacy_notes(at, &[
        Note::new("Revised").author("Bob"),
        Note::new("First").author("Alice"),
    ]).unwrap();
    let comments = part(wb.package(), COMMENTS);
    assert!(comments.find("Revised").unwrap() < comments.find("First").unwrap());
    assert!(comments.contains("<rPr>"), "{comments}");
    let mut sheet = wb.worksheet().unwrap();
    sheet.replace_legacy_notes(at, &[Note::new("First").author("Alice")]).unwrap();
    assert_eq!(sheet.note_runs(at).unwrap(), Some(runs));
}

#[test]
fn notes_sorted_authors_reused_and_text_encoded() {
    let mut wb = workbook_with_note();
    let before = part(wb.package(), VML);
    {
        let mut ws = wb.worksheet().unwrap();
        ws.add_note(
            cell("D1"),
            Note::new(" \u{0001} _x0002_ ").author("Ana").visible(true),
        )
        .unwrap();
        ws.add_note(cell("A3"), Note::new("Third").author("Sam"))
            .unwrap();
    }
    let xml = part(wb.package(), COMMENTS);
    assert!(xml.contains(" _x0001_ _x005F_x0002_ </t>"), "{xml}");
    assert!(xml.contains("xml:space=\"preserve\""));
    assert_eq!(xml.matches(">Ana</author>").count(), 1);
    assert!(xml.find("ref=\"D1\"") < xml.find("ref=\"A3\""));
    assert!(xml.find("ref=\"A3\"") < xml.find("ref=\"C3\""));
    let vml = part(wb.package(), VML);
    assert!(vml.starts_with(before.strip_suffix("</xml>").unwrap()));
    assert_eq!(vml.matches("o:spt=\"202\"").count(), 1);
    assert_eq!(vml.matches("<x:Visible/>").count(), 1);

    wb.worksheet()
        .unwrap()
        .set_note_text(cell("D1"), "\tRevised ")
        .unwrap();
    let edited = part(wb.package(), COMMENTS);
    assert_eq!(
        edited,
        xml.replace(" _x0001_ _x005F_x0002_ </t>", "\tRevised </t>")
    );
    assert_eq!(part(wb.package(), VML), vml);
    wb.worksheet().unwrap().remove_note(cell("D1")).unwrap();
    assert!(!part(wb.package(), VML).contains("_x0000_s1026"));
    wb.worksheet()
        .unwrap()
        .add_note(cell("D1"), Note::new("New"))
        .unwrap();
    assert!(part(wb.package(), VML).contains("_x0000_s1028"));
    for at in ["D1", "A3", "C3"] {
        wb.worksheet().unwrap().remove_note(cell(at)).unwrap();
    }
    assert!(part(wb.package(), COMMENTS).contains("<commentList></commentList>"));
    assert!(!part(wb.package(), VML).contains("<v:shape "));
}

#[test]
fn thread_reply_remove_and_guid_collisions() {
    let mut wb = deterministic();
    let ana = wb.person("Ana", PersonId::None).unwrap();
    let sam = wb.person("Sam", PersonId::None).unwrap();
    assert_eq!(wb.person("Ana", PersonId::None).unwrap(), ana);
    let at = cell("B2");
    wb.worksheet()
        .unwrap()
        .add_comment(at, Comment::new("First").by(&ana))
        .unwrap();
    let first = doc(wb.package(), THREAD);
    let root_id = first
        .root()
        .elements()
        .next()
        .unwrap()
        .attr(xml::attr("id"))
        .unwrap();
    let before = part(wb.package(), COMMENTS);
    assert!(before.contains(&format!(
        "xmlns:xr=\"{XR}\" mc:Ignorable=\"xr\" xr:uid=\"{root_id}\""
    )));
    assert!(before.contains(&format!(">tc={root_id}</author>")));
    wb.worksheet()
        .unwrap()
        .reply(
            at,
            Comment::new("Reply")
                .by(&sam)
                .timestamp(std::time::UNIX_EPOCH - std::time::Duration::from_secs(1)),
        )
        .unwrap();
    let updated = part(wb.package(), COMMENTS);
    assert_eq!(updated, before.replace("First</t>", "First\n\nReply</t>"));
    for name in [THREAD, PERSONS] {
        let document = doc(wb.package(), name);
        assert!(
            document
                .root()
                .elements()
                .all(|node| xml::default_namespace(node).is_none())
        );
    }
    let replies = part(wb.package(), THREAD);
    assert!(replies.contains(&format!("parentId=\"{root_id}\"")));
    assert!(replies.contains("dT=\"1969-12-31T23:59:59\""));
    assert!(replies.contains(&format!("personId=\"{}\"", sam.id())));

    // A repeatable random source exercises the retry bound and pre-apply refusal.
    wb.comment_random = Some(|| [0xff; 16]);
    let p = wb.person("Pat", PersonId::None).unwrap();
    assert_eq!(p.id(), "{FFFFFFFF-FFFF-4FFF-BFFF-FFFFFFFFFFFF}");
    std::assert_matches!(
        rejected(&mut wb, |wb| wb
            .worksheet()?
            .reply(at, Comment::new("collision").by(&ana))),
        Error::Random(_)
    );
    wb.worksheet().unwrap().remove_comment_thread(at).unwrap();
    assert!(doc(wb.package(), THREAD).root().elements().next().is_none());
    assert!(doc(wb.package(), PERSONS).root().elements().count() == 3);
    assert!(!part(wb.package(), VML).contains("<v:shape "));
    assert!(part(wb.package(), COMMENTS).contains("<commentList></commentList>"));
}

#[test]
fn note_children_reuse_only_the_matching_default_namespace() {
    let wb = workbook_with_note();
    let notes = format!(
        r#"<s:comments xmlns:s="{SML}" xmlns="urn:other"><s:authors xmlns="{SML}"/><s:commentList/></s:comments>"#
    );
    let mut wb = with_part(&wb, COMMENTS, notes);
    let at = cell("B2");
    wb.worksheet()
        .unwrap()
        .add_note(at, Note::new("First").author("Ana"))
        .unwrap();
    wb.worksheet().unwrap().set_note_text(at, "Edited").unwrap();

    let notes = doc(wb.package(), COMMENTS);
    let authors = xml::child(notes.root(), QName::new(SML, "authors")).unwrap();
    let author = xml::child(authors, QName::new(SML, "author")).unwrap();
    let list = xml::child(notes.root(), QName::new(SML, "commentList")).unwrap();
    let note = xml::child(list, QName::new(SML, "comment")).unwrap();
    let text = xml::child(note, QName::new(SML, "text")).unwrap();
    assert_eq!(xml::default_namespace(author), None);
    assert_eq!(xml::default_namespace(note), Some(SML));
    assert_eq!(xml::default_namespace(text), None);
    assert_eq!(
        walk(text)
            .find(|node| is_local(SML, "t", node))
            .unwrap()
            .text(),
        "Edited"
    );
}

#[test]
fn orphan_fallback_note_can_be_edited_and_removed() {
    let mut wb = deterministic();
    let person = wb.person("Ana", PersonId::None).unwrap();
    let at = cell("B2");
    wb.worksheet()
        .unwrap()
        .add_comment(at, Comment::new("First").by(person))
        .unwrap();
    let thread = doc(wb.package(), THREAD);
    let entry = thread.root().elements().next().unwrap().span().unwrap();
    let mut orphan = with_part(&wb, THREAD, without(thread.source(), [entry]));

    orphan
        .worksheet()
        .unwrap()
        .set_note_text(at, "Edited")
        .unwrap();
    assert!(part(orphan.package(), COMMENTS).contains(">Edited</t>"));
    assert_eq!(part(orphan.package(), VML), part(wb.package(), VML));
    orphan.worksheet().unwrap().remove_note(at).unwrap();
    assert!(part(orphan.package(), COMMENTS).contains("<commentList></commentList>"));
    assert!(
        vml::note_shape(VML, &doc(orphan.package(), VML), at)
            .unwrap()
            .is_none()
    );
}

#[test]
fn fallback_uid_matches_thread_guid_ignoring_case() {
    let mut wb = deterministic();
    let person = wb.person("Ana", PersonId::None).unwrap();
    wb.comment_random = Some(|| [0xab; 16]);
    let at = cell("B2");
    wb.worksheet()
        .unwrap()
        .add_comment(at, Comment::new("First").by(&person))
        .unwrap();
    let threads = doc(wb.package(), THREAD);
    let id = threads
        .root()
        .elements()
        .next()
        .unwrap()
        .attr(xml::attr("id"))
        .unwrap();
    let notes = part(wb.package(), COMMENTS).replace(
        &format!("xr:uid=\"{id}\""),
        &format!("xr:uid=\"{}\"", id.to_ascii_lowercase()),
    );
    let mut wb = with_part(&wb, COMMENTS, notes);
    wb.worksheet()
        .unwrap()
        .reply(at, Comment::new("Reply").by(person))
        .unwrap();
    assert!(part(wb.package(), COMMENTS).contains("First\n\nReply</t>"));
    wb.worksheet().unwrap().remove_comment_thread(at).unwrap();
    assert!(doc(wb.package(), THREAD).root().elements().next().is_none());
}

#[test]
fn new_vml_declares_namespaces_and_appends_shapes_above_existing_ones() {
    let wb = workbook_with_note();
    let new = doc(wb.package(), VML);
    for (prefix, uri) in [("v", V), ("o", OFFICE), ("x", X)] {
        let name = QName::new(XMLNS, prefix);
        assert_eq!(new.root().attr(name), Some(uri));
        assert!(
            walk(new.root())
                .skip(1)
                .all(|node| node.attr(name).is_none())
        );
    }
    assert_eq!(vml_ids(&new).len(), 1);
    let original = new.source().replace("z-index:1", "Z-INDEX : 42 ");
    let mut wb = with_part(&wb, VML, original.clone());
    wb.worksheet()
        .unwrap()
        .add_note(cell("B2"), Note::new("Top"))
        .unwrap();

    let reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let updated = doc(reopened.package(), VML);
    assert_eq!(vml_ids(&updated).len(), 2);
    let shape = vml::note_shape(VML, &updated, cell("B2")).unwrap().unwrap();
    assert!(
        shape
            .attr(xml::attr("style"))
            .unwrap()
            .contains(";z-index:43;")
    );
    assert_eq!(without(updated.source(), [shape.span().unwrap()]), original);

    let exhausted = original.replace("Z-INDEX : 42 ", &format!("z-index:{}", i64::MAX));
    let mut wb = with_part(&wb, VML, exhausted);
    std::assert_matches!(
        rejected(&mut wb, |wb| wb
            .worksheet()?
            .add_note(cell("D4"), Note::new("Overflow"))),
        Error::PartNotEditable { .. }
    );
}

#[test]
fn new_vml_shape_uses_positive_z_index_when_existing_shapes_are_negative() {
    let wb = workbook_with_note();
    let original = doc(wb.package(), VML)
        .source()
        .replace("z-index:1", "z-index:-5");
    let mut wb = with_part(&wb, VML, original);
    let at = cell("B2");
    wb.worksheet()
        .unwrap()
        .add_note(at, Note::new("Top"))
        .unwrap();

    let reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let updated = doc(reopened.package(), VML);
    let shape = vml::note_shape(VML, &updated, at).unwrap().unwrap();
    assert!(
        shape
            .attr(xml::attr("style"))
            .unwrap()
            .contains(";z-index:1;")
    );
}

#[test]
fn note_thread_refusals_preserve_the_archive() {
    let mut wb = workbook_with_note();
    let person = wb.person("Ana", PersonId::None).unwrap();
    let a = cell("C3");
    let b = cell("B2");
    std::assert_matches!(
        rejected(&mut wb, |wb| wb.worksheet()?.add_note(a, Note::new("dup"))),
        Error::NoteExists
    );
    std::assert_matches!(
        rejected(&mut wb, |wb| wb
            .worksheet()?
            .add_comment(a, Comment::new("dup").by(&person))),
        Error::NoteExists
    );
    std::assert_matches!(
        rejected(&mut wb, |wb| wb.worksheet()?.set_note_text(b, "missing")),
        Error::NoSuchNote
    );
    std::assert_matches!(
        rejected(&mut wb, |wb| wb.worksheet()?.remove_note(b)),
        Error::NoSuchNote
    );
    std::assert_matches!(
        rejected(&mut wb, |wb| wb.worksheet()?.remove_comment_thread(b)),
        Error::NoSuchNote
    );
    std::assert_matches!(
        rejected(&mut wb, |wb| wb
            .worksheet()?
            .reply(b, Comment::new("missing").by(&person))),
        Error::NoSuchNote
    );
    std::assert_matches!(
        rejected(&mut wb, |wb| wb
            .worksheet()?
            .add_comment(b, Comment::new("no author"))),
        Error::InvalidSetting { .. }
    );

    wb.worksheet()
        .unwrap()
        .add_comment(b, Comment::new("Thread").by(&person))
        .unwrap();
    for edit in [
        |ws: &mut Worksheet<'_>, at| ws.set_note_text(at, "wrong"),
        |ws: &mut Worksheet<'_>, at| ws.remove_note(at),
    ] {
        std::assert_matches!(
            rejected(&mut wb, |wb| edit(&mut wb.worksheet()?, b)),
            Error::ThreadedComment
        );
    }
    std::assert_matches!(
        rejected(&mut wb, |wb| wb
            .worksheet()?
            .add_comment(b, Comment::new("dup").by(&person))),
        Error::ThreadedComment
    );
    std::assert_matches!(
        rejected(&mut wb, |wb| wb.worksheet()?.add_note(b, Note::new("dup"))),
        Error::NoteExists
    );
}

#[test]
fn fallback_namespace_is_added_only_when_needed() {
    let mut original = workbook_with_note();
    let p = original.person("Ana", PersonId::None).unwrap();
    let comments = part(original.package(), COMMENTS);
    for (declarations, local) in [
        (
            format!("xmlns:xr=\"{XR}\" xmlns:mc=\"{MC}\" mc:Ignorable=\"x15 xr\""),
            false,
        ),
        (format!("xmlns:xr=\"{XR}\""), true),
        (
            format!("xmlns:xr=\"{XR}\" xmlns:mc=\"{MC}\" mc:Ignorable=\"xrr\""),
            true,
        ),
    ] {
        let input = comments.replacen("<comments ", &format!("<comments {declarations} "), 1);
        let mut wb = with_part(&original, COMMENTS, input.clone());
        wb.worksheet()
            .unwrap()
            .add_comment(cell("D5"), Comment::new("New").by(&p))
            .unwrap();
        let out = part(wb.package(), COMMENTS);
        assert_eq!(out.split("<authors").next(), input.split("<authors").next());
        let doc = doc(wb.package(), COMMENTS);
        let fallback = walk(doc.root())
            .find(|node| node.attr(xml::attr("ref")) == Some("D5"))
            .unwrap();
        assert_eq!(fallback.attr(QName::new(XMLNS, "xr")), local.then_some(XR));
        assert_eq!(
            fallback.attr(QName::new(MC, "Ignorable")),
            local.then_some("xr")
        );
    }
    for tag in ["comments ", "commentList"] {
        let input = comments.replacen(
            &format!("<{tag}"),
            &format!("<{} xmlns:xr=\"urn:wrong\" ", tag.trim()),
            1,
        );
        let mut wb = with_part(&original, COMMENTS, input);
        std::assert_matches!(
            rejected(&mut wb, |wb| wb
                .worksheet()?
                .add_comment(cell("D5"), Comment::new("New").by(&p))),
            Error::PartNotEditable { .. }
        );
    }
    // The effective binding is the one on the insertion parent.
    let input = comments
        .replacen("<comments ", "<comments xmlns:xr=\"urn:shadowed\" ", 1)
        .replacen(
            "<commentList>",
            &format!("<commentList xmlns:xr=\"{XR}\">"),
            1,
        );
    let mut wb = with_part(&original, COMMENTS, input);
    wb.worksheet()
        .unwrap()
        .add_comment(cell("D5"), Comment::new("New").by(&p))
        .unwrap();
    let result = doc(wb.package(), COMMENTS);
    let added = walk(result.root())
        .find(|node| node.attr(xml::attr("ref")) == Some("D5"))
        .unwrap();
    assert_eq!(added.attr(QName::new(MC, "Ignorable")), Some("xr"));
}

#[test]
fn malformed_vml_and_ambiguous_removal_are_atomic() {
    let original = workbook_with_note();
    let vml = part(original.package(), VML);
    let mut malformed = with_part(&original, VML, vml.replace("</xml>", ""));
    std::assert_matches!(
        rejected(&mut malformed, |wb| wb
            .worksheet()?
            .set_note_text(cell("C3"), "no")),
        Error::PartNotEditable { .. }
    );
    let start = vml.find("<v:shape ").unwrap();
    let duplicate = format!(
        "{}{}{}",
        &vml[..start],
        &vml[start..vml.len() - 6],
        &vml[start..]
    );
    let mut ambiguous = with_part(&original, VML, duplicate);
    std::assert_matches!(
        rejected(&mut ambiguous, |wb| wb.worksheet()?.remove_note(cell("C3"))),
        Error::PartNotEditable { .. }
    );
}

#[test]
fn exhausted_block_is_appended_and_shared_across_sheets() {
    let mut original = deterministic();
    original.add_sheet("Sheet2", 1).unwrap();
    original
        .worksheet_by_index(0)
        .unwrap()
        .add_note(cell("C3"), Note::new("Original"))
        .unwrap();
    let vml = part(original.package(), VML);
    use std::fmt::Write as _;
    let mut shapes = String::new();
    for id in 1026..2048 {
        write!(shapes, "<v:shape xmlns:v=\"{V}\" id=\"_x0000_s{id}\"/>").unwrap();
    }
    let input = vml.replace("</xml>", &format!("{shapes}</xml>"));
    let mut wb = with_part(&original, VML, input.clone());
    wb.worksheet()
        .unwrap()
        .add_note(cell("D5"), Note::new("More"))
        .unwrap();
    let edited = part(wb.package(), VML);
    assert!(edited.starts_with(&input.replace("data=\"1\"", "data=\"1,2\"")[..input.len() - 4]));
    assert!(edited.contains("id=\"_x0000_s2049\""));
    wb.worksheet_by_index(1)
        .unwrap()
        .add_note(cell("A1"), Note::new("Other sheet"))
        .unwrap();
    let second = part(wb.package(), "xl/drawings/vmlDrawing2.vml");
    assert!(second.contains("data=\"3\""));
    assert!(second.contains("id=\"_x0000_s3073\""));
}

#[cfg(feature = "fault-injection")]
#[test]
fn post_apply_failure_poisoning_and_stale_plan() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    for step in [ApplyStep::NoteDrawing, ApplyStep::CommentIndex] {
        for panic in [false, true] {
            let mut wb = Workbook::new().unwrap();
            if panic {
                wb.inject_apply_panic(step);
            } else {
                wb.inject_apply_failure(step);
            }
            let result = catch_unwind(AssertUnwindSafe(|| {
                wb.worksheet()?.add_note(cell("A1"), Note::new("Note"))
            }));
            if panic {
                assert!(result.is_err());
            } else {
                std::assert_matches!(result.unwrap(), Err(Error::Poisoned(_)));
            }
            std::assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)));
        }
    }
    let mut wb = Workbook::new().unwrap();
    wb.inject_apply_failure(ApplyStep::StaleRegistryPlan);
    std::assert_matches!(
        rejected(&mut wb, |wb| wb
            .worksheet()?
            .add_note(cell("A1"), Note::new("Note"))),
        Error::Core(ooxml_core::Error::RegistryRefused {
            reason: RegistryRefusal::StalePlan,
            ..
        })
    );
    wb.worksheet()
        .unwrap()
        .add_note(cell("A1"), Note::new("Retry"))
        .unwrap();
}

fn relationship_targets(pkg: &Package, source: &str, suffix: &str) -> Vec<String> {
    let rels = pkg.relationships(source).unwrap();
    rels.iter()
        .filter(|rel| rel.rel_type.rsplit('/').next() == Some(suffix))
        .filter_map(|rel| rels.resolve(rel))
        .collect()
}

fn without(source: &str, nodes: impl IntoIterator<Item = ooxml_core::xml::ElementSpan>) -> String {
    let mut spans: Vec<_> = nodes.into_iter().collect();
    spans.sort_by_key(|s| s.start);
    let mut result = String::new();
    let mut start = 0;
    for span in spans {
        result.push_str(&source[start..span.start]);
        start = span.end();
    }
    result.push_str(&source[start..]);
    result
}

fn only_appends(before: &Package, after: &Package, name: &str) {
    if before.part_name(name).is_none() {
        return;
    }
    let old = doc(before, name);
    let new = doc(after, name);
    if old.source() == new.source() {
        return;
    }
    let span = old.root().span().unwrap();
    let fresh: Vec<_> = new
        .root()
        .elements()
        .skip(old.root().elements().count())
        .map(|node| node.span().unwrap())
        .collect();
    assert!(!fresh.is_empty(), "{name}");
    let end = fresh.last().unwrap().end();
    if let Some((close, _)) = span.close {
        let cut = old
            .root()
            .elements()
            .last()
            .map_or(close, |n| n.span().unwrap().end());
        assert_eq!(&new.source()[..cut], &old.source()[..cut], "{name}");
        assert_eq!(&new.source()[end..], &old.source()[cut..], "{name}");
        let mut last = cut;
        for span in fresh {
            assert!(
                new.source()[last..span.start]
                    .chars()
                    .all(char::is_whitespace),
                "{name}"
            );
            last = span.end();
        }
    } else {
        let cut = span.open_end - 2;
        assert_eq!(&new.source()[..cut], &old.source()[..cut], "{name}");
        let closing = new.root().span().unwrap().end();
        assert_eq!(
            &new.source()[closing..],
            &old.source()[span.open_end..],
            "{name}"
        );
    }
}

fn relationship_part(source: &str) -> String {
    let (dir, file) = source.rsplit_once('/').unwrap();
    format!("{dir}/_rels/{file}.rels")
}

fn is_local(ns: &str, local: &'static str, n: &RawNode) -> bool {
    n.name().ns() == ns && n.name().local() == local
}

fn assert_locality(before: &Package, after: &Package, sheet: &str, wb: &str) {
    let mut allowed = HashSet::from([CT.to_string(), sheet.to_owned()]);
    for (source, kind) in [
        (sheet, "drawing"),
        (sheet, "vmlDrawing"),
        (sheet, "comments"),
        (sheet, "threadedComment"),
        (wb, "person"),
    ] {
        if kind != "drawing" {
            allowed.extend(
                relationship_targets(after, source, kind)
                    .into_iter()
                    .filter_map(|p| after.part_name(&p).map(str::to_owned)),
            );
        }
    }
    for name in [
        CT.to_string(),
        relationship_part(sheet),
        relationship_part(wb),
    ] {
        only_appends(before, after, &name);
        allowed.insert(after.part_name(&name).unwrap_or(&name).to_owned());
    }
    for p in before.part_names() {
        let bytes = after.read_part(p).unwrap();
        if !allowed.contains(p) {
            assert_eq!(bytes, before.read_part(p).unwrap(), "{p}");
        }
    }
    for p in after.part_names() {
        assert!(before.part_name(p).is_some() || allowed.contains(p), "{p}");
    }
    let old_sheet = doc(before, sheet);
    let new_sheet = doc(after, sheet);
    let legacy = |doc: &XmlDocument| {
        doc.root()
            .elements()
            .find(|n| is_local(SML, "legacyDrawing", n))
            .and_then(RawNode::span)
    };
    if legacy(&old_sheet).is_none() {
        assert_eq!(
            without(new_sheet.source(), legacy(&new_sheet)),
            old_sheet.source(),
            "{sheet}"
        );
    } else {
        assert_eq!(new_sheet.source(), old_sheet.source(), "{sheet}");
    }
}

fn preserved_children(
    old: &XmlDocument,
    new: &XmlDocument,
    before: &RawNode,
    after: &RawNode,
    allowed: impl Fn(&RawNode) -> bool,
) {
    let mut actual = after.elements();
    for node in before.elements() {
        let span = node.span().unwrap();
        let expected = &old.source()[span.start..span.end()];
        loop {
            let next = actual.next().expect("an existing child disappeared");
            let span = next.span().unwrap();
            if new.source()[span.start..span.end()] == *expected {
                break;
            }
            assert!(allowed(next), "an existing child changed: {expected}");
        }
    }
    assert!(actual.all(allowed), "an unexpected child was inserted");
}

fn assert_object_locality(
    before: &Package,
    after: &Package,
    sheet: &str,
    book: &str,
    cells: &[CellIndex],
) {
    for (source, relation) in [
        (sheet, "comments"),
        (sheet, "vmlDrawing"),
        (sheet, "threadedComment"),
        (book, "person"),
    ] {
        let targets = relationship_targets(after, source, relation);
        let Some(name) = targets
            .first()
            .filter(|name| before.part_name(name).is_some())
        else {
            continue;
        };
        let old = doc(before, name);
        let new = doc(after, name);
        let old_root = old.root();
        let new_root = new.root();
        let new_ref = |node: &RawNode| {
            node.attr(QName::new("", "ref"))
                .and_then(|value| CellIndex::parse(value).ok())
                .is_some_and(|at| cells.contains(&at))
        };
        match relation {
            "comments" => {
                for kind in ["authors", "commentList"] {
                    let old_items = old_root
                        .elements()
                        .find(|n| is_local(SML, kind, n))
                        .unwrap();
                    let new_items = new_root
                        .elements()
                        .find(|n| is_local(SML, kind, n))
                        .unwrap();
                    preserved_children(&old, &new, old_items, new_items, |n| {
                        if kind == "authors" {
                            is_local(SML, "author", n)
                        } else {
                            new_ref(n)
                        }
                    });
                }
            }
            "vmlDrawing" => {
                // The idmap is the other editable span, checked when the block is exhausted.
                let old_shapes = old_root
                    .elements()
                    .filter(|n| !is_local(OFFICE, "shapelayout", n));
                let mut current = new_root
                    .elements()
                    .filter(|n| !is_local(OFFICE, "shapelayout", n));
                for node in old_shapes {
                    let span = node.span().unwrap();
                    let other = current.next().unwrap().span().unwrap();
                    assert_eq!(
                        &old.source()[span.start..span.end()],
                        &new.source()[other.start..other.end()],
                        "{name}"
                    );
                }
                assert!(current.all(|n| is_local(V, "shape", n) || is_local(V, "shapetype", n)));
            }
            "threadedComment" => preserved_children(&old, &new, old_root, new_root, new_ref),
            "person" => preserved_children(&old, &new, old_root, new_root, |n| {
                n.name().local() == "person"
            }),
            _ => unreachable!(),
        }
    }
}

fn drawing_ids(pkg: &Package, sheet: &str) -> HashMap<u32, usize> {
    let mut ids = HashMap::new();
    for p in relationship_targets(pkg, sheet, "drawing") {
        let Some(p) = pkg.part_name(&p) else { continue };
        let Ok(bytes) = pkg.read_part(p) else {
            continue;
        };
        let Ok(doc) = XmlDocument::parse(&bytes) else {
            continue;
        };
        for node in walk(doc.root()).filter(|n| is_local(XDR, "cNvPr", n)) {
            if let Some(id) = node
                .attr(QName::new("", "id"))
                .and_then(|id| id.trim().parse().ok())
            {
                *ids.entry(id).or_insert(0) += 1;
            }
        }
    }
    ids
}

fn vml_ids(doc: &XmlDocument) -> HashMap<u32, usize> {
    let mut ids = HashMap::new();
    for node in walk(doc.root()) {
        let per_shape: HashSet<u32> = [QName::new("", "id"), QName::new(OFFICE, "spid")]
            .into_iter()
            .filter_map(|attr| node.attr(attr))
            .filter_map(|name| name.rsplit_once("_s")?.1.parse().ok())
            .collect();
        for id in per_shape {
            *ids.entry(id).or_insert(0) += 1;
        }
    }
    ids
}

fn vml_blocks(doc: &XmlDocument) -> impl Iterator<Item = u32> {
    walk(doc.root())
        .filter(|n| is_local(OFFICE, "idmap", n))
        .filter_map(|n| n.attr(QName::new("", "data")))
        .flat_map(|data| {
            data.split(',')
                .filter_map(|block| block.trim().parse().ok())
        })
}

fn all_block_counts(pkg: &Package) -> HashMap<u32, usize> {
    let mut blocks = HashMap::new();
    for name in pkg
        .part_names()
        .filter(|name| pkg.content_type(name) == Some(vml::CONTENT_TYPE))
    {
        let Ok(doc) = XmlDocument::parse(&pkg.read_part(name).unwrap()) else {
            continue;
        };
        for block in vml_blocks(&doc) {
            *blocks.entry(block).or_insert(0) += 1;
        }
    }
    blocks
}

fn check_ids(before: &Package, after: &Package, sheet: &str, wb: &str, new: &[CellIndex]) {
    let old_blocks = all_block_counts(before);
    for (block, count) in all_block_counts(after) {
        assert!(
            count == 1 || old_blocks.get(&block) == Some(&count),
            "block {block}"
        );
    }
    let sheet_xml = doc(after, sheet);
    let link = sheet_xml
        .root()
        .elements()
        .find(|node| is_local(SML, "legacyDrawing", node))
        .unwrap();
    let rels = after.relationships(sheet).unwrap();
    let vml_part = rels
        .resolve(rels.by_id(link.attr(RID).unwrap()).unwrap())
        .unwrap();
    let vml = doc(after, &vml_part);
    let old_vml = before.part_name(&vml_part).map(|p| doc(before, p));
    let old_vml_ids = old_vml.as_ref().map(vml_ids).unwrap_or_default();
    let drawing_ids = drawing_ids(after, sheet);
    for (id, count) in vml_ids(&vml) {
        if old_vml_ids.get(&id) == Some(&count) {
            continue; // Some producers mirror existing form controls across the two drawings.
        }
        assert_eq!(count, 1);
        assert!(!drawing_ids.contains_key(&id));
        assert_ne!(id % 1024, 0);
        assert!(vml_blocks(&vml).any(|block| block == id / 1024));
    }
    for cell in new {
        let mut matches = walk(vml.root())
            .filter(|n| is_local(V, "shape", n))
            .filter(|node| {
                node.elements().any(|data| {
                    let coord = |name| {
                        data.elements()
                            .find(|n| is_local(X, name, n))
                            .and_then(|n| n.text().trim().parse::<u32>().ok())
                    };
                    is_local(X, "ClientData", data)
                        && data.attr(QName::new("", "ObjectType")) == Some("Note")
                        && coord("Row") == Some(cell.row)
                        && coord("Column") == Some(cell.col)
                })
            });
        assert!(matches.next().is_some(), "missing shape for {cell}");
        assert!(matches.next().is_none(), "multiple shapes for {cell}");
    }
    let people: HashSet<String> = relationship_targets(after, wb, "person")
        .iter()
        .flat_map(|p| {
            doc(after, p)
                .root()
                .elements()
                .filter_map(|n| n.attr(QName::new("", "id")).map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect();
    if let Some(p) = relationship_targets(after, sheet, "threadedComment").first() {
        let thread = doc(after, p);
        let notes = doc(after, &relationship_targets(after, sheet, "comments")[0]);
        let root = notes.root();
        for node in thread
            .root()
            .elements()
            .filter(|n| n.name().local() == "threadedComment")
        {
            assert!(people.contains(node.attr(QName::new("", "personId")).unwrap()));
            if node.attr(QName::new("", "parentId")).is_some() {
                continue;
            }
            let id = node.attr(QName::new("", "id")).unwrap();
            let note = walk(root)
                .find(|n| n.attr(QName::new(XR, "uid")) == Some(id))
                .unwrap();
            assert_eq!(
                note.attr(QName::new("", "ref")),
                node.attr(QName::new("", "ref"))
            );
            assert_eq!(
                note.attr(QName::new(XMLNS, "xr"))
                    .or_else(|| root.attr(QName::new(XMLNS, "xr"))),
                Some(XR)
            );
            assert!(
                note.attr(QName::new(MC, "Ignorable"))
                    .or_else(|| root.attr(QName::new(MC, "Ignorable")))
                    .is_some_and(|ns| ns.split_ascii_whitespace().any(|prefix| prefix == "xr"))
            );
        }
    }
}

fn sweep_file(path: &Path, tallies: &mut [usize; 6]) {
    let explicit = [
        "BrNotClosed.xlsx",
        "mv-calculator-final-2-20-2013.xlsm",
        "45540_form_Footer.xlsx",
        "45540_form_Header.xlsx",
        "ExcelWithAttachments.xlsm",
        "right-to-left.xlsx",
        "64759.xlsx",
    ];
    let name = path.file_name().unwrap().to_str().unwrap();
    let Ok(mut wb) = Workbook::open(path) else {
        assert!(!explicit.contains(&name), "{name}");
        tallies[0] += 1;
        return;
    };
    let Some(index) = wb
        .sheets
        .iter()
        .position(|s| s.kind == crate::census::SheetKind::Worksheet)
    else {
        tallies[1] += 1;
        return;
    };
    let workbook = wb.workbook_part().to_owned();
    let sheet = wb.worksheet_by_index(index).unwrap().part_name().to_owned();
    let sheet_doc = doc(wb.package(), &sheet);
    let mut occupied: HashSet<String> = walk(sheet_doc.root())
        .filter(|n| is_local(SML, "c", n))
        .filter_map(|n| n.attr(QName::new("", "r")))
        .map(str::to_owned)
        .collect();
    for p in relationship_targets(wb.package(), &sheet, "comments") {
        if let Ok(notes) = XmlDocument::parse(
            &wb.package()
                .read_part(wb.package().part_name(&p).unwrap())
                .unwrap(),
        ) {
            occupied.extend(
                walk(notes.root())
                    .filter(|n| is_local(SML, "comment", n))
                    .filter_map(|n| n.attr(QName::new("", "ref")))
                    .map(str::to_owned),
            );
        }
    }
    let mut free = (1..1000)
        .map(|row| format!("XEZ{row}"))
        .filter(|at| !occupied.contains(at));
    let note = cell(&free.next().unwrap());
    let thread = cell(&free.next().unwrap());
    let original_bytes = wb.to_bytes().unwrap();
    let before = Package::from_bytes(original_bytes.clone()).unwrap();
    let mut thread_wb = Workbook::from_bytes(original_bytes.clone()).unwrap();
    let note_added = match wb
        .worksheet_by_index(index)
        .unwrap()
        .add_note(note, Note::new("POI note"))
    {
        Ok(()) => {
            let reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
            assert_locality(&before, reopened.package(), &sheet, &workbook);
            assert_object_locality(&before, reopened.package(), &sheet, &workbook, &[note]);
            check_ids(&before, reopened.package(), &sheet, &workbook, &[note]);
            true
        }
        Err(error) => {
            assert_eq!(wb.to_bytes().unwrap(), original_bytes, "{name}: {error}");
            eprintln!("{name}: note refused: {error}");
            tallies[2] += 1;
            if name == "BrNotClosed.xlsx" {
                std::assert_matches!(error, Error::PartNotEditable { .. });
            }
            false
        }
    };
    let thread_added = match thread_wb.person("POI author", PersonId::None) {
        Ok(person) => {
            let before_thread = thread_wb.to_bytes().unwrap();
            let mut ws = thread_wb.worksheet_by_index(index).unwrap();
            match ws.add_comment(thread, Comment::new("POI thread").by(&person)) {
                Ok(()) => {
                    ws.reply(thread, Comment::new("POI reply").by(&person))
                        .unwrap_or_else(|error| panic!("{name}: reply refused: {error}"));
                    let mut reopened = Workbook::from_bytes(thread_wb.to_bytes().unwrap()).unwrap();
                    assert_locality(&before, reopened.package(), &sheet, &workbook);
                    assert_object_locality(
                        &before,
                        reopened.package(),
                        &sheet,
                        &workbook,
                        &[thread],
                    );
                    check_ids(&before, reopened.package(), &sheet, &workbook, &[thread]);
                    let thread_part =
                        relationship_targets(reopened.package(), &sheet, "threadedComment")
                            .remove(0);
                    let thread_doc = doc(reopened.package(), &thread_part);
                    let entries = threads::at(&thread_part, &thread_doc, thread).unwrap();
                    assert_eq!(entries.len(), 2, "{name}");
                    assert_eq!(
                        entries[1].attr(xml::attr("parentId")),
                        entries[0].attr(xml::attr("id")),
                        "{name}"
                    );
                    reopened
                        .worksheet_by_index(index)
                        .unwrap()
                        .remove_comment_thread(thread)
                        .unwrap_or_else(|error| panic!("{name}: thread removal refused: {error}"));
                    let removed = Workbook::from_bytes(reopened.to_bytes().unwrap()).unwrap();
                    assert_locality(&before, removed.package(), &sheet, &workbook);
                    assert_object_locality(&before, removed.package(), &sheet, &workbook, &[]);
                    let thread_doc = doc(removed.package(), &thread_part);
                    assert!(
                        threads::at(&thread_part, &thread_doc, thread)
                            .unwrap()
                            .is_empty(),
                        "{name}"
                    );
                    let vml_part =
                        relationship_targets(removed.package(), &sheet, "vmlDrawing").remove(0);
                    assert!(
                        vml::note_shape(&vml_part, &doc(removed.package(), &vml_part), thread)
                            .unwrap()
                            .is_none(),
                        "{name}"
                    );
                    true
                }
                Err(error) => {
                    assert_eq!(
                        thread_wb.to_bytes().unwrap(),
                        before_thread,
                        "{name}: {error}"
                    );
                    eprintln!("{name}: thread refused: {error}");
                    tallies[3] += 1;
                    false
                }
            }
        }
        Err(error) => {
            assert_eq!(
                thread_wb.to_bytes().unwrap(),
                original_bytes,
                "{name}: {error}"
            );
            eprintln!("{name}: person refused: {error}");
            tallies[4] += 1;
            false
        }
    };
    if explicit.contains(&name) {
        assert_eq!(
            (note_added, thread_added),
            (name != "BrNotClosed.xlsx", name != "BrNotClosed.xlsx"),
            "{name}"
        );
        println!("{name}: note={note_added}, thread lifecycle={thread_added}");
    }
    tallies[5] += 1;
}

#[test]
#[ignore = "set OOXML_XLSX_CORPUS to Apache POI test-data/spreadsheet"]
fn poi_comments_sweep() {
    let root = std::env::var_os("OOXML_XLSX_CORPUS").expect("OOXML_XLSX_CORPUS");
    let mut paths: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "xlsx" || e == "xlsm"))
        .collect();
    paths.sort();
    let mut tallies = [0; 6];
    for path in paths {
        sweep_file(&path, &mut tallies);
    }
    let [
        unreadable,
        no_sheet,
        note_refused,
        thread_refused,
        person_refused,
        checked,
    ] = tallies;
    println!(
        "{checked} checked, {unreadable} unreadable, {no_sheet} without a worksheet; {} note successes, {note_refused} refusals; {} thread lifecycles, {person_refused} person refusals, {thread_refused} add_comment refusals",
        checked - note_refused,
        checked - person_refused - thread_refused
    );
    assert_ne!(checked, 0);
}

#[test]
fn new_parts_obey_locality_and_id_invariants() {
    let mut wb = Workbook::new().unwrap();
    let sheet = wb.worksheet_part().to_owned();
    let workbook = wb.workbook_part().to_owned();
    let before = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let person = wb.person("Ana", PersonId::None).unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_note(cell("B1"), Note::new("Note")).unwrap();
    ws.add_comment(cell("D2"), Comment::new("Start").by(&person))
        .unwrap();
    ws.reply(cell("D2"), Comment::new("End").by(&person))
        .unwrap();
    assert_locality(&before, wb.package(), &sheet, &workbook);
    check_ids(
        &before,
        wb.package(),
        &sheet,
        &workbook,
        &[cell("B1"), cell("D2")],
    );
}

#[test]
fn ambiguous_part_links_and_insertion_slots_are_atomic() {
    let mut original = Workbook::new().unwrap();
    original.add_sheet("Sheet2", 1).unwrap();
    original
        .worksheet()
        .unwrap()
        .add_note(cell("A1"), Note::new("One"))
        .unwrap();

    let mut pkg = Package::from_bytes(original.to_bytes().unwrap()).unwrap();
    pkg.add_relationship(
        Some("xl/worksheets/sheet2.xml"),
        REL_NOTES,
        ooxml_core::Target::Part(COMMENTS.into()),
    )
    .unwrap();
    let mut shared = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    std::assert_matches!(
        rejected(&mut shared, |wb| wb
            .worksheet()?
            .set_note_text(cell("A1"), "Shared")),
        Error::PartNotEditable { .. }
    );

    let second_sheet = Some("xl/worksheets/sheet2.xml");
    let mut pkg = Package::from_bytes(original.to_bytes().unwrap()).unwrap();
    pkg.add_relationship(second_sheet, REL_VML, ooxml_core::Target::Part(VML.into()))
        .unwrap();
    let mut shared = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    std::assert_matches!(
        rejected(&mut shared, |wb| wb
            .worksheet()?
            .add_note(cell("B2"), Note::new("Shared"))),
        Error::PartNotEditable { part, reason } if part == VML && reason == "part ownership is ambiguous"
    );
    std::assert_matches!(
        rejected(&mut shared, |wb| wb.worksheet()?.remove_note(cell("A1"))),
        Error::PartNotEditable { part, reason } if part == VML && reason == "part ownership is ambiguous"
    );

    let person = original.person("Ana", PersonId::None).unwrap();
    original
        .worksheet()
        .unwrap()
        .add_comment(cell("B2"), Comment::new("Thread").by(&person))
        .unwrap();
    let mut pkg = Package::from_bytes(original.to_bytes().unwrap()).unwrap();
    pkg.add_relationship(
        second_sheet,
        threads::REL,
        ooxml_core::Target::Part(THREAD.into()),
    )
    .unwrap();
    let mut shared = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    std::assert_matches!(
        rejected(&mut shared, |wb| wb
            .worksheet()?
            .reply(cell("B2"), Comment::new("Reply").by(&person))),
        Error::PartNotEditable { part, reason } if part == THREAD && reason == "part ownership is ambiguous"
    );
    std::assert_matches!(
        rejected(&mut shared, |wb| wb
            .worksheet()?
            .remove_comment_thread(cell("B2"))),
        Error::PartNotEditable { part, reason } if part == THREAD && reason == "part ownership is ambiguous"
    );

    let mut pkg = Package::from_bytes(original.to_bytes().unwrap()).unwrap();
    pkg.add_relationship(
        second_sheet,
        threads::PERSON_REL,
        ooxml_core::Target::Part(PERSONS.into()),
    )
    .unwrap();
    let mut shared = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    std::assert_matches!(
        rejected(&mut shared, |wb| wb
            .person("Sam", PersonId::None)
            .map(|_| ())),
        Error::PartNotEditable { part, reason } if part == PERSONS && reason == "part ownership is ambiguous"
    );

    let original = Workbook::new().unwrap();
    let sheet = original.worksheet_part();
    let input = part(original.package(), sheet).replace(
        "</worksheet>",
        &format!("<mc:AlternateContent xmlns:mc=\"{MC}\"><mc:Fallback><legacyDrawing/></mc:Fallback></mc:AlternateContent></worksheet>"),
    );
    let mut wrapped = with_part(&original, sheet, input);
    std::assert_matches!(
        rejected(&mut wrapped, |wb| wb
            .worksheet()?
            .add_note(cell("A1"), Note::new("No"))),
        Error::Core(ooxml_core::Error::UnresolvedSibling(_))
    );
}

#[test]
fn malformed_comment_containers_and_invalid_reference_are_atomic() {
    let original = workbook_with_note();
    let mut malformed = with_part(&original, COMMENTS, "<comments>".into());
    std::assert_matches!(
        rejected(&mut malformed, |wb| wb.worksheet()?.remove_note(cell("C3"))),
        Error::PartNotEditable { .. }
    );
    let mut missing_data = with_part(
        &original,
        VML,
        part(original.package(), VML).replace(" data=\"1\"", ""),
    );
    std::assert_matches!(
        rejected(&mut missing_data, |wb| wb
            .worksheet()?
            .add_note(cell("A1"), Note::new("No"))),
        Error::PartNotEditable { .. }
    );
    let mut fresh = Workbook::new().unwrap();
    let foreign = Workbook::new()
        .unwrap()
        .person("Ana", PersonId::None)
        .unwrap();
    std::assert_matches!(
        rejected(&mut fresh, |wb| wb
            .worksheet()?
            .add_comment(cell("A1"), Comment::new("No").by(foreign))),
        Error::InvalidSetting { .. }
    );
    std::assert_matches!(
        rejected(&mut fresh, |wb| wb.worksheet()?.add_note(
            CellIndex {
                row: u32::MAX,
                col: 0
            },
            Note::new("No")
        )),
        Error::OutOfRange { .. }
    );
}

#[test]
fn deleting_a_thread_does_not_release_its_guid() {
    let mut wb = deterministic();
    let author = wb.person("Ana", PersonId::None).unwrap();
    wb.comment_random = Some(|| [0x42; 16]);
    let at = cell("D2");
    wb.worksheet()
        .unwrap()
        .add_comment(at, Comment::new("Start").by(&author))
        .unwrap();

    // Exercise removal before the reopened workbook has run its first GUID scan.
    let mut wb = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    wb.worksheet().unwrap().remove_comment_thread(at).unwrap();
    wb.comment_random = Some(|| [0x42; 16]);
    std::assert_matches!(
        rejected(&mut wb, |wb| wb
            .worksheet()?
            .add_comment(at, Comment::new("Collision").by(&author))),
        Error::Random(_)
    );
}

#[cfg(feature = "fault-injection")]
#[test]
fn person_index_fault_after_apply_poisons() {
    for panic in [false, true] {
        let mut wb = Workbook::new().unwrap();
        if panic {
            wb.inject_apply_panic(ApplyStep::CommentIndex);
        } else {
            wb.inject_apply_failure(ApplyStep::CommentIndex);
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            wb.person("Ana", PersonId::None)
        }));
        if panic {
            assert!(outcome.is_err());
        } else {
            std::assert_matches!(outcome.unwrap(), Err(Error::Poisoned(_)));
        }
        std::assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)));
    }
}
