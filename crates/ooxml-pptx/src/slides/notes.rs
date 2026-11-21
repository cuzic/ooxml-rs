//! Notes creation and notes-master provisioning (ADR-0016 §8).

use std::fmt::Write as _;

use ooxml_core::{
    ElementSpec, Lens, LensMut, NsDecl, PartNameRequest, QName, RawNode, RegistryPlan, Target,
    XmlDocument,
};
use ooxml_schema::{dml, pml};

use super::{Counts, xml_escape};
use crate::deck::{a, child, p, rel, un};
use crate::error::{Error, Result};
use crate::names::{
    A_NS, CT_NOTES_SLIDE, P_NS, R_NS, REL_NOTES_MASTER, REL_NOTES_SLIDE, REL_SLIDE,
};
use crate::{NotesMut, Rect, SlideInfo};

const MASTER: &[u8] = include_bytes!("../../template/parts/ppt/notesMasters/notesMaster1.xml");
const THEME: &[u8] = include_bytes!("../../template/parts/ppt/theme/theme2.xml");
const CT_MASTER: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const NOTES_SIZE: (i64, i64) = (6_858_000, 9_144_000);
const SLIDE_SIZE: (i64, i64) = (12_192_000, 6_858_000);
const RID: QName = pml::NotesMasterId::ATTR_R_ID;
// DrawingML ST_PositiveCoordinate, also the type of p:notesSz extents.
const MAX_PAGE_EMU: i64 = 27_273_042_316_900;

impl crate::Presentation {
    /// Ensures that the slide at `index` has a notes slide. Creates and
    /// links a notes master when needed, and fills in the presentation's
    /// notes-master list if it is missing.
    ///
    /// Returns [`Error::NotesWithoutMaster`] if the deck already contains
    /// notes slides but has no presentation-level notes master relationship.
    pub fn ensure_notes_slide(&mut self, index: usize) -> Result<()> {
        self.ensure_notes(index)?;
        Ok(())
    }

    /// Opens the slide's notes for editing, creating them if needed.
    ///
    /// Uses the same transaction as [`Self::ensure_notes_slide`].
    pub fn notes_mut_or_create(&mut self, index: usize) -> Result<NotesMut<'_>> {
        let (slide, part) = self.ensure_notes(index)?;
        Ok(self.notes_handle(slide, part))
    }

    fn ensure_notes(&mut self, index: usize) -> Result<(SlideInfo, String)> {
        let (slide, slide_part) = self.entry(index)?;
        let notes = crate::notes::notes_part(&self.pkg, &slide_part)?;
        let main = self.main.clone();
        let rels = self.rels_of(&main)?;
        let mut pres = self.tree(&main)?.clone();
        let listed_id = pres
            .lens::<pml::Presentation>()?
            .child::<pml::NotesMasterIdList>()
            .and_then(|list| list.child::<pml::NotesMasterId>())
            .and_then(|entry| entry.attr(RID));
        let master_rel = listed_id
            .and_then(|id| rels.by_id(id))
            .filter(|rel| rel.rel_type == REL_NOTES_MASTER)
            .or_else(|| rels.by_type(REL_NOTES_MASTER).next());
        if master_rel.is_none()
            && (notes.is_some()
                || self
                    .pkg
                    .part_names()
                    .any(|part| self.pkg.content_type(part) == Some(CT_NOTES_SLIDE)))
        {
            return Err(Error::NotesWithoutMaster);
        }
        if let Some(part) = &notes {
            self.tree(part)?.lens::<pml::NotesSlide>()?;
        }
        let mut plan = RegistryPlan::new(&self.pkg);
        let (master, rid, new_notes) = if let Some(rel) = master_rel {
            let part = rels.resolve(rel).ok_or_else(|| {
                Error::UnsupportedContent("invalid presentation notes master target".into())
            })?;
            let master_doc = self.tree(&part)?;
            master_doc.lens::<pml::NotesMaster>()?;
            let new_notes = notes
                .is_none()
                .then(|| notes_document(master_doc))
                .transpose()?;
            (part, rel.id.clone(), new_notes)
        } else {
            let master_doc = scaled_master(pres.root())?;
            let new_notes = notes_document(&master_doc)?;
            let master = plan.add_part(
                &self.pkg,
                PartNameRequest::Numbered("ppt/notesMasters/notesMaster{}.xml".into()),
                CT_MASTER,
                master_doc,
            )?;
            let theme = plan.add_part(
                &self.pkg,
                PartNameRequest::Numbered("ppt/theme/theme{}.xml".into()),
                CT_THEME,
                THEME,
            )?;
            plan.add_relationship(&self.pkg, Some(&master), rel::THEME, Target::Part(theme))?;
            let rid = plan.add_relationship(
                &self.pkg,
                Some(&main),
                REL_NOTES_MASTER,
                Target::Part(master.clone()),
            )?;
            (master, rid, Some(new_notes))
        };
        let list_changed = set_master_id(&mut pres, &rid)?;
        if list_changed {
            plan.stage_part(&self.pkg, &main, pres)?;
        }

        let new_notes_created = new_notes.is_some();
        let notes = if let Some(doc) = new_notes {
            let part = plan.add_part(
                &self.pkg,
                PartNameRequest::Numbered("ppt/notesSlides/notesSlide{}.xml".into()),
                CT_NOTES_SLIDE,
                doc,
            )?;
            plan.add_relationship(
                &self.pkg,
                Some(&part),
                REL_NOTES_MASTER,
                Target::Part(master),
            )?;
            plan.add_relationship(
                &self.pkg,
                Some(&part),
                REL_SLIDE,
                Target::Part(slide_part.clone()),
            )?;
            plan.add_relationship(
                &self.pkg,
                Some(&slide_part),
                REL_NOTES_SLIDE,
                Target::Part(part.clone()),
            )?;
            if let Some((app, doc)) = self.app_with(Counts {
                notes: 1,
                ..Counts::default()
            })? {
                plan.stage_part(&self.pkg, &app, doc)?;
            }
            part
        } else {
            notes.expect("existing notes")
        };
        if list_changed || new_notes_created {
            // The slide list and its relationships are unchanged.
            self.pkg.apply(plan)?;
        }
        Ok((slide, notes))
    }
}

fn set_master_id(pres: &mut XmlDocument, rid: &str) -> Result<bool> {
    let mut root = pres.lens_mut::<pml::Presentation>()?;
    let mut list = root.get_or_insert::<pml::NotesMasterIdList>()?;
    let mut entry = list.get_or_insert_with_ns::<pml::NotesMasterId>(&[NsDecl::new("r", R_NS)])?;
    if entry.node().attr(RID) == Some(rid) {
        return Ok(false);
    }
    entry.preserve_attribute_spelling();
    entry.set_attr(RID, rid)?;
    Ok(true)
}

fn placeholder(shape: &RawNode) -> Option<&RawNode> {
    let nv = child(shape, p("nvSpPr"))?;
    child(child(nv, p("nvPr"))?, p("ph"))
}

fn master_shapes(master: &XmlDocument) -> Result<Option<Lens<'_, pml::ShapeTree>>> {
    Ok(master
        .lens::<pml::NotesMaster>()?
        .child::<pml::CommonSlideData>()
        .and_then(|slide| slide.child::<pml::ShapeTree>()))
}

fn notes_document(master: &XmlDocument) -> Result<XmlDocument> {
    let mut xml = format!(
        "<p:notes xmlns:p=\"{P_NS}\" xmlns:a=\"{A_NS}\"><p:cSld><p:spTree>\
         <p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"
    );
    let mut id = 2;
    let mut has_body = false;
    if let Some(tree) = master_shapes(master)? {
        for shape in tree.children::<pml::Shape>() {
            let Some(ph) = placeholder(shape.raw()) else {
                continue;
            };
            let Some(kind @ ("sldImg" | "body" | "sldNum")) = ph.attr(un("type")) else {
                continue;
            };
            // Keep the attributes that identify the inherited placeholder.
            write_placeholder(&mut xml, id, kind, Some(ph));
            id += 1;
            has_body |= kind == "body";
        }
    }
    if !has_body {
        write_placeholder(&mut xml, id, "body", None);
    }
    xml.push_str("</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>");
    Ok(XmlDocument::parse(xml.as_bytes())?)
}

fn write_placeholder(xml: &mut String, id: u32, kind: &str, source: Option<&RawNode>) {
    let name = match kind {
        "sldImg" => "Slide Image",
        "sldNum" => "Slide Number",
        _ => "Notes",
    };
    write!(
        xml,
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name} Placeholder {}\"/>\
         <p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"{kind}\"",
        id - 1,
    )
    .expect("writing to String");
    for attr in ["idx", "sz", "orient"] {
        if let Some(value) = source.and_then(|ph| ph.attr(un(attr))) {
            write!(xml, " {attr}=\"{}\"", xml_escape(value)).expect("writing to String");
        }
    }
    xml.push_str("/></p:nvPr></p:nvSpPr><p:spPr/>");
    if kind == "body" {
        xml.push_str("<p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody>");
    }
    xml.push_str("</p:sp>");
}

fn size(root: &RawNode, element: &'static str, default: (i64, i64)) -> (i64, i64) {
    let Some(node) = child(root, p(element)) else {
        return default;
    };
    let dimension = |name| {
        node.attr(un(name))?
            .trim_ascii()
            .parse::<i64>()
            .ok()
            .filter(|v| (1..=MAX_PAGE_EMU).contains(v))
    };
    dimension("cx").zip(dimension("cy")).unwrap_or(default)
}

fn scale(rect: Rect, page: (i64, i64)) -> Rect {
    // The embedded master has nonnegative coordinates. Round to the
    // nearest EMU, with half an EMU rounded up.
    let axis = |value, target, original| {
        ((i128::from(value) * i128::from(target) + i128::from(original) / 2) / i128::from(original))
            as i64
    };
    Rect {
        x: axis(rect.x, page.0, NOTES_SIZE.0),
        y: axis(rect.y, page.1, NOTES_SIZE.1),
        cx: axis(rect.cx, page.0, NOTES_SIZE.0),
        cy: axis(rect.cy, page.1, NOTES_SIZE.1),
    }
}

fn fit(rect: Rect, (width, height): (i64, i64)) -> Rect {
    let scaled_width = i128::from(rect.cy) * i128::from(width);
    let scaled_height = i128::from(rect.cx) * i128::from(height);
    let (cx, cy) = if scaled_width <= scaled_height {
        ((scaled_width / i128::from(height)) as i64, rect.cy)
    } else {
        (rect.cx, (scaled_height / i128::from(width)) as i64)
    };
    Rect {
        x: rect.x + (rect.cx - cx) / 2,
        y: rect.y + (rect.cy - cy) / 2,
        cx,
        cy,
    }
}

fn scaled_master(presentation: &RawNode) -> Result<XmlDocument> {
    let page = size(presentation, "notesSz", NOTES_SIZE);
    let slide = size(presentation, "sldSz", SLIDE_SIZE);
    let mut master = XmlDocument::parse(MASTER)?;
    let tree = master_shapes(&master)?.expect("embedded notes master has a shape tree");
    let shapes: Vec<_> = tree
        .children::<pml::Shape>()
        .filter_map(|shape| {
            let ph = placeholder(shape.raw())?;
            Some((shape.path().to_vec(), ph.attr(un("type")) == Some("sldImg")))
        })
        .collect();
    for (path, is_slide_image) in shapes {
        let mut shape = master.lens_mut_at::<pml::Shape>(&path)?;
        let mut props = shape
            .child_mut::<pml::ShapeProperties>()
            .expect("template shape properties");
        let mut xfrm = props
            .child_mut::<dml::Transform2D>()
            .expect("template shape transform");
        let read = |element, attr| {
            child(xfrm.node(), a(element))
                .and_then(|n| n.attr(un(attr)))
                .and_then(|v| v.parse::<i64>().ok())
                .expect("template transform coordinate")
        };
        let template = Rect {
            x: read("off", "x"),
            y: read("off", "y"),
            cx: read("ext", "cx"),
            cy: read("ext", "cy"),
        };
        let mut rect = scale(template, page);
        if is_slide_image {
            rect = fit(rect, slide);
        }
        let mut off = xfrm.child_mut::<dml::Offset>().expect("template offset");
        set_coordinate(&mut off, "x", rect.x)?;
        set_coordinate(&mut off, "y", rect.y)?;
        let mut ext = xfrm.child_mut::<dml::Extents>().expect("template extents");
        set_coordinate(&mut ext, "cx", rect.cx)?;
        set_coordinate(&mut ext, "cy", rect.cy)?;
    }
    Ok(master)
}

fn set_coordinate<E: ElementSpec>(
    node: &mut LensMut<'_, E>,
    attr: &'static str,
    value: i64,
) -> Result<()> {
    let mut buf = core::fmt::NumBuffer::new();
    node.set_attr(un(attr), value.format_into(&mut buf))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_master_transforms_follow_page_size_and_slide_aspect_ratio() {
        let cases = [
            (
                "template",
                NOTES_SIZE,
                SLIDE_SIZE,
                [685800, 1143000, 5486400, 3086100],
                [685800, 4400550, 5486400, 3600450],
            ),
            (
                "A4 / 16:10",
                (7560000, 10692000),
                (12192000, 7620000),
                [893160, 1336500, 5773680, 3608550],
                [756000, 5145525, 6048000, 4209975],
            ),
            (
                "Letter",
                (7772400, 10058400),
                SLIDE_SIZE,
                [868680, 1257300, 6035040, 3394710],
                [777240, 4840605, 6217920, 3960495],
            ),
            (
                "4:3",
                NOTES_SIZE,
                (9144000, 6858000),
                [1371600, 1143000, 4114800, 3086100],
                [685800, 4400550, 5486400, 3600450],
            ),
            (
                "16:10",
                NOTES_SIZE,
                (12192000, 7620000),
                [960120, 1143000, 4937760, 3086100],
                [685800, 4400550, 5486400, 3600450],
            ),
            (
                "21:9",
                NOTES_SIZE,
                (16002000, 6858000),
                [685800, 1510393, 5486400, 2351314],
                [685800, 4400550, 5486400, 3600450],
            ),
            (
                "landscape / 21:9",
                (9144000, 6858000),
                (16002000, 6858000),
                [1871662, 857250, 5400675, 2314575],
                [914400, 3300413, 7315200, 2700338],
            ),
        ];
        for (name, (px, py), (sx, sy), image, body) in cases {
            let pres = XmlDocument::parse(format!(
                "<p:presentation xmlns:p=\"{P_NS}\"><p:sldSz cx=\"{sx}\" cy=\"{sy}\"/><p:notesSz cx=\"{px}\" cy=\"{py}\"/></p:presentation>"
            ).as_bytes()).unwrap();
            let output = scaled_master(pres.root()).unwrap();
            let master = XmlDocument::parse(output.serialize().as_bytes()).unwrap();
            let tree = master_shapes(&master).unwrap().unwrap();
            for (kind, expected) in [("sldImg", image), ("body", body)] {
                let shape = tree
                    .children::<pml::Shape>()
                    .find(|s| placeholder(s.raw()).unwrap().attr(un("type")) == Some(kind))
                    .unwrap();
                let xfrm = shape
                    .child::<pml::ShapeProperties>()
                    .unwrap()
                    .child::<dml::Transform2D>()
                    .unwrap();
                let off = xfrm.child::<dml::Offset>().unwrap();
                let ext = xfrm.child::<dml::Extents>().unwrap();
                let actual = [
                    off.attr(un("x")),
                    off.attr(un("y")),
                    ext.attr(un("cx")),
                    ext.attr(un("cy")),
                ]
                .map(|v| v.unwrap().parse::<i64>().unwrap());
                assert_eq!(actual, expected, "{name}: {kind}");
            }
        }
    }
}
