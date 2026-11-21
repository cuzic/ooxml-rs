//! ADR-0019 §11.6. Run with COMMENT_CORPUS pointing to the pinned corpus roots
//! (as a path list). Committed fixtures are always included.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ooxml_pptx::{
    AuthorIdentity, CommentAnchor, CommentAuthor, Error, NewComment, Presentation, ShapeKind,
};
use sha2::{Digest, Sha256};

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

#[test]
#[ignore = "requires the external pinned ADR-0019 corpora"]
fn add_comments_to_corpus() {
    let roots = std::env::var_os("COMMENT_CORPUS").expect("set COMMENT_CORPUS");
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
            let result =
                anchor.and_then(|anchor| presentation.add_comment(0, anchor, new_comment()));
            match result {
                Ok(id) => {
                    let mut reopened =
                        Presentation::from_bytes(presentation.to_bytes().unwrap()).unwrap();
                    let view = reopened.deck().unwrap();
                    let slide = view.slide(0).unwrap();
                    let comments = slide.comments().unwrap();
                    let entry = comments
                        .iter()
                        .find(|entry| entry.id == Some(id.as_str()))
                        .unwrap();
                    assert_eq!(entry.text, "Corpus comment");
                    assert_eq!(entry.author.as_ref().unwrap().name, "Corpus Author");
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
