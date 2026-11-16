//! T3 (ADR-0005 §7), the in-cargo half: for every case, ooxml-pptx's read of
//! the edited output must equal `apply(edits, read(input))`, computed on
//! the whole deck model, including the full subtree of every `a:rPr`,
//! `a:pPr` and `a:endParaRPr` (review R2-2). A write to the wrong shape,
//! cell, paragraph or run, or a lost formatting child, shows up as a
//! difference. The independent half (python-pptx reads input and output and
//! applies the edits to its own tree) is `tests/oracle/check.py`, run by
//! the `pptx-readback` CI job.

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
/// Phase 0.1: the whole subtree is copied, except the link children
/// (`a:hlinkClick`, `a:hlinkMouseOver`); nothing left → no element.
fn run_fmt(p: &Value) -> Value {
    let mut f = runs(p).first().map_or_else(|| p["end"].clone(), |r| r[2].clone());
    if let Some(children) = f.get_mut("children").and_then(Value::as_array_mut) {
        const A: &str = "{http://schemas.openxmlformats.org/drawingml/2006/main}";
        children.retain(|c| c["name"] != format!("{A}hlinkClick") && c["name"] != format!("{A}hlinkMouseOver"));
        if children.is_empty() && f["attrs"].as_object().is_some_and(serde_json::Map::is_empty) {
            f = Value::Null;
        }
    }
    f
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
                    // R2-5: the run format source also becomes `a:endParaRPr`
                    let fmt = run_fmt(&last);
                    let items = build(&segs(line), None, &fmt);
                    paras.push(json!({"ppr": last["ppr"], "end": fmt, "items": items}));
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

fn expected_after(case: &cases::Case, edits: &[Value]) -> Value {
    let mut input = Presentation::from_bytes(cases::read_fixture(&case.fixture)).unwrap();
    let mut expected = model::dump(&mut input);
    for e in edits {
        apply(&mut expected, e);
    }
    expected
}

fn actual(case: &cases::Case) -> Value {
    model::dump(&mut Presentation::from_bytes(case.output.clone()).unwrap())
}

/// The oracle must notice an edit on the wrong target: pretend the first
/// edit of a sweep went to another shape and check the models differ.
#[test]
fn t3_detects_wrong_shape() {
    let case = cases::sweep_case("poi/sample.pptx");
    let mut edits: Vec<Value> = case.edits.iter().map(cases::Edit::to_json).collect();
    let expected = expected_after(&case, &edits);
    assert_eq!(actual(&case), expected);
    let other = expected["slides"][0]["shapes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["shape_id"].clone())
        .find(|id| *id != edits[0]["shape_id"])
        .unwrap();
    edits[0]["shape_id"] = other;
    assert_ne!(actual(&case), expected_after(&case, &edits));
}

/// Review R2-2: retargeting an edit to another paragraph, or to another
/// run of the same paragraph, must also be noticed. The edits come from
/// the multi-edit sweeps, which address paragraphs and runs past index 0;
/// each is replayed alone on a fresh copy of its fixture.
#[test]
fn t3_detects_wrong_paragraph_and_run() {
    let (mut para, mut run) = (false, false);
    for f in cases::FIXTURES {
        let case = cases::multi_case(f);
        let input = expected_after(&case, &[]);
        for e in &case.edits {
            if para && run {
                return;
            }
            let j = e.to_json();
            let Some(body) = input["slides"][e.slide]["shapes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["shape_id"] == j["shape_id"] && s["cell"] == j["cell"])
            else {
                continue;
            };
            let paras = body["paragraphs"].as_array().unwrap();
            let p = j["args"]["p"].as_u64().map(|p| p as usize);
            let mut changed = j.clone();
            match &e.op {
                cases::Op::ParaSetText { .. } if !para && paras.len() > 1 && p.unwrap() < paras.len() => {
                    changed["args"]["p"] = json!(if p.unwrap() == 0 { 1 } else { 0 });
                }
                cases::Op::RunSetText { r, .. }
                    if !run
                        && p.unwrap() < paras.len()
                        && paras[p.unwrap()]["items"].as_array().unwrap().iter().filter(|i| i[0] == "r").count() > (*r).max(1) =>
                {
                    changed["args"]["r"] = json!(if *r == 0 { 1 } else { 0 });
                }
                _ => continue,
            }
            let single = cases::run_case(format!("{}-single", case.name), f, |_| vec![e.clone()]);
            if single.edits.is_empty() {
                continue; // refused on the fresh copy
            }
            assert_eq!(actual(&single), expected_after(&single, std::slice::from_ref(&j)));
            assert_ne!(actual(&single), expected_after(&single, &[changed]), "{}: retarget of {j} unnoticed", case.name);
            match e.op {
                cases::Op::ParaSetText { .. } => para = true,
                _ => run = true,
            }
        }
    }
    assert!(para && run, "no edit to retarget (paragraph: {para}, run: {run})");
}

/// Review R2-2: an output that lost the child elements of a kept `a:rPr`
/// (here: the first run of the title in `sample.pptx`, whose `a:rPr` has an
/// `a:sym`) must fail T3. The round-2 model compared attributes only and
/// could not tell; the attribute-only projection below shows that.
#[test]
fn t3_detects_dropped_format_children() {
    let case = cases::named_cases().into_iter().find(|c| c.name == "title-and-body").unwrap();
    let edits: Vec<Value> = case.edits.iter().map(cases::Edit::to_json).collect();
    let expected = expected_after(&case, &edits);
    assert_eq!(actual(&case), expected);
    // the mutation: drop every child of the first non-empty a:rPr of slide 1
    let mut pkg = ooxml_core::Package::from_bytes(case.output.clone()).unwrap();
    let part = "ppt/slides/slide1.xml";
    let xml = String::from_utf8(pkg.read_part(part).unwrap()).unwrap();
    let start = xml.find("<a:rPr ").unwrap();
    let open_end = start + xml[start..].find('>').unwrap();
    assert_ne!(&xml[open_end - 1..open_end], "/", "the first a:rPr has children");
    let close = start + xml[start..].find("</a:rPr>").unwrap();
    let mutated = format!("{}/>{}", &xml[..open_end], &xml[close + "</a:rPr>".len()..]);
    pkg.set_part(part, mutated.into_bytes()).unwrap();
    let mutant = model::dump(&mut Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap());
    assert_ne!(mutant, expected, "T3 must notice the lost a:sym");
    assert_eq!(model::attrs_only(&mutant), model::attrs_only(&expected), "an attribute-only model would not");
}
