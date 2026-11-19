//! Phase 1b (ADR-0013 §4): SmartArt node text, edited in the data model
//! (`dgm:pt/dgm:t`) and mirrored into the drawing cache (`dsp:sp/dsp:txBody`).
//!
//! The shapes showing a node are the drawing shapes whose `modelId` is a
//! `presOf` destination of the node; in each, the node's **slice** is its
//! paragraphs within the concatenation of the shape's sources (by
//! `destOrd`, then `srcOrd`; a source without `dgm:t` counts as one empty
//! paragraph). Before any change the cache must pair with the data:
//! (1) the shape's paragraphs equal that concatenation; (2) no two sources
//! share a `destOrd`; (3) per paired paragraph, the runs with text (empty
//! runs are ignored on both sides) pair one to one and every data run's
//! `a:rPr` attribute, proofing aside, is on its cache run. Edits are the
//! Phase 0/0.1 text setters, applied to the data body and to each slice;
//! both parts are edited on clones and installed in one registry
//! transaction, so a refusal changes nothing.

use ooxml_core::{PartContent, QName, RawNode, RegistryPlan, XmlDocument};
use ooxml_schema::dml;

use super::{EditPart, Guard, TextBodyMut, node_at};
use crate::error::{Error, Result};
use crate::names::{R_NS, REL_DIAGRAM_DATA, REL_DIAGRAM_DRAWING};
use crate::read::{self, ShapeKind};

/// `a:graphicData@uri` of a SmartArt frame.
pub(crate) const DIAGRAM_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
const DGM: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
const DSP: &str = "http://schemas.microsoft.com/office/drawing/2008/diagram";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
/// Proofing attributes that may differ between a data run and its cache run.
const PROOFING: [&str; 5] = ["lang", "altLang", "dirty", "err", "smtClean"];

fn q(ns: &'static str, l: &'static str) -> QName {
    QName::new(ns, l)
}

/// A data point that holds text: `dgm:pt` of type `node` (default) or `asst`.
#[derive(Clone, Debug)]
struct Node {
    model_id: String,
    /// Child index of the `dgm:pt` in `dgm:ptLst`.
    pt: usize,
    /// Child index of `dgm:t` in the point.
    body: Option<usize>,
}

/// One node of a SmartArt, for reading (ADR-0013 §4 model).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartArtNode {
    /// `dgm:pt@modelId`.
    pub model_id: String,
    /// The node's paragraphs as text (`\v` for `a:br`); one empty
    /// paragraph when it has no `dgm:t`.
    pub paragraphs: Vec<String>,
}

fn para_text(p: &RawNode) -> String {
    let mut s = String::new();
    for c in p.elements() {
        if c.is(q(A, "r")) || c.is(q(A, "fld")) {
            for t in c.elements().filter(|e| e.is(q(A, "t"))) {
                s.push_str(&t.text());
            }
        } else if c.is(q(A, "br")) {
            s.push('\u{b}');
        }
    }
    s
}

fn paragraphs(body: Option<&RawNode>) -> Vec<&RawNode> {
    body.map_or_else(Vec::new, |b| b.elements().filter(|e| e.is(q(A, "p"))).collect())
}

/// Runs with text (empty runs are ignored on both sides, review round 3 P3):
/// `(index among all a:r, node)`.
fn text_runs(p: &RawNode) -> Vec<(usize, &RawNode)> {
    p.elements().filter(|e| e.is(q(A, "r"))).enumerate().filter(|(_, r)| !run_text(r).is_empty()).collect()
}

fn run_text(r: &RawNode) -> String {
    r.elements().filter(|e| e.is(q(A, "t"))).map(|t| t.text().into_owned()).collect()
}

/// Does the run's `a:rPr` subtree hold a relationship id (a hyperlink)?
fn has_link(r: &RawNode) -> bool {
    fn any_r(n: &RawNode) -> bool {
        n.attrs().iter().any(|a| a.name().ns() == R_NS) || n.elements().any(any_r)
    }
    r.elements().filter(|e| e.is(q(A, "rPr"))).any(any_r)
}

fn data_nodes(root: &RawNode) -> Result<(usize, Vec<Node>)> {
    let (pl, lst) = root.indexed_elements().find(|(_, e)| e.is(q(DGM, "ptLst"))).ok_or(Error::NotSmartArt)?;
    let nodes = lst
        .indexed_elements()
        .filter(|(_, e)| e.is(q(DGM, "pt")) && matches!(e.attr(QName::new("", "type")).unwrap_or("node"), "node" | "asst"))
        .filter_map(|(i, e)| {
            let id = e.attr(QName::new("", "modelId"))?.to_owned();
            let body = e.indexed_elements().find(|(_, c)| c.is(q(DGM, "t"))).map(|(j, _)| j);
            Some(Node { model_id: id, pt: i, body })
        })
        .collect();
    Ok((pl, nodes))
}

/// `presOf` connections: `(destId, destOrd, srcOrd, srcId)`.
fn pres_of(root: &RawNode) -> Vec<(String, i64, i64, String)> {
    let num = |e: &RawNode, a: &'static str| e.attr(QName::new("", a)).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
    root.elements()
        .filter(|e| e.is(q(DGM, "cxnLst")))
        .flat_map(|l| l.elements())
        .filter(|c| c.is(q(DGM, "cxn")) && c.attr(QName::new("", "type")) == Some("presOf"))
        .filter_map(|c| {
            Some((
                c.attr(QName::new("", "destId"))?.to_owned(),
                num(c, "destOrd"),
                num(c, "srcOrd"),
                c.attr(QName::new("", "srcId"))?.to_owned(),
            ))
        })
        .collect()
}

/// Drawing shapes with text bodies: `(path to dsp:txBody, modelId)`.
fn drawing_shapes(root: &RawNode) -> Vec<(Vec<usize>, String)> {
    fn walk(n: &RawNode, path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, String)>) {
        for (i, c) in n.indexed_elements() {
            if c.is(q(DSP, "spTree")) || c.is(q(DSP, "grpSp")) {
                path.push(i);
                walk(c, path, out);
                path.pop();
            } else if c.is(q(DSP, "sp"))
                && let Some(id) = c.attr(QName::new("", "modelId"))
                && let Some((b, _)) = c.indexed_elements().find(|(_, e)| e.is(q(DSP, "txBody")))
            {
                let mut p = path.clone();
                p.extend([i, b]);
                out.push((p, id.to_owned()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut Vec::new(), &mut out);
    out
}

/// The cache reference of the data model: `dgm:extLst/a:ext/dsp:dataModelExt@relId`.
fn data_model_ext(root: &RawNode) -> Option<(usize, usize, String)> {
    let (li, lst) = root.indexed_elements().find(|(_, e)| e.is(q(DGM, "extLst")))?;
    lst.indexed_elements().find_map(|(ei, ext)| {
        let d = ext.elements().find(|e| e.is(q(DSP, "dataModelExt")))?;
        Some((li, ei, d.attr(QName::new("", "relId"))?.to_owned()))
    })
}

/// What an edit writes, for the hyperlink check (ADR-0013 §4 P4b).
#[derive(Clone, Copy)]
enum Which {
    /// Every run of the node.
    Node,
    /// Every run of paragraph `p`.
    Paragraph(usize),
    /// Data run `r` (an `a:r` index) of paragraph `p`; `k` = the number of
    /// runs with text before it (its cache pair, or where a new cache run
    /// goes); `was_text` = it has text.
    Run { p: usize, r: usize, k: usize, was_text: bool },
}

/// One drawing shape showing a node, and the node's slice in it.
struct Slice {
    body: Vec<usize>,
    start: usize,
    len: usize,
    /// Data runs with text paired with a cache run in this slice.
    runs: usize,
}

/// What [`SmartArtMut::cache_report`] found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheReport {
    /// Text nodes (`node`/`asst` points).
    pub nodes: usize,
    /// Nodes shown by at least one drawing-cache shape.
    pub shown: usize,
    /// Nodes whose cache slices pair with the data (editable).
    pub consistent: usize,
    /// Paired runs with text over the consistent nodes' slices.
    pub run_pairs: usize,
}

/// A SmartArt (graphic frame with a diagram) opened for text edits
/// (ADR-0013 §4).
#[derive(Debug)]
pub struct SmartArtMut<'a> {
    pres: &'a mut crate::Presentation,
    slide_part: String,
    data: String,
    /// The drawing cache: `Ok(None)` none, `Err(())` a reference that does
    /// not resolve (`SmartArtCacheUnresolved`).
    drawing: std::result::Result<Option<String>, ()>,
    guard: Guard,
}

impl crate::Presentation {
    /// The SmartArt in graphic frame `frame_id` on slide `slide`.
    ///
    /// Errors: [`Error::ShapeNotFound`], [`Error::InsideAlternateContent`],
    /// [`Error::NotSmartArt`] (not a diagram, or its data part is missing).
    pub fn smart_art_mut(&mut self, slide: usize, frame_id: u32) -> Result<SmartArtMut<'_>> {
        let (_, part) = self.entry(slide)?;
        let refs = self.comment_refs(&part)?;
        let doc = self.pkg.xml_part(&part)?;
        let shape = read::find_by_id(read::shapes_of(doc), frame_id)?.ok_or(Error::ShapeNotFound)?;
        if *shape.kind() == ShapeKind::AlternateContent {
            return Err(Error::InsideAlternateContent);
        }
        let frame = shape.raw();
        let gd = frame
            .elements()
            .find(|e| e.is(q(A, "graphic")))
            .and_then(|g| g.elements().find(|e| e.is(q(A, "graphicData"))))
            .filter(|gd| gd.attr(QName::new("", "uri")) == Some(DIAGRAM_URI))
            .ok_or(Error::NotSmartArt)?;
        let dm = gd
            .elements()
            .find(|e| e.is(q(DGM, "relIds")))
            .and_then(|r| r.attr(QName::new(R_NS, "dm")))
            .map(str::to_owned)
            .ok_or(Error::NotSmartArt)?;
        let rels = self.pkg.relationships(&part)?;
        let data = rels
            .by_id(&dm)
            .filter(|r| r.rel_type == REL_DIAGRAM_DATA)
            .and_then(|r| rels.resolve(r))
            .filter(|p| self.pkg.contains(p))
            .ok_or(Error::NotSmartArt)?;
        let data_doc = self.pkg.xml_part(&data)?;
        if !data_doc.root().is(q(DGM, "dataModel")) {
            return Err(Error::NotSmartArt);
        }
        // the cache is named from the **slide's** relationships
        let drawing = match data_model_ext(data_doc.root()) {
            None => Ok(None),
            // the relationship must be a diagramDrawing one to a dsp:drawing
            // part (review #63 m2), else the reference is unresolved
            Some((_, _, rid)) => rels
                .by_id(&rid)
                .filter(|r| r.rel_type == REL_DIAGRAM_DRAWING)
                .and_then(|r| rels.resolve(r))
                .filter(|p| self.pkg.contains(p))
                .map(Some)
                .ok_or(()),
        };
        let drawing = match drawing {
            Ok(Some(d)) if !self.pkg.xml_part(&d)?.root().is(q(DSP, "drawing")) => Err(()),
            other => other,
        };
        let guard = Guard {
            links: false,
            shape_id: frame_id,
            paragraphs: false, // node animations target data model ids, not paragraphs (review m6)
            table: false,
            characters: false,
            comments: refs.comments.covers(&[frame_id]),
        };
        Ok(SmartArtMut { pres: self, slide_part: part, data, drawing, guard })
    }
}

impl SmartArtMut<'_> {
    /// The data model's text nodes (`node`/`asst` points) in document order.
    pub fn nodes(&mut self) -> Result<Vec<SmartArtNode>> {
        let doc = self.pres.pkg.xml_part(&self.data)?;
        let root = doc.root();
        let (pl, nodes) = data_nodes(root)?;
        Ok(nodes
            .iter()
            .map(|n| {
                let pt = node_at(root, &[pl, n.pt]).expect("point");
                let body = n.body.and_then(|b| node_at(pt, &[b]));
                let ps = paragraphs(body);
                SmartArtNode {
                    model_id: n.model_id.clone(),
                    paragraphs: if ps.is_empty() { vec![String::new()] } else { ps.into_iter().map(para_text).collect() },
                }
            })
            .collect())
    }

    /// Checks every node's cache slices against the data (the invariants
    /// of ADR-0013 §4), changing nothing.
    pub fn cache_report(&mut self) -> Result<CacheReport> {
        let drawing = self.drawing.clone().map_err(|()| Error::SmartArtCacheUnresolved)?;
        let data = self.pres.pkg.xml_part(&self.data)?.clone();
        let (_, nodes) = data_nodes(data.root())?;
        let mut r = CacheReport { nodes: nodes.len(), ..CacheReport::default() };
        let Some(d) = drawing else { return Ok(r) };
        let cache = self.pres.pkg.xml_part(&d)?.clone();
        for n in &nodes {
            match self.slices(&data, &nodes, n, cache.root()) {
                Ok(s) if s.is_empty() => {}
                Ok(s) => {
                    r.shown += 1;
                    r.consistent += 1;
                    r.run_pairs += s.iter().map(|x| x.runs).sum::<usize>();
                }
                Err(Error::SmartArtCacheInconsistent(_)) => r.shown += 1,
                Err(e) => return Err(e),
            }
        }
        Ok(r)
    }

    /// Replaces the node's whole text (lines become paragraphs, as
    /// [`TextBodyMut::set_text`]) in the data and in its cache slices.
    pub fn set_text(&mut self, model_id: &str, text: &str) -> Result<()> {
        self.edit(model_id, Which::Node, !text.is_empty(), |body, slice| match slice {
            None => body.set_text(text),
            Some((start, len)) => body.set_text_range(start, len, text),
        })
    }

    /// Replaces the text of paragraph `p` of the node (as
    /// [`crate::ParagraphMut::set_text`]).
    pub fn set_paragraph_text(&mut self, model_id: &str, p: usize, text: &str) -> Result<()> {
        self.edit(model_id, Which::Paragraph(p), !text.is_empty(), |body, slice| {
            let start = slice.map_or(0, |s| s.0);
            body.paragraph_mut(start + p)?.set_text(text)
        })
    }

    /// Replaces the text of run `r` (an `a:r` index) of paragraph `p` of the
    /// node (as [`crate::RunMut::set_text`]). The cache run is the one
    /// paired with it among the runs with text; text put into an empty data
    /// run goes into the cache after the cache run paired with the
    /// preceding run with text (at the paragraph start when there is none),
    /// copying that run's format.
    pub fn set_run_text(&mut self, model_id: &str, p: usize, r: usize, text: &str) -> Result<()> {
        // where the data run sits among the runs with text, decided before the edit
        let doc = self.pres.pkg.xml_part(&self.data)?.clone();
        let (pl, nodes) = data_nodes(doc.root())?;
        let n = nodes.iter().find(|n| n.model_id == model_id).ok_or_else(|| Error::SmartArtNodeNotFound(model_id.to_owned()))?;
        let pt = node_at(doc.root(), &[pl, n.pt]).expect("point");
        let ps = paragraphs(n.body.and_then(|b| node_at(pt, &[b])));
        let para = ps.get(p).ok_or(Error::ParagraphNotFound(p))?;
        let runs: Vec<&RawNode> = para.elements().filter(|e| e.is(q(A, "r"))).collect();
        let run = runs.get(r).ok_or(Error::RunNotFound(r))?;
        let was_text = !run_text(run).is_empty();
        let k = runs[..r].iter().filter(|x| !run_text(x).is_empty()).count();
        self.edit(model_id, Which::Run { p, r, k, was_text }, !text.is_empty(), |body, slice| match slice {
            None => body.paragraph_mut(p)?.run_mut(r)?.set_text(text),
            Some((start, _)) => {
                let mut para = body.paragraph_mut(start + p)?;
                let cache = para_runs_with_text(&para)?;
                if was_text {
                    let c = *cache.get(k).ok_or_else(|| Error::SmartArtCacheInconsistent("run pairing".into()))?;
                    para.run_mut(c)?.set_text(text)
                } else if text.is_empty() {
                    Ok(())
                } else {
                    let after = k.checked_sub(1).map(|i| cache[i]);
                    let fmt = after.or_else(|| cache.first().copied());
                    para.insert_run_after(after, text, fmt)
                }
            }
        })
    }

    /// The core of every edit: checks, data edit, cache edits, `phldr`,
    /// one transaction. `which` = (paragraph, Some(run among runs with
    /// text)) for the hyperlink check of narrower edits.
    fn edit(
        &mut self,
        model_id: &str,
        which: Which,
        nonempty: bool,
        f: impl Fn(&mut TextBodyMut<'_>, Option<(usize, usize)>) -> Result<()>,
    ) -> Result<()> {
        let drawing = self.drawing.clone().map_err(|()| Error::SmartArtCacheUnresolved)?;
        let mut data = self.pres.pkg.xml_part(&self.data)?.clone();
        let (pl, nodes) = data_nodes(data.root())?;
        let n = nodes.iter().find(|n| n.model_id == model_id).ok_or_else(|| Error::SmartArtNodeNotFound(model_id.to_owned()))?.clone();
        let mut cache = match &drawing {
            Some(d) => Some(self.pres.pkg.xml_part(d)?.clone()),
            None => None,
        };
        let slices = match &cache {
            Some(c) => self.slices(&data, &nodes, &n, c.root())?,
            None => Vec::new(),
        };
        // hyperlink runs cannot be mirrored (their r:id differs between the parts)
        check_links(&data, &[pl, n.pt], n.body, cache.as_ref(), &slices, which)?;
        // a node without dgm:t gets one (empty bodyPr, lstStyle, one paragraph)
        let body = match n.body {
            Some(b) => b,
            None => create_body(&mut data, pl, n.pt)?,
        };
        {
            let mut tb = TextBodyMut { doc: EditPart::Staged(&mut data), path: vec![pl, n.pt, body], guard: self.guard.clone() };
            f(&mut tb, None)?;
        }
        if nonempty {
            clear_placeholder(&mut data, pl, n.pt)?;
        }
        if let Some(c) = cache.as_mut() {
            for s in &slices {
                let mut tb = TextBodyMut { doc: EditPart::Staged(c), path: s.body.clone(), guard: self.guard.clone() };
                f(&mut tb, Some((s.start, s.len)))?;
            }
        }
        // the edit must not leave any node's slices inconsistent that were
        // consistent before (review #63 M2, m1: e.g. an empty data run whose
        // own format the copied cache format lacks, or a node shown twice
        // in one shape)
        if let Some(c) = &cache {
            let data0 = self.pres.pkg.xml_part(&self.data)?.clone();
            let cache0 = self.pres.pkg.xml_part(drawing.as_ref().expect("cache"))?.clone();
            let before = self.consistency(&data0, &cache0)?;
            let after = self.consistency(&data, c)?;
            for (id, ok) in before {
                if ok && !after.iter().any(|(j, ok2)| *j == id && *ok2) {
                    return Err(Error::SmartArtCacheInconsistent(format!("the edit would leave node {id}'s cache text out of step")));
                }
            }
        }
        let mut plan = RegistryPlan::new(&self.pres.pkg);
        plan.stage_part(&self.pres.pkg, &self.data, PartContent::Xml(data))?;
        if let (Some(d), Some(c)) = (&drawing, cache)
            && !slices.is_empty()
        {
            plan.stage_part(&self.pres.pkg, d, PartContent::Xml(c))?;
        }
        self.pres.pkg.apply(plan)?;
        Ok(())
    }

    /// Per node: do its cache slices pair with the data?
    fn consistency(&self, data: &XmlDocument, cache: &XmlDocument) -> Result<Vec<(String, bool)>> {
        let (_, nodes) = data_nodes(data.root())?;
        Ok(nodes
            .iter()
            .map(|n| (n.model_id.clone(), !matches!(self.slices(data, &nodes, n, cache.root()), Err(Error::SmartArtCacheInconsistent(_)))))
            .collect())
    }

    /// The node's slices in the drawing cache, after checking the
    /// invariants of ADR-0013 §4 (else `SmartArtCacheInconsistent`).
    fn slices(&self, data: &XmlDocument, nodes: &[Node], n: &Node, cache: &RawNode) -> Result<Vec<Slice>> {
        let bad = |m: &str| Error::SmartArtCacheInconsistent(m.to_owned());
        let shapes = drawing_shapes(cache);
        if shapes.is_empty() {
            return Ok(Vec::new()); // a cache without shapes: consumers lay out themselves
        }
        let (pl, _) = data_nodes(data.root())?;
        let paras_of = |id: &str| -> Option<Vec<&RawNode>> {
            let m = nodes.iter().find(|m| m.model_id == id)?;
            let pt = node_at(data.root(), &[pl, m.pt]).expect("point");
            Some(paragraphs(m.body.and_then(|b| node_at(pt, &[b]))))
        };
        let links = pres_of(data.root());
        let mut out = Vec::new();
        for (body, mid) in shapes {
            let mut srcs: Vec<(i64, i64, String)> =
                links.iter().filter(|(d, ..)| *d == mid).map(|(_, d, s, src)| (*d, *s, src.clone())).collect();
            if !srcs.iter().any(|(_, _, s)| *s == n.model_id) {
                continue;
            }
            srcs.sort();
            if srcs.windows(2).any(|w| w[0].0 == w[1].0) {
                return Err(bad("two presOf sources share a destOrd"));
            }
            let cbody = node_at(cache, &body).expect("body");
            let cparas = paragraphs(Some(cbody));
            // the concatenation of the sources' paragraphs
            let mut concat: Vec<Option<&RawNode>> = Vec::new();
            let mut start = None;
            let mut len = 0;
            let mut runs = 0;
            for (_, _, src) in &srcs {
                let ps = paras_of(src).unwrap_or_default();
                let here = if ps.is_empty() { vec![None] } else { ps.into_iter().map(Some).collect::<Vec<_>>() };
                if *src == n.model_id && start.is_none() {
                    start = Some(concat.len());
                    len = here.len();
                }
                concat.extend(here);
            }
            if concat.len() != cparas.len() {
                return Err(bad("the shape's paragraphs differ from its sources"));
            }
            let slice = start.map_or(0..0, |s0| s0..s0 + len);
            for (k, (d, c)) in concat.iter().zip(&cparas).enumerate() {
                let dt = d.map(para_text).unwrap_or_default();
                if dt != para_text(c) {
                    return Err(bad("the shape's text differs from its sources"));
                }
                let Some(d) = d else { continue };
                let (dr, cr) = (text_runs(d), text_runs(c));
                if dr.len() != cr.len() {
                    return Err(bad("paired paragraphs have different runs"));
                }
                if slice.contains(&k) {
                    runs += dr.len();
                }
                for ((_, x), (_, y)) in dr.iter().zip(&cr) {
                    let (xa, ya) = (rpr(x), rpr(y));
                    for a in xa.map(RawNode::attrs).unwrap_or_default() {
                        if a.name().ns().is_empty() && PROOFING.contains(&a.name().local()) {
                            continue;
                        }
                        if ya.and_then(|y| y.attrs().iter().find(|b| b.name() == a.name())).map(|b| b.value()) != Some(a.value()) {
                            return Err(bad("a data run's format is not on its cache run"));
                        }
                    }
                }
            }
            out.push(Slice { body, start: start.expect("source found"), len, runs });
        }
        Ok(out)
    }

    /// Removes the drawing cache (ADR-0013 §4 "explicit alternative"): the
    /// data model's `dsp:dataModelExt` extension (and `dgm:extLst` when it
    /// becomes empty), the drawing part with every part only it references,
    /// and the slide's `diagramDrawing` relationship — one transaction.
    ///
    /// Errors: [`Error::SmartArtCacheUnresolved`], [`Error::SmartArtCacheShared`]
    /// (another part references the drawing), `Core(…)` (an unreadable
    /// relationships part, a reference the core finds).
    pub fn drop_drawing_cache(&mut self) -> Result<()> {
        let Some(drawing) = self.drawing.clone().map_err(|()| Error::SmartArtCacheUnresolved)? else { return Ok(()) };
        // the data model without the cache reference (parsed before borrowing the package)
        let mut data = self.pres.pkg.xml_part(&self.data)?.clone();
        let pkg = &self.pres.pkg;
        // fail closed on any unreadable relationships part (ADR-0013 §4, P2)
        for name in pkg.part_names() {
            if name.to_ascii_lowercase().ends_with(".rels")
                && !pkg.read_part(name).ok().is_some_and(|b| ooxml_core::opc::Relationships::parse(None, &b).is_ok())
            {
                return Err(ooxml_core::Error::Package(format!("relationships part {name:?} cannot be read")).into());
            }
        }
        let incoming = pkg.incoming_relationships(&drawing)?;
        if incoming.len() != 1
            || incoming[0].source.as_deref() != Some(self.slide_part.as_str())
            || incoming[0].rel_type != REL_DIAGRAM_DRAWING
        {
            return Err(Error::SmartArtCacheShared);
        }
        // parts only the drawing references go with it
        let mut set = vec![drawing.clone()];
        let rels = pkg.relationships(&drawing)?;
        for r in rels.iter() {
            let Some(t) = rels.resolve(r).filter(|t| pkg.contains(t)) else { continue };
            let inc = pkg.incoming_relationships(&t)?;
            if inc.iter().all(|i| i.source.as_deref() == Some(drawing.as_str())) && !set.contains(&t) {
                set.push(t);
            }
        }
        remove_data_model_ext(&mut data)?;
        let mut plan = RegistryPlan::new(pkg);
        plan.stage_part(pkg, &self.data, PartContent::Xml(data))?;
        let names: Vec<&str> = set.iter().map(String::as_str).collect();
        plan.remove_parts(pkg, &names, &[REL_DIAGRAM_DRAWING])?;
        self.pres.pkg.apply(plan)?;
        self.drawing = Ok(None);
        Ok(())
    }
}

fn rpr(r: &RawNode) -> Option<&RawNode> {
    r.elements().find(|e| e.is(q(A, "rPr")))
}

/// The cache runs with text of a paragraph, as `a:r` indices.
fn para_runs_with_text(p: &crate::ParagraphMut<'_>) -> Result<Vec<usize>> {
    Ok(text_runs(p.as_ref()?.raw()).into_iter().map(|(i, _)| i).collect())
}

/// Refuses an edit that would write to (or copy the format of) a run whose
/// `a:rPr` holds a relationship id, in the data or in the cache (ADR-0013
/// §4 P4b; review #63 M1: the data run written is checked by its own index,
/// also when it is empty).
fn check_links(data: &XmlDocument, pt: &[usize], body: Option<usize>, cache: Option<&XmlDocument>, slices: &[Slice], which: Which) -> Result<()> {
    let refuse = || Err(Error::FormatNotCopyable("a:rPr"));
    fn all_runs(p: &RawNode) -> Vec<&RawNode> {
        p.elements().filter(|e| e.is(q(A, "r"))).collect()
    }
    let point = node_at(data.root(), pt).expect("point");
    let dps = paragraphs(body.and_then(|b| node_at(point, &[b])));
    let data_bad = match which {
        Which::Node => dps.iter().flat_map(|p| all_runs(p)).any(has_link),
        Which::Paragraph(p) => dps.get(p).is_some_and(|p| all_runs(p).into_iter().any(has_link)),
        Which::Run { p, r, .. } => dps.get(p).is_some_and(|p| all_runs(p).get(r).is_some_and(|x| has_link(x))),
    };
    if data_bad {
        return refuse();
    }
    let Some(c) = cache else { return Ok(()) };
    for s in slices {
        let cps: Vec<&RawNode> = paragraphs(node_at(c.root(), &s.body)).into_iter().skip(s.start).take(s.len).collect();
        let cache_bad = match which {
            Which::Node => cps.iter().flat_map(|p| all_runs(p)).any(has_link),
            Which::Paragraph(p) => cps.get(p).is_some_and(|p| all_runs(p).into_iter().any(has_link)),
            Which::Run { p, k, was_text, .. } => cps.get(p).is_some_and(|p| {
                let runs = text_runs(p);
                if was_text {
                    runs.get(k).is_some_and(|(_, x)| has_link(x))
                } else {
                    // the run whose format a new cache run copies
                    let src = k.checked_sub(1).and_then(|i| runs.get(i)).or_else(|| runs.first());
                    src.is_some_and(|(_, x)| has_link(x))
                }
            }),
        };
        if cache_bad {
            return refuse();
        }
    }
    Ok(())
}

/// Creates `dgm:t` (empty `a:bodyPr`, `a:lstStyle`, one `a:p`) in a point.
fn create_body(data: &mut XmlDocument, pl: usize, pt: usize) -> Result<usize> {
    let root = data.lens_mut::<dml::DgmDataModelRoot>()?;
    let lst = root.into_child_at::<dml::DgmPointList>(pl).ok_or(Error::NotSmartArt)?;
    let mut p = lst.into_child_at::<dml::DgmPoint>(pt).ok_or(Error::NotSmartArt)?;
    {
        let mut t = p.insert::<dml::DgmTextBody>()?;
        t.insert::<dml::BodyProperties>()?;
        t.insert::<dml::ListStyle>()?;
        t.insert::<dml::Paragraph>()?;
    }
    p.node().indexed_elements().find(|(_, e)| e.is(q(DGM, "t"))).map(|(i, _)| i).ok_or(Error::NotSmartArt)
}

/// Non-empty text on a placeholder node removes `prSet@phldr` (`phldrT`
/// is kept; ADR-0013 §4).
fn clear_placeholder(data: &mut XmlDocument, pl: usize, pt: usize) -> Result<()> {
    let point = node_at(data.root(), &[pl, pt]).expect("point");
    let Some((si, ps)) = point.indexed_elements().find(|(_, e)| e.is(q(DGM, "prSet"))) else { return Ok(()) };
    if !matches!(ps.attr(QName::new("", "phldr")), Some("1" | "true")) {
        return Ok(());
    }
    let root = data.lens_mut::<dml::DgmDataModelRoot>()?;
    let lst = root.into_child_at::<dml::DgmPointList>(pl).ok_or(Error::NotSmartArt)?;
    let p = lst.into_child_at::<dml::DgmPoint>(pt).ok_or(Error::NotSmartArt)?;
    let mut set = p.into_child_at::<dml::DgmPropertySet>(si).ok_or(Error::NotSmartArt)?;
    set.remove_attr(QName::new("", "phldr"))?;
    Ok(())
}

fn remove_data_model_ext(data: &mut XmlDocument) -> Result<()> {
    let Some((li, ei, _)) = data_model_ext(data.root()) else { return Ok(()) };
    let only = node_at(data.root(), &[li]).expect("extLst").elements().count() == 1;
    let mut root = data.lens_mut::<dml::DgmDataModelRoot>()?;
    if only {
        root.remove_at(li);
    } else {
        let mut lst = root.into_child_at::<dml::DgmDataModelExtensionList>(li).ok_or(Error::NotSmartArt)?;
        lst.remove_at(ei);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Presentation;
    use std::assert_matches;

    #[test]
    fn drop_refuses_when_the_only_incoming_relationship_has_the_wrong_type() {
        let mut pres = Presentation::open(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/fixtures/pptx/poi/SmartArt.pptx"
        ))
        .unwrap();
        let mut smartart = pres.smart_art_mut(0, 4).unwrap();
        let drawing = smartart.drawing.clone().unwrap().unwrap();
        let pkg = &mut smartart.pres.pkg;
        let slide = Some(smartart.slide_part.as_str());
        let incoming = pkg.incoming_relationships(&drawing).unwrap();
        // Change the package after opening the handle to exercise the drop
        // guard independently of the relationship check in smart_art_mut.
        pkg.remove_relationship(slide, &incoming[0].id, &[REL_DIAGRAM_DRAWING]).unwrap();
        pkg.add_relationship(slide, REL_DIAGRAM_DATA, ooxml_core::Target::Part(drawing)).unwrap();
        let before = smartart.pres.to_bytes().unwrap();

        assert_matches!(smartart.drop_drawing_cache(), Err(Error::SmartArtCacheShared));
        assert_eq!(smartart.pres.to_bytes().unwrap(), before);
    }
}
