use std::fmt::Write;

use super::*;

pub(super) struct Position {
    anchor: Anchor,
    extent: (u64, u64),
}

impl Position {
    pub(super) fn new(anchor: Anchor, natural: (u64, u64)) -> Result<Self> {
        let coord = |value: i64| value.unsigned_abs() <= header::MAX_EMU;
        let marker =
            |m: Marker| CellIndex::new(m.cell.row, m.cell.col).is_ok() && coord(m.x) && coord(m.y);
        let extent = match anchor {
            Anchor::OneCell { from, size } if marker(from) => match size {
                Size::Natural => natural,
                Size::Emu { width, height } => (width, height),
            },
            Anchor::TwoCell { from, to, .. }
                if marker(from)
                    && marker(to)
                    && (from.cell.row, from.y) <= (to.cell.row, to.y)
                    && (from.cell.col, from.x) <= (to.cell.col, to.x)
                    && from != to =>
            {
                natural
            }
            Anchor::Absolute {
                x,
                y,
                width,
                height,
            } if coord(x) && coord(y) => (width, height),
            _ => return Err(Error::ImageAnchor),
        };
        if !header::valid_extent(extent.0, extent.1) {
            return Err(Error::ImageSize);
        }
        Ok(Self { anchor, extent })
    }

    pub(super) fn picture(
        &self,
        id: u32,
        image: &Image,
        embed: &str,
        svg: Option<&str>,
        hyperlink: Option<&str>,
    ) -> String {
        let (tag, behavior) = match self.anchor {
            Anchor::OneCell { .. } => ("oneCellAnchor", ""),
            Anchor::TwoCell { edit_as, .. } => (
                "twoCellAnchor",
                match edit_as {
                    EditAs::OneCell => " editAs=\"oneCell\"",
                    EditAs::TwoCell => " editAs=\"twoCell\"",
                    EditAs::Absolute => " editAs=\"absolute\"",
                },
            ),
            Anchor::Absolute { .. } => ("absoluteAnchor", ""),
            Anchor::Unresolved => unreachable!("validated anchor"),
        };
        let mut xml =
            format!("<xdr:{tag} xmlns:xdr=\"{XDR}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"{behavior}>");
        let (cx, cy) = self.extent;
        match self.anchor {
            Anchor::OneCell { from, .. } => {
                write_marker(&mut xml, "from", from);
                write!(xml, "<xdr:ext cx=\"{cx}\" cy=\"{cy}\"/>").unwrap();
            }
            Anchor::TwoCell { from, to, .. } => {
                write_marker(&mut xml, "from", from);
                write_marker(&mut xml, "to", to);
            }
            Anchor::Absolute { x, y, .. } => {
                write!(
                    xml,
                    "<xdr:pos x=\"{x}\" y=\"{y}\"/><xdr:ext cx=\"{cx}\" cy=\"{cy}\"/>"
                )
                .unwrap();
            }
            Anchor::Unresolved => unreachable!("validated anchor"),
        }
        write!(xml, "<xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{id}\"").unwrap();
        if let Some(name) = &image.name {
            write_xstring_attribute(&mut xml, "name", name);
        } else {
            write!(xml, " name=\"Picture {}\"", id - 1).unwrap();
        }
        if let Some(text) = &image.alt_text {
            write_xstring_attribute(&mut xml, "descr", text);
        }
        if let Some(link) = hyperlink {
            write!(
                xml,
                "><a:hlinkClick r:id=\"{}\"/></xdr:cNvPr>",
                quick_xml::escape::escape(link)
            )
            .unwrap();
        } else {
            xml.push_str("/>");
        }
        write!(xml, "<xdr:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed=\"{}\"", quick_xml::escape::escape(embed)).unwrap();
        if let Some(svg) = svg {
            write!(xml, "><a:extLst><a:ext uri=\"{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}\"><asvg:svgBlip xmlns:asvg=\"{ASVG}\" r:embed=\"{}\"/></a:ext></a:extLst></a:blip>", quick_xml::escape::escape(svg)).unwrap();
        } else {
            xml.push_str("/>");
        }
        write!(
            xml,
            concat!(
                "<a:stretch><a:fillRect/></a:stretch></xdr:blipFill>",
                "<xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>",
                "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr>",
                "</xdr:pic><xdr:clientData/></xdr:{}>"
            ),
            cx, cy, tag
        )
        .unwrap();
        xml
    }
}

fn write_xstring_attribute(xml: &mut String, name: &str, value: &str) {
    use quick_xml::events::attributes::Attribute;
    let attribute = Attribute::from((name, crate::escape::encode(value)));
    let escaped = attribute.value;
    write!(xml, " {name}=\"{escaped}\"").unwrap();
}

fn write_marker(xml: &mut String, tag: &str, marker: Marker) {
    let Marker { cell, x, y } = marker;
    let CellIndex { row, col } = cell;
    write!(xml, "<xdr:{tag}><xdr:col>{col}</xdr:col><xdr:colOff>{x}</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>{y}</xdr:rowOff></xdr:{tag}>").unwrap();
}

pub(super) fn append(doc: Option<&XmlDocument>, anchor: &str) -> String {
    let Some(doc) = doc else {
        return format!("<xdr:wsDr xmlns:xdr=\"{XDR}\">{anchor}</xdr:wsDr>");
    };
    // wsDr is a choice list, so the insertion does not need to interpret
    // an AlternateContent sibling. Root and existing child bytes are kept.
    let root = doc.root();
    let span = root.span().expect("parsed wsDr");
    let source = doc.source();
    if let Some((close, _)) = span.close {
        return format!("{}{anchor}{}", &source[..close], &source[close..]);
    }
    let qualified = root
        .name()
        .prefix()
        .map_or(String::from("wsDr"), |prefix| format!("{prefix}:wsDr"));
    format!(
        "{}>{anchor}</{qualified}>{}",
        &source[..span.open_end - 2],
        &source[span.open_end..]
    )
}

fn is_anchor(node: &RawNode) -> bool {
    ["oneCellAnchor", "twoCellAnchor", "absoluteAnchor"]
        .into_iter()
        .any(|name| node.is(xdr(name)))
}

fn position(node: &RawNode) -> Option<Anchor> {
    let marker = |name| {
        let node = child(node, xdr(name))?;
        let value = |name| child(node, xdr(name)).map(RawNode::text);
        Some(Marker {
            cell: CellIndex::new(
                value("row")?.trim().parse().ok()?,
                value("col")?.trim().parse().ok()?,
            )
            .ok()?,
            x: value("colOff")?.trim().parse().ok()?,
            y: value("rowOff")?.trim().parse().ok()?,
        })
    };
    let extent = || {
        let ext = child(node, xdr("ext"))?;
        Some((
            ext.attr(attr("cx"))?.trim().parse().ok()?,
            ext.attr(attr("cy"))?.trim().parse().ok()?,
        ))
    };
    match node.name().local() {
        "oneCellAnchor" => {
            let (width, height) = extent()?;
            Some(Anchor::one_cell(
                marker("from")?,
                Size::Emu { width, height },
            ))
        }
        "twoCellAnchor" => {
            let edit_as = match node.attr(attr("editAs")) {
                Some("oneCell") => EditAs::OneCell,
                Some("absolute") => EditAs::Absolute,
                None | Some("twoCell") => EditAs::TwoCell,
                _ => return None,
            };
            Some(Anchor::two_cell(marker("from")?, marker("to")?).edit_as(edit_as))
        }
        "absoluteAnchor" => {
            let (width, height) = extent()?;
            let pos = child(node, xdr("pos"))?;
            Some(Anchor::absolute(
                pos.attr(attr("x"))?.trim().parse().ok()?,
                pos.attr(attr("y"))?.trim().parse().ok()?,
                width,
                height,
            ))
        }
        _ => None,
    }
}

fn properties(picture: &RawNode) -> Option<&RawNode> {
    child(child(picture, xdr("nvPicPr"))?, xdr("cNvPr"))
}

fn shape_id(node: &RawNode) -> Option<u32> {
    node.attr(attr("id"))?.trim().parse().ok()
}

pub(super) fn list(pkg: &Package, rels: &Relationships, root: &RawNode) -> Vec<ImageInfo> {
    let mut output = Vec::new();
    let mut nodes: Vec<_> = root
        .elements()
        .map(|node| (node, Anchor::Unresolved, false))
        .collect();
    nodes.reverse();
    while let Some((node, anchor, wrapped)) = nodes.pop() {
        let wrapped = wrapped || node.is(MC_ALTERNATE_CONTENT);
        let anchor = if wrapped {
            Anchor::Unresolved
        } else if is_anchor(node) {
            position(node).unwrap_or(Anchor::Unresolved)
        } else {
            anchor
        };
        if node.is(xdr("pic")) {
            let props = properties(node);
            let blip =
                child(node, xdr("blipFill")).and_then(|fill| child(fill, QName::new(A, "blip")));
            let media = |node: &RawNode| read_media(pkg, rels, node.attr(QName::new(R, "embed"))?);
            output.push(ImageInfo {
                anchor,
                id: props.and_then(shape_id),
                name: props
                    .and_then(|props| props.attr(attr("name")))
                    .map(|text| crate::escape::decode(text).into_owned()),
                alt_text: props
                    .and_then(|props| props.attr(attr("descr")))
                    .map(|text| crate::escape::decode(text).into_owned()),
                media: blip.and_then(media),
                svg: blip
                    .and_then(|blip| walk(blip).find(|node| node.is(QName::new(ASVG, "svgBlip"))))
                    .and_then(media),
            });
        } else {
            let start = nodes.len();
            nodes.extend(node.elements().map(|node| (node, anchor, wrapped)));
            nodes[start..].reverse();
        }
    }
    output
}

pub(super) fn removable(root: &RawNode, id: u32) -> Option<(&RawNode, &RawNode)> {
    let mut matches = walk(root).filter(|node| node.is(xdr("cNvPr")) && shape_id(node) == Some(id));
    let props = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    for anchor in root.elements().filter(|node| is_anchor(node)) {
        let mut picture = None;
        for node in anchor.elements() {
            if node.is(xdr("pic")) && picture.is_none() {
                picture = Some(node);
                continue;
            }
            if !["from", "to", "pos", "ext", "clientData"]
                .into_iter()
                .any(|name| node.is(xdr(name)))
            {
                picture = None;
                break;
            }
        }
        if let Some(picture) = picture
            && properties(picture).is_some_and(|node| std::ptr::eq(node, props))
        {
            return Some((anchor, picture));
        }
    }
    None
}
