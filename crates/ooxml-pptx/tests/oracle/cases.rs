//! Edit cases for the T3 oracle (ADR-0005 §7): per fixture one *sweep* that
//! edits **every** editable text body on every slide once with a rotating
//! set of operations (so an edit that lands on the wrong shape, paragraph,
//! run or cell changes the model somewhere it should not), one *multi*
//! sweep that applies three successive edits to every body, addressing
//! paragraphs and runs past index 0 from the body's state after the
//! previous edit (review R2-3: stale indices), plus named cases.
//! Edits that ooxml-pptx refuses are dropped (their refusals are tested in
//! `tests/refusals.rs`); the list of applied edits is what the oracles
//! check.

#![allow(dead_code)]

use std::path::PathBuf;

use ooxml_pptx::{Error, FindOptions, Match, MixedFormat, Presentation, ReplaceOptions, Scope, ShapeKind, TextBodyMut, TextItem};
use serde_json::{Value, json};

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures/pptx")
}

/// The 24 committed fixtures (reference/fixtures/pptx/README.md).
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
    "project/powerpoint-shape-comment.pptx",
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
    /// The slide's notes body (ADR-0008 §2); `shape_id` is the body's.
    pub notes: bool,
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
        json!({"slide": self.slide, "shape_id": self.shape_id, "cell": self.cell.map(|(r, c)| [r, c]), "notes": self.notes, "op": name, "args": args})
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
    if e.notes {
        let mut notes = pres.notes_mut(e.slide)?;
        return run_op(&mut notes.body_mut()?, &e.op);
    }
    let mut slide = pres.slide_mut(e.slide)?;
    let mut shape = slide.shape_mut_by_id(e.shape_id)?;
    let mut body = match e.cell {
        None => shape.text_body_mut()?,
        Some((r, c)) => shape.table_cell_mut(r, c)?,
    };
    run_op(&mut body, &e.op)
}

/// One editable target and a summary of its paragraphs.
struct Target {
    slide: usize,
    shape_id: u32,
    cell: Option<(usize, usize)>,
    notes: bool,
    paragraphs: usize,
    /// (run count, field count) of each paragraph
    shape: Vec<(usize, usize)>,
}

fn summary(b: ooxml_pptx::TextBodyRef<'_>) -> (usize, Vec<(usize, usize)>) {
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
    (paras.len(), shape)
}

/// The current summary of one target (after earlier edits).
fn reread(pres: &mut Presentation, t: &Target) -> Target {
    if t.notes {
        let n = pres.notes(t.slide).expect("notes").expect("notes slide");
        let (paragraphs, shape) = summary(n.body().expect("notes body"));
        return Target { slide: t.slide, shape_id: t.shape_id, cell: None, notes: true, paragraphs, shape };
    }
    let slide = pres.slide(t.slide).expect("slide");
    let s = slide.shape_by_id(t.shape_id).expect("unique id").expect("shape");
    let b = match t.cell {
        None => s.text_body(),
        Some((r, c)) => s.table().and_then(|tb| tb.cell(r, c)).and_then(|c| c.text_body()),
    }
    .expect("body");
    let (paragraphs, shape) = summary(b);
    Target { slide: t.slide, shape_id: t.shape_id, cell: t.cell, notes: false, paragraphs, shape }
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
                let (paragraphs, shape) = summary(b);
                out.push(Target { slide: si, shape_id: id, cell, notes: false, paragraphs, shape });
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
    // notes bodies last, so that the slide targets keep their operations
    for si in 0..pres.slide_count() {
        let Some(n) = pres.notes(si).expect("notes") else { continue };
        if let (Some(sh), Some(b)) = (n.body_shape(), n.body()) {
            let (paragraphs, shape) = summary(b);
            out.push(Target { slide: si, shape_id: sh.id().unwrap_or(0), cell: None, notes: true, paragraphs, shape });
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

/// The `j`-th of three successive edits of target `k` in a multi sweep,
/// chosen from the body's current state; indices are mostly the last
/// paragraph/run so that an off-by-one or stale index shows.
fn op_multi(k: usize, j: usize, t: &Target) -> Op {
    let last = t.paragraphs.saturating_sub(1);
    let with_runs: Vec<usize> = (0..t.paragraphs).filter(|&p| t.shape[p].0 > 0).collect();
    match (k + 3 * j) % 8 {
        0 => match with_runs.last() {
            Some(&p) => Op::RunSetText { p, r: t.shape[p].0 - 1, text: format!("m{k}.{j} last run") },
            None => Op::ParaSetText { p: last, text: format!("m{k}.{j} filled") },
        },
        1 => Op::ParaSetText { p: last, text: format!("m{k}.{j}\u{b}tail{j}") },
        2 => Op::BodySetText { text: (0..=t.paragraphs).map(|i| format!("m{k}.{j} line {i}")).collect::<Vec<_>>().join("\n") },
        3 => Op::AddParagraph { text: format!("m{k}.{j} added") },
        4 => match with_runs.iter().rev().find(|&&p| t.shape[p].0 > 1) {
            Some(&p) => Op::RemoveRun { p, r: t.shape[p].0 - 1 },
            None => Op::AddRun { p: last, text: format!(" m{k}.{j}") },
        },
        5 => {
            if t.paragraphs > 1 {
                Op::RemoveParagraph { p: last - 1 }
            } else {
                Op::AddBreak { p: 0 }
            }
        }
        6 => Op::AddRun { p: last / 2, text: format!(" mid{k}.{j}") },
        _ => Op::ParaSetText { p: last / 2, text: format!("m{k}.{j} middle") },
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
    /// Deck-wide find/replace calls applied after `edits` (ADR-0008 §4):
    /// `{pattern, replacement, refuse_mixed, replaced: [loc], refused: [loc]}`.
    pub replaces: Vec<Value>,
}

pub fn run_case(name: String, fixture: &str, plan: impl FnOnce(&mut Presentation) -> Vec<Edit>) -> Case {
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
    Case { name, fixture: fixture.to_owned(), edits, refused, output, replaces: Vec::new() }
}

/// The "edit everything" case of one fixture.
pub fn sweep_case(fixture: &str) -> Case {
    let name = format!("sweep-{}", fixture.replace(['/', '.'], "-"));
    run_case(name, fixture, |pres| {
        targets(pres)
            .iter()
            .enumerate()
            .map(|(k, t)| Edit { slide: t.slide, shape_id: t.shape_id, cell: t.cell, notes: t.notes, op: op_for(k, t) })
            .collect()
    })
}

/// The "three successive edits per body" case of one fixture (review R2-3).
/// Each edit is chosen after the previous one was applied (or refused).
pub fn multi_case(fixture: &str) -> Case {
    let name = format!("multi-{}", fixture.replace(['/', '.'], "-"));
    let mut pres = Presentation::from_bytes(read_fixture(fixture)).expect(fixture);
    let (mut edits, mut refused) = (Vec::new(), Vec::new());
    for (k, t0) in targets(&mut pres).iter().enumerate() {
        for j in 0..3 {
            let t = reread(&mut pres, t0);
            let e = Edit { slide: t.slide, shape_id: t.shape_id, cell: t.cell, notes: t.notes, op: op_multi(k, j, &t) };
            match apply_edit(&mut pres, &e) {
                Ok(()) => edits.push(e),
                Err(err) => refused.push((e, format!("{err:?}"))),
            }
        }
    }
    let output = pres.to_bytes().expect("save");
    Case { name, fixture: fixture.to_owned(), edits, refused, output, replaces: Vec::new() }
}

/// Named cases with hand-picked targets (ids from the fixtures).
pub fn named_cases() -> Vec<Case> {
    let e = |slide, shape_id, cell, op| Edit { slide, shape_id, cell, notes: false, op };
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
        // Phase 0.1: runs whose a:rPr has a:hlinkClick (shapes.pptx slide 5
        // table): new runs/paragraphs copy the format without the link.
        run_case("hyperlink-cells".into(), "poi/shapes.pptx", |pres| {
            let slide = pres.slide(4).unwrap();
            let t = slide.shapes().into_iter().find(|s| *s.kind() == ShapeKind::Table).expect("table");
            let id = t.id().unwrap();
            let linked: Vec<(usize, usize)> = t
                .table()
                .unwrap()
                .rows()
                .iter()
                .enumerate()
                .flat_map(|(r, row)| {
                    row.iter()
                        .enumerate()
                        .filter(|(_, c)| c.text_body().is_some_and(|b| b.paragraphs().iter().any(|q| q.raw().elements().any(|r| r.elements().any(|p| p.elements().any(|l| l.name().local() == "hlinkClick"))))))
                        .map(move |(c, _)| (r, c))
                        .collect::<Vec<_>>()
                })
                .collect();
            assert_eq!(linked.len(), 3);
            vec![
                e(4, id, Some(linked[0]), Op::ParaSetText { p: 0, text: "link\u{b}plain".into() }),
                e(4, id, Some(linked[1]), Op::BodySetText { text: "link\nplain\n".into() }),
                e(4, id, Some(linked[2]), Op::ParaSetText { p: 0, text: "\u{b}after".into() }),
            ]
        }),
    ]
}

/// All cases: one sweep and one multi sweep per fixture, plus the named ones.
pub fn all_cases() -> Vec<Case> {
    let mut v: Vec<Case> = FIXTURES.iter().map(|f| sweep_case(f)).collect();
    v.extend(FIXTURES.iter().map(|f| multi_case(f)));
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
                    "replace": c.replaces,
                })
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Find/replace cases (ADR-0008 §7 O5, review #48 M1): deck-wide
// `replace_all` calls. check.py redoes the §4 edits independently on
// python-pptx's lxml tree for the matches ooxml-pptx accepted, and checks
// that the accepted set is python's own match set minus the refused ones.

/// A phrase that crosses two adjacent runs with different `a:rPr` (the last
/// two characters of one run and the first two of the next), if any.
fn cross_run_phrase(pres: &mut Presentation) -> Option<String> {
    for i in 0..pres.slide_count() {
        let Ok(s) = pres.slide(i) else { continue };
        let mut bodies: Vec<ooxml_pptx::TextBodyRef<'_>> = Vec::new();
        for sh in s.shapes() {
            bodies.extend(sh.text_body());
            if let Some(t) = sh.table() {
                bodies.extend(t.rows().into_iter().flatten().filter_map(|c| c.text_body()));
            }
        }
        for b in bodies {
            for p in b.paragraphs() {
                for w in p.runs().windows(2) {
                    let (a, c) = (w[0].text(), w[1].text());
                    let differ = match (w[0].raw().elements().next(), w[1].raw().elements().next()) {
                        (Some(x), Some(y)) => !x.structurally_eq(y),
                        _ => true,
                    };
                    let (ta, tc): (Vec<char>, Vec<char>) = (a.chars().collect(), c.chars().collect());
                    if differ && ta.len() >= 2 && tc.len() >= 2 {
                        let phrase: String = ta[ta.len() - 2..].iter().chain(&tc[..2]).collect();
                        if !phrase.contains(['\n', '\r', '\u{b}']) {
                            return Some(phrase);
                        }
                    }
                }
            }
        }
    }
    None
}

/// The text of the second run of the first paragraph with at least two
/// non-empty runs, if any.
/// A phrase covering an `a:fld` (rule 1, review #48 r2-m1): the last
/// character before the field (if any) and the field's cached text.
fn field_phrase(pres: &mut Presentation) -> Option<String> {
    for i in 0..pres.slide_count() {
        let Ok(s) = pres.slide(i) else { continue };
        for sh in s.shapes() {
            let Some(b) = sh.text_body() else { continue };
            for p in b.paragraphs() {
                let items = p.items();
                for (k, it) in items.iter().enumerate() {
                    let TextItem::Field(f) = it else { continue };
                    let t = f.text();
                    if t.is_empty() || t.contains(['\n', '\r', '\u{b}']) {
                        continue;
                    }
                    let before = match k.checked_sub(1).map(|k| &items[k]) {
                        Some(TextItem::Run(r)) => r.text().chars().last(),
                        _ => None,
                    };
                    return Some(before.into_iter().chain(t.chars()).collect());
                }
            }
        }
    }
    None
}

fn whole_run_text(pres: &mut Presentation) -> Option<String> {
    for i in 0..pres.slide_count() {
        let Ok(s) = pres.slide(i) else { continue };
        for sh in s.shapes() {
            let Some(b) = sh.text_body() else { continue };
            for p in b.paragraphs() {
                let runs: Vec<String> = p.runs().iter().map(|r| r.text().into_owned()).filter(|t| !t.is_empty()).collect();
                if runs.len() >= 2 && !runs[1].contains(['\n', '\r', '\u{b}']) {
                    return Some(runs[1].clone());
                }
            }
        }
    }
    None
}

fn location(input: &mut Presentation, m: &Match) -> Value {
    let id = if m.location.notes {
        input.notes(m.location.slide).ok().flatten().and_then(|n| n.shapes().into_iter().find(|x| x.path() == m.location.shape).and_then(|x| x.id().ok()))
    } else {
        input.slide(m.location.slide).ok().and_then(|s| s.shapes().into_iter().find(|x| x.path() == m.location.shape).and_then(|x| x.id().ok()))
    };
    json!({
        "slide": m.location.slide,
        "notes": m.location.notes,
        "shape_id": id,
        "cell": m.location.cell.map(|(r, c)| [r, c]),
        "paragraph": m.location.paragraph,
        "start": m.range.start,
        "end": m.range.end,
    })
}

/// `notes`: search notes bodies too ([`Scope::SlidesAndNotes`], ADR-0008 §2).
pub fn replace_case(name: String, fixture: &str, pattern: &str, replacement: &str, refuse_mixed: bool, notes: bool) -> Case {
    let mut pres = Presentation::from_bytes(read_fixture(fixture)).expect(fixture);
    let scope = if notes { Scope::SlidesAndNotes } else { Scope::Slides };
    let o = ReplaceOptions::default()
        .find(FindOptions::default().scope(scope))
        .mixed_format(if refuse_mixed { MixedFormat::Refuse } else { MixedFormat::Allow });
    let rep = pres.replace_all(pattern, replacement, &o).expect("replace_all");
    let output = pres.to_bytes().expect("save");
    let mut input = Presentation::from_bytes(read_fixture(fixture)).expect(fixture);
    let replaced: Vec<Value> = rep.replaced.iter().map(|m| location(&mut input, m)).collect();
    let refused: Vec<Value> = rep
        .skipped
        .iter()
        .map(|(m, e)| {
            let mut v = location(&mut input, m);
            v["error"] = json!(format!("{e:?}"));
            v
        })
        .collect();
    let spec = json!({"pattern": pattern, "replacement": replacement, "refuse_mixed": refuse_mixed, "notes": notes, "replaced": replaced, "refused": refused});
    Case { name, fixture: fixture.to_owned(), edits: Vec::new(), refused: Vec::new(), output, replaces: vec![spec] }
}

/// Per fixture: a frequent letter replaced by text with a line break
/// (rules 2–5, `FieldInMatch`, `LinkSplitByBreak`), a deletion, and a
/// cross-run phrase; plus two `MixedFormat::Refuse` cases.
pub fn replace_cases() -> Vec<Case> {
    let mut out = Vec::new();
    for f in FIXTURES {
        let tag = f.replace(['/', '.'], "-");
        out.push(replace_case(format!("replace-e-{tag}"), f, "e", "\u{1E8C}\u{b}Q", false, true));
        out.push(replace_case(format!("replace-the-{tag}"), f, "the", "", false, true));
        let mut pres = Presentation::from_bytes(read_fixture(f)).expect(f);
        if let Some(whole) = whole_run_text(&mut pres) {
            // deletes a whole run of a multi-run paragraph (rule 4)
            out.push(replace_case(format!("replace-whole-run-{tag}"), f, &whole, "", false, false));
        }
        if let Some(phrase) = field_phrase(&mut pres) {
            out.push(replace_case(format!("replace-field-{tag}"), f, &phrase, "F", false, false));
        }
        if let Some(phrase) = cross_run_phrase(&mut pres) {
            out.push(replace_case(format!("replace-cross-{tag}"), f, &phrase, "置換", false, false));
            if matches!(*f, "poi/with_japanese.pptx" | "poi/shapes.pptx") {
                out.push(replace_case(format!("replace-cross-refuse-{tag}"), f, &phrase, "置換\u{b}X", true, false));
            }
        }
    }
    out
}

