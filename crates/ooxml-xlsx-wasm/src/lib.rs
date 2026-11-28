#![cfg(target_arch = "wasm32")]

use ooxml_xlsx::{
    Allow, CellIndex, CellRange, CellRef, CellValue, CellWrite, Error, HeaderFooter, Margins, Orientation,
    PageSetup, SheetProtection, SheetProtectionModel, SheetState, Workbook, Worksheet,
};
use wasm_bindgen::prelude::*;
mod streaming_zip;
mod streaming_archive;
mod streaming_rows;

fn encode_metadata_node(node: &ooxml_core::RawNode, output: &mut Vec<String>) {
    let allowed = [
        "cfRule",
        "formula",
        "extLst",
        "ext",
        "id",
        "sqref",
        "f",
        "cfIcon",
        "fillColor",
        "borderColor",
        "negativeFillColor",
        "negativeBorderColor",
        "axisColor",
        "colorScale",
        "iconSet",
        "dataBar",
        "cfvo",
        "color",
        "font",
        "b",
        "i",
        "strike",
        "u",
        "name",
        "sz",
        "family",
        "charset",
        "scheme",
        "vertAlign",
        "numFmt",
        "fill",
        "patternFill",
        "gradientFill",
        "fgColor",
        "bgColor",
        "stop",
        "border",
        "left",
        "right",
        "top",
        "bottom",
        "diagonal",
        "horizontal",
        "vertical",
        "alignment",
        "protection",
        "tableColumns",
        "tableColumn",
        "tableStyleInfo",
        "autoFilter",
        "filterColumn",
        "calculatedColumnFormula",
        "totalsRowFormula",
    ];
    output.push(node.name().local().to_owned());
    let attributes: Vec<_> = node
        .attrs()
        .iter()
        .filter(|attr| attr.name().ns().is_empty())
        .collect();
    output.push(attributes.len().to_string());
    for attr in attributes {
        output.push(attr.name().local().to_owned());
        output.push(attr.value().to_string());
    }
    output.push(
        if matches!(
            node.name().local(),
            "formula" | "id" | "sqref" | "f" | "calculatedColumnFormula" | "totalsRowFormula"
        ) {
            node.text().into_owned()
        } else {
            String::new()
        },
    );
    let children: Vec<_> = node
        .elements()
        .filter(|child| allowed.contains(&child.name().local()))
        .collect();
    output.push(children.len().to_string());
    for child in children {
        encode_metadata_node(child, output);
    }
}

fn decode_icon_thresholds(fields: &[String]) -> Result<Vec<(ooxml_xlsx::Cfvo, Option<bool>)>, JsError> {
    let invalid = || JsError::new("Invalid conditional formatting icon thresholds");
    let (entries, remainder) = fields.as_chunks::<3>();
    if !remainder.is_empty() {
        return Err(invalid());
    }
    entries.iter().map(|[kind, value, gte]| {
        let inclusion = match gte.as_str() {
            "" => None,
            "true" => Some(true),
            "false" => Some(false),
            _ => return Err(invalid()),
        };
        Ok((conditional_threshold(kind, value)?, inclusion))
    }).collect()
}

fn conditional_threshold(kind: &str, value: &str) -> Result<ooxml_xlsx::Cfvo, JsError> {
    use ooxml_xlsx::Cfvo;

    let number = || {
        value
            .parse()
            .map_err(|_| JsError::new("Invalid conditional threshold value"))
    };
    Ok(match kind {
        "min" => Cfvo::Min,
        "max" => Cfvo::Max,
        "num" => Cfvo::Num(number()?),
        "percent" => Cfvo::Percent(number()?),
        "percentile" => Cfvo::Percentile(number()?),
        "formula" => Cfvo::Formula(value.to_owned()),
        _ => return Err(JsError::new("Unsupported conditional threshold type")),
    })
}

fn decode_sheet_protection_permissions(values: &[u8]) -> Result<Allow, JsError> {
    let &[
        objects,
        scenarios,
        select_locked_cells,
        select_unlocked_cells,
        format_cells,
        format_columns,
        format_rows,
        insert_columns,
        insert_rows,
        insert_hyperlinks,
        delete_columns,
        delete_rows,
        sort,
        auto_filter,
        pivot_tables,
    ] = values
    else {
        return Err(JsError::new(
            "Expected fifteen sheet protection permissions",
        ));
    };
    Ok(Allow {
        objects: objects != 0,
        scenarios: scenarios != 0,
        select_locked_cells: select_locked_cells != 0,
        select_unlocked_cells: select_unlocked_cells != 0,
        format_cells: format_cells != 0,
        format_columns: format_columns != 0,
        format_rows: format_rows != 0,
        insert_columns: insert_columns != 0,
        insert_rows: insert_rows != 0,
        insert_hyperlinks: insert_hyperlinks != 0,
        delete_columns: delete_columns != 0,
        delete_rows: delete_rows != 0,
        sort: sort != 0,
        auto_filter: auto_filter != 0,
        pivot_tables: pivot_tables != 0,
    })
}

fn decode_conditional_style(mut values: &[String]) -> Result<ooxml_xlsx::DxfFormat, JsError> {
    use ooxml_xlsx::{DxfFont, DxfFormat, Protection};

    let mut style = DxfFormat::new();
    let invalid = || JsError::new("Invalid differential style");
    while !values.is_empty() {
        let [kind, length, rest @ ..] = values else {
            return Err(invalid());
        };
        let length: usize = length.parse().map_err(|_| invalid())?;
        let (args, remaining) = rest.split_at_checked(length).ok_or_else(invalid)?;
        match (kind.as_str(), args) {
            ("number_format", [format]) => style = style.set_num_format(format),
            ("font", args) if args.len() == 12 => {
                let mut encoded = args.to_vec();
                if encoded[1].is_empty() {
                    encoded[1] = "11".into();
                }
                let font = decode_font(encoded)?;
                style.font = Some(DxfFont {
                    name: (!font.name.is_empty()).then_some(font.name),
                    size: (!args[1].is_empty()).then_some(font.size),
                    bold: font.bold,
                    italic: font.italic,
                    strike: font.strike,
                    underline: font.underline,
                    color: font.color,
                    vert_align: font.vert_align,
                    scheme: font.scheme,
                    family: font.family,
                    charset: font.charset,
                });
            }
            ("fill", args) => style.fill = Some(decode_fill(args.to_vec())?),
            ("border", args) => style.border = Some(decode_border(args.to_vec())?),
            ("alignment", args) => style.alignment = Some(decode_alignment(args.to_vec())?),
            ("protection", [locked, hidden]) => {
                style.protection = Some(Protection {
                    locked: locked == "true",
                    hidden: hidden == "true",
                })
            }
            _ => return Err(invalid()),
        }
        values = remaining;
    }
    Ok(style)
}

fn encode_style_color(color: Option<ooxml_xlsx::Color>) -> String {
    use ooxml_xlsx::Color;
    match color {
        Some(Color::Rgb(value)) => format!("rgb:{value:08X}"),
        Some(Color::Indexed(value)) => format!("indexed:{value}"),
        Some(Color::Theme { index, tint }) => format!("theme:{index}:{}", tint.get()),
        Some(Color::Auto) => "auto".into(),
        None => String::new(),
    }
}

fn decode_style_color(color: &str) -> Result<Option<ooxml_xlsx::Color>, JsError> {
    use ooxml_xlsx::{Color, Tint};
    let parts: Vec<_> = color.split(':').collect();
    let color = match parts.as_slice() {
        [""] => return Ok(None),
        ["auto"] => Some(Color::Auto),
        ["rgb", value] => u32::from_str_radix(value, 16).ok().map(Color::Rgb),
        ["indexed", value] => value.parse().ok().map(Color::Indexed),
        ["theme", value, tint] => value
            .parse()
            .ok()
            .zip(tint.parse().ok().and_then(Tint::new))
            .map(|(index, tint)| Color::Theme { index, tint }),
        _ => None,
    };
    color
        .map(Some)
        .ok_or_else(|| JsError::new("Invalid style color"))
}

fn encode_font(font: ooxml_xlsx::Font) -> Vec<String> {
    use ooxml_xlsx::{Color, Underline, VertAlign};
    let underline = match font.underline {
        None => "",
        Some(Underline::Single) => "single",
        Some(Underline::Double) => "double",
        Some(Underline::SingleAccounting) => "singleAccounting",
        Some(Underline::DoubleAccounting) => "doubleAccounting",
    };
    let vertical = match font.vert_align {
        None => "",
        Some(VertAlign::Baseline) => "baseline",
        Some(VertAlign::Superscript) => "superscript",
        Some(VertAlign::Subscript) => "subscript",
    };
    let (color, tint) = match font.color {
        Some(Color::Rgb(value)) => (format!("rgb:{value:08X}"), String::new()),
        Some(Color::Indexed(value)) => (format!("indexed:{value}"), String::new()),
        Some(Color::Theme { index, tint }) => (format!("theme:{index}"), tint.get().to_string()),
        _ => (String::new(), String::new()),
    };
    vec![
        font.name,
        font.size.to_string(),
        font.bold.to_string(),
        font.italic.to_string(),
        font.strike.to_string(),
        underline.into(),
        vertical.into(),
        font.scheme.unwrap_or_default(),
        font.family.map(|v| v.to_string()).unwrap_or_default(),
        font.charset.map(|v| v.to_string()).unwrap_or_default(),
        color,
        tint,
    ]
}

/// [`encode_font`] followed by the declared-field bits, so the adapter can
/// omit a name or size the font element did not write.
fn encode_declared_font(font: ooxml_xlsx::DeclaredFont) -> Vec<String> {
    let mut values = encode_font(font.font);
    values.push(font.fields.to_string());
    values
}

fn decode_font(values: Vec<String>) -> Result<ooxml_xlsx::Font, JsError> {
    use ooxml_xlsx::{Color, Font, FontSize, Tint, Underline, VertAlign};
    let [
        face,
        size,
        bold,
        italic,
        strike,
        underline,
        vertical,
        scheme,
        family,
        charset,
        color,
        tint,
    ] = <[String; 12]>::try_from(values)
        .map_err(|_| JsError::new("Expected twelve font entries"))?;
    let color = match color.split_once(':') {
        Some(("rgb", value)) => u32::from_str_radix(value, 16).ok().map(Color::Rgb),
        Some(("indexed", value)) => value.parse().ok().map(Color::Indexed),
        Some(("theme", value)) => value.parse().ok().map(|index| Color::Theme {
            index,
            tint: tint.parse().ok().and_then(Tint::new).unwrap_or(Tint::NONE),
        }),
        Some(("auto", _)) => Some(Color::Auto),
        _ => None,
    };
    Ok(Font {
        name: face,
        size: size
            .parse()
            .ok()
            .and_then(FontSize::new)
            .ok_or_else(|| JsError::new("Invalid font size"))?,
        bold: bold == "true",
        italic: italic == "true",
        strike: strike == "true",
        underline: match underline.as_str() {
            "single" => Some(Underline::Single),
            "double" => Some(Underline::Double),
            "singleAccounting" => Some(Underline::SingleAccounting),
            "doubleAccounting" => Some(Underline::DoubleAccounting),
            _ => None,
        },
        vert_align: match vertical.as_str() {
            "baseline" => Some(VertAlign::Baseline),
            "superscript" => Some(VertAlign::Superscript),
            "subscript" => Some(VertAlign::Subscript),
            _ => None,
        },
        scheme: (!scheme.is_empty()).then_some(scheme),
        family: family.parse().ok(),
        charset: charset.parse().ok(),
        color,
        ..Font::default()
    })
}

fn decode_alignment(values: Vec<String>) -> Result<ooxml_xlsx::Alignment, JsError> {
    let [
        horizontal,
        vertical,
        rotation,
        wrap,
        indent,
        shrink,
        reading_order,
    ] = <[String; 7]>::try_from(values)
        .map_err(|_| JsError::new("Expected seven alignment entries"))?;
    let text = |value: String| (!value.is_empty()).then_some(value);
    Ok(ooxml_xlsx::Alignment {
        horizontal: text(horizontal),
        vertical: text(vertical),
        rotation: rotation.parse().ok(),
        wrap: wrap == "true",
        indent: indent.parse().ok(),
        shrink: shrink == "true",
        reading_order: reading_order.parse().ok(),
    })
}

fn encode_fill(fill: ooxml_xlsx::Fill) -> Vec<String> {
    use ooxml_xlsx::Fill;
    match fill {
        Fill::NoFill => vec!["none".into(), String::new(), String::new()],
        Fill::Pattern { pattern, fg, bg } => {
            vec![pattern, encode_style_color(fg), encode_style_color(bg)]
        }
        Fill::Gradient { geometry, stops } => {
            let mut values = match geometry {
                ooxml_xlsx::Gradient::Angle(degree) => vec!["angle".into(), degree.to_string()],
                ooxml_xlsx::Gradient::Path {
                    left,
                    top,
                    right,
                    bottom,
                } => vec![
                    "path".into(),
                    left.get().to_string(),
                    top.get().to_string(),
                    right.get().to_string(),
                    bottom.get().to_string(),
                ],
            };
            values.extend(stops.into_iter().flat_map(|(position, color)| {
                [position.get().to_string(), encode_style_color(Some(color))]
            }));
            values
        }
    }
}

fn decode_fill(values: Vec<String>) -> Result<ooxml_xlsx::Fill, JsError> {
    use ooxml_xlsx::Fill;
    use ooxml_xlsx::{Gradient, Tint};
    let invalid = || JsError::new("Invalid gradient fill");
    let fraction = |value: &str| value.parse().ok().and_then(Tint::new).ok_or_else(invalid);
    let geometry = match values.as_slice() {
        [kind, degree, stops @ ..] if kind == "angle" => Some((
            Gradient::Angle(degree.parse().map_err(|_| invalid())?),
            stops,
        )),
        [kind, left, top, right, bottom, stops @ ..] if kind == "path" => Some((
            Gradient::Path {
                left: fraction(left)?,
                top: fraction(top)?,
                right: fraction(right)?,
                bottom: fraction(bottom)?,
            },
            stops,
        )),
        _ => None,
    };
    if let Some((geometry, stops)) = geometry {
        let (chunks, remainder) = stops.as_chunks::<2>();
        let stops = chunks
            .iter()
            .map(|pair| {
                Ok((
                    fraction(&pair[0])?,
                    decode_style_color(&pair[1])?.ok_or_else(invalid)?,
                ))
            })
            .collect::<Result<Vec<_>, JsError>>()?;
        if !remainder.is_empty() {
            return Err(invalid());
        }
        return Ok(Fill::Gradient { geometry, stops });
    }
    let [pattern, foreground, background] = <[String; 3]>::try_from(values)
        .map_err(|_| JsError::new("Expected a pattern and two fill colors"))?;
    let fg = decode_style_color(&foreground)?;
    let bg = decode_style_color(&background)?;
    let fill = if pattern == "none" && fg.is_none() && bg.is_none() {
        Fill::NoFill
    } else {
        Fill::Pattern { pattern, fg, bg }
    };
    Ok(fill)
}

fn encode_border(border: ooxml_xlsx::Border) -> Vec<String> {
    let mut values = vec![
        border.diagonal_up.to_string(),
        border.diagonal_down.to_string(),
    ];
    for side in [
        border.top,
        border.left,
        border.bottom,
        border.right,
        border.diagonal,
    ] {
        values.extend(match side {
            Some(side) => [side.style, encode_style_color(side.color)],
            None => [String::new(), String::new()],
        });
    }
    values
}

fn decode_border(values: Vec<String>) -> Result<ooxml_xlsx::Border, JsError> {
    use ooxml_xlsx::{Border, Side};
    if values.len() != 12 {
        return Err(JsError::new(
            "Expected five border sides and two diagonal flags",
        ));
    }
    let side = |index: usize| -> Result<Option<Side>, JsError> {
        let style = &values[index];
        if style.is_empty() {
            return Ok(None);
        }
        Ok(Some(Side {
            style: style.clone(),
            color: decode_style_color(&values[index + 1])?,
        }))
    };
    Ok(Border {
        top: side(2)?,
        left: side(4)?,
        bottom: side(6)?,
        right: side(8)?,
        diagonal: side(10)?,
        diagonal_up: values[0] == "true",
        diagonal_down: values[1] == "true",
    })
}

#[wasm_bindgen]
#[derive(Default)]
pub struct XlsxSparseCells {
    coordinates: Vec<u32>,
    values: Vec<JsValue>,
    format_styles: Vec<u32>,
    fill_styles: Vec<u32>,
    special_values: Vec<JsValue>,
    formulas: Vec<JsValue>,
    shared_strings: Vec<u32>,
    extent: Vec<u32>,
}

#[wasm_bindgen]
impl XlsxSparseCells {
    /// The extent of the projected data as zero-based `[top, left, bottom,
    /// right]`, or empty when no cell was projected: columns from the
    /// projected cells; rows from every `row` element read (with or without
    /// cells), or only from the projected cells when the dimension is ignored.
    pub fn take_extent(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.extent)
    }

    pub fn take_coordinates(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.coordinates)
    }

    pub fn take_format_styles(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.format_styles)
    }

    pub fn take_fill_styles(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.fill_styles)
    }

    /// Sparse tuples of cell index, ISO-date flag and stored text.
    pub fn take_special_values(&mut self) -> Vec<JsValue> {
        std::mem::take(&mut self.special_values)
    }

    /// Sparse tuples of cell index, formula text or array index, scalar, error and ISO-date result.
    pub fn take_formulas(&mut self) -> Vec<JsValue> {
        std::mem::take(&mut self.formulas)
    }

    pub fn take_values(&mut self) -> Vec<JsValue> {
        std::mem::take(&mut self.values)
    }

    /// Pairs of cell index and shared-string index for every `t="s"` cell.
    pub fn take_shared_strings(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.shared_strings)
    }
}

#[wasm_bindgen]
pub struct XlsxDocument {
    workbook: Workbook,
    scalar_read_cache: ScalarReadCache,
}

#[derive(Default)]
struct ScalarReadCache {
    generation: u64,
    sheets: std::collections::HashMap<String, (std::sync::Arc<Vec<CellRange>>, usize)>,
}

#[wasm_bindgen]
impl XlsxDocument {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<Self, JsError> {
        Ok(Self {
            workbook: Workbook::from_bytes(bytes).map_err(js_error)?,
            scalar_read_cache: ScalarReadCache::default(),
        })
    }

    pub fn open_exceljs(bytes: &[u8]) -> Result<Self, JsError> {
        let workbook = Workbook::from_bytes_allow_empty(bytes.to_vec()).map_err(js_error)?;
        Ok(Self { workbook, scalar_read_cache: ScalarReadCache::default() })
    }

    /// Creates a candidate that can be edited without reopening the workbook.
    pub fn fork(&self) -> Self {
        Self {
            workbook: self.workbook.fork(),
            scalar_read_cache: ScalarReadCache::default(),
        }
    }

    pub fn create(
        epoch_seconds: u32,
        preserve_formula_results: Option<bool>,
    ) -> Result<XlsxDocument, JsError> {
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(epoch_seconds.into());
        let mut workbook = Workbook::new_at(time).map_err(js_error)?;
        if preserve_formula_results.unwrap_or(false) {
            workbook.set_calc_policy(ooxml_xlsx::CalcPolicy::KeepStaleCaches);
        }
        Ok(Self { workbook, scalar_read_cache: ScalarReadCache::default() })
    }

    pub fn create_empty(epoch_seconds: u32) -> Result<Self, JsError> {
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(epoch_seconds.into());
        let workbook = Workbook::new_empty_at(time).map_err(js_error)?;
        Ok(Self { workbook, scalar_read_cache: ScalarReadCache::default() })
    }

    pub fn prune_empty_template_shared_strings(&mut self) -> Result<(), JsError> {
        self.workbook
            .prune_empty_template_shared_strings()
            .map(drop)
            .map_err(js_error)
    }

    pub fn zip_entry_names(&self) -> Vec<String> {
        self.workbook
            .package()
            .part_names()
            .map(str::to_owned)
            .collect()
    }

    pub fn zip_entry_bytes(&self, name: &str) -> Result<Vec<u8>, JsError> {
        self.workbook
            .package()
            .read_part(name)
            .map_err(|error| js_error(error.into()))
    }

    pub fn zip_entry_modified(&self, name: &str) -> Option<Vec<u16>> {
        self.workbook.package().part_modified(name).map(Vec::from)
    }

    pub fn calculation_chain(&self) -> Result<Option<Vec<String>>, JsError> {
        use ooxml_core::{QName, XmlDocument};
        const CONTENT_TYPE: &str =
            "application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml";
        const SML: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        let package = self.workbook.package();
        let Some((path, _)) = package
            .content_types()
            .overrides()
            .iter()
            .find(|(_, content_type)| content_type == CONTENT_TYPE)
        else {
            return Ok(None);
        };
        let bytes = package
            .read_part(path.trim_start_matches('/'))
            .map_err(|error| js_error(error.into()))?;
        let chain = XmlDocument::parse(&bytes).map_err(|error| js_error(error.into()))?;
        let mut fields = Vec::new();
        for cell in chain
            .root()
            .elements()
            .filter(|node| node.is(QName::new(SML, "c")))
        {
            fields.extend([String::new(), String::new()]);
            for attr in cell
                .attrs()
                .iter()
                .filter(|attr| attr.name().ns().is_empty())
            {
                fields.extend([attr.name().local().to_owned(), attr.value().to_owned()]);
            }
        }
        Ok(Some(fields))
    }

    pub fn xml_workbook_type(&self) -> Option<String> {
        self.workbook.xml_workbook_type().map(str::to_owned)
    }

    pub fn set_template_workbook_type(&mut self, format: &str) -> Result<(), JsError> {
        self.workbook
            .set_template_workbook_type(format)
            .map_err(js_error)
    }

    pub fn vba_project(&self) -> Result<Option<Vec<u8>>, JsError> {
        package_vba_project(self.workbook.package()).map_err(|error| js_error(error.into()))
    }

    pub fn document_properties(&mut self) -> Vec<String> {
        self.workbook
            .document_properties()
            .into_iter()
            .flat_map(|(section, key, kind, value)| [section, key, kind, value])
            .collect()
    }

    pub fn add_custom_properties(&mut self, fields: Vec<String>) -> Result<(), JsError> {
        self.workbook
            .add_custom_properties(&custom_property_entries(&fields)?)
            .map_err(js_error)
    }

    pub fn core_text_property(&mut self, key: &str) -> Result<Option<String>, JsError> {
        if matches!(key, "company" | "manager") {
            return self.workbook.app_text_property(key).map_err(js_error);
        }
        self.workbook.core_text_property(key).map_err(js_error)
    }

    pub fn set_core_text_property(&mut self, key: &str, value: &str) -> Result<(), JsError> {
        if matches!(key, "company" | "manager") {
            return self
                .workbook
                .set_app_text_property(key, value)
                .map_err(js_error);
        }
        self.workbook
            .set_core_text_property(key, value)
            .map_err(js_error)
    }

    pub fn defined_names(&mut self) -> Result<Vec<String>, JsError> {
        use ooxml_xlsx::NameScope;
        let mut values = Vec::new();
        for name in self.workbook.defined_names().map_err(js_error)? {
            let scope = match name.scope {
                NameScope::Workbook => String::new(),
                NameScope::Sheet(id) => id.to_string(),
                NameScope::Invalid(raw) => raw,
            };
            values.extend([name.name, name.formula, scope, name.comment.unwrap_or_default()]);
        }
        Ok(values)
    }

    pub fn global_defined_names(&mut self) -> Result<Vec<String>, JsError> {
        Ok(self
            .workbook
            .global_defined_names()
            .map_err(js_error)?
            .into_iter()
            .flat_map(|(name, formula)| [name, formula])
            .collect())
    }

    pub fn global_defined_name_formulas(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        self.workbook
            .global_defined_name_formulas(name)
            .map_err(js_error)
    }

    pub fn set_global_defined_name(&mut self, name: &str, formula: &str) -> Result<(), JsError> {
        self.workbook
            .set_global_defined_name(name, formula)
            .map_err(js_error)
    }

    pub fn set_global_defined_names(&mut self, fields: Vec<String>) -> Result<(), JsError> {
        let (pairs, trailing) = fields.as_chunks::<2>();
        if !trailing.is_empty() {
            return Err(JsError::new("Expected defined name and formula pairs"));
        }
        self.workbook.set_global_defined_names(pairs.iter().map(|[name, formula]|
            (name.as_str(), formula.as_str()))).map_err(js_error)
    }

    pub fn set_defined_names(&mut self, fields: Vec<String>, scopes: &[u32], comments: Vec<JsValue>) -> Result<(), JsError> {
        let (pairs, trailing) = fields.as_chunks::<2>();
        if !trailing.is_empty() || pairs.len() != scopes.len() || pairs.len() != comments.len() {
            return Err(JsError::new("Expected one scope and optional comment per defined name"));
        }
        let comments = comments.iter().map(|comment| {
            if comment.is_undefined() {
                Ok(None)
            } else {
                comment.as_string().map(Some).ok_or_else(|| JsError::new("Expected a defined name comment"))
            }
        }).collect::<Result<Vec<_>, _>>()?;
        let edits = pairs.iter().zip(scopes).zip(&comments).map(|(([name, formula], &scope), comment)| {
            (name.as_str(), formula.as_str(), scope.checked_sub(1), comment.as_deref())
        });
        self.workbook.set_defined_names(edits).map_err(js_error)
    }

    pub fn set_theme_xml(&mut self, xml: &str) -> Result<(), JsError> {
        self.workbook.set_theme_xml(xml).map_err(js_error)
    }

    pub fn reset_theme(&mut self) -> Result<(), JsError> {
        self.workbook.reset_theme().map_err(js_error)
    }

    pub fn workbook_views(&mut self) -> Result<Vec<String>, JsError> {
        Ok(self
            .workbook
            .workbook_views()
            .map_err(js_error)?
            .into_iter()
            .flatten()
            .collect())
    }

    pub fn set_workbook_views(&mut self, values: Vec<String>) -> Result<(), JsError> {
        let (views, remainder) = values.as_chunks::<7>();
        if !remainder.is_empty() {
            return Err(JsError::new("Expected seven fields per workbook view"));
        }
        self.workbook.set_workbook_views(views).map_err(js_error)
    }

    pub fn full_calc_on_load(&self) -> bool {
        self.workbook.full_calc_on_load()
    }

    pub fn set_full_calc_on_load(&mut self, enabled: bool) -> Result<(), JsError> {
        self.workbook
            .set_full_calc_on_load(enabled)
            .map_err(js_error)
    }

    pub fn insert_rows(&mut self, name: &str, at: u32, count: u32) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .insert_rows(at, count)
            .map(drop)
            .map_err(js_error)
    }

    pub fn delete_rows(&mut self, name: &str, at: u32, count: u32) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .delete_rows(at, count)
            .map(drop)
            .map_err(js_error)
    }

    pub fn insert_columns(&mut self, name: &str, at: u32, count: u32) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .insert_cols(at, count)
            .map(drop)
            .map_err(js_error)
    }

    pub fn delete_columns(&mut self, name: &str, at: u32, count: u32) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .delete_cols(at, count)
            .map(drop)
            .map_err(js_error)
    }

    pub fn append_worksheet(&mut self, name: &str, id: Option<u32>) -> Result<(), JsError> {
        let position = self.workbook.sheet_names().count();
        self.insert_worksheet(name, id, position)
    }

    pub fn insert_worksheet(&mut self, name: &str, id: Option<u32>, position: usize) -> Result<(), JsError> {
        self.workbook
            .add_sheet_with_id(name, position, id)
            .map_err(js_error)
    }

    pub fn remove_worksheet(&mut self, name: &str) -> Result<(), JsError> {
        self.workbook.delete_sheet(name).map(drop).map_err(js_error)
    }

    pub fn remove_exceljs_worksheet(&mut self, name: &str) -> Result<(), JsError> {
        self.workbook.delete_sheet_allow_empty(name).map(drop).map_err(js_error)
    }

    pub fn rename_worksheet(&mut self, old: &str, new: &str) -> Result<(), JsError> {
        self.workbook
            .rename_sheet(old, new)
            .map(drop)
            .map_err(js_error)
    }

    pub fn sheet_visibility(&mut self) -> Result<Vec<u8>, JsError> {
        self.workbook.sheet_visibility().map_err(js_error)
    }

    pub fn sheet_state(&mut self, name: &str) -> Result<String, JsError> {
        let state = self.workbook.sheet_state(name).map_err(js_error)?;
        Ok(match state {
            SheetState::Visible => "visible",
            SheetState::Hidden => "hidden",
            SheetState::VeryHidden => "veryHidden",
        }
        .into())
    }

    pub fn set_sheet_state(&mut self, name: &str, state: &str) -> Result<(), JsError> {
        let state = match state {
            "visible" => SheetState::Visible,
            "hidden" => SheetState::Hidden,
            "veryHidden" => SheetState::VeryHidden,
            _ => return Err(JsError::new("Invalid worksheet state")),
        };
        self.workbook.set_sheet_state(name, state).map_err(js_error)
    }

    pub fn rename_initial_worksheet(&mut self, name: &str) -> Result<(), JsError> {
        if name != "Sheet1" {
            self.rename_worksheet("Sheet1", name)?;
        }
        Ok(())
    }

    pub fn check_grid_limits(
        &mut self,
        name: &str,
        max_rows: usize,
        max_cols: usize,
    ) -> Result<(), JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        if max_rows != 0 && sheet.row_numbers().map_err(js_error)?.count() > max_rows {
            return Err(JsError::new(&format!(
                "Max row count ({max_rows}) exceeded"
            )));
        }
        if max_cols != 0 {
            let mut row = None;
            let mut count = 0;
            for cell in sheet.cells() {
                let cell = cell.map_err(js_error)?;
                let at = cell.index();
                if row != Some(at.row) {
                    row = Some(at.row);
                    count = 0;
                }
                count += 1;
                if count > max_cols {
                    return Err(JsError::new(&format!(
                        "Max column count ({max_cols}) exceeded"
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn sheet_names(&self) -> Vec<String> {
        self.workbook.sheet_names().map(str::to_owned).collect()
    }

    pub fn worksheet_names(&self) -> Vec<String> {
        self.workbook.worksheet_names().map(str::to_owned).collect()
    }

    pub fn worksheet_ids(&self) -> Vec<JsValue> {
        self.workbook
            .worksheet_identities()
            .map(|(_, id)| id.map_or(JsValue::UNDEFINED, |id| JsValue::from_f64(f64::from(id))))
            .collect()
    }

    pub fn merged_ranges(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .merged_ranges()
            .iter()
            .map(ToString::to_string)
            .collect())
    }

    /// First row-major offset of a merged cell at or after the cursor.
    pub fn first_merged_overlap(
        &mut self,
        name: &str,
        reference: &str,
        row: u32,
        col: u32,
    ) -> Result<Option<f64>, JsError> {
        let range = CellRange::parse(reference).map_err(js_error)?;
        let cursor = CellIndex::new(row, col).map_err(js_error)?;
        if !range.contains(cursor) {
            return Err(JsError::new("Merge overlap cursor is outside the range"));
        }
        let width = f64::from(range.last.col - range.first.col + 1);
        Ok(self.worksheet(name).map_err(js_error)?.merged_ranges().into_iter()
            .filter_map(|merged| {
                let first_col = range.first.col.max(merged.first.col);
                let last_col = range.last.col.min(merged.last.col);
                if first_col > last_col {
                    return None;
                }
                let mut row = cursor.row.max(merged.first.row);
                let mut col = first_col;
                if row == cursor.row {
                    col = col.max(cursor.col);
                    if col > last_col {
                        row += 1;
                        col = first_col;
                    }
                }
                (row <= range.last.row.min(merged.last.row)).then_some(CellIndex { row, col })
            })
            .min_by_key(|at| (at.row, at.col))
            .map(|at| f64::from(at.row - range.first.row) * width + f64::from(at.col - range.first.col)))
    }

    /// Infers a rectangle per master from packed
    /// [master row, master column, follower row, follower column] records.
    /// Worksheet model edits require upper-left masters by default.
    pub fn infer_merge_ranges(
        links: &[u32],
        allow_non_corner_master: Option<bool>,
    ) -> Result<Vec<String>, JsError> {
        let allow_non_corner_master = allow_non_corner_master.unwrap_or(false);
        let (links, remainder) = links.as_chunks::<4>();
        if !remainder.is_empty() {
            return Err(JsError::new("Expected four coordinates per merge follower"));
        }
        let mut slots = std::collections::HashMap::new();
        let mut ranges: Vec<CellRange> = Vec::new();
        for &[master_row, master_col, row, col] in links {
            let master = CellIndex::new(master_row, master_col).map_err(js_error)?;
            let follower = CellIndex::new(row, col).map_err(js_error)?;
            if follower == master {
                return Err(JsError::new("Merge follower must differ from the master"));
            }
            if !allow_non_corner_master && (row < master_row || col < master_col) {
                return Err(JsError::new("Merge master must be the upper-left cell of the inferred range"));
            }
            let slot = slots.entry(master).or_insert_with(|| {
                ranges.push(CellRange { first: master, last: master });
                ranges.len() - 1
            });
            ranges[*slot] = ranges[*slot].union(follower);
        }
        Ok(ranges.into_iter().map(|range| range.to_string()).collect())
    }

    pub fn merge_range(&mut self, name: &str, range: &str) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .merge(range)
            .map_err(js_error)
    }

    pub fn merge_preserving_covered_values(&mut self, name: &str, range: &str) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .merge_preserving_covered_values(range)
            .map_err(js_error)
    }

    pub fn merge_clearing(
        &mut self,
        name: &str,
        range: &str,
        copy_style: Option<bool>,
    ) -> Result<(), JsError> {
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        sheet.merge_clearing(range).map_err(js_error)?;
        if copy_style.unwrap_or(false) {
            let range = CellRange::parse(range).map_err(js_error)?;
            sheet.copy_merge_style(range, range.first, &Default::default()).map_err(js_error)?;
        }
        Ok(())
    }

    pub fn sync_merge_style(
        &mut self,
        name: &str,
        reference: &str,
        row: u32,
        column: u32,
        detached: Vec<String>,
    ) -> Result<(), JsError> {
        let range = CellRange::parse(reference).map_err(js_error)?;
        let source = CellIndex::new(row, column).map_err(js_error)?;
        let detached = detached.iter().map(|key| CellIndex::parse(key))
            .collect::<Result<std::collections::HashSet<_>, _>>().map_err(js_error)?;
        self.worksheet(name).map_err(js_error)?.copy_merge_style(range, source, &detached).map_err(js_error)
    }

    pub fn unmerge_intersecting(&mut self, name: &str, reference: &str) -> Result<(), JsError> {
        let range = CellRange::parse(reference).map_err(js_error)?;
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        sheet.reset_merge_follower_styles(range).map_err(js_error)?;
        sheet.unmerge_intersecting(range).map_err(js_error)
    }

    pub fn unmerge_range(&mut self, name: &str, range: &str) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .unmerge(range)
            .map_err(js_error)
    }

    pub fn autofilter(&mut self, name: &str) -> Result<Option<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .autofilter_range()
            .map(str::to_owned))
    }

    pub fn set_autofilter(&mut self, name: &str, range: Option<String>) -> Result<(), JsError> {
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        if let Some(range) = range {
            sheet.set_autofilter(&range).map_err(js_error)
        } else {
            sheet.remove_autofilter().map(drop).map_err(js_error)
        }
    }

    pub fn set_ignore_numbers_stored_as_text(&mut self, enabled: bool) -> Result<(), JsError> {
        for name in self.sheet_names() {
            match self.workbook.worksheet_by_name(&name) {
                Ok(mut worksheet) => worksheet
                    .set_ignore_numbers_stored_as_text(enabled)
                    .map_err(js_error)?,
                Err(Error::NotAWorksheet(_)) => {}
                Err(error) => return Err(js_error(error)),
            }
        }
        Ok(())
    }

    pub fn sheet_range(&mut self, name: &str) -> Result<Option<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .declared_bound()
            .map(|range| range.to_string()))
    }

    /// The raw declared `dimension/@ref` when it parses (any form,
    /// including a lone cell): presence tells reads apart a missing
    /// dimension (fail closed) from a single-cell one (data fallback).
    pub fn sheet_declared_range(&mut self, name: &str) -> Result<Option<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .declared_range()
            .map(|range| range.to_string()))
    }

    pub fn guarded_ranges(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .non_scalar_ranges()
            .map_err(js_error)?
            .iter()
            .map(ToString::to_string)
            .collect())
    }

    /// Array-formula and data-table result ranges, whose cells do not hold
    /// their own scalar values (merged ranges are excluded).
    pub fn formula_result_ranges(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .formula_result_ranges()
            .map_err(js_error)?
            .iter()
            .map(ToString::to_string)
            .collect())
    }

    pub fn explicit_margins(&mut self, name: &str) -> Result<Vec<f64>, JsError> {
        if !self
            .worksheet(name)
            .map_err(js_error)?
            .has_page_margins()
            .map_err(js_error)?
        {
            return Ok(Vec::new());
        }
        self.margins(name)
    }

    pub fn margins(&mut self, name: &str) -> Result<Vec<f64>, JsError> {
        let m = self
            .worksheet(name)
            .map_err(js_error)?
            .margins()
            .map_err(js_error)?;
        Ok(vec![m.left, m.right, m.top, m.bottom, m.header, m.footer])
    }

    pub fn set_margins(&mut self, name: &str, values: &[f64]) -> Result<(), JsError> {
        let &[left, right, top, bottom, header, footer] = values else {
            return Err(JsError::new("Expected six worksheet margins"));
        };
        self.worksheet(name)
            .map_err(js_error)?
            .set_margins(&Margins {
                left,
                right,
                top,
                bottom,
                header,
                footer,
            })
            .map_err(js_error)
    }

    pub fn page_setup(&mut self, name: &str) -> Result<Vec<f64>, JsError> {
        let settings = self
            .worksheet(name)
            .map_err(js_error)?
            .page_setup()
            .map_err(js_error)?;
        Ok(vec![
            f64::from(u8::from(
                settings.orientation == Some(Orientation::Landscape),
            )),
            settings.paper_size.map_or(f64::NAN, f64::from),
            f64::from(settings.scale.unwrap_or(100)),
            f64::from(settings.fit_to_width.unwrap_or(1)),
            f64::from(settings.fit_to_height.unwrap_or(1)),
            f64::from(u8::from(settings.fit_to_page.unwrap_or(false))),
        ])
    }

    pub fn set_page_setup(&mut self, name: &str, values: &[f64]) -> Result<(), JsError> {
        let &[
            orientation,
            paper_size,
            scale,
            fit_to_width,
            fit_to_height,
            fit_to_page,
        ] = values
        else {
            return Err(JsError::new("Expected six page setup values"));
        };
        let number = |value: f64| value.is_finite().then_some(value as u32);
        self.worksheet(name)
            .map_err(js_error)?
            .set_page_setup(&PageSetup {
                orientation: number(orientation).map(|value| {
                    if value == 1 {
                        Orientation::Landscape
                    } else {
                        Orientation::Portrait
                    }
                }),
                paper_size: number(paper_size),
                scale: number(scale),
                fit_to_width: number(fit_to_width),
                fit_to_height: number(fit_to_height),
                fit_to_page: number(fit_to_page).map(|value| value != 0),
            })
            .map_err(js_error)
    }

    pub fn printer_settings(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        let settings = self
            .worksheet(name)
            .map_err(js_error)?
            .printer_settings()
            .map_err(js_error)?;
        Ok(settings
            .into_iter()
            .flat_map(|(key, value)| [key, value])
            .collect())
    }

    pub fn set_printer_settings(&mut self, name: &str, values: Vec<String>) -> Result<(), JsError> {
        let mut pairs = values.into_iter();
        let mut settings = Vec::new();
        while let Some(key) = pairs.next() {
            let value = pairs
                .next()
                .ok_or_else(|| JsError::new("Expected printer setting pairs"))?;
            settings.push((key, value));
        }
        self.worksheet(name)
            .map_err(js_error)?
            .set_printer_settings(&settings)
            .map_err(js_error)
    }

    pub fn tab_color(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        let color = self
            .worksheet(name)
            .map_err(js_error)?
            .tab_color()
            .map_err(js_error)?;
        Ok(color
            .into_iter()
            .flat_map(|(key, value)| [key, value])
            .collect())
    }

    pub fn set_tab_color(&mut self, name: &str, values: Vec<String>) -> Result<(), JsError> {
        let (chunks, remainder) = values.as_chunks::<2>();
        let attributes: Vec<_> = chunks
            .iter()
            .map(|pair| (pair[0].as_str(), pair[1].clone()))
            .collect();
        if !remainder.is_empty() {
            return Err(JsError::new("Expected tab color attribute pairs"));
        }
        self.workbook
            .set_tab_color_attributes(name, &attributes)
            .map_err(js_error)
    }

    pub fn sheet_protection_attributes(
        &mut self,
        name: &str,
    ) -> Result<Option<Vec<String>>, JsError> {
        let attributes = self
            .worksheet(name)
            .map_err(js_error)?
            .sheet_protection_attributes()
            .map_err(js_error)?;
        Ok(attributes.map(|pairs| {
            pairs
                .into_iter()
                .flat_map(|(key, value)| [key, value])
                .collect()
        }))
    }

    pub fn protect_sheet(
        &mut self,
        name: &str,
        password: &str,
        spin_count: u32,
        values: &[u8],
    ) -> Result<(), JsError> {
        let allow = decode_sheet_protection_permissions(values)?;
        let protection = SheetProtection::new()
            .password(password)
            .with_spin_count(spin_count)
            .allow(allow);
        self.worksheet(name)
            .map_err(js_error)?
            .protect_sheet(&protection)
            .map_err(js_error)
    }

    #[expect(clippy::too_many_arguments, reason = "Wasm ABI passes stored hash fields independently")]
    pub fn replace_sheet_protection(
        &mut self,
        name: &str,
        sheet: bool,
        permissions: &[u8],
        algorithm_name: Option<String>,
        hash_value: Option<String>,
        salt_value: Option<String>,
        spin_count: Option<u32>,
    ) -> Result<(), JsError> {
        let model = SheetProtectionModel {
            sheet,
            allow: decode_sheet_protection_permissions(permissions)?,
            algorithm_name: algorithm_name.as_deref(),
            hash_value: hash_value.as_deref(),
            salt_value: salt_value.as_deref(),
            spin_count,
        };
        self.worksheet(name)
            .map_err(js_error)?
            .replace_sheet_protection(&model)
            .map_err(js_error)
    }

    pub fn unprotect_sheet(&mut self, name: &str) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .unprotect_sheet()
            .map(drop)
            .map_err(js_error)
    }

    pub fn header_footer(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        let hf = self
            .worksheet(name)
            .map_err(js_error)?
            .header_footer()
            .map_err(js_error)?;
        Ok(vec![
            hf.odd_header.unwrap_or_default(),
            hf.odd_footer.unwrap_or_default(),
            hf.even_header.unwrap_or_default(),
            hf.even_footer.unwrap_or_default(),
            hf.first_header.unwrap_or_default(),
            hf.first_footer.unwrap_or_default(),
            hf.different_odd_even.to_string(),
            hf.different_first.to_string(),
        ])
    }

    pub fn set_header_footer(
        &mut self,
        name: &str,
        values: Vec<String>,
        changed: u8,
    ) -> Result<(), JsError> {
        let [
            odd_header,
            odd_footer,
            even_header,
            even_footer,
            first_header,
            first_footer,
            different_odd_even,
            different_first,
        ] = <[String; 8]>::try_from(values)
            .map_err(|_| JsError::new("Expected eight header and footer entries"))?;
        let text = |value: String| (!value.is_empty()).then_some(value);
        self.worksheet(name)
            .map_err(js_error)?
            .update_header_footer(
                &HeaderFooter {
                    odd_header: text(odd_header),
                    odd_footer: text(odd_footer),
                    even_header: text(even_header),
                    even_footer: text(even_footer),
                    first_header: text(first_header),
                    first_footer: text(first_footer),
                    different_odd_even: different_odd_even == "true",
                    different_first: different_first == "true",
                },
                changed,
            )
            .map_err(js_error)
    }

    pub fn print_references(&mut self, name: &str, titles: bool) -> Result<Vec<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .print_references(titles))
    }

    pub fn set_print_references(
        &mut self,
        name: &str,
        titles: bool,
        formulas: Vec<String>,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .set_print_references(titles, &formulas)
            .map_err(js_error)
    }

    pub fn print_options(&mut self, name: &str) -> Result<Vec<u8>, JsError> {
        let options = self
            .worksheet(name)
            .map_err(js_error)?
            .print_options()
            .map_err(js_error)?;
        Ok([
            options.gridlines,
            options.headings,
            options.horizontal_centered,
            options.vertical_centered,
        ]
        .into_iter()
        .map(u8::from)
        .collect())
    }

    /// Update supplied print flags. Values 0 and 1 set a flag, while 2 preserves it.
    /// Returns a read error separately from an edit error for the adapter.
    pub fn update_print_options(
        &mut self,
        name: &str,
        values: &[u8],
    ) -> Result<Option<String>, JsError> {
        if values.len() != 4 || values.iter().any(|&value| value > 2) {
            return Err(JsError::new("Expected four print option updates"));
        }
        let mut sheet = match self.worksheet(name) {
            Ok(sheet) => sheet,
            Err(error) => return Ok(Some(error.to_string())),
        };
        let mut options = match sheet.print_options() {
            Ok(options) => options,
            Err(error) => return Ok(Some(error.to_string())),
        };
        let mut changed = false;
        let flags = [
            &mut options.gridlines,
            &mut options.headings,
            &mut options.horizontal_centered,
            &mut options.vertical_centered,
        ];
        for (flag, &value) in flags.into_iter().zip(values) {
            if value < 2 {
                changed |= *flag != (value != 0);
                *flag = value != 0;
            }
        }
        if changed {
            sheet.set_print_options(&options).map_err(js_error)?;
        }
        Ok(None)
    }

    /// Applies a block of rules in order. Each header contains the field
    /// count, style count, optional priority (zero if omitted), and stop flag.
    pub fn add_conditional_rules(
        &mut self,
        name: &str,
        headers: &[u32],
        entries: Vec<String>,
    ) -> Result<(), JsError> {
        let (rules, remainder) = headers.as_chunks::<4>();
        let expected = rules.iter().try_fold(0usize, |count, &[fields, style, _, _]| {
            count.checked_add(fields as usize)?.checked_add(style as usize)
        });
        if rules.is_empty() || !remainder.is_empty() || expected != Some(entries.len()) {
            return Err(JsError::new("Invalid conditional formatting block"));
        }
        let floor = rules.iter().map(|rule| rule[2]).max().unwrap_or(0);
        let mut styles = std::collections::HashMap::new();
        let mut entries = entries.into_iter();
        let mut has_standard = false;
        for &[fields, style, priority, stop] in rules {
            let mut fields = entries.by_ref().take(fields as usize);
            let reference = fields.next().ok_or_else(|| JsError::new("Missing conditional formatting range"))?;
            let fields: Vec<_> = fields.collect();
            let standard = fields.first().is_none_or(|kind| !matches!(kind.as_str(), "extendedIconSet" | "standaloneDataBar"));
            let style = entries.by_ref().take(style as usize).collect();
            self.add_conditional_rule_cached(
                name, &reference, fields, (priority != 0).then_some(priority),
                floor, stop != 0, style, has_standard, &mut styles,
            )?;
            has_standard |= standard;
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Preserve the positional JavaScript ABI"
    )]
    pub fn add_conditional_rule(
        &mut self,
        name: &str,
        reference: &str,
        fields: Vec<String>,
        priority: Option<u32>,
        priority_floor: u32,
        stop_if_true: bool,
        style: Vec<String>,
        append: bool,
    ) -> Result<(), JsError> {
        self.add_conditional_rule_cached(
            name, reference, fields, priority, priority_floor, stop_if_true, style, append,
            &mut std::collections::HashMap::new(),
        )
    }

    #[expect(clippy::too_many_arguments, reason = "Shares the conditional rule binding's typed fields")]
    fn add_conditional_rule_cached(
        &mut self,
        name: &str,
        reference: &str,
        fields: Vec<String>,
        priority: Option<u32>,
        priority_floor: u32,
        stop_if_true: bool,
        style: Vec<String>,
        append: bool,
        styles: &mut std::collections::HashMap<Vec<String>, u32>,
    ) -> Result<(), JsError> {
        use ooxml_xlsx::{CfOperator, Rule, RuleKind};

        let invalid = || JsError::new("Invalid conditional formatting rule");
        if let [kind, id, icon_count, icon_set, reverse, show_value, payload @ ..] = fields.as_slice()
            && kind == "extendedIconSet"
        {
            if !style.is_empty() || stop_if_true {
                return Err(invalid());
            }
            let count = icon_count.parse::<usize>().map_err(|_| invalid())?;
            let fields = count.checked_mul(2).ok_or_else(invalid)?;
            let (selected, thresholds) = payload.split_at_checked(fields).ok_or_else(invalid)?;
            let (selected, _) = selected.as_chunks::<2>();
            let icons = selected.iter().map(|[name, id]| {
                Ok((name.clone(), id.parse::<u32>().map_err(|_| invalid())?))
            }).collect::<Result<Vec<_>, JsError>>()?;
            let thresholds = decode_icon_thresholds(thresholds)?;
            return self.worksheet(name).map_err(js_error)?.add_extended_icon_rule(reference,
                &ooxml_xlsx::ExtendedIconRule {
                    id: (!id.is_empty()).then_some(id.as_str()), priority, priority_floor,
                    name: icon_set, thresholds: &thresholds, icons: &icons,
                    reverse: reverse == "true", show_value: show_value == "true",
                }).map_err(js_error);
        }
        let mut add = |document: &mut Self, rule: Rule| {
            let mut rule = rule
                .with_priority(priority, priority_floor)
                .stop_if_true(stop_if_true);
            if !style.is_empty() {
                let id = match styles.get(&style) {
                    Some(&id) => id,
                    None => {
                        let differential = decode_conditional_style(&style)?;
                        let id = document.workbook.add_differential_format(&differential).map_err(js_error)?;
                        styles.insert(style.clone(), id);
                        id
                    }
                };
                rule = rule.with_dxf(id);
            }
            let mut sheet = document.worksheet(name).map_err(js_error)?;
            if append {
                sheet.append_conditional_format_rule(reference, &rule)
            } else {
                sheet.add_conditional_format(reference, &rule)
            }
            .map_err(js_error)
        };
        if let [kind, icon_set, reverse, show_value, thresholds @ ..] = fields.as_slice()
            && kind == "iconSet"
        {
            let (values, inclusion) = decode_icon_thresholds(thresholds)?.into_iter().unzip();
            let rule = Rule::icon_set(icon_set, values, reverse == "true", show_value == "true")
                .and_then(|rule| rule.icon_threshold_inclusion(inclusion))
                .map_err(js_error)?;
            return add(self, rule);
        }
        if let [
            kind, color, min, max, show_value, low_type, low, high_type, high, extended @ ..
        ] = fields.as_slice()
            && matches!(kind.as_str(), "dataBar" | "standaloneDataBar")
        {
            let standalone = kind == "standaloneDataBar";
            // An empty length or visibility field means the option was not
            // declared, and the rule omits the attribute. The x14 extension
            // has its own length fields, with the same convention.
            let declared = |value: &str| (!value.is_empty()).then(|| value.parse::<u32>().map_err(|_| invalid())).transpose();
            let declared_lengths = (declared(min)?, declared(max)?);
            let declared_show = (!show_value.is_empty()).then(|| show_value == "true");
            let base = if standalone {
                if !style.is_empty() || stop_if_true {
                    return Err(invalid());
                }
                None
            } else {
                let thresholds = [conditional_threshold(low_type, low)?, conditional_threshold(high_type, high)?];
                let color = decode_style_color(color)?.ok_or_else(invalid)?;
                let rule = Rule::data_bar_declared(thresholds[0].clone(), thresholds[1].clone(), color,
                    declared_lengths.0, declared_lengths.1, declared_show).map_err(js_error)?;
                if extended.is_empty() {
                    return add(self, rule);
                }
                Some((rule, thresholds))
            };
            let [id, ext_min, ext_max, border, gradient, same_color, same_border, axis, direction,
                fill_color, border_color, negative_fill, negative_border, axis_color, ext_thresholds @ ..] = extended else {
                return Err(invalid());
            };
            use ooxml_xlsx::ExtendedDataBarThreshold as Threshold;
            let thresholds = match ext_thresholds {
                [] => {
                    let Some((_, thresholds)) = &base else { return Err(invalid()) };
                    thresholds.clone().map(Threshold::Value)
                }
                [low_kind, low_val, high_kind, high_val] => {
                    let decode = |kind: &str, value: &str| match (kind, value) {
                        ("autoMin", "") => Ok(Threshold::AutoMin),
                        ("autoMax", "") => Ok(Threshold::AutoMax),
                        _ => conditional_threshold(kind, value).map(Threshold::Value),
                    };
                    [decode(low_kind, low_val)?, decode(high_kind, high_val)?]
                }
                _ => return Err(invalid()),
            };
            let colors = [
                decode_style_color(fill_color)?, decode_style_color(border_color)?,
                decode_style_color(negative_fill)?, decode_style_color(negative_border)?,
                decode_style_color(axis_color)?,
            ];
            let flag = |value: &String| match value.as_str() {
                "" => Ok(None),
                "true" => Ok(Some(true)),
                "false" => Ok(Some(false)),
                _ => Err(invalid()),
            };
            let model = ooxml_xlsx::ExtendedDataBar {
                id: (!id.is_empty()).then_some(id.as_str()), thresholds: &thresholds,
                lengths: (declared(ext_min)?, declared(ext_max)?),
                flags: [flag(border)?, flag(gradient)?, flag(same_color)?, flag(same_border)?],
                show_value: declared_show.filter(|_| standalone),
                axis: (!axis.is_empty()).then_some(axis.as_str()),
                direction: (!direction.is_empty()).then_some(direction.as_str()),
                colors,
            };
            if let Some((rule, _)) = base {
                add(self, rule)?;
                return self.worksheet(name).map_err(js_error)?.attach_extended_data_bar(reference, &model).map_err(js_error);
            }
            return self.worksheet(name).map_err(js_error)?.add_standalone_data_bar(reference, &model, priority).map_err(js_error);
        }
        if let [kind, period, formula] = fields.as_slice()
            && kind == "timePeriod"
        {
            let rule = Rule::time_period(period, (!formula.is_empty()).then(|| formula.clone()))
                .map_err(js_error)?;
            return add(self, rule);
        }
        let kind = match fields.as_slice() {
            [kind, formula] if kind == "expression" => RuleKind::Expression(formula.clone()),
            [kind, operator, first, second] if kind == "cellIs" => {
                let operator = match operator.as_str() {
                    "between" => CfOperator::Between,
                    "notBetween" => CfOperator::NotBetween,
                    "equal" => CfOperator::Equal,
                    "notEqual" => CfOperator::NotEqual,
                    "greaterThan" => CfOperator::GreaterThan,
                    "lessThan" => CfOperator::LessThan,
                    "greaterThanOrEqual" => CfOperator::GreaterThanOrEqual,
                    "lessThanOrEqual" => CfOperator::LessThanOrEqual,
                    _ => return Err(invalid()),
                };
                RuleKind::CellIs(
                    operator,
                    first.clone(),
                    (!second.is_empty()).then(|| second.clone()),
                )
            }
            [kind, operator, text, formulas @ ..] if kind == "containsText" && matches!(formulas.len(), 0 | 3) => {
                let kind = match operator.as_str() {
                    "containsText" => RuleKind::ContainsText(text.clone()),
                    "notContains" => RuleKind::NotContainsText(text.clone()),
                    "beginsWith" => RuleKind::BeginsWith(text.clone()),
                    "endsWith" => RuleKind::EndsWith(text.clone()),
                    "containsBlanks" => RuleKind::ContainsBlanks(true),
                    "notContainsBlanks" => RuleKind::ContainsBlanks(false),
                    "containsErrors" => RuleKind::ContainsErrors(true),
                    "notContainsErrors" => RuleKind::ContainsErrors(false),
                    _ => return Err(invalid()),
                };
                let mut rule = Rule::new(kind);
                if let [formula, omit_text, omit_operator] = formulas {
                    rule = rule.text_formula(formula.clone(), omit_text == "1", omit_operator == "1").map_err(js_error)?;
                }
                return add(self, rule);
            }

            [kind, stops @ ..] if kind == "colorScale" => {
                let (entries, remainder) = stops.as_chunks::<3>();
                if !remainder.is_empty() {
                    return Err(invalid());
                }
                let stops = entries
                    .iter()
                    .map(|[kind, value, color]| {
                        Ok((conditional_threshold(kind, value)?, decode_style_color(color)?.ok_or_else(invalid)?))
                    })
                    .collect::<Result<_, JsError>>()?;
                return add(self, Rule::color_scale(stops));
            }
            [kind, rank, percent, bottom] if kind == "top10" => RuleKind::Top10 {
                rank: rank.parse().map_err(|_| invalid())?,
                percent: percent == "true",
                bottom: bottom == "true",
            },
            [kind, above] if kind == "aboveAverage" => RuleKind::AboveAverage(above == "true"),
            [kind, above, equal, std_dev] if kind == "aboveAverage" => {
                let equal = match equal.as_str() {
                    "" => None,
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => return Err(invalid()),
                };
                let std_dev = (!std_dev.is_empty()).then(|| std_dev.parse()).transpose().map_err(|_| invalid())?;
                let rule = Rule::above_average(above == "true", equal, std_dev).map_err(js_error)?;
                return add(self, rule);
            }
            [kind] if kind == "duplicateValues" => RuleKind::DuplicateValues,
            [kind] if kind == "uniqueValues" => RuleKind::UniqueValues,
            _ => return Err(invalid()),
        };
        add(self, Rule::new(kind))
    }

    pub fn add_table(
        &mut self,
        sheet: &str,
        reference: &str,
        settings: Vec<String>,
        columns: Vec<String>,
    ) -> Result<String, JsError> {
        self.add_table_with_columns(sheet, reference, settings, columns, None, &[])
    }

    pub fn attach_table_model(
        &mut self,
        sheet: &str,
        reference: &str,
        settings: Vec<String>,
        columns: Vec<String>,
        calculated: Vec<String>,
        differential: Vec<String>,
    ) -> Result<String, JsError> {
        self.add_table_with_columns(sheet, reference, settings, columns, Some(calculated), &differential)
    }
}

impl XlsxDocument {
    fn add_table_with_columns(
        &mut self,
        sheet: &str,
        reference: &str,
        settings: Vec<String>,
        columns: Vec<String>,
        calculated: Option<Vec<String>>,
        mut differential: &[String],
    ) -> Result<String, JsError> {
        use ooxml_xlsx::{Table, TableStyle};

        let invalid = || JsError::new("Invalid table options");
        let [
            name,
            display_name,
            header,
            totals,
            theme,
            first,
            last,
            rows,
            cols,
        ] = settings.as_slice()
        else {
            return Err(invalid());
        };
        let style = TableStyle::parse(theme).map_err(js_error)?;
        let (columns, remainder) = columns.as_chunks::<4>();
        if !remainder.is_empty() {
            return Err(invalid());
        }
        let mut columns = columns
            .iter()
            .map(decode_table_column)
            .collect::<Result<Vec<_>, JsError>>()?;
        let metadata_only = calculated.is_some();
        if let Some(formulas) = calculated {
            if formulas.len() != columns.len() {
                return Err(invalid());
            }
            for (column, formula) in columns.iter_mut().zip(formulas) {
                if !formula.is_empty() {
                    *column = column.clone().formula(formula);
                }
            }
        }
        let mut table_dxfs = [None; 3];
        let mut border_dxfs = [None; 3];
        if !differential.is_empty() {
            let mut read_formats = || -> Result<[Option<u32>; 3], JsError> {
                let mut ids = [None; 3];
                for slot in &mut ids {
                    let (length, rest) = differential.split_first().ok_or_else(invalid)?;
                    differential = rest;
                    if length.is_empty() {
                        continue;
                    }
                    let length = length.parse::<usize>().map_err(|_| invalid())?;
                    let (fields, rest) = differential.split_at_checked(length).ok_or_else(invalid)?;
                    differential = rest;
                    let format = decode_conditional_style(fields)?;
                    *slot = Some(self.workbook.add_differential_format(&format).map_err(js_error)?);
                }
                Ok(ids)
            };
            table_dxfs = read_formats()?;
            border_dxfs = read_formats()?;
            for column in &mut columns {
                *column = column.clone().differential_formats(read_formats()?);
            }
            if !differential.is_empty() {
                return Err(invalid());
            }
        }
        let mut table = Table::new(CellRange::parse(reference).map_err(js_error)?)
            .name(name)
            .header_row(header == "true")
            .totals_row(totals == "true")
            .columns(columns)
            .differential_formats(table_dxfs)
            .border_formats(border_dxfs)
            .style(style)
            .overwrite(true)
            .row_stripes(rows == "true")
            .column_stripes(cols == "true")
            .emphasize_columns(first == "true", last == "true");
        if metadata_only {
            table = table.metadata_only();
        }
        if !display_name.is_empty() {
            table = table.display_name(display_name);
        }
        self.worksheet(sheet)
            .map_err(js_error)?
            .add_table(table)
            .map_err(js_error)
    }

}

#[wasm_bindgen]
impl XlsxDocument {
    pub fn remove_table_tail_rows(&mut self, sheet: &str, name: &str, count: u32) -> Result<(), JsError> {
        self.worksheet(sheet).map_err(js_error)?.remove_table_tail_rows(name, count).map_err(js_error)
    }

    pub fn remove_table_rows(&mut self, sheet: &str, name: &str, removals: &[u32]) -> Result<(), JsError> {
        let (removals, leftover) = removals.as_chunks::<2>();
        if !leftover.is_empty() {
            return Err(JsError::new("Expected a position and count per table row removal"));
        }
        let mut worksheet = self.worksheet(sheet).map_err(js_error)?;
        for &[start, count] in removals {
            worksheet.remove_table_rows(name, start, count).map_err(js_error)?;
        }
        Ok(())
    }

    pub fn write_table_initial_values(
        &mut self,
        name: &str,
        reference: &str,
        start: usize,
        values: Vec<JsValue>,
        kinds: &[u8],
        results: Vec<JsValue>,
    ) -> Result<(), JsError> {
        let range = CellRange::parse(reference).map_err(js_error)?;
        let width = u64::from(range.last.col - range.first.col + 1);
        let height = u64::from(range.last.row - range.first.row + 1);
        let capacity = width * height;
        let start = start as u64;
        let end = start + values.len() as u64;
        if end > capacity {
            return Err(JsError::new("Table values exceed the data range"));
        }
        let covers_range = start == 0 && end == capacity;
        if kinds.iter().any(|kind| !matches!(kind, 0..=2 | 5)) {
            return Err(JsError::new("Expected a scalar, Excel error, or ordinary formula"));
        }
        let scalar_kinds;
        let kinds = if kinds.is_empty() {
            scalar_kinds = vec![0; values.len()];
            &scalar_kinds
        } else {
            kinds
        };
        with_table_cell_writes(values, kinds, results, |writes| {
            let mut sheet = self.worksheet(name).map_err(js_error)?;
            let guarded = classify_sheet(&sheet).map_err(js_error)?;
            let mut cells = Vec::with_capacity(writes.len());
            let mut linked_cells = std::collections::HashSet::new();
            for (index, &write) in (start..end).zip(writes) {
                let at = CellIndex {
                    row: range.first.row + (index / width) as u32,
                    col: range.first.col + (index % width) as u32,
                };
                if !covers_range {
                    linked_cells.insert(at);
                }
                if let CellWrite::FormulaWithResult(text, result) = write {
                    // Later edits may invalidate this cache, so apply formulas in row-major order.
                    if !cells.is_empty() {
                        sheet.write_cells(&cells).map_err(js_error)?;
                        cells.clear();
                    }
                    check_excel_edit(&mut sheet, &guarded, at, true).map_err(js_error)?;
                    sheet.cell_mut(at.row, at.col).map_err(js_error)?
                        .set_formula_with_result(text, result.clone()).map_err(js_error)?;
                    continue;
                }
                let empty = check_excel_edit(&mut sheet, &guarded, at, true).map_err(js_error)?;
                if write != CellWrite::Clear || !empty {
                    cells.push((at, write));
                }
            }
            sheet.write_cells(&cells).map_err(js_error)?;
            if covers_range {
                sheet.remove_cell_hyperlinks(range).map_err(js_error)
            } else {
                sheet.remove_cell_hyperlinks_at(&linked_cells).map_err(js_error)
            }
        })
    }

    pub fn write_table_rows(
        &mut self,
        sheet: &str,
        name: &str,
        placement: &[u32],
        values: Vec<JsValue>,
        formula_flags: &[u8],
        results: Vec<JsValue>,
    ) -> Result<(), JsError> {
        let (width, position) = match *placement {
            [width] => (width as usize, None),
            [width, position] => (width as usize, Some(position)),
            _ => return Err(JsError::new("Expected table row width and optional position")),
        };
        if width == 0 || !values.len().is_multiple_of(width) {
            return Err(JsError::new("Expected complete table rows"));
        }
        with_table_cell_writes(values, formula_flags, results, |writes| {
            let rows: Vec<_> = writes.chunks_exact(width).collect();
            let mut worksheet = self.worksheet(sheet).map_err(js_error)?;
            match position {
                Some(position) => worksheet.insert_table_rows(name, position, &rows),
                None => worksheet.append_table_rows(name, &rows),
            }.map_err(js_error)
        })
    }

    pub fn insert_table_column(
        &mut self,
        sheet: &str,
        name: &str,
        settings: Vec<String>,
        values: Vec<JsValue>,
        formula_flags: &[u8],
        results: Vec<JsValue>,
    ) -> Result<(), JsError> {
        let [position, header, filter_button] = settings.as_slice() else {
            return Err(JsError::new("Expected table column position, header, and filter button"));
        };
        let position = position.parse::<u32>()
            .map_err(|_| JsError::new("Expected a table column position"))?;
        let column = ooxml_xlsx::Col::new(header).filter_button(filter_button == "true");
        with_table_cell_writes(values, formula_flags, results, |writes| {
            self.worksheet(sheet).map_err(js_error)?
                .insert_table_column(name, position, column, writes).map_err(js_error)
        })
    }

    pub fn replace_table_columns(
        &mut self,
        sheet: &str,
        name: &str,
        settings: Vec<String>,
        values: Vec<JsValue>,
        formula_flags: &[u8],
        results: Vec<JsValue>,
    ) -> Result<(), JsError> {
        let (columns, trailing) = settings.as_chunks::<4>();
        if !trailing.is_empty() || columns.is_empty() || values.is_empty() || !values.len().is_multiple_of(columns.len()) {
            return Err(JsError::new("Expected equal-length replacement columns"));
        }
        let height = values.len() / columns.len();
        let columns = columns.iter().map(decode_table_column).collect::<Result<Vec<_>, _>>()?;
        with_table_cell_writes(values, formula_flags, results, |writes| {
            let replacements = columns.into_iter().zip(writes.chunks_exact(height)).collect();
            self.worksheet(sheet).map_err(js_error)?.replace_table_columns(name, replacements).map_err(js_error)
        })
    }

    pub fn patch_table_column_style(
        &mut self,
        sheet: &str,
        name: &str,
        column: u32,
        fields: Vec<String>,
    ) -> Result<(), JsError> {
        let format = decode_table_column_style(&fields)?;
        self.worksheet(sheet).map_err(js_error)?.patch_table_column_style(name, column, format).map_err(js_error)
    }

    pub fn patch_table_column_styles(
        &mut self,
        sheet: &str,
        name: &str,
        edits: &[u32],
        fields: Vec<String>,
    ) -> Result<(), JsError> {
        let (positions, trailing) = edits.as_chunks::<2>();
        let invalid = || JsError::new("Mismatched table column styles");
        if !trailing.is_empty() {
            return Err(invalid());
        }
        let mut fields = fields.as_slice();
        let mut edits = Vec::with_capacity(positions.len());
        for &[column, length] in positions {
            let (style, rest) = fields.split_at_checked(length as usize).ok_or_else(invalid)?;
            edits.push((column, decode_table_column_style(style)?));
            fields = rest;
        }
        if !fields.is_empty() {
            return Err(invalid());
        }
        self.worksheet(sheet).map_err(js_error)?.patch_table_column_styles(name, edits).map_err(js_error)
    }

    pub fn set_table_column_totals(
        &mut self,
        sheet: &str,
        name: &str,
        columns: &[u32],
        settings: Vec<String>,
        results: Vec<JsValue>,
        kinds: Vec<String>,
    ) -> Result<(), JsError> {
        let (settings, trailing) = settings.as_chunks::<2>();
        if !trailing.is_empty() || columns.len() != settings.len() ||
            columns.len() != results.len() || columns.len() != kinds.len()
        {
            return Err(JsError::new("Mismatched table totals settings"));
        }
        let edits = columns.iter().zip(settings).zip(results.iter().zip(kinds.iter()))
            .map(|((&column, [function, value]), (result, kind))| {
                let total = decode_table_total(function, value)?;
                let result = decode_formula_result(result, (!kind.is_empty()).then_some(kind.as_str()))?;
                Ok((column, total, result))
            }).collect::<Result<Vec<_>, JsError>>()?;
        self.worksheet(sheet).map_err(js_error)?.set_table_column_totals(name, edits).map_err(js_error)
    }

    pub fn remove_table_tail_columns(&mut self, sheet: &str, name: &str, count: u32) -> Result<(), JsError> {
        self.worksheet(sheet).map_err(js_error)?.remove_table_tail_columns(name, count).map_err(js_error)
    }

    pub fn remove_table_columns(&mut self, sheet: &str, name: &str, removals: &[u32]) -> Result<(), JsError> {
        let (removals, leftover) = removals.as_chunks::<2>();
        if !leftover.is_empty() {
            return Err(JsError::new("Expected a position and count per table column removal"));
        }
        let mut worksheet = self.worksheet(sheet).map_err(js_error)?;
        for &[start, count] in removals {
            worksheet.remove_table_columns(name, start, count).map_err(js_error)?;
        }
        Ok(())
    }

    pub fn set_table_style(
        &mut self,
        sheet: &str,
        name: &str,
        theme: Option<String>,
        flags: &[u8],
    ) -> Result<(), JsError> {
        let flags: [u8; 4] = flags
            .try_into()
            .map_err(|_| JsError::new("Expected four table style flags"))?;
        let theme = theme
            .as_deref()
            .map(ooxml_xlsx::TableStyle::parse)
            .transpose()
            .map_err(js_error)?;
        self.worksheet(sheet)
            .map_err(js_error)?
            .set_table_style(name, theme, flags.map(|value| value != 0))
            .map_err(js_error)
    }

    pub fn write_table_headers(
        &mut self,
        sheet: &str,
        row: u32,
        columns: &[u32],
        names: Vec<String>,
    ) -> Result<(), JsError> {
        if columns.len() != names.len() {
            return Err(JsError::new("Expected one name per table column"));
        }
        let writes = columns
            .iter()
            .zip(&names)
            .map(|(&column, name)| {
                Ok((
                    CellIndex::new(row, column).map_err(js_error)?,
                    CellWrite::String(name),
                ))
            })
            .collect::<Result<Vec<_>, JsError>>()?;
        let cells = writes.iter().map(|(cell, _)| *cell).collect();
        let mut worksheet = self.worksheet(sheet).map_err(js_error)?;
        worksheet.write_cells(&writes).map_err(js_error)?;
        worksheet.remove_cell_hyperlinks_at(&cells).map_err(js_error)
    }

    pub fn set_table_filter_button(
        &mut self,
        sheet: &str,
        name: &str,
        column: u32,
        visible: bool,
    ) -> Result<(), JsError> {
        self.worksheet(sheet)
            .map_err(js_error)?
            .set_table_filter_button(name, column, visible)
            .map_err(js_error)
    }

    pub fn set_table_filter_buttons(
        &mut self,
        sheet: &str,
        name: &str,
        columns: &[u32],
        visible: &[u8],
    ) -> Result<(), JsError> {
        if columns.len() != visible.len() {
            return Err(JsError::new("Expected one visibility flag per table column"));
        }
        let edits: Vec<_> = columns.iter().copied().zip(visible.iter().map(|&flag| flag != 0)).collect();
        self.worksheet(sheet)
            .map_err(js_error)?
            .set_table_filter_buttons(name, &edits)
            .map_err(js_error)
    }

    pub fn insert_worksheet_image(
        &mut self,
        sheet: &str,
        bytes: &[u8],
        coordinates: &[f64],
        edit_as: &str,
        hyperlink: Option<String>,
        tooltip: Option<String>,
    ) -> Result<Vec<String>, JsError> {
        use ooxml_xlsx::{Anchor, EditAs, Image, Marker, Size};
        fn marker(values: &[f64]) -> Result<Marker, JsError> {
            Ok(Marker {
                cell: CellIndex::new(values[2] as u32, values[0] as u32).map_err(js_error)?,
                x: values[1] as i64,
                y: values[3] as i64,
            })
        }
        let anchor = match coordinates {
            [from @ .., width, height] if from.len() == 4 => Anchor::one_cell(
                marker(from)?,
                Size::Emu {
                    width: *width as u64,
                    height: *height as u64,
                },
            ),
            [
                from_col,
                from_x,
                from_row,
                from_y,
                to_col,
                to_x,
                to_row,
                to_y,
            ] => {
                let behavior = match edit_as {
                    "absolute" => EditAs::Absolute,
                    "oneCell" => EditAs::OneCell,
                    "twoCell" => EditAs::TwoCell,
                    _ => return Err(JsError::new("Invalid image editAs setting")),
                };
                Anchor::two_cell(
                    marker(&[*from_col, *from_x, *from_row, *from_y])?,
                    marker(&[*to_col, *to_x, *to_row, *to_y])?,
                )
                .edit_as(behavior)
            }
            _ => return Err(JsError::new("Invalid image coordinates")),
        };
        let mut image = Image::from_bytes(bytes.to_vec()).map_err(js_error)?;
        if let Some(hyperlink) = hyperlink {
            image = image.hyperlink(hyperlink);
            if let Some(tooltip) = tooltip {
                image = image.hyperlink_tooltip(tooltip);
            }
        }
        let mut worksheet = self.worksheet(sheet).map_err(js_error)?;
        let id = worksheet.insert_image(anchor, &image).map_err(js_error)?;
        worksheet
            .images()
            .map_err(js_error)?
            .into_iter()
            .find(|image| image.id == Some(id))
            .and_then(|image| image.media)
            .map(|media| vec![media.part, id.to_string()])
            .ok_or_else(|| JsError::new("Inserted image media could not be located"))
    }

    pub fn worksheet_background_image(&mut self, name: &str) -> Result<Option<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .background_image()
            .map_err(js_error)?
            .map(|image| image.part))
    }

    pub fn worksheet_background_bytes(&mut self, name: &str) -> Result<Vec<u8>, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .background_image()
            .map_err(js_error)?
            .map(|image| image.bytes)
            .ok_or_else(|| JsError::new("Worksheet background image is missing"))
    }

    pub fn set_worksheet_background_image(
        &mut self,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, JsError> {
        let image = ooxml_xlsx::Image::from_bytes(bytes.to_vec()).map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_background_image(&image)
            .map_err(js_error)
    }

    pub fn remove_worksheet_image(&mut self, name: &str, id: u32) -> Result<(), JsError> {
        self.worksheet(name).map_err(js_error)?.remove_image(id).map_err(js_error)
    }

    pub fn remove_worksheet_background_image(&mut self, name: &str) -> Result<bool, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .remove_background_image()
            .map_err(js_error)
    }

    pub fn worksheet_images(&mut self, sheet: &str) -> Result<Vec<String>, JsError> {
        use ooxml_xlsx::{Anchor, EditAs, Marker, Size};

        fn marker(values: &mut Vec<String>, at: Marker) {
            values.extend([
                at.cell.col.to_string(),
                at.x.to_string(),
                at.cell.row.to_string(),
                at.y.to_string(),
            ]);
        }

        let mut values = Vec::new();
        for (index, image) in self
            .worksheet(sheet)
            .map_err(js_error)?
            .images()
            .map_err(js_error)?
            .into_iter()
            .enumerate()
        {
            let Some(media) = image.media else { continue };
            let mut record = vec![
                index.to_string(),
                image.id.map(|id| id.to_string()).unwrap_or_default(),
                media.part,
                image.name.unwrap_or_default(),
                image.alt_text.unwrap_or_default(),
                image.hyperlink.unwrap_or_default(),
                image.hyperlink_tooltip.unwrap_or_default(),
            ];
            match image.anchor {
                Anchor::OneCell {
                    from,
                    size: Size::Emu { width, height },
                } => {
                    record.push("oneCell".into());
                    marker(&mut record, from);
                    record.extend([width.to_string(), height.to_string()]);
                }
                Anchor::TwoCell { from, to, edit_as } => {
                    record.push(
                        match edit_as {
                            EditAs::TwoCell => "twoCell",
                            EditAs::OneCell => "oneCell",
                            EditAs::Absolute => "absolute",
                        }
                        .into(),
                    );
                    marker(&mut record, from);
                    marker(&mut record, to);
                }
                _ => continue,
            }
            values.push(record.len().to_string());
            values.extend(record);
        }
        Ok(values)
    }

    pub fn replace_image_part(&mut self, part: &str, bytes: Vec<u8>) -> Result<(), JsError> {
        self.workbook.replace_image_part(part, bytes).map_err(js_error)
    }

    pub fn worksheet_image_bytes(&mut self, sheet: &str, index: usize) -> Result<Vec<u8>, JsError> {
        self.worksheet(sheet)
            .map_err(js_error)?
            .images()
            .map_err(js_error)?
            .into_iter()
            .nth(index)
            .and_then(|image| image.media)
            .map(|media| media.bytes)
            .ok_or_else(|| JsError::new("Worksheet image is missing"))
    }

    pub fn table_nodes(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        let tables = self
            .worksheet(name)
            .map_err(js_error)?
            .table_nodes()
            .map_err(js_error)?;
        let mut output = Vec::new();
        for table in &tables {
            encode_metadata_node(table, &mut output);
        }
        Ok(output)
    }

    pub fn remove_table(&mut self, sheet: &str, name: &str) -> Result<(), JsError> {
        self.worksheet(sheet)
            .map_err(js_error)?
            .remove_table(name)
            .map_err(js_error)
    }

    pub fn differential_format(&mut self, id: usize) -> Result<Vec<String>, JsError> {
        let mut output = Vec::new();
        if let Some(node) = self
            .workbook
            .differential_format_node(id)
            .map_err(js_error)?
        {
            encode_metadata_node(&node, &mut output);
        }
        Ok(output)
    }

    pub fn differential_formats(&mut self, ids: &[u32]) -> Result<Vec<String>, JsError> {
        let mut output = Vec::new();
        for (id, node) in self.workbook.differential_format_nodes(ids).map_err(js_error)? {
            output.push(id.to_string());
            let length = output.len();
            output.push(String::new());
            encode_metadata_node(&node, &mut output);
            output[length] = (output.len() - length - 1).to_string();
        }
        Ok(output)
    }

    pub fn conditional_format_blocks(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        let blocks = self
            .worksheet(name)
            .map_err(js_error)?
            .conditional_format_blocks()
            .map_err(js_error)?;
        let mut output = Vec::new();
        for block in &blocks {
            encode_metadata_node(block, &mut output);
        }
        Ok(output)
    }

    pub fn extended_conditional_format_blocks(
        &mut self,
        name: &str,
    ) -> Result<Vec<String>, JsError> {
        let rules = self
            .worksheet(name)
            .map_err(js_error)?
            .extended_conditional_format_blocks()
            .map_err(js_error)?;
        let mut output = Vec::new();
        for rule in &rules {
            encode_metadata_node(rule, &mut output);
        }
        Ok(output)
    }

    pub fn remove_standalone_conditional_formats(
        &mut self,
        name: &str,
        ids: Vec<String>,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .remove_standalone_conditional_formats(&ids)
            .map_err(js_error)
    }

    pub fn remove_selected_conditional_formats(
        &mut self,
        name: &str,
        ordinals: &[u32],
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .remove_selected_conditional_formats(ordinals)
            .map_err(js_error)
    }

    pub fn conditional_format_count(&mut self, name: &str) -> Result<usize, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .conditional_format_count()
            .map_err(js_error)
    }

    pub fn remove_conditional_format_blocks(
        &mut self,
        name: &str,
        ordinal: Option<usize>,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .remove_conditional_format_blocks(ordinal)
            .map_err(js_error)
    }

    pub fn sheet_views(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        let mut entries = Vec::new();
        for view in self
            .worksheet(name)
            .map_err(js_error)?
            .sheet_views()
            .map_err(js_error)?
        {
            entries.push(view.len().to_string());
            entries.extend(view.into_iter().flat_map(|(key, value)| [key, value]));
        }
        Ok(entries)
    }

    pub fn reset_sheet_views(&mut self, name: &str, ids: &[u32]) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .reset_sheet_views(ids)
            .map_err(js_error)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Keep the worksheet view fields typed across the JavaScript boundary"
    )]
    pub fn set_sheet_view(
        &mut self,
        name: &str,
        id: u32,
        state: &str,
        x: f64,
        y: f64,
        zoom: Option<u32>,
        grid: Option<bool>,
        rtl: Option<bool>,
        normal_zoom: Option<u32>,
        ruler: Option<bool>,
        headings: Option<bool>,
        selected: Option<bool>,
        style: Option<String>,
        top_left: Option<String>,
        active_cell: Option<String>,
        active_pane: Option<String>,
    ) -> Result<(), JsError> {
        let mut sheet = self
            .worksheet(name)
            .map_err(js_error)?
            .for_workbook_view(id);
        match state {
            "normal" => sheet.unfreeze().map(drop),
            "frozen"
                if [x, y].into_iter().all(|value| {
                    (0.0..=f64::from(u32::MAX)).contains(&value) && value.fract() == 0.0
                }) =>
            {
                sheet.freeze_panes(y as u32, x as u32)
            }
            "frozen" => return Err(JsError::new("Expected integer frozen pane counts")),
            "split" => sheet.split_panes_at(x, y),
            _ => return Err(JsError::new("Unsupported worksheet view state")),
        }
        .map_err(js_error)?;
        if let Some(zoom) = zoom {
            sheet.set_zoom(zoom).map_err(js_error)?;
        }
        if let Some(grid) = grid {
            sheet.set_show_gridlines(grid).map_err(js_error)?;
        }
        if let Some(rtl) = rtl {
            sheet.set_right_to_left(rtl).map_err(js_error)?;
        }
        if normal_zoom.is_some() || ruler.is_some() || headings.is_some() || selected.is_some() || style.is_some() {
            sheet.set_view_display(normal_zoom, ruler, headings, selected, style.as_deref())
                .map_err(js_error)?;
        }
        if top_left.is_some() || active_cell.is_some() || active_pane.is_some() {
            sheet.set_view_selection(top_left.as_deref(), active_cell.as_deref(), active_pane.as_deref())
                .map_err(js_error)?;
        }
        Ok(())
    }

    pub fn outline_summary(&mut self, name: &str) -> Result<Vec<u8>, JsError> {
        let (below, right) = self
            .worksheet(name)
            .map_err(js_error)?
            .outline_summary()
            .map_err(js_error)?;
        Ok(vec![below.map_or(2, u8::from), right.map_or(2, u8::from)])
    }

    pub fn set_outline_summary(
        &mut self,
        name: &str,
        below: Option<bool>,
        right: Option<bool>,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .set_outline_summary(below, right)
            .map_err(js_error)
    }

    pub fn default_row_descent(&mut self, name: &str) -> Result<Option<f64>, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .default_row_descent()
            .map_err(js_error)
    }

    pub fn set_default_row_descent(&mut self, name: &str, value: f64) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .set_default_row_descent(value)
            .map_err(js_error)
    }

    pub fn default_dimensions(&mut self, name: &str) -> Result<Vec<f64>, JsError> {
        let (height, width) = self
            .worksheet(name)
            .map_err(js_error)?
            .default_dimensions()
            .map_err(js_error)?;
        Ok(vec![height, width.unwrap_or(f64::NAN)])
    }

    pub fn set_default_dimension(
        &mut self,
        name: &str,
        rows: bool,
        value: Option<f64>,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .set_default_dimension(rows, value)
            .map_err(js_error)
    }

    pub fn outline_levels(&mut self, name: &str) -> Result<Vec<u8>, JsError> {
        let (rows, columns) = self
            .worksheet(name)
            .map_err(js_error)?
            .outline_levels()
            .map_err(js_error)?;
        Ok(vec![rows, columns])
    }

    pub fn set_outline_level(&mut self, name: &str, rows: bool, level: u8) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .set_outline_level(rows, level)
            .map_err(js_error)
    }

    pub fn sheet_layout(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .layout_records()
            .map_err(js_error)
    }

    pub fn column_layout(&mut self, name: &str, column: u32) -> Result<Vec<JsValue>, JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        let (width, hidden, outline) = sheet.column_layout(column).map_err(js_error)?;
        Ok(vec![
            width.map_or(JsValue::UNDEFINED, JsValue::from_f64),
            JsValue::from_bool(hidden),
            JsValue::from_f64(f64::from(outline)),
        ])
    }

    pub fn set_column_layout(
        &mut self,
        name: &str,
        column: u32,
        width: Option<f64>,
        hidden: Option<bool>,
        outline: Option<u8>,
        last: Option<u32>,
    ) -> Result<(), JsError> {
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let last = last.unwrap_or(column);
        if let Some(width) = width {
            sheet
                .set_column_width(column, last, width)
                .map_err(js_error)?;
        }
        if let Some(hidden) = hidden {
            sheet
                .set_column_hidden(column, last, hidden)
                .map_err(js_error)?;
        }
        if let Some(level) = outline {
            let threshold = sheet.outline_levels().map_err(js_error)?.1;
            sheet
                .set_column_outline(column, last, level)
                .map_err(js_error)?;
            if level > threshold {
                sheet
                    .set_outline_level(false, threshold)
                    .map_err(js_error)?;
            }
        }
        Ok(())
    }

    pub fn column_number_format(
        &mut self,
        name: &str,
        column: u32,
    ) -> Result<Option<String>, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .column_number_format(column)
            .map_err(js_error)
    }

    pub fn set_column_number_format(
        &mut self,
        name: &str,
        column: u32,
        code: &str,
    ) -> Result<(), JsError> {
        let previous = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        let style = self
            .workbook
            .register_number_format_style(previous, code)
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_column_style_index(column, column, style)
            .map_err(js_error)
    }

    pub fn column_border(&mut self, name: &str, column: u32) -> Result<Vec<String>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        Ok(self
            .workbook
            .border_of_style(index)
            .map_err(js_error)?
            .map(encode_border)
            .unwrap_or_default())
    }

    pub fn set_column_border(
        &mut self,
        name: &str,
        column: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let border = decode_border(values)?;
        let current = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        let index = self
            .workbook
            .register_border_style(current, border)
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_column_style_index(column, column, index)
            .map_err(js_error)
    }

    pub fn column_fill(&mut self, name: &str, column: u32) -> Result<Vec<String>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        Ok(self
            .workbook
            .fill_of_style(index)
            .map_err(js_error)?
            .map(encode_fill)
            .unwrap_or_default())
    }

    pub fn set_column_fill(
        &mut self,
        name: &str,
        column: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let fill = decode_fill(values)?;
        let current = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        let index = self
            .workbook
            .register_fill_style(current, fill)
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_column_style_index(column, column, index)
            .map_err(js_error)
    }

    pub fn column_protection(&mut self, name: &str, column: u32) -> Result<Vec<u8>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        let protection = self.workbook.protection_of_style(index).map_err(js_error)?;
        Ok(protection
            .map(|p| vec![u8::from(p.locked), u8::from(p.hidden)])
            .unwrap_or_default())
    }

    pub fn set_column_protection(
        &mut self,
        name: &str,
        column: u32,
        locked: bool,
        hidden: bool,
    ) -> Result<(), JsError> {
        let current = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        let index = self
            .workbook
            .register_protection_style(current, ooxml_xlsx::Protection { locked, hidden })
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_column_style_index(column, column, index)
            .map_err(js_error)
    }

    pub fn column_alignment(&mut self, name: &str, column: u32) -> Result<Vec<String>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        let attributes = self.workbook.alignment_of_style(index).map_err(js_error)?;
        Ok(attributes
            .into_iter()
            .flat_map(|(key, value)| [key, value])
            .collect())
    }

    pub fn set_column_alignment(
        &mut self,
        name: &str,
        column: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let alignment = decode_alignment(values)?;
        let current = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        let index = self
            .workbook
            .register_alignment_style(current, alignment)
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_column_style_index(column, column, index)
            .map_err(js_error)
    }

    pub fn column_font(&mut self, name: &str, column: u32) -> Result<Vec<String>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        if index == 0 {
            return Ok(Vec::new());
        }
        Ok(self
            .workbook
            .declared_font_of_style(index)
            .map_err(js_error)?
            .map(encode_declared_font)
            .unwrap_or_default())
    }

    pub fn set_column_font(
        &mut self,
        name: &str,
        column: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let font = decode_font(values)?;
        let current = self
            .worksheet(name)
            .map_err(js_error)?
            .column_style_index(column)
            .map_err(js_error)?;
        let index = self
            .workbook
            .register_font_style(current, font)
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_column_style_index(column, column, index)
            .map_err(js_error)
    }

    pub fn clear_column_style(&mut self, name: &str, column: u32, last: Option<u32>) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .set_column_style_index(column, last.unwrap_or(column), 0)
            .map_err(js_error)
    }

    pub fn clear_column_width(&mut self, name: &str, column: u32, last: Option<u32>) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .clear_column_width(column, last.unwrap_or(column))
            .map_err(js_error)
    }

    pub fn extend_column_style(&mut self, name: &str, first: u32, last: u32) -> Result<(), JsError> {
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let style = sheet.column_style_index(first).map_err(js_error)?;
        sheet.set_column_style_index(first, last, style).map_err(js_error)
    }

    pub fn row_style_index(&mut self, name: &str, row: u32) -> Result<Option<u32>, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)
    }

    pub fn set_row_style_index(
        &mut self,
        name: &str,
        row: u32,
        index: Option<u32>,
    ) -> Result<(), JsError> {
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        match index {
            Some(index) => sheet.set_row_style_index(row, index),
            None => sheet.clear_row_style_index(row),
        }
        .map_err(js_error)
    }

    pub fn row_border(&mut self, name: &str, row: u32) -> Result<Vec<String>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?;
        let Some(index) = index else {
            return Ok(Vec::new());
        };
        Ok(self
            .workbook
            .border_of_style(index)
            .map_err(js_error)?
            .map(encode_border)
            .unwrap_or_default())
    }

    pub fn set_row_border(
        &mut self,
        name: &str,
        row: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let border = decode_border(values)?;
        let current = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?
            .unwrap_or(0);
        let index = self
            .workbook
            .register_border_style(current, border)
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_row_style_index(row, index)
            .map_err(js_error)
    }

    pub fn row_fill(&mut self, name: &str, row: u32) -> Result<Vec<String>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?;
        let Some(index) = index else {
            return Ok(Vec::new());
        };
        Ok(self
            .workbook
            .fill_of_style(index)
            .map_err(js_error)?
            .map(encode_fill)
            .unwrap_or_default())
    }

    pub fn set_row_fill(
        &mut self,
        name: &str,
        row: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let fill = decode_fill(values)?;
        let current = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?
            .unwrap_or(0);
        let index = self
            .workbook
            .register_fill_style(current, fill)
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_row_style_index(row, index)
            .map_err(js_error)
    }

    pub fn row_protection(&mut self, name: &str, row: u32) -> Result<Vec<u8>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?;
        let Some(index) = index else {
            return Ok(Vec::new());
        };
        let protection = self.workbook.protection_of_style(index).map_err(js_error)?;
        Ok(protection
            .map(|p| vec![u8::from(p.locked), u8::from(p.hidden)])
            .unwrap_or_default())
    }

    pub fn set_row_protection(
        &mut self,
        name: &str,
        row: u32,
        locked: bool,
        hidden: bool,
    ) -> Result<(), JsError> {
        let current = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?
            .unwrap_or(0);
        let index = self
            .workbook
            .register_protection_style(current, ooxml_xlsx::Protection { locked, hidden })
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_row_style_index(row, index)
            .map_err(js_error)
    }

    pub fn row_alignment(&mut self, name: &str, row: u32) -> Result<Vec<String>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?;
        let Some(index) = index else {
            return Ok(Vec::new());
        };
        let attributes = self.workbook.alignment_of_style(index).map_err(js_error)?;
        Ok(attributes
            .into_iter()
            .flat_map(|(key, value)| [key, value])
            .collect())
    }

    pub fn set_row_alignment(
        &mut self,
        name: &str,
        row: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let alignment = decode_alignment(values)?;
        let current = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?
            .unwrap_or(0);
        let index = self
            .workbook
            .register_alignment_style(current, alignment)
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_row_style_index(row, index)
            .map_err(js_error)
    }

    pub fn row_font(&mut self, name: &str, row: u32) -> Result<Vec<String>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?;
        // ExcelJS reads format 0 as an unstyled row.
        let Some(index) = index.filter(|&index| index != 0) else {
            return Ok(Vec::new());
        };
        Ok(self
            .workbook
            .declared_font_of_style(index)
            .map_err(js_error)?
            .map(encode_declared_font)
            .unwrap_or_default())
    }

    pub fn set_row_font(
        &mut self,
        name: &str,
        row: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let font = decode_font(values)?;
        let current = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?
            .unwrap_or(0);
        let index = self
            .workbook
            .register_font_style(current, font)
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_row_style_index(row, index)
            .map_err(js_error)
    }

    pub fn row_number_format(&mut self, name: &str, row: u32) -> Result<Option<String>, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .row_number_format(row)
            .map_err(js_error)
    }

    pub fn set_row_number_format(
        &mut self,
        name: &str,
        row: u32,
        code: &str,
    ) -> Result<(), JsError> {
        let previous = self
            .worksheet(name)
            .map_err(js_error)?
            .row_style_index(row)
            .map_err(js_error)?
            .unwrap_or(0);
        let style = self
            .workbook
            .register_number_format_style(previous, code)
            .map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_row_style_index(row, style)
            .map_err(js_error)
    }

    pub fn row_layout(&mut self, name: &str, row: u32) -> Result<Vec<JsValue>, JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        let (height, hidden, outline) = sheet.row_layout(row).map_err(js_error)?;
        Ok(vec![
            height.map_or(JsValue::UNDEFINED, JsValue::from_f64),
            JsValue::from_bool(hidden),
            JsValue::from_f64(f64::from(outline)),
        ])
    }

    pub fn set_row_layout(
        &mut self,
        name: &str,
        row: u32,
        height: Option<f64>,
        hidden: Option<bool>,
        outline: Option<u8>,
    ) -> Result<(), JsError> {
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        if let Some(height) = height {
            sheet.set_row_height(row, height).map_err(js_error)?;
        }
        if let Some(hidden) = hidden {
            sheet.set_row_hidden(row, hidden).map_err(js_error)?;
        }
        if let Some(level) = outline {
            let threshold = sheet.outline_levels().map_err(js_error)?.0;
            sheet.set_row_outline(row, level).map_err(js_error)?;
            if level > threshold {
                sheet.set_outline_level(true, threshold).map_err(js_error)?;
            }
        }
        Ok(())
    }

    pub fn clear_row_height(&mut self, name: &str, row: u32) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .clear_row_height(row)
            .map_err(js_error)
    }

    /// Translates a formula whose master and member are supplied by a streaming writer.
    pub fn translate_streaming_formula(
        &mut self, name: &str, text: &str, from: &str, to: &str,
    ) -> Result<Option<String>, JsError> {
        let from = CellIndex::parse(from).map_err(js_error)?;
        let to = CellIndex::parse(to).map_err(js_error)?;
        let sheet = self.worksheet(name).map_err(js_error)?;
        Ok(sheet.translate_shared_formula(text, from, to))
    }

    pub fn translate_detached_formula(
        text: &str, from: &str, to: &str, names: Option<Vec<String>>,
    ) -> Result<Option<String>, JsError> {
        let from = CellIndex::parse(from).map_err(js_error)?;
        let to = CellIndex::parse(to).map_err(js_error)?;
        Ok(ooxml_xlsx::streaming::translate_shared_formula(text, from, to, names.as_deref().unwrap_or_default()))
    }

    /// Returns groups of four integers: row ID, first column, last column, manual flag.
    pub fn row_page_breaks(&mut self, name: &str) -> Result<Vec<u32>, JsError> {
        let breaks = self.worksheet(name).map_err(js_error)?
            .row_page_breaks().map_err(js_error)?;
        Ok(breaks.into_iter().flat_map(|entry| {
            [entry.id, entry.min, entry.max, u32::from(entry.manual)]
        }).collect())
    }

    pub fn add_row_page_break(
        &mut self,
        name: &str,
        row: u32,
        first: u32,
        last: u32,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .add_row_break_span(row, first, last)
            .map(drop)
            .map_err(js_error)
    }

    pub fn row_numbers(&mut self, name: &str) -> Result<Vec<u32>, JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        Ok(sheet.row_numbers().map_err(js_error)?.collect())
    }

    /// A row count, the one-based row numbers, then one pair per stored or note-only cell.
    /// The pair holds a one-based row and a one-based column with presence in its low bit.
    pub fn worksheet_presence(&mut self, name: &str) -> Result<Vec<u32>, JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        let mut entries = vec![0];
        entries.extend(sheet.row_numbers().map_err(js_error)?);
        let mut notes: std::collections::BTreeSet<_> = sheet.note_positions().map_err(js_error)?.into_iter().collect();
        if !notes.is_empty() {
            entries.extend(notes.iter().map(|at| at.row + 1));
            entries.sort_unstable();
            entries.dedup();
        }
        entries[0] = (entries.len() - 1) as u32;
        for cell in sheet.cells() {
            let cell = cell.map_err(js_error)?;
            let at = cell.index();
            notes.remove(&at);
            let present = cell.formula().is_some()
                || !matches!(cell.value().map_err(js_error)?, CellValue::Empty);
            entries.extend([at.row + 1, ((at.col + 1) << 1) | u32::from(present)]);
        }
        entries.extend(notes.into_iter().flat_map(|at| [at.row + 1, (at.col + 1) << 1]));
        Ok(entries)
    }

    /// Sparse row/column/type/value records for CSV with ordinary values and formula caches.
    /// A null type requests an individual read for the cell. An empty response
    /// lets the adapter use its full row path for structurally complex sheets.
    pub fn csv_scalar_cells(
        &mut self,
        name: &str,
        ignore_hyperlinks: bool,
        ignore_merges: bool,
    ) -> Result<Vec<JsValue>, JsError> {
        let date_offset = if self.workbook.date1904().map_err(js_error)? { 1462.0 } else { 0.0 };
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let guarded = sheet.non_scalar_ranges().map_err(js_error)?;
        let merges = if ignore_merges { Vec::new() } else { sheet.merged_ranges() };
        // Large merged blocks are cheaper to expand a row at a time.
        let followers = merges.iter().map(|range| {
            u64::from(range.last.row - range.first.row + 1)
                * u64::from(range.last.col - range.first.col + 1) - 1
        }).sum::<u64>();
        if followers > 65_536 {
            return Ok(Vec::new());
        }
        let links = if ignore_hyperlinks {
            Vec::new()
        } else {
            sheet.hyperlinks().map_err(js_error)?.into_iter().filter_map(|link| {
                let range = CellRange::parse(&link.range).ok()?;
                Some((range, excel_hyperlink_target(link.target, link.location)))
            }).collect::<Vec<_>>()
        };
        let mut cells = Vec::new();
        for cell in sheet.cells() {
            let cell = cell.map_err(js_error)?;
            let complex = cell.has_complex_value(false).map_err(js_error)?;
            let has_formula = complex && cell.formula().is_some();
            let value = cell.value().map_err(js_error)?;
            if !has_formula && matches!(value, CellValue::Empty) {
                continue;
            }
            if guarded.iter().any(|range| range.contains(cell.index())) {
                cells.push((cell.index(), None));
                continue;
            }
            let simple = !complex || (has_formula && cell.ordinary_formula().is_some_and(|text| !text.is_empty())
                && cell.rich_text().map_err(js_error)?.is_empty());
            let scalar = match value {
                CellValue::Number(number) => number.is_finite(),
                CellValue::Empty | CellValue::String(_) | CellValue::Bool(_) => true,
                _ => false,
            };
            cells.push((cell.index(), (simple && scalar).then_some((value, has_formula))));
        }
        if followers > 0 {
            for range in merges {
                for row in range.first.row..=range.last.row {
                    for col in range.first.col..=range.last.col {
                        let at = CellIndex { row, col };
                        if at != range.first {
                            cells.push((at, None));
                        }
                    }
                }
            }
            cells.sort_unstable_by_key(|(at, _)| *at);
            cells.dedup_by_key(|(at, _)| *at);
        }
        let mut fields = Vec::with_capacity(cells.len() * 4);
        for (at, scalar) in cells {
            fields.extend([JsValue::from(at.row + 1), JsValue::from(at.col)]);
            let Some((value, has_formula)) = scalar else {
                fields.extend([JsValue::NULL, JsValue::UNDEFINED]);
                continue;
            };
            if !has_formula
                && let Some((_, target)) = links.iter().find(|(range, _)| range.contains(at))
                && !target.is_empty()
            {
                fields.extend([JsValue::FALSE, JsValue::from_str(target)]);
                continue;
            }
            let (date, value) = match value {
                CellValue::Number(serial) if sheet.is_date(at.row, at.col).map_err(js_error)? => {
                    if has_formula {
                        fields.extend([JsValue::NULL, JsValue::UNDEFINED]);
                        continue;
                    }
                    (true, JsValue::from_f64(((serial - 25569.0 + date_offset) * 86_400_000.0).round()))
                }
                CellValue::Empty | CellValue::Bool(false) | CellValue::Number(0.0) if has_formula =>
                    (false, JsValue::from_str("")),
                _ => (false, scalar_js(value)),
            };
            fields.extend([JsValue::from_bool(date), value]);
        }
        Ok(fields)
    }

    pub fn sparse_cells(
        &mut self,
        name: &str,
        include_stubs: bool,
        ignore_dimension: bool,
        row_limit: u32,
        include_format_styles: bool,
        include_fill_styles: bool,
    ) -> Result<XlsxSparseCells, JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        // Validates the merge list; merged cells still project their own values.
        sheet.non_scalar_ranges().map_err(js_error)?;
        let ranges = sheet.formula_result_ranges().map_err(js_error)?;
        let arrays = sheet.array_formulas();
        let mut entries = XlsxSparseCells::default();
        let end = if row_limit == 0 { u32::MAX } else { row_limit };
        // Cells outside the declared dimension are projected too; `!ref` is
        // assembled from the declared bound or, without one, from `extent`.
        let mut extent: Option<CellRange> = None;
        for cell in sheet.cells_before_row(end) {
            let cell = cell.map_err(js_error)?;
            let at = cell.index();
            let guarded = ranges.iter().any(|range| range.contains(at));
            let value = if guarded || cell.has_non_scalar_content().map_err(js_error)? {
                JsValue::UNDEFINED
            } else {
                scalar_js(cell.value().map_err(js_error)?)
            };
            if include_stubs || !value.is_null() {
                if !guarded
                    && value.is_undefined()
                    && let Some((iso_date, text)) =
                        sheetjs_special_value(&cell).map_err(js_error)?
                {
                    entries.special_values.extend([
                        JsValue::from(entries.values.len() as u32),
                        JsValue::from_bool(iso_date),
                        JsValue::from_str(&text),
                    ]);
                }
                if value.is_undefined() {
                    let array = guarded
                        .then(|| arrays.iter().position(|(_, range, _, _)| range.contains(at)))
                        .flatten();
                    let formula = if let Some(index) = array {
                        Some(JsValue::from(index as u32))
                    } else if guarded {
                        None
                    } else {
                        sheetjs_formula(&sheet, &cell).map(|text| JsValue::from_str(&text))
                    };
                    if let Some(formula) = formula {
                        entries
                            .formulas
                            .extend([JsValue::from(entries.values.len() as u32), formula]);
                        entries
                            .formulas
                            .extend(sheetjs_formula_result(cell.value().map_err(js_error)?));
                    }
                }
                if let Some(index) = cell.shared_string_index() {
                    entries.shared_strings.extend([entries.values.len() as u32, index]);
                }
                entries.coordinates.extend([at.row, at.col]);
                extent = Some(extent.map_or(CellRange { first: at, last: at }, |range| range.union(at)));
                entries.values.push(value);
                if include_fill_styles {
                    entries
                        .fill_styles
                        .push(cell.style_index().map_err(js_error)?);
                }
                if include_format_styles {
                    entries
                        .format_styles
                        .push(sheet.effective_style_for_cell(&cell).map_err(js_error)?);
                }
            }
        }
        if let Some(mut range) = extent {
            if !ignore_dimension {
                // Every `row` element read counts, as the stored row span.
                let mut rows = sheet.row_numbers().map_err(js_error)?.take_while(|&row| row <= end);
                if let Some(top) = rows.next() {
                    range.first.row = range.first.row.min(top - 1);
                    range.last.row = range.last.row.max(rows.last().unwrap_or(top) - 1);
                }
            }
            entries.extent = vec![range.first.row, range.first.col, range.last.row, range.last.col];
        }
        Ok(entries)
    }

    pub fn read_scalar(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        dates_unsupported: bool,
        allow_error: Option<bool>,
    ) -> Result<JsValue, JsError> {
        let at = CellIndex::new(row, column).map_err(js_error)?;
        self.read_classified_scalar(name, at, dates_unsupported, allow_error.unwrap_or(false))
            .and_then(|value| value.ok_or_else(|| unsupported(name, at))).map_err(js_error)
    }

    /// Probes for an ExcelJS scalar without constructing a JavaScript exception.
    /// NaN is reserved for the typed fallback; stored scalar numbers are finite.
    pub fn probe_excel_scalar(&mut self, name: &str, row: u32, column: u32) -> JsValue {
        CellIndex::new(row, column)
            .and_then(|at| self.read_classified_scalar(name, at, false, true))
            .ok().flatten().unwrap_or_else(|| JsValue::from_f64(f64::NAN))
    }

    pub fn clear_static_array_row(&mut self, name: &str, reference: &str) -> Result<(), JsError> {
        let range = CellRange::parse(reference).map_err(js_error)?;
        self.worksheet(name).map_err(js_error)?.clear_static_array_row(range).map_err(js_error)
    }

    pub fn array_formulas(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .array_formulas()
            .into_iter()
            .flat_map(|(at, range, text, dynamic)| {
                [at.to_string(), range.to_string(), text, dynamic.to_string()]
            })
            .collect())
    }

    pub fn array_result(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Vec<JsValue>, JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        let Some(cell) = sheet.cell(row, column).map_err(js_error)? else {
            return Ok(Vec::new());
        };
        let value = cell.value().map_err(js_error)?;
        Ok(match value {
            CellValue::Empty => Vec::new(),
            CellValue::Error(error) => vec![JsValue::UNDEFINED, JsValue::from_str(&error)],
            CellValue::Date(date) => vec![
                JsValue::UNDEFINED,
                JsValue::UNDEFINED,
                JsValue::from_str(&date),
            ],
            value => vec![scalar_js(value)],
        })
    }

    pub fn read_excel_array_result(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Vec<JsValue>, JsError> {
        let at = CellIndex::new(row, column).map_err(js_error)?;
        let sheet = self.worksheet(name).map_err(js_error)?;
        if !sheet.is_array_result_cell(at) {
            return Ok(Vec::new());
        }
        let Some(cell) = sheet.cell(row, column).map_err(js_error)? else {
            return Ok(vec![JsValue::NULL]);
        };
        if cell.has_complex_content(true).map_err(js_error)?
            && self.read_sheetjs_iso_date(name, row, column)?.is_none()
        {
            return Ok(Vec::new());
        }
        let values = self.array_result(name, row, column)?;
        Ok(if values.is_empty() {
            vec![JsValue::NULL]
        } else {
            values
        })
    }

    pub fn read_sheetjs_iso_date(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Option<String>, JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        let at = CellIndex::new(row, column).map_err(js_error)?;
        sheet_iso_date(&sheet, at)
    }

    pub fn read_excel_iso_date(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Option<String>, JsError> {
        let at = CellIndex::new(row, column).map_err(js_error)?;
        let ranges = self.read_ranges_at(name, at, true).map_err(js_error)?;
        if ranges.iter().any(|range| range.contains(at)) {
            return Ok(None);
        }
        let sheet = self.worksheet(name).map_err(js_error)?;
        sheet_iso_date(&sheet, at)
    }

    pub fn write_iso_date_as_number(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        value: f64,
    ) -> Result<(), JsError> {
        if self.read_sheetjs_iso_date(name, row, column)?.is_none() {
            return Err(JsError::new("expected an existing ISO date cell"));
        }
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_number(value)
            .map_err(js_error)
    }

    pub fn write_iso_date(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        value: &str,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_iso_date(value)
            .map_err(js_error)
    }

    pub fn read_sheetjs_error(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Option<String>, JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        let Some(cell) = sheet.cell(row, column).map_err(js_error)? else {
            return Ok(None);
        };
        if cell.formula().is_some() {
            return Ok(None);
        }
        Ok(match cell.value().map_err(js_error)? {
            CellValue::Error(code) => Some(code),
            _ => None,
        })
    }

    pub fn read_excel_error(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Option<String>, JsError> {
        let at = CellIndex::new(row, column).map_err(js_error)?;
        let ranges = self.read_ranges_at(name, at, true).map_err(js_error)?;
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let value = classified_value(&mut sheet, &ranges, at, false, true).map_err(js_error)?;
        Ok(match value {
            CellValue::Error(code) => Some(code),
            _ => None,
        })
    }

    pub fn read_excel_formula(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Vec<JsValue>, JsError> {
        let at = CellIndex::new(row, column).map_err(js_error)?;
        let ranges = self.read_ranges_at(name, at, true).map_err(js_error)?;
        let sheet = self.worksheet(name).map_err(js_error)?;
        let Some(cell) = sheet.cell(row, column).map_err(js_error)? else {
            return Ok(Vec::new());
        };
        excel_formula_fields(&sheet, at, &ranges, &cell)
    }

    pub fn read_excel_complex(&mut self, name: &str, row: u32, column: u32) -> Result<Vec<JsValue>, JsError> {
        let at = CellIndex::new(row, column).map_err(js_error)?;
        let ranges = self.read_ranges_at(name, at, true).map_err(js_error)?;
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        if let Some(cell) = sheet.cell(row, column).map_err(js_error)? {
            if !ranges.iter().any(|range| range.contains(at))
                && let Some(text) = cell_iso_date(&cell)?
            {
                return Ok(vec![JsValue::from_str("date"), JsValue::from_str(&text)]);
            }
            let rich_text = declared_rich_text_fields(&cell)?;
            if !rich_text.is_empty() {
                return Ok(std::iter::once(JsValue::from_str("richText"))
                    .chain(rich_text.iter().map(|field| JsValue::from_str(field))).collect());
            }
            let mut fields = excel_formula_fields(&sheet, at, &ranges, &cell)?;
            if !fields.is_empty() {
                if let Some(serial) = fields[1].as_f64()
                    && sheet.is_date(row, column).map_err(js_error)?
                {
                    fields.push(JsValue::from_f64(self.excel_serial_millis(serial)?));
                }
                fields.insert(0, JsValue::from_str("formula"));
                return Ok(fields);
            }
        }

        let mut fields = self.read_excel_array_result(name, row, column)?;
        if !fields.is_empty() {
            if let Some(serial) = fields[0].as_f64()
                && let Some(millis) = self.excel_date_millis(name, row, column, serial)?
            {
                fields.resize(3, JsValue::UNDEFINED);
                fields.push(JsValue::from_f64(millis));
            }
            fields.insert(0, JsValue::from_str("arrayResult"));
        }
        Ok(fields)
    }

    /// Clears a complete shared formula group in one edit plan.
    pub fn clear_shared_formula_for_move(
        &mut self, name: &str, row: u32, master_column: u32, members: &[u32],
    ) -> Result<(), JsError> {
        let master = CellIndex::new(row, master_column).map_err(js_error)?;
        self.worksheet(name).map_err(js_error)?.clear_shared_formula_row(master, members).map_err(js_error)
    }

    pub fn write_shared_formula_master(
        &mut self,
        name: &str,
        reference: &str,
        formula: &str,
        value: JsValue,
        result_type: Option<String>,
    ) -> Result<(), JsError> {
        let range = CellRange::parse(reference).map_err(js_error)?;
        let result = decode_formula_result(&value, result_type.as_deref())?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_shared_formula_master(range, formula, result)
            .map_err(js_error)
    }

    pub fn write_shared_formula_member(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        master: &str,
        value: JsValue,
        result_type: Option<String>,
    ) -> Result<(), JsError> {
        let at = CellIndex::new(row, column).map_err(js_error)?;
        let master = CellIndex::parse(master).map_err(js_error)?;
        let result = decode_formula_result(&value, result_type.as_deref())?;
        self.worksheet(name)
            .map_err(js_error)?
            .set_shared_formula_member(at, master, result)
            .map_err(js_error)
    }

    pub fn write_shared_formula(
        &mut self,
        name: &str,
        reference: &str,
        formula: &str,
        values: Vec<JsValue>,
        types: Vec<String>,
    ) -> Result<(), JsError> {
        if values.len() != types.len() {
            return Err(JsError::new("mismatched shared formula results and types"));
        }
        let range = CellRange::parse(reference).map_err(js_error)?;
        let width = (range.last.col - range.first.col + 1) as usize;
        let height = (range.last.row - range.first.row + 1) as usize;
        if width.checked_mul(height) != Some(values.len()) {
            return Err(JsError::new("shared formula results must cover the range"));
        }
        self.worksheet(name)
            .map_err(js_error)?
            .remove_cell_hyperlinks(range)
            .map_err(js_error)?;
        let mut dates = Vec::new();
        let mut date_offset = None;
        let mut results = Vec::with_capacity(values.len());
        for (index, (value, kind)) in values.iter().zip(&types).enumerate() {
            let at = CellIndex {
                row: range.first.row + (index / width) as u32,
                col: range.first.col + (index % width) as u32,
            };
            if kind == "dateMillis" { dates.push(at); }
            let result = self.decode_shared_formula_result(value, kind, &mut date_offset)?;
            results.push(result);
        }
        self.worksheet(name)
            .map_err(js_error)?
            .set_shared_formula(range, formula, results)
            .map_err(js_error)?;
        for at in dates {
            if !self.is_date_cell(name, at.row, at.col)? {
                self.set_number_format(name, at.row, at.col, "mm-dd-yy")?;
            }
        }
        Ok(())
    }

    pub fn write_sparse_shared_formula_row(
        &mut self, name: &str, reference: &str, formula: &str, columns: &[u32],
        values: Vec<JsValue>, types: Vec<String>,
    ) -> Result<(), JsError> {
        let range = CellRange::parse(reference).map_err(js_error)?;
        if values.len() != types.len() || values.len() != columns.len() + 1 {
            return Err(JsError::new("Expected a cached result for the master and each shared member"));
        }
        let mut dates = Vec::new();
        let mut date_offset = None;
        let mut results = std::iter::once(range.first.col).chain(columns.iter().copied())
            .zip(values.iter().zip(&types))
            .map(|(col, (value, kind))| {
                if kind == "dateMillis" { dates.push(col); }
                let result = self.decode_shared_formula_result(value, kind, &mut date_offset)?;
                Ok((col, result))
            })
            .collect::<Result<Vec<_>, JsError>>()?.into_iter();
        let (_, master) = results.next().expect("validated result count");
        self.worksheet(name).map_err(js_error)?
            .set_sparse_shared_formula_row(range, formula, master, results.collect()).map_err(js_error)?;
        for col in dates {
            if !self.is_date_cell(name, range.first.row, col)? {
                self.set_number_format(name, range.first.row, col, "mm-dd-yy")?;
            }
        }
        Ok(())
    }

    /// Writes array followers before registering the array formula at its anchor.
    pub fn write_excel_array_formula(
        &mut self,
        name: &str,
        reference: &str,
        formula: &str,
        values: Vec<JsValue>,
        types: Vec<String>,
        dates: Vec<String>,
    ) -> Result<(), JsError> {
        let range = CellRange::parse(reference).map_err(js_error)?;
        let width = (range.last.col - range.first.col + 1) as usize;
        let height = (range.last.row - range.first.row + 1) as usize;
        if width.checked_mul(height) != Some(values.len())
            || values.len() != types.len()
            || dates.len() != types.iter().skip(1).filter(|kind| *kind == "dateMillis").count()
        {
            return Err(JsError::new("array formula results must cover the range"));
        }
        self.worksheet(name)
            .map_err(js_error)?
            .remove_cell_hyperlinks(range)
            .map_err(js_error)?;
        let mut dates = dates.iter();
        let mut date_offset = None;
        // Follower writes leave the guarded ranges intact until the anchor is written.
        let mut follower_ranges = None;
        for (index, (value, kind)) in values.iter().zip(&types).enumerate().skip(1) {
            let row = range.first.row + (index / width) as u32;
            let col = range.first.col + (index % width) as u32;
            let at = CellIndex { row, col };
            match kind.as_str() {
                "dateMillis" => {
                    let millis = value
                        .as_f64()
                        .ok_or_else(|| JsError::new("expected a valid JavaScript date"))?;
                    let iso = dates.next().expect("validated date count");
                    let existing_iso = {
                        let sheet = self.worksheet(name).map_err(js_error)?;
                        let ranges = array_follower_ranges(&sheet, &mut follower_ranges, at)?;
                        !ranges.iter().any(|range| range.contains(at))
                            && sheet_iso_date(&sheet, at)?.is_some()
                    };
                    if existing_iso {
                        self.write_iso_date(name, row, col, iso)?;
                    } else {
                        let serial = self.excel_date_serial_with_offset(millis, &mut date_offset)?;
                        let mut sheet = self.worksheet(name).map_err(js_error)?;
                        let keep_format = sheet.is_date(row, col).map_err(js_error)?;
                        let ranges = array_follower_ranges(&sheet, &mut follower_ranges, at)?;
                        write_classified_scalar(
                            &mut sheet, &ranges, at, JsValue::from_f64(serial), false, true,
                        )?;
                        if !keep_format {
                            self.set_number_format(name, row, col, "mm-dd-yy")?;
                        }
                    }
                }
                _ => {
                    let error = if kind == "e" {
                        Some(
                            value.as_string().ok_or_else(|| {
                                JsError::new("expected a formula result error code")
                            })?,
                        )
                    } else {
                        None
                    };
                    let mut sheet = self.worksheet(name).map_err(js_error)?;
                    let ranges = array_follower_ranges(&sheet, &mut follower_ranges, at)?;
                    if let Some(code) = error {
                        write_classified_excel_error(&mut sheet, &ranges, at, &code)?;
                    } else {
                        let value = if value.is_undefined() { JsValue::NULL } else { value.clone() };
                        write_classified_scalar(&mut sheet, &ranges, at, value, true, true)?;
                    }
                }
            }
        }
        let anchor = range.first;
        let kind = types[0].as_str();
        let value = if kind == "dateMillis" {
            let millis = values[0]
                .as_f64()
                .ok_or_else(|| JsError::new("expected a valid JavaScript date"))?;
            JsValue::from_f64(self.excel_date_serial_with_offset(millis, &mut date_offset)?)
        } else {
            values[0].clone()
        };
        self.write_array_formula(
            name, reference, formula, false, value, (kind == "e").then(|| kind.to_owned()),
        )?;
        if kind == "dateMillis" && !self.is_date_cell(name, anchor.row, anchor.col)? {
            self.set_number_format(name, anchor.row, anchor.col, "mm-dd-yy")?;
        }
        Ok(())
    }

    pub fn write_array_formula(
        &mut self,
        name: &str,
        reference: &str,
        formula: &str,
        dynamic: bool,
        value: JsValue,
        result_type: Option<String>,
    ) -> Result<(), JsError> {
        let result = decode_formula_result(&value, result_type.as_deref())?;
        let range = CellRange::parse(reference).map_err(js_error)?;
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(range.first.row, range.first.col)
            .map_err(js_error)?
            .set_array_formula_with_result(formula, range, dynamic, result)
            .map_err(js_error)
    }

    pub fn write_formula_with_result(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        formula: Option<String>,
        value: JsValue,
        result_type: Option<String>,
    ) -> Result<(), JsError> {
        let result = decode_formula_result(&value, result_type.as_deref())?;
        let at = CellIndex::new(row, column).map_err(js_error)?;
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let ranges = classify_cell(&sheet, at, true).map_err(js_error)?;
        check_excel_edit(&mut sheet, &ranges, at, true).map_err(js_error)?;
        let formula = match formula {
            Some(text) => text,
            None => sheet
                .cell(row, column)
                .map_err(js_error)?
                .and_then(|cell| cell.ordinary_formula())
                .ok_or_else(|| JsError::new("expected an existing ordinary formula"))?,
        };
        sheet
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_formula_with_result(&formula, result)
            .map_err(js_error)
    }

    pub fn write_formula(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        formula: &str,
        value: JsValue,
        result_type: Option<String>,
    ) -> Result<(), JsError> {
        let result = decode_formula_result(&value, result_type.as_deref())?;
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_formula_with_result(formula, result)
            .map_err(js_error)
    }

    pub fn write_excel_error(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        code: &str,
    ) -> Result<(), JsError> {
        let at = CellIndex::new(row, column).map_err(js_error)?;
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let ranges = classify_cell(&sheet, at, true).map_err(js_error)?;
        write_classified_excel_error(&mut sheet, &ranges, at, code)
    }

    pub fn set_date1904(&mut self, enabled: bool) -> Result<(), JsError> {
        self.workbook.set_date1904(enabled).map_err(js_error)
    }

    pub fn date1904(&mut self) -> Result<bool, JsError> {
        self.workbook.date1904().map_err(js_error)
    }

    pub fn is_date_cell(&mut self, name: &str, row: u32, column: u32) -> Result<bool, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .is_date(row, column)
            .map_err(js_error)
    }

    pub fn excel_date_millis(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        serial: f64,
    ) -> Result<Option<f64>, JsError> {
        if !self
            .worksheet(name)
            .map_err(js_error)?
            .is_date(row, column)
            .map_err(js_error)?
        {
            return Ok(None);
        }
        self.excel_serial_millis(serial).map(Some)
    }

    pub fn excel_date_serial(&mut self, millis: f64) -> Result<f64, JsError> {
        self.excel_date_serial_with_offset(millis, &mut None)
    }

    pub fn write_excel_date(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        millis: f64,
    ) -> Result<(), JsError> {
        let serial = self.excel_date_serial(millis)?;
        let keep_format = self
            .worksheet(name)
            .map_err(js_error)?
            .is_date(row, column)
            .map_err(js_error)?;
        self.write_scalar(
            name,
            row,
            column,
            JsValue::from_f64(serial),
            false,
            Some(true),
        )?;
        if !keep_format {
            self.set_number_format(name, row, column, "mm-dd-yy")?;
        }
        Ok(())
    }

    pub fn sheet_hyperlinks(&mut self, name: &str, include_display: Option<bool>) -> Result<Vec<String>, JsError> {
        let mut values = Vec::new();
        for link in self
            .worksheet(name)
            .map_err(js_error)?
            .hyperlinks()
            .map_err(js_error)?
        {
            let mut target = link.target.unwrap_or_default();
            if let Some(location) = link.location {
                target.push('#');
                target.push_str(&location);
            }
            if !target.is_empty() {
                values.extend([link.range, target, link.tooltip.unwrap_or_default()]);
                if include_display == Some(true) {
                    values.push(link.display.map(|display| format!("1{display}")).unwrap_or_default());
                }
            }
        }
        Ok(values)
    }

    pub fn cell_hyperlink(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Vec<String>, JsError> {
        let cell = CellIndex { row, col: column };
        let links = self
            .worksheet(name)
            .map_err(js_error)?
            .hyperlinks()
            .map_err(js_error)?;
        let Some(link) = links
            .into_iter()
            .find(|link| CellRange::parse(&link.range).is_ok_and(|range| range.contains(cell)))
        else {
            return Ok(Vec::new());
        };
        let target = excel_hyperlink_target(link.target, link.location);
        if target.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![
            target,
            link.tooltip.unwrap_or_default(),
            link.display.unwrap_or_default(),
        ])
    }

    pub fn set_cell_hyperlink(
        &mut self,
        name: &str,
        reference: &str,
        target: &str,
        text: Option<String>,
        tooltip: Option<String>,
        display: Option<String>,
    ) -> Result<(), JsError> {
        use ooxml_xlsx::{Link, LinkOptions};
        let link = if let Some(location) = target.strip_prefix('#') {
            Link::Location(location.to_owned())
        } else if target.split_once(':').is_some_and(|(scheme, _)| {
            scheme.len() > 1
                && scheme
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
        }) {
            Link::Url(target.to_owned())
        } else {
            Link::File(target.to_owned())
        };
        self.worksheet(name)
            .map_err(js_error)?
            .set_hyperlink(
                reference,
                &link,
                &LinkOptions {
                    text,
                    tooltip,
                    display,
                },
            )
            .map_err(js_error)
    }

    pub fn set_cell_hyperlink_tooltip(
        &mut self,
        name: &str,
        reference: &str,
        tooltip: Option<String>,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .set_hyperlink_tooltip(reference, tooltip.as_deref())
            .map_err(js_error)
    }

    pub fn clear_cell_hyperlink(&mut self, name: &str, reference: &str) -> Result<(), JsError> {
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        match sheet.remove_hyperlink(reference) {
            Ok(()) | Err(Error::NoSuchRule { .. }) => Ok(()),
            Err(error) => Err(js_error(error)),
        }
    }

    pub fn cell_border(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Vec<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .cell_border(row, column)
            .map_err(js_error)?
            .map(encode_border)
            .unwrap_or_default())
    }

    pub fn set_cell_border(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let border = decode_border(values)?;
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_border(&border)
            .map_err(js_error)
    }

    pub fn cell_fill(&mut self, name: &str, row: u32, column: u32) -> Result<Vec<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .cell_fill(row, column)
            .map_err(js_error)?
            .map(encode_fill)
            .unwrap_or_default())
    }

    pub fn sheetjs_theme_colors(&mut self) -> Vec<String> {
        self.workbook
            .theme_colors()
            .map(|palette| palette.into_iter().map(Option::unwrap_or_default).collect())
            .unwrap_or_default()
    }

    pub fn sheetjs_style_fill(&mut self, style: u32) -> Result<Vec<String>, JsError> {
        Ok(self
            .workbook
            .fill_of_style_including_default(style)
            .map_err(js_error)?
            .map(encode_fill)
            .unwrap_or_default())
    }

    pub fn set_cell_fill(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let fill = decode_fill(values)?;
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_fill(&fill)
            .map_err(js_error)
    }

    pub fn cell_font(&mut self, name: &str, row: u32, column: u32) -> Result<Vec<String>, JsError> {
        let index = self
            .worksheet(name)
            .map_err(js_error)?
            .effective_cell_style_index(row, column)
            .map_err(js_error)?;
        // ExcelJS reads format 0 as an unstyled cell.
        let Some(index) = index.filter(|&index| index != 0) else {
            return Ok(Vec::new());
        };
        Ok(self
            .workbook
            .declared_font_of_style(index)
            .map_err(js_error)?
            .map(encode_declared_font)
            .unwrap_or_default())
    }

    pub fn set_cell_font(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let font = decode_font(values)?;
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_font(&font)
            .map_err(js_error)
    }

    pub fn cell_validation(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Vec<String>, JsError> {
        let values = self
            .worksheet(name)
            .map_err(js_error)?
            .validation_attributes(CellIndex { row, col: column })
            .map_err(js_error)?;
        Ok(values
            .into_iter()
            .flat_map(|(key, value)| [key, value])
            .collect())
    }

    pub fn validation_rules(&mut self, name: &str) -> Result<Vec<String>, JsError> {
        let rules = self.worksheet(name).map_err(js_error)?.validation_rules().map_err(js_error)?;
        Ok(rules.into_iter().flat_map(|rule| {
            [rule.reference, rule.attributes.len().to_string()].into_iter()
                .chain(rule.attributes.into_iter().flat_map(|(key, value)| [key, value]))
        }).collect())
    }

    pub fn range_validation(
        &mut self,
        name: &str,
        reference: &str,
    ) -> Result<Vec<String>, JsError> {
        let values = self
            .worksheet(name)
            .map_err(js_error)?
            .validation_attributes_for(reference)
            .map_err(js_error)?;
        Ok(values
            .into_iter()
            .flat_map(|(key, value)| [key, value])
            .collect())
    }

    pub fn set_cell_validation(
        &mut self,
        name: &str,
        reference: &str,
        mut values: Vec<String>,
        split: bool,
        first_date: Option<f64>,
        second_date: Option<f64>,
    ) -> Result<(), JsError> {
        use ooxml_xlsx::{
            ErrorStyle, Validation, ValidationOperator as Op, ValidationType as Type,
        };
        let mut date_offset = None;
        for (index, millis) in [(2, first_date), (3, second_date)] {
            if let Some(millis) = millis {
                let value = values.get_mut(index)
                    .ok_or_else(|| JsError::new("Missing validation formula for date"))?;
                *value = self.excel_date_serial_with_offset(millis, &mut date_offset)?.to_string();
            }
        }
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let remove = |sheet: &mut Worksheet<'_>| match sheet.remove_validation(reference) {
            Ok(()) => Ok(()),
            Err(Error::NoSuchRule { .. }) if split => {
                let at = CellIndex::parse(reference).map_err(js_error)?;
                sheet.remove_validation_at(at).map_err(js_error)
            }
            Err(Error::NoSuchRule { .. }) => Ok(()),
            Err(error) => Err(js_error(error)),
        };
        if values.is_empty() {
            return remove(&mut sheet);
        }
        let [
            kind,
            operator,
            formula1,
            formula2,
            blank,
            show_input,
            prompt_title,
            prompt,
            show_error,
            error_title,
            error,
            style,
        ] = <[String; 12]>::try_from(values)
            .map_err(|_| JsError::new("Expected twelve validation entries"))?;
        let mut validation = match kind.as_str() {
            "any" => Validation::any(),
            "list" => Validation::list_from(&formula1),
            "custom" => Validation::custom(&formula1),
            _ => {
                let kind = match kind.as_str() {
                    "whole" => Type::Whole,
                    "decimal" => Type::Decimal,
                    "date" => Type::Date,
                    "time" => Type::Time,
                    "textLength" => Type::TextLength,
                    _ => return Err(JsError::new("Invalid validation type")),
                };
                let op = match operator.as_str() {
                    "between" => Op::Between,
                    "notBetween" => Op::NotBetween,
                    "equal" => Op::Equal,
                    "notEqual" => Op::NotEqual,
                    "lessThan" => Op::LessThan,
                    "lessThanOrEqual" => Op::LessThanOrEqual,
                    "greaterThan" => Op::GreaterThan,
                    "greaterThanOrEqual" => Op::GreaterThanOrEqual,
                    _ => return Err(JsError::new("Invalid validation operator")),
                };
                Validation::compare(
                    kind,
                    op,
                    &formula1,
                    (!formula2.is_empty()).then_some(formula2.as_str()),
                )
            }
        }
        .allow_blank(blank == "true");
        if !prompt_title.is_empty() || !prompt.is_empty() {
            validation = validation.input_message(&prompt_title, &prompt);
        }
        if !error_title.is_empty() || !error.is_empty() || style != "stop" {
            let style = match style.as_str() {
                "warning" => ErrorStyle::Warning,
                "information" => ErrorStyle::Information,
                _ => ErrorStyle::Stop,
            };
            validation = validation.error_message(&error_title, &error, style);
        }
        validation = validation.show_messages(show_input == "true", show_error == "true");
        remove(&mut sheet)?;
        sheet
            .add_validation(reference, &validation)
            .map_err(js_error)
    }

    /// Each legacy note as the cell reference, author, text, stored `text`
    /// markup (or undefined), run count and 14 [`encode_declared_rich_text_runs`]
    /// fields per run (no runs for plain text).
    pub fn sheet_notes(&mut self, name: &str) -> Result<Vec<JsValue>, JsError> {
        let notes = self.worksheet(name).map_err(js_error)?.note_sources().map_err(js_error)?;
        let mut fields = Vec::new();
        for note in notes {
            let runs = encode_declared_rich_text_runs(note.runs.unwrap_or_default());
            fields.extend([
                JsValue::from_str(&note.at.to_string()),
                JsValue::from_str(&note.author),
                JsValue::from_str(&note.text),
                note.text_xml.as_deref().map_or(JsValue::UNDEFINED, JsValue::from_str),
                JsValue::from(runs.len() as u32 / 14),
            ]);
            fields.extend(runs.iter().map(|field| JsValue::from_str(field)));
        }
        Ok(fields)
    }

    /// Pairs of cell reference and visibility for the sheet's VML note shapes.
    pub fn note_visibility(&mut self, name: &str) -> Result<Vec<JsValue>, JsError> {
        Ok(self.worksheet(name).map_err(js_error)?.note_visibility().map_err(js_error)?
            .into_iter()
            .flat_map(|(at, visible)| [JsValue::from_str(&at.to_string()), JsValue::from_bool(visible)])
            .collect())
    }

    /// The stored XML of each requested shared-string item, or undefined.
    pub fn shared_string_items_xml(&self, indices: Vec<u32>) -> Result<Vec<JsValue>, JsError> {
        Ok(self.workbook.shared_string_items_xml(&indices).map_err(js_error)?
            .into_iter()
            .map(|xml| xml.as_deref().map_or(JsValue::UNDEFINED, JsValue::from_str))
            .collect())
    }

    pub fn row_notes(&mut self, name: &str, row: u32) -> Result<Vec<String>, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .row_notes(row)
            .map_err(js_error)?
            .into_iter()
            .flat_map(|(at, author, text)| [at.to_string(), author, text])
            .collect())
    }

    pub fn row_note_columns(&mut self, name: &str, row: u32) -> Result<Vec<u32>, JsError> {
        Ok(self.worksheet(name).map_err(js_error)?.row_note_positions(row).map_err(js_error)?
            .into_iter().map(|at| at.col).collect())
    }

    /// Applies a legacy note splice to an isolated workbook edit candidate.
    pub fn splice_row_notes(
        &mut self, name: &str, row: u32, start: u32, removed: u32, inserted: u32,
    ) -> Result<(), JsError> {
        use ooxml_xlsx::Note;

        CellIndex::new(row, start).map_err(js_error)?;
        let end = start.checked_add(removed).ok_or_else(|| JsError::new("Invalid note splice range"))?;
        let delta = i64::from(inserted) - i64::from(removed);
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let mut removals = Vec::new();
        let mut moves = Vec::new();
        for (at, author, text) in sheet.row_notes(row).map_err(js_error)? {
            if at.col < start || (delta == 0 && at.col >= end) { continue; }
            removals.push(at);
            if at.col < end { continue; }
            let col = u32::try_from(i64::from(at.col) + delta).map_err(|_| JsError::new("Invalid note splice column"))?;
            let target = CellIndex::new(row, col).map_err(js_error)?;
            let runs = sheet.note_runs(at).map_err(js_error)?;
            let mut note = runs.map_or_else(|| Note::new(text), Note::rich_text).author(author);
            if let Some(settings) = decode_moved_note_box(sheet.note_box_fields(at).map_err(js_error)?)? {
                note = note.box_settings(settings);
            }
            moves.push((target, note));
        }
        for at in removals {
            sheet.remove_note(at).map_err(js_error)?;
        }
        for (at, note) in moves {
            sheet.add_note(at, note).map_err(js_error)?;
        }
        Ok(())
    }

    pub fn cell_note(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Option<String>, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .note_text(CellIndex { row, col: column })
            .map_err(js_error)
    }

    /// Note runs as 14 fields each: the text, the run font and its declared-field bits.
    pub fn cell_note_runs(
        &mut self, name: &str, row: u32, column: u32,
    ) -> Result<Option<Vec<String>>, JsError> {
        let runs = self.worksheet(name).map_err(js_error)?
            .declared_note_runs(CellIndex { row, col: column }).map_err(js_error)?;
        Ok(runs.map(encode_declared_rich_text_runs))
    }

    pub fn cell_note_box(
        &mut self, name: &str, row: u32, column: u32,
    ) -> Result<Vec<String>, JsError> {
        self.worksheet(name).map_err(js_error)?.note_box_fields(CellIndex { row, col: column }).map_err(js_error)
    }

    pub fn set_cell_note(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        text: Option<String>,
    ) -> Result<(), JsError> {
        let at = CellIndex { row, col: column };
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        match text {
            Some(text) => match sheet.set_note_text(at, &text) {
                Err(Error::NoSuchNote) => sheet.add_note(at, ooxml_xlsx::Note::new(text)),
                result => result,
            },
            None => match sheet.remove_note(at) {
                Err(Error::NoSuchNote) => Ok(()),
                result => result,
            },
        }
        .map_err(js_error)
    }

    pub fn set_cell_note_runs(
        &mut self, name: &str, row: u32, column: u32, values: Vec<String>, box_fields: Vec<String>,
    ) -> Result<(), JsError> {
        use ooxml_xlsx::Note;

        let runs = decode_rich_text_runs(&values)?;
        let settings = decode_note_box(&box_fields)?;
        let at = CellIndex { row, col: column };
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let result = match &settings {
            Some(settings) => sheet.set_note_model(at, &runs, settings),
            None => sheet.set_note_runs(at, &runs),
        };
        match result {
            Err(Error::NoSuchNote) => {
                let note = Note::rich_text(runs);
                let note = match settings {
                    Some(settings) => note.box_settings(settings),
                    None => note,
                };
                sheet.add_note(at, note)
            }
            result => result,
        }.map_err(js_error)
    }

    pub fn add_cell_note(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        author: &str,
        text: &str,
    ) -> Result<(), JsError> {
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        sheet
            .add_note(
                CellIndex { row, col: column },
                ooxml_xlsx::Note::new(text).author(author),
            )
            .map_err(js_error)
    }

    pub fn set_sheetjs_comments(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        fields: Vec<String>,
    ) -> Result<(), JsError> {
        let (pairs, remainder) = fields.as_chunks::<2>();
        if !remainder.is_empty() {
            return Err(JsError::new("Expected comment author and text pairs"));
        }
        let notes: Vec<_> = pairs
            .iter()
            .map(|[author, text]| ooxml_xlsx::Note::new(text).author(author))
            .collect();
        self.worksheet(name)
            .map_err(js_error)?
            .replace_legacy_notes(CellIndex { row, col: column }, &notes)
            .map_err(js_error)
    }

    /// The text of a shared or inline string cell followed by its runs as
    /// [`encode_declared_rich_text_runs`] fields: no runs for plain text
    /// (phonetic data included) or when a run font cannot be read; empty
    /// for any other cell.
    pub fn sheetjs_string_cell(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Vec<String>, JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        let Some(cell) = sheet.cell(row, column).map_err(js_error)? else {
            return Ok(Vec::new());
        };
        let CellValue::String(text) = cell.value().map_err(js_error)? else {
            return Ok(Vec::new());
        };
        let runs = cell.declared_rich_text().unwrap_or_default();
        Ok(std::iter::once(text).chain(encode_declared_rich_text_runs(runs)).collect())
    }

    pub fn set_cell_rich_text(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let runs = decode_rich_text_runs(&values)?;
        let borrowed: Vec<_> = runs.iter().map(|(text, font)| (font.as_ref(), text.as_str())).collect();
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_rich_text(&borrowed)
            .map_err(js_error)
    }

    pub fn cell_alignment(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Vec<String>, JsError> {
        let attributes = self
            .worksheet(name)
            .map_err(js_error)?
            .cell_alignment(row, column)
            .map_err(js_error)?;
        Ok(attributes
            .into_iter()
            .flat_map(|(key, value)| [key, value])
            .collect())
    }

    pub fn set_cell_alignment(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        values: Vec<String>,
    ) -> Result<(), JsError> {
        let alignment = decode_alignment(values)?;
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_alignment(alignment)
            .map_err(js_error)
    }

    pub fn cell_protection(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Vec<u8>, JsError> {
        let protection = self
            .worksheet(name)
            .map_err(js_error)?
            .cell_protection(row, column)
            .map_err(js_error)?;
        Ok(protection
            .map(|p| vec![u8::from(p.locked), u8::from(p.hidden)])
            .unwrap_or_default())
    }

    pub fn set_cell_protection(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        locked: bool,
        hidden: bool,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_protection(ooxml_xlsx::Protection { locked, hidden })
            .map_err(js_error)
    }

    pub fn number_format(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
    ) -> Result<Option<String>, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .number_format(row, column)
            .map_err(js_error)
    }

    pub fn sheetjs_number_format(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        date_nf: Option<String>,
    ) -> Result<String, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .number_format_with_date_override(
                row,
                column,
                Some(date_nf.as_deref().unwrap_or("m/d/yy")),
            )
            .map_err(js_error)?
            .unwrap_or_else(|| "General".into()))
    }

    pub fn is_date_style(&mut self, name: &str, style: u32) -> Result<bool, JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .is_date_style(style)
            .map_err(js_error)
    }

    pub fn sheetjs_number_format_for_style(
        &mut self,
        name: &str,
        style: u32,
        date_nf: Option<String>,
    ) -> Result<String, JsError> {
        Ok(self
            .worksheet(name)
            .map_err(js_error)?
            .number_format_for_style(style, Some(date_nf.as_deref().unwrap_or("m/d/yy")))
            .map_err(js_error)?
            .unwrap_or_else(|| "General".into()))
    }

    pub fn register_streaming_style(&mut self, fields: Vec<String>) -> Result<u32, JsError> {
        let format = decode_table_column_style(&fields)?;
        self.workbook.register_streaming_style(format).map_err(js_error)
    }

    pub fn cell_style_index(&mut self, name: &str, row: u32, column: u32) -> Result<u32, JsError> {
        let sheet = self.worksheet(name).map_err(js_error)?;
        Ok(sheet
            .cell(row, column)
            .map_err(js_error)?
            .map(|cell| cell.style_index())
            .transpose()
            .map_err(js_error)?
            .unwrap_or(0))
    }

    pub fn set_cell_style_index(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        style: u32,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_style_index(style)
            .map_err(js_error)
    }

    pub fn set_number_format(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        code: &str,
    ) -> Result<(), JsError> {
        self.worksheet(name)
            .map_err(js_error)?
            .cell_mut(row, column)
            .map_err(js_error)?
            .set_num_format(code)
            .map_err(js_error)
    }

    pub fn write_scalar(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        value: JsValue,
        dates_unsupported: bool,
        excel_values: Option<bool>,
    ) -> Result<(), JsError> {
        let at = CellIndex::new(row, column).map_err(js_error)?;
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let ranges = classify_cell(&sheet, at, excel_values.unwrap_or(false)).map_err(js_error)?;
        write_classified_scalar(
            &mut sheet,
            &ranges,
            at,
            value,
            dates_unsupported,
            excel_values.unwrap_or(false),
        )
    }

    pub fn prune_empty_cells(&mut self, name: &str, row: u32, columns: &[u32]) -> Result<(), JsError> {
        let positions = columns.iter().map(|&column| CellIndex::new(row, column))
            .collect::<Result<Vec<_>, _>>().map_err(js_error)?;
        self.worksheet(name).map_err(js_error)?.prune_empty_cells(&positions).map_err(js_error)
    }

    pub fn write_excel_scalar_batch(
        &mut self,
        name: &str,
        coordinates: &[u32],
        values: Vec<JsValue>,
        styles: Option<Vec<u32>>,
    ) -> Result<(), JsError> {
        let (positions, remainder) = coordinates.as_chunks::<2>();
        if !remainder.is_empty() || positions.len() != values.len() {
            return Err(JsError::new("mismatched cell positions and values"));
        }
        if styles.as_ref().is_some_and(|styles| styles.len() != values.len()) {
            return Err(JsError::new("mismatched cell positions and styles"));
        }
        let values = values.iter().map(decode_scalar_cell).collect::<Result<Vec<_>, _>>()?;
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let guarded = classify_sheet(&sheet).map_err(js_error)?;
        let mut edits = Vec::with_capacity(values.len());
        let mut linked = std::collections::HashSet::with_capacity(values.len());
        let mut edit_styles = Vec::new();
        for (index, (&[row, column], value)) in positions.iter().zip(&values).enumerate() {
            let at = CellIndex::new(row, column).map_err(js_error)?;
            let write = scalar_cell_write(value);
            let empty = check_excel_edit(&mut sheet, &guarded, at, true).map_err(js_error)?;
            let style = if let Some(styles) = &styles {
                let current = sheet.cell(row, column).map_err(js_error)?
                    .map(|cell| cell.style_index()).transpose().map_err(js_error)?.unwrap_or(0);
                (current != styles[index]).then_some(styles[index])
            } else {
                None
            };
            if write != CellWrite::Clear || !empty || style.is_some() {
                edits.push((at, write));
                if styles.is_some() { edit_styles.push(style); }
            }
            linked.insert(at);
        }
        if styles.is_some() {
            sheet.write_cells_with_styles(&edits, &edit_styles).map_err(js_error)?;
        } else {
            sheet.write_cells(&edits).map_err(js_error)?;
        }
        sheet.remove_cell_hyperlinks_at(&linked).map_err(js_error)
    }

    pub fn write_scalar_batch(
        &mut self,
        name: &str,
        coordinates: &[u32],
        values: Vec<JsValue>,
        allow_extent_growth: Option<bool>,
    ) -> Result<(), JsError> {
        let mut sheet = self.worksheet(name).map_err(js_error)?;
        let range = if allow_extent_growth.unwrap_or(false) {
            None
        } else {
            // The bound the SheetJS projection uses for its original `!ref`:
            // the declared extent, or the stored rows and cells without one.
            Some(match sheet.declared_bound() {
                Some(range) => range,
                None => sheet
                    .stored_extent()
                    .map_err(js_error)?
                    .ok_or_else(|| JsError::new("no worksheet extent"))?,
            })
        };
        let (positions, remainder) = coordinates.as_chunks::<2>();
        if !remainder.is_empty() || positions.len() != values.len() {
            return Err(JsError::new("mismatched cell positions and values"));
        }
        let ranges = classify_sheet(&sheet).map_err(js_error)?;
        let mut entries = Vec::with_capacity(values.len());
        for (&[row, column], value) in positions.iter().zip(values) {
            let at = CellIndex::new(row, column).map_err(js_error)?;
            if range.is_some_and(|range| !range.contains(at)) {
                return Err(JsError::new(
                    "cell is outside the declared worksheet extent",
                ));
            }
            classified_value(&mut sheet, &ranges, at, false, false).map_err(js_error)?;
            entries.push((at, decode_scalar_cell(&value)?));
        }
        let writes: Vec<_> = entries
            .iter()
            .map(|(at, value)| (*at, scalar_cell_write(value)))
            .collect();
        sheet.write_cells(&writes).map_err(js_error)
    }

    #[cfg(feature = "fault-injection")]
    pub fn inject_apply_failure(&mut self, step: &str) -> Result<(), JsError> {
        use ooxml_xlsx::ApplyStep;

        let step = match step {
            "registry" => ApplyStep::Registries,
            "target" => ApplyStep::Target,
            _ => return Err(JsError::new("unknown apply step")),
        };
        self.workbook.inject_apply_failure(step);
        Ok(())
    }

    pub fn to_bytes_with_compression(&self, compressed: bool) -> Result<Vec<u8>, JsError> {
        self.workbook
            .to_bytes_with_compression(compressed)
            .map_err(js_error)
    }

    pub fn to_bytes_with_deflate_level(&self, level: u8) -> Result<Vec<u8>, JsError> {
        self.workbook
            .to_bytes_with_deflate_level(level)
            .map_err(js_error)
    }

    pub fn to_bytes_with_string_storage(
        &self,
        shared: bool,
        plain_as_value: bool,
        compression: Option<bool>,
        level: Option<u8>,
    ) -> Result<Vec<u8>, JsError> {
        self.workbook
            .to_bytes_with_string_storage(shared, plain_as_value, compression, level)
            .map_err(js_error)
    }

    pub fn to_bytes_without_cell_styles(
        &self,
        use_shared_strings: Option<bool>,
        compression: Option<bool>,
        level: Option<u8>,
    ) -> Result<Vec<u8>, JsError> {
        self.workbook
            .to_bytes_without_cell_styles(use_shared_strings, compression, level)
            .map_err(js_error)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, JsError> {
        self.workbook.to_bytes().map_err(js_error)
    }
}

impl XlsxDocument {
    fn decode_shared_formula_result(
        &mut self, value: &JsValue, kind: &str, date_offset: &mut Option<f64>,
    ) -> Result<CellValue, JsError> {
        if kind == "dateMillis" {
            let millis = value.as_f64().ok_or_else(|| JsError::new("expected a valid JavaScript date"))?;
            Ok(CellValue::Number(self.excel_date_serial_with_offset(millis, date_offset)?))
        } else {
            decode_formula_result(value, (!kind.is_empty()).then_some(kind))
        }
    }

    fn excel_date_serial_with_offset(
        &mut self,
        millis: f64,
        cached_offset: &mut Option<f64>,
    ) -> Result<f64, JsError> {
        if !millis.is_finite() {
            return Err(JsError::new("expected a valid JavaScript date"));
        }
        let offset = match *cached_offset {
            Some(offset) => offset,
            None => {
                let offset = if self.workbook.date1904().map_err(js_error)? {
                    1462.0
                } else {
                    0.0
                };
                *cached_offset = Some(offset);
                offset
            }
        };
        Ok(excel_serial_from_millis(millis, offset))
    }

    fn excel_serial_millis(&mut self, serial: f64) -> Result<f64, JsError> {
        let offset = if self.workbook.date1904().map_err(js_error)? {
            1462.0
        } else {
            0.0
        };
        Ok(((serial - 25569.0 + offset) * 86_400_000.0).round())
    }

    fn read_classified_scalar(
        &mut self,
        name: &str,
        at: CellIndex,
        dates_unsupported: bool,
        allow_error: bool,
    ) -> Result<Option<JsValue>, Error> {
        let ranges = self.read_ranges_at(name, at, allow_error)?;
        let mut sheet = self.worksheet(name)?;
        classify_scalar_value(&mut sheet, &ranges, at, dates_unsupported, allow_error)
            .map(|value| value.map(scalar_js))
    }

    fn read_ranges_at(
        &mut self,
        name: &str,
        at: CellIndex,
        allow_merge_anchor: bool,
    ) -> Result<std::sync::Arc<Vec<CellRange>>, Error> {
        let (mut ranges, merged_count) = self.scalar_read_ranges(name)?;
        if allow_merge_anchor
            && let Some(index) = ranges[..merged_count].iter().position(|range| range.first == at)
        {
            std::sync::Arc::make_mut(&mut ranges).remove(index);
        }
        Ok(ranges)
    }

    fn scalar_read_ranges(&mut self, name: &str) -> Result<(std::sync::Arc<Vec<CellRange>>, usize), Error> {
        let generation = self.workbook.package().generation();
        if self.scalar_read_cache.generation != generation {
            self.scalar_read_cache.sheets.clear();
            self.scalar_read_cache.generation = generation;
        }
        if let Some(cached) = self.scalar_read_cache.sheets.get(name) {
            return Ok(cached.clone());
        }
        let sheet = self.worksheet(name)?;
        let ranges = std::sync::Arc::new(classify_sheet(&sheet)?);
        let merged_count = sheet.merged_ranges().len();
        let entry = (ranges, merged_count);
        self.scalar_read_cache.sheets.insert(name.to_owned(), entry.clone());
        Ok(entry)
    }

    fn worksheet(&mut self, name: &str) -> Result<Worksheet<'_>, Error> {
        let position = self
            .workbook
            .sheet_names()
            .position(|candidate| candidate == name)
            .ok_or_else(|| Error::NoSuchSheet(name.to_owned()))?;
        self.workbook.worksheet_by_index(position)
    }
}

fn classify_sheet(sheet: &Worksheet<'_>) -> Result<Vec<CellRange>, Error> {
    for cell in sheet.cells() {
        cell?;
    }
    sheet.non_scalar_ranges()
}

fn classify_cell(
    sheet: &Worksheet<'_>,
    at: CellIndex,
    allow_merge_anchor: bool,
) -> Result<Vec<CellRange>, Error> {
    let mut ranges = classify_sheet(sheet)?;
    if allow_merge_anchor {
        for (index, range) in sheet.merged_ranges().iter().enumerate() {
            if range.first == at {
                ranges.remove(index);
                break;
            }
        }
    }
    Ok(ranges)
}

fn excel_hyperlink_target(target: Option<String>, location: Option<String>) -> String {
    ooxml_xlsx::hyperlink_address(target.as_deref(), location.as_deref())
}

/// The ExcelJS hyperlink address for a resolved relationship target and a
/// `location` (the streaming reader resolves the two separately).
#[wasm_bindgen]
pub fn excel_hyperlink_address(target: Option<String>, location: Option<String>) -> String {
    excel_hyperlink_target(target, location)
}

fn sheet_iso_date(sheet: &Worksheet<'_>, at: CellIndex) -> Result<Option<String>, JsError> {
    let Some(cell) = sheet.cell(at.row, at.col).map_err(js_error)? else {
        return Ok(None);
    };
    cell_iso_date(&cell)
}

fn cell_iso_date(cell: &CellRef<'_>) -> Result<Option<String>, JsError> {
    use ooxml_core::QName;
    if cell.formula().is_some()
        || ["cm", "vm"]
            .iter()
            .any(|key| cell.raw().attr(QName::new("", key)).is_some())
    {
        return Ok(None);
    }
    Ok(match cell.value().map_err(js_error)? {
        CellValue::Date(text) => Some(text),
        _ => None,
    })
}

/// Runs as 14 fields each: the text, [`encode_font`] (empty when the run has
/// no properties) and the [`ooxml_xlsx::DeclaredFont`] field bits.
fn declared_rich_text_fields(cell: &CellRef<'_>) -> Result<Vec<String>, JsError> {
    Ok(encode_declared_rich_text_runs(cell.declared_rich_text().map_err(js_error)?))
}

fn encode_declared_rich_text_runs(runs: Vec<(String, Option<ooxml_xlsx::DeclaredFont>)>) -> Vec<String> {
    let mut values = Vec::new();
    for (text, font) in runs {
        values.push(text);
        values.extend(font.map(encode_declared_font).unwrap_or_else(|| vec![String::new(); 13]));
    }
    values
}

fn decode_moved_note_box(mut fields: Vec<String>) -> Result<Option<ooxml_xlsx::NoteBox>, JsError> {
    if fields.is_empty() { return Ok(None); }
    for (field, default) in fields.iter_mut().zip(["auto", "True", "True", "absolute"]) {
        if field.is_empty() { *field = default.into(); }
    }
    if fields.len() != 8 {
        fields.truncate(4);
        fields.extend(["0.13", "0.13", "0.25", "0.25"].into_iter().map(str::to_owned));
    }
    decode_note_box(&fields)
}

fn decode_note_box(box_fields: &[String]) -> Result<Option<ooxml_xlsx::NoteBox>, JsError> {
    use ooxml_xlsx::{NoteBox, NoteEditAs};

    let settings = match box_fields {
        [] => None,
        [mode, locked, lock_text, edit_as, left, top, right, bottom] => {
            let flag = |value: &String| match value.as_str() {
                "True" => Ok(true),
                "False" => Ok(false),
                _ => Err(JsError::new("Invalid note box protection")),
            };
            let inset = |value: &String| value.parse::<f64>()
                .map_err(|_| JsError::new("Invalid note box inset"));
            Some(NoteBox {
                custom_inset: match mode.as_str() {
                    "auto" => false,
                    "custom" => true,
                    _ => return Err(JsError::new("Invalid note box inset mode")),
                },
                inset: [inset(left)?, inset(top)?, inset(right)?, inset(bottom)?],
                locked: flag(locked)?,
                lock_text: flag(lock_text)?,
                edit_as: match edit_as.as_str() {
                    "absolute" => NoteEditAs::Absolute,
                    "oneCells" => NoteEditAs::OneCells,
                    "twoCells" => NoteEditAs::TwoCells,
                    _ => return Err(JsError::new("Invalid note box positioning")),
                },
            })
        }
        _ => return Err(JsError::new("Invalid note box fields")),
    };
    Ok(settings)
}

fn decode_rich_text_runs(values: &[String]) -> Result<Vec<ooxml_xlsx::RichTextRun>, JsError> {
    let (chunks, remainder) = values.as_chunks::<13>();
    if !remainder.is_empty() {
        return Err(JsError::new("Invalid rich text entries"));
    }
    chunks.iter().map(|run| {
        let font = if run[1].is_empty() { None } else { Some(decode_font(run[1..].to_vec())?) };
        Ok((run[0].clone(), font))
    }).collect()
}

fn encode_rich_text_runs(runs: Vec<ooxml_xlsx::RichTextRun>) -> Vec<String> {
    let mut values = Vec::new();
    for (text, font) in runs {
        values.push(text);
        values.extend(font.map(encode_font).unwrap_or_else(|| vec![String::new(); 12]));
    }
    values
}

fn excel_formula_fields(
    sheet: &Worksheet<'_>,
    at: CellIndex,
    ranges: &[CellRange],
    cell: &CellRef<'_>,
) -> Result<Vec<JsValue>, JsError> {
    let (formula, reference, share_type, master) =
        if let Some((text, range)) = sheet.array_formula_at(at) {
            (
                JsValue::from_str(text),
                JsValue::from_str(&range.to_string()),
                JsValue::from_str("array"),
                JsValue::UNDEFINED,
            )
        } else if ranges.iter().any(|range| range.contains(at)) {
            return Ok(Vec::new());
        } else if let Some((source, range, text)) = sheet.shared_formula_at(at) {
            if source == at {
                (
                    JsValue::from_str(text),
                    JsValue::from_str(&range.to_string()),
                    JsValue::from_str("shared"),
                    JsValue::UNDEFINED,
                )
            } else {
                let formula = sheet.translate_shared_formula(text, source, at);
                (
                    formula
                        .as_deref()
                        .map_or(JsValue::UNDEFINED, JsValue::from_str),
                    JsValue::UNDEFINED,
                    JsValue::UNDEFINED,
                    JsValue::from_str(&source.to_string()),
                )
            }
        } else {
            let Some(text) = cell.ordinary_formula() else {
                return Ok(Vec::new());
            };
            (
                JsValue::from_str(&text),
                JsValue::UNDEFINED,
                JsValue::UNDEFINED,
                JsValue::UNDEFINED,
            )
        };
    let [result, code, date] = sheetjs_formula_result(cell.value().map_err(js_error)?);
    Ok(vec![
        formula, result, code, date, reference, share_type, master,
    ])
}

fn array_follower_ranges<'a>(
    sheet: &Worksheet<'_>,
    cached: &'a mut Option<Vec<CellRange>>,
    at: CellIndex,
) -> Result<std::borrow::Cow<'a, [CellRange]>, JsError> {
    use std::borrow::Cow;
    let ranges = match cached {
        Some(ranges) => ranges,
        None => cached.insert(classify_sheet(sheet).map_err(js_error)?),
    };
    if ranges.iter().any(|range| range.contains(at)) {
        // Preserve the merge-anchor exception on guarded coordinates.
        Ok(Cow::Owned(classify_cell(sheet, at, true).map_err(js_error)?))
    } else {
        Ok(Cow::Borrowed(ranges))
    }
}

fn write_classified_excel_error(
    sheet: &mut Worksheet<'_>,
    ranges: &[CellRange],
    at: CellIndex,
    code: &str,
) -> Result<(), JsError> {
    check_excel_edit(sheet, ranges, at, true).map_err(js_error)?;
    sheet
        .cell_mut(at.row, at.col)
        .map_err(js_error)?
        .set_error(code)
        .map_err(js_error)
}

fn write_classified_scalar(
    sheet: &mut Worksheet<'_>,
    ranges: &[CellRange],
    at: CellIndex,
    value: JsValue,
    dates_unsupported: bool,
    excel_values: bool,
) -> Result<(), JsError> {
    let empty = if excel_values {
        check_excel_edit(sheet, ranges, at, dates_unsupported).map_err(js_error)?
    } else {
        matches!(
            classified_value(sheet, ranges, at, dates_unsupported, false)
                .map_err(js_error)?,
            CellValue::Empty
        )
    };
    if value.is_null() && empty {
        return Ok(());
    }
    let cell = sheet.cell_mut(at.row, at.col).map_err(js_error)?;
    if value.is_null() {
        cell.clear_value()
    } else if let Some(number) = value.as_f64() {
        cell.set_number(number)
    } else if let Some(text) = value.as_string() {
        cell.set_string(&text)
    } else if let Some(boolean) = value.as_bool() {
        cell.set_bool(boolean)
    } else {
        return Err(JsError::new("expected a scalar cell value"));
    }
    .map_err(js_error)
}

// True means the target is already empty and needs no clear operation.
fn check_excel_edit(
    sheet: &mut Worksheet<'_>,
    ranges: &[CellRange],
    at: CellIndex,
    dates_unsupported: bool,
) -> Result<bool, Error> {
    if !ranges.iter().any(|range| range.contains(at))
        && sheet
            .cell(at.row, at.col)?
            .is_some_and(|cell| cell.ordinary_formula().is_some())
    {
        return Ok(false);
    }
    Ok(matches!(
        classified_value(sheet, ranges, at, dates_unsupported, true)?,
        CellValue::Empty
    ))
}

fn classified_value(
    sheet: &mut Worksheet<'_>,
    ranges: &[CellRange],
    at: CellIndex,
    dates_unsupported: bool,
    allow_error: bool,
) -> Result<CellValue, Error> {
    classify_scalar_value(sheet, ranges, at, dates_unsupported, allow_error)?
        .ok_or_else(|| unsupported(sheet.name(), at))
}

fn classify_scalar_value(
    sheet: &mut Worksheet<'_>,
    ranges: &[CellRange],
    at: CellIndex,
    dates_unsupported: bool,
    allow_error: bool,
) -> Result<Option<CellValue>, Error> {
    if ranges.iter().any(|range| range.contains(at)) {
        return Ok(None);
    }
    let Some(cell) = sheet.cell(at.row, at.col)? else {
        return Ok(Some(CellValue::Empty));
    };
    if cell.has_complex_value(allow_error)? {
        return Ok(None);
    }
    let value = cell.value()?;
    if matches!(value, CellValue::Number(number) if !number.is_finite()) {
        return Ok(None);
    }
    if dates_unsupported
        && matches!(value, CellValue::Number(_))
        && sheet.is_date(at.row, at.col)?
    {
        return Ok(None);
    }
    Ok(Some(value))
}

fn decode_table_column_style(mut fields: &[String]) -> Result<ooxml_xlsx::Format, JsError> {
    use ooxml_xlsx::{Format, NumFormat, Protection};
    let mut format = Format::new();
    let invalid = || JsError::new("Invalid table column style");
    while !fields.is_empty() {
        let [kind, count, remaining @ ..] = fields else {
            return Err(invalid());
        };
        let count = count.parse().map_err(|_| invalid())?;
        let (values, rest) = remaining.split_at_checked(count).ok_or_else(invalid)?;
        match (kind.as_str(), values) {
            ("number_format", [value]) => format.num_format = Some(NumFormat::Code(value.clone())),
            ("font", values) => format.font = Some(decode_font(values.to_vec())?),
            ("alignment", values) => format.alignment = Some(decode_alignment(values.to_vec())?),
            ("protection", [locked, hidden]) => format.protection = Some(Protection {
                locked: locked.parse().map_err(|_| invalid())?,
                hidden: hidden.parse().map_err(|_| invalid())?,
            }),
            ("fill", values) => format.fill = Some(decode_fill(values.to_vec())?),
            ("border", values) => format.border = Some(decode_border(values.to_vec())?),
            _ => return Err(invalid()),
        }
        fields = rest;
    }
    Ok(format)
}

fn decode_table_column([name, button, function, label]: &[String; 4]) -> Result<ooxml_xlsx::Col, JsError> {
    use ooxml_xlsx::{Col, ColumnTotal};
    let column = Col::new(name).filter_button(button == "true");
    let (column, function) = match function.as_str() {
        "explicit-none" => (column.no_total_function(), "none"),
        "label-with-none" => (column.no_total_function(), "label"),
        function => (column, function),
    };
    Ok(match decode_table_total(function, label)? {
        None => column,
        Some(ColumnTotal::Function(function)) => column.total(function),
        Some(ColumnTotal::Label(label)) => column.total_label(label),
        Some(ColumnTotal::Formula(formula)) => column.total_formula(formula),
    })
}

fn decode_table_total(function: &str, value: &str) -> Result<Option<ooxml_xlsx::ColumnTotal>, JsError> {
    use ooxml_xlsx::{ColumnTotal, Total};
    let total = match function {
        "none" => return Ok(None),
        "label" => return Ok(Some(ColumnTotal::Label(value.into()))),
        "custom" => return Ok(Some(ColumnTotal::Formula(value.into()))),
        "sum" => Total::Sum,
        "average" => Total::Average,
        "count" => Total::Count,
        "countNums" => Total::CountNums,
        "max" => Total::Max,
        "min" => Total::Min,
        "stdDev" => Total::StdDev,
        "var" => Total::Var,
        _ => return Err(JsError::new("Invalid table settings")),
    };
    Ok(Some(ColumnTotal::Function(total)))
}

fn decode_scalar_cell(value: &JsValue) -> Result<CellValue, JsError> {
    if value.is_null() {
        Ok(CellValue::Empty)
    } else if let Some(number) = value.as_f64() {
        Ok(CellValue::Number(number))
    } else if let Some(boolean) = value.as_bool() {
        Ok(CellValue::Bool(boolean))
    } else if let Some(text) = value.as_string() {
        Ok(CellValue::String(text))
    } else {
        Err(JsError::new("Expected a number, string, boolean, or null"))
    }
}

/// Serializes one sparse worksheet row for streaming XLSX output.
#[wasm_bindgen]
pub fn encode_xlsx_row(
    row: u32, columns: &[u32], values: Vec<JsValue>, layout: Option<Vec<f64>>,
) -> Result<String, JsError> {
    encode_stream_row(row, columns, values, layout, None)
}

fn encode_stream_row(
    row: u32, columns: &[u32], values: Vec<JsValue>, layout: Option<Vec<f64>>,
    strings: Option<&mut ooxml_xlsx::streaming::SharedTextWriter>,
) -> Result<String, JsError> {
    if columns.len() != values.len() {
        return Err(JsError::new("Expected one value per column"));
    }
    let cells = values.iter().map(|value| decode_scalar_cell(value).map(Into::into)).collect::<Result<Vec<_>, _>>()?;
    let cells = columns.iter().copied().zip(cells);
    let layout = decode_row_layout(layout)?;
    match strings {
        Some(strings) => strings.encode_row(row, cells, layout).map_err(js_error),
        None => ooxml_xlsx::streaming::encode_cells_with_layout(row, cells, layout).map_err(js_error),
    }
}

/// Serializes a row containing dates, errors, and ordinary formulas.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn encode_xlsx_typed_row(
    row: u32,
    columns: &[u32],
    values: Vec<JsValue>,
    kinds: &[u8],
    formulas: Vec<JsValue>,
    date_style: u32,
    layout: Option<Vec<f64>>,
    styles: Option<Vec<u32>>,
    date1904: Option<bool>,
) -> Result<String, JsError> {
    encode_typed_stream_row(row, columns, values, kinds, formulas, date_style, layout, styles, date1904, None)
}

#[allow(clippy::too_many_arguments)]
fn encode_typed_stream_row(
    row: u32,
    columns: &[u32],
    values: Vec<JsValue>,
    kinds: &[u8],
    formulas: Vec<JsValue>,
    date_style: u32,
    layout: Option<Vec<f64>>,
    styles: Option<Vec<u32>>,
    date1904: Option<bool>,
    strings: Option<&mut ooxml_xlsx::streaming::SharedTextWriter>,
) -> Result<String, JsError> {
    use ooxml_xlsx::streaming::{RowCell, RowFormula};
    if columns.len() != values.len() || columns.len() != kinds.len() || columns.len() != formulas.len() {
        return Err(JsError::new("Expected one value, kind, and formula slot per column"));
    }
    if styles.as_ref().is_some_and(|styles| styles.len() != columns.len()) {
        return Err(JsError::new("Expected one style index per column"));
    }
    let cells = values.iter().zip(kinds).zip(&formulas).enumerate().map(|(index, ((value, kind), formula))| {
        let bad = || JsError::new("Invalid streaming row cell");
        let rich_text = if *kind == 3 {
            if !js_sys::Array::is_array(value) { return Err(bad()); }
            let fields = js_sys::Array::from(value).iter().map(|field| field.as_string().ok_or_else(bad))
                .collect::<Result<Vec<_>, _>>()?;
            Some(decode_rich_text_runs(&fields)?)
        } else {
            None
        };
        let value = match kind {
            3 => CellValue::Empty,
            4 if value.is_null() && formula.is_undefined() => CellValue::Empty,
            0 => decode_scalar_cell(value)?,
            1 => decode_formula_result(value, Some("e"))?,
            2 => {
                let millis = value.as_f64().filter(|value| value.is_finite()).ok_or_else(bad)?;
                CellValue::Number(excel_serial_from_millis(millis, if date1904 == Some(true) { 1462.0 } else { 0.0 }))
            }
            _ => return Err(bad()),
        };
        let formula = if formula.is_undefined() {
            None
        } else if js_sys::Array::is_array(formula) {
            let parts = js_sys::Array::from(formula);
            let shared_index = |index| parts.get(index).as_f64()
                .filter(|value| (0.0..=f64::from(u32::MAX)).contains(value) && value.fract() == 0.0)
                .map(|value| value as u32).ok_or_else(bad);
            if parts.length() == 1 {
                Some(RowFormula::SharedMember { index: shared_index(0)? })
            } else if matches!(parts.length(), 2 | 3) {
                let text = parts.get(0).as_string().ok_or_else(bad)?;
                let range = parts.get(1).as_string().ok_or_else(bad)?;
                let range = CellRange::parse(&range).map_err(js_error)?;
                if parts.length() == 3 {
                    Some(RowFormula::SharedMaster { text, range, index: shared_index(2)? })
                } else {
                    Some(RowFormula::Array { text, range })
                }
            } else {
                return Err(bad());
            }
        } else {
            Some(RowFormula::Ordinary(formula.as_string().ok_or_else(bad)?))
        };
        let style = styles.as_ref()
            .map(|styles| (styles[index] != 0).then_some(styles[index]))
            .unwrap_or_else(|| (*kind == 2).then_some(date_style))
            .or_else(|| (*kind == 4).then_some(0));
        Ok(RowCell { value, formula, style, rich_text })
    }).collect::<Result<Vec<_>, JsError>>()?;
    let cells = columns.iter().copied().zip(cells);
    let layout = decode_row_layout(layout)?;
    match strings {
        Some(strings) => strings.encode_row(row, cells, layout).map_err(js_error),
        None => ooxml_xlsx::streaming::encode_cells_with_layout(row, cells, layout).map_err(js_error),
    }
}

/// Shared-text registry for a streaming workbook writer.
#[wasm_bindgen]
#[derive(Default)]
pub struct XlsxStreamingStrings {
    strings: ooxml_xlsx::streaming::SharedTextWriter,
}

#[wasm_bindgen]
impl XlsxStreamingStrings {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn size(&self) -> usize {
        self.strings.len()
    }

    pub fn items_xml(&self, start: usize, count: usize) -> String {
        self.strings.items_xml(start, count)
    }

    pub fn encode_row(&mut self, row: u32, columns: &[u32], values: Vec<JsValue>, layout: Option<Vec<f64>>) -> Result<String, JsError> {
        encode_stream_row(row, columns, values, layout, Some(&mut self.strings))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn encode_typed_row(
        &mut self, row: u32, columns: &[u32], values: Vec<JsValue>, kinds: &[u8],
        formulas: Vec<JsValue>, date_style: u32, layout: Option<Vec<f64>>, styles: Option<Vec<u32>>,
        date1904: Option<bool>,
    ) -> Result<String, JsError> {
        encode_typed_stream_row(row, columns, values, kinds, formulas, date_style, layout, styles, date1904, Some(&mut self.strings))
    }
}

fn decode_row_layout(layout: Option<Vec<f64>>) -> Result<ooxml_xlsx::streaming::RowLayout, JsError> {
    use ooxml_xlsx::streaming::RowLayout;
    let Some(layout) = layout else {
        return Ok(RowLayout::default());
    };
    let [height, hidden, outline, collapsed, style @ ..] = layout.as_slice() else {
        return Err(JsError::new("Expected row-layout fields"));
    };
    let style = match style {
        [] => None,
        [id] if (1.0..=f64::from(u32::MAX)).contains(id) && id.fract() == 0.0 => Some(*id as u32),
        _ => return Err(JsError::new("Invalid streaming row style")),
    };
    if !matches!(*hidden, 0.0 | 1.0) || !matches!(*collapsed, 0.0 | 1.0)
        || !(0.0..=7.0).contains(outline) || outline.fract() != 0.0 {
        return Err(JsError::new("Invalid streaming row layout"));
    }
    Ok(RowLayout {
        style,
        height: (!height.is_nan()).then_some(*height),
        hidden: *hidden == 1.0,
        outline_level: *outline as u8,
        collapsed: *collapsed == 1.0,
    })
}

fn excel_serial_from_millis(millis: f64, offset: f64) -> f64 {
    25569.0 + millis / 86_400_000.0 - offset
}

fn scalar_cell_write(value: &CellValue) -> CellWrite<'_> {
    match value {
        CellValue::Empty => CellWrite::Clear,
        CellValue::Number(value) => CellWrite::Number(*value),
        CellValue::String(value) => CellWrite::String(value),
        CellValue::Bool(value) => CellWrite::Bool(*value),
        _ => unreachable!("decoded scalar cell value"),
    }
}

fn with_table_cell_writes(
    values: Vec<JsValue>,
    formula_flags: &[u8],
    results: Vec<JsValue>,
    apply: impl FnOnce(&[CellWrite<'_>]) -> Result<(), JsError>,
) -> Result<(), JsError> {
    if values.len() != formula_flags.len() {
        return Err(JsError::new("Expected one kind per table cell"));
    }
    let values = values.iter().map(decode_scalar_cell).collect::<Result<Vec<_>, _>>()?;
    let mut results_iter = results.iter();
    let caches = formula_flags.iter().map(|&flag| match flag {
        0 | 4 | 5 => Ok(CellValue::Empty),
        1..=3 => results_iter.next()
            .ok_or_else(|| JsError::new("Expected one result per table formula"))
            .and_then(|result| decode_formula_result(result, match flag {
                2 => Some("e"),
                3 => Some("d"),
                _ => None,
            })),
        _ => Err(JsError::new("Unexpected table formula flag")),
    }).collect::<Result<Vec<_>, JsError>>()?;
    if results_iter.next().is_some() {
        return Err(JsError::new("Expected one result per table formula"));
    }
    let writes = values.iter().zip(formula_flags).zip(&caches).map(|((value, &formula), cache)| {
        Ok(match (value, formula) {
            (CellValue::String(text), 1..=3) => CellWrite::FormulaWithResult(text, cache),
            (CellValue::String(text), 4) => CellWrite::IsoDate(text),
            (CellValue::String(text), 5) => CellWrite::Error(text),
            (value, 0) => scalar_cell_write(value),
            _ => return Err(JsError::new("Expected text for a marked table cell")),
        })
    }).collect::<Result<Vec<_>, JsError>>()?;
    apply(&writes)
}

fn decode_formula_result(value: &JsValue, result_type: Option<&str>) -> Result<CellValue, JsError> {
    let bad_result = || JsError::new("expected a scalar formula result");
    Ok(match result_type {
        Some("e") => CellValue::Error(value.as_string().ok_or_else(bad_result)?),
        Some("d") => CellValue::Date(value.as_string().ok_or_else(bad_result)?),
        None => {
            if value.is_undefined() {
                CellValue::Empty
            } else if let Some(number) = value.as_f64() {
                CellValue::Number(number)
            } else if let Some(boolean) = value.as_bool() {
                CellValue::Bool(boolean)
            } else if let Some(text) = value.as_string() {
                CellValue::String(text)
            } else {
                return Err(bad_result());
            }
        }
        _ => return Err(bad_result()),
    })
}

fn sheetjs_formula(sheet: &Worksheet<'_>, cell: &ooxml_xlsx::CellRef<'_>) -> Option<String> {
    let at = cell.index();
    if let Some((source, _, text)) = sheet.shared_formula_at(at) {
        return if source == at {
            Some(text.to_owned())
        } else {
            sheet.translate_shared_formula(text, source, at)
        };
    }
    cell.ordinary_formula()
}

fn sheetjs_formula_result(value: CellValue) -> [JsValue; 3] {
    match value {
        CellValue::Empty => [JsValue::UNDEFINED, JsValue::UNDEFINED, JsValue::UNDEFINED],
        CellValue::Error(code) => [
            JsValue::UNDEFINED,
            JsValue::from_str(&code),
            JsValue::UNDEFINED,
        ],
        CellValue::Date(date) => [
            JsValue::UNDEFINED,
            JsValue::UNDEFINED,
            JsValue::from_str(&date),
        ],
        other => [scalar_js(other), JsValue::UNDEFINED, JsValue::UNDEFINED],
    }
}

fn sheetjs_special_value(cell: &ooxml_xlsx::CellRef<'_>) -> Result<Option<(bool, String)>, Error> {
    use ooxml_core::QName;
    let kind = cell.data_type();
    if !matches!(kind.as_str(), "e" | "d") || cell.formula().is_some() {
        return Ok(None);
    }
    if kind.as_str() == "d"
        && ["cm", "vm"]
            .iter()
            .any(|key| cell.raw().attr(QName::new("", key)).is_some())
    {
        return Ok(None);
    }
    Ok(match cell.value()? {
        CellValue::Error(text) => Some((false, text)),
        CellValue::Date(text) => Some((true, text)),
        _ => None,
    })
}

fn scalar_js(value: CellValue) -> JsValue {
    match value {
        CellValue::Empty => JsValue::NULL,
        CellValue::Number(number) if number.is_finite() => JsValue::from_f64(number),
        CellValue::String(text) => JsValue::from_str(&text),
        CellValue::Bool(value) => JsValue::from_bool(value),
        _ => JsValue::UNDEFINED,
    }
}

fn unsupported(sheet: &str, at: CellIndex) -> Error {
    Error::InvalidValue {
        cell: at.to_string(),
        message: format!("{sheet}: cell does not have an editable scalar representation"),
    }
}

fn js_error(error: Error) -> JsError {
    JsError::new(&error.to_string())
}

/// Comparison candidate for ADR-0021. Retains committed bytes on the Rust
/// side and only transfers them to JS when the caller requests a save.
#[cfg(feature = "transaction-spike")]
#[wasm_bindgen]
pub struct AtomicXlsxDocument {
    bytes: std::sync::Arc<[u8]>,
    read_view: XlsxDocument,
}

#[cfg(feature = "transaction-spike")]
#[wasm_bindgen]
impl AtomicXlsxDocument {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<Self, JsError> {
        let bytes = std::sync::Arc::<[u8]>::from(bytes);
        let read_view = Self::open(&bytes)?;
        Ok(Self { bytes, read_view })
    }

    pub fn read_scalar(&mut self, name: &str, row: u32, column: u32) -> Result<JsValue, JsError> {
        self.read_view.read_scalar(name, row, column, true, None)
    }

    pub fn write_scalar(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        value: JsValue,
    ) -> Result<(), JsError> {
        self.stage(
            name,
            row,
            column,
            value,
            |_| Ok(()),
            |candidate| candidate.to_bytes(),
        )
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.to_vec()
    }

    #[cfg(feature = "fault-injection")]
    pub fn write_with_failure(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        value: JsValue,
        step: &str,
    ) -> Result<(), JsError> {
        self.stage(
            name,
            row,
            column,
            value,
            |candidate| {
                if step == "serialize" {
                    Ok(())
                } else {
                    candidate.inject_apply_failure(step)
                }
            },
            |candidate| {
                if step == "serialize" {
                    Err(JsError::new("injected serialization failure"))
                } else {
                    candidate.to_bytes()
                }
            },
        )
    }
}

#[cfg(feature = "transaction-spike")]
impl AtomicXlsxDocument {
    fn open(bytes: &std::sync::Arc<[u8]>) -> Result<XlsxDocument, JsError> {
        Ok(XlsxDocument {
            workbook: Workbook::from_bytes(bytes.clone()).map_err(js_error)?,
            scalar_read_cache: ScalarReadCache::default(),
        })
    }

    fn stage(
        &mut self,
        name: &str,
        row: u32,
        column: u32,
        value: JsValue,
        prepare: impl FnOnce(&mut XlsxDocument) -> Result<(), JsError>,
        save: impl FnOnce(&XlsxDocument) -> Result<Vec<u8>, JsError>,
    ) -> Result<(), JsError> {
        let mut candidate = Self::open(&self.bytes)?;
        prepare(&mut candidate)?;
        candidate.write_scalar(name, row, column, value, true, None)?;
        let bytes = save(&candidate)?.into();
        self.bytes = bytes;
        self.read_view = candidate;
        Ok(())
    }
}

#[wasm_bindgen]
pub fn write_zip_entries(
    names: Vec<String>,
    offsets: &[u32],
    content: &[u8],
    compressed: Option<bool>,
) -> Result<Vec<u8>, JsError> {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    if offsets.len() != names.len() + 1
        || offsets.first() != Some(&0)
        || offsets.last().copied().map(|end| end as usize) != Some(content.len())
    {
        return Err(JsError::new("Invalid ZIP entry offsets"));
    }
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let file_options = if compressed.unwrap_or(false) {
        stored.compression_method(CompressionMethod::Deflated)
    } else {
        stored
    };
    for (name, bounds) in names.into_iter().zip(offsets.windows(2)) {
        let bytes = content
            .get(bounds[0] as usize..bounds[1] as usize)
            .ok_or_else(|| JsError::new("Invalid ZIP entry range"))?;
        if name.ends_with('/') && bytes.is_empty() {
            archive.add_directory(name, stored).map_err(zip_io_error)?;
        } else {
            archive
                .start_file(name, file_options)
                .map_err(zip_io_error)?;
            archive.write_all(bytes).map_err(zip_io_error)?;
        }
    }
    Ok(archive.finish().map_err(zip_io_error)?.into_inner())
}

const ODS_MEDIA_TYPE: &str = "application/vnd.oasis.opendocument.spreadsheet";
const ODS_STYLES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" office:version="1.2">"#,
    "<office:styles/></office:document-styles>",
);
const ODS_MANIFEST: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">"#,
    r#"<manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/>"#,
    r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
    r#"<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>"#,
    r#"<manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>"#,
    "</manifest:manifest>",
);

fn zip_io_error(error: impl std::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

#[wasm_bindgen]
pub fn write_ods(content: &[u8], metadata: &[u8], compression: bool) -> Result<Vec<u8>, JsError> {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let options = if compression {
        stored.compression_method(CompressionMethod::Deflated)
    } else {
        stored
    };
    archive
        .start_file("mimetype", stored)
        .map_err(zip_io_error)?;
    archive
        .write_all(ODS_MEDIA_TYPE.as_bytes())
        .map_err(zip_io_error)?;
    archive
        .start_file("content.xml", options)
        .map_err(zip_io_error)?;
    archive.write_all(content).map_err(zip_io_error)?;
    archive
        .start_file("styles.xml", options)
        .map_err(zip_io_error)?;
    archive
        .write_all(ODS_STYLES.as_bytes())
        .map_err(zip_io_error)?;
    archive
        .start_file("meta.xml", options)
        .map_err(zip_io_error)?;
    archive.write_all(metadata).map_err(zip_io_error)?;
    archive
        .start_file("META-INF/manifest.xml", options)
        .map_err(zip_io_error)?;
    archive
        .write_all(ODS_MANIFEST.as_bytes())
        .map_err(zip_io_error)?;
    Ok(archive.finish().map_err(zip_io_error)?.into_inner())
}

#[wasm_bindgen]
pub fn read_ods_parts(bytes: &[u8]) -> Result<Vec<JsValue>, JsError> {
    use std::io::{Cursor, Read};

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_io_error)?;
    {
        let Ok(mimetype) = archive.by_name("mimetype") else {
            return Ok(Vec::new());
        };
        let mut media_type = Vec::new();
        mimetype
            .take(128)
            .read_to_end(&mut media_type)
            .map_err(zip_io_error)?;
        if media_type != ODS_MEDIA_TYPE.as_bytes() {
            return Ok(Vec::new());
        }
    }
    let read_part = |archive: &mut zip::ZipArchive<_>,
                     name: &str,
                     required: bool|
     -> Result<JsValue, JsError> {
        let file = match archive.by_name(name) {
            Ok(file) => file,
            Err(zip::result::ZipError::FileNotFound) if !required => return Ok(JsValue::UNDEFINED),
            Err(error) => return Err(zip_io_error(error)),
        };
        let limit = 64 * 1024 * 1024;
        let mut content = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut content)
            .map_err(zip_io_error)?;
        if content.len() as u64 > limit {
            return Err(JsError::new(&format!(
                "ODS {name} exceeds the XML size limit"
            )));
        }
        Ok(JsValue::from_str(
            std::str::from_utf8(&content).map_err(zip_io_error)?,
        ))
    };
    Ok(vec![
        read_part(&mut archive, "content.xml", true)?,
        read_part(&mut archive, "styles.xml", false)?,
        read_part(&mut archive, "meta.xml", false)?,
    ])
}

fn custom_property_entries(fields: &[String]) -> Result<Vec<(String, String, String)>, JsError> {
    let (entries, rest) = fields.as_chunks::<3>();
    if !rest.is_empty() {
        return Err(JsError::new("expected custom property entries"));
    }
    Ok(entries
        .iter()
        .map(|[key, kind, value]| (key.clone(), kind.clone(), value.clone()))
        .collect())
}

#[wasm_bindgen]
pub fn document_property_parts(
    fields: Vec<String>,
    custom: Vec<String>,
    sheet_names: Vec<String>,
) -> Result<Vec<JsValue>, JsError> {
    let (entries, rest) = fields.as_chunks::<2>();
    if !rest.is_empty() {
        return Err(JsError::new("expected standard property entries"));
    }
    let entries: Vec<_> = entries
        .iter()
        .map(|[key, value]| (key.clone(), value.clone()))
        .collect();
    Ok(ooxml_xlsx::encode_document_properties(
        &entries,
        &custom_property_entries(&custom)?,
        &sheet_names,
    )
    .map_err(js_error)?
    .into_iter()
    .map(|part| {
        part.as_deref()
            .map_or(JsValue::UNDEFINED, JsValue::from_str)
    })
    .collect())
}

#[wasm_bindgen]
pub fn biff_document_properties(bytes: &[u8], document_summary: bool) -> Option<Vec<String>> {
    Some(
        ooxml_xlsx::biff_document_properties(bytes, document_summary)?
            .into_iter()
            .flat_map(|(name, kind, text)| ["standard".into(), name, kind.into(), text])
            .collect(),
    )
}

#[wasm_bindgen]
pub fn write_biff_property_streams(fields: Vec<String>) -> Result<Vec<u8>, JsError> {
    let [summary, document] =
        ooxml_xlsx::encode_biff_document_properties(&custom_property_entries(&fields)?)
            .ok_or_else(|| JsError::new("Invalid XLS document properties"))?;
    let summary = summary.unwrap_or_default();
    let length =
        u32::try_from(summary.len()).map_err(|_| JsError::new("XLS properties are too large"))?;
    let mut bytes = length.to_le_bytes().to_vec();
    bytes.extend(summary);
    bytes.extend(document.unwrap_or_default());
    Ok(bytes)
}

#[wasm_bindgen]
pub fn read_biff_shared_strings(bytes: &[u8]) -> Result<Vec<u16>, JsError> {
    ooxml_xlsx::biff_shared_strings(bytes)
        .ok_or_else(|| JsError::new("Truncated or invalid XLS shared string table"))
}

#[wasm_bindgen]
pub fn write_biff_shared_strings(
    total: u32,
    lengths: &[u32],
    text: &[u16],
) -> Result<Vec<u8>, JsError> {
    ooxml_xlsx::encode_biff_shared_strings(total, lengths, text)
        .ok_or_else(|| JsError::new("Invalid XLS shared strings"))
}

fn package_vba_project(package: &ooxml_core::Package) -> ooxml_core::Result<Option<Vec<u8>>> {
    const VBA: &str = "application/vnd.ms-office.vbaProject";
    let project = package
        .content_types()
        .overrides()
        .iter()
        .find(|(_, content_type)| content_type == VBA)
        .map(|(path, _)| path.trim_start_matches('/'))
        .or_else(|| {
            (package.content_type("xl/vbaProject.bin") == Some(VBA)).then_some("xl/vbaProject.bin")
        });
    project
        .filter(|path| package.contains(path))
        .map(|path| package.read_part(path))
        .transpose()
}

const XLSB_MAIN: &str = "application/vnd.ms-excel.sheet.binary.macroEnabled.main";

#[wasm_bindgen]
pub struct XlsbArchive {
    package: ooxml_core::Package,
    workbook: String,
    relationships: ooxml_core::opc::Relationships,
}

#[wasm_bindgen]
impl XlsbArchive {
    pub fn open(bytes: &[u8]) -> Result<Option<XlsbArchive>, JsError> {
        use std::io::{Cursor, Read};
        const OFFICE_DOCUMENT: &str =
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_io_error)?;
        let Ok(mut entry) = zip.by_name("[Content_Types].xml") else {
            return Ok(None);
        };
        let mut content_types = Vec::new();
        entry
            .read_to_end(&mut content_types)
            .map_err(zip_io_error)?;
        if !content_types
            .windows(XLSB_MAIN.len())
            .any(|part| part == XLSB_MAIN.as_bytes())
        {
            return Ok(None);
        }
        let package = ooxml_core::Package::from_bytes(bytes.to_vec()).map_err(zip_io_error)?;
        let links = package.package_relationships();
        let workbook = links
            .by_type(OFFICE_DOCUMENT)
            .find_map(|rel| links.resolve(rel))
            .ok_or_else(|| JsError::new("Missing XLSB workbook part"))?;
        if package.content_type(&workbook) != Some(XLSB_MAIN) {
            return Ok(None);
        }
        let relationships = package.relationships(&workbook).map_err(zip_io_error)?;
        Ok(Some(Self {
            package,
            workbook,
            relationships,
        }))
    }

    pub fn zip_entry_names(&self) -> Vec<String> {
        self.package.part_names().map(str::to_owned).collect()
    }

    pub fn zip_entry_bytes(&self, name: &str) -> Result<Vec<u8>, JsError> {
        self.package.read_part(name).map_err(zip_io_error)
    }

    pub fn zip_entry_modified(&self, name: &str) -> Option<Vec<u16>> {
        self.package.part_modified(name).map(Vec::from)
    }

    pub fn vba_project(&self) -> Result<Option<Vec<u8>>, JsError> {
        package_vba_project(&self.package).map_err(zip_io_error)
    }

    pub fn document_properties(&mut self) -> Vec<String> {
        ooxml_xlsx::package_document_properties(&mut self.package)
            .into_iter()
            .flat_map(|(section, key, kind, value)| [section, key, kind, value])
            .collect()
    }

    pub fn calculation_chain(&self) -> Result<Option<Vec<u32>>, JsError> {
        const CONTENT_TYPE: &str = "application/vnd.ms-excel.calcChain";
        let Some((part, _)) = self
            .package
            .content_types()
            .overrides()
            .iter()
            .find(|(_, content_type)| content_type == CONTENT_TYPE)
        else {
            return Ok(None);
        };
        let bytes = self
            .package
            .read_part(part.trim_start_matches('/'))
            .map_err(zip_io_error)?;
        let entries = ooxml_xlsx::xlsb::read_calculation_chain(&bytes)
            .map_err(|error| JsError::new(&format!("INVALID_INPUT: {error}")))?;
        Ok(Some(entries.into_iter().flatten().collect()))
    }

    pub fn workbook_info(&self, catalogue_only: bool) -> Result<XlsbWorkbookInfo, JsError> {
        let bytes = self
            .package
            .read_part(&self.workbook)
            .map_err(zip_io_error)?;
        let info = if catalogue_only {
            ooxml_xlsx::xlsb::WorkbookInfo::read_catalogue(&bytes)
        } else {
            ooxml_xlsx::xlsb::WorkbookInfo::read(&bytes)
        };
        let info = info.map_err(|error| JsError::new(&format!("INVALID_INPUT: {error}")))?;
        Ok(XlsbWorkbookInfo(info))
    }

    pub fn project_sheet(
        &self,
        relationship: &str,
        row_limit: Option<u32>,
        column_styles: bool,
        stubs: bool,
        formulas: bool,
    ) -> Result<XlsbSheetProjection, JsError> {
        let name = self
            .relationships
            .by_id(relationship)
            .and_then(|link| self.relationships.resolve(link))
            .ok_or_else(|| JsError::new("Unresolved XLSB worksheet relationship"))?;
        let bytes = self.package.read_part(&name).map_err(zip_io_error)?;
        let layout = ooxml_xlsx::xlsb::SheetProjection::read(
            &bytes,
            row_limit,
            column_styles,
            stubs,
            formulas,
        )
        .map_err(|error| JsError::new(&format!("INVALID_INPUT: {error}")))?;
        let hyperlinks = &layout.tail.hyperlinks;
        let relationships = hyperlinks
            .iter()
            .any(|link| !link.relationship.is_empty())
            .then(|| self.package.relationships(&name))
            .transpose()
            .map_err(zip_io_error)?;
        let mut hyperlink_fields = Vec::new();
        for link in hyperlinks {
            let [mut target, mut kind, mut mode] = Default::default();
            if let Some(rel) = relationships
                .as_ref()
                .and_then(|rels| rels.by_id(&link.relationship))
            {
                target = rel.target.clone();
                kind = rel.rel_type.clone();
                if rel.target_mode == ooxml_core::opc::TargetMode::External {
                    mode = "External".into();
                }
            }
            hyperlink_fields.extend([
                link.relationship.clone(),
                link.location.clone(),
                link.tooltip.clone(),
                link.display.clone(),
                target,
                kind,
                mode,
            ]);
        }
        Ok(XlsbSheetProjection {
            layout,
            hyperlink_fields,
        })
    }

    pub fn styles(&self) -> Result<XlsbStyles, JsError> {
        let bytes = self.resources("styles")?.unwrap_or_default();
        let styles = ooxml_xlsx::xlsb::Styles::read(&bytes)
            .map_err(|error| JsError::new(&format!("INVALID_INPUT: {error}")))?;
        Ok(XlsbStyles(styles))
    }

    pub fn shared_strings(&self) -> Result<XlsbSharedStrings, JsError> {
        let bytes = self.resources("sharedStrings")?.unwrap_or_default();
        let strings = ooxml_xlsx::xlsb::SharedStrings::read(&bytes)
            .map_err(|error| JsError::new(&format!("INVALID_INPUT: {error}")))?;
        Ok(XlsbSharedStrings(strings))
    }

    pub fn resources(&self, resource: &str) -> Result<Option<Vec<u8>>, JsError> {
        let relationship = match resource {
            "styles" => {
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles"
            }
            "sharedStrings" => {
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings"
            }
            _ => return Err(JsError::new("Unknown XLSB workbook resource")),
        };
        self.relationships
            .by_type(relationship)
            .find_map(|link| self.relationships.resolve(link))
            .map(|name| self.package.read_part(&name).map_err(zip_io_error))
            .transpose()
    }
}

#[wasm_bindgen]
pub struct XlsbWorkbookInfo(ooxml_xlsx::xlsb::WorkbookInfo);

#[wasm_bindgen]
impl XlsbWorkbookInfo {
    pub fn date1904(&self) -> bool {
        self.0.date1904
    }

    pub fn take_sheets(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.0.sheets)
    }

    pub fn take_text(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.0.text)
    }

    pub fn take_references(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.0.references)
    }

    pub fn take_names(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.0.names)
    }

    pub fn take_name_text(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.0.name_text)
    }

    pub fn take_name_formulas(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.0.name_formulas)
    }
}

#[wasm_bindgen]
pub struct XlsbStyles(ooxml_xlsx::xlsb::Styles);

#[wasm_bindgen]
impl XlsbStyles {
    pub fn take_formats(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.0.formats)
    }

    pub fn take_text(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.0.text)
    }

    pub fn take_cell_formats(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.0.cell_formats)
    }
}

#[wasm_bindgen]
pub struct XlsbSharedStrings(ooxml_xlsx::xlsb::SharedStrings);

#[wasm_bindgen]
impl XlsbSharedStrings {
    pub fn take_lengths(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.0.lengths)
    }

    pub fn take_units(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.0.units)
    }
}

#[wasm_bindgen]
pub struct XlsbSheetProjection {
    layout: ooxml_xlsx::xlsb::SheetProjection,
    hyperlink_fields: Vec<String>,
}

#[wasm_bindgen]
impl XlsbSheetProjection {
    pub fn take_cells(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.layout.cells.values)
    }

    pub fn take_text(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.layout.cells.text)
    }

    pub fn take_formulas(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.layout.cells.formulas)
    }

    pub fn dimensions(&self) -> Option<Vec<u32>> {
        self.layout.dimensions.map(|range| range.0.to_vec())
    }

    pub fn rows(&self) -> Vec<u32> {
        self.layout.rows.iter().flatten().copied().collect()
    }

    pub fn columns(&self) -> Vec<u32> {
        self.layout.columns.iter().flatten().copied().collect()
    }

    pub fn filter(&self) -> Option<Vec<u32>> {
        self.layout.tail.filter.map(|range| range.0.to_vec())
    }

    pub fn merges(&self) -> Vec<u32> {
        self.layout
            .tail
            .merges
            .iter()
            .flat_map(|range| range.0)
            .collect()
    }

    pub fn hyperlink_ranges(&self) -> Vec<u32> {
        self.layout
            .tail
            .hyperlinks
            .iter()
            .flat_map(|link| link.range.0)
            .collect()
    }

    pub fn take_hyperlink_fields(&mut self) -> Vec<String> {
        std::mem::take(&mut self.hyperlink_fields)
    }

    pub fn margins(&self) -> Option<Vec<f64>> {
        self.layout.tail.margins.map(|margins| margins.to_vec())
    }
}

#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn write_xlsb_workbook(
    date1904: bool,
    visibility: &[u32],
    lengths: &[u32],
    names: &[u16],
    references: &[u32],
    name_fields: &[u32],
    name_text: &[u16],
    name_formulas: &[u8],
) -> Result<Vec<u8>, JsError> {
    ooxml_xlsx::xlsb::write_workbook_with_names(
        date1904,
        visibility,
        lengths,
        names,
        references,
        ooxml_xlsx::xlsb::DefinedNames {
            fields: name_fields,
            text: name_text,
            formulas: name_formulas,
        },
    )
    .map_err(zip_io_error)
}

#[wasm_bindgen]
pub fn write_xlsb_styles(ids: &[i32], lengths: &[u32], text: &[u16]) -> Result<Vec<u8>, JsError> {
    ooxml_xlsx::xlsb::write_styles(ids, lengths, text).map_err(zip_io_error)
}

#[wasm_bindgen]
pub fn write_xlsb_shared_strings(
    total: u32,
    lengths: &[u32],
    units: &[u16],
) -> Result<Vec<u8>, JsError> {
    ooxml_xlsx::xlsb::write_shared_strings(total, lengths, units).map_err(zip_io_error)
}

#[wasm_bindgen]
pub fn write_xlsb_cells(
    rows: &[u32],
    counts: &[u32],
    cells: &[u32],
    numbers: &[f64],
    text: &[u16],
    tokens: &[u8],
) -> Result<Vec<u8>, JsError> {
    let (cells, remainder) = cells.as_chunks::<9>();
    if !remainder.is_empty() {
        return Err(JsError::new("Invalid XLSB cell batch"));
    }
    ooxml_xlsx::xlsb::write_cells(rows, counts, cells, numbers, text, tokens).map_err(zip_io_error)
}

#[wasm_bindgen]
pub fn write_xlsb_sheet_header(dimensions: &[u32], columns: &[u32]) -> Result<Vec<u8>, JsError> {
    let dimensions = dimensions
        .try_into()
        .map(ooxml_xlsx::xlsb::Range)
        .map_err(|_| JsError::new("Invalid XLSB sheet dimensions"))?;
    let (columns, remainder) = columns.as_chunks::<3>();
    if !remainder.is_empty() {
        return Err(JsError::new("Invalid XLSB column layout"));
    }
    ooxml_xlsx::xlsb::write_sheet_header(dimensions, columns).map_err(zip_io_error)
}

#[wasm_bindgen]
pub fn write_xlsb_sheet_end(
    filter: Option<Vec<u32>>,
    merges: &[u32],
    margins: Option<Vec<f64>>,
    link_ranges: &[u32],
    link_text: Vec<String>,
    protection_password: Option<String>,
    protection_flags: u32,
) -> Result<Vec<u8>, JsError> {
    use ooxml_xlsx::xlsb::{
        Hyperlink, MAX_HYPERLINK_CELLS, Range, SheetTail, hyperlink_cell_count,
    };

    let filter = filter
        .map(|range| range.try_into().map(Range))
        .transpose()
        .map_err(|_| JsError::new("Invalid XLSB filter coordinates"))?;
    let (merges, remainder) = merges.as_chunks::<4>();
    if !remainder.is_empty() {
        return Err(JsError::new("Invalid XLSB merge coordinates"));
    }
    let margins = margins
        .map(Vec::try_into)
        .transpose()
        .map_err(|_| JsError::new("Invalid XLSB margin count"))?;
    let (ranges, trailing) = link_ranges.as_chunks::<4>();
    let mut text = link_text.into_iter();
    if !trailing.is_empty() || ranges.len() != text.len() / 4 || !text.len().is_multiple_of(4) {
        return Err(JsError::new("Invalid XLSB hyperlink fields"));
    }
    let mut linked_cells = 0;
    let mut hyperlinks = Vec::new();
    for &bounds in ranges {
        let range = Range(bounds);
        linked_cells += hyperlink_cell_count(range).map_err(zip_io_error)?;
        if linked_cells > MAX_HYPERLINK_CELLS {
            return Err(JsError::new("XLSB hyperlink cell limit exceeded"));
        }
        hyperlinks.push(Hyperlink {
            range,
            relationship: text.next().unwrap(),
            location: text.next().unwrap(),
            tooltip: text.next().unwrap(),
            display: text.next().unwrap(),
        });
    }
    Ok(SheetTail {
        filter,
        merges: merges.iter().copied().map(Range).collect(),
        margins,
        hyperlinks,
        protection: protection_password.map(|password| {
            ooxml_xlsx::xlsb::SheetProtection::new(
                &password,
                protection_flags as u16,
                (protection_flags >> 16) as u16,
            )
        }),
    }
    .encode_sheet_end())
}
