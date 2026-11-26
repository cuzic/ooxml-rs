//! The projection P of oracle 1 (ADR-0024 "Test oracles" 1), over serialized
//! bytes, with its exclusions only. The predicates that justify them are J1's.
//!
//! P is a map from a path to the debug form of the typed model of that item
//! (cells, rows, parts), so two models are equal when every item is, and
//! [`first_diff`] names the first item that is not.

use std::collections::{BTreeMap, HashMap};

use ooxml_core::opc::Relationships;
use ooxml_core::{Package, QName, RawNode};

use super::state::{Tree, tree};
use crate::cell_state::FormulaState;
use crate::census::SheetKind;
use crate::formula::translate::translate;
use crate::reference::{CellIndex, MAX_COLS};
use crate::workbook::{REL_CALC_CHAIN, REL_METADATA, REL_SHARED_STRINGS, Workbook};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Model(BTreeMap<String, String>);

/// The path of the first item that differs (an item present on one side only counts).
pub(crate) fn first_diff(a: &Model, b: &Model) -> Option<String> {
    let mut keys: Vec<&String> = a.0.keys().chain(b.0.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .find(|k| a.0.get(*k) != b.0.get(*k))
        .map(|k| format!("{k}: {:?} vs {:?}", a.0.get(k), b.0.get(k)))
}

/// A cell value as P sees it: storage is not a type (decision A4).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Val {
    /// The bits of the parsed `v` text.
    Number(u64),
    Str {
        text: String,
        runs: Option<Runs>,
    },
    Bool(bool),
    Error(String),
    Empty,
    /// A formula cell or an array/data-table result cell: the cache is excluded.
    Cache,
}

/// The shape of an `f` element. Normal and shared formulas are both `Plain`:
/// the group is storage (decision A4).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FormulaKind {
    Plain,
    Array(String),
    DataTable {
        reference: String,
        dt2d: bool,
        dtr: bool,
        del1: bool,
        del2: bool,
        r1: Option<String>,
        r2: Option<String>,
    },
    /// A `t` the schema does not list.
    Other(String),
}

/// The attributes of an `f` element that are neither the group's shape nor
/// the data table's (PR 159 review, round 1 m3).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FormulaAttrs {
    ca: bool,
    aca: bool,
    bx: bool,
    other: Vec<(String, String, String)>,
}

/// A cell of the Base as an op that would write it back (the generator's "revert").
#[derive(Clone, Debug)]
pub(crate) struct BaseCell {
    pub(crate) sheet: usize,
    pub(crate) at: CellIndex,
    pub(crate) value: Val,
    pub(crate) formula: Option<String>,
    /// A shared-group member or an array/data-table cell.
    pub(crate) group: bool,
}

pub(crate) fn project(bytes: &[u8]) -> Model {
    read(bytes).0
}

pub(crate) fn base_cells(bytes: &[u8]) -> Vec<BaseCell> {
    read(bytes).1
}

fn local<'a>(n: &'a RawNode, name: &str) -> Option<&'a RawNode> {
    n.elements().find(|e| e.name().local() == name)
}

fn attr<'a>(n: &'a RawNode, name: &'static str) -> Option<&'a str> {
    n.attr(QName::new("", name))
}

fn el_name(t: &Tree) -> Option<&str> {
    match t {
        Tree::El { name, .. } => Some(name),
        Tree::Text(_) => None,
    }
}

/// Removes the children named `names` of the top element.
fn drop_children(t: &mut Tree, names: &[&str]) {
    if let Tree::El { children, .. } = t {
        children.retain(|c| {
            el_name(c).is_none_or(|n| !names.contains(&n.rsplit('}').next().unwrap_or(n)))
        });
    }
}

fn child_named<'a>(t: &'a Tree, name: &str) -> Option<&'a Tree> {
    match t {
        Tree::El { children, .. } => children
            .iter()
            .find(|c| el_name(c).is_some_and(|n| n.rsplit('}').next() == Some(name))),
        Tree::Text(_) => None,
    }
}

fn list_of(t: &Tree, name: &str) -> Vec<Tree> {
    match child_named(t, name) {
        Some(Tree::El { children, .. }) => children.clone(),
        _ => Vec::new(),
    }
}

/// `styles.xml`'s tables, for resolving an `s` to its content.
struct Styles {
    numfmts: HashMap<u32, String>,
    fonts: Vec<Tree>,
    fills: Vec<Tree>,
    borders: Vec<Tree>,
    style_xfs: Vec<Tree>,
    xfs: Vec<Tree>,
    /// `(xfId, name)` of each `cellStyle`.
    names: Vec<(u32, String)>,
}

fn attr_of<'a>(t: &'a Tree, a: &str) -> Option<&'a str> {
    match t {
        Tree::El { attrs, .. } => attrs.get(a).map(String::as_str),
        Tree::Text(_) => None,
    }
}

impl Styles {
    fn read(root: &RawNode) -> Styles {
        let t = tree(root);
        let numfmts = list_of(&t, "numFmts")
            .iter()
            .filter_map(|n| {
                Some((
                    attr_of(n, "numFmtId")?.parse().ok()?,
                    attr_of(n, "formatCode")?.to_owned(),
                ))
            })
            .collect();
        let names = list_of(&t, "cellStyles")
            .iter()
            .filter_map(|n| {
                Some((
                    attr_of(n, "xfId")?.parse().ok()?,
                    attr_of(n, "name")?.to_owned(),
                ))
            })
            .collect();
        Styles {
            numfmts,
            fonts: list_of(&t, "fonts"),
            fills: list_of(&t, "fills"),
            borders: list_of(&t, "borders"),
            style_xfs: list_of(&t, "cellStyleXfs"),
            xfs: list_of(&t, "cellXfs"),
            names,
        }
    }

    /// An `xf` with its table references replaced by the entries they name.
    fn resolve_xf(&self, xf: &Tree, with_style: bool) -> Tree {
        let Tree::El {
            name,
            mut attrs,
            mut children,
        } = xf.clone()
        else {
            panic!("an xf is an element")
        };
        let id = |attrs: &mut BTreeMap<String, String>, a: &str| {
            attrs
                .remove(a)
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
        };
        let num = id(&mut attrs, "numFmtId") as u32;
        attrs.insert(
            "numFmt".into(),
            self.numfmts
                .get(&num)
                .cloned()
                .unwrap_or_else(|| format!("builtin:{num}")),
        );
        for (a, list, n) in [
            ("fontId", &self.fonts, "font"),
            ("fillId", &self.fills, "fill"),
            ("borderId", &self.borders, "border"),
        ] {
            let i = id(&mut attrs, a);
            children.push(Tree::El {
                name: format!("resolved-{n}"),
                attrs: BTreeMap::new(),
                children: list.get(i).cloned().into_iter().collect(),
            });
        }
        if with_style {
            let i = id(&mut attrs, "xfId");
            let mut link = vec![];
            if let Some(sx) = self.style_xfs.get(i) {
                link.push(self.resolve_xf(sx, false));
            }
            if let Some((_, n)) = self.names.iter().find(|(x, _)| *x as usize == i) {
                link.push(Tree::Text(n.clone()));
            }
            children.push(Tree::El {
                name: "resolved-xfId".into(),
                attrs: BTreeMap::new(),
                children: link,
            });
        }
        Tree::El {
            name,
            attrs,
            children,
        }
    }

    /// The content of cell format `s`; panics when it is out of range.
    fn resolve(&self, s: u32) -> Tree {
        let xf = self.xfs.get(s as usize).unwrap_or_else(|| {
            panic!(
                "style index {s} is out of range ({} cellXfs)",
                self.xfs.len()
            )
        });
        self.resolve_xf(xf, true)
    }
}

const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

/// How P reads an `s`: through the style table, or raw when the table cannot
/// be modeled.
enum StyleTable {
    Table(Styles),
    /// The table has an `mc:AlternateContent` child: cells keep the raw `s`.
    Opaque,
    /// No styles part: only `s = 0` is valid.
    Missing,
}

impl StyleTable {
    fn read(root: Option<&RawNode>) -> StyleTable {
        let Some(root) = root else {
            return StyleTable::Missing;
        };
        if root
            .elements()
            .any(|e| e.name().local() == "AlternateContent")
        {
            return StyleTable::Opaque;
        }
        StyleTable::Table(Styles::read(root))
    }

    /// The content of the `s` attribute text `s`.
    fn resolve(&self, s: &str) -> Tree {
        match self {
            StyleTable::Table(t) => t.resolve(s.trim().parse().expect("s")),
            StyleTable::Opaque => Tree::Text(format!("raw s {}", s.trim())),
            StyleTable::Missing => {
                assert_eq!(s.trim(), "0", "style {s} but no styles part");
                Tree::Text("<no styles>".into())
            }
        }
    }
}

/// `xl/metadata.xml`'s tables, for resolving `cm` and `vm` to their content.
struct Meta {
    types: Vec<Tree>,
    future: HashMap<String, Vec<Tree>>,
    cell: Vec<Tree>,
    value: Vec<Tree>,
}

impl Meta {
    fn read(root: &RawNode) -> Meta {
        let t = tree(root);
        let mut by_name: HashMap<String, Vec<Tree>> = HashMap::new();
        if let Tree::El { children, .. } = &t {
            for c in children {
                if el_name(c).is_some_and(|n| n.rsplit('}').next() == Some("futureMetadata")) {
                    let name = attr_of(c, "name").unwrap_or_default().to_owned();
                    let bks = match c {
                        Tree::El { children, .. } => children.clone(),
                        Tree::Text(_) => Vec::new(),
                    };
                    by_name.entry(name).or_default().extend(bks);
                }
            }
        }
        Meta {
            types: list_of(&t, "metadataTypes"),
            future: by_name,
            cell: list_of(&t, "cellMetadata"),
            value: list_of(&t, "valueMetadata"),
        }
    }

    /// Block `k` (1-based) of `list` with its `rc` references replaced by what
    /// they name. Panics when a reference is out of range.
    fn block(&self, list: &[Tree], k: &str, what: &str) -> Tree {
        let i: usize = k.trim().parse().expect("a number");
        let bk = i
            .checked_sub(1)
            .and_then(|i| list.get(i))
            .unwrap_or_else(|| panic!("{what}={k} is out of range ({} blocks)", list.len()));
        let Tree::El {
            name,
            attrs,
            children,
        } = bk.clone()
        else {
            panic!("a block is an element")
        };
        let children = children
            .into_iter()
            .map(|rc| match rc {
                Tree::El {
                    name,
                    mut attrs,
                    mut children,
                } if name.rsplit('}').next() == Some("rc") => {
                    let t: usize = attrs
                        .remove("t")
                        .and_then(|t| t.parse().ok())
                        .expect("rc/@t");
                    let ty = t
                        .checked_sub(1)
                        .and_then(|t| self.types.get(t))
                        .unwrap_or_else(|| panic!("rc t={t} is out of range"));
                    children.push(Tree::El {
                        name: "resolved-type".into(),
                        attrs: BTreeMap::new(),
                        children: vec![ty.clone()],
                    });
                    if let Some(bks) = attr_of(ty, "name").and_then(|n| self.future.get(n)) {
                        let v: usize = attrs
                            .remove("v")
                            .and_then(|v| v.parse().ok())
                            .expect("rc/@v");
                        children.push(Tree::El {
                            name: "resolved-v".into(),
                            attrs: BTreeMap::new(),
                            children: vec![
                                bks.get(v)
                                    .unwrap_or_else(|| panic!("rc v={v} is out of range"))
                                    .clone(),
                            ],
                        });
                    }
                    Tree::El {
                        name,
                        attrs,
                        children,
                    }
                }
                other => other,
            })
            .collect();
        Tree::El {
            name,
            attrs,
            children,
        }
    }
}

/// The text and runs of an `si` or `is`.
type Runs = Vec<(Option<Tree>, String)>;

fn string_of(n: &RawNode) -> (String, Option<Runs>) {
    let t_of = |e: &RawNode| {
        e.elements()
            .filter(|c| c.name().local() == "t")
            .map(|c| c.text().into_owned())
            .collect::<String>()
    };
    if n.elements().any(|c| c.name().local() == "r") {
        let runs: Vec<(Option<Tree>, String)> = n
            .elements()
            .filter(|c| c.name().local() == "r")
            .map(|r| (local(r, "rPr").map(tree), t_of(r)))
            .collect();
        let text = runs.iter().map(|(_, t)| t.as_str()).collect();
        (text, Some(runs))
    } else {
        (t_of(n), None)
    }
}

fn str_val(n: &RawNode) -> Val {
    let (text, runs) = string_of(n);
    Val::Str { text, runs }
}

/// The part a workbook relationship of type `ty` names, if the package has it.
fn related(pkg: &Package, rels: &Relationships, ty: &str) -> Option<String> {
    rels.by_type(ty)
        .next()
        .and_then(|r| rels.resolve(r))
        .filter(|p| pkg.contains(p))
}

fn rels_part_of(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

fn read(bytes: &[u8]) -> (Model, Vec<BaseCell>) {
    let mut pkg = Package::from_bytes(bytes.to_vec()).expect("package");
    let wb = Workbook::from_bytes(bytes.to_vec()).expect("workbook");
    let is_name = |t: &str| wb.env.is_name(t);
    let sheet_parts: Vec<(usize, String)> = wb
        .sheets
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == SheetKind::Worksheet)
        .filter_map(|(i, s)| s.part.clone().map(|p| (i, p)))
        .collect();
    let wb_rels = pkg
        .relationships(wb.workbook_part())
        .expect("workbook relationships");
    let styles_part = related(&pkg, &wb_rels, REL_STYLES);
    let sst_part = related(&pkg, &wb_rels, REL_SHARED_STRINGS);
    let calc_chain = related(&pkg, &wb_rels, REL_CALC_CHAIN);
    let metadata = related(&pkg, &wb_rels, REL_METADATA);
    let styles = StyleTable::read(
        styles_part
            .as_ref()
            .map(|p| pkg.xml_part(p).expect("styles").root().clone())
            .as_ref(),
    );
    let sst: Vec<RawNode> = match &sst_part {
        Some(p) => pkg
            .xml_part(p)
            .expect("sst")
            .root()
            .elements()
            .filter(|e| e.name().local() == "si")
            .cloned()
            .collect(),
        None => Vec::new(),
    };
    let meta = metadata
        .as_ref()
        .map(|p| Meta::read(pkg.xml_part(p).expect("metadata").root()));
    // the parts a Commit may add, rewrite or drop by itself (oracle 1's
    // exclusions), found by relationship and not by name
    let mut excluded: Vec<String> = Vec::new();
    for p in [&calc_chain, &metadata].into_iter().flatten() {
        excluded.push(p.clone());
        excluded.push(rels_part_of(p));
    }
    let excluded = |name: &str| excluded.iter().any(|e| e == name);
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    let mut m: BTreeMap<String, String> = BTreeMap::new();
    let mut base = Vec::new();
    m.insert(
        "parts".into(),
        format!(
            "{:?}",
            names.iter().filter(|n| !excluded(n)).collect::<Vec<_>>()
        ),
    );
    for name in &names {
        if excluded(name) || Some(name) == sst_part.as_ref() {
            continue;
        }
        if let Some((sheet, _)) = sheet_parts.iter().find(|(_, p)| p == name) {
            let root = pkg.xml_part(name).expect("worksheet").root().clone();
            sheet_items(
                *sheet,
                name,
                &root,
                &Resolve {
                    styles: &styles,
                    sst: &sst,
                    meta: meta.as_ref(),
                    is_name: &is_name,
                },
                &mut m,
                &mut base,
            );
            continue;
        }
        let key = name.clone();
        let item = if name == "[Content_Types].xml" || name.ends_with(".rels") {
            let mut t = tree(pkg.xml_part(name).expect("part").root());
            if let Tree::El { children, .. } = &mut t {
                children.retain(|c| {
                    let part = attr_of(c, "PartName").map(|p| p.trim_start_matches('/'));
                    let ty = attr_of(c, "Type").unwrap_or_default();
                    !part.is_some_and(excluded) && !(ty == REL_CALC_CHAIN || ty == REL_METADATA)
                });
            }
            format!("{t:?}")
        } else if name == "xl/workbook.xml" {
            let mut t = tree(pkg.xml_part(name).expect("part").root());
            if let Tree::El { children, .. } = &mut t {
                for c in children {
                    if el_name(c).is_some_and(|n| n.ends_with("calcPr"))
                        && let Tree::El { attrs, .. } = c
                    {
                        attrs.remove("fullCalcOnLoad");
                    }
                }
            }
            format!("{t:?}")
        } else if Some(name) == styles_part.as_ref() {
            let mut t = tree(pkg.xml_part(name).expect("part").root());
            if matches!(styles, StyleTable::Table(_)) {
                drop_children(&mut t, &["numFmts", "fonts", "fills", "borders", "cellXfs"]);
            }
            format!("{t:?}")
        } else if name == "docProps/app.xml" {
            format!("{:?}", tree(pkg.xml_part(name).expect("part").root()))
        } else {
            let b = pkg.read_part(name).expect("part");
            let mut h = std::hash::DefaultHasher::new();
            std::hash::Hash::hash(&b, &mut h);
            format!(
                "{} bytes, hash {:x}",
                b.len(),
                std::hash::Hasher::finish(&h)
            )
        };
        m.insert(key, item);
    }
    (Model(m), base)
}

/// What resolving a worksheet's references needs.
struct Resolve<'a> {
    styles: &'a StyleTable,
    sst: &'a [RawNode],
    meta: Option<&'a Meta>,
    is_name: &'a dyn Fn(&str) -> bool,
}

/// Columns as canonical maximal runs (B4): every `col` of every `cols`
/// expanded to one attribute map per column (`min`, `max` out), equal
/// neighbours merged, empty maps dropped, so that equal column properties are
/// equal however the ranges were split.
fn cols_model(root: &RawNode, styles: &StyleTable) -> Option<String> {
    let mut per: BTreeMap<u32, BTreeMap<String, String>> = BTreeMap::new();
    let mut odd: Vec<Tree> = Vec::new();
    for cols in root.elements().filter(|e| e.name().local() == "cols") {
        for col in cols.elements().filter(|e| e.name().local() == "col") {
            let num = |a: &'static str| attr(col, a).and_then(|v| v.trim().parse::<u32>().ok());
            let (Some(min), Some(max)) = (num("min"), num("max")) else {
                odd.push(tree(col));
                continue;
            };
            let map: BTreeMap<String, String> = col
                .attrs()
                .iter()
                .filter(|a| !["min", "max"].contains(&a.name().local()))
                .map(|a| {
                    let v = if a.name().local() == "style" {
                        format!("{:?}", styles.resolve(a.value()))
                    } else {
                        a.value().to_owned()
                    };
                    (a.name().local().to_owned(), v)
                })
                .collect();
            for c in min..=max.min(MAX_COLS) {
                per.insert(c, map.clone());
            }
        }
    }
    let mut runs: Vec<(u32, u32, BTreeMap<String, String>)> = Vec::new();
    for (c, map) in per {
        if map.is_empty() {
            continue;
        }
        match runs.last_mut() {
            Some((_, last, m)) if *last + 1 == c && *m == map => *last = c,
            _ => runs.push((c, c, map)),
        }
    }
    (!runs.is_empty() || !odd.is_empty()).then(|| format!("{runs:?} {odd:?}"))
}

fn sheet_items(
    sheet: usize,
    part: &str,
    root: &RawNode,
    rs: &Resolve<'_>,
    m: &mut BTreeMap<String, String>,
    base: &mut Vec<BaseCell>,
) {
    let styles = rs.styles;
    let at_of = |c: &RawNode| {
        CellIndex::parse(attr(c, "r").expect("a `c` without `r`")).expect("a cell reference")
    };
    // a sheet whose rows or cells have no `r` cannot be cell-edited
    // (`MissingReference`): its Tree stands for it
    if let Some(sd) = local(root, "sheetData")
        && sd
            .elements()
            .any(|row| attr(row, "r").is_none() || row.elements().any(|c| attr(c, "r").is_none()))
    {
        let mut t = tree(root);
        drop_children(&mut t, &["dimension"]);
        m.insert(format!("{part}/tree"), format!("{t:?}"));
        return;
    }
    if let Some(sd) = local(root, "sheetData") {
        cells_and_rows(sheet, part, sd, rs, &at_of, m, base);
    }
    // the rest of the sheet: no sheetData, no dimension, no cols
    let mut rest = tree(root);
    drop_children(&mut rest, &["sheetData", "dimension", "cols"]);
    m.insert(format!("{part}/rest"), format!("{rest:?}"));
    if let Some(cols) = cols_model(root, styles) {
        m.insert(format!("{part}/cols"), cols);
    }
}

fn cells_and_rows(
    sheet: usize,
    part: &str,
    sd: &RawNode,
    rs: &Resolve<'_>,
    at_of: &dyn Fn(&RawNode) -> CellIndex,
    m: &mut BTreeMap<String, String>,
    base: &mut Vec<BaseCell>,
) {
    let (styles, sst, meta, is_name) = (rs.styles, rs.sst, rs.meta, rs.is_name);
    // shared masters and array ranges
    let mut masters: HashMap<String, (CellIndex, String)> = HashMap::new();
    let mut ranges: Vec<(CellIndex, CellIndex)> = Vec::new();
    for row in sd.elements() {
        for c in row.elements() {
            let Some(f) = local(c, "f") else { continue };
            match (attr(f, "t"), attr(f, "ref")) {
                (Some("shared"), Some(_)) => {
                    if let Some(si) = attr(f, "si") {
                        masters.insert(si.to_owned(), (at_of(c), f.text().into_owned()));
                    }
                }
                (Some("array" | "dataTable"), Some(r)) => {
                    let (a, b) = r.split_once(':').unwrap_or((r, r));
                    ranges.push((
                        CellIndex::parse(a).expect("ref"),
                        CellIndex::parse(b).expect("ref"),
                    ));
                }
                _ => {}
            }
        }
    }
    for row in sd.elements() {
        let r: u32 = attr(row, "r")
            .expect("a row without `r`")
            .parse()
            .expect("row number");
        let attrs: BTreeMap<String, String> = row
            .attrs()
            .iter()
            .filter(|a| !["r", "spans"].contains(&a.name().local()))
            .map(|a| {
                let v = if a.name().local() == "s" {
                    format!("{:?}", styles.resolve(a.value()))
                } else {
                    a.value().to_owned()
                };
                (a.name().local().to_owned(), v)
            })
            .collect();
        if !attrs.is_empty() {
            m.insert(format!("{part}/row/{r}"), format!("{attrs:?}"));
        }
        for c in row.elements() {
            let at = at_of(c);
            let f = local(c, "f");
            let formula = f.map(|f| {
                let text = f.text().into_owned();
                let own = match (attr(f, "t"), attr(f, "si")) {
                    (Some("shared"), Some(si)) if text.is_empty() => match masters.get(si) {
                        Some((mat, mtext)) => translate(
                            mtext,
                            i64::from(at.row) - i64::from(mat.row),
                            i64::from(at.col) - i64::from(mat.col),
                            is_name,
                        )
                        .unwrap_or_else(|e| format!("<untranslatable: {e}>")),
                        None => text,
                    },
                    _ => text,
                };
                let st = FormulaState::of(c).expect("a formula");
                let kind = match st.t.as_str() {
                    "normal" | "shared" => FormulaKind::Plain,
                    "array" => FormulaKind::Array(st.reference.clone().unwrap_or_default()),
                    "dataTable" => FormulaKind::DataTable {
                        reference: st.reference.clone().unwrap_or_default(),
                        dt2d: st.dt2d,
                        dtr: st.dtr,
                        del1: st.del1,
                        del2: st.del2,
                        r1: st.r1.clone(),
                        r2: st.r2.clone(),
                    },
                    other => FormulaKind::Other(other.to_owned()),
                };
                let attrs = FormulaAttrs {
                    ca: st.ca,
                    aca: st.aca,
                    bx: st.bx,
                    other: st.other,
                };
                (own, kind, attrs)
            });
            let in_range = ranges.iter().any(|(a, b)| {
                (a.row..=b.row).contains(&at.row) && (a.col..=b.col).contains(&at.col)
            });
            // a storage type P does not model (`t="d"`, an unknown `t`) or a
            // number that does not parse: the raw `t` and `v` (the Base's
            // `value` is `Empty` then, so that a revert removes the cell's content)
            let mut raw_value: Option<(Option<String>, String)> = None;
            let value = if formula.is_some() || in_range {
                Val::Cache
            } else {
                let v = local(c, "v").map(|v| v.text().into_owned());
                let mut raw = |v: String| {
                    raw_value = Some((attr(c, "t").map(str::to_owned), v));
                    Val::Empty
                };
                match (attr(c, "t"), v) {
                    (Some("inlineStr"), _) => local(c, "is").map_or(Val::Empty, str_val),
                    (_, None) => Val::Empty,
                    (Some("s"), Some(v)) => match v.trim().parse::<usize>() {
                        Ok(i) => sst
                            .get(i)
                            .map_or_else(|| panic!("sst index {v} out of range"), str_val),
                        Err(_) => raw(v),
                    },
                    (Some("str"), Some(v)) => Val::Str {
                        text: v,
                        runs: None,
                    },
                    (Some("b"), Some(v)) => Val::Bool(matches!(v.trim(), "1" | "true")),
                    (Some("e"), Some(v)) => Val::Error(v),
                    (None | Some("n"), Some(v)) => match v.trim().parse::<f64>() {
                        Ok(n) => Val::Number(n.to_bits()),
                        Err(_) => raw(v),
                    },
                    (_, Some(v)) => raw(v),
                }
            };
            let h1_attrs: BTreeMap<String, String> = c
                .attrs()
                .iter()
                .filter(|a| !["r", "s", "t", "cm", "vm"].contains(&a.name().local()))
                .map(|a| (a.name().local().to_owned(), a.value().to_owned()))
                .collect();
            let h1_children: Vec<Tree> = c
                .elements()
                .filter(|e| !["f", "v", "is"].contains(&e.name().local()))
                .map(tree)
                .collect();
            let block = |a: &'static str, pick: fn(&Meta) -> &[Tree]| -> Option<Tree> {
                attr(c, a).map(|k| {
                    let meta = meta.unwrap_or_else(|| panic!("{a}={k} but no metadata part"));
                    meta.block(pick(meta), k, a)
                })
            };
            let cm = block("cm", |m| &m.cell);
            let vm = block("vm", |m| &m.value);
            let model = (
                &value,
                &raw_value,
                &formula,
                styles.resolve(attr(c, "s").unwrap_or("0")),
                h1_attrs,
                h1_children,
                cm,
                vm,
            );
            m.insert(
                format!("{part}/cell/{},{}", at.row, at.col),
                format!("{model:?}"),
            );
            let group = in_range || f.is_some_and(|f| attr(f, "t") == Some("shared"));
            base.push(BaseCell {
                sheet,
                at,
                group,
                value,
                formula: formula.map(|(t, _, _)| t),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::tests::fixtures::{
        CT_METADATA, Extra, REL_METADATA_TYPE, custom_styles, dapr_block, meta_workbook,
        meta_workbook_named, metadata_with, package, package_ext, plain, rich,
    };

    const SHEET_HEAD: &str = r#"<dimension ref="A1:C3"/><sheetData>"#;

    fn book(sheet_data: &str) -> Vec<u8> {
        package(
            &[("Sheet1", format!("{SHEET_HEAD}{sheet_data}</sheetData>"))],
            &[plain("x"), rich()],
            None,
            &custom_styles(false),
        )
    }

    fn eq(a: &[u8], b: &[u8]) {
        assert_eq!(first_diff(&project(a), &project(b)), None);
    }

    fn ne(a: &[u8], b: &[u8]) {
        assert!(first_diff(&project(a), &project(b)).is_some());
    }

    #[test]
    fn equal_content_under_two_xf_indices() {
        let a = package(
            &[(
                "Sheet1",
                format!(r#"{SHEET_HEAD}<row r="1"><c r="A1" s="1"><v>1</v></c></row></sheetData>"#),
            )],
            &[],
            None,
            &custom_styles(true),
        );
        let b = package(
            &[(
                "Sheet1",
                format!(r#"{SHEET_HEAD}<row r="1"><c r="A1" s="4"><v>1</v></c></row></sheetData>"#),
            )],
            &[],
            None,
            &custom_styles(true),
        );
        eq(&a, &b);
        let c = package(
            &[(
                "Sheet1",
                format!(r#"{SHEET_HEAD}<row r="1"><c r="A1" s="2"><v>1</v></c></row></sheetData>"#),
            )],
            &[],
            None,
            &custom_styles(true),
        );
        ne(&a, &c);
    }

    #[test]
    fn shared_and_inline_storage_of_one_text_are_equal() {
        eq(
            &book(r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#),
            &book(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row>"#),
        );
        eq(
            &book(r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#),
            &book(r#"<row r="1"><c r="A1" t="str"><v>x</v></c></row>"#),
        );
    }

    #[test]
    fn a_shared_member_equals_its_expanded_normal_formula() {
        let shared = book(
            r#"<row r="1"><c r="D1"><f t="shared" ref="D1:D2" si="0">A1+1</f><v>2</v></c></row><row r="2"><c r="D2"><f t="shared" si="0"/><v>3</v></c></row>"#,
        );
        let normal = book(
            r#"<row r="1"><c r="D1"><f>A1+1</f><v>2</v></c></row><row r="2"><c r="D2"><f>A2+1</f><v>3</v></c></row>"#,
        );
        eq(&shared, &normal);
        let other = book(
            r#"<row r="1"><c r="D1"><f>A1+1</f><v>2</v></c></row><row r="2"><c r="D2"><f>A3+1</f><v>3</v></c></row>"#,
        );
        ne(&shared, &other);
    }

    #[test]
    fn a_missing_cell_differs_from_an_empty_one_with_the_same_style() {
        ne(
            &book(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#),
            &book(r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"/></row>"#),
        );
        ne(
            &book(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#),
            &book(r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1" s="1"/></row>"#),
        );
    }

    #[test]
    fn a_rich_string_differs_from_a_plain_one_with_the_same_text() {
        // si #1 is "ab" (bold) + "c"; si #2 is "abc" plain
        let a = package(
            &[(
                "Sheet1",
                format!(r#"{SHEET_HEAD}<row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData>"#),
            )],
            &[rich()],
            None,
            &custom_styles(false),
        );
        let b = package(
            &[(
                "Sheet1",
                format!(r#"{SHEET_HEAD}<row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData>"#),
            )],
            &[plain("abc")],
            None,
            &custom_styles(false),
        );
        ne(&a, &b);
    }

    #[test]
    fn an_unknown_attribute_on_c_is_visible() {
        ne(
            &book(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#),
            &book(r#"<row r="1"><c r="A1" zz="1"><v>1</v></c></row>"#),
        );
    }

    #[test]
    fn a_formula_cache_is_excluded_and_a_constant_is_not() {
        let f = |v: u32| {
            book(&format!(
                r#"<row r="1"><c r="A1"><f>1+1</f><v>{v}</v></c></row>"#
            ))
        };
        eq(&f(2), &f(3));
        let c = |v: u32| book(&format!(r#"<row r="1"><c r="A1"><v>{v}</v></c></row>"#));
        ne(&c(2), &c(3));
    }

    #[test]
    fn rows_with_only_r_and_spans_are_omitted() {
        eq(
            &book(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#),
            &book(r#"<row r="1" spans="1:1"><c r="A1"><v>1</v></c></row><row r="2"/>"#),
        );
        ne(
            &book(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#),
            &book(r#"<row r="1" ht="20" customHeight="1"><c r="A1"><v>1</v></c></row>"#),
        );
    }

    fn sheet_with(cols: &str, cell: &str) -> String {
        format!(
            r#"<dimension ref="A1:C3"/><cols>{cols}</cols><sheetData><row r="1">{cell}</row></sheetData>"#
        )
    }

    fn cols_book(cols: &str) -> Vec<u8> {
        package(
            &[("Sheet1", sheet_with(cols, r#"<c r="A1"><v>1</v></c>"#))],
            &[],
            None,
            &custom_styles(false),
        )
    }

    /// A book with a metadata part of the given blocks and one cell.
    fn md_book(future: &str, cell_md: &str, value_md: &str, cell: &str) -> Vec<u8> {
        package_ext(
            &[("Sheet1", sheet_with("", cell))],
            &[],
            None,
            &custom_styles(false),
            &[Extra {
                name: "xl/metadata.xml",
                body: metadata_with(future, cell_md, value_md),
                content_type: CT_METADATA,
                rel_type: REL_METADATA_TYPE,
            }],
        )
    }

    const BK_CELL: &str = r#"<bk><rc t="1" v="0"/></bk>"#;
    const BK_VALUE: &str = r#"<bk><rc t="2" v="0"/></bk>"#;

    #[test]
    fn h_one_block_content_under_two_cm_indices_is_equal() {
        // blocks 1 and 2 name different future blocks with the same content
        let two = format!("{}{}", dapr_block(0), dapr_block(0));
        let cm2 = r#"<bk><rc t="1" v="0"/></bk><bk><rc t="1" v="1"/></bk>"#;
        let a = md_book(&two, cm2, BK_VALUE, r#"<c r="A1" cm="1"><v>1</v></c>"#);
        let b = md_book(&two, cm2, BK_VALUE, r#"<c r="A1" cm="2"><v>1</v></c>"#);
        eq(&a, &b);
    }

    #[test]
    fn i_different_block_content_is_different() {
        let two = format!("{}{}", dapr_block(0), dapr_block(1));
        let cm2 = r#"<bk><rc t="1" v="0"/></bk><bk><rc t="1" v="1"/></bk>"#;
        let a = md_book(&two, cm2, BK_VALUE, r#"<c r="A1" cm="1"><v>1</v></c>"#);
        let b = md_book(&two, cm2, BK_VALUE, r#"<c r="A1" cm="2"><v>1</v></c>"#);
        ne(&a, &b);
        // and a cell with `cm` differs from one without
        ne(
            &a,
            &md_book(&two, cm2, BK_VALUE, r#"<c r="A1"><v>1</v></c>"#),
        );
    }

    #[test]
    fn j_vm_resolves_like_cm() {
        // `XLRICHVALUE` has no future block: `v` stays raw
        let vm2 =
            r#"<bk><rc t="2" v="0"/></bk><bk><rc t="2" v="0"/></bk><bk><rc t="2" v="5"/></bk>"#;
        let at = |vm: u32| {
            md_book(
                &dapr_block(0),
                BK_CELL,
                vm2,
                &format!(r#"<c r="A1" vm="{vm}"><v>1</v></c>"#),
            )
        };
        eq(&at(1), &at(2));
        ne(&at(1), &at(3));
        ne(
            &at(1),
            &md_book(&dapr_block(0), BK_CELL, vm2, r#"<c r="A1"><v>1</v></c>"#),
        );
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn a_cm_out_of_range_panics() {
        project(&md_book(
            &dapr_block(0),
            BK_CELL,
            BK_VALUE,
            r#"<c r="A1" cm="2"><v>1</v></c>"#,
        ));
    }

    #[test]
    fn k_formula_attributes_are_values() {
        let f = |attrs: &str| {
            book(&format!(
                r#"<row r="1"><c r="A1"><f{attrs}>1+1</f><v>2</v></c></row>"#
            ))
        };
        ne(&f(r#" ca="1""#), &f(""));
        eq(&f(r#" ca="true""#), &f(r#" ca="1""#));
        eq(&f(r#" ca="0""#), &f(""));
        ne(&f(r#" aca="1""#), &f(""));
        ne(&f(r#" zz="1""#), &f(""));
    }

    #[test]
    fn l_columns_equal_however_the_ranges_were_split() {
        let attrs = r#"width="10" customWidth="1""#;
        let one = cols_book(&format!(r#"<col min="2" max="4" {attrs}/>"#));
        let two = cols_book(&format!(
            r#"<col min="2" max="2" {attrs}/><col min="3" max="4" {attrs}/>"#
        ));
        eq(&one, &two);
        // another `cols` element, and an empty `col`, are no difference either
        let three = cols_book(&format!(
            r#"<col min="2" max="3" {attrs}/><col min="4" max="4" {attrs}/><col min="6" max="6"/>"#
        ));
        eq(&one, &three);
    }

    #[test]
    fn m_one_columns_width_is_visible() {
        let a = cols_book(r#"<col min="2" max="4" width="10" customWidth="1"/>"#);
        let b = cols_book(
            r#"<col min="2" max="3" width="10" customWidth="1"/><col min="4" max="4" width="11" customWidth="1"/>"#,
        );
        ne(&a, &b);
        ne(&a, &cols_book(""));
        // a column style is resolved, not compared by index
        let styled = |s: u32| cols_book(&format!(r#"<col min="2" max="2" style="{s}"/>"#));
        ne(&styled(1), &styled(2));
    }

    #[test]
    fn n_a_metadata_part_is_excluded_by_relationship_not_by_name() {
        eq(&meta_workbook(), &meta_workbook_named("xl/metadata2.xml"));
        eq(
            &meta_workbook(),
            &meta_workbook_named("xl/richData/metadata.xml"),
        );
    }

    #[test]
    fn o_other_value_types_and_sheets_without_r_do_not_panic() {
        let m = project(&book(
            r#"<row r="1"><c r="A1" t="d"><v>2024-01-02T00:00:00Z</v></c><c r="B1" t="zz"><v>q</v></c><c r="C1"><v>1e</v></c></row>"#,
        ));
        assert!(first_diff(&m, &m).is_none());
        // raw values are values
        ne(
            &book(r#"<row r="1"><c r="A1" t="d"><v>2024-01-02T00:00:00Z</v></c></row>"#),
            &book(r#"<row r="1"><c r="A1" t="d"><v>2024-01-03T00:00:00Z</v></c></row>"#),
        );
        let no_r = |v: u32| {
            package(
                &[(
                    "Sheet1",
                    format!(r#"<sheetData><row><c><v>{v}</v></c></row></sheetData>"#),
                )],
                &[],
                None,
                &custom_styles(false),
            )
        };
        eq(&no_r(1), &no_r(1));
        ne(&no_r(1), &no_r(2));
    }

    #[test]
    fn a_worksheet_without_sheet_data_keeps_its_rest_and_columns() {
        let b = |w: u32| {
            package(
                &[(
                    "Sheet1",
                    format!(r#"<cols><col min="1" max="1" width="{w}"/></cols>"#),
                )],
                &[],
                None,
                &custom_styles(false),
            )
        };
        eq(&b(5), &b(5));
        ne(&b(5), &b(6));
    }
}
