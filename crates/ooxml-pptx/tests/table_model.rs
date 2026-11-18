//! ADR-0013 §2 merge rules as a property test (review N1): random tables
//! (1–5 rows × 1–5 columns, up to four non-overlapping merges, each 2-D
//! merge in the edge form or the flag-only form), every insert/delete
//! position of rows and columns, compared with an independent logical
//! model (each grid slot owned by a merge or by its own cell). The output
//! must have exactly the expected merge rectangles, correct flags, every
//! touched 2-D merge in the edge form, the original cell texts in their new
//! places, and empty new cells; refusals must leave the slide unchanged.

use std::collections::BTreeSet;
use std::io::{Cursor, Read, Write};

use ooxml_core::{QName, RawNode, XmlDocument};
use ooxml_pptx::{Error, Presentation};

const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx/poi/table_test2.pptx");

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Rect {
    r0: usize,
    c0: usize,
    nr: usize,
    nc: usize,
}

#[derive(Clone, Debug)]
struct Model {
    rows: usize,
    cols: usize,
    rects: Vec<(Rect, bool)>, // (rectangle, edge form)
}

fn random_model(rng: &mut Rng) -> Model {
    let rows = 1 + rng.below(5);
    let cols = 1 + rng.below(5);
    let mut taken = vec![vec![false; cols]; rows];
    let mut rects = Vec::new();
    for _ in 0..rng.below(5) {
        let (r0, c0) = (rng.below(rows), rng.below(cols));
        let nr = 1 + rng.below(rows - r0);
        let nc = 1 + rng.below(cols - c0);
        if nr * nc < 2 || (r0..r0 + nr).any(|r| (c0..c0 + nc).any(|c| taken[r][c])) {
            continue;
        }
        for row in taken.iter_mut().skip(r0).take(nr) {
            for t in row.iter_mut().skip(c0).take(nc) {
                *t = true;
            }
        }
        rects.push((Rect { r0, c0, nr, nc }, rng.below(2) == 0));
    }
    Model { rows, cols, rects }
}

fn tc_attrs(m: &Model, r: usize, c: usize) -> String {
    for (x, edge) in &m.rects {
        if !(x.r0..x.r0 + x.nr).contains(&r) || !(x.c0..x.c0 + x.nc).contains(&c) {
            continue;
        }
        let two = x.nr > 1 && x.nc > 1;
        let mut s = String::new();
        if (r, c) == (x.r0, x.c0) {
            if x.nc > 1 {
                s += &format!(" gridSpan=\"{}\"", x.nc);
            }
            if x.nr > 1 {
                s += &format!(" rowSpan=\"{}\"", x.nr);
            }
            return s;
        }
        if r == x.r0 && two && *edge {
            s += &format!(" rowSpan=\"{}\"", x.nr);
        }
        if c == x.c0 && two && *edge {
            s += &format!(" gridSpan=\"{}\"", x.nc);
        }
        if c != x.c0 {
            s += " hMerge=\"1\"";
        }
        if r != x.r0 {
            s += " vMerge=\"1\"";
        }
        return s;
    }
    String::new()
}

fn table_xml(m: &Model) -> String {
    let mut s = String::from("<a:tbl><a:tblPr firstRow=\"1\"/><a:tblGrid>");
    for c in 0..m.cols {
        s += &format!("<a:gridCol w=\"{}\"/>", 1000 + c);
    }
    s += "</a:tblGrid>";
    for r in 0..m.rows {
        s += &format!("<a:tr h=\"{}\">", 100 + r);
        for c in 0..m.cols {
            s += &format!(
                "<a:tc{}><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\"/><a:t>{r},{c}</a:t></a:r></a:p></a:txBody><a:tcPr marL=\"{c}\"/></a:tc>",
                tc_attrs(m, r, c)
            );
        }
        s += "</a:tr>";
    }
    s + "</a:tbl>"
}

fn base() -> (Vec<u8>, String) {
    let bytes = std::fs::read(FIXTURE).unwrap();
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut slide = String::new();
    z.by_name("ppt/slides/slide1.xml").unwrap().read_to_string(&mut slide).unwrap();
    (bytes, slide)
}

fn package_with(bytes: &[u8], slide: &str) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut e = z.by_index(i).unwrap();
        let n = e.name().unwrap().into_owned();
        let mut v = Vec::new();
        e.read_to_end(&mut v).unwrap();
        if n == "ppt/slides/slide1.xml" {
            v = slide.as_bytes().to_vec();
        }
        w.start_file(n, opts).unwrap();
        w.write_all(&v).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn with_table(base_bytes: &[u8], slide: &str, m: &Model) -> Vec<u8> {
    let i = slide.find("<a:tbl>").unwrap();
    let j = slide.find("</a:tbl>").unwrap() + "</a:tbl>".len();
    package_with(base_bytes, &format!("{}{}{}", &slide[..i], table_xml(m), &slide[j..]))
}

/// Independent read of the output table: per cell (gs, rs, hm, vm, text).
type Cell = (u32, u32, bool, bool, String);

fn read_table(p: &Presentation) -> (Vec<Vec<Cell>>, usize) {
    let part = p.slides().next().unwrap().unwrap().part_name;
    let doc = XmlDocument::parse(&p.package().read_part(&part).unwrap()).unwrap();
    fn find(n: &RawNode, q: QName) -> Option<&RawNode> {
        if n.is(q) {
            return Some(n);
        }
        n.elements().find_map(|c| find(c, q))
    }
    let tbl = find(doc.root(), QName::new(A, "tbl")).unwrap();
    let ncols = tbl.elements().find(|e| e.is(QName::new(A, "tblGrid"))).unwrap().elements().count();
    let num = |n: &RawNode, a: &'static str| n.attr(QName::new("", a)).map_or(1, |v| v.parse().unwrap());
    let flag = |n: &RawNode, a: &'static str| matches!(n.attr(QName::new("", a)), Some("1" | "true"));
    let rows = tbl
        .elements()
        .filter(|e| e.is(QName::new(A, "tr")))
        .map(|tr| {
            tr.elements()
                .filter(|e| e.is(QName::new(A, "tc")))
                .map(|tc| (num(tc, "gridSpan"), num(tc, "rowSpan"), flag(tc, "hMerge"), flag(tc, "vMerge"), deep_text(tc)))
                .collect()
        })
        .collect();
    (rows, ncols)
}

fn deep_text(n: &RawNode) -> String {
    if n.is(QName::new(A, "t")) {
        return n.text().into_owned();
    }
    n.elements().map(deep_text).collect()
}

/// Owner of each slot: `Some(k)` = rect k, `None` = own cell. Plus the
/// original position of each slot (None = new).
#[derive(Clone)]
struct Logical {
    owner: Vec<Vec<Option<usize>>>,
    orig: Vec<Vec<Option<(usize, usize)>>>,
}

fn logical(m: &Model) -> Logical {
    let mut owner = vec![vec![None; m.cols]; m.rows];
    for (k, (x, _)) in m.rects.iter().enumerate() {
        for row in owner.iter_mut().skip(x.r0).take(x.nr) {
            for o in row.iter_mut().skip(x.c0).take(x.nc) {
                *o = Some(k);
            }
        }
    }
    let orig = (0..m.rows).map(|r| (0..m.cols).map(|c| Some((r, c))).collect()).collect();
    Logical { owner, orig }
}

fn transpose<T: Clone>(v: &[Vec<T>]) -> Vec<Vec<T>> {
    (0..v[0].len()).map(|c| v.iter().map(|row| row[c].clone()).collect()).collect()
}

/// Insert a row at `at`: a new slot joins a merge only when the slots
/// above and below are the same merge (strictly inside it).
fn insert_logical(l: &Logical, at: usize) -> Logical {
    let cols = l.owner[0].len();
    let new_row: Vec<Option<usize>> = (0..cols)
        .map(|c| {
            let above = at.checked_sub(1).and_then(|r| l.owner[r][c]);
            let below = l.owner.get(at).and_then(|row| row[c]);
            if above.is_some() && above == below { above } else { None }
        })
        .collect();
    let mut owner = l.owner.clone();
    owner.insert(at, new_row);
    let mut orig = l.orig.clone();
    orig.insert(at, vec![None; cols]);
    Logical { owner, orig }
}

fn delete_logical(l: &Logical, r: usize) -> Logical {
    let mut owner = l.owner.clone();
    owner.remove(r);
    let mut orig = l.orig.clone();
    orig.remove(r);
    Logical { owner, orig }
}

fn rects_of(owner: &[Vec<Option<usize>>]) -> BTreeSet<Rect> {
    let mut out = BTreeSet::new();
    let ks: BTreeSet<usize> = owner.iter().flatten().flatten().copied().collect();
    for k in ks {
        let cells: Vec<(usize, usize)> =
            owner.iter().enumerate().flat_map(|(r, row)| row.iter().enumerate().filter(move |(_, o)| **o == Some(k)).map(move |(c, _)| (r, c))).collect();
        let r0 = cells.iter().map(|x| x.0).min().unwrap();
        let c0 = cells.iter().map(|x| x.1).min().unwrap();
        let nr = cells.iter().map(|x| x.0).max().unwrap() - r0 + 1;
        let nc = cells.iter().map(|x| x.1).max().unwrap() - c0 + 1;
        assert_eq!(cells.len(), nr * nc, "logical model is not rectangular");
        if nr * nc > 1 {
            out.insert(Rect { r0, c0, nr, nc });
        }
    }
    out
}

/// Checks the output against the expected logical model.
fn check(out: &[Vec<Cell>], ncols: usize, want: &Logical, edge_for: impl Fn(&Rect) -> Option<bool>, label: &str) {
    let rows = out.len();
    assert_eq!(rows, want.owner.len(), "{label}: rows");
    assert_eq!(ncols, want.owner[0].len(), "{label}: grid columns");
    for row in out {
        assert_eq!(row.len(), ncols, "{label}: short row");
    }
    // merges read from the output
    let mut found = BTreeSet::new();
    let mut covered = vec![vec![false; ncols]; rows];
    for r in 0..rows {
        for c in 0..ncols {
            let (gs, rs, hm, vm, _) = out[r][c].clone();
            if hm || vm || (gs == 1 && rs == 1) {
                continue;
            }
            let x = Rect { r0: r, c0: c, nr: rs as usize, nc: gs as usize };
            let two = x.nr > 1 && x.nc > 1;
            for rr in r..r + x.nr {
                for cc in c..c + x.nc {
                    if (rr, cc) == (r, c) {
                        continue;
                    }
                    covered[rr][cc] = true;
                    let (g2, r2, h2, v2, _) = out[rr][cc].clone();
                    assert_eq!((h2, v2), (cc != c, rr != r), "{label}: flags at ({rr},{cc})");
                    let edge = edge_for(&x);
                    if rr == r && two {
                        let want_rs = match edge {
                            Some(true) => x.nr as u32,
                            Some(false) => 1,
                            None => r2,
                        };
                        assert_eq!(r2, want_rs, "{label}: edge rowSpan at ({rr},{cc})");
                    } else if cc == c && two {
                        let want_gs = match edge {
                            Some(true) => x.nc as u32,
                            Some(false) => 1,
                            None => g2,
                        };
                        assert_eq!(g2, want_gs, "{label}: edge gridSpan at ({rr},{cc})");
                    } else {
                        assert_eq!((g2, r2), (1, 1), "{label}: span at ({rr},{cc})");
                    }
                }
            }
            found.insert(x);
        }
    }
    for r in 0..rows {
        for c in 0..ncols {
            let (_, _, hm, vm, ref text) = out[r][c];
            if hm || vm {
                assert!(covered[r][c], "{label}: stray flag at ({r},{c})");
            }
            match want.orig[r][c] {
                Some((or, oc)) => assert_eq!(text, &format!("{or},{oc}"), "{label}: text at ({r},{c})"),
                None => assert_eq!(text, "", "{label}: new cell has text"),
            }
        }
    }
    assert_eq!(found, rects_of(&want.owner), "{label}: merge rectangles");
}

fn run(op: &str, m: &Model, at: usize, bytes: &[u8], slide: &str) -> (Result<(), Error>, Presentation, Vec<u8>) {
    let input = with_table(bytes, slide, m);
    let mut p = Presentation::from_bytes(input.clone()).unwrap();
    let before = p.to_bytes().unwrap();
    let r = {
        let mut s = p.slide_mut(0).unwrap();
        let mut sh = s.shape_mut_by_id(4).unwrap();
        let mut t = sh.table_mut().unwrap();
        match op {
            "insert_row" => t.insert_row(at),
            "delete_row" => t.delete_row(at),
            "insert_column" => t.insert_column(at),
            _ => t.delete_column(at),
        }
    };
    (r, p, before)
}

#[test]
fn random_merges_against_the_logical_model() {
    let (bytes, slide) = base();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut ok, mut refused) = (0usize, 0usize);
    // TABLE_MODEL_CASES raises the count (the PR records a release run)
    let cases: usize = std::env::var("TABLE_MODEL_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    for case in 0..cases {
        let m = random_model(&mut rng);
        let l = logical(&m);
        let ops: Vec<(&str, usize)> = (0..=m.rows)
            .map(|a| ("insert_row", a))
            .chain((0..m.rows).map(|a| ("delete_row", a)))
            .chain((0..=m.cols).map(|a| ("insert_column", a)))
            .chain((0..m.cols).map(|a| ("delete_column", a)))
            .collect();
        for (op, at) in ops {
            let label = format!("case {case} {m:?} {op}({at})");
            let (r, p, before) = run(op, &m, at, &bytes, &slide);
            // which merges the logical op touches (they must be in edge form)
            let col = op.ends_with("column");
            let lt = if col { Logical { owner: transpose(&l.owner), orig: transpose(&l.orig) } } else { l.clone() };
            let rect_t = |x: &Rect| if col { Rect { r0: x.c0, c0: x.r0, nr: x.nc, nc: x.nr } } else { *x };
            let refuse_expected = if op.starts_with("delete") {
                lt.owner.len() == 1 || m.rects.iter().any(|(x, _)| {
                    let y = rect_t(x);
                    y.r0 == at && y.nr > 1
                })
            } else {
                false
            };
            match r {
                Err(e) => {
                    assert!(refuse_expected, "{label}: unexpected refusal {e}");
                    assert_eq!(p.to_bytes().unwrap(), before, "{label}: refusal changed the package");
                    refused += 1;
                }
                Ok(()) => {
                    assert!(!refuse_expected, "{label}: should have been refused");
                    let want_t = if op.starts_with("insert") { insert_logical(&lt, at) } else { delete_logical(&lt, at) };
                    let want = if col { Logical { owner: transpose(&want_t.owner), orig: transpose(&want_t.orig) } } else { want_t };
                    let (out, ncols) = read_table(&p);
                    // expected form of each output merge: touched ones are edge
                    // form; untouched keep their input form
                    let edge_for = |x: &Rect| -> Option<bool> {
                        let k = want.owner[x.r0][x.c0]?;
                        let (orig_rect, orig_edge) = m.rects[k];
                        Some(if (x.nr, x.nc) == (orig_rect.nr, orig_rect.nc) { orig_edge } else { true })
                    };
                    check(&out, ncols, &want, edge_for, &label);
                    ok += 1;
                }
            }
        }
    }
    eprintln!("table model: {cases} tables, {ok} edits checked, {refused} refusals");
    assert!(ok > cases * 10 && refused > cases / 4, "ok {ok}, refused {refused}");
}
