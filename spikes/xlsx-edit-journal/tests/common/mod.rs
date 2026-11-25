#![allow(dead_code)]

use ooxml_xlsx::{Border, Color, ExcelDateTime, Fill, Font, Format, Side, Workbook};
use xlsx_edit_journal_spike::op::{apply_seq, start_of};
use xlsx_edit_journal_spike::project::{model, predicates, strip_set};
use xlsx_edit_journal_spike::{Api, CellKey, CommitReport, Content, JError, Journal, Op, Patch, Start};

pub fn k(a1: &str) -> CellKey {
    let i = ooxml_xlsx::CellIndex::parse(a1).unwrap();
    CellKey::new(0, i.row, i.col)
}
pub fn k2(a1: &str) -> CellKey {
    CellKey { sheet: 1, ..k(a1) }
}
pub fn bold() -> Format {
    Format::new().set_bold()
}
pub fn date() -> ExcelDateTime {
    ExcelDateTime::from_ymd(2024, 1, 2).unwrap()
}
pub fn red() -> Fill {
    Fill::Pattern { pattern: "solid".into(), fg: Some(Color::rgb(0xFF0000)), bg: None }
}
pub fn italic() -> Font {
    Font { italic: true, ..Font::default() }
}
pub fn thin() -> Border {
    let s = Some(Side { style: "thin".into(), color: None });
    Border { left: s.clone(), right: s.clone(), top: s.clone(), bottom: s, ..Border::default() }
}

/// The sequential reference: each op by its API call, refusals dropped.
pub struct SeqOut {
    pub bytes: Vec<u8>,
    pub refused: Vec<Option<std::mem::Discriminant<ooxml_xlsx::Error>>>,
    pub written: Vec<CellKey>,
}

pub fn sequential(base: &[u8], ops: &[Op]) -> SeqOut {
    let mut wb = Workbook::from_bytes(base.to_vec()).unwrap();
    let mut refused = Vec::new();
    let mut written = Vec::new();
    for op in ops {
        let start = start_of(&mut wb, op.key()).unwrap();
        match apply_seq(&mut wb, op) {
            Ok(()) => {
                refused.push(None);
                let skipped = matches!(op, Op::Content(_, Content::RemoveFormula)) && start != Start::Formula;
                if !skipped {
                    written.push(op.key());
                }
            }
            Err(e) => refused.push(Some(std::mem::discriminant(&e))),
        }
    }
    SeqOut { bytes: wb.to_bytes().unwrap(), refused, written }
}

pub struct JOut {
    pub bytes: Vec<u8>,
    pub refused: Vec<Option<std::mem::Discriminant<ooxml_xlsx::Error>>>,
    pub report: CommitReport,
    pub journal: Journal,
}

pub fn journaled(base: &[u8], ops: &[Op]) -> JOut {
    let mut j = Journal::open(base.to_vec()).unwrap();
    let mut refused = Vec::new();
    for op in ops {
        match j.append(Api::Native, op.clone(), None) {
            Ok(_) => refused.push(None),
            Err(JError::Xlsx(e)) => refused.push(Some(std::mem::discriminant(&e))),
            Err(e) => panic!("unexpected journal error {e:?} for {op:?}"),
        }
    }
    let (bytes, report) = j.commit().unwrap_or_else(|e| panic!("accepted ⇒ compiles: {e:?}"));
    JOut { bytes, refused, report, journal: j }
}

/// Oracle 1 for one case. Returns the two outputs for extra assertions.
pub fn oracle1(base: &[u8], ops: &[Op]) -> (SeqOut, JOut) {
    let s = sequential(base, ops);
    let j = journaled(base, ops);
    assert_eq!(s.refused, j.refused, "(i) refusal equivalence");
    let (ms, mj) = (model(&s.bytes), model(&j.bytes));
    if ms != mj {
        for (key, c) in &ms.cells {
            if mj.cells.get(key) != Some(c) {
                eprintln!("cell {key:?}: sequential {c:?}\n            journal {:?}", mj.cells.get(key));
            }
        }
        for key in mj.cells.keys().filter(|k| !ms.cells.contains_key(*k)) {
            eprintln!("cell {key:?} only on the journal side: {:?}", mj.cells[key]);
        }
        assert!(ms.rows == mj.rows, "rows differ: {:?} vs {:?}", ms.rows, mj.rows);
        assert!(ms.ws_rest == mj.ws_rest, "worksheet rest differs");
        assert!(ms.parts == mj.parts, "other parts differ");
        panic!("(iii) projections differ for {ops:?}");
    }
    predicates(base, &s.bytes, &s.written, false).unwrap_or_else(|e| panic!("(iv) sequential predicate: {e}"));
    predicates(base, &j.bytes, &j.report.written, true).unwrap_or_else(|e| panic!("(iv) journal predicate: {e}"));
    let (ss, js) = (strip_set(base, &s.bytes), strip_set(base, &j.bytes));
    assert!(js.is_subset(&ss), "journal strip set {js:?} ⊄ sequential {ss:?}");
    (s, j)
}

pub fn content(at: CellKey, c: Content) -> Op {
    Op::Content(at, c)
}
pub fn patch(at: CellKey, p: Patch) -> Op {
    Op::Style(at, p)
}

/// The raw `<c>` element of a cell in the output, for fixed-case checks.
pub fn raw_cell(bytes: &[u8], sheet: &str, a1: &str) -> Option<String> {
    let pkg = ooxml_core::Package::from_bytes(bytes.to_vec()).unwrap();
    let s = String::from_utf8(pkg.read_part(sheet).unwrap()).unwrap();
    let pat = format!(r#"<c r="{a1}""#);
    let a = s.find(&pat)?;
    let rest = &s[a..];
    let e1 = rest.find("/>").unwrap_or(usize::MAX);
    let e2 = rest.find("</c>").map_or(usize::MAX, |e| e + 4);
    let open_end = rest.find('>').unwrap();
    let end = if e1 + 1 == open_end { e1 + 2 } else { e2 };
    Some(rest[..end].to_owned())
}

pub fn dimension(bytes: &[u8], part: &str) -> Option<String> {
    let pkg = ooxml_core::Package::from_bytes(bytes.to_vec()).unwrap();
    let s = String::from_utf8(pkg.read_part(part).unwrap()).unwrap();
    let a = s.find("<dimension ref=\"")? + 16;
    Some(s[a..a + s[a..].find('"')?].to_owned())
}

pub const S1: &str = "xl/worksheets/sheet1.xml";
