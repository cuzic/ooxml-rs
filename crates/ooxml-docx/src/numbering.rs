//! List definitions and paragraph numbering (ADR-0011 §5a).

use std::collections::{BTreeSet, hash_map::RandomState};
use std::fmt::NumBuffer;
use std::hash::BuildHasher;

use ooxml_core::package::RegistryRefusal;
use ooxml_core::{ElementSpec, LensMut, NsDecl, PartNameRequest, RawNode, RegistryPlan, Target, XmlDocument};
use ooxml_schema::wml::{
    AbstractNum, AbstractNumId, Indentation, Level, LevelJustification, LevelOverride, LevelText, MultiLevelType, NS_R, NS_W, Nsid,
    Numbering, NumberingFormat, NumberingId, NumberingInstance, NumberingLevelReference, NumberingProperties, Paragraph,
    ParagraphProperties, PreviousParagraphProperties, StartNumberingValue, StartOverrideNumberingValue,
};

use crate::format::wml_prefix_in_scope;
use crate::names::{VAL, descendants, w};
use crate::{Document, Error, Operation, ParaRef, Result};

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
const CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const W: &[NsDecl<'_>] = &[NsDecl { prefix: "w", uri: NS_W }];

/// A numbering instance id. Zero denotes suppressed numbering in Word.
pub type NumId = u32;

/// A nine-level list template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListTemplate {
    /// Bullets with alternating markers.
    Bullet,
    /// Decimal numbering at each level.
    Decimal,
}

fn direct<'a>(node: &'a RawNode, name: &'static str) -> Option<&'a RawNode> {
    node.elements().find(|child| child.is(w(name)))
}

fn numeric(value: Option<&str>) -> Option<u32> {
    value?.trim().parse().ok()
}

fn registry_id(value: Option<&str>) -> Option<u32> {
    Some(u32::try_from(value?.trim().parse::<u64>().ok()?).unwrap_or(u32::MAX))
}

fn new_id(max: u32) -> Result<u32> {
    max.checked_add(1)
        .filter(|id| *id <= i32::MAX as u32)
        .ok_or(Error::NumberingIdsExhausted)
}

#[derive(Default)]
struct UsedNumbering {
    num: u32,
    abstract_num: Option<u32>,
    nsids: BTreeSet<u32>,
}

impl UsedNumbering {
    fn scan(&mut self, root: &RawNode) {
        for node in descendants(root).filter(|n| n.name().ns() == NS_W) {
            let num = match node.name().local() {
                "num" => registry_id(node.attr(w("numId"))),
                "numId" | "numIdMacAtCleanup" => registry_id(node.attr(VAL)),
                "abstractNum" | "abstractNumId" => {
                    let id = registry_id(node.attr(if node.is(w("abstractNum")) { w("abstractNumId") } else { VAL }));
                    self.abstract_num = self.abstract_num.max(id);
                    None
                }
                "nsid" => {
                    self.nsids
                        .extend(node.attr(VAL).and_then(|v| u32::from_str_radix(v.trim(), 16).ok()));
                    None
                }
                _ => None,
            };
            self.num = self.num.max(num.unwrap_or_default());
        }
    }

    fn nsid(&self) -> u32 {
        let seed = RandomState::new();
        (0..)
            .map(|i| seed.hash_one(i) as u32)
            .find(|id| !self.nsids.contains(id))
            .expect("available nsid")
    }
}

impl Document {
    fn numbering_part(&self) -> Result<Option<String>> {
        let relationships = self.pkg.relationships(self.main_part_name())?;
        let mut targets = relationships.by_type(REL);
        let Some(rel) = targets.next() else { return Ok(None) };
        if targets.next().is_some() {
            return Err(Error::InvalidNumbering);
        }
        let name = relationships
            .resolve(rel)
            .filter(|name| self.pkg.contains(name))
            .ok_or(Error::InvalidNumbering)?;
        Ok(Some(name))
    }

    fn used_numbering(&mut self, part: Option<&str>) -> Result<UsedNumbering> {
        let mut parts = self.registry_parts(&["numbering", "styles", "stylesWithEffects"])?;
        if let Some(part) = part
            && !parts.iter().any(|name| name == part)
        {
            parts.push(part.into());
        }
        let mut used = UsedNumbering::default();
        for part in parts {
            used.scan(self.pkg.xml_part(&part)?.root());
        }
        Ok(used)
    }

    /// Add a list definition and an instance. Creates a numbering part when needed.
    pub fn create_list(&mut self, template: ListTemplate) -> Result<NumId> {
        self.write_gate_for(Operation::Structure)?;
        let part = self.numbering_part()?;
        let used = self.used_numbering(part.as_deref())?;
        let num = new_id(used.num)?;
        let abstract_num = used.abstract_num.map_or(Ok(0), new_id)?;
        let mut xml = match &part {
            Some(part) => self.pkg.xml_part(part)?.clone(),
            None => XmlDocument::parse_with(format!(r#"<w:numbering xmlns:w="{NS_W}"/>"#).as_bytes(), &self.pkg.parse_options())?,
        };
        let mut numbering = xml.lens_mut::<Numbering>()?;
        let mut definition = numbering.insert_with_ns::<AbstractNum>(W)?;
        definition.set_attr(w("abstractNumId"), abstract_num.format_into(&mut NumBuffer::new()))?;
        definition.insert::<Nsid>()?.set_attr(VAL, &format!("{:08X}", used.nsid()))?;
        definition.insert::<MultiLevelType>()?.set_attr(VAL, "hybridMultilevel")?;
        for level in 0_usize..9 {
            let mut lvl = definition.insert::<Level>()?;
            lvl.set_attr(w("ilvl"), level.format_into(&mut NumBuffer::new()))?;
            lvl.insert::<StartNumberingValue>()?.set_attr(VAL, "1")?;
            let (format, text) = match template {
                ListTemplate::Bullet => ("bullet", ["•", "○", "▪"][level % 3].into()),
                ListTemplate::Decimal => ("decimal", format!("%{}.", level + 1)),
            };
            lvl.insert::<NumberingFormat>()?.set_attr(VAL, format)?;
            lvl.insert::<LevelText>()?.set_attr(VAL, &text)?;
            lvl.insert::<LevelJustification>()?.set_attr(VAL, "left")?;
            let mut ppr = lvl.insert::<PreviousParagraphProperties>()?;
            let mut indent = ppr.insert::<Indentation>()?;
            indent.set_attr(w("left"), ((level + 1) * 720).format_into(&mut NumBuffer::new()))?;
            indent.set_attr(w("hanging"), "360")?;
        }
        append_num(&mut numbering, num, abstract_num)?;

        let mut plan = RegistryPlan::new(&self.pkg);
        if let Some(part) = part {
            plan.stage_part(&self.pkg, &part, xml)?;
        } else {
            let name = match plan.add_part(
                &self.pkg,
                PartNameRequest::Exact("word/numbering.xml".into()),
                CONTENT_TYPE,
                xml.clone(),
            ) {
                Err(ooxml_core::Error::RegistryRefused {
                    reason: RegistryRefusal::NameInUse { .. },
                    ..
                }) => plan.add_part(
                    &self.pkg,
                    PartNameRequest::Numbered("word/numbering{}.xml".into()),
                    CONTENT_TYPE,
                    xml,
                )?,
                result => result?,
            };
            let main = self.main_part_name();
            let id = plan.add_relationship(&self.pkg, Some(main), REL, Target::Part(name))?;
            if descendants(self.story_root_node(crate::StoryIndex::MAIN)).any(|node| {
                node.attrs()
                    .iter()
                    .any(|a| a.name().ns() == NS_R && a.value().eq_ignore_ascii_case(&id))
            }) {
                return Err(Error::RelIdReferenced { id });
            }
        }
        self.pkg.apply(plan)?;
        Ok(num)
    }

    /// Apply an existing list at a zero-based level (0 through 8).
    pub fn apply_list(&mut self, para: ParaRef, num: NumId, level: u8) -> Result<()> {
        self.write_gate_for(Operation::Structure)?;
        check_level(level)?;
        let part = self.numbering_part()?.ok_or(Error::UnknownNumId { id: num })?;
        find_num(self.pkg.xml_part(&part)?.root(), num)?;
        self.check_list_target(para)?;
        let props = self.paragraph_at(para).and_then(|p| direct_num(p.raw()));
        let (mut num_buf, mut level_buf) = (NumBuffer::new(), NumBuffer::new());
        let num_value = num.format_into(&mut num_buf);
        let level_value = level.format_into(&mut level_buf);
        if props.is_some_and(|props| {
            direct(props, "numId").and_then(|n| n.attr(VAL)) == Some(num_value)
                && direct(props, "ilvl").and_then(|n| n.attr(VAL)) == Some(level_value)
        }) {
            return Ok(());
        }
        self.check_num_write(para, Some(level))?;
        self.write_num(para, num, Some(level))?;
        self.reindex(para.story());
        self.bump(para);
        Ok(())
    }

    /// Create a fresh instance starting at `start` and reassign this
    /// paragraph and later paragraphs in its story with the same direct id.
    /// Numbering inherited solely from styles keeps its original instance.
    pub fn restart_list(&mut self, para: ParaRef, start: u32) -> Result<NumId> {
        self.write_gate_for(Operation::Structure)?;
        self.check_list_target(para)?;
        if start > i32::MAX as u32 {
            return Err(Error::InvalidValue {
                what: "list start",
                value: start.to_string(),
            });
        }
        let props = direct_num(self.paragraph_at(para).expect("checked paragraph").raw()).ok_or(Error::InvalidNumbering)?;
        let id = num_id(props).filter(|id| *id != 0).ok_or(Error::InvalidNumbering)?;
        let level = direct(props, "ilvl")
            .map_or(Some(0), |n| n.attr(VAL)?.trim().parse::<u8>().ok())
            .ok_or(Error::InvalidNumbering)?;
        check_level(level)?;
        let part = self.numbering_part()?.ok_or(Error::UnknownNumId { id })?;
        let new_num = new_id(self.used_numbering(Some(&part))?.num)?;
        let mut xml = self.pkg.xml_part(&part)?.clone();
        let old = find_num(xml.root(), id)?;
        let abstract_id = numeric(direct(old, "abstractNumId").and_then(|n| n.attr(VAL))).ok_or(Error::InvalidNumbering)?;
        let root = xml.lens::<Numbering>()?;
        let old = root
            .children::<NumberingInstance>()
            .find(|n| numeric(n.attr(w("numId"))) == Some(id))
            .expect("checked");
        let overrides = old
            .children::<LevelOverride>()
            .filter(|n| numeric(n.attr(w("ilvl"))) != Some(u32::from(level)))
            .map(|n| n.detach_as::<LevelOverride>(&[]))
            .collect::<ooxml_core::Result<Vec<_>>>()?;
        let mut numbering = xml.lens_mut::<Numbering>()?;
        let mut instance = append_num(&mut numbering, new_num, abstract_id)?;
        for old in overrides {
            instance.insert_detached(&old)?;
        }
        let mut restart = instance.insert::<LevelOverride>()?;
        restart.set_attr(w("ilvl"), level.format_into(&mut NumBuffer::new()))?;
        restart
            .insert::<StartOverrideNumberingValue>()?
            .set_attr(VAL, start.format_into(&mut NumBuffer::new()))?;

        let affected: Vec<_> = self
            .story_at(para.story())
            .expect("checked story")
            .paragraphs()
            .skip(para.index())
            .filter(|p| direct_num(p.raw()).and_then(num_id) == Some(id))
            .map(|p| p.para_ref())
            .collect();
        for &p in &affected {
            self.check_list_target(p)?;
            self.check_num_write(p, None)?;
        }
        let mut plan = RegistryPlan::new(&self.pkg);
        plan.stage_part(&self.pkg, &part, xml)?;
        self.pkg.apply(plan)?;
        for p in affected {
            self.write_num(p, new_num, None)?;
            self.bump(p);
        }
        Ok(new_num)
    }

    /// Remove direct numbering, suppressing any inherited style numbering.
    pub fn remove_from_list(&mut self, para: ParaRef) -> Result<()> {
        self.write_gate_for(Operation::Structure)?;
        self.check_list_target(para)?;
        let p = self.paragraph_at(para).expect("checked paragraph");
        let ppr = direct(p.raw(), "pPr");
        let style = ppr.and_then(|n| direct(n, "pStyle")).and_then(|n| n.attr(VAL));
        let suppress = self.inherited_numbering(style)?.is_some_and(|id| id != 0);
        let current = direct_num(p.raw());
        if suppress {
            if current.is_some_and(|props| num_id(props) == Some(0) && direct(props, "ilvl").is_none()) {
                return Ok(());
            }
            self.check_num_write(para, None)?;
            self.with_para(para, |p| {
                let mut ppr = p.get_or_insert_with_ns::<ParagraphProperties>(W)?;
                let mut props = ppr.get_or_insert_with_ns::<NumberingProperties>(W)?;
                props.remove::<NumberingLevelReference>();
                props.get_or_insert_with_ns::<NumberingId>(W)?.set_attr(VAL, "0")?;
                Ok(())
            })?;
            self.reindex(para.story());
        } else if current.is_some() {
            self.with_para(para, |p| {
                p.child_mut::<ParagraphProperties>()
                    .expect("existing pPr")
                    .remove::<NumberingProperties>();
                Ok(())
            })?;
        } else {
            return Ok(());
        }
        self.bump(para);
        Ok(())
    }

    fn check_list_target(&self, para: ParaRef) -> Result<()> {
        self.para_location(para)?;
        self.check_structure_neighbour(para)?;
        let p = self.paragraph_at(para).expect("checked paragraph");
        let Some(ppr) = list_child(p.raw(), ParagraphProperties::QNAME)? else {
            return Ok(());
        };
        let Some(num) = list_child(ppr, NumberingProperties::QNAME)? else {
            return Ok(());
        };
        list_child(num, NumberingId::QNAME)?;
        list_child(num, NumberingLevelReference::QNAME)?;
        if num.elements().any(|n| n.is(w("ins")) || n.is(w("numberingChange"))) {
            return Err(Error::TrackedPropertyChange { element: "w:numPr" });
        }
        Ok(())
    }

    fn check_num_write(&mut self, para: ParaRef, level: Option<u8>) -> Result<()> {
        let limit = self.pkg.parse_options().max_depth;
        let (parent, item) = self.para_location(para)?;
        let mut path = parent;
        path.push(item);
        let root = self.story_root_node(para.story());
        let prefix = |path: &[usize]| wml_prefix_in_scope(root, path);
        let p = self.paragraph_at(para).expect("checked paragraph");
        let ppr = direct(p.raw(), "pPr");
        let props = ppr.and_then(|n| direct(n, "numPr"));
        for name in ["ilvl", "numId"] {
            if let Some(props) = props
                && (name == "numId" || level.is_some())
                && let Some((index, node)) = props.indexed_elements().find(|(_, n)| n.is(w(name)))
                && node.attr(VAL).is_none()
            {
                let mut attr_path = path.clone();
                for (n, child) in [(p.raw(), "pPr"), (ppr.expect("existing pPr"), "numPr")] {
                    attr_path.push(n.indexed_elements().find(|(_, e)| e.is(w(child))).expect("existing property").0);
                }
                attr_path.push(index);
                if !prefix(&attr_path) {
                    return Err(ooxml_core::Error::NamespaceNotInScope(NS_W.into()).into());
                }
            }
        }
        self.with_para(para, |p| {
            if let Some(mut ppr) = p.child_mut::<ParagraphProperties>() {
                if let Some(props) = ppr.child_mut::<NumberingProperties>() {
                    props.check_get_or_insert_with_ns::<NumberingId>(W)?;
                    if level.is_some() {
                        props.check_get_or_insert_with_ns::<NumberingLevelReference>(W)?;
                    }
                } else {
                    ppr.check_insert_with_ns::<NumberingProperties>(W)?;
                }
            } else {
                p.check_insert_with_ns::<ParagraphProperties>(W)?;
            }
            if p.path().len() + 4 > limit {
                return Err(ooxml_core::Error::TooDeep { limit, position: 0 }.into());
            }
            Ok(())
        })
    }

    fn write_num(&mut self, para: ParaRef, num: NumId, level: Option<u8>) -> Result<()> {
        self.with_para(para, |p: &mut LensMut<'_, Paragraph>| {
            let mut ppr = p.get_or_insert_with_ns::<ParagraphProperties>(W)?;
            let mut props = ppr.get_or_insert_with_ns::<NumberingProperties>(W)?;
            if let Some(level) = level {
                props
                    .get_or_insert_with_ns::<NumberingLevelReference>(W)?
                    .set_attr(VAL, level.format_into(&mut NumBuffer::new()))?;
            }
            props
                .get_or_insert_with_ns::<NumberingId>(W)?
                .set_attr(VAL, num.format_into(&mut NumBuffer::new()))?;
            Ok(())
        })
    }
}

fn list_child(parent: &RawNode, q: ooxml_core::QName) -> Result<Option<&RawNode>> {
    let mut children = parent.elements().filter(|n| n.is(q));
    let first = children.next();
    if children.next().is_some() {
        return Err(Error::InvalidNumbering);
    }
    Ok(first)
}

fn direct_num(para: &RawNode) -> Option<&RawNode> {
    direct(direct(para, "pPr")?, "numPr")
}

fn num_id(props: &RawNode) -> Option<NumId> {
    numeric(direct(props, "numId")?.attr(VAL))
}

fn check_level(level: u8) -> Result<()> {
    if level > 8 {
        return Err(Error::InvalidValue {
            what: "list level",
            value: level.to_string(),
        });
    }
    Ok(())
}

fn find_num(root: &RawNode, id: NumId) -> Result<&RawNode> {
    if !root.is(w("numbering")) {
        return Err(Error::InvalidNumbering);
    }
    let mut nodes = root
        .elements()
        .filter(|n| n.is(w("num")) && numeric(n.attr(w("numId"))) == Some(id));
    let node = nodes.next().filter(|_| id != 0).ok_or(Error::UnknownNumId { id })?;
    if nodes.next().is_some() {
        return Err(Error::InvalidNumbering);
    }
    Ok(node)
}

fn append_num<'a>(numbering: &'a mut LensMut<'_, Numbering>, id: NumId, abstract_id: u32) -> Result<LensMut<'a, NumberingInstance>> {
    let mut num = numbering.insert_with_ns::<NumberingInstance>(W)?;
    num.set_attr(w("numId"), id.format_into(&mut NumBuffer::new()))?;
    num.insert::<AbstractNumId>()?
        .set_attr(VAL, abstract_id.format_into(&mut NumBuffer::new()))?;
    Ok(num)
}
