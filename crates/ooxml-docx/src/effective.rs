//! Read-only Word formatting cascade (ADR-0015 §8).

use std::collections::BTreeMap;

use ooxml_core::{QName, RawNode, value::parse_on_off};
use ooxml_schema::wml::NS_W;

use crate::{Document, Error, Paragraph, Result, Run, StyleKind};

const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const TOGGLES: &[&str] = &[
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "vanish",
];

/// The level that determined a resolved property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Provenance {
    /// Application defaults.
    ApplicationDefault,
    /// The document's default run or paragraph properties.
    DocumentDefaults,
    /// The enclosing table's style.
    TableStyle,
    /// The paragraph style chain.
    ParagraphStyle,
    /// The paragraph's numbering level.
    Numbering,
    /// The character style chain.
    CharacterStyle,
    /// Formatting on the run or paragraph itself.
    Direct,
}

/// A value together with its source in the cascade.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveValue<T> {
    /// The resolved value.
    pub value: T,
    /// The level that determined the value.
    pub provenance: Provenance,
}

/// Resolved run or paragraph attributes, using WordprocessingML local names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffectiveProperties(BTreeMap<String, BTreeMap<String, EffectiveValue<String>>>);

impl EffectiveProperties {
    /// Gets one resolved property attribute, for example `("ind", "left")`.
    pub fn attribute(&self, element: &str, attribute: &str) -> Option<&EffectiveValue<String>> {
        self.0.get(element)?.get(attribute)
    }

    /// Gets a property's `w:val` attribute, for example `"jc"`.
    pub fn value(&self, element: &str) -> Option<&EffectiveValue<String>> {
        self.attribute(element, "val")
    }

    fn set(
        &mut self,
        element: &str,
        attribute: &str,
        value: impl Into<String>,
        provenance: Provenance,
    ) {
        self.0.entry(element.into()).or_default().insert(
            attribute.into(),
            EffectiveValue {
                value: value.into(),
                provenance,
            },
        );
    }

    fn overlay(&mut self, other: Self) {
        for (element, attributes) in other.0 {
            self.0.entry(element).or_default().extend(attributes);
        }
    }
}

/// Effective formatting of a run, with a source per attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveRun {
    /// All resolved run attributes, including the Latin and complex-script slots.
    pub properties: EffectiveProperties,
}

impl EffectiveRun {
    /// Resolves a Boolean toggle, such as `"b"` or `"iCs"`.
    pub fn toggle(&self, element: &str) -> Option<EffectiveValue<bool>> {
        let value = self.properties.value(element)?;
        Some(EffectiveValue {
            value: parse_on_off(&value.value)?,
            provenance: value.provenance,
        })
    }

    /// Latin font size in half-points.
    pub fn size_half_points(&self) -> Option<EffectiveValue<u32>> {
        let size = self.properties.value("sz")?;
        Some(EffectiveValue {
            value: size.value.parse().ok()?,
            provenance: size.provenance,
        })
    }
}

/// Effective formatting of a paragraph and its paragraph mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveParagraph {
    /// Resolved paragraph properties.
    pub properties: EffectiveProperties,
    /// Effective formatting of the paragraph mark.
    pub mark: EffectiveRun,
}

const fn w(local: &'static str) -> QName {
    QName::new(NS_W, local)
}

fn child<'a>(node: &'a RawNode, local: &'static str) -> Option<&'a RawNode> {
    node.elements().find(|node| node.is(w(local)))
}

fn val<'a>(node: &'a RawNode, local: &'static str) -> Option<&'a str> {
    child(node, local)?.attr(w("val"))
}

fn on(node: &RawNode) -> Option<bool> {
    node.attr(w("val")).map_or(Some(true), parse_on_off)
}

const FONT_SLOTS: [(&str, &str); 4] = [
    ("ascii", "asciiTheme"),
    ("hAnsi", "hAnsiTheme"),
    ("eastAsia", "eastAsiaTheme"),
    ("cs", "cstheme"),
];

fn properties(
    target: &mut EffectiveProperties,
    container: Option<&RawNode>,
    source: Provenance,
    theme: &Theme<'_>,
) {
    for property in container.into_iter().flat_map(RawNode::elements) {
        let name = property.name();
        if name.ns() != NS_W {
            continue;
        }
        let element = name.local();
        match element {
            "rPr" | "pPrChange" | "rPrChange" | "cnfStyle" | "pStyle" | "rStyle" | "sectPr"
            | "numPr" => continue,
            "rFonts" => {
                for (attribute, themed) in FONT_SLOTS {
                    if let Some(value) = property
                        .attr(w(themed))
                        .and_then(|key| theme.font(key, attribute))
                        .or_else(|| property.attr(w(attribute)))
                    {
                        target.set(element, attribute, value, source);
                    }
                }
            }
            "color" => {
                if let Some(rgb) = theme
                    .color(property)
                    .or_else(|| property.attr(w("val")).map(str::to_owned))
                {
                    target.set(element, "val", rgb, source);
                }
            }
            _ if TOGGLES.contains(&element)
                || matches!(
                    element,
                    "keepNext" | "keepLines" | "pageBreakBefore" | "widowControl"
                ) =>
            {
                if let Some(value) = on(property) {
                    target.set(element, "val", if value { "1" } else { "0" }, source);
                }
            }
            _ => {
                if element == "ind" {
                    for (first, second) in
                        [("firstLine", "hanging"), ("firstLineChars", "hangingChars")]
                    {
                        if property.attr(w(second)).is_some() {
                            if let Some(attrs) = target.0.get_mut(element) {
                                attrs.remove(first);
                            }
                        } else if property.attr(w(first)).is_some()
                            && let Some(attrs) = target.0.get_mut(element)
                        {
                            attrs.remove(second);
                        }
                    }
                }
                for attribute in property
                    .attrs()
                    .iter()
                    .filter(|attr| attr.name().ns() == NS_W)
                {
                    target.set(element, attribute.name().local(), attribute.value(), source);
                }
            }
        }
    }
}

struct Theme<'a> {
    elements: Option<&'a RawNode>,
    settings: Option<&'a RawNode>,
}

fn a_child<'a>(node: &'a RawNode, local: &'static str) -> Option<&'a RawNode> {
    node.elements()
        .find(|node| node.is(QName::new(NS_A, local)))
}

impl<'a> Theme<'a> {
    fn font(&self, key: &str, slot: &str) -> Option<&'a str> {
        let (kind, script) = key
            .strip_prefix("major")
            .map(|script| ("majorFont", script))
            .or_else(|| {
                key.strip_prefix("minor")
                    .map(|script| ("minorFont", script))
            })?;
        let scheme = a_child(self.elements?, "fontScheme")?;
        let group = a_child(scheme, kind)?;
        let attribute = QName::new("", "typeface");
        let family = match script {
            "Ascii" | "HAnsi" => "latin",
            "EastAsia" => "ea",
            "Bidi" => "cs",
            _ => return None,
        };
        let lang = self
            .settings
            .and_then(|settings| child(settings, "themeFontLang"));
        let script = match slot {
            "eastAsia" => lang
                .and_then(|lang| lang.attr(w("eastAsia")))
                .and_then(script_for_language),
            "cs" => lang
                .and_then(|lang| lang.attr(w("bidi")))
                .and_then(script_for_language),
            _ => None,
        };
        if let Some(script) = script
            && let Some(font) = group
                .elements()
                .find(|font| {
                    font.is(QName::new(NS_A, "font"))
                        && font.attr(QName::new("", "script")) == Some(script)
                })
                .and_then(|font| font.attr(attribute))
                .filter(|s| !s.is_empty())
        {
            return Some(font);
        }
        a_child(group, family)
            .and_then(|font| font.attr(attribute))
            .filter(|s| !s.is_empty())
    }

    fn color(&self, value: &RawNode) -> Option<String> {
        let theme_color = value.attr(w("themeColor"))?;
        let mapping = match theme_color {
            "background1" => "bg1",
            "background2" => "bg2",
            "text1" => "t1",
            "text2" => "t2",
            "hyperlink" => "hyperlink",
            "followedHyperlink" => "followedHyperlink",
            "accent1" => "accent1",
            "accent2" => "accent2",
            "accent3" => "accent3",
            "accent4" => "accent4",
            "accent5" => "accent5",
            "accent6" => "accent6",
            _ => "",
        };
        let key = self
            .settings
            .and_then(|settings| child(settings, "clrSchemeMapping"))
            .and_then(|map| map.attr(w(mapping)))
            .unwrap_or(theme_color);
        let key = match key {
            "dark1" | "text1" => "dk1",
            "dark2" | "text2" => "dk2",
            "light1" | "background1" => "lt1",
            "light2" | "background2" => "lt2",
            "hyperlink" => "hlink",
            "followedHyperlink" => "folHlink",
            "accent1" => "accent1",
            "accent2" => "accent2",
            "accent3" => "accent3",
            "accent4" => "accent4",
            "accent5" => "accent5",
            "accent6" => "accent6",
            _ => return None,
        };
        let scheme = a_child(self.elements?, "clrScheme")?;
        let entry = a_child(scheme, key)?.elements().next()?;
        let rgb = entry.attr(QName::new(
            "",
            if entry.is(QName::new(NS_A, "sysClr")) {
                "lastClr"
            } else {
                "val"
            },
        ))?;
        let rgb = u32::from_str_radix(rgb, 16)
            .ok()
            .filter(|rgb| *rgb <= 0xff_ffff)?;
        let factor = |name| {
            value
                .attr(w(name))
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .map(|n| f64::from(n) / 255.0)
        };
        Some(format!(
            "{:06X}",
            shade_tint(rgb, factor("themeShade"), factor("themeTint"))
        ))
    }
}

fn script_for_language(lang: &str) -> Option<&'static str> {
    let primary = lang.split('-').next()?;
    match primary {
        "zh" if lang.contains("Hant")
            || lang.ends_with("-TW")
            || lang.ends_with("-HK")
            || lang.ends_with("-MO") =>
        {
            Some("Hant")
        }
        "zh" => Some("Hans"),
        "ja" => Some("Jpan"),
        "ko" => Some("Hang"),
        "ar" | "fa" | "ur" => Some("Arab"),
        "he" | "yi" => Some("Hebr"),
        "th" => Some("Thai"),
        "hi" | "mr" | "ne" => Some("Deva"),
        "bn" => Some("Beng"),
        "ta" => Some("Taml"),
        "te" => Some("Telu"),
        "gu" => Some("Gujr"),
        "pa" => Some("Guru"),
        "kn" => Some("Knda"),
        "ml" => Some("Mlym"),
        "si" => Some("Sinh"),
        "km" => Some("Khmr"),
        "lo" => Some("Laoo"),
        "my" => Some("Mymr"),
        "bo" => Some("Tibt"),
        "am" => Some("Ethi"),
        "syr" => Some("Syrc"),
        "dv" => Some("Thaa"),
        _ => None,
    }
}

fn shade_tint(rgb: u32, shade: Option<f64>, tint: Option<f64>) -> u32 {
    if shade.unwrap_or(1.0) == 1.0 && tint.unwrap_or(1.0) == 1.0 {
        return rgb;
    }
    let channels = [16, 8, 0].map(|shift| f64::from((rgb >> shift) & 255) / 255.0);
    let max = channels.into_iter().reduce(f64::max).expect("RGB channels");
    let min = channels.into_iter().reduce(f64::min).expect("RGB channels");
    let lightness = (max + min) / 2.0;
    let mut adjusted = lightness * shade.unwrap_or(1.0);
    if let Some(tint) = tint {
        adjusted = 1.0 - (1.0 - adjusted) * tint;
    }
    let amplitude = lightness.min(1.0 - lightness);
    let scale = if amplitude == 0.0 {
        0.0
    } else {
        adjusted.min(1.0 - adjusted) / amplitude
    };
    channels.into_iter().fold(0, |rgb, channel| {
        let value = (adjusted + (channel - lightness) * scale).clamp(0.0, 1.0) * 255.0;
        // Chromatic Word caches truncate more often than they round.
        // Achromatic colors retain exact byte factors.
        let byte = if amplitude == 0.0 {
            value.round()
        } else {
            (value + 1e-9).floor()
        };
        (rgb << 8) | byte as u32
    })
}

fn chain<'a>(doc: &'a Document, id: Option<&str>, kind: StyleKind) -> Result<Vec<&'a RawNode>> {
    let by_id = |id: &str| {
        doc.style_nodes().find(|node| {
            StyleKind::of(node.attr(w("type"))) == kind && node.attr(w("styleId")) == Some(id)
        })
    };
    let mut next = id.and_then(by_id).or_else(|| {
        doc.style_nodes().find(|node| {
            StyleKind::of(node.attr(w("type"))) == kind
                && node.attr(w("default")).and_then(parse_on_off) == Some(true)
        })
    });
    let mut result: Vec<&RawNode> = Vec::new();
    while let Some(node) = next {
        if result.len() == 64 || result.iter().any(|&prev| std::ptr::eq(prev, node)) {
            return Err(Error::StyleCycle);
        }
        result.push(node);
        next = val(node, "basedOn").and_then(by_id);
    }
    result.reverse();
    Ok(result)
}

fn table_children<'a>(parent: &'a RawNode, name: &'static str) -> Vec<&'a RawNode> {
    let mut result = Vec::new();
    for element in parent.elements() {
        if element.is(w(name)) {
            result.push(element);
        } else if matches!(
            element.name().local(),
            "sdt" | "sdtContent" | "customXml" | "ins" | "del" | "moveTo" | "moveFrom"
        ) {
            result.extend(table_children(element, name));
        }
    }
    result
}

fn grid_span(cell: &RawNode) -> usize {
    child(cell, "tcPr")
        .and_then(|pr| val(pr, "gridSpan"))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

fn grid_before(row: &RawNode) -> usize {
    child(row, "trPr")
        .and_then(|pr| val(pr, "gridBefore"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn table_conditions(
    table: &RawNode,
    row: &RawNode,
    cell: &RawNode,
    table_styles: &[&RawNode],
) -> Vec<&'static str> {
    let tbl_pr = child(table, "tblPr");
    let look = tbl_pr.and_then(|pr| child(pr, "tblLook"));
    let look_flags = look
        .and_then(|look| look.attr(w("val")))
        .and_then(|bits| u16::from_str_radix(bits, 16).ok())
        .unwrap_or(0);
    let flag = |name, bit| {
        look.and_then(|look| look.attr(w(name)))
            .and_then(parse_on_off)
            .unwrap_or(look_flags & bit != 0)
    };
    let band_size = |name| {
        tbl_pr
            .and_then(|pr| val(pr, name))
            .or_else(|| {
                table_styles
                    .iter()
                    .rev()
                    .find_map(|style| child(style, "tblPr").and_then(|pr| val(pr, name)))
            })
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1)
            .max(1)
    };
    let rows = table_children(table, "tr");
    let cells = table_children(row, "tc");
    let Some(row_index) = rows.iter().position(|&item| std::ptr::eq(item, row)) else {
        return vec!["wholeTable"];
    };
    let Some(cell_index) = cells.iter().position(|&item| std::ptr::eq(item, cell)) else {
        return vec!["wholeTable"];
    };
    let col = cells[..cell_index]
        .iter()
        .fold(grid_before(row), |col, cell| {
            col.saturating_add(grid_span(cell))
        });
    let columns = child(table, "tblGrid")
        .map(|grid| grid.elements().filter(|node| node.is(w("gridCol"))).count())
        .filter(|&count| count > 0)
        .unwrap_or_else(|| {
            rows.iter()
                .map(|row| {
                    table_children(row, "tc")
                        .iter()
                        .fold(grid_before(row), |col, cell| {
                            col.saturating_add(grid_span(cell))
                        })
                })
                .max()
                .unwrap_or(0)
        });
    let merge = |cell| {
        child(cell, "tcPr")
            .and_then(|pr| child(pr, "vMerge"))
            .map(|node| node.attr(w("val")) == Some("restart"))
    };
    let mut first_row = row_index;
    if merge(cell) == Some(false) {
        for (index, previous) in rows[..row_index].iter().enumerate().rev() {
            let mut start = grid_before(previous);
            let found = table_children(previous, "tc").into_iter().find(|cell| {
                let current = start;
                start = start.saturating_add(grid_span(cell));
                current == col && grid_span(cell) == grid_span(cells[cell_index])
            });
            match found.and_then(merge) {
                Some(restart) => {
                    first_row = index;
                    if restart {
                        break;
                    }
                }
                None => break,
            }
        }
    }
    let first_row_enabled = flag("firstRow", 0x20);
    let first_col_enabled = flag("firstColumn", 0x80);
    let top = first_row == 0 && first_row_enabled;
    let bottom = row_index + 1 == rows.len() && flag("lastRow", 0x40);
    let left = col == 0 && first_col_enabled;
    let right = col.saturating_add(grid_span(cell)) >= columns && flag("lastColumn", 0x100);
    let mut result = vec!["wholeTable"];
    if !flag("noVBand", 0x400) && !left && !right {
        let band =
            col.saturating_sub(usize::from(first_col_enabled)) / band_size("tblStyleColBandSize");
        result.push(if band.is_multiple_of(2) {
            "band1Vert"
        } else {
            "band2Vert"
        });
    }
    if !flag("noHBand", 0x200) && !top && !bottom {
        let band = first_row.saturating_sub(usize::from(first_row_enabled))
            / band_size("tblStyleRowBandSize");
        result.push(if band.is_multiple_of(2) {
            "band1Horz"
        } else {
            "band2Horz"
        });
    }
    for (active, name) in [
        (left, "firstCol"),
        (right, "lastCol"),
        (top, "firstRow"),
        (bottom, "lastRow"),
        (top && left, "nwCell"),
        (top && right, "neCell"),
        (bottom && left, "swCell"),
        (bottom && right, "seCell"),
    ] {
        if active {
            result.push(name);
        }
    }
    result
}

fn numbering<'a>(
    root: Option<&'a RawNode>,
    pprs: impl Iterator<Item = &'a RawNode>,
    styles: &[&RawNode],
) -> Option<&'a RawNode> {
    let root = root?;
    let mut id = None;
    let mut level = None;
    for numpr in pprs.filter_map(|node| child(node, "numPr")) {
        id = val(numpr, "numId").or(id);
        level = val(numpr, "ilvl").or(level);
    }
    let id = id?.parse::<u32>().ok().filter(|&n| n != 0)?;
    let id_matches =
        |node: &RawNode, key, id| node.attr(w(key)).and_then(|n| n.parse::<u32>().ok()) == Some(id);
    let num = root
        .elements()
        .find(|node| node.is(w("num")) && id_matches(node, "numId", id))?;
    let abstract_id = val(num, "abstractNumId")?.parse::<u32>().ok()?;
    let abstract_num = root
        .elements()
        .find(|node| node.is(w("abstractNum")) && id_matches(node, "abstractNumId", abstract_id))?;
    let levels = || abstract_num.elements().filter(|node| node.is(w("lvl")));
    let lvl = level
        .or_else(|| {
            styles.iter().rev().find_map(|style| {
                let style_id = style.attr(w("styleId"))?;
                levels()
                    .find(|node| val(node, "pStyle") == Some(style_id))?
                    .attr(w("ilvl"))
            })
        })
        .unwrap_or("0")
        .parse::<u32>()
        .ok()?;
    if let Some(override_level) = num
        .elements()
        .find(|node| node.is(w("lvlOverride")) && id_matches(node, "ilvl", lvl))
        .and_then(|node| child(node, "lvl"))
    {
        return child(override_level, "pPr");
    }
    let level = levels().find(|node| id_matches(node, "ilvl", lvl))?;
    child(level, "pPr")
}

struct Cascade<'a> {
    doc: &'a Document,
    theme: Theme<'a>,
    defaults: EffectiveProperties,
    table: EffectiveProperties,
    paragraph: EffectiveProperties,
    direct_ppr: Option<&'a RawNode>,
    paragraph_properties: EffectiveProperties,
}

impl<'a> Cascade<'a> {
    fn new(paragraph: &Paragraph<'a>) -> Result<Self> {
        let doc = paragraph.doc;
        let theme = Theme {
            elements: doc
                .theme_part
                .as_deref()
                .and_then(|name| doc.pkg.parsed_part(name))
                .and_then(|xml| a_child(xml.root(), "themeElements")),
            settings: doc
                .settings_parts
                .first()
                .and_then(|name| doc.pkg.parsed_part(name))
                .map(|xml| xml.root()),
        };
        let defaults_node = doc
            .styles_root()
            .and_then(|root| child(root, "docDefaults"));
        let mut defaults = EffectiveProperties::default();
        let app = Provenance::ApplicationDefault;
        for element in TOGGLES {
            defaults.set(element, "val", "0", app);
        }
        for (element, value) in [
            ("sz", "20"),
            ("szCs", "20"),
            ("color", "auto"),
            ("u", "none"),
            ("vertAlign", "baseline"),
        ] {
            defaults.set(element, "val", value, app);
        }
        for (attribute, _) in FONT_SLOTS {
            defaults.set("rFonts", attribute, "Times New Roman", app);
        }
        properties(
            &mut defaults,
            defaults_node
                .and_then(|d| child(d, "rPrDefault"))
                .and_then(|d| child(d, "rPr")),
            Provenance::DocumentDefaults,
            &theme,
        );
        let mut paragraph_properties = EffectiveProperties::default();
        properties(
            &mut paragraph_properties,
            defaults_node
                .and_then(|d| child(d, "pPrDefault"))
                .and_then(|d| child(d, "pPr")),
            Provenance::DocumentDefaults,
            &theme,
        );

        let mut table_run = EffectiveProperties::default();
        if let Some((table, row, cell)) = paragraph.table_context() {
            let table_styles = chain(
                doc,
                child(table, "tblPr").and_then(|p| val(p, "tblStyle")),
                StyleKind::Table,
            )?;
            let source = Provenance::TableStyle;
            for style in &table_styles {
                properties(&mut table_run, child(style, "rPr"), source, &theme);
                properties(
                    &mut paragraph_properties,
                    child(style, "pPr"),
                    source,
                    &theme,
                );
            }
            for condition in table_conditions(table, row, cell, &table_styles) {
                for style in &table_styles {
                    for rules in style.elements().filter(|node| {
                        node.is(w("tblStylePr")) && node.attr(w("type")) == Some(condition)
                    }) {
                        properties(&mut table_run, child(rules, "rPr"), source, &theme);
                        properties(
                            &mut paragraph_properties,
                            child(rules, "pPr"),
                            source,
                            &theme,
                        );
                    }
                }
            }
        }

        let direct_ppr = child(paragraph.raw(), "pPr");
        let pstyles = chain(
            doc,
            direct_ppr.and_then(|ppr| val(ppr, "pStyle")),
            StyleKind::Paragraph,
        )?;
        let mut paragraph_run = EffectiveProperties::default();
        for style in &pstyles {
            properties(
                &mut paragraph_run,
                child(style, "rPr"),
                Provenance::ParagraphStyle,
                &theme,
            );
            properties(
                &mut paragraph_properties,
                child(style, "pPr"),
                Provenance::ParagraphStyle,
                &theme,
            );
        }
        let num = doc
            .numbering_part
            .as_deref()
            .and_then(|name| doc.pkg.parsed_part(name))
            .map(|xml| xml.root());
        let pprs = pstyles
            .iter()
            .filter_map(|style| child(style, "pPr"))
            .chain(direct_ppr);
        properties(
            &mut paragraph_properties,
            numbering(num, pprs, &pstyles),
            Provenance::Numbering,
            &theme,
        );
        properties(
            &mut paragraph_properties,
            direct_ppr,
            Provenance::Direct,
            &theme,
        );
        Ok(Self {
            doc,
            theme,
            defaults,
            table: table_run,
            paragraph: paragraph_run,
            direct_ppr,
            paragraph_properties,
        })
    }

    fn run(&self, direct: Option<&RawNode>) -> Result<EffectiveRun> {
        let mut character = EffectiveProperties::default();
        for style in chain(
            self.doc,
            direct.and_then(|rpr| val(rpr, "rStyle")),
            StyleKind::Character,
        )? {
            properties(
                &mut character,
                child(style, "rPr"),
                Provenance::CharacterStyle,
                &self.theme,
            );
        }
        let mut direct_properties = EffectiveProperties::default();
        properties(
            &mut direct_properties,
            direct,
            Provenance::Direct,
            &self.theme,
        );
        let mut resolved = self.defaults.clone();
        let layers = [&self.table, &self.paragraph, &character, &direct_properties];
        for layer in layers {
            resolved.overlay(layer.clone());
        }
        for element in TOGGLES {
            let default = self
                .defaults
                .value(element)
                .is_some_and(|value| value.value == "1");
            let mut result = default;
            let mut source = self
                .defaults
                .value(element)
                .expect("every toggle defaults")
                .provenance;
            for layer in layers {
                let Some(value) = layer.value(element) else {
                    continue;
                };
                let on = value.value == "1";
                if default {
                    if !on {
                        result = !result;
                        source = value.provenance;
                    }
                } else if value.provenance == Provenance::CharacterStyle {
                    if on {
                        result = !result;
                        source = value.provenance;
                    }
                } else {
                    result = on;
                    source = value.provenance;
                }
            }
            resolved.set(element, "val", if result { "1" } else { "0" }, source);
        }
        Ok(EffectiveRun {
            properties: resolved,
        })
    }
}

impl Paragraph<'_> {
    /// Resolves paragraph and paragraph-mark formatting without editing the package.
    pub fn effective(&self) -> Result<EffectiveParagraph> {
        let cascade = Cascade::new(self)?;
        let mark = cascade.run(cascade.direct_ppr.and_then(|ppr| child(ppr, "rPr")))?;
        Ok(EffectiveParagraph {
            properties: cascade.paragraph_properties,
            mark,
        })
    }
}

impl Run<'_> {
    /// Resolves run formatting without editing the package.
    pub fn effective(&self) -> Result<EffectiveRun> {
        Cascade::new(&self.paragraph)?.run(child(self.raw(), "rPr"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ST_Cnf lists row/column flags, vertical/horizontal bands, then corners.
    const CONDITIONS: [&str; 12] = [
        "firstRow",
        "lastRow",
        "firstCol",
        "lastCol",
        "band1Vert",
        "band2Vert",
        "band1Horz",
        "band2Horz",
        "nwCell",
        "neCell",
        "swCell",
        "seCell",
    ];
    const CACHE_ATTRIBUTES: [&str; 12] = [
        "firstRow",
        "lastRow",
        "firstColumn",
        "lastColumn",
        "oddVBand",
        "evenVBand",
        "oddHBand",
        "evenHBand",
        "firstRowFirstColumn",
        "firstRowLastColumn",
        "lastRowFirstColumn",
        "lastRowLastColumn",
    ];

    fn conditional_cache(node: &RawNode) -> Option<[bool; 12]> {
        let binary = node.attr(w("val")).filter(|value| {
            value.len() == 12 && value.bytes().all(|byte| matches!(byte, b'0' | b'1'))
        });
        if binary.is_none()
            && !CACHE_ATTRIBUTES
                .iter()
                .any(|&name| node.attr(w(name)).is_some())
        {
            return None;
        }
        Some(std::array::from_fn(|i| {
            node.attr(w(CACHE_ATTRIBUTES[i]))
                .and_then(parse_on_off)
                .unwrap_or_else(|| binary.is_some_and(|mask| mask.as_bytes()[i] == b'1'))
        }))
    }

    #[test]
    #[ignore = "set DOCX_CORPUS to a directory of DOCX inputs"]
    fn compare_cached_formatting() {
        let dir = std::env::var_os("DOCX_CORPUS").expect("DOCX_CORPUS");
        let mut colors = (0, 0);
        let mut adjusted = (0, 0);
        let mut tables = (0, 0);
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if !path.extension().is_some_and(|ext| {
                ["docx", "docm", "dotx", "dotm"]
                    .iter()
                    .any(|suffix| ext.eq_ignore_ascii_case(suffix))
            }) {
                continue;
            }
            let Ok(mut doc) = Document::open(&path) else {
                continue;
            };
            for part in doc.registry_parts(&["styles"]).unwrap() {
                let _ = doc.pkg.xml_part(&part);
            }
            let theme = Theme {
                elements: doc
                    .theme_part
                    .as_deref()
                    .and_then(|name| doc.pkg.parsed_part(name))
                    .and_then(|xml| a_child(xml.root(), "themeElements")),
                settings: doc
                    .settings_parts
                    .first()
                    .and_then(|name| doc.pkg.parsed_part(name))
                    .map(|xml| xml.root()),
            };
            for part in doc.registry_parts(&["styles"]).unwrap() {
                let Some(xml) = doc.pkg.parsed_part(&part) else {
                    continue;
                };
                for color in
                    crate::names::descendants(xml.root()).filter(|node| node.is(w("color")))
                {
                    let (Some(expected), Some(computed)) =
                        (color.attr(w("val")), theme.color(color))
                    else {
                        continue;
                    };
                    let matched = expected.eq_ignore_ascii_case(&computed);
                    colors.0 += 1;
                    colors.1 += usize::from(matched);
                    if color.attr(w("themeShade")).is_some() || color.attr(w("themeTint")).is_some()
                    {
                        adjusted.0 += 1;
                        adjusted.1 += usize::from(matched);
                    }
                }
            }
            for p in doc.stories().flat_map(|story| story.paragraphs()) {
                let Some((table, row, cell)) = p.table_context() else {
                    continue;
                };
                // A paragraph or cell cache describes the full applicable set.
                let cache = child(p.raw(), "pPr")
                    .and_then(|props| child(props, "cnfStyle"))
                    .or_else(|| child(cell, "tcPr").and_then(|props| child(props, "cnfStyle")));
                let Some(cache) = cache.and_then(conditional_cache) else {
                    continue;
                };
                let styles = chain(
                    &doc,
                    child(table, "tblPr").and_then(|pr| val(pr, "tblStyle")),
                    StyleKind::Table,
                )
                .unwrap();
                let computed = table_conditions(table, row, cell, &styles);
                tables.0 += 1;
                tables.1 += usize::from(CONDITIONS.map(|kind| computed.contains(&kind)) == cache);
            }
        }
        println!(
            "theme color calculated/matched={colors:?} shade/tint={adjusted:?} conditional style cache={tables:?}"
        );
    }
}
