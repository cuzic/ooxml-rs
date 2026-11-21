//! Content control value edits and XML store transactions (ADR-0015 §6).

mod binding;
mod date;
mod wrap;

pub use binding::Binding;
pub use wrap::{ControlLock, ControlSpec, NewControlKind, WrapTarget};

use std::collections::BTreeMap;

use chrono::NaiveDateTime;
use ooxml_core::{
    ChildOf, Detached, ElementSpec, LensMut, QName, RawNode, RegistryPlan, XmlDocument,
};
use ooxml_schema::wml::{
    Checked, Paragraph, Run, RunFonts, RunProperties, SdtContentCheckBox, SdtContentDate,
    SdtContentRun, SdtProperties, ShowingPlaceholder,
};

use crate::document::{node_at, write_pieces};
use crate::fields::Operation;
use crate::names::{SDT, SDT_CONTENT, VAL, w};
use crate::revisions::check_path;
use crate::story::StoryIndex;
use crate::text::{check_text_only, map_text};
use crate::{Document, Error, ParaRef, Result};

const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

const fn w14(name: &'static str) -> QName {
    QName::new(W14, name)
}

/// A control's position in a story's XML order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlRef {
    /// Story containing the control.
    pub story: StoryIndex,
    /// Zero-based position among all controls in the story.
    pub index: usize,
}

/// The signed, document-wide identifier written to `w:sdtPr/w:id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdtId(pub i32);

/// Kind of an existing content control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ControlKind {
    /// Plain text.
    PlainText,
    /// Rich text.
    RichText,
    /// Date picker.
    Date,
    /// Checkbox.
    Checkbox,
    /// Fixed list.
    DropDown,
    /// Fixed list with a free-text option.
    ComboBox,
    /// Any other kind.
    Unsupported,
}

/// Metadata about an existing content control.
#[derive(Clone, Copy, Debug)]
pub struct ContentControl<'a> {
    /// Position for value edits.
    pub location: ControlRef,
    /// Stored identifier, when present.
    pub id: Option<SdtId>,
    /// Control kind.
    pub kind: ControlKind,
    /// Optional tag.
    pub tag: Option<&'a str>,
    properties: &'a RawNode,
}

impl<'a> ContentControl<'a> {
    /// Stored values and display labels for the control's fixed choices.
    pub fn list_items(&self) -> impl Iterator<Item = (&'a str, &'a str)> {
        self.properties
            .elements()
            .filter(|node| node.is(w("dropDownList")) || node.is(w("comboBox")))
            .flat_map(RawNode::elements)
            .filter(|node| node.is(w("listItem")))
            .filter_map(|node| {
                let value = node.attr(w("value"))?;
                Some((value, node.attr(w("displayText")).unwrap_or(value)))
            })
    }
}

fn child(node: &RawNode, name: QName) -> Option<&RawNode> {
    node.elements().find(|n| n.is(name))
}

fn val(node: &RawNode, name: QName) -> Option<&str> {
    child(node, name)?.attr(VAL)
}

fn walk(root: &RawNode) -> impl Iterator<Item = (Vec<usize>, &RawNode)> {
    let mut pending = vec![(Vec::new(), root)];
    std::iter::from_fn(move || {
        let (path, node) = pending.pop()?;
        pending.extend(
            node.indexed_elements()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|(item, node)| ([path.as_slice(), &[item]].concat(), node)),
        );
        Some((path, node))
    })
}

fn controls(root: &RawNode) -> impl Iterator<Item = (Vec<usize>, &RawNode)> {
    walk(root).filter(|(_, node)| node.is(SDT))
}

fn kind(pr: &RawNode) -> ControlKind {
    let mut kinds = pr.elements().filter_map(|n| {
        Some(match (n.name().ns(), n.name().local()) {
            (W14, "checkbox") => ControlKind::Checkbox,
            (ooxml_schema::wml::NS_W, "text") => ControlKind::PlainText,
            (ooxml_schema::wml::NS_W, "date") => ControlKind::Date,
            (ooxml_schema::wml::NS_W, "dropDownList") => ControlKind::DropDown,
            (ooxml_schema::wml::NS_W, "comboBox") => ControlKind::ComboBox,
            (ooxml_schema::wml::NS_W, "richText") => ControlKind::RichText,
            (
                _,
                "picture"
                | "docPartObj"
                | "docPartList"
                | "citation"
                | "bibliography"
                | "equation"
                | "group"
                | "repeatingSection"
                | "repeatingSectionItem"
                | "entityPicker",
            ) => ControlKind::Unsupported,
            _ => return None,
        })
    });
    let first = kinds.next().unwrap_or(ControlKind::RichText);
    if kinds.next().is_some() {
        ControlKind::Unsupported
    } else {
        first
    }
}

#[derive(Clone, Copy)]
enum Value<'a> {
    Text(&'a str),
    Date(NaiveDateTime, Option<&'a str>),
    Checked(bool),
    Selection(&'a str),
    Store(&'a str),
}

enum State {
    None,
    Date(String),
    Checked(bool, String),
}

struct Rendered {
    display: String,
    store: String,
    state: State,
}

fn render(pr: &RawNode, kind: ControlKind, value: Value<'_>) -> Result<Rendered> {
    let text = |display: &str, store: &str| Rendered {
        display: display.into(),
        store: store.into(),
        state: State::None,
    };
    match (kind, value) {
        (
            ControlKind::PlainText | ControlKind::RichText,
            Value::Text(value) | Value::Store(value),
        ) => {
            if kind == ControlKind::PlainText
                && value.contains(['\r', '\n'])
                && !child(pr, w("text"))
                    .and_then(|node| node.attr(w("multiLine")))
                    .is_some_and(is_true)
            {
                return Err(Error::InvalidValue {
                    what: "single-line content control",
                    value: value.into(),
                });
            }
            Ok(text(value, value))
        }
        (
            ControlKind::DropDown | ControlKind::ComboBox,
            Value::Selection(value) | Value::Store(value),
        ) => {
            let local = if kind == ControlKind::DropDown {
                "dropDownList"
            } else {
                "comboBox"
            };
            let list = child(pr, w(local)).ok_or(Error::UnsupportedControl)?;
            if let Some(item) = list
                .elements()
                .find(|n| n.is(w("listItem")) && n.attr(w("value")) == Some(value))
            {
                Ok(text(item.attr(w("displayText")).unwrap_or(value), value))
            } else if kind == ControlKind::ComboBox {
                Ok(text(value, value))
            } else {
                Err(Error::InvalidValue {
                    what: "content control list item",
                    value: value.into(),
                })
            }
        }
        (ControlKind::Date, Value::Date(date, display)) => render_date(pr, date, display),
        (ControlKind::Date, Value::Store(value)) => {
            let date = value
                .strip_suffix('Z')
                .unwrap_or(value)
                .parse()
                .ok()
                .or_else(|| {
                    value
                        .parse::<chrono::NaiveDate>()
                        .ok()?
                        .and_hms_opt(0, 0, 0)
                })
                .ok_or(Error::UnsupportedBinding)?;
            render_date(pr, date, None)
        }
        (ControlKind::Checkbox, Value::Checked(checked)) => render_checkbox(pr, checked),
        (ControlKind::Checkbox, Value::Store(value)) => match value {
            "1" | "true" => render_checkbox(pr, true),
            "0" | "false" => render_checkbox(pr, false),
            _ => Err(Error::UnsupportedBinding),
        },
        _ => Err(Error::UnsupportedControl),
    }
}

fn is_true(value: &str) -> bool {
    matches!(value, "1" | "true" | "on")
}

fn date_picture(pr: &RawNode) -> Option<(&str, &str)> {
    let date = child(pr, w("date"))?;
    Some((
        val(date, w("dateFormat")).unwrap_or("M/d/yyyy"),
        val(date, w("lid")).unwrap_or("en-US"),
    ))
}

fn render_date(pr: &RawNode, date: NaiveDateTime, display: Option<&str>) -> Result<Rendered> {
    let node = child(pr, w("date")).ok_or(Error::UnsupportedControl)?;
    let (picture, language) = date_picture(pr).ok_or(Error::UnsupportedControl)?;
    let display = match display {
        Some(display) => display.into(),
        None => date::format(date, picture, language)?,
    };
    let store = date::mapped(
        date,
        val(node, w("storeMappedDataAs")).unwrap_or("dateTime"),
        &display,
    )?;
    let full_date = date::mapped(date, "dateTime", &display)?;
    Ok(Rendered {
        display,
        store,
        state: State::Date(full_date),
    })
}

fn render_checkbox(pr: &RawNode, checked: bool) -> Result<Rendered> {
    let node = child(pr, w14("checkbox")).ok_or(Error::UnsupportedControl)?;
    let symbol = child(
        node,
        w14(if checked {
            "checkedState"
        } else {
            "uncheckedState"
        }),
    );
    let character = match symbol.and_then(|n| n.attr(w14("val"))) {
        Some(hex) => u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or(Error::UnsupportedControl)?,
        None if checked => '☒',
        None => '☐',
    };
    let font = symbol
        .and_then(|n| n.attr(w14("font")))
        .unwrap_or("MS Gothic");
    Ok(Rendered {
        display: character.into(),
        store: if checked { "true" } else { "false" }.into(),
        state: State::Checked(checked, font.into()),
    })
}

impl Document {
    /// Lists content controls across all stories in XML order.
    pub fn content_controls(&self) -> impl Iterator<Item = ContentControl<'_>> {
        self.stories().flat_map(|story| {
            controls(story.root)
                .enumerate()
                .map(move |(index, (_, node))| {
                    let pr = child(node, w("sdtPr")).unwrap_or(node);
                    ContentControl {
                        location: ControlRef {
                            story: story.index,
                            index,
                        },
                        id: val(pr, w("id")).and_then(|id| id.parse().ok()).map(SdtId),
                        kind: kind(pr),
                        tag: val(pr, w("tag")),
                        properties: pr,
                    }
                })
        })
    }

    /// Writes plain text or single-paragraph rich text into a control.
    pub fn set_text(&mut self, control: ControlRef, text: &str) -> Result<()> {
        self.edit_control(control, Value::Text(text))
    }

    /// Sets a date and formats its displayed value using the Word picture.
    pub fn set_date(&mut self, control: ControlRef, date: NaiveDateTime) -> Result<()> {
        self.edit_control(control, Value::Date(date, None))
    }

    /// Sets a date with caller-supplied display text.
    pub fn set_date_with_display(
        &mut self,
        control: ControlRef,
        date: NaiveDateTime,
        display: &str,
    ) -> Result<()> {
        self.edit_control(control, Value::Date(date, Some(display)))
    }

    /// Sets a checkbox and refreshes its glyph.
    pub fn set_checked(&mut self, control: ControlRef, checked: bool) -> Result<()> {
        self.edit_control(control, Value::Checked(checked))
    }

    /// Selects a list value or supplies free text to a combo box.
    pub fn select(&mut self, control: ControlRef, value: &str) -> Result<()> {
        self.edit_control(control, Value::Selection(value))
    }

    fn edit_control(&mut self, control: ControlRef, value: Value<'_>) -> Result<()> {
        self.content_control_gate(true)?;
        let story = self
            .story_at(control.story)
            .ok_or(Error::UnsupportedControl)?;
        let (path, node) = controls(story.root)
            .nth(control.index)
            .ok_or(Error::UnsupportedControl)?;
        let pr = editable_properties(node)?;
        let primary_properties = pr;
        let target = binding::read(pr)?
            .map(|binding| binding.target())
            .transpose()?;
        let primary = render(pr, kind(pr), value)?;
        let mut edits = BTreeMap::from([(control.story, vec![(path.clone(), primary)])]);
        let mut plan = RegistryPlan::new(&self.pkg);
        if let Some(target) = target {
            let (part, mut xml) = target.store(self)?;
            let store = edits[&control.story][0].1.store.clone();
            target.write(self, &mut xml, &store)?;
            for peer_story in self.stories() {
                for (peer_path, peer) in controls(peer_story.root) {
                    if peer_story.index == control.story && peer_path == path {
                        continue;
                    }
                    let Some(pr) = child(peer, w("sdtPr")) else {
                        continue;
                    };
                    if binding::matches_target(pr, &target)? {
                        editable_properties(peer)?;
                        let peer_kind = kind(pr);
                        let peer_value = match (value, peer_kind) {
                            (Value::Date(date, display), ControlKind::Date) => {
                                let same_picture =
                                    date_picture(primary_properties) == date_picture(pr);
                                Value::Date(date, display.filter(|_| same_picture))
                            }
                            _ => Value::Store(&store),
                        };
                        let rendered = render(pr, peer_kind, peer_value)?;
                        edits
                            .entry(peer_story.index)
                            .or_default()
                            .push((peer_path, rendered));
                    }
                }
            }
            plan.stage_part(&self.pkg, &part, xml)?;
        }
        for (story, changes) in &mut edits {
            let part = &self.stories[story.0].id.part;
            let mut xml = self.pkg.parsed_part(part).expect("parsed story").clone();
            changes.sort_by(|a, b| b.0.cmp(&a.0));
            for (path, rendered) in changes {
                self.check_control_runs(*story, path)?;
                apply(&mut xml, path, rendered)?;
            }
            plan.stage_part(&self.pkg, part, xml)?;
        }
        self.pkg.apply(plan)?;
        for story in edits.into_keys() {
            self.refresh_control_story(story);
        }
        Ok(())
    }

    fn check_control_runs(&self, story: StoryIndex, path: &[usize]) -> Result<()> {
        let story = self.story_at(story).ok_or(Error::UnsupportedControl)?;
        check_path(story.root, path)?;
        for depth in 0..path.len() {
            let ancestor = node_at(story.root, &path[..depth]);
            if ancestor.is(w("fldSimple")) {
                return Err(Error::InsideField);
            }
            if ancestor.is(QName::new(ooxml_core::name::MC_NS, "AlternateContent"))
                || ancestor.is(w("txbxContent"))
            {
                return Err(Error::UnsupportedControl);
            }
            if ancestor.is(SDT) {
                editable_properties(ancestor)?;
            }
        }
        for paragraph in story.paragraphs() {
            let paragraph_path = story.data.index.path(paragraph.info.frame);
            if paragraph_path.starts_with(path) {
                if paragraph.in_separator_note() {
                    return Err(Error::UnsupportedControl);
                }
                if paragraph.info.in_revision_range {
                    return Err(Error::InsideRevision {
                        context: crate::RunContext::Direct,
                    });
                }
                if paragraph.info.starts_in_field {
                    return Err(Error::InsideField);
                }
            }
            for run in paragraph.runs() {
                if story.data.index.path(run.info.frame).starts_with(path) {
                    if paragraph.in_separator_note() {
                        return Err(Error::UnsupportedControl);
                    }
                    if run.in_field() || run.wrappers().contains(&crate::body::Wrapper::SimpleField)
                    {
                        return Err(Error::InsideField);
                    }
                    crate::revisions::check_run_revision(&run)?;
                }
            }
        }
        Ok(())
    }

    fn refresh_control_story(&mut self, story: StoryIndex) {
        self.reindex(story);
        for index in 0..self.generations[story.0].len() {
            self.bump(ParaRef::new(story, index));
        }
    }

    fn content_control_gate(&self, value: bool) -> Result<()> {
        self.control_gate_for(Operation::Text, value)
    }
}

fn editable_properties(node: &RawNode) -> Result<&RawNode> {
    let pr = child(node, w("sdtPr")).unwrap_or(node);
    if pr
        .elements()
        .filter(|n| n.is(w("lock")))
        .any(|n| matches!(n.attr(VAL), Some("contentLocked" | "sdtContentLocked")))
    {
        return Err(Error::ControlLocked);
    }
    Ok(pr)
}

fn apply(xml: &mut XmlDocument, path: &[usize], value: &Rendered) -> Result<()> {
    let node = node_at(xml.root(), path);
    let properties = node.indexed_elements().find(|(_, pr)| pr.is(w("sdtPr")));
    let (content_index, content) = node
        .indexed_elements()
        .find(|(_, n)| n.is(SDT_CONTENT))
        .ok_or(Error::UnsupportedControl)?;
    let content_path = [path, &[content_index]].concat();
    let pr_path = properties.map(|(index, _)| [path, &[index]].concat());
    let placeholder = properties
        .and_then(|(_, pr)| child(pr, w("showingPlcHdr")))
        .is_some_and(|node| node.attr(VAL).is_none_or(is_true));
    let (container, container_path) = run_container(content, content_path)?;
    let block = container.is(w("p"));
    if !block
        && container.elements().next().is_none()
        && !(0..path.len()).any(|depth| node_at(xml.root(), &path[..depth]).is(w("p")))
    {
        return Err(Error::UnsupportedControl);
    }
    let rich = properties.is_none_or(|(_, pr)| kind(pr) == ControlKind::RichText);
    let runs = cache_runs(container, &container_path, rich)?;
    let placeholder_rpr = properties
        .filter(|_| placeholder)
        .and_then(|(pr_index, pr)| {
            pr.indexed_elements()
                .find(|(_, node)| node.is(w("rPr")))
                .map(|(index, _)| [path, &[pr_index, index]].concat())
        });
    let drop_style = placeholder && placeholder_rpr.is_none();
    let properties = placeholder_rpr
        .map(|path| {
            check_path(xml.root(), &path)?;
            if child(node_at(xml.root(), &path), w("rPrChange")).is_some() {
                return Err(Error::InsideRevision {
                    context: crate::RunContext::Direct,
                });
            }
            Ok(xml
                .lens_at::<RunProperties>(&path)?
                .detach_as::<RunProperties>(&[])?)
        })
        .transpose()?;
    let pieces = map_text(&value.display)?;
    let structured = runs.iter().any(|run| run.len() != container_path.len() + 1)
        || (rich && container.elements().any(is_passive_marker));
    if structured {
        if let Some((first, rest)) = runs.split_first() {
            let mut run = xml.lens_mut_at::<Run>(first)?;
            write_cache_run(
                &mut run,
                &pieces,
                properties.as_ref(),
                drop_style,
                &value.state,
            )?;
            for path in rest {
                write_pieces(&mut xml.lens_mut_at::<Run>(path)?, &[])?;
            }
        } else {
            return Err(Error::UnsupportedControl);
        }
    } else {
        let items: Vec<_> = runs
            .iter()
            .map(|path| *path.last().expect("run path"))
            .collect();
        let mut run = if block {
            replace_runs(xml.lens_mut_at::<Paragraph>(&container_path)?, &items)?
        } else {
            replace_runs(xml.lens_mut_at::<SdtContentRun>(&container_path)?, &items)?
        };
        write_cache_run(
            &mut run,
            &pieces,
            properties.as_ref(),
            drop_style,
            &value.state,
        )?;
    }
    let Some(pr_path) = pr_path else {
        return Ok(());
    };
    let mut pr = xml.lens_mut_at::<SdtProperties>(&pr_path)?;
    if placeholder {
        pr.remove::<ShowingPlaceholder>();
    }
    match &value.state {
        State::None => {}
        State::Date(date) => pr
            .child_mut::<SdtContentDate>()
            .ok_or(Error::UnsupportedControl)?
            .set_attr(w("fullDate"), date)?,
        State::Checked(checked, _) => pr
            .child_mut::<SdtContentCheckBox>()
            .ok_or(Error::UnsupportedControl)?
            .get_or_insert::<Checked>()?
            .set_attr(w14("val"), if *checked { "1" } else { "0" })?,
    }
    Ok(())
}

fn is_passive_marker(node: &RawNode) -> bool {
    node.name().ns() == ooxml_schema::wml::NS_W
        && matches!(
            node.name().local(),
            "bookmarkStart" | "bookmarkEnd" | "commentRangeStart" | "commentRangeEnd" | "proofErr"
        )
}

fn cache_runs(container: &RawNode, path: &[usize], rich: bool) -> Result<Vec<Vec<usize>>> {
    let mut runs = Vec::new();
    let mut parents = vec![(container, path.to_vec(), container.indexed_elements())];
    while let Some((parent, path, children)) = parents.last_mut() {
        let Some((index, node)) = children.next() else {
            parents.pop();
            continue;
        };
        if (parent.is(w("p")) && node.is(w("pPr"))) || (rich && is_passive_marker(node)) {
            continue;
        }
        let path = [path.as_slice(), &[index]].concat();
        if node.is(w("r")) {
            check_text_only(node)?;
            runs.push(path);
        } else if rich && node.is(w("hyperlink")) {
            parents.push((node, path, node.indexed_elements()));
        } else {
            return Err(Error::UnsupportedControl);
        }
    }
    Ok(runs)
}

fn replace_runs<'a, P: ElementSpec>(
    mut container: LensMut<'a, P>,
    runs: &[usize],
) -> Result<LensMut<'a, Run>>
where
    Run: ChildOf<P>,
{
    let index = if let Some((&first, rest)) = runs.split_first() {
        for &index in rest.iter().rev() {
            container.remove_at(index);
        }
        first
    } else {
        let position = container.node().children().len();
        container.insert_at_with_ns::<Run>(
            position,
            &[ooxml_core::NsDecl::new("w", ooxml_schema::wml::NS_W)],
        )?;
        position
    };
    Ok(container
        .into_child_at::<Run>(index)
        .expect("run at selected position"))
}

fn write_cache_run(
    run: &mut LensMut<'_, Run>,
    pieces: &[crate::text::Piece],
    properties: Option<&Detached<RunProperties>>,
    drop_style: bool,
    state: &State,
) -> Result<()> {
    if let Some(properties) = properties {
        while run.remove::<RunProperties>() {}
        run.insert_detached::<RunProperties>(properties)?;
    } else if drop_style && let Some(mut rpr) = run.child_mut::<RunProperties>() {
        while rpr.remove::<ooxml_schema::wml::RunStyle>() {}
    }
    if let State::Checked(_, font) = state {
        let mut properties = run.get_or_insert::<RunProperties>()?;
        let mut fonts = properties.get_or_insert::<RunFonts>()?;
        for attribute in ["ascii", "hAnsi", "eastAsia", "cs"] {
            fonts.set_attr(w(attribute), font)?;
        }
    }
    write_pieces(run, pieces)
}

// A row/cell control may expose a single paragraph without replacing its
// structural containers. Preserve their properties and refuse ambiguous shapes.
fn run_container(mut node: &RawNode, mut path: Vec<usize>) -> Result<(&RawNode, Vec<usize>)> {
    for _ in 0..3 {
        let properties = match node.name().local() {
            "tr" => Some("trPr"),
            "tc" => Some("tcPr"),
            _ => None,
        };
        let mut content = node
            .indexed_elements()
            .filter(|(_, child)| !properties.is_some_and(|name| child.is(w(name))));
        let Some((index, first)) = content.next() else {
            return if node.is(SDT_CONTENT) {
                Ok((node, path))
            } else {
                Err(Error::UnsupportedControl)
            };
        };
        if node.is(SDT_CONTENT)
            && ![w("p"), w("tr"), w("tc")]
                .into_iter()
                .any(|name| first.is(name))
        {
            return Ok((node, path));
        }
        if content.next().is_some() {
            return Err(Error::UnsupportedControl);
        }
        path.push(index);
        if first.is(w("p")) {
            return Ok((first, path));
        }
        if !first.is(w("tr")) && !first.is(w("tc")) {
            return Err(Error::UnsupportedControl);
        }
        node = first;
    }
    Err(Error::UnsupportedControl)
}
