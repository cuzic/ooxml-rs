//! The projection P of oracle 1 (ADR-0024 "Test oracles" 1), over serialized
//! bytes, with its exclusions only. The predicates that justify them are J1's.
//!
//! P is a map from a path to the debug form of the typed model of that item
//! (cells, rows, parts), so two models are equal when every item is, and
//! [`first_diff`] names the first item that is not.

use std::collections::{BTreeMap, HashMap};

use ooxml_core::{Package, QName, RawNode};

use super::state::{Tree, tree};
use crate::census::SheetKind;
use crate::formula::translate::translate;
use crate::reference::CellIndex;
use crate::workbook::Workbook;

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

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FormulaKind {
    Plain,
    Array(String),
    DataTable(String),
}

/// A cell of the Base as an op that would write it back (the generator's "revert").
#[derive(Clone, Debug)]
pub(crate) struct BaseCell {
    pub(crate) sheet: usize,
    pub(crate) at: CellIndex,
    pub(crate) value: Val,
    pub(crate) formula: Option<String>,
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
    let styles = Styles::read(pkg.xml_part("xl/styles.xml").expect("styles").root());
    let sst: Vec<RawNode> = if pkg.contains("xl/sharedStrings.xml") {
        pkg.xml_part("xl/sharedStrings.xml")
            .expect("sst")
            .root()
            .elements()
            .filter(|e| e.name().local() == "si")
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let excluded = |name: &str| name == "xl/calcChain.xml" || name == "xl/metadata.xml";
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
        if excluded(name) || name == "xl/sharedStrings.xml" {
            continue;
        }
        if let Some((sheet, _)) = sheet_parts.iter().find(|(_, p)| p == name) {
            let root = pkg.xml_part(name).expect("worksheet").root().clone();
            sheet_items(
                *sheet, name, &root, &styles, &sst, &is_name, &mut m, &mut base,
            );
            continue;
        }
        let key = name.clone();
        let item = if name == "[Content_Types].xml" || name.ends_with(".rels") {
            let mut t = tree(pkg.xml_part(name).expect("part").root());
            if let Tree::El { children, .. } = &mut t {
                children.retain(|c| {
                    !["calcChain.xml", "metadata.xml"].iter().any(|x| {
                        attr_of(c, "PartName")
                            .or(attr_of(c, "Target"))
                            .is_some_and(|v| v.ends_with(x))
                    })
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
        } else if name == "xl/styles.xml" {
            let mut t = tree(pkg.xml_part(name).expect("part").root());
            drop_children(&mut t, &["numFmts", "fonts", "fills", "borders", "cellXfs"]);
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

#[allow(clippy::too_many_arguments)]
fn sheet_items(
    sheet: usize,
    part: &str,
    root: &RawNode,
    styles: &Styles,
    sst: &[RawNode],
    is_name: &dyn Fn(&str) -> bool,
    m: &mut BTreeMap<String, String>,
    base: &mut Vec<BaseCell>,
) {
    let at_of = |c: &RawNode| {
        CellIndex::parse(attr(c, "r").expect("a `c` without `r`")).expect("a cell reference")
    };
    let Some(sd) = local(root, "sheetData") else {
        return;
    };
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
                    format!("{:?}", styles.resolve(a.value().trim().parse().expect("s")))
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
            let s: u32 = attr(c, "s").map_or(0, |s| s.trim().parse().expect("s"));
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
                // J0b: P keeps only `t`/`ref` of the `f` group and drops the cell's
                // `cm`/`vm` below; J0b resolves them and adds the rest of the `f`
                // attributes (`ca`, `aca`, ...) with a fixture that has them
                // (PR 159 review, m3).
                let kind = match (attr(f, "t"), attr(f, "ref")) {
                    (Some("array"), Some(r)) => FormulaKind::Array(r.to_owned()),
                    (Some("dataTable"), Some(r)) => FormulaKind::DataTable(r.to_owned()),
                    _ => FormulaKind::Plain,
                };
                (own, kind)
            });
            let in_range = ranges.iter().any(|(a, b)| {
                (a.row..=b.row).contains(&at.row) && (a.col..=b.col).contains(&at.col)
            });
            let value = if formula.is_some() || in_range {
                Val::Cache
            } else {
                let v = local(c, "v").map(|v| v.text().into_owned());
                match (attr(c, "t"), v) {
                    (Some("inlineStr"), _) => local(c, "is").map_or(Val::Empty, str_val),
                    (_, None) => Val::Empty,
                    (Some("s"), Some(v)) => sst
                        .get(v.trim().parse::<usize>().expect("sst index"))
                        .map_or_else(|| panic!("sst index {v} out of range"), str_val),
                    (Some("str"), Some(v)) => Val::Str {
                        text: v,
                        runs: None,
                    },
                    (Some("b"), Some(v)) => Val::Bool(matches!(v.trim(), "1" | "true")),
                    (Some("e"), Some(v)) => Val::Error(v),
                    (_, Some(v)) => Val::Number(
                        v.trim()
                            .parse::<f64>()
                            .unwrap_or_else(|_| panic!("number {v:?}"))
                            .to_bits(),
                    ),
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
            let model = (&value, &formula, styles.resolve(s), h1_attrs, h1_children);
            m.insert(
                format!("{part}/cell/{},{}", at.row, at.col),
                format!("{model:?}"),
            );
            base.push(BaseCell {
                sheet,
                at,
                value,
                formula: formula.map(|(t, _)| t),
            });
        }
    }
    // the rest of the sheet: no sheetData, no dimension; `col/@style` resolved
    let mut rest = tree(root);
    drop_children(&mut rest, &["sheetData", "dimension"]);
    if let Tree::El { children, .. } = &mut rest {
        for c in children {
            if el_name(c).is_some_and(|n| n.ends_with("cols"))
                && let Tree::El { children: cols, .. } = c
            {
                for col in cols {
                    if let Tree::El { attrs, .. } = col
                        && let Some(s) = attrs
                            .get("style")
                            .and_then(|s| s.trim().parse::<u32>().ok())
                    {
                        attrs.insert("style".into(), format!("{:?}", styles.resolve(s)));
                    }
                }
            }
        }
    }
    m.insert(format!("{part}/rest"), format!("{rest:?}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::tests::fixtures::{custom_styles, package, plain, rich};

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
}
