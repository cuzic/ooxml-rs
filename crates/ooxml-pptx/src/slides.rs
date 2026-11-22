//! Phase 2: slides as units (ADR-0012). Every operation is **one** core
//! registry transaction (ADR-0012 §2): the trees that change are cloned from
//! the parsed parts and edited through the lens, every refusal and every
//! registry step is recorded in one [`RegistryPlan`], and one
//! [`Package::apply`] makes the change. A refusal — this crate's or the
//! core's — happens before `apply`, so the package stays byte-identical.

use ooxml_core::opc::{Relationships, TargetMode};
use ooxml_core::{
    ElementSpec, LensMut, PartNameRequest, QName, RawNode, RegistryPlan, Target, XmlDocument,
};
use ooxml_schema::pml;

use crate::error::{Error, Result};
use crate::names::*;
use crate::write::SlideMut;
mod notes;

/// Largest `p:sldId@id` (`ST_SlideId`: 256 ≤ id < 2³¹).
const MAX_SLIDE_ID: u32 = 2_147_483_647;
/// Largest shape id (POI reads ids as `int`).
const MAX_SHAPE_ID: u32 = i32::MAX as u32;

/// Relationship types `delete_slide` removes with the slide when nothing
/// outside the removed set references them (ADR-0012 §6).
const CASCADE_TYPES: &[&str] = &[
    REL_NOTES_SLIDE,
    REL_COMMENTS,
    MODERN_COMMENTS_REL,
    REL_IMAGE,
    REL_MEDIA,
    REL_AUDIO,
    REL_VIDEO,
    REL_HDPHOTO,
    REL_CHART,
    REL_PACKAGE,
    REL_CHART_STYLE,
    REL_CHART_COLOR_STYLE,
    REL_CHART_USER_SHAPES,
    REL_DIAGRAM_DATA,
    REL_DIAGRAM_LAYOUT,
    REL_DIAGRAM_STYLE,
    REL_DIAGRAM_COLORS,
    REL_DIAGRAM_DRAWING,
    REL_OLE_OBJECT,
    REL_VML_DRAWING,
    REL_TAGS,
    REL_THEME_OVERRIDE,
];

/// Relationship types that never cascade: the relationship goes with its
/// source, the target stays (layouts, masters, links).
const KEEP_TYPES: &[&str] = &[REL_SLIDE_LAYOUT, REL_NOTES_MASTER, REL_HYPERLINK, REL_SLIDE];

/// Relationship types `duplicate_slide` copies, sharing the target (or
/// mapping it to a copy: notes, tags). ADR-0012 §4.4.
const DUPLICATE_TYPES: &[&str] = &[
    REL_SLIDE_LAYOUT,
    REL_NOTES_SLIDE,
    REL_NOTES_MASTER,
    REL_IMAGE,
    REL_MEDIA,
    REL_AUDIO,
    REL_VIDEO,
    REL_HDPHOTO,
    REL_HYPERLINK,
    REL_SLIDE,
    REL_TAGS,
];

/// Comment relationship types: not copied by `duplicate_slide`.
const COMMENT_TYPES: &[&str] = &[REL_COMMENTS, MODERN_COMMENTS_REL];

/// A slide layout reachable from a master in `p:sldMasterIdLst`
/// (ADR-0012 §4.1; the subset of ADR-0008's `LayoutRef` Phase 2 needs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutRef {
    /// Index of the master in `p:sldMasterIdLst`.
    pub master_index: usize,
    /// The master part.
    pub master_part: String,
    /// The layout part.
    pub part: String,
    /// `p:cSld@name`.
    pub name: Option<String>,
    /// `p:sldLayout@type` (`title`, `obj`, `blank`, …).
    pub layout_type: Option<String>,
}

/// Count changes for `docProps/app.xml` (ADR-0012 §7).
#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    slides: i64,
    notes: i64,
    hidden: i64,
}

fn q(ns: &'static str, local: &'static str) -> QName {
    QName::new(ns, local)
}

fn is_hidden(slide: &RawNode) -> bool {
    matches!(slide.attr(q("", "show")), Some("0" | "false"))
}

fn walk<'a>(n: &'a RawNode, f: &mut impl FnMut(&'a RawNode)) {
    f(n);
    for c in n.elements() {
        walk(c, f);
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

impl crate::Presentation {
    // ------------------------------------------------------------ reading

    fn rels_of(&self, part: &str) -> Result<Relationships> {
        Ok(self.pkg.relationships(part)?)
    }

    fn parse_part(&self, part: &str) -> Result<XmlDocument> {
        Ok(XmlDocument::parse_with(
            &self.pkg.read_part(part)?,
            &self.pkg.parse_options(),
        )?)
    }

    /// The tree of a part, parsed once (the part stays raw-copied on save
    /// while unedited).
    fn tree(&mut self, part: &str) -> Result<&XmlDocument> {
        Ok(self.pkg.xml_part(part)?)
    }

    fn slide_part(&self, index: usize) -> Result<String> {
        let e = self.slides.get(index).ok_or(Error::SlideIndexOutOfRange {
            index,
            count: self.slides.len(),
        })?;
        e.part.clone().ok_or(Error::DanglingSlide {
            slide_id: e.slide_id,
        })
    }

    /// The presentation's relationship `Id` for the slide at `index`.
    fn slide_rel_id(&mut self, index: usize) -> Result<String> {
        let main = self.main.clone();
        let slide_id = self.slides.get(index).map_or(0, |e| e.slide_id);
        let doc = self.tree(&main)?;
        doc.root()
            .elements()
            .find(|e| e.is(pml::SlideIdList::QNAME))
            .and_then(|l| {
                l.elements()
                    .filter(|e| e.is(pml::SlideId::QNAME))
                    .nth(index)
            })
            .and_then(|e| e.attr(pml::SlideId::ATTR_R_ID))
            .map(str::to_owned)
            .ok_or(Error::DanglingSlide { slide_id })
    }

    /// The slide layouts of every master in `p:sldMasterIdLst`, in master
    /// then `p:sldLayoutIdLst` order.
    pub fn layouts(&mut self) -> Result<Vec<LayoutRef>> {
        let main = self.main.clone();
        let rels = self.rels_of(&main)?;
        let master_rids: Vec<String> = self
            .tree(&main)?
            .root()
            .elements()
            .filter(|e| e.is(q(P_NS, "sldMasterIdLst")))
            .flat_map(|l| l.elements().filter(|e| e.is(q(P_NS, "sldMasterId"))))
            .filter_map(|e| e.attr(q(R_NS, "id")).map(str::to_owned))
            .collect();
        let mut out = Vec::new();
        for (master_index, rid) in master_rids.iter().enumerate() {
            let Some(master) = rels
                .by_id(rid)
                .and_then(|r| rels.resolve(r))
                .filter(|p| self.pkg.contains(p))
            else {
                continue;
            };
            let mrels = self.rels_of(&master)?;
            let layout_rids: Vec<String> = self
                .tree(&master)?
                .root()
                .elements()
                .filter(|e| e.is(q(P_NS, "sldLayoutIdLst")))
                .flat_map(|l| l.elements().filter(|e| e.is(q(P_NS, "sldLayoutId"))))
                .filter_map(|e| e.attr(q(R_NS, "id")).map(str::to_owned))
                .collect();
            for lrid in layout_rids {
                let Some(part) = mrels
                    .by_id(&lrid)
                    .and_then(|r| mrels.resolve(r))
                    .filter(|p| self.pkg.contains(p))
                else {
                    continue;
                };
                let root = self.tree(&part)?.root();
                let name = root
                    .elements()
                    .find(|e| e.is(q(P_NS, "cSld")))
                    .and_then(|c| c.attr(q("", "name")))
                    .map(str::to_owned);
                let layout_type = root.attr(q("", "type")).map(str::to_owned);
                out.push(LayoutRef {
                    master_index,
                    master_part: master.clone(),
                    part,
                    name,
                    layout_type,
                });
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------ checks

    /// `presProps` `p:showPr/p:sldRg` refuses every position change.
    fn check_show_range(&mut self) -> Result<()> {
        let main = self.main.clone();
        let rels = self.rels_of(&main)?;
        let Some(props) = rels
            .by_type(REL_PRES_PROPS)
            .filter_map(|r| rels.resolve(r))
            .find(|p| self.pkg.contains(p))
        else {
            return Ok(());
        };
        let mut found = false;
        walk(self.tree(&props)?.root(), &mut |n| {
            found |= n.is(q(P_NS, "sldRg"))
        });
        if found {
            Err(Error::ShowRangeByPosition)
        } else {
            Ok(())
        }
    }

    fn slide_ids(&self) -> Vec<u32> {
        self.slides.iter().map(|e| e.slide_id).collect()
    }

    /// The section lists (`None` without sections), checked against
    /// `p:sldIdLst` (ADR-0012 §5).
    fn sections(&mut self) -> Result<Option<Vec<Vec<u32>>>> {
        let ids = self.slide_ids();
        let main = self.main.clone();
        let root = self.tree(&main)?.root();
        let mut all = 0usize;
        walk(root, &mut |n| {
            all += usize::from(n.is(q(P14_NS, "sectionLst")))
        });
        if all == 0 {
            return Ok(None);
        }
        let list = root
            .elements()
            .filter(|e| e.is(q(P_NS, "extLst")))
            .flat_map(|l| l.elements())
            .filter(|e| e.is(q(P_NS, "ext")) && e.attr(q("", "uri")) == Some(SECTION_LIST_URI))
            .flat_map(|e| e.elements())
            .filter(|e| e.is(q(P14_NS, "sectionLst")))
            .collect::<Vec<_>>();
        if all != 1 || list.len() != 1 {
            return Err(Error::UnsupportedSections);
        }
        let mut sections = Vec::new();
        for sec in list[0].elements() {
            if !sec.is(q(P14_NS, "section")) {
                return Err(Error::UnsupportedSections);
            }
            let mut v = Vec::new();
            for l in sec.elements().filter(|e| e.is(q(P14_NS, "sldIdLst"))) {
                for s in l.elements() {
                    let id = s
                        .attr(q("", "id"))
                        .and_then(|v| v.trim().parse::<u32>().ok());
                    v.push(id.ok_or(Error::SectionsInconsistent)?);
                }
            }
            sections.push(v);
        }
        if sections.concat() != ids {
            return Err(Error::SectionsInconsistent);
        }
        Ok(Some(sections))
    }

    /// Every relationships part must parse (fail closed, ADR-0012 §4.2 N4).
    fn check_rels_readable(&self) -> Result<()> {
        for name in self.pkg.part_names() {
            if !name.to_ascii_lowercase().ends_with(".rels") {
                continue;
            }
            let ok = self
                .pkg
                .read_part(name)
                .ok()
                .is_some_and(|b| Relationships::parse(None, &b).is_ok());
            if !ok {
                return Err(Error::SlideIsReferenced {
                    from_part: name.to_owned(),
                    rel_type: "?".into(),
                });
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ allocation

    /// The next `p:sldId@id`: above every id in the list and every id
    /// allocated or removed in this session, at least 256 (ADR-0012 §3).
    fn next_slide_id(&self) -> Result<u32> {
        let max = self
            .slide_ids()
            .into_iter()
            .chain([self.slide_id_high_water, 255])
            .max()
            .unwrap_or(255);
        max.checked_add(1)
            .filter(|&n| n <= MAX_SLIDE_ID)
            .ok_or(Error::SlideIdsExhausted)
    }

    /// The largest `p14:creationId@val` of every slide and notes slide;
    /// new values go above it (ADR-0012 §3).
    fn max_creation_id(&mut self) -> Result<u32> {
        let mut parts: Vec<String> = self.slides.iter().filter_map(|e| e.part.clone()).collect();
        parts.extend(
            self.pkg
                .part_names()
                .filter(|p| self.pkg.content_type(p) == Some(CT_NOTES_SLIDE))
                .map(str::to_owned),
        );
        let mut max = 0u32;
        for p in parts {
            walk(self.tree(&p)?.root(), &mut |n| {
                if n.is(pml::CreationId::QNAME)
                    && let Some(v) = n
                        .attr(pml::CreationId::ATTR_VAL)
                        .and_then(|v| v.trim().parse::<u32>().ok())
                {
                    max = max.max(v);
                }
            });
        }
        Ok(max)
    }

    // ------------------------------------------------------------ staged edits

    /// A clone of the presentation tree with `id`/`rid` inserted at
    /// position `at` of `p:sldIdLst`, and the section membership updated.
    fn pres_with_inserted(
        &mut self,
        at: usize,
        id: u32,
        rid: &str,
        sections: &Option<Vec<Vec<u32>>>,
    ) -> Result<XmlDocument> {
        let ids = self.slide_ids();
        let main = self.main.clone();
        let mut doc = self.tree(&main)?.clone();
        {
            let mut root = doc.lens_mut::<pml::Presentation>()?;
            let mut list = root.get_or_insert::<pml::SlideIdList>()?;
            let mut s = list.insert_by::<pml::SlideId>(|k, _| k == at)?;
            s.set_attr(pml::SlideId::ATTR_ID, &id.to_string())?;
            s.set_attr(pml::SlideId::ATTR_R_ID, rid)?;
        }
        if let Some(secs) = sections {
            insert_into_sections(&mut doc, secs, &ids, at, id)?;
        }
        Ok(doc)
    }

    /// `app.xml` with the counts adjusted, if present, readable and
    /// changed (ADR-0012 §7; otherwise untouched, not a refusal).
    fn app_with(&self, counts: Counts) -> Result<Option<(String, XmlDocument)>> {
        let Some(app) = self
            .pkg
            .package_relationships()
            .by_type(REL_EXTENDED_PROPERTIES)
            .filter_map(|r| self.pkg.package_relationships().resolve(r))
            .find(|p| self.pkg.contains(p))
        else {
            return Ok(None);
        };
        let Ok(mut doc) = self.parse_part(&app) else {
            return Ok(None);
        };
        let Ok(mut root) = doc.lens_mut::<pml::Properties>() else {
            return Ok(None);
        };
        let mut changed = false;
        changed |= bump::<pml::Slides>(&mut root, counts.slides)?;
        changed |= bump::<pml::Notes>(&mut root, counts.notes)?;
        changed |= bump::<pml::HiddenSlides>(&mut root, counts.hidden)?;
        drop(root);
        Ok(changed.then_some((app, doc)))
    }

    fn finish(&mut self, plan: RegistryPlan) -> Result<()> {
        self.pkg.apply(plan)?;
        self.rescan()
    }

    /// The id a new shape on the slide at `index` gets (ADR-0012 §3): one
    /// more than every numeric `@id`, `@spid` (also `_x0000_s####`) in the
    /// slide and every `ac:*Mk@id` in its modern comment parts. Unreadable
    /// comment parts refuse (their ids are unknown).
    pub fn next_shape_id(&mut self, index: usize) -> Result<u32> {
        let part = self.slide_part(index)?;
        let rels = self.rels_of(&part)?;
        let mut comments = Vec::new();
        for c in rels
            .by_type(MODERN_COMMENTS_REL)
            .filter_map(|r| rels.resolve(r))
            .filter(|p| self.pkg.contains(p))
        {
            comments.push(self.parse_part(&c)?);
        }
        let roots: Vec<&RawNode> = comments.iter().map(XmlDocument::root).collect();
        next_shape_id(self.tree(&part)?.root(), &roots)
    }

    // ------------------------------------------------------------ add

    /// Adds a slide from `layout` at position `at` (default: the end) and
    /// returns it for editing (ADR-0012 §4.1). Placeholders are cloned
    /// python-pptx's way: `type`/`orient`/`sz`/`idx` only, no position,
    /// no prompt text; `dt`, `ftr`, `sldNum` are skipped.
    pub fn add_slide(&mut self, layout: &LayoutRef, at: Option<usize>) -> Result<SlideMut<'_>> {
        let count = self.slides.len();
        let at = at.unwrap_or(count);
        if at > count {
            return Err(Error::SlideIndexOutOfRange { index: at, count });
        }
        if !self.layouts()?.iter().any(|l| l.part == layout.part) {
            return Err(Error::LayoutNotFound(layout.part.clone()));
        }
        self.check_show_range()?;
        let sections = self.sections()?;
        let id = self.next_slide_id()?;
        let xml = new_slide_xml(self.tree(&layout.part)?.root());
        let slide_doc = XmlDocument::parse(xml.as_bytes())?;
        let main = self.main.clone();
        let mut plan = RegistryPlan::new(&self.pkg);
        let part = plan.add_part(
            &self.pkg,
            PartNameRequest::Numbered("ppt/slides/slide{}.xml".into()),
            CT_SLIDE,
            slide_doc,
        )?;
        plan.add_relationship(
            &self.pkg,
            Some(&part),
            REL_SLIDE_LAYOUT,
            Target::Part(layout.part.clone()),
        )?;
        let rid = plan.add_relationship(
            &self.pkg,
            Some(&main),
            REL_SLIDE,
            Target::Part(part.clone()),
        )?;
        let pres = self.pres_with_inserted(at, id, &rid, &sections)?;
        plan.stage_part(&self.pkg, &main, pres)?;
        if let Some((app, doc)) = self.app_with(Counts {
            slides: 1,
            ..Counts::default()
        })? {
            plan.stage_part(&self.pkg, &app, doc)?;
        }
        self.finish(plan)?;
        self.slide_id_high_water = self.slide_id_high_water.max(id);
        self.slide_mut(at)
    }

    // ------------------------------------------------------------ move

    /// Moves the slide at `from` to position `to` (positions after the
    /// move). Only `p:sldIdLst` and the section lists change (ADR-0012
    /// §4.3, §5).
    pub fn move_slide(&mut self, from: usize, to: usize) -> Result<()> {
        let count = self.slides.len();
        for index in [from, to] {
            if index >= count {
                return Err(Error::SlideIndexOutOfRange { index, count });
            }
        }
        self.check_show_range()?;
        let sections = self.sections()?;
        if from == to {
            return Ok(());
        }
        let ids = self.slide_ids();
        let main = self.main.clone();
        let mut doc = self.tree(&main)?.clone();
        let (id, rid) = {
            let mut root = doc.lens_mut::<pml::Presentation>()?;
            let mut list = root
                .child_mut::<pml::SlideIdList>()
                .ok_or(Error::SlideIndexOutOfRange { index: from, count })?;
            let (item, node) = list
                .node()
                .indexed_elements()
                .filter(|(_, e)| e.is(pml::SlideId::QNAME))
                .nth(from)
                .map(|(i, e)| (i, e.clone()))
                .ok_or(Error::SlideIndexOutOfRange { index: from, count })?;
            plain_slide_id(&node)?;
            let id = node
                .attr(pml::SlideId::ATTR_ID)
                .unwrap_or_default()
                .to_owned();
            let rid = node
                .attr(pml::SlideId::ATTR_R_ID)
                .unwrap_or_default()
                .to_owned();
            list.remove_at(item);
            let mut s = list.insert_by::<pml::SlideId>(|k, _| k == to)?;
            s.set_attr(pml::SlideId::ATTR_ID, &id)?;
            s.set_attr(pml::SlideId::ATTR_R_ID, &rid)?;
            (ids[from], rid)
        };
        let _ = rid;
        if let Some(secs) = &sections {
            remove_from_sections(&mut doc, secs, id)?;
            let rest: Vec<u32> = ids.iter().copied().filter(|&x| x != id).collect();
            let secs_after: Vec<Vec<u32>> = secs
                .iter()
                .map(|s| s.iter().copied().filter(|&x| x != id).collect())
                .collect();
            insert_into_sections(&mut doc, &secs_after, &rest, to, id)?;
        }
        let mut plan = RegistryPlan::new(&self.pkg);
        plan.stage_part(&self.pkg, &main, doc)?;
        self.finish(plan)
    }

    // ------------------------------------------------------------ delete

    /// Deletes the slide at `index` with its notes slide and every part
    /// only it references (ADR-0012 §4.2, §6). Refused when anything else
    /// references the slide.
    pub fn delete_slide(&mut self, index: usize) -> Result<()> {
        let part = self.slide_part(index)?;
        let rid = self.slide_rel_id(index)?;
        let id = self.slides[index].slide_id;
        self.check_show_range()?;
        let sections = self.sections()?;
        self.check_rels_readable()?;
        let main = self.main.clone();
        // custom shows name slides by r:id: any presentation relationship
        // to the slide part, not only the one in p:sldIdLst (review n3)
        let _ = &rid;
        let pres_rels = self.rels_of(&main)?;
        let rids: Vec<String> = pres_rels
            .iter()
            .filter(|r| r.target_mode == TargetMode::Internal && pres_rels.resolve(r).as_deref() == Some(part.as_str()))
            .map(|r| r.id.clone())
            .collect();
        let mut in_show = false;
        walk(self.tree(&main)?.root(), &mut |n| {
            if n.is(q(P_NS, "custShow")) {
                walk(n, &mut |s| {
                    in_show |= s.is(q(P_NS, "sld")) && s.attr(q(R_NS, "id")).is_some_and(|v| rids.iter().any(|r| r == v))
                });
            }
        });
        if in_show {
            return Err(Error::SlideInCustomShow);
        }
        let set = self.cascade(&part)?;
        let main_rels = self.rels_of(&main)?;
        let view_props: Option<String> = main_rels
            .by_type(REL_VIEW_PROPS)
            .filter_map(|r| main_rels.resolve(r))
            .find(|p| self.pkg.contains(p));
        // references by relationship
        for inc in self.pkg.incoming_relationships(&part)? {
            let src = inc.source.clone().unwrap_or_default();
            let ok = set.iter().any(|s| s == &src)
                || (src == main && inc.rel_type == REL_SLIDE)
                || (Some(&src) == view_props.as_ref() && inc.rel_type == REL_SLIDE);
            if !ok {
                return Err(Error::SlideIsReferenced {
                    from_part: src,
                    rel_type: inc.rel_type,
                });
            }
        }
        // references by sldId value (Slide Zoom, unknown numeric sldId attributes)
        let others: Vec<String> = self
            .slides
            .iter()
            .filter_map(|e| e.part.clone())
            .filter(|p| *p != part)
            .collect();
        for p in std::iter::once(main.clone()).chain(others) {
            if let Some(found) = sld_id_reference(self.tree(&p)?.root(), id) {
                let _ = found;
                return Err(Error::SlideIsReferenced {
                    from_part: p,
                    rel_type: "sldId".into(),
                });
            }
        }
        // the plan
        let mut plan = RegistryPlan::new(&self.pkg);
        let mut doc = self.tree(&main)?.clone();
        {
            let mut root = doc.lens_mut::<pml::Presentation>()?;
            let mut list = root
                .child_mut::<pml::SlideIdList>()
                .ok_or(Error::DanglingSlide { slide_id: id })?;
            let item = list
                .node()
                .indexed_elements()
                .filter(|(_, e)| e.is(pml::SlideId::QNAME))
                .nth(index)
                .map(|(i, _)| i)
                .ok_or(Error::DanglingSlide { slide_id: id })?;
            list.remove_at(item);
        }
        if let Some(secs) = &sections {
            remove_from_sections(&mut doc, secs, id)?;
        }
        plan.stage_part(&self.pkg, &main, doc)?;
        if let Some(vp) = &view_props {
            let vrels = self.rels_of(vp)?;
            let vids: Vec<String> = vrels
                .iter()
                .filter(|r| {
                    r.target_mode == TargetMode::Internal
                        && vrels.resolve(r).as_deref() == Some(part.as_str())
                })
                .map(|r| r.id.clone())
                .collect();
            if !vids.is_empty() {
                let mut vdoc = self.tree(vp)?.clone();
                if remove_outline_entries(&mut vdoc, &vids)? {
                    plan.stage_part(&self.pkg, vp, vdoc)?;
                }
            }
        }
        let names: Vec<&str> = set.iter().map(String::as_str).collect();
        let mut allowed: Vec<&str> = CASCADE_TYPES.to_vec();
        allowed.push(REL_SLIDE);
        plan.remove_parts(&self.pkg, &names, &allowed)?;
        let hidden = is_hidden(self.tree(&part)?.root());
        let notes = set
            .iter()
            .any(|p| self.pkg.content_type(p) == Some(CT_NOTES_SLIDE));
        if let Some((app, adoc)) = self.app_with(Counts {
            slides: -1,
            notes: -i64::from(notes),
            hidden: -i64::from(hidden),
        })? {
            plan.stage_part(&self.pkg, &app, adoc)?;
        }
        self.finish(plan)?;
        self.slide_id_high_water = self.slide_id_high_water.max(id);
        Ok(())
    }

    /// The slide and every part it reaches through cascade types that no
    /// part outside the set references (ADR-0012 §6).
    fn cascade(&self, slide: &str) -> Result<Vec<String>> {
        let mut reach: Vec<String> = Vec::new();
        let mut queue = vec![slide.to_owned()];
        while let Some(p) = queue.pop() {
            let rels = self.rels_of(&p)?;
            for r in rels.iter() {
                if r.target_mode == TargetMode::External
                    || KEEP_TYPES.contains(&r.rel_type.as_str())
                {
                    continue;
                }
                if !CASCADE_TYPES.contains(&r.rel_type.as_str()) {
                    return Err(Error::UnsupportedSlidePart(r.rel_type.clone()));
                }
                let Some(t) = rels.resolve(r).filter(|t| self.pkg.contains(t)) else {
                    continue; // resolves to no part: relationship only
                };
                if t != slide && !reach.contains(&t) {
                    reach.push(t.clone());
                    queue.push(t);
                }
            }
        }
        // drop candidates referenced from outside, until stable
        loop {
            let inside = |n: &str, reach: &[String]| n == slide || reach.iter().any(|r| r == n);
            let before = reach.len();
            let snapshot = reach.clone();
            let mut keep = Vec::with_capacity(reach.len());
            for c in &snapshot {
                let incoming = self.pkg.incoming_relationships(c)?;
                keep.push(incoming.iter().all(|inc| inc.source.as_deref().is_some_and(|s| inside(s, &snapshot))));
            }
            let mut k = keep.into_iter();
            reach.retain(|_| k.next().unwrap_or(false));
            if reach.len() == before {
                break;
            }
        }
        Ok(std::iter::once(slide.to_owned()).chain(reach).collect())
    }

    // ------------------------------------------------------------ duplicate

    /// Duplicates the slide at `index` to position `at` (default: right
    /// after it) and returns the copy (ADR-0012 §4.4). The copy keeps every
    /// relationship `Id`; layout and media are shared, notes and tags are
    /// copied. Comments are not copied (they are about the original): their
    /// relationships are skipped and the modern-comment reference
    /// (`p188:commentRel`) is dropped from the copy's `p:extLst`.
    pub fn duplicate_slide(&mut self, index: usize, at: Option<usize>) -> Result<SlideMut<'_>> {
        let part = self.slide_part(index)?;
        let count = self.slides.len();
        let at = at.unwrap_or(index + 1);
        if at > count {
            return Err(Error::SlideIndexOutOfRange { index: at, count });
        }
        self.check_show_range()?;
        let sections = self.sections()?;
        let rels = self.rels_of(&part)?;
        let notes: Option<String> = rels
            .by_type(REL_NOTES_SLIDE)
            .filter_map(|r| rels.resolve(r))
            .find(|p| self.pkg.contains(p));
        let notes_rels = match &notes {
            Some(n) => Some(self.rels_of(n)?),
            None => None,
        };
        for rs in std::iter::once(&rels).chain(notes_rels.as_ref()) {
            for r in rs.iter() {
                let t = r.rel_type.as_str();
                if !DUPLICATE_TYPES.contains(&t) && !COMMENT_TYPES.contains(&t) {
                    return Err(Error::UnsupportedSlidePart(r.rel_type.clone()));
                }
            }
        }
        // tags of the slide and its notes: one copy per distinct target
        let mut tags: Vec<String> = Vec::new();
        for rs in std::iter::once(&rels).chain(notes_rels.as_ref()) {
            for t in rs
                .by_type(REL_TAGS)
                .filter_map(|r| rs.resolve(r))
                .filter(|p| self.pkg.contains(p))
            {
                if !tags.contains(&t) {
                    if !self.rels_of(&t)?.iter().next().is_none() {
                        return Err(Error::UnsupportedSlidePart("tags".into()));
                    }
                    tags.push(t);
                }
            }
        }
        let id = self.next_slide_id()?;
        let mut slide_doc = self.tree(&part)?.clone();
        let hidden = is_hidden(slide_doc.root());
        // the copy refers to none of the skipped comment relationships
        let comment_ids: Vec<String> =
            rels.iter().filter(|r| COMMENT_TYPES.contains(&r.rel_type.as_str())).map(|r| r.id.clone()).collect();
        if !comment_ids.is_empty() {
            drop_comment_rel(&mut slide_doc)?;
            if refers_to_any(slide_doc.root(), &comment_ids) {
                return Err(Error::UnsupportedContent("a reference to the slide's comments outside p:extLst/p:ext/p188:commentRel".into()));
            }
        }
        let mut next_cid = None;
        if has_creation_id(slide_doc.root()) {
            let c = self
                .max_creation_id()?
                .checked_add(1)
                .ok_or(Error::CreationIdsExhausted)?;
            set_creation_id::<pml::Slide>(&mut slide_doc, c)?;
            next_cid = Some(c);
        }
        let main = self.main.clone();
        let mut plan = RegistryPlan::new(&self.pkg);
        let new_slide = plan.add_part(
            &self.pkg,
            PartNameRequest::Numbered("ppt/slides/slide{}.xml".into()),
            CT_SLIDE,
            slide_doc,
        )?;
        let new_notes = match &notes {
            Some(n) => {
                let mut ndoc = self.tree(n)?.clone();
                if has_creation_id(ndoc.root()) {
                    let base = match next_cid {
                        Some(c) => c,
                        None => self.max_creation_id()?,
                    };
                    let c = base.checked_add(1).ok_or(Error::CreationIdsExhausted)?;
                    set_creation_id::<pml::NotesSlide>(&mut ndoc, c)?;
                }
                Some(plan.add_part(
                    &self.pkg,
                    PartNameRequest::Numbered("ppt/notesSlides/notesSlide{}.xml".into()),
                    CT_NOTES_SLIDE,
                    ndoc,
                )?)
            }
            None => None,
        };
        let mut map: Vec<(String, String)> = Vec::new();
        for t in &tags {
            let ct = self
                .pkg
                .content_type(t)
                .unwrap_or("application/vnd.openxmlformats-officedocument.presentationml.tags+xml")
                .to_owned();
            let bytes = self.pkg.read_part(t)?;
            let copy = plan.add_part(
                &self.pkg,
                PartNameRequest::Numbered("ppt/tags/tag{}.xml".into()),
                &ct,
                bytes,
            )?;
            map.push((t.clone(), copy));
        }
        if let (Some(n), Some(nn)) = (&notes, &new_notes) {
            map.push((n.clone(), nn.clone()));
        }
        let pairs: Vec<(&str, &str)> = map.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        plan.copy_relationships(&self.pkg, &part, &new_slide, &pairs, COMMENT_TYPES)?;
        if let (Some(n), Some(nn)) = (&notes, &new_notes) {
            let mut nmap = pairs.clone();
            nmap.retain(|(from, _)| from != n);
            nmap.push((part.as_str(), new_slide.as_str()));
            plan.copy_relationships(&self.pkg, n, nn, &nmap, COMMENT_TYPES)?;
        }
        let rid = plan.add_relationship(
            &self.pkg,
            Some(&main),
            REL_SLIDE,
            Target::Part(new_slide.clone()),
        )?;
        let pres = self.pres_with_inserted(at, id, &rid, &sections)?;
        plan.stage_part(&self.pkg, &main, pres)?;
        let counts = Counts {
            slides: 1,
            notes: i64::from(notes.is_some()),
            hidden: i64::from(hidden),
        };
        if let Some((app, doc)) = self.app_with(counts)? {
            plan.stage_part(&self.pkg, &app, doc)?;
        }
        self.finish(plan)?;
        self.slide_id_high_water = self.slide_id_high_water.max(id);
        self.slide_mut(at)
    }
}

// ---------------------------------------------------------------- helpers

/// A `p:sldId` the move can rebuild: `id` and `r:id` only, no children.
fn plain_slide_id(n: &RawNode) -> Result<()> {
    let extra = n.attrs().iter().any(|a| {
        !(a.name().is(pml::SlideId::ATTR_ID)
            || a.name().is(pml::SlideId::ATTR_R_ID)
            || a.name().ns() == "http://www.w3.org/2000/xmlns/")
    });
    if extra || n.elements().next().is_some() {
        return Err(Error::UnsupportedContent(
            "p:sldId with children or extra attributes".into(),
        ));
    }
    Ok(())
}

/// The first attribute named `sldId` (any namespace) equal to `id`,
/// outside the section list (ADR-0012 §4.2: Slide Zoom and unknown
/// numeric references fail closed).
fn sld_id_reference(root: &RawNode, id: u32) -> Option<String> {
    fn go(n: &RawNode, id: &str, out: &mut Option<String>) {
        if out.is_some() || n.is(QName::new(P14_NS, "sectionLst")) {
            return;
        }
        for a in n.attrs() {
            if a.name().local() == "sldId" && a.value().trim() == id {
                *out = Some(n.name().local().to_owned());
                return;
            }
        }
        for c in n.elements() {
            go(c, id, out);
        }
    }
    let mut out = None;
    go(root, &id.to_string(), &mut out);
    out
}

fn bump<C: ooxml_core::ChildOf<pml::Properties>>(
    root: &mut LensMut<'_, pml::Properties>,
    delta: i64,
) -> Result<bool> {
    if delta == 0 {
        return Ok(false);
    }
    let Some(mut e) = root.child_mut::<C>() else {
        return Ok(false);
    };
    let Ok(old) = e.node().text().trim().parse::<i64>() else {
        return Ok(false);
    };
    let Some(new) = old.checked_add(delta).map(|n| n.max(0)) else {
        return Ok(false);
    };
    if new == old {
        return Ok(false);
    }
    let mut buf = core::fmt::NumBuffer::new();
    e.set_text(new.format_into(&mut buf))?;
    Ok(true)
}

/// The section lens path: presentation → extLst → ext(uri) → sectionLst.
fn with_section_list<T>(
    doc: &mut XmlDocument,
    f: impl FnOnce(&mut LensMut<'_, pml::SectionList>) -> Result<T>,
) -> Result<T> {
    let mut root = doc.lens_mut::<pml::Presentation>()?;
    let mut lst = root
        .child_mut::<pml::PresentationExtensionList>()
        .ok_or(Error::UnsupportedSections)?;
    let mut ext = lst
        .child_mut_by::<pml::PresentationExtension>(|n| {
            n.attr(pml::PresentationExtension::ATTR_URI) == Some(SECTION_LIST_URI)
        })
        .ok_or(Error::UnsupportedSections)?;
    let mut sl = ext
        .child_mut::<pml::SectionList>()
        .ok_or(Error::UnsupportedSections)?;
    f(&mut sl)
}

/// Adds `id` to a section for a slide inserted at position `at` of `ids`
/// (the list before insertion): the predecessor's section, after it; at
/// position 0 the following slide's, first (ADR-0012 §5).
fn insert_into_sections(
    doc: &mut XmlDocument,
    secs: &[Vec<u32>],
    ids: &[u32],
    at: usize,
    id: u32,
) -> Result<()> {
    let (k, pos) = if at > 0 {
        let prev = ids[at - 1];
        let k = secs
            .iter()
            .position(|s| s.contains(&prev))
            .ok_or(Error::SectionsInconsistent)?;
        (
            k,
            secs[k].iter().position(|&x| x == prev).expect("contains") + 1,
        )
    } else if let Some(&next) = ids.first() {
        (
            secs.iter()
                .position(|s| s.contains(&next))
                .ok_or(Error::SectionsInconsistent)?,
            0,
        )
    } else if secs.is_empty() {
        return Ok(()); // no sections: nothing to join
    } else {
        // sections but no slides: the first section, so the sections still
        // concatenate to p:sldIdLst (ADR-0012 §5 post-acceptance note)
        (0, 0)
    };
    with_section_list(doc, |sl| {
        let mut sec = sl
            .nth_child_mut::<pml::Section>(k)
            .ok_or(Error::SectionsInconsistent)?;
        let mut list = sec.get_or_insert::<pml::SectionSlideIdList>()?;
        let mut e = list.insert_by::<pml::SectionSlideIdListEntry>(|i, _| i == pos)?;
        e.set_attr(pml::SectionSlideIdListEntry::ATTR_ID, &id.to_string())?;
        Ok(())
    })
}

/// Removes `id` from its section; the section stays (possibly empty).
fn remove_from_sections(doc: &mut XmlDocument, secs: &[Vec<u32>], id: u32) -> Result<()> {
    let Some(k) = secs.iter().position(|s| s.contains(&id)) else {
        return Ok(());
    };
    let pos = secs[k].iter().position(|&x| x == id).expect("contains");
    with_section_list(doc, |sl| {
        let mut sec = sl
            .nth_child_mut::<pml::Section>(k)
            .ok_or(Error::SectionsInconsistent)?;
        let mut list = sec
            .child_mut::<pml::SectionSlideIdList>()
            .ok_or(Error::SectionsInconsistent)?;
        let item = list
            .node()
            .indexed_elements()
            .filter(|(_, e)| e.is(pml::SectionSlideIdListEntry::QNAME))
            .nth(pos)
            .map(|(i, _)| i)
            .ok_or(Error::SectionsInconsistent)?;
        list.remove_at(item);
        Ok(())
    })
}

/// Removes `p:outlineViewPr/p:sldLst/p:sld` entries naming `rids`; an
/// emptied `p:sldLst` is kept. Returns whether anything changed.
fn remove_outline_entries(doc: &mut XmlDocument, rids: &[String]) -> Result<bool> {
    let mut root = doc.lens_mut::<pml::ViewProperties>()?;
    let Some(mut ov) = root.child_mut::<pml::OutlineViewProperties>() else {
        return Ok(false);
    };
    let Some(mut list) = ov.child_mut::<pml::OutlineViewSlideList>() else {
        return Ok(false);
    };
    let items: Vec<usize> = list
        .node()
        .indexed_elements()
        .filter(|(_, e)| {
            e.is(pml::OutlineViewSlideListEntry::QNAME)
                && e.attr(pml::OutlineViewSlideListEntry::ATTR_R_ID)
                    .is_some_and(|r| rids.iter().any(|x| x == r))
        })
        .map(|(i, _)| i)
        .collect();
    for &i in items.iter().rev() {
        list.remove_at(i);
    }
    Ok(!items.is_empty())
}

/// Removes `p:sld/p:extLst/p:ext` holding `p188:commentRel` (and the
/// `p:extLst` when it becomes empty).
fn drop_comment_rel(doc: &mut XmlDocument) -> Result<()> {
    let comment_rel = QName::new(P188_NS, "commentRel");
    let mut root = doc.lens_mut::<pml::Slide>()?;
    let empty = {
        let Some(mut lst) = root.child_mut::<pml::SlideExtensionList>() else { return Ok(()) };
        let items: Vec<usize> = lst
            .node()
            .indexed_elements()
            .filter(|(_, e)| e.is(pml::SlideExtension::QNAME) && e.elements().any(|c| c.is(comment_rel)))
            .map(|(i, _)| i)
            .collect();
        for &i in items.iter().rev() {
            lst.remove_at(i);
        }
        lst.node().elements().next().is_none()
    };
    let at = root.node().indexed_elements().find(|(_, e)| e.is(pml::SlideExtensionList::QNAME)).map(|(i, _)| i);
    if empty && let Some(i) = at {
        root.remove_at(i);
    }
    Ok(())
}

/// Does any `r:` attribute in the tree carry one of `ids`?
fn refers_to_any(root: &RawNode, ids: &[String]) -> bool {
    let mut found = false;
    walk(root, &mut |n| {
        found |= n.attrs().iter().any(|a| a.name().ns() == R_NS && ids.iter().any(|i| i == a.value().trim()));
    });
    found
}

fn has_creation_id(root: &RawNode) -> bool {
    let mut found = false;
    walk(root, &mut |n| found |= n.is(pml::CreationId::QNAME));
    found
}

/// Sets `p:cSld/p:extLst/p:ext/p14:creationId@val` (where PowerPoint writes
/// it: 1,159 of 1,159 corpus slides). A `creationId` elsewhere is refused
/// rather than left equal to the original's.
fn set_creation_id<R: ElementSpec>(doc: &mut XmlDocument, cid: u32) -> Result<()>
where
    pml::CommonSlideData: ooxml_core::ChildOf<R>,
{
    let unsupported = || Error::UnsupportedContent("p14:creationId outside p:cSld/p:extLst".into());
    let mut root = doc.lens_mut::<R>()?;
    let mut csld = root
        .child_mut::<pml::CommonSlideData>()
        .ok_or_else(unsupported)?;
    let mut lst = csld
        .child_mut::<pml::CommonSlideDataExtensionList>()
        .ok_or_else(unsupported)?;
    let mut ext = lst
        .child_mut_by::<pml::CommonSlideDataExtension>(|n| {
            n.elements().any(|c| c.is(pml::CreationId::QNAME))
        })
        .ok_or_else(unsupported)?;
    let mut c = ext.child_mut::<pml::CreationId>().ok_or_else(unsupported)?;
    c.set_attr(pml::CreationId::ATTR_VAL, &cid.to_string())?;
    Ok(())
}

/// The XML of a new slide for `layout` (ADR-0012 §4.1).
fn new_slide_xml(layout: &RawNode) -> String {
    let tree = layout
        .elements()
        .find(|e| e.is(q(P_NS, "cSld")))
        .and_then(|c| c.elements().find(|e| e.is(q(P_NS, "spTree"))));
    let mut shapes = String::new();
    let mut id = 2u32;
    for sp in tree
        .into_iter()
        .flat_map(|t| t.elements())
        .filter(|e| e.is(q(P_NS, "sp")))
    {
        let Some(ph) = sp
            .elements()
            .find(|e| e.is(q(P_NS, "nvSpPr")))
            .and_then(|n| n.elements().find(|e| e.is(q(P_NS, "nvPr"))))
            .and_then(|n| n.elements().find(|e| e.is(q(P_NS, "ph"))))
        else {
            continue;
        };
        let ty = ph.attr(q("", "type")).unwrap_or("obj");
        if matches!(ty, "dt" | "ftr" | "sldNum") {
            continue;
        }
        let mut attrs = String::new();
        for a in ["type", "orient", "sz", "idx"] {
            if let Some(v) = ph.attr(q("", a)) {
                attrs.push_str(&format!(" {a}=\"{}\"", xml_escape(v)));
            }
        }
        let base = match ty {
            "title" | "ctrTitle" => "Title",
            "subTitle" => "Subtitle",
            "body" => "Text Placeholder",
            "chart" => "Chart Placeholder",
            "tbl" => "Table Placeholder",
            "pic" => "Picture Placeholder",
            "clipArt" => "ClipArt Placeholder",
            "dgm" => "SmartArt Placeholder",
            "media" => "Media Placeholder",
            "sldImg" => "Slide Image Placeholder",
            "hdr" => "Header Placeholder",
            _ => "Content Placeholder",
        };
        let vert = if ph.attr(q("", "orient")) == Some("vert") {
            "Vertical "
        } else {
            ""
        };
        let body = if matches!(ty, "title" | "ctrTitle" | "subTitle" | "body" | "obj") {
            "<p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody>"
        } else {
            ""
        };
        shapes.push_str(&format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{vert}{base} {}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph{attrs}/></p:nvPr></p:nvSpPr><p:spPr/>{body}</p:sp>",
            id - 1
        ));
        id += 1;
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<p:sld xmlns:a=\"{A_NS}\" xmlns:r=\"{R_NS}\" xmlns:p=\"{P_NS}\"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
    )
}

/// The next shape id of a slide (ADR-0012 §3): one more than every
/// numeric unprefixed `@id` in the part, every numeric `@spid` and
/// `_x0000_s####` `@spid`, and every `ac:*Mk@id` of the slide's modern
/// comment parts; refused above `i32::MAX`.
pub(crate) fn next_shape_id(slide: &RawNode, comments: &[&RawNode]) -> Result<u32> {
    let mut max: u64 = 0;
    let num = |v: &str| v.trim().parse::<u64>().ok();
    walk(slide, &mut |n| {
        for a in n.attrs() {
            let name = a.name();
            if name.ns().is_empty()
                && name.local() == "id"
                && let Some(v) = num(a.value())
            {
                max = max.max(v);
            }
            if name.local() == "spid" {
                let v = a.value().trim();
                let v = v.strip_prefix("_x0000_s").unwrap_or(v);
                if let Some(v) = num(v) {
                    max = max.max(v);
                }
            }
        }
    });
    for c in comments {
        walk(c, &mut |n| {
            if n.name().ns() == AC_NS
                && n.name().local().ends_with("Mk")
                && let Some(v) = n.attr(q("", "id")).and_then(num)
            {
                max = max.max(v);
            }
        });
    }
    let next = max + 1;
    if next > u64::from(MAX_SHAPE_ID) {
        return Err(Error::ShapeIdsExhausted);
    }
    Ok(next as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_insert_refuses_same_slot_alternate_content_after_planning() {
        // The public section reader rejects wrapped IDs. Exercise insertion
        // with a planned section and a valid same-slot wrapper in its list.
        let source = format!(
            r#"<p:presentation xmlns:p="{P_NS}" xmlns:p14="{P14_NS}" xmlns:mc="{}">
                <p:extLst><p:ext uri="{SECTION_LIST_URI}"><p14:sectionLst>
                    <p14:section><p14:sldIdLst><p14:sldId id="256"/>
                        <mc:AlternateContent><mc:Choice Requires="p14">
                            <p14:sldId id="257"/>
                        </mc:Choice></mc:AlternateContent>
                    </p14:sldIdLst></p14:section>
                </p14:sectionLst></p:ext></p:extLst>
            </p:presentation>"#,
            ooxml_core::name::MC_NS
        );
        let mut doc = XmlDocument::parse(source.as_bytes()).unwrap();
        std::assert_matches!(
            insert_into_sections(&mut doc, &[vec![256]], &[256], 1, 258),
            Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_)))
        );
        assert_eq!(doc.serialize(), source);
    }
}
