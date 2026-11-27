//! The style registry (ADR-0006 §5).
//!
//! - Tables `numFmts`, `fonts`, `fills`, `borders`, `cellXfs` only grow; a
//!   missing table is inserted in schema order; an empty table gets its
//!   reserved leading entries first (font 0, border 0, `xf` 0, fills 0/1).
//! - A new `Format` resolves bottom-up: number format, font, fill and border
//!   ids first, then the `xf` built from them. Each is reused when an entry
//!   with exactly the canonical bytes exists (including entries a previous
//!   session appended), else appended.
//! - Unset groups copy the default style's ids (`cellStyleXfs[0]`, else
//!   `cellXfs[0]`).
//! - A new custom `numFmtId` is above every `numFmtId` in any XML part of
//!   the package.
//! - A table with an `mc:AlternateContent` child never grows; indices in a
//!   table that is not index-stable are refused.
//! - Every plan works on a clone of the registry and a staged clone of
//!   `styles.xml`; apply swaps both in.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{ChildOf, ElementSpec, LensMut, QName, RawNode, XmlDocument};
use ooxml_schema::sml;

use super::format::{Alignment, BUILTIN, Border, Color, DxfFormat, Fill, Font, Format, NumFormat, Protection, Side, Underline, is_date_format};
use super::{Table, limit, render};
use crate::error::{Error, Result};
use crate::workbook::Workbook;

const REL_STYLES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

/// The tables a registry can grow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Which {
    NumFmts,
    Fonts,
    Fills,
    Borders,
    CellXfs,
    Dxfs,
}

impl Which {
    fn name(self) -> &'static str {
        match self {
            Which::NumFmts => "numFmts",
            Which::Fonts => "fonts",
            Which::Fills => "fills",
            Which::Borders => "borders",
            Which::CellXfs => "cellXfs",
            Which::Dxfs => "dxfs",
        }
    }
}

/// The unqualified attributes and children of an `xf`.
#[derive(Clone, Debug, Default)]
pub(crate) struct XfInfo {
    pub(crate) attrs: Vec<(String, String)>,
    /// A namespaced attribute or an unknown child: cannot be derived from.
    pub(crate) opaque: bool,
    pub(crate) alignment: Option<Vec<(String, String)>>,
    pub(crate) protection: Option<Vec<(String, String)>>,
}

impl XfInfo {
    fn read(n: &RawNode) -> XfInfo {
        let mut x = XfInfo::default();
        for a in n.attrs() {
            if a.name().ns().is_empty() && known(XF_ATTRS, a.name().local()).is_some() {
                x.attrs.push((a.name().local().to_owned(), a.value().to_owned()));
            } else if a.name().ns() != ooxml_core::name::XMLNS_NS {
                x.opaque = true;
            }
        }
        for c in n.elements() {
            let plain = |c: &RawNode| c.attrs().iter().map(|a| (a.name().local().to_owned(), a.value().to_owned())).collect::<Vec<_>>();
            let only = |c: &RawNode, set: &'static [&'static str]| {
                c.elements().next().is_none() && c.attrs().iter().all(|a| a.name().ns().is_empty() && known(set, a.name().local()).is_some())
            };
            if c.is(sml::Alignment::QNAME) && only(c, ALIGNMENT_ATTRS) {
                x.alignment = Some(plain(c));
            } else if c.is(sml::Protection::QNAME) && only(c, PROTECTION_ATTRS) {
                x.protection = Some(plain(c));
            } else {
                x.opaque = true;
            }
        }
        x
    }

    pub(crate) fn get(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    fn set(&mut self, k: &str, v: &str) {
        match self.attrs.iter_mut().find(|(n, _)| n == k) {
            Some(slot) => slot.1 = v.to_owned(),
            None => self.attrs.push((k.to_owned(), v.to_owned())),
        }
    }
}

/// The registry state.
#[derive(Clone, Debug)]
pub(crate) struct Styles {
    pub(crate) part: String,
    root_prefix: Option<String>,
    tables: HashMap<Which, Table>,
    prefixes: HashMap<Which, Option<String>>,
    exists: HashSet<Which>,
    count_attr: HashSet<Which>,
    csx: Table,
    next_numfmt: u32,
    // The large members are `Arc`s: `plan` clones the registry for every
    // write, and only a plan that appends copies them (`Arc::make_mut`;
    // PR #50 review M5).
    custom_codes: Arc<HashMap<String, u32>>,
    /// Custom codes given an id for a `dxf/numFmt` this session (they are
    /// self-contained and never added to `numFmts`).
    dxf_codes: Arc<HashMap<String, u32>>,
    redefined: Arc<HashSet<u32>>,
    pub(crate) codes: Arc<HashMap<u32, String>>,
    custom_count: u32,
    pub(crate) default: Option<XfInfo>,
    /// The Normal style cannot be read (`cellStyleXfs` is not index-stable,
    /// or its entry 0 is an `mc:AlternateContent`): a `Format` with an
    /// unset group is refused instead of inheriting id 0 (review m6).
    default_unreadable: bool,
    pub(crate) xfs: Arc<Vec<Option<XfInfo>>>,
    csx_infos: Arc<Vec<Option<XfInfo>>>,
    named: Arc<Vec<(String, u32)>>,
    /// Largest existing reference into each table (cell/row `s`, column
    /// `style`, `xf` ids).
    max_ref: HashMap<Which, u32>,
}

/// What a style plan produces.
#[derive(Debug)]
pub(crate) struct StylePlan {
    pub(crate) s: u32,
    pub(crate) staged: Option<XmlDocument>,
    pub(crate) next: Styles,
}

/// What a write asks of the style registry.
#[derive(Clone, Debug)]
pub(crate) enum StyleRequest {
    /// A value format (unset groups inherit from the default style).
    Format(Box<Format>),
    /// Replaces only the format groups supplied by the caller.
    Patch(std::sync::Arc<Format>),
    /// An existing `cellXfs` index.
    Index(u32),
    /// The row format with fields it omits supplied by the column.
    RowWithColumnDefaults { row: u32, column: u32 },
    /// A named style (`cellStyles/cellStyle/@name`).
    Named(String),
    /// The cell's current `xf` with another number format (ADR-0006 §5.6,
    /// "preserving edits").
    NumFormatOnly(NumFormat),
    /// The cell's current `xf` with another font (§5.6).
    FontOnly(Box<Font>),
    /// The cell's current `xf` with another fill (§5.6).
    FillOnly(Fill),
    /// The cell's current `xf` with other borders (§5.6).
    BorderOnly(Box<Border>),
    ProtectionOnly(Protection),
    AlignmentOnly(Alignment),
    /// The cell's current `xf` if its number format is a date or time
    /// format, else that `xf` with built-in 14 (ADR-0006 §6).
    EnsureDate,
}

fn table_node<'d>(root: &'d RawNode, local: &str) -> Option<&'d RawNode> {
    root.elements().find(|e| e.name().ns() == crate::SML_NS && e.name().local() == local)
}

/// The largest values of the id attributes that share a package-wide space,
/// in any XML part.
struct IdScan {
    /// Every `numFmtId`.
    numfmt: u32,
    /// Main-table `dxfId` and `…DxfId` (`cfRule`, table and table-style
    /// elements, pivot formats): references into `dxfs`.
    dxf: Option<u32>,
}

/// Scans every XML part once (review n6: lexical rules, content-type
/// selection, UTF-16 decoded; a part that cannot be decoded refuses).
fn scan_ids(wb: &Workbook, styles_part: &str) -> Result<IdScan> {
    let mut out = IdScan { numfmt: 0, dxf: None };
    let names: Vec<String> = wb.pkg.part_names().map(str::to_owned).collect();
    for name in names {
        let is_xml = wb.pkg.content_type(&name).is_some_and(|ct| {
            let e = ct.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
            e.ends_with("/xml") || e.ends_with("+xml")
        });
        if !is_xml {
            continue;
        }
        let bytes = wb.pkg.read_part(&name)?;
        let text = decode_xml_text(&bytes).ok_or_else(|| Error::Malformed(format!("{name}: cannot decode to scan numFmtId")))?;
        out.numfmt = out.numfmt.max(max_attr(&name, &text, "numFmtId", &[])?.unwrap_or(0));
        let slicer_tags = if name == styles_part && text.contains("slicerStyleElement") { slicer_style_tags(&text) } else { Vec::new() };
        for (needle, excluded) in [("dxfId", slicer_tags.as_slice()), ("DxfId", &[])] {
            if let Some(v) = max_attr(&name, &text, needle, excluded)? {
                out.dxf = out.dxf.max(Some(v));
            }
        }
    }
    Ok(out)
}

/// Opening tags whose unqualified dxfId indexes x14:dxfs (MS-XLSX
/// §2.6.45 and §2.4.24). Match the namespace and the extension path.
fn slicer_style_tags(text: &str) -> Vec<Range<usize>> {
    const X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
    let Ok(doc) = XmlDocument::parse(text.as_bytes()) else { return Vec::new() };
    if !doc.root().is(sml::Stylesheet::QNAME) || doc.source() != text {
        return Vec::new();
    }
    let mut nodes = vec![doc.root()];
    for (ns, local) in [
        (crate::SML_NS, "extLst"),
        (crate::SML_NS, "ext"),
        (X14, "slicerStyles"),
        (X14, "slicerStyle"),
        (X14, "slicerStyleElements"),
        (X14, "slicerStyleElement"),
    ] {
        let name = QName::new(ns, local);
        nodes = nodes.into_iter().flat_map(RawNode::elements).filter(|node| node.is(name)).collect();
    }
    nodes.into_iter().filter_map(RawNode::span).map(|span| span.start..span.open_end).collect()
}

/// The largest value of attributes whose name ends with `needle` in `text`.
/// Skip exact unqualified names in the excluded opening tags, which are
/// in document order. Outside those tags keep the conservative suffix scan.
fn max_attr(name: &str, text: &str, needle: &str, excluded: &[Range<usize>]) -> Result<Option<u32>> {
    let mut best: Option<u32> = None;
    {
        let b = text.as_bytes();
        let mut i = 0;
        while let Some(off) = text[i..].find(needle) {
            let attr = i + off;
            let mut j = attr + needle.len();
            i = j;
            if attr > 0 && b[attr - 1].is_ascii_whitespace()
                && excluded.get(excluded.partition_point(|tag| tag.end <= attr)).is_some_and(|tag| tag.contains(&attr))
            {
                continue;
            }
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if b.get(j) != Some(&b'=') {
                continue;
            }
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            let Some(&q) = b.get(j).filter(|q| **q == b'"' || **q == b'\'') else { continue };
            let start = j + 1;
            let end = text[start..].find(q as char).map(|e| start + e).unwrap_or(start);
            let raw = &text[start..end];
            // a value with references (`&#49;70`) is decoded, never skipped:
            // the scan must not under-estimate (ADR-0006 n6; PR #50 review m7)
            let value = if raw.contains('&') {
                std::borrow::Cow::Owned(decode_references(raw).ok_or_else(|| Error::Malformed(format!("{name}: cannot decode {needle} value {raw:?}")))?)
            } else {
                std::borrow::Cow::Borrowed(raw)
            };
            let v = value.trim_matches([' ', '\t', '\r', '\n']);
            if let Ok(v) = v.parse::<u64>() {
                best = best.max(Some(v.min(u64::from(u32::MAX)) as u32));
            }
        }
    }
    Ok(best)
}

/// Decodes the five predefined entities and numeric character references of
/// an attribute value; `None` for anything else (an unknown entity, a bad
/// reference).
fn decode_references(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let end = after.find(';')?;
        let name = &after[..end];
        let ch = match name {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let n = if let Some(h) = name.strip_prefix("#x") {
                    u32::from_str_radix(h, 16).ok()?
                } else {
                    name.strip_prefix('#')?.parse::<u32>().ok()?
                };
                char::from_u32(n)?
            }
        };
        out.push(ch);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

fn decode_xml_text(b: &[u8]) -> Option<String> {
    let utf16 = |le: bool, b: &[u8]| -> Option<String> {
        let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) }).collect();
        String::from_utf16(&units).ok()
    };
    match b {
        [0xFF, 0xFE, rest @ ..] => utf16(true, rest),
        [0xFE, 0xFF, rest @ ..] => utf16(false, rest),
        _ => std::str::from_utf8(b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b)).ok().map(str::to_owned),
    }
}

/// The workbook's styles part, if it has one (through the workbook
/// relationship, as Phase0 locates it).
pub(crate) fn styles_part(wb: &Workbook) -> Result<Option<String>> {
    let rels = wb.pkg.relationships(&wb.workbook_part)?;
    Ok(rels.by_type(REL_STYLES).next().and_then(|r| rels.resolve(r)).filter(|p| wb.pkg.contains(p)))
}

impl Styles {
    /// Activates the registry. [`Error::NoStylesPart`] without a styles
    /// part; [`Error::UnsupportedStylesPart`] when its root is not a
    /// SpreadsheetML `styleSheet`.
    pub(crate) fn activate(wb: &mut Workbook) -> Result<Styles> {
        let part = styles_part(wb)?.ok_or(Error::NoStylesPart)?;
        let ids = scan_ids(wb, &part)?;
        let max_id = ids.numfmt;
        let mut sheet_max_s: Option<u32> = None;
        for p in super::census_sheet_parts(wb) {
            let scan = super::scan_sheet(&wb.pkg.read_part(&p)?)?;
            sheet_max_s = sheet_max_s.max(scan.max_s);
        }
        let doc = super::current_document(wb.pkg.xml_part(&part)?)?;
        let root = doc.root();
        if !root.is(sml::Stylesheet::QNAME) {
            return Err(Error::UnsupportedStylesPart(format!("{part}: root is {{{}}}{}", root.name().ns(), root.name().local())));
        }
        if root.elements().any(|e| e.is(MC_ALTERNATE_CONTENT)) {
            // a table wrapped in AC is neither present nor absent (review n9)
            return Err(ooxml_core::Error::UnresolvedSibling(sml::Stylesheet::QNAME).into());
        }
        let mut st = Styles {
            part: part.clone(),
            root_prefix: root.name().prefix().map(str::to_owned),
            tables: HashMap::new(),
            prefixes: HashMap::new(),
            exists: HashSet::new(),
            count_attr: HashSet::new(),
            csx: Table::default(),
            next_numfmt: max_id.max(163) + 1,
            custom_codes: Arc::default(),
            dxf_codes: Arc::default(),
            redefined: Arc::default(),
            codes: Arc::default(),
            custom_count: 0,
            default: None,
            default_unreadable: false,
            xfs: Arc::default(),
            csx_infos: Arc::default(),
            named: Arc::default(),
            max_ref: HashMap::new(),
        };
        let entry = |w: Which| match w {
            Which::NumFmts => sml::NumberingFormat::QNAME,
            Which::Fonts => sml::Font::QNAME,
            Which::Fills => sml::Fill::QNAME,
            Which::Borders => sml::Border::QNAME,
            Which::CellXfs => sml::CellFormat::QNAME,
            Which::Dxfs => sml::DifferentialFormat::QNAME,
        };
        if let Some(d) = ids.dxf {
            st.max_ref.insert(Which::Dxfs, d);
        }
        for w in [Which::NumFmts, Which::Fonts, Which::Fills, Which::Borders, Which::CellXfs, Which::Dxfs] {
            let node = table_node(root, w.name());
            if let Some(n) = node {
                st.exists.insert(w);
                st.prefixes.insert(w, n.name().prefix().map(str::to_owned));
                if n.attr(QName::new("", "count")).is_some() {
                    st.count_attr.insert(w);
                }
            }
            st.tables.insert(w, Table::read(&doc, node, entry(w)));
        }
        // fills 0 and 1 are reserved; 0 is a reuse target only as canonical `none`
        let none_key = render::<sml::Fills>(st.prefix(Which::Fills).as_deref(), |p| build_fill(p, &Fill::NoFill))?;
        let gray_key = render::<sml::Fills>(st.prefix(Which::Fills).as_deref(), |p| build_fill(p, &gray125()))?;
        Arc::make_mut(&mut st.tables.get_mut(&Which::Fills).expect("read").reuse).retain(|k, i| *i >= 2 || (*i == 0 && *k == none_key) || (*i == 1 && *k == gray_key));
        // number formats
        if let Some(n) = table_node(root, "numFmts") {
            for e in n.elements().filter(|e| e.is(sml::NumberingFormat::QNAME)) {
                let (Some(id), Some(code)) = (e.attr(QName::new("", "numFmtId")).and_then(|v| v.trim().parse::<u32>().ok()), e.attr(QName::new("", "formatCode"))) else {
                    continue;
                };
                Arc::make_mut(&mut st.codes).insert(id, code.to_owned());
                if id < 164 {
                    Arc::make_mut(&mut st.redefined).insert(id);
                } else {
                    st.custom_count += 1;
                    if e.attrs().len() == 2 && e.elements().next().is_none() {
                        Arc::make_mut(&mut st.custom_codes).entry(code.to_owned()).or_insert(id);
                    }
                }
            }
        }
        if let Some(dxfs) = table_node(root, "dxfs") {
            for dxf in dxfs.elements().filter(|e| e.is(sml::DifferentialFormat::QNAME)) {
                let Some(n) = dxf.elements().find(|e| e.is(sml::NumberingFormat::QNAME)) else { continue };
                let (Some(id), Some(code)) = (n.attr(QName::new("", "numFmtId")).and_then(|v| v.trim().parse::<u32>().ok()), n.attr(QName::new("", "formatCode"))) else {
                    continue;
                };
                if (164..=65_535).contains(&id) && st.codes.get(&id).is_none_or(|existing| existing == code) {
                    Arc::make_mut(&mut st.dxf_codes).entry(code.to_owned()).and_modify(|existing| *existing = (*existing).min(id)).or_insert(id);
                }
            }
        }
        // xfs, cell style xfs, named styles
        let xfs = |local: &str| -> Vec<Option<XfInfo>> {
            table_node(root, local)
                .map(|t| t.elements().map(|e| e.is(sml::CellFormat::QNAME).then(|| XfInfo::read(e))).collect())
                .unwrap_or_default()
        };
        st.xfs = Arc::new(xfs("cellXfs"));
        st.csx_infos = Arc::new(xfs("cellStyleXfs"));
        st.csx = Table::read(&doc, table_node(root, "cellStyleXfs"), sml::CellFormat::QNAME);
        if st.csx.count > 0 {
            st.default = if st.csx.stable { st.csx_infos.first().cloned().flatten() } else { None };
            st.default_unreadable = st.default.is_none();
        } else {
            st.default = st.xfs.first().cloned().flatten();
        }
        for x in st.xfs.iter().chain(st.csx_infos.iter()).flatten() {
            for (k, w) in [("fontId", Which::Fonts), ("fillId", Which::Fills), ("borderId", Which::Borders)] {
                if let Some(v) = x.get(k).and_then(|v| v.trim().parse::<u32>().ok()) {
                    let e = st.max_ref.entry(w).or_insert(0);
                    *e = (*e).max(v);
                }
            }
        }
        if let Some(m) = sheet_max_s {
            st.max_ref.insert(Which::CellXfs, m);
        }
        if let Some(cs) = table_node(root, "cellStyles") {
            for e in cs.elements().filter(|e| e.is(sml::CellStyle::QNAME)) {
                if let (Some(n), Some(x)) = (e.attr(QName::new("", "name")), e.attr(QName::new("", "xfId")).and_then(|v| v.trim().parse().ok())) {
                    Arc::make_mut(&mut st.named).push((n.to_owned(), x));
                }
            }
        }
        Ok(st)
    }

    fn prefix(&self, w: Which) -> Option<String> {
        if self.exists.contains(&w) { self.prefixes.get(&w).cloned().flatten() } else { self.root_prefix.clone() }
    }

    fn table(&self, w: Which) -> &Table {
        self.tables.get(&w).expect("all tables read")
    }

    /// Entries in `cellXfs`.
    pub(crate) fn xf_count(&self) -> u32 {
        self.table(Which::CellXfs).count
    }

    /// The number format code of a `cellXfs` index (built-in table, or
    /// the file's `numFmts`), and whether it is a date.
    pub(crate) fn num_format_of(&self, s: u32) -> (Option<u32>, Option<String>) {
        let id = self.xfs.get(s as usize).cloned().flatten().and_then(|x| x.get("numFmtId").and_then(|v| v.parse::<u32>().ok()));
        let code = id.and_then(|i| self.codes.get(&i).cloned().or_else(|| BUILTIN.iter().find(|(b, _)| u32::from(*b) == i).map(|(_, c)| (*c).to_owned())));
        (id, code)
    }

    /// Plans a style request for a cell whose current `s` is `current`.
    pub(crate) fn plan(&self, wb: &mut Workbook, req: &StyleRequest, current: u32, staged: Option<&XmlDocument>) -> Result<StylePlan> {
        let mut p = Planner { next: self.clone(), staged: staged.cloned() };
        let s = match req {
            StyleRequest::Index(n) => {
                p.next.table(Which::CellXfs).require_stable()?;
                if *n >= p.next.table(Which::CellXfs).count {
                    return Err(Error::StyleIndexOutOfRange { index: *n, count: p.next.table(Which::CellXfs).count });
                }
                *n
            }
            StyleRequest::RowWithColumnDefaults { row, column } => {
                let mut spec = p.current(wb, *row)?;
                let fallback = p.current(wb, *column)?;
                if spec.opaque || fallback.opaque {
                    return Err(Error::NotExpressible("Cannot combine opaque row and column formats".into()));
                }
                let (row_num_fmt, row_code) = p.next.num_format_of(*row);
                for (key, apply, use_column) in [
                    ("numFmtId", "applyNumberFormat", row_num_fmt.is_none_or(|id| id == 0) || row_code.is_none()),
                    ("fontId", "applyFont", spec.get("fontId").is_none_or(|id| id == "0") && spec.get("applyFont") != Some("1")),
                    ("fillId", "applyFill", spec.get("fillId").is_none_or(|id| id == "0") && spec.get("applyFill") != Some("1")),
                    ("borderId", "applyBorder", spec.get("borderId").is_none_or(|id| id == "0") && spec.get("applyBorder") != Some("1")),
                ] {
                    if use_column && let Some(value) = fallback.get(key) {
                        spec.set(key, value);
                        spec.set(apply, fallback.get(apply).unwrap_or("1"));
                    }
                }
                if spec.alignment.is_none() && fallback.alignment.is_some() {
                    spec.alignment = fallback.alignment;
                    spec.set("applyAlignment", "1");
                }
                if spec.protection.is_none() && fallback.protection.is_some() {
                    spec.protection = fallback.protection;
                    spec.set("applyProtection", "1");
                }
                p.xf(wb, spec)?
            }
            StyleRequest::Format(f) => p.format(wb, f)?,
            StyleRequest::Patch(f) => p.patch(wb, f, current)?,
            StyleRequest::Named(name) => p.named(wb, name, current)?,
            StyleRequest::NumFormatOnly(nf) => p.derive(wb, &Group::NumFormat(nf), current)?,
            StyleRequest::FontOnly(f) => p.derive(wb, &Group::Font(f), current)?,
            StyleRequest::FillOnly(f) => p.derive(wb, &Group::Fill(f), current)?,
            StyleRequest::BorderOnly(b) => p.derive(wb, &Group::Border(b), current)?,
            StyleRequest::AlignmentOnly(alignment) => {
                Format { alignment: Some(alignment.clone()), ..Format::default() }.validate()?;
                let mut spec = p.current(wb, current)?;
                if spec.opaque {
                    return Err(Error::NotExpressible(format!("cellXfs[{current}] has content the registry does not model")));
                }
                let attrs = alignment.attrs();
                spec.alignment = (!attrs.is_empty()).then(|| attrs.into_iter().map(|(key, value)| (key.into(), value)).collect());
                spec.set("applyAlignment", if spec.alignment.is_some() { "1" } else { "0" });
                p.xf(wb, spec)?
            }
            StyleRequest::ProtectionOnly(protection) => {
                let mut spec = p.current(wb, current)?;
                if spec.opaque {
                    return Err(Error::NotExpressible(format!("cellXfs[{current}] has content the registry does not model")));
                }
                spec.protection = Some(vec![
                    ("locked".into(), u8::from(protection.locked).to_string()),
                    ("hidden".into(), u8::from(protection.hidden).to_string()),
                ]);
                spec.set("applyProtection", "1");
                p.xf(wb, spec)?
            }
            StyleRequest::EnsureDate => {
                let (id, code) = p.next.num_format_of(current);
                let custom = id.filter(|i| p.next.codes.contains_key(i)).and(code);
                if is_date_format(id, custom.as_deref()) && p.next.table(Which::CellXfs).stable {
                    current
                } else {
                    p.derive(wb, &Group::NumFormat(&NumFormat::DefaultDate), current)?
                }
            }
        };
        // Only a table this plan grows is checked: a file already past a
        // limit can still apply, reuse and derive from existing entries.
        let n = &p.next;
        let check = |table: &'static str, before: u32, after: u32| -> Result<()> {
            if after > before && after > limit(table) { Err(Error::StyleTableFull { table: table.into(), limit: limit(table) }) } else { Ok(()) }
        };
        check("cellXfs+cellStyleXfs", self.table(Which::CellXfs).count + self.csx.count, n.table(Which::CellXfs).count + n.csx.count)?;
        check("fonts", self.table(Which::Fonts).count, n.table(Which::Fonts).count)?;
        check("fills", self.table(Which::Fills).count, n.table(Which::Fills).count)?;
        check("borders", self.table(Which::Borders).count, n.table(Which::Borders).count)?;
        check("custom numFmts", self.custom_count, n.custom_count)?;
        Ok(StylePlan { s, staged: p.staged, next: p.next })
    }
}

/// The group a preserving edit replaces.
enum Group<'a> {
    NumFormat(&'a NumFormat),
    Font(&'a Font),
    Fill(&'a Fill),
    Border(&'a Border),
}

impl Styles {
    /// Plans one differential format (ADR-0006 §5.8): reused when an
    /// existing `dxf` has exactly its canonical bytes, else appended. The
    /// result's `s` is the `dxf` index.
    pub(crate) fn plan_dxf(&self, wb: &mut Workbook, d: &DxfFormat) -> Result<StylePlan> {
        let mut p = Planner { next: self.clone(), staged: None };
        let s = p.dxf(wb, d)?;
        Ok(StylePlan { s, staged: p.staged, next: p.next })
    }
}

struct Planner {
    next: Styles,
    staged: Option<XmlDocument>,
}

/// Appends under table `T` of the staged `styleSheet`.
fn append_to<T>(doc: &mut XmlDocument, set_count: Option<u32>, build: impl FnOnce(&mut LensMut<'_, T>) -> Result<()>) -> Result<()>
where
    T: ElementSpec + ChildOf<sml::Stylesheet>,
{
    let mut ss = doc.lens_mut::<sml::Stylesheet>()?;
    let mut t = ss.get_or_insert::<T>()?;
    build(&mut t)?;
    if let Some(n) = set_count {
        t.set_attr(QName::new("", "count"), &n.to_string())?;
    }
    Ok(())
}

impl Planner {
    fn stage(&mut self, wb: &mut Workbook) -> Result<&mut XmlDocument> {
        if self.staged.is_none() {
            self.staged = Some(wb.pkg.xml_part(&self.next.part)?.clone());
        }
        Ok(self.staged.as_mut().expect("staged"))
    }

    /// Reuses an entry with exactly these bytes, else appends it.
    fn entry<T>(&mut self, wb: &mut Workbook, w: Which, build: &dyn Fn(&mut LensMut<'_, T>) -> Result<()>) -> Result<u32>
    where
        T: ElementSpec + ChildOf<sml::Stylesheet>,
    {
        let key = render::<T>(self.next.prefix(w).as_deref(), |p| build(p))?;
        let t = self.next.table(w);
        if t.stable
            && let Some(&i) = t.reuse.get(&key)
        {
            return Ok(i);
        }
        self.ensure_reserved(wb, w)?;
        self.push::<T>(wb, w, key, build)
    }

    /// Appends a new entry after the dangling-reference check (ADR-0006 §2,
    /// review m2): an existing reference at or past the end would silently
    /// start meaning the new entry.
    fn push<T>(&mut self, wb: &mut Workbook, w: Which, key: String, build: &dyn Fn(&mut LensMut<'_, T>) -> Result<()>) -> Result<u32>
    where
        T: ElementSpec + ChildOf<sml::Stylesheet>,
    {
        let count = self.next.table(w).count;
        if let Some(&m) = self.next.max_ref.get(&w)
            && m >= count
        {
            return Err(Error::DanglingReference { table: w.name().into(), index: m });
        }
        self.push_reserved::<T>(wb, w, key, build)
    }

    /// Appends without the dangling check: the reserved entries give an
    /// index-0 (or `fills[1]`) reference exactly the meaning a reader
    /// already assumes (review n8).
    fn push_reserved<T>(&mut self, wb: &mut Workbook, w: Which, key: String, build: &dyn Fn(&mut LensMut<'_, T>) -> Result<()>) -> Result<u32>
    where
        T: ElementSpec + ChildOf<sml::Stylesheet>,
    {
        let t = self.next.table(w);
        if t.has_ac {
            return Err(ooxml_core::Error::UnresolvedSibling(T::QNAME).into());
        }
        t.require_stable()?;
        let index = t.count;
        let created = !self.next.exists.contains(&w);
        let set_count = (created || self.next.count_attr.contains(&w)).then_some(index + 1);
        let doc = self.stage(wb)?;
        append_to::<T>(doc, set_count, |p| build(p))?;
        if created {
            self.next.exists.insert(w);
            self.next.count_attr.insert(w);
            self.next.prefixes.insert(w, self.next.root_prefix.clone());
        }
        let t = self.next.tables.get_mut(&w).expect("read");
        t.count += 1;
        Arc::make_mut(&mut t.reuse).entry(key).or_insert(index);
        Ok(index)
    }

    /// Reserved leading entries of an empty table (ADR-0006 §5.4).
    fn ensure_reserved(&mut self, wb: &mut Workbook, w: Which) -> Result<()> {
        let count = self.next.table(w).count;
        match w {
            Which::Fonts if count == 0 => {
                let f = Font::default();
                let key = render::<sml::Fonts>(self.next.prefix(w).as_deref(), |p| build_font(p, &f))?;
                self.push_reserved::<sml::Fonts>(wb, w, key, &|p| build_font(p, &f))?;
            }
            Which::Borders if count == 0 => {
                let b = Border::default();
                let key = render::<sml::Borders>(self.next.prefix(w).as_deref(), |p| build_border(p, &b))?;
                self.push_reserved::<sml::Borders>(wb, w, key, &|p| build_border(p, &b))?;
            }
            Which::Fills if count < 2 => {
                for k in count..2 {
                    let f = if k == 0 { Fill::NoFill } else { gray125() };
                    let key = render::<sml::Fills>(self.next.prefix(w).as_deref(), |p| build_fill(p, &f))?;
                    self.push_reserved::<sml::Fills>(wb, w, key, &|p| build_fill(p, &f))?;
                }
            }
            Which::CellXfs if count == 0 => {
                for dep in [Which::Fonts, Which::Fills, Which::Borders] {
                    self.ensure_reserved(wb, dep)?;
                }
                let mut spec = XfInfo::default();
                for k in ["numFmtId", "fontId", "fillId", "borderId"] {
                    spec.set(k, "0");
                }
                if self.next.csx.count > 0 {
                    spec.set("xfId", "0");
                }
                let key = render::<sml::CellFormats>(self.next.prefix(w).as_deref(), |p| build_xf(p, &spec))?;
                self.push_reserved::<sml::CellFormats>(wb, w, key, &|p| build_xf(p, &spec))?;
                let xfs = Arc::make_mut(&mut self.next.xfs);
                xfs.resize(1, None);
                xfs[0] = Some(spec);
                if self.next.default.is_none() {
                    self.next.default = self.next.xfs[0].clone();
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn num_fmt_id(&mut self, wb: &mut Workbook, nf: &NumFormat) -> Result<u32> {
        match nf {
            NumFormat::General => self.num_fmt_id(wb, &NumFormat::Builtin(0)),
            NumFormat::DefaultDate => {
                if self.next.redefined.contains(&14) {
                    self.num_fmt_id(wb, &NumFormat::Code("yyyy-mm-dd".into()))
                } else {
                    Ok(14)
                }
            }
            NumFormat::Builtin(id) => {
                if *id >= 164 {
                    return Err(Error::InvalidFormat(format!("numFmtId {id} is not a built-in id (0-163)")));
                }
                let id = u32::from(*id);
                if self.next.redefined.contains(&id) {
                    return Err(Error::BuiltinNumFormatRedefined { id });
                }
                Ok(id)
            }
            NumFormat::Code(code) => {
                if code.is_empty() {
                    return Err(Error::InvalidFormat("empty number format code".into()));
                }
                if let Some((id, _)) = BUILTIN.iter().find(|(id, c)| c == code && !super::format::LOCALE_DEPENDENT.contains(id))
                    && !self.next.redefined.contains(&u32::from(*id))
                {
                    return Ok(u32::from(*id));
                }
                if let Some(&id) = self.next.custom_codes.get(code) {
                    return Ok(id);
                }
                if crate::escape::encode(code) != *code || ooxml_core::value::first_invalid_xml_char(code).is_some() {
                    return Err(Error::InvalidNumFormat(code.clone()));
                }
                let id = self.next.next_numfmt;
                if id > 65_535 {
                    return Err(Error::NumFmtIdExhausted);
                }
                let code_c = code.clone();
                let key = render::<sml::NumberingFormats>(self.next.prefix(Which::NumFmts).as_deref(), |p| build_numfmt(p, id, &code_c))?;
                self.push::<sml::NumberingFormats>(wb, Which::NumFmts, key, &|p| build_numfmt(p, id, &code_c))?;
                self.next.next_numfmt += 1;
                self.next.custom_count += 1;
                Arc::make_mut(&mut self.next.custom_codes).insert(code.clone(), id);
                Arc::make_mut(&mut self.next.codes).insert(id, code.clone());
                Ok(id)
            }
        }
    }

    /// The `(numFmtId, formatCode)` of a `dxf/numFmt`. Built-in ids keep
    /// their id with the table's code. A custom code reuses an id from
    /// `dxfs` or `numFmts`, or gets one above every id in the package
    /// (the same space as `numFmts`, ADR-0006 §5.3).
    fn dxf_num_fmt(&mut self, nf: &NumFormat) -> Result<(u32, String)> {
        let builtin = |next: &Styles, id: u16| -> Result<(u32, String)> {
            if next.redefined.contains(&u32::from(id)) {
                return Err(Error::BuiltinNumFormatRedefined { id: u32::from(id) });
            }
            let code = BUILTIN.iter().find(|(b, _)| *b == id).map(|(_, c)| (*c).to_owned());
            code.map(|c| (u32::from(id), c)).ok_or_else(|| Error::InvalidFormat(format!("built-in number format {id} has no fixed code; give the code")))
        };
        match nf {
            NumFormat::General => builtin(&self.next, 0),
            NumFormat::Builtin(id) if *id >= 164 => Err(Error::InvalidFormat(format!("numFmtId {id} is not a built-in id (0-163)"))),
            NumFormat::Builtin(id) => builtin(&self.next, *id),
            NumFormat::DefaultDate if self.next.redefined.contains(&14) => self.dxf_num_fmt(&NumFormat::Code("yyyy-mm-dd".into())),
            NumFormat::DefaultDate => builtin(&self.next, 14),
            NumFormat::Code(code) => {
                if code.is_empty() {
                    return Err(Error::InvalidFormat("empty number format code".into()));
                }
                if let Some((id, _)) = BUILTIN.iter().find(|(id, c)| c == code && !super::format::LOCALE_DEPENDENT.contains(id))
                    && !self.next.redefined.contains(&u32::from(*id))
                {
                    return Ok((u32::from(*id), code.clone()));
                }
                if crate::escape::encode(code) != *code || ooxml_core::value::first_invalid_xml_char(code).is_some() {
                    return Err(Error::InvalidNumFormat(code.clone()));
                }
                if let Some(&id) = self.next.dxf_codes.get(code).or_else(|| self.next.custom_codes.get(code)) {
                    return Ok((id, code.clone()));
                }
                let id = self.next.next_numfmt;
                if id > 65_535 {
                    return Err(Error::NumFmtIdExhausted);
                }
                self.next.next_numfmt += 1;
                Arc::make_mut(&mut self.next.dxf_codes).insert(code.clone(), id);
                Ok((id, code.clone()))
            }
        }
    }

    fn dxf(&mut self, wb: &mut Workbook, d: &DxfFormat) -> Result<u32> {
        d.validate()?;
        let nf = d.num_format.as_ref().map(|n| self.dxf_num_fmt(n)).transpose()?;
        let nf = nf.as_ref().map(|(i, c)| (*i, c.as_str()));
        self.entry::<sml::DifferentialFormats>(wb, Which::Dxfs, &|p| build_dxf(p, d, nf))
    }

    fn default_attr(&self, k: &str) -> String {
        self.next.default.as_ref().and_then(|d| d.get(k)).unwrap_or("0").to_owned()
    }

    fn format(&mut self, wb: &mut Workbook, f: &Format) -> Result<u32> {
        f.validate()?;
        let inherits = f.num_format.is_none() || f.font.is_none() || f.fill.is_none() || f.border.is_none() || f.alignment.is_none() || f.protection.is_none();
        if inherits && self.next.default_unreadable {
            return Err(Error::NotExpressible("the Normal style (cellStyleXfs[0]) is inside mc:AlternateContent; a Format with unset groups cannot inherit it".into()));
        }
        let mut spec = XfInfo::default();
        let num = match &f.num_format {
            Some(nf) => self.num_fmt_id(wb, nf)?.to_string(),
            None => self.default_attr("numFmtId"),
        };
        let font = match &f.font {
            Some(font) => self.entry::<sml::Fonts>(wb, Which::Fonts, &|p| build_font(p, font))?.to_string(),
            None => {
                self.ensure_reserved(wb, Which::Fonts)?;
                self.default_attr("fontId")
            }
        };
        let fill = match &f.fill {
            Some(fill) => {
                self.ensure_reserved(wb, Which::Fills)?;
                self.entry::<sml::Fills>(wb, Which::Fills, &|p| build_fill(p, fill))?.to_string()
            }
            None => {
                self.ensure_reserved(wb, Which::Fills)?;
                self.default_attr("fillId")
            }
        };
        let border = match &f.border {
            Some(b) => self.entry::<sml::Borders>(wb, Which::Borders, &|p| build_border(p, b))?.to_string(),
            None => {
                self.ensure_reserved(wb, Which::Borders)?;
                self.default_attr("borderId")
            }
        };
        spec.set("numFmtId", &num);
        spec.set("fontId", &font);
        spec.set("fillId", &fill);
        spec.set("borderId", &border);
        if self.next.csx.count > 0 {
            self.next.csx.require_stable()?;
            spec.set("xfId", "0");
        }
        if f.quote_prefix {
            spec.set("quotePrefix", "1");
        }
        // rust_xlsxwriter's pattern: a flag for each group the format sets
        // to something other than index 0 / the default
        let nonzero = |v: &str| v != "0";
        for (set, k) in [
            (f.num_format.is_some() && nonzero(&num), "applyNumberFormat"),
            (f.font.is_some() && nonzero(&font), "applyFont"),
            (f.fill.is_some() && nonzero(&fill), "applyFill"),
            (f.border.is_some() && nonzero(&border), "applyBorder"),
            (f.alignment.as_ref().is_some_and(|a| !a.attrs().is_empty()), "applyAlignment"),
            (f.protection.is_some_and(|p| p != Protection::default()), "applyProtection"),
        ] {
            if set {
                spec.set(k, "1");
            }
        }
        let default = self.next.default.clone();
        spec.alignment = match &f.alignment {
            Some(a) => Some(a.attrs().into_iter().map(|(k, v)| (k.to_owned(), v)).collect::<Vec<_>>()).filter(|v| !v.is_empty()),
            None => default.as_ref().and_then(|d| d.alignment.clone()),
        };
        spec.protection = match &f.protection {
            Some(pr) => {
                let mut v = Vec::new();
                if !pr.locked {
                    v.push(("locked".to_owned(), "0".to_owned()));
                }
                if pr.hidden {
                    v.push(("hidden".to_owned(), "1".to_owned()));
                }
                Some(v).filter(|v| !v.is_empty())
            }
            None => default.as_ref().and_then(|d| d.protection.clone()),
        };
        self.xf(wb, spec)
    }

    fn xf(&mut self, wb: &mut Workbook, spec: XfInfo) -> Result<u32> {
        self.ensure_reserved(wb, Which::CellXfs)?;
        let s = self.entry::<sml::CellFormats>(wb, Which::CellXfs, &|p| build_xf(p, &spec))?;
        if s as usize >= self.next.xfs.len() {
            let xfs = Arc::make_mut(&mut self.next.xfs);
            xfs.resize(s as usize + 1, None);
            xfs[s as usize] = Some(spec);
        }
        Ok(s)
    }

    /// The cell's current `xf` (an empty `cellXfs` first gets its reserved
    /// `xf` 0, which a cell without `s` already means). A cell whose `xf`
    /// is an `mc:AlternateContent` entry, or whose `s` is out of range,
    /// cannot be derived from: using any other `xf` would change its look.
    fn current(&mut self, wb: &mut Workbook, current: u32) -> Result<XfInfo> {
        self.next.table(Which::CellXfs).require_stable()?;
        self.ensure_reserved(wb, Which::CellXfs)?;
        let count = self.next.table(Which::CellXfs).count;
        if current >= count {
            return Err(Error::DanglingReference { table: "cellXfs".into(), index: current });
        }
        self.next
            .xfs
            .get(current as usize)
            .cloned()
            .flatten()
            .ok_or_else(|| Error::NotExpressible(format!("cellXfs[{current}] is an mc:AlternateContent entry")))
    }

    fn named(&mut self, wb: &mut Workbook, name: &str, current: u32) -> Result<u32> {
        let fold = crate::formula::precedents::fold;
        let hits: Vec<u32> = self.next.named.iter().filter(|(n, _)| fold(n) == fold(name)).map(|(_, x)| *x).collect();
        let xf_id = match hits[..] {
            [x] => x,
            [] => return Err(Error::NoSuchStyle(name.to_owned())),
            _ => return Err(Error::AmbiguousStyleName(name.to_owned())),
        };
        self.next.csx.require_stable()?;
        let style = self.next.csx_infos.get(xf_id as usize).cloned().flatten().ok_or_else(|| Error::NoSuchStyle(name.to_owned()))?;
        if style.opaque {
            return Err(Error::NotExpressible(format!("named style {name:?} has extension content")));
        }
        let cell = self.current(wb, current)?;
        let mut spec = XfInfo::default();
        for (id, apply) in [("numFmtId", "applyNumberFormat"), ("fontId", "applyFont"), ("fillId", "applyFill"), ("borderId", "applyBorder")] {
            let from_style = style.get(apply) != Some("0") && style.get(apply) != Some("false");
            let v = if from_style { style.get(id) } else { cell.get(id) }.unwrap_or("0").to_owned();
            spec.set(id, &v);
        }
        spec.set("xfId", &xf_id.to_string());
        for apply in ["applyNumberFormat", "applyFont", "applyFill", "applyBorder", "applyAlignment", "applyProtection"] {
            if style.get(apply) != Some("0") && style.get(apply) != Some("false") {
                spec.set(apply, "1");
            }
        }
        let al = style.get("applyAlignment") != Some("0") && style.get("applyAlignment") != Some("false");
        spec.alignment = if al { style.alignment.clone() } else { cell.alignment.clone() };
        let pr = style.get("applyProtection") != Some("0") && style.get("applyProtection") != Some("false");
        spec.protection = if pr { style.protection.clone() } else { cell.protection.clone() };
        self.xf(wb, spec)
    }

    /// A preserving edit (ADR-0006 §5.6, Q4): the cell's current `xf`
    /// with one group replaced. Only that group's id attribute and its
    /// `apply*` flag change; every other attribute and the `alignment` and
    /// `protection` children are carried over as they are, so the cell's
    /// named style and the rest of its look are kept. Ids are copied by
    /// index; no existing entry is parsed into a value.
    fn derive(&mut self, wb: &mut Workbook, group: &Group<'_>, current: u32) -> Result<u32> {
        let mut spec = self.current(wb, current)?;
        if spec.opaque {
            return Err(Error::NotExpressible(format!("cellXfs[{current}] has content the registry does not model")));
        }
        let probe = Format {
            font: match group {
                Group::Font(f) => Some((*f).clone()),
                _ => None,
            },
            fill: match group {
                Group::Fill(f) => Some((*f).clone()),
                _ => None,
            },
            border: match group {
                Group::Border(b) => Some((*b).clone()),
                _ => None,
            },
            ..Format::default()
        };
        probe.validate()?;
        self.apply_group(wb, group, &mut spec)?;
        self.xf(wb, spec)
    }

    fn patch(&mut self, wb: &mut Workbook, format: &Format, current: u32) -> Result<u32> {
        format.validate()?;
        let mut spec = self.current(wb, current)?;
        if spec.opaque {
            return Err(Error::NotExpressible(format!("cellXfs[{current}] has content the registry does not model")));
        }
        for group in [
            format.num_format.as_ref().map(Group::NumFormat),
            format.font.as_ref().map(Group::Font),
            format.fill.as_ref().map(Group::Fill),
            format.border.as_ref().map(Group::Border),
        ].into_iter().flatten() {
            self.apply_group(wb, &group, &mut spec)?;
        }
        if let Some(alignment) = &format.alignment {
            let attrs = alignment.attrs();
            spec.alignment = (!attrs.is_empty()).then(|| attrs.into_iter().map(|(key, value)| (key.into(), value)).collect());
            spec.set("applyAlignment", if spec.alignment.is_some() { "1" } else { "0" });
        }
        if let Some(protection) = format.protection {
            spec.protection = Some(vec![
                ("locked".into(), u8::from(protection.locked).to_string()),
                ("hidden".into(), u8::from(protection.hidden).to_string()),
            ]);
            spec.set("applyProtection", "1");
        }
        if format.quote_prefix {
            spec.set("quotePrefix", "1");
        }
        self.xf(wb, spec)
    }

    fn apply_group(&mut self, wb: &mut Workbook, group: &Group<'_>, spec: &mut XfInfo) -> Result<()> {
        let (id_attr, apply, id) = match group {
            Group::NumFormat(nf) => ("numFmtId", "applyNumberFormat", self.num_fmt_id(wb, nf)?),
            Group::Font(f) => ("fontId", "applyFont", self.entry::<sml::Fonts>(wb, Which::Fonts, &|p| build_font(p, f))?),
            Group::Fill(f) => {
                self.ensure_reserved(wb, Which::Fills)?;
                ("fillId", "applyFill", self.entry::<sml::Fills>(wb, Which::Fills, &|p| build_fill(p, f))?)
            }
            Group::Border(b) => ("borderId", "applyBorder", self.entry::<sml::Borders>(wb, Which::Borders, &|p| build_border(p, b))?),
        };
        spec.set(id_attr, &id.to_string());
        spec.set(apply, "1");
        Ok(())
    }
}

// ------------------------------------------------------------ builders

fn gray125() -> Fill {
    Fill::Pattern { pattern: "gray125".into(), fg: None, bg: None }
}

fn color<C: ElementSpec>(l: &mut LensMut<'_, C>, c: &Color) -> Result<()> {
    for (k, v) in c.attrs() {
        l.set_attr(QName::new("", k), &v)?;
    }
    Ok(())
}

pub(crate) fn build_font(p: &mut LensMut<'_, sml::Fonts>, f: &Font) -> Result<()> {
    let mut n = p.insert::<sml::Font>()?;
    if f.bold {
        n.insert::<sml::Bold>()?;
    }
    if f.italic {
        n.insert::<sml::Italic>()?;
    }
    if f.strike {
        n.insert::<sml::Strike>()?;
    }
    if f.condense {
        n.insert::<sml::Condense>()?;
    }
    if f.extend {
        n.insert::<sml::Extend>()?;
    }
    if let Some(u) = f.underline {
        let mut e = n.insert::<sml::Underline>()?;
        match u {
            Underline::Single => {}
            Underline::Double => e.set_attr(QName::new("", "val"), "double")?,
            Underline::SingleAccounting => e.set_attr(QName::new("", "val"), "singleAccounting")?,
            Underline::DoubleAccounting => e.set_attr(QName::new("", "val"), "doubleAccounting")?,
        }
    }
    if let Some(v) = f.vert_align {
        n.insert::<sml::VerticalTextAlignment>()?.set_attr(QName::new("", "val"), v.as_str())?;
    }
    n.insert::<sml::FontSize>()?.set_attr(QName::new("", "val"), &f.size.to_string())?;
    if let Some(c) = &f.color {
        color(&mut n.insert::<sml::Color>()?, c)?;
    }
    n.insert::<sml::FontName>()?.set_attr(QName::new("", "val"), &f.name)?;
    if let Some(fam) = f.family {
        n.insert::<sml::FontFamilyNumbering>()?.set_attr(QName::new("", "val"), &fam.to_string())?;
    }
    if let Some(cs) = f.charset {
        n.insert::<sml::FontCharSet>()?.set_attr(QName::new("", "val"), &cs.to_string())?;
    }
    if let Some(s) = &f.scheme {
        n.insert::<sml::FontScheme>()?.set_attr(QName::new("", "val"), s)?;
    }
    Ok(())
}

enum GradientFill {}
enum GradientStop {}
enum GradientColor {}

macro_rules! gradient_element {
    ($tag:ident, $name:literal, $parent:ty, $content:expr) => {
        impl ElementSpec for $tag {
            const QNAME: QName = QName::new(crate::SML_NS, $name);
            const SDK_NAME: &'static str = $name;
            const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
            const CONTENT: ooxml_core::Content = $content;
        }
        impl ooxml_core::ChildOf<$parent> for $tag {}
    };
}

gradient_element!(GradientFill, "gradientFill", sml::Fill, ooxml_core::Content::Slots(&[
    ooxml_core::Slot { names: &[GradientStop::QNAME], wildcard: None, max_each: None, max_total: None },
]));
gradient_element!(GradientStop, "stop", GradientFill, ooxml_core::Content::Slots(&[
    ooxml_core::Slot { names: &[GradientColor::QNAME], wildcard: None, max_each: Some(1), max_total: Some(1) },
]));
gradient_element!(GradientColor, "color", GradientStop, ooxml_core::Content::Empty);

fn build_gradient(n: &mut LensMut<'_, sml::Fill>, geometry: &crate::Gradient, stops: &[(crate::Tint, Color)]) -> Result<()> {
    let mut gradient = n.insert::<GradientFill>()?;
    match geometry {
        crate::Gradient::Angle(degree) => gradient.set_attr(QName::new("", "degree"), &degree.to_string())?,
        crate::Gradient::Path { left, top, right, bottom } => {
            gradient.set_attr(QName::new("", "type"), "path")?;
            for (name, offset) in [("left", left), ("top", top), ("right", right), ("bottom", bottom)] {
                if offset.get() != 0.0 {
                    gradient.set_attr(QName::new("", name), &offset.get().to_string())?;
                }
            }
        }
    }
    for (position, stop_color) in stops {
        let mut stop = gradient.insert::<GradientStop>()?;
        stop.set_attr(QName::new("", "position"), &position.get().to_string())?;
        color(&mut stop.insert::<GradientColor>()?, stop_color)?;
    }
    Ok(())
}

pub(crate) fn build_fill(p: &mut LensMut<'_, sml::Fills>, f: &Fill) -> Result<()> {
    let mut n = p.insert::<sml::Fill>()?;
    if let Fill::Gradient { geometry, stops } = f {
        return build_gradient(&mut n, geometry, stops);
    }
    let mut pf = n.insert::<sml::PatternFill>()?;
    match f {
        Fill::Gradient { .. } => unreachable!("gradient handled before pattern insertion"),
        Fill::NoFill => pf.set_attr(QName::new("", "patternType"), "none")?,
        Fill::Pattern { pattern, fg, bg } => {
            // rust_xlsxwriter's forms: a bare pattern without colours; a
            // solid/none pattern gets the default `bgColor indexed="64"`
            pf.set_attr(QName::new("", "patternType"), pattern)?;
            if fg.is_none() && bg.is_none() && pattern != "none" {
                return Ok(());
            }
            if let Some(c) = fg {
                color(&mut pf.insert::<sml::ForegroundColor>()?, c)?;
            }
            match bg {
                Some(c) => color(&mut pf.insert::<sml::BackgroundColor>()?, c)?,
                None if pattern == "solid" || pattern == "none" => {
                    pf.insert::<sml::BackgroundColor>()?.set_attr(QName::new("", "indexed"), "64")?;
                }
                None => {}
            }
        }
    }
    Ok(())
}

fn side<S: ElementSpec + ChildOf<sml::Border>>(b: &mut LensMut<'_, sml::Border>, s: &Option<Side>) -> Result<()>
where
    sml::Color: ChildOf<S>,
{
    let mut e = b.insert::<S>()?;
    if let Some(s) = s {
        e.set_attr(QName::new("", "style"), &s.style)?;
        color(&mut e.insert::<sml::Color>()?, s.color.as_ref().unwrap_or(&Color::Auto))?;
    }
    Ok(())
}

pub(crate) fn build_border(p: &mut LensMut<'_, sml::Borders>, b: &Border) -> Result<()> {
    let mut n = p.insert::<sml::Border>()?;
    if b.diagonal_up {
        n.set_attr(QName::new("", "diagonalUp"), "1")?;
    }
    if b.diagonal_down {
        n.set_attr(QName::new("", "diagonalDown"), "1")?;
    }
    side::<sml::LeftBorder>(&mut n, &b.left)?;
    side::<sml::RightBorder>(&mut n, &b.right)?;
    side::<sml::TopBorder>(&mut n, &b.top)?;
    side::<sml::BottomBorder>(&mut n, &b.bottom)?;
    side::<sml::DiagonalBorder>(&mut n, &b.diagonal)?;
    Ok(())
}

/// The canonical `dxf`: only the overriding parts, in schema order (`font`,
/// `numFmt`, `fill`, `alignment`, `border`, `protection`).
pub(crate) fn build_dxf(p: &mut LensMut<'_, sml::DifferentialFormats>, d: &DxfFormat, nf: Option<(u32, &str)>) -> Result<()> {
    let val = QName::new("", "val");
    let mut x = p.insert::<sml::DifferentialFormat>()?;
    if let Some(f) = &d.font {
        let mut n = x.insert::<sml::Font>()?;
        if f.bold {
            n.insert::<sml::Bold>()?;
        }
        if f.italic {
            n.insert::<sml::Italic>()?;
        }
        if f.strike {
            n.insert::<sml::Strike>()?;
        }
        if let Some(u) = f.underline {
            let mut e = n.insert::<sml::Underline>()?;
            match u {
                Underline::Single => {}
                Underline::Double => e.set_attr(val, "double")?,
                Underline::SingleAccounting => e.set_attr(val, "singleAccounting")?,
                Underline::DoubleAccounting => e.set_attr(val, "doubleAccounting")?,
            }
        }
        if let Some(sz) = f.size {
            n.insert::<sml::FontSize>()?.set_attr(val, &sz.to_string())?;
        }
        if let Some(c) = &f.color {
            color(&mut n.insert::<sml::Color>()?, c)?;
        }
        if let Some(name) = &f.name {
            n.insert::<sml::FontName>()?.set_attr(val, name)?;
        }
        if let Some(align) = f.vert_align {
            n.insert::<sml::VerticalTextAlignment>()?.set_attr(val, align.as_str())?;
        }
        if let Some(scheme) = &f.scheme {
            n.insert::<sml::FontScheme>()?.set_attr(val, scheme)?;
        }
        if let Some(family) = f.family {
            n.insert::<sml::FontFamilyNumbering>()?.set_attr(val, &family.to_string())?;
        }
        if let Some(charset) = f.charset {
            n.insert::<sml::FontCharSet>()?.set_attr(val, &charset.to_string())?;
        }
    }
    if let Some((id, code)) = nf {
        let mut n = x.insert::<sml::NumberingFormat>()?;
        n.set_attr(QName::new("", "numFmtId"), &id.to_string())?;
        n.set_attr(QName::new("", "formatCode"), code)?;
    }
    if let Some(fill) = &d.fill {
        let mut n = x.insert::<sml::Fill>()?;
        if let Fill::Gradient { geometry, stops } = fill {
            build_gradient(&mut n, geometry, stops)?;
        } else {
            let mut pf = n.insert::<sml::PatternFill>()?;
            match fill {
                Fill::Gradient { .. } => unreachable!("gradient handled before pattern insertion"),
                Fill::NoFill => pf.set_attr(QName::new("", "patternType"), "none")?,
                Fill::Pattern { pattern, fg, bg } => {
                    // in a dxf an omitted patternType means solid (Excel's form)
                    if pattern != "solid" {
                        pf.set_attr(QName::new("", "patternType"), pattern)?;
                    }
                    if let Some(c) = fg {
                        color(&mut pf.insert::<sml::ForegroundColor>()?, c)?;
                    }
                    if let Some(c) = bg {
                        color(&mut pf.insert::<sml::BackgroundColor>()?, c)?;
                    }
                }
            }
        }
    }
    if let Some(a) = &d.alignment {
        let mut n = x.insert::<sml::Alignment>()?;
        for (k, v) in a.attrs() {
            n.set_attr(QName::new("", k), &v)?;
        }
    }
    if let Some(b) = &d.border {
        let mut n = x.insert::<sml::Border>()?;
        if b.diagonal_up {
            n.set_attr(QName::new("", "diagonalUp"), "1")?;
        }
        if b.diagonal_down {
            n.set_attr(QName::new("", "diagonalDown"), "1")?;
        }
        // only the sides that override
        if b.left.is_some() {
            side::<sml::LeftBorder>(&mut n, &b.left)?;
        }
        if b.right.is_some() {
            side::<sml::RightBorder>(&mut n, &b.right)?;
        }
        if b.top.is_some() {
            side::<sml::TopBorder>(&mut n, &b.top)?;
        }
        if b.bottom.is_some() {
            side::<sml::BottomBorder>(&mut n, &b.bottom)?;
        }
        if b.diagonal.is_some() {
            side::<sml::DiagonalBorder>(&mut n, &b.diagonal)?;
        }
    }
    if let Some(pr) = &d.protection {
        let mut n = x.insert::<sml::Protection>()?;
        n.set_attr(QName::new("", "locked"), if pr.locked { "1" } else { "0" })?;
        n.set_attr(QName::new("", "hidden"), if pr.hidden { "1" } else { "0" })?;
    }
    Ok(())
}

pub(crate) fn build_numfmt(p: &mut LensMut<'_, sml::NumberingFormats>, id: u32, code: &str) -> Result<()> {
    let mut n = p.insert::<sml::NumberingFormat>()?;
    n.set_attr(QName::new("", "numFmtId"), &id.to_string())?;
    n.set_attr(QName::new("", "formatCode"), code)?;
    Ok(())
}

pub(crate) fn build_xf(p: &mut LensMut<'_, sml::CellFormats>, x: &XfInfo) -> Result<()> {
    let mut n = p.insert::<sml::CellFormat>()?;
    for (k, v) in &x.attrs {
        n.set_attr(QName::new("", known(XF_ATTRS, k).ok_or_else(|| Error::Malformed(format!("xf attribute {k}")))?), v)?;
    }
    if let Some(a) = &x.alignment {
        let mut e = n.insert::<sml::Alignment>()?;
        for (k, v) in a {
            e.set_attr(QName::new("", known(ALIGNMENT_ATTRS, k).ok_or_else(|| Error::Malformed(format!("alignment attribute {k}")))?), v)?;
        }
    }
    if let Some(pr) = &x.protection {
        let mut e = n.insert::<sml::Protection>()?;
        for (k, v) in pr {
            e.set_attr(QName::new("", known(PROTECTION_ATTRS, k).ok_or_else(|| Error::Malformed(format!("protection attribute {k}")))?), v)?;
        }
    }
    Ok(())
}

/// The unqualified attributes an `xf` and its `alignment`/`protection`
/// children may carry (ECMA-376 `CT_Xf`, `CT_CellAlignment`,
/// `CT_CellProtection`). `QName` takes `&'static str`, so a name read from
/// the file is mapped onto this closed set; anything else makes the entry
/// opaque (never interned or leaked).
const XF_ATTRS: &[&str] = &[
    "numFmtId", "fontId", "fillId", "borderId", "xfId", "quotePrefix", "pivotButton",
    "applyNumberFormat", "applyFont", "applyFill", "applyBorder", "applyAlignment", "applyProtection",
];
const ALIGNMENT_ATTRS: &[&str] =
    &["horizontal", "vertical", "textRotation", "wrapText", "indent", "relativeIndent", "justifyLastLine", "shrinkToFit", "readingOrder"];
const PROTECTION_ATTRS: &[&str] = &["locked", "hidden"];

fn known(set: &'static [&'static str], k: &str) -> Option<&'static str> {
    set.iter().copied().find(|n| *n == k)
}

/// Exports worksheets using neutral cell formats, retaining the independent
/// named, differential and table style registries for other package parts.
pub(crate) fn without_cell_styles(wb: &Workbook) -> Result<Option<ooxml_core::Package>> {
    let Some(part) = styles_part(wb)? else { return Ok(None) };
    wb.check_editable()?;
    let mut styles = XmlDocument::parse(&wb.pkg.read_part(&part)?)?;
    styles.lens::<sml::Stylesheet>()?;
    let style_table = styles.root().elements().find(|node| node.is(sml::CellFormats::QNAME));
    let formats = Table::read(&styles, style_table, sml::CellFormat::QNAME);
    if !formats.stable || formats.has_ac {
        return Err(Error::UnsupportedStylesPart(part));
    }
    let custom: HashMap<_, _> = styles.root().elements()
        .find(|node| node.is(sml::NumberingFormats::QNAME))
        .into_iter().flat_map(RawNode::elements)
        .filter(|node| node.is(sml::NumberingFormat::QNAME))
        .filter_map(|node| {
            let id = export_style_id(node, QName::new("", "numFmtId"))?;
            Some((id, node.attr(QName::new("", "formatCode"))?.to_owned()))
        })
        .collect();
    let dates: Vec<_> = style_table.into_iter().flat_map(RawNode::elements)
        .map(|node| {
            let id = export_style_id(node, sml::CellFormat::ATTR_NUM_FMT_ID);
            is_date_format(id, id.and_then(|id| custom.get(&id)).map(String::as_str))
        })
        .collect();
    let font = export_style_entry::<sml::Fonts>(&mut styles, sml::Font::QNAME, |table| build_font(table, &Font::default()))?;
    let fill = export_style_entry::<sml::Fills>(&mut styles, sml::Fill::QNAME, |table| build_fill(table, &Fill::NoFill))?;
    let border = export_style_entry::<sml::Borders>(&mut styles, sml::Border::QNAME, |table| build_border(table, &Border::default()))?;
    let xf = |format: u32| XfInfo {
        attrs: [("numFmtId", format), ("fontId", font), ("fillId", fill), ("borderId", border), ("xfId", 0)]
            .into_iter().map(|(key, value)| (key.to_owned(), value.to_string())).collect(),
        ..XfInfo::default()
    };
    let normal = export_style_entry::<sml::CellFormats>(&mut styles, sml::CellFormat::QNAME, |table| build_xf(table, &xf(0)))?;
    let date = if dates.iter().any(|date| *date) {
        let format = if custom.contains_key(&14) {
            let id = scan_ids(wb, &part)?.numfmt.max(163).checked_add(1)
                .ok_or_else(|| Error::Malformed("no available date format ID".into()))?;
            export_style_entry::<sml::NumberingFormats>(&mut styles, sml::NumberingFormat::QNAME, |table| {
                let mut format = table.insert::<sml::NumberingFormat>()?;
                format.set_attr(QName::new("", "numFmtId"), &id.to_string())?;
                format.set_attr(QName::new("", "formatCode"), "mm-dd-yy")?;
                Ok(())
            })?;
            id
        } else {
            14
        };
        Some(export_style_entry::<sml::CellFormats>(&mut styles, sml::CellFormat::QNAME, |table| build_xf(table, &xf(format)))?)
    } else {
        None
    };

    let mut output = wb.pkg.fork();
    for sheet in super::census_sheet_parts(wb) {
        let mut doc = XmlDocument::parse(&wb.pkg.read_part(&sheet)?)?;
        export_sheet_styles(&mut doc, &dates, normal, date)?;
        if doc.is_dirty() {
            *output.xml_part_mut(&sheet)? = doc;
        }
    }
    if styles.is_dirty() {
        *output.xml_part_mut(&part)? = styles;
    }
    Ok(Some(output))
}

fn export_style_id(node: &RawNode, attr: QName) -> Option<u32> {
    node.attr(attr).and_then(ooxml_core::value::parse_u32)
}

fn export_style_entry<T>(
    doc: &mut XmlDocument, child: QName, build: impl Fn(&mut LensMut<'_, T>) -> Result<()>,
) -> Result<u32>
where
    T: ElementSpec + ChildOf<sml::Stylesheet>,
{
    let container = doc.root().elements().find(|node| node.is(T::QNAME));
    let table = Table::read(doc, container, child);
    if !table.stable || table.has_ac {
        return Err(Error::UnsupportedStylesPart(T::QNAME.local.into()));
    }
    let prefix = container.unwrap_or_else(|| doc.root()).name().prefix();
    let key = render::<T>(prefix, &build)?;
    if let Some(&index) = table.reuse.get(&key) {
        return Ok(index);
    }
    append_to(doc, Some(table.count + 1), build)?;
    Ok(table.count)
}

fn export_sheet_styles(doc: &mut XmlDocument, dates: &[bool], normal: u32, date: Option<u32>) -> Result<()> {
    let mut column_styles = vec![0; 16_384];
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    for (index, node) in doc.root().indexed_elements() {
        if node.is(sml::Columns::QNAME) {
            for (column_index, col) in node.indexed_elements().filter(|(_, col)| col.is(sml::Column::QNAME)) {
                let start = export_style_id(col, sml::Column::ATTR_MIN);
                let end = export_style_id(col, sml::Column::ATTR_MAX);
                let old = export_style_id(col, sml::Column::ATTR_STYLE).unwrap_or(0);
                if let (Some(start), Some(end)) = (start, end)
                    && (1..=16_384).contains(&start) && (start..=16_384).contains(&end)
                {
                    column_styles[(start - 1) as usize..end as usize].fill(old);
                }
                if col.attr(sml::Column::ATTR_STYLE).is_some() {
                    columns.push([index, column_index]);
                }
            }
        }
        if node.is(sml::SheetData::QNAME) {
            rows.extend(node.indexed_elements().filter(|(_, row)| row.is(sml::Row::QNAME))
                .map(|(row_index, _)| [index, row_index]));
        }
    }
    for path in columns {
        let mut column = sml::ColumnViewMut(doc.lens_mut_at::<sml::Column>(&path)?);
        column.remove_style()?;
    }
    for [data, row] in rows {
        let current = doc.lens_at::<sml::Row>(&[data, row])?;
        let row_style = export_style_id(current.raw(), sml::Row::ATTR_S);
        let reset_row = row_style.is_some() || current.raw().attr(sml::Row::ATTR_CUSTOM_FORMAT).is_some();
        let cells: Vec<_> = current.raw().indexed_elements().filter(|(_, cell)| cell.is(sml::Cell::QNAME))
            .filter_map(|(position, cell)| {
                let explicit = export_style_id(cell, sml::Cell::ATTR_S);
                let old = explicit.or(row_style).or_else(|| {
                    cell.attr(sml::Cell::ATTR_R)
                        .and_then(|address| crate::reference::CellIndex::parse(address).ok())
                        .map(|index| column_styles[index.col as usize])
                }).unwrap_or(0);
                let numeric = matches!(cell.attr(sml::Cell::ATTR_T), None | Some("n"));
                let has_value = cell.elements().any(|child| child.is(sml::CellValue::QNAME));
                let desired = if numeric && has_value && dates.get(old as usize) == Some(&true) {
                    date.unwrap_or(normal)
                } else {
                    normal
                };
                (explicit != (desired != 0).then_some(desired)).then_some((position, desired))
            })
            .collect();
        if reset_row {
            let mut row = sml::RowViewMut(doc.lens_mut_at::<sml::Row>(&[data, row])?);
            row.remove_style_index()?;
            row.remove_custom_format()?;
        }
        for (position, desired) in cells {
            let mut cell = sml::CellViewMut(doc.lens_mut_at::<sml::Cell>(&[data, row, position])?);
            if desired == 0 {
                cell.remove_style_index()?;
            } else {
                cell.set_style_index(desired)?;
            }
        }
    }
    Ok(())
}
