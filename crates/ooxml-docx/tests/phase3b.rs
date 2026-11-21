mod common;

use common::{W_NS, document_xml, docx, fixture, openable, package, part, part_names, with_part};
use ooxml_docx::{
    CommentId, Document, Error, LocalDateTime, NaiveDateTime, NewComment, RunContext, RunRef,
    StoryKind, TextRange,
};
use std::{assert_matches, collections::BTreeSet};

const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const CID: &str = "http://schemas.microsoft.com/office/word/2016/wordml/cid";
const CEX: &str = "http://schemas.microsoft.com/office/word/2018/wordml/cex";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const RUN: &str = "<w:p><w:r><w:t>Anchor</w:t></w:r></w:p>";

fn comment(text: &str) -> NewComment {
    NewComment {
        author: "Ada & Grace".into(),
        initials: Some("AG".into()),
        date: Some(LocalDateTime {
            wall_clock: "2021-05-20T10:57:00".parse::<NaiveDateTime>().unwrap(),
            utc_offset_minutes: 480,
        }),
        paragraphs: vec![text.into()],
    }
}

fn range(doc: &Document) -> TextRange {
    let run = doc
        .paragraphs()
        .flat_map(|p| p.runs())
        .find(|run| {
            run.context() == RunContext::Direct
                && run.check_settable().is_ok()
                && !run.text().is_empty()
        })
        .unwrap()
        .run_ref();
    TextRange::new(run, run)
}

fn run(doc: &Document, p: usize, r: usize) -> RunRef {
    doc.paragraph(p).unwrap().run(r).unwrap().run_ref()
}

fn xml(bytes: &[u8], name: &str) -> String {
    String::from_utf8(part(bytes, name)).unwrap()
}

#[test]
fn split_range_preserves_surroundings_and_keeps_annotation_high_water() {
    let p0 = "<w:p w:rsidR='CAFE'><w:r><w:t>Hi αβ</w:t></w:r></w:p>";
    let p1 = "<w:p><w:r><w:t>last run</w:t></w:r></w:p>";
    let p2 = "<w:p><w:bookmarkStart w:id='132' w:name='high'/><w:bookmarkEnd w:id='132'/><w:r><w:t>Keep me</w:t></w:r></w:p>";
    let input = docx(&format!("{p0}<!-- T2 -->{p1}{p2}"), None, Some(RUN));
    let mut d = Document::from_bytes(input.clone()).unwrap();
    let first = run(&d, 0, 0);
    let last = run(&d, 1, 0);
    assert_matches!(
        d.add_comment(TextRange::new(last, first), comment("reversed")),
        Err(Error::InvalidTextRange)
    );
    assert_eq!(d.to_bytes().unwrap(), input);
    let id = d
        .add_comment(
            TextRange::with_offsets(first, 3, last, 4),
            comment(
                "First
line",
            ),
        )
        .unwrap();
    assert_eq!(id, CommentId(133));
    assert_eq!(d.paragraph(0).unwrap().text(), "Hi αβ");
    assert_eq!(d.paragraph(1).unwrap().text(), "last run");
    let output = d.to_bytes().unwrap();
    assert_eq!(
        part(&input, "word/header1.xml"),
        part(&output, "word/header1.xml")
    );
    let original = xml(&input, "word/document.xml");
    let edited = xml(&output, "word/document.xml");
    let original = original.replace(p0, "").replace(p1, "");
    let tree = roxmltree::Document::parse(&edited).unwrap();
    let paragraphs = tree
        .descendants()
        .filter(|n| n.has_tag_name((W_NS, "p")))
        .take(2)
        .map(|n| n.range())
        .collect::<Vec<_>>();
    let mut outside = edited.clone();
    for range in paragraphs.into_iter().rev() {
        outside.replace_range(range, "");
    }
    assert_eq!(outside, original);
    assert!(edited.contains("<!-- T2 -->"));
    let mut reopened = Document::from_bytes(output).unwrap();
    let line = reopened.comments().next().unwrap();
    assert_eq!(line.text, "First\nline");
    assert_eq!(line.date, Some("2021-05-20T10:57:00Z"));
    assert!(line.anchor.unwrap().reference.is_some());
    reopened.delete_comment(id).unwrap();
    let next = reopened
        .add_comment(range(&reopened), comment("after delete"))
        .unwrap();
    assert_eq!(next, CommentId(134));
}

#[test]
fn modern_companions_match_and_nested_reply_is_atomic() {
    let input = fixture("poi-testComment");
    let mut d = Document::from_bytes(input).unwrap();
    let root = d.add_comment(range(&d), comment("new parent")).unwrap();
    let reply = d.reply(root, comment("new reply")).unwrap();
    let next = d.reply(root, comment("second reply")).unwrap();
    d.set_resolved(reply, true).unwrap();
    let bytes = d.to_bytes().unwrap();
    d = Document::from_bytes(bytes.clone()).unwrap();
    assert_matches!(d.reply(reply, comment("nested")), Err(Error::NestedReply));
    assert_eq!(d.to_bytes().unwrap(), bytes);
    let created = d
        .comments()
        .filter(|c| [root, reply, next].contains(&c.id))
        .collect::<Vec<_>>();
    assert_eq!(created.len(), 3);
    assert_eq!(
        created
            .iter()
            .map(|c| (c.parent, c.done))
            .collect::<Vec<_>>(),
        [(None, true), (Some(root), true), (Some(root), true)]
    );
    assert_thread_parts(&bytes, root, &[reply, next]);
    d.set_resolved(root, false).unwrap();
    assert!(
        d.comments()
            .filter(|c| [root, reply, next].contains(&c.id))
            .all(|c| !c.done)
    );
    d.delete_comment(root).unwrap();
    let deleted = d.to_bytes().unwrap();
    let reopened = Document::from_bytes(deleted.clone()).unwrap();
    assert!(
        reopened
            .comments()
            .all(|c| ![root, reply, next].contains(&c.id))
    );
    for name in [
        "comments.xml",
        "commentsExtended.xml",
        "commentsIds.xml",
        "commentsExtensible.xml",
    ] {
        assert_eq!(
            xml(&deleted, &format!("word/{name}")),
            xml(&fixture("poi-testComment"), &format!("word/{name}"))
        );
    }
}

#[test]
fn resolving_a_reply_with_a_missing_or_cyclic_parent_is_atomic() {
    let mut doc = Document::from_bytes(docx(RUN, None, None)).unwrap();
    let parent = doc.add_comment(range(&doc), comment("parent")).unwrap();
    let reply = doc.reply(parent, comment("reply")).unwrap();
    let bytes = doc.to_bytes().unwrap();
    let name = "word/commentsExtended.xml";
    let extended = xml(&bytes, name);
    let parsed = roxmltree::Document::parse(&extended).unwrap();
    let reply_entry = parsed
        .root_element()
        .children()
        .find(|n| n.attribute((W15, "paraIdParent")).is_some())
        .unwrap();
    let parent_attr = reply_entry
        .attributes()
        .find(|attr| attr.namespace() == Some(W15) && attr.name() == "paraIdParent")
        .unwrap();
    let reply_para = reply_entry.attribute((W15, "paraId")).unwrap();
    for replacement in ["00000000", reply_para] {
        let mut modified = extended.clone();
        modified.replace_range(parent_attr.range_value(), replacement);
        let input = with_part(&bytes, name, modified.as_bytes());
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        assert_matches!(
            doc.set_resolved(reply, true),
            Err(Error::InvalidCommentPart)
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
}

fn assert_thread_parts(bytes: &[u8], root: CommentId, replies: &[CommentId]) {
    let strings = [
        "comments.xml",
        "commentsExtended.xml",
        "commentsIds.xml",
        "commentsExtensible.xml",
    ]
    .map(|name| xml(bytes, &format!("word/{name}")));
    let [comments, ex, ids, extensible] = strings
        .each_ref()
        .map(|xml| roxmltree::Document::parse(xml).unwrap());
    let para = |id: CommentId| {
        comments
            .root_element()
            .children()
            .find(|n| n.attribute((W_NS, "id")) == Some(id.0.to_string().as_str()))
            .unwrap()
            .children()
            .rfind(|n| n.has_tag_name((W_NS, "p")))
            .unwrap()
            .attribute((W14, "paraId"))
            .unwrap()
    };
    for id in std::iter::once(root).chain(replies.iter().copied()) {
        let p = para(id);
        let entry = ex
            .root_element()
            .children()
            .find(|n| n.attribute((W15, "paraId")) == Some(p))
            .unwrap();
        assert_eq!(
            entry.attribute((W15, "paraIdParent")),
            (id != root).then(|| para(root))
        );
        assert_eq!(entry.attribute((W15, "done")), Some("1"));
        let durable = ids
            .root_element()
            .children()
            .find(|n| n.attribute((CID, "paraId")) == Some(p))
            .unwrap()
            .attribute((CID, "durableId"))
            .unwrap();
        let entry = extensible
            .root_element()
            .children()
            .find(|n| n.attribute((CEX, "durableId")) == Some(durable))
            .unwrap();
        assert_eq!(
            entry.attribute((CEX, "dateUtc")),
            Some("2021-05-20T02:57:00Z")
        );
    }
}

#[test]
fn legacy_companion_is_readable_but_never_edited() {
    let input = fixture("sdk-Comment010");
    // The package writer canonicalizes ZIP headers even on a no-op save.
    // Reopen its output to exercise strict whole-file equality on refusal.
    let baseline = Document::from_bytes(input.clone())
        .unwrap()
        .to_bytes()
        .unwrap();
    let mut d = Document::from_bytes(baseline.clone()).unwrap();
    let root = d.comments().find(|c| c.parent.is_none()).unwrap().id;
    assert!(d.comments().any(|c| c.parent.is_some()));
    assert_matches!(
        d.reply(root, comment("legacy")),
        Err(Error::LegacyCommentsExtended)
    );
    assert!(d.to_bytes().unwrap() == baseline);
    assert_matches!(
        d.set_resolved(root, true),
        Err(Error::LegacyCommentsExtended)
    );
    assert!(d.to_bytes().unwrap() == baseline);
    assert_matches!(d.delete_comment(root), Err(Error::LegacyCommentsExtended));
    assert!(d.to_bytes().unwrap() == baseline);
    d.add_comment(range(&d), comment("independent")).unwrap();
    let bytes = d.to_bytes().unwrap();
    assert_eq!(
        part(&input, "word/commentsExtended.xml"),
        part(&bytes, "word/commentsExtended.xml")
    );
    assert_eq!(
        part(&input, "word/people.xml"),
        part(&bytes, "word/people.xml")
    );
}

fn existing(body: &str, declarations: &str) -> Vec<u8> {
    let comments = format!(
        r#"<w:comments xmlns:w="{W_NS}" {declarations}><w:comment w:id="7" w:author="Existing"><w:p><w:r><w:t>First</w:t></w:r></w:p></w:comment></w:comments>"#
    );
    package(
        &document_xml(body),
        &[("comments", "comments.xml", &comments)],
    )
}

#[test]
fn reply_refuses_alternate_content_and_block_level_starts_atomically() {
    let start = "<w:commentRangeStart w:id='7'/>";
    let tail = "<w:r><w:t>Anchor</w:t></w:r><w:commentRangeEnd w:id='7'/><w:r><w:commentReference w:id='7'/></w:r>";
    let textbox = format!("<w:txbxContent><w:p>{start}{tail}</w:p></w:txbxContent>");
    let alternate = format!(
        "<w:p><w:r><compat:AlternateContent xmlns:compat='{MC}'><compat:Choice Requires='w'>{textbox}</compat:Choice><compat:Fallback>{textbox}</compat:Fallback></compat:AlternateContent></w:r></w:p>"
    );
    for (body, expected) in [
        (alternate.clone(), "mc:AlternateContent"),
        (
            format!("<w:p>{start}{tail}</w:p>{alternate}"),
            "mc:AlternateContent",
        ),
        (
            format!("{start}<w:p>{tail}</w:p>"),
            "a block-level comment range",
        ),
    ] {
        let input = existing(&body, "");
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        assert_matches!(
            doc.reply(CommentId(7), comment("reply")),
            Err(Error::UnsupportedContext { ancestor }) if ancestor == expected
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
}

#[test]
fn add_comment_accepts_table_cells_and_refuses_other_stories_atomically() {
    let header = docx(RUN, None, Some(RUN));
    let footer = common::story_xml("ftr", RUN);
    let footnotes = common::story_xml(
        "footnotes",
        &format!("<w:footnote w:id='1'>{RUN}</w:footnote>"),
    );
    let endnotes = common::story_xml(
        "endnotes",
        &format!("<w:endnote w:id='1'>{RUN}</w:endnote>"),
    );
    let notes = package(
        &document_xml(RUN),
        &[
            ("footer", "footer1.xml", &footer),
            ("footnotes", "footnotes.xml", &footnotes),
            ("endnotes", "endnotes.xml", &endnotes),
        ],
    );
    for (input, kind) in [
        (&header, StoryKind::Header),
        (&notes, StoryKind::Footer),
        (&notes, StoryKind::Footnotes),
        (&notes, StoryKind::Endnotes),
    ] {
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let at = doc
            .stories()
            .find(|story| story.id().kind() == kind)
            .unwrap()
            .paragraphs()
            .next()
            .unwrap()
            .run(0)
            .unwrap()
            .run_ref();
        assert_matches!(
            doc.add_comment(TextRange::new(at, at), comment("outside body")),
            Err(Error::UnsupportedContext { .. })
        );
        assert_eq!(&doc.to_bytes().unwrap(), input);
    }

    let body = format!("<w:tbl><w:tr><w:tc>{RUN}</w:tc></w:tr></w:tbl>");
    let mut doc = Document::from_bytes(docx(&body, None, None)).unwrap();
    doc.add_comment(range(&doc), comment("table cell")).unwrap();
}

#[test]
fn extensible_entries_require_a_comments_ids_mapping() {
    let original = fixture("poi-testComment");
    let ids = "word/commentsIds.xml";
    let rels = "word/_rels/document.xml.rels";
    let extensible = "word/commentsExtensible.xml";
    let inactive =
        xml(&original, rels).replace("/relationships/commentsIds", "/relationships/other");
    for input in [
        with_part(&original, rels, inactive.as_bytes()),
        with_part(&original, ids, b"<unknown/>"),
    ] {
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        doc.add_comment(range(&doc), comment("without mapping"))
            .unwrap();
        assert_eq!(
            part(&doc.to_bytes().unwrap(), extensible),
            part(&input, extensible)
        );
    }
}

#[test]
fn out_of_range_comment_years_are_rejected_atomically() {
    let input = docx(RUN, None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    for (date, offset) in [("+10000-01-01T00:00:00", 0), ("9999-12-31T23:59:00", -1)] {
        let mut new = comment("bad date");
        new.date = Some(LocalDateTime {
            wall_clock: date.parse().unwrap(),
            utc_offset_minutes: offset,
        });
        assert_matches!(
            doc.add_comment(range(&doc), new),
            Err(Error::InvalidValue {
                what: "date year",
                ..
            })
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
}

#[test]
fn missing_parent_address_or_ignorable_binding_is_atomic() {
    let body = "<w:p><w:r><w:t>Anchor</w:t></w:r><w:r><w:commentReference w:id='7'/></w:r></w:p>";
    for decls in [
        "",
        &format!(r#"xmlns:w14="{W14}" xmlns:mc="{MC}" mc:Ignorable="other""#),
        &format!(r#"xmlns:x="{W14}" xmlns:w14="urn:other" xmlns:mc="{MC}" mc:Ignorable="w14""#),
    ] {
        let input = existing(body, decls);
        let mut d = Document::from_bytes(input.clone()).unwrap();
        assert_matches!(
            d.reply(CommentId(7), comment("reply")),
            Err(Error::ParentNotAddressable)
        );
        assert_eq!(d.to_bytes().unwrap(), input);
        assert_matches!(
            d.set_resolved(CommentId(7), true),
            Err(Error::ParentNotAddressable)
        );
        assert_eq!(d.to_bytes().unwrap(), input);
        let new_id = d.add_comment(range(&d), comment("independent")).unwrap();
        let bytes = d.to_bytes().unwrap();
        let xml = xml(&bytes, "word/comments.xml");
        let tree = roxmltree::Document::parse(&xml).unwrap();
        let para = tree
            .root_element()
            .children()
            .next_back()
            .unwrap()
            .first_element_child()
            .unwrap();
        assert!(para.attribute((W14, "paraId")).is_some());
        assert_eq!(para.attribute((W14, "textId")), Some("77777777"));
        let prefix = para.attribute((MC, "Ignorable")).unwrap();
        assert_eq!(para.lookup_namespace_uri(Some(prefix)), Some(W14));
        assert!(d.comments().any(|c| c.id == new_id));
    }
    let input = existing(
        body,
        &format!(
            r#"xmlns:alternate="{W14}" xmlns:w14="urn:wrong" xmlns:mc="{MC}" mc:Ignorable="alternate""#
        ),
    );
    let mut d = Document::from_bytes(input).unwrap();
    let reply = d.reply(CommentId(7), comment("reply")).unwrap();
    d.set_resolved(CommentId(7), true).unwrap();
    let reply = d.comments().find(|c| c.id == reply).unwrap();
    assert_eq!(reply.parent, Some(CommentId(7)));
    let anchor = reply.anchor.unwrap();
    assert!(anchor.start.is_none() && anchor.end.is_none() && anchor.reference.is_some());
    assert!(reply.done);
}

#[test]
fn deleting_reference_keeps_run_content_and_rejects_revisions() {
    let body = "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>survive</w:t><w:commentReference w:id='7'/></w:r><w:r><w:rPr/><w:commentReference w:id='7'/></w:r></w:p>";
    let input = existing(body, "");
    let mut d = Document::from_bytes(input.clone()).unwrap();
    d.delete_comment(CommentId(7)).unwrap();
    let actual = xml(&d.to_bytes().unwrap(), "word/document.xml");
    assert_eq!(
        actual,
        xml(&input, "word/document.xml")
            .replace("<w:r><w:rPr/><w:commentReference w:id='7'/></w:r>", "")
            .replace("<w:commentReference w:id='7'/>", "")
    );
    for anchor in [
        "<w:commentRangeStart w:id='7'/>",
        "<w:commentRangeEnd w:id='7'/>",
        "<w:r><w:commentReference w:id='7'/></w:r>",
    ] {
        let body = format!("<w:p><w:ins w:id='8' w:author='other'>{anchor}</w:ins></w:p>");
        let input = existing(&body, "");
        let mut d = Document::from_bytes(input.clone()).unwrap();
        assert_matches!(
            d.delete_comment(CommentId(7)),
            Err(Error::InsideRevision { .. })
        );
        assert_eq!(d.to_bytes().unwrap(), input);
    }
}

#[test]
fn comment_write_and_boundary_gates_are_atomic() {
    for edit in ["readOnly", "forms", "trackedChanges", "comments", "none"] {
        let settings =
            format!("<w:documentProtection w:edit='{edit}' w:enforcement='1'/><w:trackRevisions/>");
        let input = docx(RUN, Some(&settings), None);
        let mut d = Document::from_bytes(input.clone()).unwrap();
        let result = d.add_comment(range(&d), comment("protected"));
        if matches!(edit, "comments" | "none") {
            let id = result.unwrap();
            d.reply(id, comment("allowed")).unwrap();
            d.set_resolved(id, true).unwrap();
            d.delete_comment(id).unwrap();
        } else {
            assert_matches!(result, Err(Error::DocumentProtected { .. }));
            assert_eq!(d.to_bytes().unwrap(), input);
            assert_matches!(
                d.reply(CommentId(7), comment("protected")),
                Err(Error::DocumentProtected { .. })
            );
            assert_eq!(d.to_bytes().unwrap(), input);
            assert_matches!(
                d.set_resolved(CommentId(7), true),
                Err(Error::DocumentProtected { .. })
            );
            assert_eq!(d.to_bytes().unwrap(), input);
            assert_matches!(
                d.delete_comment(CommentId(7)),
                Err(Error::DocumentProtected { .. })
            );
            assert_eq!(d.to_bytes().unwrap(), input);
        }
    }
    for (body, gate) in [
        (
            "<w:fldSimple w:instr='PAGE'><w:r><w:t>1</w:t></w:r></w:fldSimple>",
            1,
        ),
        (
            "<w:ins w:id='9' w:author='Other'><w:r><w:t>revision</w:t></w:r></w:ins>",
            2,
        ),
        ("<w:smartTag><w:r><w:t>wrapped</w:t></w:r></w:smartTag>", 3),
        ("<w:r><w:t>drawing</w:t><w:drawing/></w:r>", 4),
    ] {
        let input = docx(&format!("<w:p>{body}</w:p>"), None, None);
        let mut d = Document::from_bytes(input.clone()).unwrap();
        let at = run(&d, 0, 0);
        let result = d.add_comment(TextRange::new(at, at), comment("gate"));
        match gate {
            1 => assert_matches!(result, Err(Error::InsideField)),
            2 => assert_matches!(result, Err(Error::InsideRevision { .. })),
            3 => assert_matches!(result, Err(Error::UnsupportedContext { .. })),
            4 => assert_matches!(result, Err(Error::RunNotTextOnly { .. })),
            _ => unreachable!(),
        }
        assert_eq!(d.to_bytes().unwrap(), input);
    }
}

#[test]
fn modern_ignorable_binding_is_checked_before_any_part_is_committed() {
    let fixture = fixture("poi-testComment");
    let name = "word/commentsExtended.xml";
    let ex = xml(&fixture, name).replace("mc:Ignorable=\"w14 w15 ", "mc:Ignorable=\"w14 ");
    let input = with_part(&fixture, name, ex.as_bytes());
    let mut d = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(
        d.add_comment(range(&d), comment("ignored extension")),
        Err(Error::NotIgnorable(W15))
    );
    assert_eq!(d.to_bytes().unwrap(), input);
}

#[test]
fn corpus_comment_edits_round_trip_without_new_inconsistencies() {
    let mut added = 0;
    let mut resolved = 0;
    let mut deleted = 0;
    let mut expected_refusals = 0;
    for stem in openable() {
        let input = fixture(stem);
        let mut d = Document::from_bytes(input.clone()).unwrap();
        let baseline = d.to_bytes().unwrap();
        let existing = d.comments().next().map(|c| c.id);
        let first = d
            .stories()
            .filter(|story| story.id().kind() == StoryKind::Main)
            .flat_map(|story| story.paragraphs())
            .flat_map(|p| p.runs())
            .find(|r| r.check_settable().is_ok() && !r.text().trim().is_empty())
            .map(|r| r.run_ref());
        if let Some(first) = first {
            match d.add_comment(TextRange::new(first, first), comment("Corpus parent")) {
                Ok(root) => {
                    added += 1;
                    let added_bytes = d.to_bytes().unwrap();
                    match d.reply(root, comment("Corpus reply")) {
                        Ok(reply) => {
                            d.set_resolved(root, true)
                                .unwrap_or_else(|e| panic!("{stem}: {e}"));
                            resolved += 1;
                            assert!(
                                d.comments()
                                    .any(|c| c.id == reply && c.done && c.parent == Some(root))
                            );
                        }
                        Err(Error::LegacyCommentsExtended) => {
                            expected_refusals += 1;
                            assert_eq!(d.to_bytes().unwrap(), added_bytes, "{stem}");
                        }
                        Err(e) => panic!("{stem}: {e}"),
                    }
                    assert!(d.comments().any(|c| c.id == root));
                }
                Err(Error::DocumentProtected { .. }) => {
                    expected_refusals += 1;
                    assert!(
                        d.to_bytes().unwrap() == baseline,
                        "{stem}: refusal changed the package"
                    );
                }
                Err(e) => panic!("{stem}: {e}"),
            }
        } else {
            eprintln!("{stem}: no eligible runs");
        }
        let output = d.to_bytes().unwrap();
        check_consistency(&output, stem);
        if let Some(id) = existing {
            // Independently exercise deletion against the original package.
            let mut d = Document::from_bytes(input.clone()).unwrap();
            match d.delete_comment(id) {
                Ok(()) => {
                    deleted += 1;
                    let bytes = d.to_bytes().unwrap();
                    check_consistency(&bytes, stem);
                    assert!(
                        Document::from_bytes(bytes)
                            .unwrap()
                            .comments()
                            .all(|c| c.id != id)
                    );
                }
                Err(Error::LegacyCommentsExtended | Error::InsideRevision { .. }) => {
                    expected_refusals += 1;
                    assert!(
                        d.to_bytes().unwrap() == baseline,
                        "{stem}: refusal changed the package"
                    );
                }
                Err(e) => panic!("{stem}: {e}"),
            }
        }
    }
    eprintln!(
        "corpus: {added} additions, {resolved} replies + resolutions, {deleted} deletions, {expected_refusals} expected refusals"
    );
    assert!(added > 0 && resolved > 0 && deleted > 0);
}

fn check_consistency(bytes: &[u8], stem: &str) {
    let d = Document::from_bytes(bytes.to_vec()).unwrap_or_else(|e| panic!("{stem}: {e}"));
    let mut ids = BTreeSet::new();
    assert!(
        d.comments().all(|c| ids.insert(c.id)),
        "{stem}: duplicate comment id"
    );
    if !part_names(bytes).contains(&"word/commentsExtended.xml".to_owned()) {
        return;
    }
    let comments = xml(bytes, "word/comments.xml");
    let extended = xml(bytes, "word/commentsExtended.xml");
    let comments = roxmltree::Document::parse(&comments).unwrap();
    let extended = roxmltree::Document::parse(&extended).unwrap();
    let paras = comments
        .root_element()
        .children()
        .filter(|n| n.has_tag_name((W_NS, "comment")))
        .filter_map(|n| n.children().rfind(|p| p.has_tag_name((W_NS, "p"))))
        .filter_map(|p| p.attribute((W14, "paraId")))
        .collect::<BTreeSet<_>>();
    let ns = extended.root_element().tag_name().namespace().unwrap();
    for entry in extended
        .root_element()
        .children()
        .filter(|n| n.has_tag_name((ns, "commentEx")))
    {
        let para = entry.attribute((ns, "paraId")).unwrap();
        assert!(paras.contains(para), "{stem}: orphan commentEx for {para}");
    }
}

#[test]
fn people_are_appended_once_and_survive_thread_deletion() {
    let people = format!(
        r#"<custom:people xmlns:custom="{W15}" xmlns:mc="{MC}" mc:Ignorable="custom"><custom:person custom:author="Legacy"><custom:presenceInfo custom:providerId="None" custom:userId="Legacy"/></custom:person></custom:people>"#
    );
    let input = package(&document_xml(RUN), &[("people", "people.xml", &people)]);
    let rels = xml(&input, "word/_rels/document.xml.rels").replace(
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/people",
        "http://schemas.microsoft.com/office/2011/relationships/people",
    );
    let input = with_part(&input, "word/_rels/document.xml.rels", rels.as_bytes());
    let mut d = Document::from_bytes(input).unwrap();
    let parent = d.add_comment(range(&d), comment("same person")).unwrap();
    d.reply(parent, comment("same person again")).unwrap();
    d.delete_comment(parent).unwrap();
    let bytes = d.to_bytes().unwrap();
    let after = xml(&bytes, "word/people.xml");
    let tree = roxmltree::Document::parse(&after).unwrap();
    let people = tree
        .root_element()
        .children()
        .filter(|n| n.has_tag_name((W15, "person")))
        .collect::<Vec<_>>();
    assert_eq!(people.len(), 2);
    assert_eq!(people[0].attribute((W15, "author")), Some("Legacy"));
    assert_eq!(people[1].attribute((W15, "author")), Some("Ada & Grace"));
    let presence = people[1].first_element_child().unwrap();
    assert_eq!(presence.attribute((W15, "userId")), Some("Ada & Grace"));
    assert_eq!(presence.attribute((W15, "providerId")), Some("None"));
    assert!(
        !part_names(&bytes)
            .iter()
            .any(|p| p.contains("commentsIds") || p.contains("commentsExtensible"))
    );
}

#[test]
fn range_can_split_a_hyperlink_run_without_changing_link_text() {
    let input = docx(
        "<w:p><w:hyperlink w:anchor='target'><w:r><w:t>Link target</w:t></w:r></w:hyperlink></w:p>",
        None,
        None,
    );
    let mut doc = Document::from_bytes(input).unwrap();
    let at = run(&doc, 0, 0);
    let parent = doc
        .add_comment(TextRange::with_offsets(at, 5, at, 11), comment("link"))
        .unwrap();
    doc.reply(parent, comment("reply")).unwrap();
    assert_eq!(doc.paragraph(0).unwrap().text(), "Link target");
    doc.delete_comment(parent).unwrap();
    assert_eq!(doc.paragraph(0).unwrap().text(), "Link target");
    assert!(doc.comments().next().is_none());
}

#[test]
fn deletion_removes_anchors_in_existing_content_controls() {
    let body = "<w:sdt><w:sdtPr/><w:sdtContent><w:commentRangeStart w:id='7'/><w:p><w:r><w:t>Keep this</w:t></w:r><w:commentRangeEnd w:id='7'/><w:r><w:commentReference w:id='7'/></w:r></w:p></w:sdtContent></w:sdt>";
    let input = existing(body, "");
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    doc.delete_comment(CommentId(7)).unwrap();
    let expected = xml(&input, "word/document.xml")
        .replace("<w:commentRangeStart w:id='7'/>", "")
        .replace("<w:commentRangeEnd w:id='7'/>", "")
        .replace("<w:r><w:commentReference w:id='7'/></w:r>", "");
    assert_eq!(xml(&doc.to_bytes().unwrap(), "word/document.xml"), expected);
}
