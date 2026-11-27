//! Commit = shadow serialization (README): history independence, replay
//! equality, bisimulation after a save, locality, and one white-box check
//! per remedy. The other oracles (1, 2, 3, 5, 8) run in this mode through
//! the default `Mode` of `Journal::open`.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use ooxml_core::{Package, PartStateKind, XmlDocument};
use ooxml_xlsx::{Format, Workbook};
use proptest::prelude::*;
use xlsx_edit_journal_spike::fixture::oracle_workbook;
use xlsx_edit_journal_spike::project::{model, predicates, strip_set, worksheet_parts};
use xlsx_edit_journal_spike::{Api, CellKey, Content, JError, Journal, Mode, Op, Patch};

fn cases() -> u32 {
    std::env::var("EJ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512)
}

// ------------------------------------------------------------ generators

fn cell() -> impl Strategy<Value = CellKey> {
    prop_oneof![
        8 => (0u32..7, 0u32..5).prop_map(|(r, c)| CellKey::new(0, r, c)),
        6 => prop::sample::select(vec![k("B5"), k("C3"), k("C1"), k("A3"), k("D7"), k("B6")]),
        1 => (0u32..2, 0u32..2).prop_map(|(r, c)| CellKey::new(1, r, c)),
    ]
}

fn content_s() -> impl Strategy<Value = Content> {
    prop_oneof![
        2 => prop::sample::select(vec![1.0, 2.0, 3.0, 7.0, 5.0, 0.5]).prop_map(Content::Number),
        1 => prop::sample::select(vec!["x", "m", "y", "z"]).prop_map(|s| Content::String(s.into())),
        1 => any::<bool>().prop_map(Content::Bool),
        1 => Just(Content::Error("#N/A".into())),
        2 => prop::sample::select(vec!["A1*2", "=SUM(A1:A3)", "1+1", "B5*2", "SUM("]).prop_map(|s| Content::Formula(s.into())),
        2 => Just(Content::RemoveFormula),
    ]
}

fn patch_s() -> impl Strategy<Value = Patch> {
    prop_oneof![
        Just(Patch::Format(Box::new(bold()))),
        Just(Patch::Format(Box::new(Format::new().set_num_format("0.0%")))),
        prop::sample::select(vec![0u32, 1, 2, 3, 99]).prop_map(Patch::Index),
        Just(Patch::NumFmt("0.00".into())),
        Just(Patch::Font(italic())),
        Just(Patch::Fill(red())),
        Just(Patch::Border(thin())),
    ]
}

fn op_s() -> impl Strategy<Value = Op> {
    prop_oneof![
        5 => (cell(), content_s()).prop_map(|(k, c)| Op::Content(k, c)),
        5 => (cell(), patch_s()).prop_map(|(k, p)| Op::Style(k, p)),
        1 => cell().prop_map(|k| Op::DateTime(k, date())),
        1 => cell().prop_map(|k| Op::NumberWithFormat(k, 5.0, bold())),
    ]
}

/// The Base cells of `oracle_workbook`, with an op that writes each one's
/// content back and its `s`.
fn base_cells() -> Vec<(CellKey, Content, u32)> {
    vec![
        (k("A1"), Content::Number(1.0), 0),
        (k("B1"), Content::String("x".into()), 0),
        (k("C1"), Content::Formula("A1*2".into()), 0),
        (k("A2"), Content::Number(2.0), 0),
        (k("C2"), Content::Formula("SUM(A1:A3)".into()), 0),
        (k("A3"), Content::Number(3.0), 1),
        (k("C3"), Content::Number(7.0), 0),
        (k("A4"), Content::Bool(true), 0),
        (k("B4"), Content::String("m".into()), 0),
        (k("A5"), Content::Formula("1+1".into()), 0),
        (k2("A1"), Content::Formula("Sheet1!A1+1".into()), 0),
    ]
}

/// A block of ops on one Base cell that ends in its Base state: changed and
/// written back, a style changed and set back, or a write of the Base value.
fn noise_block() -> impl Strategy<Value = (usize, Vec<Op>)> {
    let noise_value = prop::sample::select(vec![
        Content::Number(42.0),
        Content::String("noise".into()),
        Content::String("n2".into()),
        Content::Bool(false),
        Content::Error("#DIV/0!".into()),
        Content::Formula("B5*2".into()),
    ]);
    let noise_patch = prop::sample::select(vec![
        Patch::Format(Box::new(bold())),
        Patch::Font(italic()),
        Patch::Fill(red()),
        Patch::NumFmt("0.000".into()),
        Patch::Border(thin()),
        Patch::Index(2),
    ]);
    (0usize..11, 0u8..5, prop::collection::vec(noise_value, 1..3), prop::collection::vec(noise_patch, 1..3)).prop_map(|(i, kind, vals, pats)| {
        let (at, back, s) = base_cells()[i].clone();
        let mut ops = Vec::new();
        if kind == 0 || kind == 2 {
            ops.extend(vals.into_iter().map(|v| Op::Content(at, v)));
            ops.push(Op::Content(at, back.clone()));
        }
        if kind == 1 || kind == 2 {
            ops.extend(pats.into_iter().map(|p| Op::Style(at, p)));
            ops.push(Op::Style(at, Patch::Index(s)));
        }
        if kind == 3 {
            ops.push(Op::Content(at, back));
        }
        if kind == 4 {
            ops.push(Op::Style(at, Patch::Index(s)));
        }
        (i, ops)
    })
}

#[derive(Debug, Clone)]
struct Case {
    short: Vec<Op>,
    long: Vec<Op>,
}

/// `short` = F; `long` = F with noise blocks and overwrites mixed in. With
/// `disjoint`, noise lands only on cells F never writes; otherwise a block
/// on a cell F writes comes before F's first op there, and a full content op
/// of F on a Base cell may be preceded by another value (an overwrite).
fn case(disjoint: bool) -> impl Strategy<Value = Case> {
    (prop::collection::vec(op_s(), 1..=16), prop::collection::vec((noise_block(), any::<prop::sample::Index>()), 0..=6), prop::collection::vec(any::<bool>(), 16)).prop_map(move |(f, noise, over)| {
        let base_keys: Vec<CellKey> = base_cells().iter().map(|c| c.0).collect();
        // insert position (in F's index space) → blocks
        let mut at: BTreeMap<usize, Vec<Vec<Op>>> = BTreeMap::new();
        for ((ci, block), pos) in noise {
            let key = base_cells()[ci].0;
            let first = f.iter().position(|o| o.key() == key);
            if disjoint && first.is_some() {
                continue;
            }
            let p = pos.index(first.unwrap_or(f.len()) + 1);
            at.entry(p).or_default().push(block);
        }
        let mut long = Vec::new();
        for (i, op) in f.iter().enumerate() {
            for b in at.remove(&i).unwrap_or_default() {
                long.extend(b);
            }
            let overwritable = matches!(op, Op::Content(_, c) if !c.is_conditional() && *c != Content::Formula("SUM(".into())) || matches!(op, Op::DateTime(..) | Op::NumberWithFormat(..));
            if !disjoint && over[i] && overwritable && base_keys.contains(&op.key()) {
                long.push(Op::Content(op.key(), Content::Number(99.0)));
            }
            long.push(op.clone());
        }
        for (_, bs) in at {
            for b in bs {
                long.extend(b);
            }
        }
        Case { short: f, long }
    })
}

// --------------------------------------------------------------- helpers

fn run(base: &[u8], ops: &[Op]) -> (Journal, Vec<Option<String>>) {
    let mut j = Journal::open_with(base.to_vec(), Mode::Shadow).unwrap();
    let mut refused = Vec::new();
    for op in ops {
        match j.append(Api::Native, op.clone(), None) {
            Ok(_) => refused.push(None),
            Err(JError::Xlsx(e)) => refused.push(Some(format!("{:?}", std::mem::discriminant(&e)))),
            Err(e) => panic!("unexpected {e:?} for {op:?}"),
        }
    }
    (j, refused)
}

/// Every `<c>` of every worksheet, by (sheet, ref) → bytes.
fn raw_cells(bytes: &[u8]) -> BTreeMap<(usize, String), String> {
    let pkg = Package::from_bytes(bytes.to_vec()).unwrap();
    let mut out = BTreeMap::new();
    for (i, p) in worksheet_parts(bytes).iter().enumerate() {
        let src = String::from_utf8(pkg.read_part(p).unwrap()).unwrap();
        let doc = XmlDocument::parse(src.as_bytes()).unwrap();
        let Some(sd) = doc.root().elements().find(|e| e.name().local() == "sheetData") else { continue };
        for c in sd.elements().flat_map(|r| r.elements()) {
            let sp = c.span().unwrap();
            let r = c.attr(ooxml_core::QName::new("", "r")).unwrap().to_owned();
            out.insert((i, r), src[sp.start..sp.end()].to_owned());
        }
    }
    out
}

/// Locality: a Base cell that is not a changed cell keeps its bytes, except
/// a dependent whose cache was stripped.
fn check_locality(base: &[u8], out: &[u8], written: &[CellKey]) {
    let (b, o) = (raw_cells(base), raw_cells(out));
    let stripped = strip_set(base, out);
    let written: BTreeSet<(usize, String)> = written.iter().map(|k| (k.sheet, ooxml_xlsx::CellIndex { row: k.row, col: k.col }.to_string())).collect();
    for (key, bytes) in &b {
        if written.contains(key) || stripped.contains(key) {
            continue;
        }
        assert_eq!(o.get(key), Some(bytes), "locality: {key:?} changed without being written");
    }
}

fn assert_same_p(a: &[u8], b: &[u8], what: &str) {
    let (ma, mb) = (model(a), model(b));
    if ma != mb {
        for (key, c) in &ma.cells {
            if mb.cells.get(key) != Some(c) {
                eprintln!("cell {key:?}: {c:?}\n      vs {:?}", mb.cells.get(key));
            }
        }
        panic!("{what}: projections differ");
    }
}

fn grid() -> Vec<CellKey> {
    let mut v: Vec<CellKey> = (0..8).flat_map(|r| (0..5).map(move |c| CellKey::new(0, r, c))).collect();
    v.extend((0..2).flat_map(|r| (0..2).map(move |c| CellKey::new(1, r, c))));
    v
}

/// One history-independence case.
fn history(dim: &str, c: &Case, bytes_must_match: bool) {
    let base = oracle_workbook(dim);
    let (mut js, _) = run(&base, &c.short);
    let (mut jl, _) = run(&base, &c.long);
    let (s, _) = js.commit().unwrap();
    let (l, rep) = jl.commit().unwrap();
    assert_same_p(&s, &l, "history independence");
    predicates(&base, &l, &rep.written, true).unwrap_or_else(|e| panic!("predicate (long): {e}"));
    check_locality(&base, &l, &rep.written);
    if bytes_must_match && s != l {
        let (rs, rl) = (raw_cells(&s), raw_cells(&l));
        for (key, v) in &rs {
            if rl.get(key) != Some(v) {
                eprintln!("{key:?}: short {v}\n           long  {:?}", rl.get(key));
            }
        }
        let pk = |b: &[u8]| -> BTreeMap<String, Vec<u8>> {
            let p = Package::from_bytes(b.to_vec()).unwrap();
            p.part_names().map(|n| (n.to_owned(), p.read_part(n).unwrap())).collect()
        };
        let (ps, pl) = (pk(&s), pk(&l));
        for (n, v) in &ps {
            if pl.get(n) != Some(v) {
                eprintln!("part {n}:\n short {}\n long  {}", String::from_utf8_lossy(v), pl.get(n).map(|x| String::from_utf8_lossy(x).into_owned()).unwrap_or_default());
            }
        }
        panic!("history independence: bytes differ\nshort {:?}\nlong {:?}", c.short, c.long);
    }
}

fn dim_s() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec!["A1:C5", "A1:A2", "A1"])
}

// ------------------------------------------------------------- proptests

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), failure_persistence: None, ..ProptestConfig::default() })]

    /// Noise on cells F does not write: the outputs are byte-identical.
    #[test]
    fn history_independence_bytes(dim in dim_s(), c in case(true)) {
        history(dim, &c, true);
    }

    /// Noise also on cells F writes later, and overwrites: byte-identical
    /// too (`Act::BaseStyle` and the canonical form of written cells).
    #[test]
    fn history_independence_with_overwrites(dim in dim_s(), c in case(false)) {
        history(dim, &c, true);
    }

    /// The live shadow and a shadow rebuilt by replay from the Base give the
    /// same bytes, also after a save on the live side.
    #[test]
    fn replay_equals_live(c in case(false), d in case(false)) {
        let base = oracle_workbook("A1:C5");
        let (mut j, _) = run(&base, &c.long);
        let live = j.preview().unwrap();
        j.mark_shadow_stale();
        prop_assert!(live == j.preview().unwrap(), "replay");
        // after a save: the live shadow carries state; the replay starts fresh
        let (mut j, _) = run(&base, &c.long);
        j.commit().unwrap();
        for op in &d.long {
            let _ = j.append(Api::Native, op.clone(), None);
        }
        let live = j.preview().unwrap();
        j.mark_shadow_stale();
        prop_assert!(live == j.preview().unwrap(), "replay after a save");
    }

    /// Bisimulation: the shadow kept after a save and a Journal opened from
    /// the output take the same ops to the same reads and the same bytes.
    #[test]
    fn bisimulation_after_save(c in case(false), d in case(false), dim in dim_s()) {
        let base = oracle_workbook(dim);
        let (mut j1, _) = run(&base, &c.long);
        let (out, _) = j1.commit().unwrap();
        let mut j2 = Journal::open_with(out.clone(), Mode::Shadow).unwrap();
        let xfs = Workbook::from_bytes(out.clone()).unwrap().cell_format_count().unwrap();
        // an `xf` this session created has no Base number yet; its index is
        // the shadow's own (README): compared by content only
        let read = |j: &mut Journal, k| j.normalized(k).unwrap().map(|mut n| { if n.style_index >= xfs { n.style_index = u32::MAX; } n });
        for k in grid() {
            prop_assert_eq!(read(&mut j1, k), read(&mut j2, k), "read of {:?} right after the save", k);
        }
        for op in &d.long {
            let (a, b) = (j1.append(Api::Native, op.clone(), None), j2.append(Api::Native, op.clone(), None));
            prop_assert_eq!(a.is_ok(), b.is_ok(), "refusal of {:?}", op);
        }
        for k in grid() {
            prop_assert_eq!(read(&mut j1, k), read(&mut j2, k), "read of {:?}", k);
        }
        let (o1, _) = j1.commit().unwrap();
        let (o2, _) = j2.commit().unwrap();
        assert_same_p(&o1, &o2, "bisimulation");
        prop_assert!(o1 == o2, "bisimulation: bytes differ");
    }
}

// ------------------------------------------------------- fixed / remedies

/// Remedy 1: writes that change nothing are not applied, so the shadow's
/// worksheet stays clean (raw-copied by the shadow's own serializer).
#[test]
fn noop_writes_leave_the_shadow_clean() {
    let base = oracle_workbook("A1:A2");
    let mut j = Journal::open_with(base.clone(), Mode::Shadow).unwrap();
    for op in [content(k("C3"), Content::Number(7.0)), content(k("C1"), Content::Formula("A1*2".into())), content(k("A4"), Content::Bool(true)), patch(k("A3"), Patch::Index(1))] {
        j.append(Api::Native, op, None).unwrap();
    }
    let wb = j.__shadow_if_live().unwrap();
    // R1 is off by default (revision 8 §4.5, until C1): only the old,
    // unvalidated R1 leaves the shadow clean
    if cfg!(feature = "r1_unvalidated") {
        assert!(matches!(wb.package().state(S1), Some(PartStateKind::Raw | PartStateKind::Parsed { dirty: false })), "no-op writes did not touch the shadow");
    }
    // the formula keeps its cache in the View
    assert_eq!(j.normalized(k("C1")).unwrap().unwrap().value, ooxml_xlsx::CellValue::Number(2.0));
    assert_eq!(j.commit().unwrap().0, base);
}

/// Remedy 2: a changed-and-restored cell keeps its Base bytes, also the
/// formula cache that the shadow's `set_formula` dropped, and `dimension`
/// does not keep the growth.
#[test]
fn a_restored_cell_keeps_its_base_bytes() {
    let base = oracle_workbook("A1:A2");
    let mut j = Journal::open_with(base.clone(), Mode::Shadow).unwrap();
    for op in [
        content(k("C1"), Content::Formula("B5*2".into())),
        content(k("C1"), Content::Formula("A1*2".into())),
        content(k("C3"), Content::Number(8.0)),
        content(k("C3"), Content::Number(7.0)),
    ] {
        j.append(Api::Native, op, None).unwrap();
    }
    assert_eq!(j.normalized(k("C1")).unwrap().unwrap().value, ooxml_xlsx::CellValue::Number(2.0), "View reads the Base cache");
    assert_eq!(j.commit().unwrap().0, base);
}

/// Remedy 3: a value written and restored strips nothing; a value changed
/// strips its dependents once, at save, on a sheet the shadow never dirtied
/// too (Sheet2), and that strip carries over the next save.
#[test]
fn calc_state_is_decided_at_save() {
    let base = oracle_workbook("A1:C5");
    let mut j = Journal::open_with(base.clone(), Mode::Shadow).unwrap();
    j.append(Api::Native, content(k("A1"), Content::Number(5.0)), None).unwrap();
    assert_eq!(j.normalized(k("C1")).unwrap().unwrap().value, ooxml_xlsx::CellValue::Empty, "a pending strip reads Empty");
    let (out, _) = j.commit().unwrap();
    assert!(!raw_cell(&out, "xl/worksheets/sheet2.xml", "A1").unwrap().contains("<v>"));
    assert!(!raw_cell(&out, S1, "C1").unwrap().contains("<v>"));
    // next save, a write elsewhere: the stripped caches stay stripped
    j.append(Api::Native, content(k("A2"), Content::Number(2.0)), None).unwrap(); // no-op
    j.append(Api::Native, content(k("B5"), Content::Number(1.0)), None).unwrap();
    let (out2, _) = j.commit().unwrap();
    assert!(!raw_cell(&out2, "xl/worksheets/sheet2.xml", "A1").unwrap().contains("<v>"));
    let fresh = Journal::open_with(out.clone(), Mode::Shadow).unwrap();
    drop(fresh);
}

/// Remedy 4: items the session added and no longer references are gone;
/// the survivors are numbered after the Base's, in cell order.
#[test]
fn session_registry_items_are_collected_and_numbered_canonically() {
    let base = oracle_workbook("A1:C5");
    let p = |b: &[u8], n: &str| String::from_utf8(Package::from_bytes(b.to_vec()).unwrap().read_part(n).unwrap()).unwrap();
    let mut j = Journal::open_with(base.clone(), Mode::Shadow).unwrap();
    for op in [
        content(k("D7"), Content::String("late".into())),
        content(k("A2"), Content::String("gone".into())),
        content(k("A2"), Content::Number(2.0)),
        content(k("D1"), Content::String("early".into())),
        patch(k("D7"), Patch::Fill(red())),
        patch(k("A3"), Patch::Font(italic())),
        patch(k("A3"), Patch::Index(1)),
        patch(k("D1"), Patch::Border(thin())),
    ] {
        j.append(Api::Native, op, None).unwrap();
    }
    let (out, _) = j.commit().unwrap();
    let sst = p(&out, "xl/sharedStrings.xml");
    assert!(!sst.contains("gone"), "{sst}");
    assert!(sst.find("early").unwrap() < sst.find("late").unwrap(), "{sst}");
    let st = p(&out, "xl/styles.xml");
    assert!(!st.contains("<i/>"), "the italic font is collected: {st}");
    assert!(raw_cell(&out, S1, "D1").unwrap().contains(r#"s="4""#));
    assert!(raw_cell(&out, S1, "D7").unwrap().contains(r#"s="5""#));
    // the same final state written directly
    let mut d = Journal::open_with(base, Mode::Shadow).unwrap();
    for op in [content(k("D1"), Content::String("early".into())), patch(k("D1"), Patch::Border(thin())), content(k("D7"), Content::String("late".into())), patch(k("D7"), Patch::Fill(red()))] {
        d.append(Api::Native, op, None).unwrap();
    }
    assert_eq!(d.commit().unwrap().0, out);
}

/// The shadow cannot restore a node, so a cell reset to `xf` 0 keeps
/// `s="0"` there. While the cell is restored the output has the Base bytes;
/// once a later write changes it, the text layer writes `xf` 0 as the Base
/// cell did (here: no `s`), so the output does not depend on the reset
/// (README; core would restore the node instead).
#[test]
fn style_reset_to_zero_then_written_matches_the_direct_write() {
    let base = oracle_workbook("A1:C5");
    let (mut a, _) = run(&base, &[patch(k("A1"), Patch::Fill(red())), patch(k("A1"), Patch::Index(0)), content(k("A1"), Content::Number(9.0))]);
    let (mut b, _) = run(&base, &[content(k("A1"), Content::Number(9.0))]);
    let (oa, ob) = (a.commit().unwrap().0, b.commit().unwrap().0);
    assert_eq!(raw_cell(&oa, S1, "A1").unwrap(), r#"<c r="A1"><v>9</v></c>"#);
    assert_eq!(oa, ob);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), failure_persistence: None, ..ProptestConfig::default() })]

    /// `truncate_fast` (undo from the snapshots) and `truncate` (replay)
    /// give the same reads and the same bytes, also after a save.
    #[test]
    fn fast_truncate_equals_replay(c in case(false), d in case(false), cut in any::<prop::sample::Index>(), save in any::<bool>()) {
        let base = oracle_workbook("A1:C5");
        let mut js = [Journal::open_with(base.clone(), Mode::Shadow).unwrap(), Journal::open_with(base, Mode::Shadow).unwrap()];
        for j in &mut js {
            if save {
                for op in &d.long {
                    let _ = j.append(Api::Native, op.clone(), None);
                }
                j.commit().unwrap();
            }
            for op in &c.long {
                let _ = j.append(Api::Native, op.clone(), None);
            }
        }
        let first = js[0].commit_mark();
        let last = js[0].next_seq();
        let t = first + cut.index((last - first) as usize) as u64;
        js[0].truncate(t).unwrap();
        js[1].truncate_fast(t).unwrap();
        let [a, b] = &mut js;
        let xfs = Workbook::from_bytes(a.base().to_vec()).unwrap().cell_format_count().unwrap();
        let read = |j: &mut Journal, k| j.normalized(k).unwrap().map(|mut n| { if n.style_index >= xfs { n.style_index = u32::MAX; } n });
        for k in grid() {
            prop_assert_eq!(read(a, k), read(b, k), "read of {:?}", k);
        }
        prop_assert!(a.preview().unwrap() == b.preview().unwrap(), "bytes");
    }
}
