//! Reading a cell's format back as a [`Format`] (ADR-0006 §3 "Reading").
//!
//! The result is a **projection**: `apply*` flags are ignored, and every
//! group the `xf` refers to is filled in. It is never used to match an
//! existing entry (writes compare bytes only, §2). Whatever a [`Format`]
//! cannot express makes the result [`FormatRead::NotExpressible`], with the
//! reasons; [`crate::CellRef::style_index`] stays the lossless handle.

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{ElementSpec, QName, RawNode, XmlDocument};
use ooxml_schema::sml;

use super::format::{Alignment, Border, Color, Fill, Font, FontSize, Format, NumFormat, Protection, Side, Tint, Underline, VertAlign};

/// A cell's format, read back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatRead {
    /// Every part of the cell's `xf` is expressible. Writing this `Format`
    /// to another cell gives the same look (not necessarily the same bytes:
    /// `apply*` flags and attribute order may differ).
    Expressible(Box<Format>),
    /// Something in the `xf`, or in the font, fill or border it uses, is
    /// outside the model. Copy the style losslessly with
    /// [`crate::CellMut::set_style_index`] instead.
    NotExpressible {
        /// The cell's `s`.
        style_index: u32,
        /// What could not be expressed (`xfId 3`, `pivotButton`, `font`,
        /// `gradientFill`, …).
        reasons: Vec<String>,
    },
}

fn sml_child<'n>(n: &'n RawNode, local: &str) -> Option<&'n RawNode> {
    n.elements().find(|e| e.name().ns() == crate::SML_NS && e.name().local() == local)
}

fn plain_attrs(n: &RawNode) -> impl Iterator<Item = (&str, &str)> {
    n.attrs().iter().filter(|a| a.name().ns().is_empty()).map(|a| (a.name().local(), a.value()))
}

fn has_foreign_attrs(n: &RawNode) -> bool {
    n.attrs().iter().any(|a| !a.name().ns().is_empty() && a.name().ns() != ooxml_core::name::XMLNS_NS)
}

fn on(v: &str) -> Option<bool> {
    match v.trim() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

/// A `CT_Color` element (`color`, `fgColor`, `bgColor`).
pub(crate) fn color_from_node(e: &RawNode) -> Option<Color> {
    if e.elements().next().is_some() || has_foreign_attrs(e) {
        return None;
    }
    let a: Vec<(&str, &str)> = plain_attrs(e).collect();
    let get = |k: &str| a.iter().find(|(n, _)| *n == k).map(|(_, v)| v.trim());
    let n = a.len();
    match (get("rgb"), get("theme"), get("indexed"), get("auto")) {
        (Some(rgb), None, None, None) if n == 1 && rgb.len() == 8 => Some(Color::Rgb(u32::from_str_radix(rgb, 16).ok()?)),
        (None, Some(t), None, None) => {
            let tint = match get("tint") {
                Some(t) => Tint::new(t.parse().ok()?)?,
                None => Tint::NONE,
            };
            (n == 1 + usize::from(get("tint").is_some())).then_some(())?;
            Some(Color::Theme { index: t.parse().ok()?, tint })
        }
        (None, None, Some(i), None) if n == 1 => Some(Color::Indexed(i.parse().ok()?)),
        (None, None, None, Some(v)) if n == 1 && on(v) == Some(true) => Some(Color::Auto),
        _ => None,
    }
}

/// A `font` entry, or `None` when it has anything a [`Font`] cannot express.
pub(crate) fn font_from_node(n: &RawNode) -> Option<Font> {
    if !n.attrs().is_empty() {
        return None;
    }
    let mut f = Font { color: None, family: None, scheme: None, name: String::new(), ..Font::default() };
    let val = |e: &RawNode| e.attr(QName::new("", "val")).map(str::to_owned);
    let only = |e: &RawNode| e.elements().next().is_none() && e.attrs().iter().all(|a| a.name().ns().is_empty() && a.name().local() == "val");
    let mut has_size = false;
    for e in n.elements() {
        if e.name().ns() != crate::SML_NS {
            return None;
        }
        match e.name().local() {
            "b" | "i" | "strike" | "condense" | "extend" if only(e) => {
                let v = match val(e) {
                    Some(v) => on(&v)?,
                    None => true,
                };
                match e.name().local() {
                    "b" => f.bold = v,
                    "i" => f.italic = v,
                    "condense" => f.condense = v,
                    "extend" => f.extend = v,
                    _ => f.strike = v,
                }
            }
            "charset" if only(e) => f.charset = Some(val(e)?.trim().parse().ok()?),
            "vertAlign" if only(e) => {
                f.vert_align = Some(match val(e)?.trim() {
                    "baseline" => VertAlign::Baseline,
                    "superscript" => VertAlign::Superscript,
                    "subscript" => VertAlign::Subscript,
                    _ => return None,
                })
            }
            "u" if only(e) => {
                f.underline = match val(e).as_deref().map(str::trim) {
                    None | Some("single") => Some(Underline::Single),
                    Some("double") => Some(Underline::Double),
                    Some("singleAccounting") => Some(Underline::SingleAccounting),
                    Some("doubleAccounting") => Some(Underline::DoubleAccounting),
                    Some("none") => None,
                    Some(_) => return None,
                }
            }
            "sz" if only(e) => {
                f.size = FontSize::new(val(e)?.trim().parse().ok()?)?;
                has_size = true;
            }
            "name" if only(e) => f.name = val(e)?,
            "family" if only(e) => f.family = Some(val(e)?.trim().parse().ok()?),
            "scheme" if only(e) => f.scheme = Some(val(e)?),
            "color" => f.color = Some(color_from_node(e)?),
            _ => return None,
        }
    }
    (!f.name.is_empty() && has_size).then_some(f)
}

/// A `fill` entry: a pattern fill (`gradientFill` is outside the model).
pub(crate) fn fill_from_node(n: &RawNode) -> Result<Fill, String> {
    let mut kids = n.elements();
    let (Some(pf), None) = (kids.next(), kids.next()) else { return Err("fill".into()) };
    if !pf.is(sml::PatternFill::QNAME) {
        return Err(format!("fill: {}", pf.name().local()));
    }
    if has_foreign_attrs(pf) || plain_attrs(pf).any(|(k, _)| k != "patternType") {
        return Err("fill attributes".into());
    }
    let pattern = pf.attr(QName::new("", "patternType")).map_or("none", str::trim).to_owned();
    let (mut fg, mut bg) = (None, None);
    for c in pf.elements() {
        let color = color_from_node(c).ok_or("fill colour")?;
        match c.name().local() {
            "fgColor" if c.name().ns() == crate::SML_NS && fg.is_none() => fg = Some(color),
            "bgColor" if c.name().ns() == crate::SML_NS && bg.is_none() => bg = Some(color),
            _ => return Err("fill children".into()),
        }
    }
    Ok(if pattern == "none" && fg.is_none() && bg.is_none() { Fill::NoFill } else { Fill::Pattern { pattern, fg, bg } })
}

/// A `border` entry.
pub(crate) fn border_from_node(n: &RawNode) -> Result<Border, String> {
    let mut b = Border::default();
    if has_foreign_attrs(n) {
        return Err("border attributes".into());
    }
    for (k, v) in plain_attrs(n) {
        match k {
            "diagonalUp" => b.diagonal_up = on(v).ok_or("diagonalUp")?,
            "diagonalDown" => b.diagonal_down = on(v).ok_or("diagonalDown")?,
            // `outline` (default true) only matters for ranges
            "outline" if on(v) == Some(true) => {}
            _ => return Err(format!("border attribute {k}")),
        }
    }
    for e in n.elements() {
        if e.name().ns() != crate::SML_NS || has_foreign_attrs(e) {
            return Err("border children".into());
        }
        let style = e.attr(QName::new("", "style")).map(|s| s.trim().to_owned());
        if plain_attrs(e).any(|(k, _)| k != "style") {
            return Err("border side attributes".into());
        }
        let mut kids = e.elements();
        let color = match (kids.next(), kids.next()) {
            (None, _) => None,
            (Some(c), None) if c.name().ns() == crate::SML_NS && c.name().local() == "color" => Some(color_from_node(c).ok_or("border colour")?),
            _ => return Err("border side children".into()),
        };
        let side = match style.as_deref() {
            None | Some("none") if color.is_none() => None,
            None | Some("none") => return Err("border colour without a style".into()),
            Some(s) => Some(Side { style: s.to_owned(), color }),
        };
        let slot = match e.name().local() {
            "left" => &mut b.left,
            "right" => &mut b.right,
            "top" => &mut b.top,
            "bottom" => &mut b.bottom,
            "diagonal" => &mut b.diagonal,
            // `start`/`end` (bidi) and `vertical`/`horizontal` (ranges)
            other if side.is_none() => {
                let _ = other;
                continue;
            }
            other => return Err(format!("border {other}")),
        };
        *slot = side;
    }
    Ok(b)
}

fn alignment_from_node(n: &RawNode, reasons: &mut Vec<String>) -> Alignment {
    let mut a = Alignment::default();
    if has_foreign_attrs(n) || n.elements().next().is_some() {
        reasons.push("alignment extension content".into());
    }
    for (k, v) in plain_attrs(n) {
        let v = v.trim();
        let num = |v: &str| v.parse::<u16>().ok();
        match k {
            "horizontal" => a.horizontal = Some(v.to_owned()),
            "vertical" => a.vertical = Some(v.to_owned()),
            "textRotation" => match num(v) {
                Some(r) => a.rotation = Some(r),
                None => reasons.push(format!("textRotation {v}")),
            },
            "wrapText" => a.wrap = on(v).unwrap_or(false),
            "indent" => match v.parse::<u8>() {
                Ok(i) => a.indent = Some(i),
                Err(_) => reasons.push(format!("indent {v}")),
            },
            "shrinkToFit" => a.shrink = on(v).unwrap_or(false),
            "readingOrder" => match v.parse::<u8>() {
                Ok(r) => a.reading_order = Some(r),
                Err(_) => reasons.push(format!("readingOrder {v}")),
            },
            "justifyLastLine" | "relativeIndent" => reasons.push(k.to_owned()),
            other => reasons.push(format!("alignment attribute {other}")),
        }
    }
    a
}

fn protection_from_node(n: &RawNode, reasons: &mut Vec<String>) -> Protection {
    let mut p = Protection::default();
    if has_foreign_attrs(n) || n.elements().next().is_some() {
        reasons.push("protection extension content".into());
    }
    for (k, v) in plain_attrs(n) {
        match (k, on(v)) {
            ("locked", Some(b)) => p.locked = b,
            ("hidden", Some(b)) => p.hidden = b,
            _ => reasons.push(format!("protection {k}={v}")),
        }
    }
    p
}

/// The `n`-th element child of a table, refusing an `mc:AlternateContent`.
fn entry<'d>(table: Option<&'d RawNode>, index: u32, what: &str, reasons: &mut Vec<String>) -> Option<&'d RawNode> {
    let Some(t) = table else {
        reasons.push(format!("no {what} table"));
        return None;
    };
    if t.elements().take(index as usize + 1).any(|e| e.is(MC_ALTERNATE_CONTENT)) {
        reasons.push(format!("{what}: mc:AlternateContent"));
        return None;
    }
    let e = t.elements().nth(index as usize);
    if e.is_none() {
        reasons.push(format!("{what}[{index}] does not exist"));
    }
    e
}

/// Reads `cellXfs[s]` of a `styleSheet` as a [`FormatRead`].
pub(crate) fn read_format(doc: &XmlDocument, s: u32) -> FormatRead {
    let root = doc.root();
    let mut reasons = Vec::new();
    let not = |reasons: Vec<String>| FormatRead::NotExpressible { style_index: s, reasons };
    if !root.is(sml::Stylesheet::QNAME) {
        return not(vec!["the styles part is not a SpreadsheetML styleSheet".into()]);
    }
    if root.elements().any(|e| e.is(MC_ALTERNATE_CONTENT)) {
        return not(vec!["styleSheet: mc:AlternateContent".into()]);
    }
    let cell_xfs = sml_child(root, "cellXfs");
    if cell_xfs.is_none() && s == 0 {
        // no cellXfs at all: the cell looks like the application default
        return FormatRead::Expressible(Box::new(Format::new()));
    }
    let Some(xf) = entry(cell_xfs, s, "cellXfs", &mut reasons) else { return not(reasons) };
    // Absent children have application defaults. Unset Format groups inherit Normal.
    let mut f = Format { alignment: Some(Alignment::default()), protection: Some(Protection::default()), ..Format::new() };
    if has_foreign_attrs(xf) {
        reasons.push("xf extension attributes".into());
    }
    let id = |k: &'static str| xf.attr(QName::new("", k)).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
    for (k, v) in plain_attrs(xf) {
        match k {
            "numFmtId" | "fontId" | "fillId" | "borderId" => {}
            "xfId" if v.trim() == "0" => {}
            "xfId" => reasons.push(format!("xfId {}", v.trim())),
            "quotePrefix" => f.quote_prefix = on(v).unwrap_or(false),
            "pivotButton" if on(v) == Some(true) => reasons.push("pivotButton".into()),
            "pivotButton" => {}
            k if k.starts_with("apply") => {}
            other => reasons.push(format!("xf attribute {other}")),
        }
    }
    // number format
    let nf = id("numFmtId");
    let code = sml_child(root, "numFmts").and_then(|t| {
        t.elements()
            .filter(|e| e.is(sml::NumberingFormat::QNAME))
            .find(|e| e.attr(QName::new("", "numFmtId")).and_then(|v| v.trim().parse::<u32>().ok()) == Some(nf))
            .and_then(|e| e.attr(QName::new("", "formatCode")).map(str::to_owned))
    });
    f.num_format = Some(match code {
        Some(c) => NumFormat::Code(c),
        None if nf == 0 => NumFormat::General,
        None if nf < 164 => NumFormat::Builtin(nf as u16),
        None => {
            reasons.push(format!("numFmtId {nf} is not defined"));
            NumFormat::General
        }
    });
    // font, fill, border
    if let Some(n) = entry(sml_child(root, "fonts"), id("fontId"), "fonts", &mut reasons) {
        match font_from_node(n) {
            Some(font) => f.font = Some(font),
            None => reasons.push("font".into()),
        }
    }
    if let Some(n) = entry(sml_child(root, "fills"), id("fillId"), "fills", &mut reasons) {
        match fill_from_node(n) {
            Ok(fill) => f.fill = Some(fill),
            Err(r) => reasons.push(r),
        }
    }
    if let Some(n) = entry(sml_child(root, "borders"), id("borderId"), "borders", &mut reasons) {
        match border_from_node(n) {
            Ok(b) => f.border = Some(b),
            Err(r) => reasons.push(r),
        }
    }
    for c in xf.elements() {
        match c.name().local() {
            "alignment" if c.name().ns() == crate::SML_NS => f.alignment = Some(alignment_from_node(c, &mut reasons)),
            "protection" if c.name().ns() == crate::SML_NS => f.protection = Some(protection_from_node(c, &mut reasons)),
            other => reasons.push(format!("xf child {other}")),
        }
    }
    if reasons.is_empty() { FormatRead::Expressible(Box::new(f)) } else { not(reasons) }
}
