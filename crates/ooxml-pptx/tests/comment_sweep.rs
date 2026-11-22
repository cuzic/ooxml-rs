//! ADR-0019 §11.6. Run with COMMENT_CORPUS pointing to the pinned corpus roots
//! (as a path list). Committed fixtures are always included.
//! Set COMMENT_CORPUS_ORACLE_DIR to retain the successful input/output
//! pairs for `tests/oracle/comments.py --sweep DIR`.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ooxml_core::{Package, opc::rels_part_name};
use ooxml_pptx::{
    Anchor, AuthorIdentity, CommentAnchor, CommentAuthor, Error, NewComment, Presentation,
    ShapeKind,
};
use sha2::{Digest, Sha256};

const MODERN: &str = "http://schemas.microsoft.com/office/2018/10/relationships/comments";
const AUTHORS: &str = "http://schemas.microsoft.com/office/2018/10/relationships/authors";
const LEGACY: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";

fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    for item in std::fs::read_dir(path).unwrap() {
        let item = item.unwrap();
        let kind = item.file_type().unwrap();
        if kind.is_dir() {
            collect(&item.path(), files);
        } else if kind.is_file()
            && item
                .path()
                .extension()
                .is_some_and(|v| v.eq_ignore_ascii_case("pptx"))
        {
            files.push(item.path());
        }
    }
}

fn new_comment() -> NewComment {
    NewComment {
        author: CommentAuthor {
            name: "Corpus Author".into(),
            identity: AuthorIdentity::Local,
            initials: None,
        },
        text: "Corpus comment".into(),
        created: Some(std::time::UNIX_EPOCH),
        lang: None,
    }
}

fn variant(error: &Error) -> &str {
    match error {
        Error::ShapeNotFound => "ShapeNotFound",
        Error::UnsupportedContent(_) => "UnsupportedContent",
        Error::CreationIdsExhausted => "CreationIdsExhausted",
        Error::CommentPartNotEditable { .. } => "CommentPartNotEditable",
        Error::AuthorsPartNotEditable { .. } => "AuthorsPartNotEditable",
        _ => "Unexpected",
    }
}

fn check_locality(input_bytes: Vec<u8>, output_bytes: &[u8], output: &Package, slide: &str) {
    let input = Package::from_bytes(input_bytes.clone()).unwrap();
    let main = output.main_document_part().unwrap();
    let slide_rels = output.relationships(slide).unwrap();
    let comments = slide_rels
        .resolve(slide_rels.by_type(MODERN).next().unwrap())
        .unwrap();
    let main_rels = output.relationships(&main).unwrap();
    let authors = main_rels
        .resolve(main_rels.by_type(AUTHORS).next().unwrap())
        .unwrap();
    let mut allowed = vec![comments.as_str(), authors.as_str(), "[Content_Types].xml"];
    let slide_rels_part = rels_part_name(Some(slide));
    let main_rels_part = rels_part_name(Some(&main));
    if !input.contains(&comments) {
        allowed.extend([slide, &slide_rels_part]);
    }
    if !input.contains(&authors) {
        allowed.push(&main_rels_part);
    }
    let mut input_zip = zip::ZipArchive::new(std::io::Cursor::new(input_bytes)).unwrap();
    let mut output_zip = zip::ZipArchive::new(std::io::Cursor::new(output_bytes)).unwrap();
    for part in input.part_names().filter(|part| !allowed.contains(part)) {
        // The sweep includes packages whose untouched entries have bad CRCs.
        // ZIP raw readers compare what the lossless writer passes through.
        let mut original = input_zip.by_name_raw(part.as_bytes()).unwrap();
        let mut saved = output_zip.by_name_raw(part.as_bytes()).unwrap();
        assert_eq!(original.crc32(), saved.crc32(), "{part}");
        let mut original_bytes = Vec::new();
        let mut saved_bytes = Vec::new();
        std::io::Read::read_to_end(&mut original, &mut original_bytes).unwrap();
        std::io::Read::read_to_end(&mut saved, &mut saved_bytes).unwrap();
        assert_eq!(original_bytes, saved_bytes, "changed {part}");
    }
    for part in output.part_names().filter(|part| !input.contains(part)) {
        assert!(allowed.contains(&part), "unexpected new part {part}");
    }
}

#[test]
#[ignore = "requires the external pinned ADR-0019 corpora"]
fn add_comments_to_corpus() {
    let roots = std::env::var_os("COMMENT_CORPUS").expect("set COMMENT_CORPUS");
    let artifacts = std::env::var_os("COMMENT_CORPUS_ORACLE_DIR").map(PathBuf::from);
    if let Some(dir) = &artifacts {
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut cases = Vec::new();
    let mut files = Vec::new();
    for root in std::env::split_paths(&roots)
        .chain([Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures/pptx")])
    {
        collect(&root, &mut files);
    }
    files.sort();
    let allowlist: BTreeSet<_> = include_str!("data/comment_sweep_allowlist.txt")
        .lines()
        .map(|line| line.split('#').next().unwrap().trim())
        .filter(|line| !line.is_empty())
        .collect();
    let mut actual = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let (mut unopened, mut unavailable, mut legacy, mut added) = (0, 0, 0, 0);
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if !seen.insert(hash.clone()) {
            continue;
        }
        let mut presentation = match Presentation::from_bytes(bytes) {
            Ok(deck) => deck,
            Err(error) => {
                unopened += 1;
                eprintln!(
                    "open skip {hash} {}: {error}",
                    path.file_name().unwrap().display()
                );
                continue;
            }
        };
        if presentation.slide_count() == 0 {
            unavailable += 1;
            continue;
        }
        let pkg = presentation.package();
        if pkg.part_names().any(|part| {
            pkg.relationships(part)
                .is_ok_and(|rels| rels.by_type(LEGACY).next().is_some())
        }) {
            legacy += 1;
            continue;
        }
        let shape = match presentation.slide(0) {
            Ok(slide) => slide
                .shapes()
                .into_iter()
                .find(|shape| shape.depth() == 0 && *shape.kind() == ShapeKind::Shape)
                .map(|shape| shape.id())
                .transpose(),
            Err(error) => {
                unavailable += 1;
                eprintln!(
                    "slide skip {hash} {}: {error}",
                    path.file_name().unwrap().display()
                );
                continue;
            }
        };
        for (step, anchor) in [
            ("slide", Ok(CommentAnchor::Slide)),
            (
                "shape",
                shape.and_then(|id| id.map(CommentAnchor::Shape).ok_or(Error::ShapeNotFound)),
            ),
        ] {
            let before = presentation.to_bytes().unwrap();
            let expected = anchor.as_ref().ok().map(|anchor| match anchor {
                CommentAnchor::Slide => Anchor::Slide,
                CommentAnchor::Shape(id) => Anchor::Shapes(vec![*id]),
            });
            let result =
                anchor.and_then(|anchor| presentation.add_comment(0, anchor, new_comment()));
            match result {
                Ok(id) => {
                    let saved = presentation.to_bytes().unwrap();
                    let mut reopened = Presentation::from_bytes(saved.clone()).unwrap();
                    let view = reopened.deck().unwrap();
                    let slide = view.slide(0).unwrap();
                    let comments = slide.comments().unwrap();
                    let entry = comments
                        .iter()
                        .find(|entry| entry.id == Some(id.as_str()))
                        .unwrap();
                    assert_eq!(Some(&entry.anchor), expected.as_ref());
                    assert_eq!(entry.text, "Corpus comment");
                    assert_eq!(entry.author.as_ref().unwrap().name, "Corpus Author");
                    let source = presentation.slide(0).unwrap().info().part_name.clone();
                    if let Some(dir) = &artifacts {
                        let case = format!("{hash}-{step}");
                        std::fs::write(dir.join(format!("{case}.input.pptx")), &before).unwrap();
                        std::fs::write(dir.join(format!("{case}.pptx")), &saved).unwrap();
                        cases.push((case, source.clone()));
                    }
                    check_locality(before, &saved, reopened.package(), &source);
                    added += 1;
                }
                Err(error) => {
                    assert_eq!(
                        before,
                        presentation.to_bytes().unwrap(),
                        "{hash} {step}: {error}"
                    );
                    let key = format!("{hash} {step} {}", variant(&error));
                    eprintln!(
                        "refusal {key} # {}: {error}",
                        path.file_name().unwrap().display()
                    );
                    actual.insert(key);
                }
            }
        }
    }
    if let Some(dir) = &artifacts {
        std::fs::write(dir.join("sweep.json"), serde_json::to_vec(&cases).unwrap()).unwrap();
    }
    let actual: BTreeSet<_> = actual.iter().map(String::as_str).collect();
    let extra: Vec<_> = actual.difference(&allowlist).collect();
    let stale: Vec<_> = allowlist.difference(&actual).collect();
    println!(
        "{} distinct packages; {unopened} open skips; {unavailable} no readable first slide; {legacy} legacy skips; {added} adds; {} refusals; {} unallowlisted; {} stale",
        seen.len(),
        actual.len(),
        extra.len(),
        stale.len()
    );
    assert!(
        extra.is_empty() && stale.is_empty(),
        "unallowlisted: {extra:?}; stale: {stale:?}"
    );
}
