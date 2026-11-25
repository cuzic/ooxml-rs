//! Stage 2 equivalence checks. Kept here to inspect the internal AC bit and
//! exercise start-tag rendering while the edited child list is still deferred.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{RawAttr, RawItem, RawNode, XmlDocument, mark_path_dirty, node_at};
use crate::{Error, Package, ParseOptions, QName, XName};

#[path = "../../tests/support/fixture_packages.rs"]
mod fixture_packages;

type NameSet = Vec<(String, String)>;

#[derive(Default)]
struct Oracle {
    // ParseOptions requires static slices. Share the test-only allocations
    // across parts that use the same set of names.
    name_sets: BTreeMap<NameSet, &'static [QName]>,
    single_names: BTreeMap<(String, String), &'static [QName]>,
    // Parts with more distinct names than this skip the per-name run.
    max_single_names: usize,
}

fn count_names<'a>(node: &'a RawNode, names: &mut BTreeMap<(&'a str, &'a str), usize>) {
    let name = node.name();
    *names.entry((name.ns(), name.local())).or_default() += 1;
    for child in node.elements() {
        count_names(child, names);
    }
}

fn compare_tree(full: &RawNode, lazy: &RawNode, context: &str) {
    assert_eq!(full.name(), lazy.name(), "{context}: element name");
    assert_eq!(full.attrs(), lazy.attrs(), "{context}: attributes");
    assert_eq!(full.span(), lazy.span(), "{context}: element span");
    assert_eq!(
        full.may_have_alternate_content(),
        lazy.may_have_alternate_content(),
        "{context}: AC bit"
    );
    let (full_children, lazy_children) = (full.children(), lazy.children());
    assert_eq!(
        full_children.len(),
        lazy_children.len(),
        "{context}: child count"
    );
    for (full, lazy) in full_children.iter().zip(lazy_children) {
        match (full, lazy) {
            (RawItem::Element(full), RawItem::Element(lazy)) => compare_tree(full, lazy, context),
            (RawItem::Text(full), RawItem::Text(lazy)) => {
                assert_eq!(
                    (full.value(), full.is_cdata(), full.span()),
                    (lazy.value(), lazy.is_cdata(), lazy.span()),
                    "{context}: text"
                );
            }
            (RawItem::Markup(full), RawItem::Markup(lazy)) => {
                assert_eq!(
                    (full.kind(), full.raw()),
                    (lazy.kind(), lazy.raw()),
                    "{context}: markup"
                );
            }
            _ => panic!("{context}: different child kinds"),
        }
    }
}

fn compare_outcomes(input: &[u8], base: &ParseOptions, deferred: &ParseOptions, context: &str) {
    match (
        XmlDocument::parse_with(input, base),
        XmlDocument::parse_with(input, deferred),
    ) {
        (Ok(full), Ok(lazy)) => {
            assert_eq!(lazy.serialize().as_bytes(), input, "{context}: T1");
            compare_tree(full.root(), lazy.root(), context);
        }
        (Err(full), Err(lazy)) => assert_eq!(format!("{full:?}"), format!("{lazy:?}"), "{context}"),
        (full, lazy) => panic!(
            "{context}: full error {:?}, deferred error {:?}",
            full.err(),
            lazy.err()
        ),
    }
}

fn compare_attribute_edits(
    full: &mut XmlDocument,
    lazy: &mut XmlDocument,
    input: &[u8],
    context: &str,
) {
    // Prefer a nested target with source content. Descending expands the
    // parent but must leave the target's own children deferred.
    let child = full
        .root()
        .indexed_elements()
        .find(|(_, n)| !n.children().is_empty());
    let path: Vec<usize> = child.map(|(index, _)| index).into_iter().collect();
    let span = node_at(full.root(), &path).span().unwrap();
    let outside = (&input[..span.start], &input[span.end()..]);
    let index = node_at(full.root(), &path).attrs().len();
    let mut added = "stage2-oracle".to_owned();
    while node_at(full.root(), &path)
        .attrs()
        .iter()
        .any(|a| a.name().local() == added)
    {
        added.push('_');
    }

    // Each mutation renders independently, with the target still deferred.
    for (step, value) in [Some("v"), Some("e&<\"\r\n"), None].into_iter().enumerate() {
        for doc in [&mut *full, &mut *lazy] {
            let node = mark_path_dirty(&mut doc.root, &path);
            match value {
                Some(value) if step == 0 => node.push_attr(RawAttr {
                    name: XName::new(Arc::from(""), &added, None),
                    value: value.into(),
                }),
                Some(value) => node.attrs[index].value = value.into(),
                None => node.remove_attr(index),
            }
            node.mark_own_dirty();
        }
        let rendered = lazy.serialize();
        assert!(
            node_at(lazy.root(), &path).is_deferred(),
            "{context}: attribute edit expanded target"
        );
        assert_eq!(rendered, full.serialize(), "{context}: attribute edit");
        assert!(
            rendered.as_bytes().starts_with(outside.0) && rendered.as_bytes().ends_with(outside.1),
            "{context}: T2"
        );
    }
}

fn release_all(node: &mut RawNode) {
    for child in node
        .children_slice_mut()
        .iter_mut()
        .filter_map(RawItem::as_element_mut)
    {
        release_all(child);
    }
    node.release_children();
}

impl Oracle {
    // Defer each element name on its own. Unlike the all-names run, the
    // ancestors that declare prefixes stay built, so a snapshot shared between
    // deferred siblings has to be invalidated by them.
    fn compare_single_names(
        &mut self,
        input: &[u8],
        full: &XmlDocument,
        counts: &BTreeMap<(&str, &str), usize>,
        context: &str,
    ) {
        let base = ParseOptions::default();
        for (&(ns, local), &count) in counts {
            let one = *self
                .single_names
                .entry((ns.to_owned(), local.to_owned()))
                .or_insert_with_key(|(ns, local)| {
                    Vec::from([QName::new(ns.clone().leak(), local.clone().leak())]).leak()
                });
            let context = format!("{context}: only {one:?} deferred");
            let options = base.with_defer(one).with_count(one);
            let mut lazy = XmlDocument::parse_with(input, &options)
                .unwrap_or_else(|err| panic!("{context}: {err}"));
            assert_eq!(lazy.serialize().as_bytes(), input, "{context}: T1");
            assert_eq!(lazy.census(one[0]), Some(count), "{context}: census");
            compare_tree(full.root(), lazy.root(), &context);
            release_all(&mut lazy.root);
            assert_eq!(lazy.serialize().as_bytes(), input, "{context}: release");
            compare_tree(full.root(), lazy.root(), &context);
        }
    }

    fn compare(&mut self, input: &[u8], context: &str) -> bool {
        let base = ParseOptions::default();
        let Ok(mut full) = XmlDocument::parse(input) else {
            // Malformed seeds cannot supply a trusted list of names. Include
            // the synthetic seed root and common OOXML part roots.
            const SEED_ROOTS: &[QName] = &[
                QName::new("", "r"),
                QName::new(
                    "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
                    "worksheet",
                ),
                QName::new(
                    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                    "document",
                ),
                QName::new(
                    "http://schemas.openxmlformats.org/presentationml/2006/main",
                    "sld",
                ),
            ];
            let options = base.with_defer(SEED_ROOTS);
            compare_outcomes(input, &base, &options, context);
            return false;
        };
        let mut counts = BTreeMap::new();
        count_names(full.root(), &mut counts);
        let key = counts
            .keys()
            .map(|&(ns, local)| (ns.to_owned(), local.to_owned()))
            .collect();
        let all = *self.name_sets.entry(key).or_insert_with_key(|key| {
            key.iter()
                .map(|(ns, local)| QName::new(ns.clone().leak(), local.clone().leak()))
                .collect::<Vec<_>>()
                .leak()
        });
        let options = base.with_defer(all).with_count(all);
        let mut lazy = XmlDocument::parse_with(input, &options)
            .unwrap_or_else(|err| panic!("{context}: {err}"));
        assert!(
            lazy.root().is_deferred(),
            "{context}: root was built eagerly"
        );
        assert_eq!(lazy.serialize().as_bytes(), input, "{context}: T1");
        for &name in all {
            assert_eq!(
                lazy.census(name),
                Some(counts[&(name.ns, name.local)]),
                "{context}: census of {name:?}"
            );
        }

        if counts.len() <= self.max_single_names {
            self.compare_single_names(input, &full, &counts, context);
        }

        let mut edited_full = full.clone();
        compare_attribute_edits(&mut edited_full, &mut lazy, input, context);
        compare_tree(edited_full.root(), lazy.root(), context);
        release_all(&mut lazy.root);
        assert_eq!(
            lazy.serialize(),
            edited_full.serialize(),
            "{context}: after release"
        );
        compare_tree(edited_full.root(), lazy.root(), context);

        let mut clean = XmlDocument::parse_with(input, &options).unwrap();
        clean.force_rerender();
        full.force_rerender();
        assert_eq!(
            clean.serialize(),
            full.serialize(),
            "{context}: full re-render"
        );
        assert!(clean.root().structurally_eq(full.root()), "{context}: T1b");

        if let Some((close, _)) = clean.root().span().unwrap().close {
            let open = clean.root().span().unwrap().open_end;
            for (position, bad) in [
                (open, b"&unknown;".as_slice()),
                (close, b"<unclosed>"),
                (open + (close - open) / 2, b"<q:unbound/>"),
            ] {
                let mutated = [&input[..position], bad, &input[position..]].concat();
                for base in [base, base.with_max_depth(4)] {
                    let deferred = base.with_defer(all).with_count(all);
                    compare_outcomes(
                        &mutated,
                        &base,
                        &deferred,
                        &format!("{context}: mutation at {position}"),
                    );
                }
            }
        }
        true
    }
}

fn contains_dtd(input: &[u8]) -> bool {
    input
        .windows(b"<!DOCTYPE".len())
        .any(|window| window == b"<!DOCTYPE")
}

#[test]
fn deferred_parser_matches_on_every_fixture_part() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference");
    let mut files = Vec::new();
    fixture_packages::collect(&root.join("fixtures"), &mut files);
    fixture_packages::collect(&root.join("round_trip_corpus"), &mut files);
    files.sort();

    let mut oracle = Oracle {
        max_single_names: 6,
        ..Oracle::default()
    };
    let mut checked = 0;
    for file in files {
        let pkg = match Package::from_bytes(std::fs::read(&file).unwrap()) {
            Ok(pkg) => pkg,
            Err(Error::EncryptedPackage | Error::StrictNotSupported(_)) => continue,
            Err(err) => panic!("{}: {err}", file.display()),
        };
        for name in pkg.part_names().filter(|name| {
            name.ends_with(".xml")
                || name.ends_with(".rels")
                || name.ends_with(".vml")
                || pkg.content_type(name).is_some_and(|content_type| {
                    content_type.ends_with("+xml") || content_type.ends_with("/xml")
                })
        }) {
            let input = pkg.read_part(name).unwrap();
            if !contains_dtd(&input) {
                checked +=
                    usize::from(oracle.compare(&input, &format!("{} {name}", file.display())));
            }
        }
    }
    assert!(checked > 1000, "compared {checked} fixture parts");
}

#[test]
fn deferred_parser_matches_on_xml_fuzz_seeds() {
    let corpus = std::env::var_os("OOXML_XML_SEEDS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/xml_seeds"),
        PathBuf::from,
    );
    let mut oracle = Oracle {
        max_single_names: usize::MAX,
        ..Oracle::default()
    };
    let mut replayed = 0;
    for entry in std::fs::read_dir(&corpus)
        .unwrap_or_else(|err| panic!("XML seed replay unavailable at {}: {err}", corpus.display()))
    {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_file() {
            continue;
        }
        let path = entry.path();
        let input = std::fs::read(&path).unwrap();
        if contains_dtd(&input) {
            continue;
        }
        oracle.compare(&input, &path.display().to_string());
        replayed += 1;
    }
    assert!(
        replayed > 0,
        "XML seed replay found no eligible inputs in {}",
        corpus.display()
    );
    eprintln!("Replayed {replayed} XML fuzz seeds");
}
