//! Legacy drawing parts used by notes and threaded comment fallbacks.

use std::collections::HashSet;
use std::sync::LazyLock;

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{ElementSpec, Package, QName, RawNode, XmlDocument};
use ooxml_schema::sml;

use super::xml::{self, Patch, RID, attr, child};
use crate::images::{OFFICE, REL_VML, internal_target, part_error, walk};
use crate::reference::{MAX_COLS, MAX_ROWS};
use crate::{CellIndex, Result, Workbook};

const V: &str = "urn:schemas-microsoft-com:vml";
const X: &str = "urn:schemas-microsoft-com:office:excel";
const LAYOUT: QName = QName::new(OFFICE, "shapelayout");
const IDMAP: QName = QName::new(OFFICE, "idmap");
const ROOT: QName = QName::new("", "xml");
pub(super) const CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.vmlDrawing";

pub(super) fn read(pkg: &mut Package, sheet: &str) -> Result<Option<(String, XmlDocument)>> {
    let rels = pkg.relationships(sheet)?;
    let doc = pkg.xml_part(sheet)?;
    let mut links = doc
        .root()
        .elements()
        .filter(|node| node.is(sml::LegacyDrawing::QNAME));
    let wrapped = doc
        .root()
        .elements()
        .filter(|node| node.is(MC_ALTERNATE_CONTENT))
        .any(|node| walk(node).any(|node| node.is(sml::LegacyDrawing::QNAME)));
    if wrapped {
        return Err(ooxml_core::Error::UnresolvedSibling(sml::Worksheet::QNAME).into());
    }
    let Some(link) = links.next() else {
        return Ok(None);
    };
    if links.next().is_some() {
        return Err(part_error(sheet, "multiple legacy drawing references"));
    }
    let target = link
        .attr(RID)
        .and_then(|id| internal_target(&rels, id, REL_VML));
    drop(links);
    let part = target
        .and_then(|target| pkg.part_name(&target).map(str::to_owned))
        .ok_or_else(|| part_error(sheet, "unresolved legacy drawing reference"))?;
    let doc = xml::read(pkg, &part, ROOT)?;
    Ok(Some((part, doc)))
}

fn unique_child<'a>(parent: &'a RawNode, name: QName, part: &str) -> Result<Option<&'a RawNode>> {
    let mut matches = parent.elements().filter(|node| node.is(name));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(part_error(
            part,
            format!("multiple VML {} elements", name.local),
        ));
    }
    Ok(first)
}

fn blocks(data: &str, part: &str) -> Result<Vec<u32>> {
    data.split(',')
        .map(|block| {
            block
                .trim()
                .parse()
                .map_err(|_| part_error(part, "invalid VML block"))
        })
        .collect()
}

fn all_blocks(pkg: &Package) -> Result<u32> {
    static DATA: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
        regex::bytes::Regex::new(r#"(?-u:<o:idmap\b[^>]*?\bdata\s*=\s*(?:"([^"]*)"|'([^']*)'))"#)
            .expect("fixed idmap pattern")
    });
    let mut max = 0;
    for part in pkg.part_names().filter(|part| {
        pkg.content_type(part) == Some(CONTENT_TYPE)
            || part
                .rsplit_once('.')
                .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("vml"))
    }) {
        let bytes = pkg
            .read_part(part)
            .map_err(|error| part_error(part, error))?;
        let found = match XmlDocument::parse_with(&bytes, &pkg.parse_options()) {
            Ok(doc) => walk(doc.root())
                .filter(|node| node.is(IDMAP))
                .map(|node| {
                    let data = node
                        .attr(attr("data"))
                        .ok_or_else(|| part_error(part, "missing VML block data"))?;
                    blocks(data, part)
                })
                .collect::<Result<Vec<_>>>()?,
            Err(error @ ooxml_core::Error::DtdNotAllowed { .. }) => {
                return Err(part_error(part, error));
            }
            Err(_) => {
                let found = DATA
                    .captures_iter(&bytes)
                    .map(|m| {
                        let value = m.get(1).or_else(|| m.get(2)).expect("one capture");
                        let data = std::str::from_utf8(value.as_bytes())
                            .map_err(|error| part_error(part, error))?;
                        blocks(data, part)
                    })
                    .collect::<Result<Vec<_>>>()?;
                if found.is_empty() {
                    return Err(part_error(part, "cannot read blocks from malformed VML"));
                }
                found
            }
        };
        max = max.max(found.into_iter().flatten().max().unwrap_or_default());
    }
    Ok(max)
}

fn in_blocks(blocks: &[u32], start: u32, used: &HashSet<u32>) -> Option<u32> {
    blocks
        .iter()
        .filter_map(|block| {
            let base = block.checked_mul(1024)?;
            let first = (base + 1).max(start);
            (first..=base + 1023).find(|id| !used.contains(id))
        })
        .min()
}

fn pick(blocks: &[u32], high: u32, used: &HashSet<u32>) -> Option<u32> {
    high.checked_add(1)
        .and_then(|start| in_blocks(blocks, start, used))
        .or_else(|| in_blocks(blocks, 1, used))
}

fn layout(block: u32, namespaces: &str) -> String {
    format!(
        "<o:shapelayout{namespaces} v:ext=\"edit\"><o:idmap v:ext=\"edit\" data=\"{block}\"/></o:shapelayout>"
    )
}

fn note_type(namespaces: &str) -> String {
    format!(
        concat!(
            "<v:shapetype{} id=\"_x0000_t202\" coordsize=\"21600,21600\"",
            " o:spt=\"202\" path=\"m,l,21600r21600,l21600,xe\"><v:stroke joinstyle=\"miter\"/>",
            "<v:path gradientshapeok=\"t\" o:connecttype=\"rect\"/></v:shapetype>"
        ),
        namespaces
    )
}

fn shape(
    at: CellIndex,
    id: u32,
    shape_type: &str,
    visible: bool,
    namespaces: &str,
    z_index: i64,
) -> String {
    let CellIndex { row, col } = at;
    let (left, x_offset) = if col >= MAX_COLS - 3 {
        (col - 3, 49)
    } else {
        (col + 1, 15)
    };
    let (top, y_offset) = match MAX_ROWS - row {
        1 => (row - 4, 14),
        2 | 3 => (row - 4, 16),
        MAX_ROWS => (0, 2),
        _ => (row - 1, 10),
    };
    // Default cells are 64 by 20 px. VML offsets are in pixels.
    let end_row = top + (y_offset + 74) / 20;
    let end_y = (y_offset + 74) % 20;
    let anchor = format!(
        "{left}, {x_offset}, {top}, {y_offset}, {}, {x_offset}, {end_row}, {end_y}",
        left + 2
    );
    let margin_left = f64::from(left * 64 + x_offset) * 0.75;
    let margin_top = f64::from(top * 20 + y_offset) * 0.75;
    let (visibility, marker) = if visible {
        ("visible", "<x:Visible/>")
    } else {
        ("hidden", "")
    };
    let shape_type = xml::attribute(shape_type);
    format!(
        concat!(
            "<v:shape{namespaces} id=\"_x0000_s{id}\" o:spid=\"_x0000_s{id}\" type=\"#{shape_type}\"",
            " style=\"position:absolute;margin-left:{margin_left}pt;margin-top:{margin_top}pt;width:96pt;height:55.5pt;z-index:{z_index};visibility:{visibility}\"",
            " fillcolor=\"#ffffe1\" o:insetmode=\"auto\"><v:fill color2=\"#ffffe1\"/>",
            "<v:shadow on=\"t\" color=\"black\" obscured=\"t\"/><v:path o:connecttype=\"none\"/>",
            "<v:textbox style=\"mso-direction-alt:auto\"><div style=\"text-align:left\"></div></v:textbox>",
            "<x:ClientData ObjectType=\"Note\"><x:MoveWithCells/><x:SizeWithCells/><x:Anchor>{anchor}</x:Anchor>",
            "<x:AutoFill>False</x:AutoFill><x:Row>{row}</x:Row><x:Column>{col}</x:Column>{marker}</x:ClientData></v:shape>"
        ),
        namespaces = namespaces,
        z_index = z_index,
        id = id,
        shape_type = shape_type,
        visibility = visibility,
        anchor = anchor,
        row = row,
        col = col,
        marker = marker,
        margin_left = margin_left,
        margin_top = margin_top
    )
}

pub(super) enum Addition {
    New(String),
    Existing(Vec<Patch>),
}

pub(super) struct AddShape {
    pub id: u32,
    pub new_block: Option<u32>,
    pub part: Addition,
}

pub(super) fn append(
    wb: &mut Workbook,
    sheet: &str,
    existing: Option<&(String, XmlDocument)>,
    at: CellIndex,
    visible: bool,
) -> Result<AddShape> {
    let (high, used) = crate::images::note_shape_numbers(wb, sheet)?;
    let mut edits = Vec::new();
    let mut prefix = String::new();
    let (part, doc) = existing.map_or((sheet, None), |(part, doc)| (part.as_str(), Some(doc)));
    let root = doc.map(XmlDocument::root);
    let vo = format!(" xmlns:v=\"{V}\" xmlns:o=\"{OFFICE}\"");
    let vox = format!("{vo} xmlns:x=\"{X}\"");
    let child_vo = if root.is_some() { vo.as_str() } else { "" };
    let child_vox = if root.is_some() { vox.as_str() } else { "" };
    let z_index = root
        .into_iter()
        .flat_map(walk)
        .filter(|node| node.is(QName::new(V, "shape")))
        .filter_map(|node| {
            node.attr(attr("style"))?.rsplit(';').find_map(|property| {
                let (name, value) = property.split_once(':')?;
                if name.trim().eq_ignore_ascii_case("z-index") {
                    value.trim().parse::<i64>().ok()
                } else {
                    None
                }
            })
        })
        .max()
        .unwrap_or_default()
        .max(0)
        .checked_add(1)
        .ok_or_else(|| part_error(part, "VML z-index exhausted"))?;
    if let Some(doc) = doc
        && note_shape(part, doc, at)?.is_some()
    {
        return Err(part_error(
            part,
            "a VML note shape already matches the cell",
        ));
    }
    let layout_node = root
        .map(|root| unique_child(root, LAYOUT, part))
        .transpose()?
        .flatten();
    let map = layout_node
        .map(|layout| unique_child(layout, IDMAP, part))
        .transpose()?
        .flatten();
    let current = map
        .map(|node| {
            let data = node
                .attr(attr("data"))
                .ok_or_else(|| part_error(part, "missing VML block data"))?;
            blocks(data, part)
        })
        .transpose()?
        .unwrap_or_default();
    let mut new_block = None;
    let id = if let Some(id) = pick(&current, high, &used) {
        id
    } else {
        let max = match wb.vml_block_high {
            Some(max) => max,
            None => {
                let max = all_blocks(&wb.pkg)?;
                wb.vml_block_high = Some(max);
                max
            }
        };
        let block = max
            .checked_add(1)
            .ok_or_else(|| part_error(part, "VML block exhausted"))?;
        new_block = Some(block);
        let id = in_blocks(&[block], 1, &used)
            .ok_or_else(|| part_error(part, "VML block has no free shape number"))?;
        if let Some(map) = map {
            let doc = doc.expect("existing idmap");
            let span = map.span().expect("parsed idmap");
            let tag = &doc.source()[span.start..span.open_end];
            let mut reader = quick_xml::Reader::from_str(tag);
            let event = reader
                .read_event()
                .map_err(|error| part_error(part, error))?;
            let (quick_xml::events::Event::Start(start) | quick_xml::events::Event::Empty(start)) =
                event
            else {
                unreachable!("idmap start tag")
            };
            let value = start
                .try_get_attribute("data")
                .map_err(|error| part_error(part, error))?
                .expect("parsed idmap data");
            let end = doc
                .source()
                .substr_range(value.value.as_ref())
                .expect("borrowed attribute")
                .end;
            edits.push((end..end, format!(",{block}")));
        } else if let Some(layout_node) = layout_node {
            edits.push(xml::insert(layout_node, None, format!(
                "<o:idmap xmlns:o=\"{OFFICE}\" xmlns:v=\"{V}\" v:ext=\"edit\" data=\"{block}\"/>"
            )));
        } else if let Some(root) = root.filter(|root| root.elements().next().is_some()) {
            edits.push(xml::insert(
                root,
                root.elements().next(),
                layout(block, child_vo),
            ));
        } else {
            prefix.push_str(&layout(block, child_vo));
        }
        id
    };
    let existing_type = root.and_then(|root| {
        walk(root).find(|node| {
            node.is(QName::new(V, "shapetype"))
                && node.attr(QName::new(OFFICE, "spt")) == Some("202")
        })
    });
    let shape_type = if let Some(node) = existing_type {
        node.attr(attr("id"))
            .filter(|id| !id.is_empty())
            .ok_or_else(|| part_error(part, "note shapetype has no id"))?
    } else {
        if root
            .is_some_and(|root| walk(root).any(|node| node.attr(attr("id")) == Some("_x0000_t202")))
        {
            return Err(part_error(part, "note shapetype id is already used"));
        }
        prefix.push_str(&note_type(child_vo));
        "_x0000_t202"
    };
    prefix.push_str(&shape(at, id, shape_type, visible, child_vox, z_index));
    let addition = if let Some(root) = root {
        edits.push(xml::insert(root, None, prefix));
        Addition::Existing(edits)
    } else {
        Addition::New(format!("<xml{vox}>{prefix}</xml>"))
    };
    Ok(AddShape {
        id,
        new_block,
        part: addition,
    })
}

pub(super) fn note_shape<'a>(
    part: &str,
    doc: &'a XmlDocument,
    at: CellIndex,
) -> Result<Option<&'a RawNode>> {
    let mut shapes = walk(doc.root()).filter(|shape| {
        shape.is(QName::new(V, "shape"))
            && shape.elements().any(|node| {
                let coord = |local| {
                    child(node, QName::new(X, local))
                        .and_then(|value| value.text().trim().parse::<u32>().ok())
                };
                node.is(QName::new(X, "ClientData"))
                    && node.attr(attr("ObjectType")) == Some("Note")
                    && coord("Row") == Some(at.row)
                    && coord("Column") == Some(at.col)
            })
    });
    let first = shapes.next();
    if shapes.next().is_some() {
        return Err(part_error(part, "multiple VML note shapes match the cell"));
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_follows_block_order_and_fills_gaps() {
        assert_eq!(pick(&[2, 1], 1025, &HashSet::from([1026])), Some(1027));
        assert_eq!(pick(&[2, 1], 5000, &HashSet::from([1025])), Some(1026));
        assert_eq!(pick(&[1], 1, &(1025..2048).collect()), None);
        assert_eq!(pick(&[1], u32::MAX, &HashSet::from([1025])), Some(1026));
    }
}
