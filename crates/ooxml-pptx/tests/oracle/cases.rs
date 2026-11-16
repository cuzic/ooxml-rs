//! Edit cases for the T3 oracle (ADR-0005 §7): one case per fixture that
//! edits **every** editable text body on every slide with a rotating set of
//! operations (so an edit that lands on the wrong shape, paragraph, run or
//! cell changes the model somewhere it should not), plus named cases.
//! Edits that ooxml-pptx refuses are dropped (their refusals are tested in
//! `tests/refusals.rs`); the list of applied edits is what the oracles
//! check.

#![allow(dead_code)]

use std::path::PathBuf;

use ooxml_pptx::{Error, Presentation, ShapeKind, TextBodyMut, TextItem};
use serde_json::{Value, json};

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures/pptx")
}

/// The 23 committed fixtures (reference/fixtures/pptx/README.md).
pub const FIXTURES: &[&str] = &[
    "poi/sample.pptx",
    "poi/layouts.pptx",
    "poi/with_japanese.pptx",
    "poi/shapes.pptx",
    "poi/sample_pptx_grouping_issues.pptx",
    "poi/table_test2.pptx",
    "poi/bar-chart.pptx",
    "poi/SmartArt.pptx",
    "poi/45545_Comment.pptx",
    "poi/bug65551.pptx",
    "poi/bug68703.pptx",
    "poi/picture-transparency.pptx",
    "poi/placeholder-layout-color.pptx",
    "poi/EmbeddedAudio.pptx",
    "poi/bug62513.pptx",
    "python-pptx/ph-inherit-props.pptx",
    "python-pptx/shp-connector-props.pptx",
    "python-pptx/sld-notes.pptx",
    "open-xml-sdk/animation.pptx",
    "open-xml-sdk/Of16-01.pptx",
    "libreoffice/modern-comments.pptx",
    "libreoffice/airplane-transition.pptx",
    "libreoffice/slide-sections.pptx",
];

pub fn read_fixture(rel: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[derive(Clone, Debug)]
pub enum Op {
    RunSetText { p: usize, r: usize, text: String },
    ParaSetText { p: usize, text: String },
    AddRun { p: usize, text: String },
    AddBreak { p: usize },
    RemoveRun { p: usize, r: usize },
    RemoveField { p: usize, f: usize },
    AddParagraph { text: String },
    RemoveParagraph { p: usize },
    BodySetText { text: String },
}

#[derive(Clone, Debug)]
pub struct Edit {
    pub slide: usize,
    pub shape_id: u32,
    pub cell: Option<(usize, usize)>,
    pub op: Op,
}

impl Edit {
    pub fn to_json(&self) -> Value {
        let (name, args) = match &self.op {
            Op::RunSetText { p, r, text } => ("run_set_text", json!({"p": p, "r": r, "text": text})),
            Op::ParaSetText { p, text } => ("para_set_text", json!({"p": p, "text": text})),
            Op::AddRun { p, text } => ("add_run", json!({"p": p, "text": text})),
            Op::AddBreak { p } => ("add_break", json!({"p": p})),
            Op::RemoveRun { p, r } => ("remove_run", json!({"p": p, "r": r})),
            Op::RemoveField { p, f } => ("remove_field", json!({"p": p, "f": f})),
            Op::AddParagraph { text } => ("add_paragraph", json!({"text": text})),
            Op::RemoveParagraph { p } => ("remove_paragraph", json!({"p": p})),
            Op::BodySetText { text } => ("body_set_text", json!({"text": text})),
        };
        json!({"slide": self.slide, "shape_id": self.shape_id, "cell": self.cell.map(|(r, c)| [r, c]), "op": name, "args": args})
    }
}

fn run_op(b: &mut TextBodyMut<'_>, op: &Op) -> Result<(), Error> {
    match op {
        Op::RunSetText { p, r, text } => b.paragraph_mut(*p)?.run_mut(*r)?.set_text(text),
        Op::ParaSetText { p, text } => b.paragraph_mut(*p)?.set_text(text),
        Op::AddRun { p, text } => b.paragraph_mut(*p)?.add_run(text),
        Op::AddBreak { p } => b.paragraph_mut(*p)?.add_break(),
        Op::RemoveRun { p, r } => b.paragraph_mut(*p)?.remove_run(*r),
        Op::RemoveField { p, f } => b.paragraph_mut(*p)?.remove_field(*f),
        Op::AddParagraph { text } => b.add_paragraph(text).map(|_| ()),
        Op::RemoveParagraph { p } => b.remove_paragraph(*p),
        Op::BodySetText { text } => b.set_text(text),
    }
}

/// Applies one edit through the public API.
pub fn apply_edit(pres: &mut Presentation, e: &Edit) -> Result<(), Error> {
    let mut slide = pres.slide_mut(e.slide)?;
    let mut shape = slide.shape_mut_by_id(e.shape_id)?;
    let mut body = match e.cell {
        None => shape.text_body_mut()?,
        Some((r, c)) => shape.table_cell_mut(r, c)?,
    };
    run_op(&mut body, &e.op)
}

/// One editable target and a summary of its first paragraphs.
struct Target {
    slide: usize,
    shape_id: u32,
    cell: Option<(usize, usize)>,
    paragraphs: usize,
    /// (paragraph index, run count, field count) of each paragraph
    shape: Vec<(usize, usize)>,
}

fn targets(pres: &mut Presentation) -> Vec<Target> {
    let mut out = Vec::new();
    for si in 0..pres.slide_count() {
        let slide = pres.slide(si).expect("slide");
        let shapes = slide.shapes();
        for s in &shapes {
            let Ok(id) = s.id() else { continue };
            // ids must be unique for id-based targeting
            if slide.shape_by_id(id).is_err() {
                continue;
            }
            let mut push = |cell, b: ooxml_pptx::TextBodyRef<'_>| {
                let paras = b.paragraphs();
                let shape = paras
                    .iter()
                    .map(|p| {
                        let items = p.items();
                        (
                            items.iter().filter(|i| matches!(i, TextItem::Run(_))).count(),
                            items.iter().filter(|i| matches!(i, TextItem::Field(_))).count(),
                        )
                    })
                    .collect();
                out.push(Target { slide: si, shape_id: id, cell, paragraphs: paras.len(), shape });
            };
            match s.kind() {
                ShapeKind::Shape => {
                    if let Some(b) = s.text_body() {
                        push(None, b);
                    }
                }
                ShapeKind::Table => {
                    for (r, row) in s.table().expect("table").rows().iter().enumerate() {
                        for (c, cell) in row.iter().enumerate() {
                            if let Some(b) = cell.text_body() {
                                push(Some((r, c)), b);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn op_for(k: usize, t: &Target) -> Op {
    let first_with_run = t.shape.iter().position(|&(r, _)| r > 0);
    let last = t.paragraphs.saturating_sub(1);
    match k % 9 {
        0 => match first_with_run {
            Some(p) => Op::RunSetText { p, r: 0, text: format!("run «{k}» ✓ 日本語 & <x>") },
            None => Op::ParaSetText { p: 0, text: format!("filled {k}") },
        },
        1 => Op::ParaSetText { p: 0, text: format!("para {k}") },
        2 => Op::BodySetText { text: format!("line one {k}\r\nline two\u{b}after break\n") },
        3 => Op::AddParagraph { text: format!("added {k}\u{b}second") },
        4 => Op::AddRun { p: last, text: format!(" +run{k} ") },
        5 => Op::AddBreak { p: last },
        6 => {
            if t.paragraphs > 1 {
                Op::RemoveParagraph { p: 0 }
            } else if t.shape[0].0 > 1 {
                Op::RemoveRun { p: 0, r: 1 }
            } else {
                Op::BodySetText { text: String::new() }
            }
        }
        7 => match t.shape.iter().position(|&(_, f)| f > 0) {
            Some(p) => Op::RemoveField { p, f: 0 },
            None => Op::ParaSetText { p: 0, text: format!("a{k}\u{b}b\u{b}") },
        },
        _ => Op::BodySetText { text: format!("single {k}") },
    }
}

/// A case: a fixture and the edits that were applied to it.
pub struct Case {
    pub name: String,
    pub fixture: String,
    pub edits: Vec<Edit>,
    /// (edit, refusal) pairs that were attempted but refused.
    pub refused: Vec<(Edit, String)>,
    pub output: Vec<u8>,
}

fn run_case(name: String, fixture: &str, plan: impl FnOnce(&mut Presentation) -> Vec<Edit>) -> Case {
    let mut pres = Presentation::from_bytes(read_fixture(fixture)).expect(fixture);
    let planned = plan(&mut pres);
    let (mut edits, mut refused) = (Vec::new(), Vec::new());
    for e in planned {
        match apply_edit(&mut pres, &e) {
            Ok(()) => edits.push(e),
            Err(err) => refused.push((e, format!("{err:?}"))),
        }
    }
    let output = pres.to_bytes().expect("save");
    Case { name, fixture: fixture.to_owned(), edits, refused, output }
}

/// The "edit everything" case of one fixture.
pub fn sweep_case(fixture: &str) -> Case {
    let name = format!("sweep-{}", fixture.replace(['/', '.'], "-"));
    run_case(name, fixture, |pres| {
        targets(pres)
            .iter()
            .enumerate()
            .map(|(k, t)| Edit { slide: t.slide, shape_id: t.shape_id, cell: t.cell, op: op_for(k, t) })
            .collect()
    })
}

/// Named cases with hand-picked targets (ids from the fixtures).
pub fn named_cases() -> Vec<Case> {
    let e = |slide, shape_id, cell, op| Edit { slide, shape_id, cell, op };
    vec![
        run_case("title-and-body".into(), "poi/sample.pptx", |pres| {
            let ph = pres.slide(0).unwrap().placeholders();
            let (title, body) = (ph[0].id().unwrap(), ph[1].id().unwrap());
            vec![
                e(0, title, None, Op::BodySetText { text: "New title".into() }),
                e(0, body, None, Op::BodySetText { text: "first\nsecond\nthird\u{b}continued\nfourth".into() }),
            ]
        }),
        run_case("japanese-table".into(), "poi/with_japanese.pptx", |pres| {
            let slide = pres.slide(0).unwrap();
            let t = slide.shapes().into_iter().find(|s| *s.kind() == ShapeKind::Table).expect("table");
            let id = t.id().unwrap();
            vec![
                e(0, id, Some((0, 0)), Op::BodySetText { text: "見出し".into() }),
                e(0, id, Some((1, 1)), Op::BodySetText { text: "セル\n二行目".into() }),
            ]
        }),
    ]
}

/// All cases: one sweep per fixture plus the named ones.
pub fn all_cases() -> Vec<Case> {
    let mut v: Vec<Case> = FIXTURES.iter().map(|f| sweep_case(f)).collect();
    v.extend(named_cases());
    v
}

pub fn cases_json(cases: &[Case]) -> Value {
    Value::Array(
        cases
            .iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "fixture": c.fixture,
                    "output": format!("{}.pptx", c.name),
                    "edits": c.edits.iter().map(Edit::to_json).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}
