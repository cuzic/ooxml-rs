//! T3 (ADR-0005 §7), the in-cargo half: for every case, ooxml-pptx's read of
//! the edited output must equal `apply(edits, read(input))`, computed on
//! the whole deck model. A write to the wrong shape, cell, paragraph or run
//! shows up as a difference somewhere else in the deck. The independent
//! half (python-pptx reads input and output) is `tests/oracle/check.py`,
//! run by the `pptx-readback` CI job.

#[path = "oracle/cases.rs"]
mod cases;
#[path = "oracle/model.rs"]
mod model;

use ooxml_pptx::Presentation;
use serde_json::{Value, json};

fn segs(text: &str) -> Vec<String> {
    text.split('\u{b}').map(String::from).collect()
}

/// Items of a paragraph built from `\v` segments: segment 0 goes to
/// `first` (an existing run, kept even if empty) or to a new run if
/// non-empty; each later segment is a break plus a run if non-empty.
fn build(segments: &[String], first: Option<Value>, fmt: &Value) -> Vec<Value> {
    let mut items = Vec::new();
    match first {
        Some(mut r) => {
            r[1] = json!(segments[0]);
            items.push(r);
        }
        None if !segments[0].is_empty() => items.push(json!(["r", segments[0], fmt])),
        None => {}
    }
    for s in &segments[1..] {
        items.push(json!(["br"]));
        if !s.is_empty() {
            items.push(json!(["r", s, fmt]));
        }
    }
    items
}

fn runs(p: &Value) -> Vec<Value> {
    p["items"].as_array().unwrap().iter().filter(|i| i[0] == "r").cloned().collect()
}

/// Format source for new runs: the first run's rPr, else `endParaRPr`.
fn run_fmt(p: &Value) -> Value {
    runs(p).first().map_or_else(|| p["end"].clone(), |r| r[2].clone())
}

fn para_set(p: &mut Value, text: &str) {
    let first = runs(p).into_iter().next();
    let fmt = run_fmt(p);
    p["items"] = Value::Array(build(&segs(text), first, &fmt));
}

fn nth_kind(items: &[Value], kind: &str, n: usize) -> usize {
    items.iter().enumerate().filter(|(_, i)| i[0] == kind).nth(n).map(|(k, _)| k).expect("item exists")
}

fn apply(model: &mut Value, e: &Value) {
    let slide = &mut model["slides"][e["slide"].as_u64().unwrap() as usize];
    let target = slide["shapes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["shape_id"] == e["shape_id"] && s["cell"] == e["cell"])
        .expect("target in model");
    let paras = target["paragraphs"].as_array_mut().unwrap();
    let a = &e["args"];
    let pi = || a["p"].as_u64().unwrap() as usize;
    let text = || a["text"].as_str().unwrap().to_owned();
    match e["op"].as_str().unwrap() {
        "run_set_text" => {
            let items = paras[pi()]["items"].as_array_mut().unwrap();
            let k = nth_kind(items, "r", a["r"].as_u64().unwrap() as usize);
            items[k][1] = json!(text());
        }
        "para_set_text" => para_set(&mut paras[pi()], &text()),
        "add_run" => paras[pi()]["items"].as_array_mut().unwrap().push(json!(["r", text(), null])),
        "add_break" => paras[pi()]["items"].as_array_mut().unwrap().push(json!(["br"])),
        "remove_run" | "remove_field" => {
            let (kind, n) = if e["op"] == "remove_run" { ("r", &a["r"]) } else { ("fld", &a["f"]) };
            let items = paras[pi()]["items"].as_array_mut().unwrap();
            let k = nth_kind(items, kind, n.as_u64().unwrap() as usize);
            items.remove(k);
        }
        "add_paragraph" => {
            paras.push(json!({"ppr": null, "end": null, "items": build(&segs(&text()), None, &Value::Null)}));
        }
        "remove_paragraph" => {
            paras.remove(pi());
        }
        "body_set_text" => {
            let t = text().replace("\r\n", "\n").replace('\r', "\n");
            let lines: Vec<&str> = t.split('\n').collect();
            let last = paras.last().unwrap().clone();
            let n = paras.len();
            for (i, line) in lines.iter().enumerate() {
                if i < n {
                    para_set(&mut paras[i], line);
                } else {
                    let items = build(&segs(line), None, &run_fmt(&last));
                    paras.push(json!({"ppr": last["ppr"], "end": null, "items": items}));
                }
            }
            paras.truncate(lines.len());
        }
        op => panic!("unknown op {op}"),
    }
}

#[test]
fn t3_expected_model_after_edits() {
    let mut total = 0;
    let mut refused = std::collections::BTreeMap::<String, usize>::new();
    for case in cases::all_cases() {
        let mut input = Presentation::from_bytes(cases::read_fixture(&case.fixture)).unwrap();
        let mut expected = model::dump(&mut input);
        for e in &case.edits {
            apply(&mut expected, &e.to_json());
        }
        let mut output = Presentation::from_bytes(case.output.clone()).unwrap();
        let actual = model::dump(&mut output);
        assert!(actual == expected, "{}: model mismatch\nactual:   {actual}\nexpected: {expected}", case.name);
        total += case.edits.len();
        for (_, r) in &case.refused {
            let kind = r.split([' ', '(', '{']).next().unwrap().to_owned();
            *refused.entry(kind).or_default() += 1;
        }
    }
    eprintln!("T3 (Rust model): {total} edits applied and checked; refused: {refused:?}");
    assert!(total > 300, "the sweep should exercise many edits, got {total}");
}

/// The oracle must notice an edit on the wrong target: pretend the first
/// edit of a sweep went to another shape and check the models differ.
#[test]
fn t3_detects_wrong_target() {
    let case = cases::sweep_case("poi/sample.pptx");
    let mut input = Presentation::from_bytes(cases::read_fixture(&case.fixture)).unwrap();
    let mut expected = model::dump(&mut input);
    let mut edits: Vec<Value> = case.edits.iter().map(cases::Edit::to_json).collect();
    let other = expected["slides"][0]["shapes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["shape_id"].clone())
        .find(|id| *id != edits[0]["shape_id"])
        .unwrap();
    edits[0]["shape_id"] = other;
    for e in &edits {
        apply(&mut expected, e);
    }
    let mut output = Presentation::from_bytes(case.output.clone()).unwrap();
    assert_ne!(model::dump(&mut output), expected);
}
