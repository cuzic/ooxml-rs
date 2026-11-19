use super::*;

const RD: &str = "http://schemas.microsoft.com/office/spreadsheetml/2017/richdata";
const RVREL: &str = "http://schemas.microsoft.com/office/spreadsheetml/2022/richvaluerel";
const METADATA: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sheetMetadata";
const DATA: &str = "http://schemas.microsoft.com/office/2017/06/relationships/rdRichValue";
const STRUCTURE: &str =
    "http://schemas.microsoft.com/office/2017/06/relationships/rdRichValueStructure";
const RELATIONS: &str = "http://schemas.microsoft.com/office/2022/10/relationships/richValueRel";

fn unresolved(step: &'static str) -> Error {
    Error::UnresolvedRichValue { step }
}

fn part(
    pkg: &Package,
    rels: &Relationships,
    kind: &str,
    root: QName,
    step: &'static str,
) -> Result<(String, XmlDocument)> {
    let mut rel = rels.by_type(kind);
    let name = rel
        .next()
        .and_then(|rel| rels.resolve(rel))
        .ok_or_else(|| unresolved(step))?;
    if rel.next().is_some() {
        return Err(unresolved(step));
    }
    let bytes = pkg.read_part(&name).map_err(|_| unresolved(step))?;
    let doc =
        XmlDocument::parse_with(&bytes, &pkg.parse_options()).map_err(|_| unresolved(step))?;
    if !doc.root().is(root) {
        return Err(unresolved(step));
    }
    Ok((name, doc))
}

fn nth(node: &RawNode, name: QName, index: usize) -> Option<&RawNode> {
    node.elements().filter(|child| child.is(name)).nth(index)
}

fn index(value: Option<&str>, step: &'static str) -> Result<usize> {
    value
        .and_then(ooxml_core::value::parse_u32)
        .map(|index| index as usize)
        .ok_or_else(|| unresolved(step))
}

const fn s(local: &'static str) -> QName {
    QName::new(SML, local)
}
const fn rd(local: &'static str) -> QName {
    QName::new(RD, local)
}

pub(crate) fn image(pkg: &Package, workbook: &str, cell: &RawNode) -> Result<Option<CellImage>> {
    let Some(vm) = cell.attr(attr("vm")) else {
        return Ok(None);
    };
    let vm = index(Some(vm), "vm")?
        .checked_sub(1)
        .ok_or_else(|| unresolved("vm"))?;
    let rels = pkg
        .relationships(workbook)
        .map_err(|_| unresolved("workbook relationships"))?;
    let (_, metadata) = part(pkg, &rels, METADATA, s("metadata"), "metadata")?;
    let root = metadata.root();
    let block = child(root, s("valueMetadata"))
        .and_then(|node| nth(node, s("bk"), vm))
        .ok_or_else(|| unresolved("valueMetadata"))?;
    let types = child(root, s("metadataTypes")).ok_or_else(|| unresolved("metadataTypes"))?;
    let mut future_index = None;
    for record in block.elements().filter(|node| node.is(s("rc"))) {
        let ty = index(record.attr(attr("t")), "metadata type")?
            .checked_sub(1)
            .ok_or_else(|| unresolved("metadata type"))?;
        let ty = nth(types, s("metadataType"), ty).ok_or_else(|| unresolved("metadata type"))?;
        if ty.attr(attr("name")) == Some("XLRICHVALUE") {
            let value = index(record.attr(attr("v")), "futureMetadata index")?;
            if future_index.replace(value).is_some() {
                return Err(unresolved("ambiguous rich value metadata"));
            }
        }
    }
    let future_index = future_index.ok_or_else(|| unresolved("XLRICHVALUE record"))?;
    let future = root
        .elements()
        .find(|node| node.is(s("futureMetadata")) && node.attr(attr("name")) == Some("XLRICHVALUE"))
        .and_then(|node| nth(node, s("bk"), future_index))
        .ok_or_else(|| unresolved("futureMetadata"))?;
    let rvb = walk(future)
        .find(|node| node.is(rd("rvb")))
        .ok_or_else(|| unresolved("rvb"))?;
    let value_index = index(rvb.attr(attr("i")), "rvb index")?;
    let (_, values) = part(pkg, &rels, DATA, rd("rvData"), "rich value data")?;
    let value =
        nth(values.root(), rd("rv"), value_index).ok_or_else(|| unresolved("rich value"))?;
    let structure_index = index(value.attr(attr("s")), "structure index")?;
    let (_, structures) = part(
        pkg,
        &rels,
        STRUCTURE,
        rd("rvStructures"),
        "rich value structures",
    )?;
    let structure = nth(structures.root(), rd("s"), structure_index)
        .ok_or_else(|| unresolved("rich value structure"))?;
    let kind = structure
        .attr(attr("t"))
        .ok_or_else(|| unresolved("structure type"))?;
    if kind != "_localImage" {
        return Ok(None);
    }
    let mut media_index = None;
    let mut alt_text = None;
    for (position, key) in structure
        .elements()
        .filter(|node| node.is(rd("k")))
        .enumerate()
    {
        match key.attr(attr("n")) {
            Some("_rvRel:LocalImageIdentifier") => {
                let node = nth(value, rd("v"), position)
                    .ok_or_else(|| unresolved("local image identifier"))?;
                media_index = Some(index(Some(&node.text()), "local image identifier")?);
            }
            Some("Text") => {
                let node = nth(value, rd("v"), position).ok_or_else(|| unresolved("alt text"))?;
                alt_text = Some(crate::escape::decode(&node.text()).into_owned());
            }
            _ => {}
        }
    }
    let media_index = media_index.ok_or_else(|| unresolved("local image key"))?;
    let (name, relations) = part(
        pkg,
        &rels,
        RELATIONS,
        QName::new(RVREL, "richValueRels"),
        "rich value relationships",
    )?;
    let id = nth(relations.root(), QName::new(RVREL, "rel"), media_index)
        .and_then(|node| node.attr(QName::new(R, "id")))
        .ok_or_else(|| unresolved("image relationship id"))?;
    let rels = pkg
        .relationships(&name)
        .map_err(|_| unresolved("image relationships"))?;
    let media = read_media(pkg, &rels, id).ok_or_else(|| unresolved("image media"))?;
    Ok(Some(CellImage { media, alt_text }))
}
