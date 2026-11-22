"""Compare real lens outcomes to the ADR-0020 spike, pair by pair.

Run from the repository root with python3 -I spikes/ac-rank/check_core.py
DIR_OR_FILE... . With just reference/fixtures, check the committed subset.
Use the pinned POI directories from README.md to reproduce the full counts.
"""
import collections
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile

SPIKE = runpy.run_path(str(Path(__file__).with_name("ac_rank.py")))
ROOT = Path(__file__).resolve().parents[2]

RUST = """
use ooxml_core::{ChildOf, ElementSpec, Error, NsDecl, XmlDocument};

fn outcome<T>(result: Result<T, Error>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(Error::AlreadyPresent(_)) => "limit",
        Err(Error::UnresolvedSibling(_)) => "unresolved",
        Err(error) => panic!("unexpected placement error: {error}"),
    }
}

fn check<P: ElementSpec, C: ChildOf<P>>(
    source: &[u8],
    element_path: &str,
    append: &str,
    positional: &str,
) {
    let mut doc = XmlDocument::parse(source).unwrap();
    let mut node = doc.root();
    let path = element_path.split(',').filter(|i| !i.is_empty()).map(|i| {
        let (index, child) = node.indexed_elements().nth(i.parse().unwrap()).unwrap();
        node = child;
        index
    }).collect::<Vec<_>>();
    // Supply a namespace when the corpus has no binding for the new QName.
    // The spike measures placement independently of prefix availability.
    let decls = [NsDecl::new("acRankProbe", C::QNAME.ns)];
    let mut parent = doc.lens_mut_at::<P>(&path).unwrap();
    assert_eq!(outcome(parent.check_insert_with_ns::<C>(&decls)), append);
    assert_eq!(outcome(parent.check_insert_by_with_ns::<C>(|_, _| false, &decls)), positional);
    let count = parent.node().children().len();
    let can_insert_at = (0..=count).any(|index| {
        parent.check_insert_at_with_ns::<C>(index, &decls).is_ok()
    });
    assert_eq!(can_insert_at, positional == "ok");
    if append == "ok" {
        assert_eq!(outcome(parent.insert_with_ns::<C>(&decls)), append);
    } else {
        assert_eq!(outcome(parent.insert_with_ns::<C>(&decls)), append);
        assert_eq!(doc.serialize().as_bytes(), source);
    }
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args_os().nth(1).unwrap());
    let manifest = std::fs::read_to_string(dir.join("pairs.tsv")).unwrap();
    let mut last = String::new();
    let mut source = Vec::new();
    for line in manifest.lines() {
        let [id, path, pair, append, positional, description] =
            line.splitn(6, '\\t').collect::<Vec<_>>().try_into().unwrap();
        if last != id {
            source = std::fs::read(dir.join(id)).unwrap();
            last = id.to_owned();
        }
        let result = std::panic::catch_unwind(|| match pair {
            MATCH_ARMS
            _ => unreachable!(),
        });
        assert!(result.is_ok(), "{description}, element path {path}, {pair}");
    }
}
"""


def corpus_paths(arguments):
    for arg in map(Path, arguments):
        if arg.is_dir():
            yield from sorted(p for p in arg.rglob("*") if p.is_file())
        else:
            yield arg


def main(arguments):
    generated = SPIKE["load_generated"](ROOT / "crates/ooxml-schema/src/generated")
    stats, seen = SPIKE["Stats"](), set()
    arms, pairs = {}, []
    counts = collections.Counter()
    with tempfile.TemporaryDirectory(prefix="ac-core-") as scratch:
        scratch = Path(scratch)

        def part_checker(data, fmt, filename, part):
            # Save only parts with typed pairs, using a numeric filename.
            part_id = str(len(pairs))

            def check_pair(path, parent, child, outcome, same_slot):
                target = scratch / part_id
                if not target.exists():
                    target.write_bytes(data)
                append = "limit" if outcome in ("limit", "limit_ac") else outcome
                if append not in ("ok", "limit"):
                    append = "unresolved"
                positional = "unresolved" if same_slot else append
                pair = f"{parent}/{child}"
                arms[pair] = f"check::<ooxml_schema::{parent}, ooxml_schema::{child}>"
                pairs.append((part_id, ",".join(map(str, path)), pair, append, positional, f"{filename}/{part}"))
                counts[fmt, "pairs"] += 1
                counts[fmt, "append"] += append == "ok"
                counts[fmt, "positional"] += positional == "ok"

            return check_pair

        for path in corpus_paths(arguments):
            fmt = SPIKE["EXTS"].get(path.suffix.lower())
            if fmt:
                SPIKE["scan"](path, fmt, stats, generated, seen, part_checker)
        if not pairs:
            raise SystemExit("no typed AlternateContent pairs found")
        (scratch / "pairs.tsv").write_text("".join("\t".join(row) + "\n" for row in pairs))
        (scratch / "src").mkdir()
        match_arms = "\n".join(
            f'{json.dumps(pair)} => {call}(&source, path, append, positional),' for pair, call in sorted(arms.items())
        )
        (scratch / "src/main.rs").write_text(RUST.replace("MATCH_ARMS", match_arms))
        (scratch / "Cargo.toml").write_text(f"""
[package]
name = "ac-rank-core-check"
version = "0.1.0"
edition = "2024"

[dependencies]
ooxml-core = {{ path = {json.dumps(str(ROOT / "crates/ooxml-core"))} }}
ooxml-schema = {{ path = {json.dumps(str(ROOT / "crates/ooxml-schema"))}, features = ["sml", "wml", "pml", "dml", "props"] }}
""")
        # Keep the probe's build output inside the same repository clone.
        env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / "target"))
        subprocess.run(
            ["cargo", "run", "--quiet", "--manifest-path", str(scratch / "Cargo.toml"), "--", str(scratch)],
            check=True, cwd=ROOT, env=env,
        )
    for fmt in sorted({fmt for fmt, _ in counts}):
        print(f"{fmt}: {counts[fmt, 'pairs']} pairs, {counts[fmt, 'append']} append, {counts[fmt, 'positional']} positional")


if __name__ == "__main__":
    main(sys.argv[1:])
