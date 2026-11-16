//! Phase2 registries (ADR-0006): the shared-string table and the style
//! table, append-only, with byte-identical reuse of existing entries.
//!
//! Both are activated lazily by the first write that needs them. Until
//! then `sharedStrings.xml` and `styles.xml` stay exactly as Phase0/1 left
//! them (raw, or dirty-free parsed). Every write plans its registry
//! additions first (the style table on a staged clone; the shared-string
//! append after a dry-run check), and the apply step installs them in the
//! same apply phase as the cell write, under Phase1's poisoning.

pub(crate) mod dates;
pub mod format;
pub(crate) mod strings;
pub(crate) mod styles;

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{ElementSpec, LensMut, QName, RawNode, XmlDocument};

use crate::error::{Error, Result};

/// The registries of one workbook (each `None` until activated).
#[derive(Debug, Default)]
pub(crate) struct Registries {
    pub(crate) strings: Option<strings::SharedStrings>,
    pub(crate) styles: Option<styles::Styles>,
}

/// Exclusive upper bounds from Microsoft's "Excel specifications and
/// limits" (ADR-0006 §5.10). The mapping onto tables is **inferred**
/// (review n10) and kept here so it can be replaced.
pub(crate) const LIMITS: &[(&str, u32)] = &[
    ("cellXfs+cellStyleXfs", 65_490),
    ("fills", 256),
    ("borders", 256),
    ("fonts", 512),
    ("custom numFmts", 200),
];

pub(crate) fn limit(table: &str) -> u32 {
    LIMITS.iter().find(|(t, _)| *t == table).map_or(u32::MAX, |(_, l)| *l)
}

/// One registry table as found at activation (ADR-0006 §2).
#[derive(Clone, Debug, Default)]
pub(crate) struct Table {
    /// Number of entries (an index-stable `mc:AlternateContent` counts as one).
    pub(crate) count: u32,
    /// Has an `mc:AlternateContent` child: appending is refused.
    pub(crate) has_ac: bool,
    /// Every index can be computed (review n9).
    pub(crate) stable: bool,
    /// Canonical bytes → smallest index, for exact-form reuse.
    pub(crate) reuse: std::collections::HashMap<String, u32>,
}

impl Table {
    /// Reads `table`'s entries (`entry` QName) from `doc`.
    pub(crate) fn read(doc: &XmlDocument, table: Option<&RawNode>, entry: QName) -> Table {
        let mut t = Table { stable: true, ..Table::default() };
        let Some(node) = table else { return t };
        for child in node.elements() {
            if child.is(entry) {
                if let Some(span) = child.span() {
                    t.reuse.entry(doc.source()[span.start..span.end()].to_owned()).or_insert(t.count);
                }
                t.count += 1;
            } else if child.is(MC_ALTERNATE_CONTENT) {
                t.has_ac = true;
                if !ac_is_index_stable(child, entry) {
                    t.stable = false;
                }
                t.count += 1;
            } else {
                t.stable = false;
            }
        }
        t
    }

    /// An index-dependent operation needs every index (review n9).
    pub(crate) fn require_stable(&self) -> Result<()> {
        if self.stable {
            Ok(())
        } else {
            Err(ooxml_core::Error::UnresolvedSibling(QName::new(crate::SML_NS, "styleSheet")).into())
        }
    }
}

/// Every branch of `ac` holds exactly one `entry` and no other element, and
/// there is a `Fallback` (ADR-0006 §2, review M4/n9).
fn ac_is_index_stable(ac: &RawNode, entry: QName) -> bool {
    let mc = ooxml_core::name::MC_NS;
    let mut has_fallback = false;
    for branch in ac.elements() {
        let local = branch.name().local();
        if branch.name().ns() != mc || !(local == "Choice" || local == "Fallback") {
            return false;
        }
        has_fallback |= local == "Fallback";
        let mut kids = branch.elements();
        if !matches!((kids.next(), kids.next()), (Some(e), None) if e.is(entry)) {
            return false;
        }
    }
    has_fallback
}

/// The canonical bytes of an entry: built by `build` under a scratch parent
/// `T` with the table's prefix, then cut out of the serialisation. The same
/// builder appends the real entry, so the bytes agree by construction.
pub(crate) fn render<T: ElementSpec>(prefix: Option<&str>, build: impl FnOnce(&mut LensMut<'_, T>) -> Result<()>) -> Result<String> {
    let (local, ns) = (T::QNAME.local, T::QNAME.ns);
    let src = match prefix {
        Some(p) => format!("<{p}:{local} xmlns:{p}=\"{ns}\"/>"),
        None => format!("<{local} xmlns=\"{ns}\"/>"),
    };
    let mut doc = XmlDocument::parse(src.as_bytes())?;
    {
        let mut lens = doc.lens_mut::<T>()?;
        build(&mut lens)?;
    }
    let s = doc.serialize();
    let open_end = s.find('>').ok_or_else(|| Error::Malformed("render".into()))? + 1;
    let close = s.rfind("</").ok_or_else(|| Error::Malformed("render".into()))?;
    Ok(s[open_end..close].to_owned())
}

/// What a sheet part holds that the registries need: the number of `c`
/// elements with `t="s"`, and the largest `s` (cell, row) or `style`
/// (column) value.
#[derive(Debug, Default)]
pub(crate) struct SheetScan {
    pub(crate) shared: u64,
    pub(crate) max_s: Option<u32>,
}

/// The sheet parts the Phase1 census scans: worksheets, macro sheets and
/// dialog sheets (ADR-0006 review n1).
pub(crate) fn census_sheet_parts(wb: &crate::workbook::Workbook) -> Vec<String> {
    use crate::census::SheetKind;
    wb.sheets
        .iter()
        .filter(|s| matches!(s.kind, SheetKind::Worksheet | SheetKind::MacroSheet | SheetKind::DialogSheet))
        .filter_map(|s| s.part.clone())
        .collect()
}

pub(crate) fn scan_sheet(xml: &[u8]) -> Result<SheetScan> {
    use quick_xml::events::Event;
    use quick_xml::name::ResolveResult;
    use quick_xml::{NsReader, XmlVersion};
    let fail = |e: String| Error::Malformed(format!("cannot scan a sheet part for shared strings and styles: {e}"));
    let mut reader = NsReader::from_reader(xml);
    let mut out = SheetScan::default();
    loop {
        let (ns, ev) = reader.read_resolved_event().map_err(|e| fail(e.to_string()))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) if matches!(ns, ResolveResult::Bound(b) if b.into_inner() == crate::SML_NS) => {
                let local = e.local_name().into_inner().to_owned();
                let style_attr = match local.as_str() {
                    "c" | "row" => "s",
                    "col" => "style",
                    _ => continue,
                };
                for a in e.attributes().with_checks(true) {
                    let a = a.map_err(|e| fail(e.to_string()))?;
                    if a.key.prefix().is_some() {
                        continue;
                    }
                    let k = a.key.local_name().into_inner().to_owned();
                    if local == "c" && k == "t" || k == style_attr {
                        let v = a.normalized_value(XmlVersion::Implicit1_0).map_err(|e| fail(e.to_string()))?;
                        if k == "t" {
                            out.shared += u64::from(v.trim() == "s");
                        } else if let Ok(n) = v.trim().parse::<u32>() {
                            out.max_s = out.max_s.max(Some(n));
                        }
                    }
                }
            }
            Event::Eof => return Ok(out),
            _ => {}
        }
    }
}
