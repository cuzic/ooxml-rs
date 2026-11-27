//! Dynamic-array metadata and the Phase1 cell transaction (ADR-0017 §9).

use ooxml_core::{
    AttrSpec, ChildOf, Content, ElementSpec, NsDecl, Package, PartNameRequest, QName, RawNode,
    RegistryPlan, RegistryRefusal, SimpleType, Target, XmlDocument,
};
use ooxml_schema::sml;

use crate::edit::{CellOp, CellWritePlan, Extra};
use crate::reference::{CellIndex, CellRange};
use crate::workbook::{REL_METADATA, Workbook};
use crate::{CellMut, CellValue, Error, Result, SML_NS};

pub(crate) const XDA: &str = "http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray";
const URI: &str = "{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}";
const TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml";
const NAME: QName = QName::new("", "name");
const COUNT: QName = QName::new("", "count");
const DYNAMIC: QName = QName::new("", "fDynamic");
const COLLAPSED: QName = QName::new("", "fCollapsed");

enum Properties {}
impl ElementSpec for Properties {
    const QNAME: QName = QName::new(XDA, "dynamicArrayProperties");
    const SDK_NAME: &'static str = "xda:dynamicArrayProperties";
    const ATTRS: &'static [AttrSpec] = &[
        AttrSpec {
            name: DYNAMIC,
            ty: SimpleType::String,
            default: None,
        },
        AttrSpec {
            name: COLLAPSED,
            ty: SimpleType::String,
            default: None,
        },
    ];
    const CONTENT: Content = Content::Empty;
}
impl ChildOf<sml::Extension> for Properties {}

fn uncollapsed(value: Option<&str>) -> bool {
    matches!(value, None | Some("0" | "false"))
}

fn active_block(block: &RawNode) -> bool {
    block
        .elements()
        .filter(|n| n.is(sml::ExtensionList::QNAME))
        .flat_map(RawNode::elements)
        .filter(|n| n.is(sml::Extension::QNAME))
        .flat_map(RawNode::elements)
        .any(|n| {
            n.is(Properties::QNAME)
                && matches!(n.attr(DYNAMIC), Some("1" | "true"))
                && uncollapsed(n.attr(COLLAPSED))
        })
}

fn ensure_block(doc: &mut XmlDocument) -> Result<u32> {
    if let Some(cm) =
        crate::census::scan_dynamic_array_cms(doc.serialize().as_bytes(), |properties| {
            Ok(uncollapsed(
                crate::census::attr(properties, "fCollapsed")?.as_deref(),
            ))
        })
        .map_err(Error::Malformed)?
        .into_iter()
        .min()
    {
        return Ok(cm);
    }

    let mut metadata = doc.lens_mut::<sml::Metadata>()?;
    let t = {
        let mut types = metadata.get_or_insert::<sml::MetadataTypes>()?;
        let existing = types
            .as_lens()
            .children::<sml::MetadataType>()
            .position(|entry| entry.attr(NAME) == Some("XLDAPR"));
        if let Some(index) = existing {
            index + 1
        } else {
            let count = types.as_lens().children::<sml::MetadataType>().count() + 1;
            let mut entry = types.insert::<sml::MetadataType>()?;
            entry.set_attr(NAME, "XLDAPR")?;
            entry.set_attr(QName::new("", "minSupportedVersion"), "120000")?;
            for name in [
                "copy",
                "pasteAll",
                "pasteValues",
                "merge",
                "splitFirst",
                "rowColShift",
                "clearFormats",
                "clearComments",
                "assign",
                "coerce",
                "cellMeta",
            ] {
                entry.set_attr(QName::new("", name), "1")?;
            }
            types.set_attr(COUNT, &count.to_string())?;
            count
        }
    };
    let v = {
        let mut future = match metadata
            .child_mut_by::<sml::FutureMetadata>(|n| n.attr(NAME) == Some("XLDAPR"))
        {
            Some(future) => future,
            None => {
                let mut future = metadata.insert::<sml::FutureMetadata>()?;
                future.set_attr(NAME, "XLDAPR")?;
                future
            }
        };
        if let Some(index) = future
            .as_lens()
            .children::<sml::FutureMetadataBlock>()
            .position(|b| active_block(b.raw()))
        {
            index
        } else {
            let index = future
                .as_lens()
                .children::<sml::FutureMetadataBlock>()
                .count();
            let mut block = future.insert::<sml::FutureMetadataBlock>()?;
            let mut list = block.insert::<sml::ExtensionList>()?;
            let mut ext = list.insert_with_ns::<sml::Extension>(&[NsDecl::new("xda", XDA)])?;
            ext.set_attr(QName::new("", "uri"), URI)?;
            let mut properties = ext.insert::<Properties>()?;
            properties.set_attr(DYNAMIC, "1")?;
            properties.set_attr(COLLAPSED, "0")?;
            future.set_attr(COUNT, &(index + 1).to_string())?;
            index
        }
    };
    let mut cells = metadata.get_or_insert::<sml::CellMetadata>()?;
    let count = cells.as_lens().children::<sml::MetadataBlock>().count() + 1;
    let mut block = cells.insert::<sml::MetadataBlock>()?;
    let mut record = block.insert::<sml::MetadataRecord>()?;
    record.set_attr(QName::new("", "t"), &t.to_string())?;
    record.set_attr(QName::new("", "v"), &v.to_string())?;
    cells.set_attr(COUNT, &count.to_string())?;
    u32::try_from(count).map_err(|_| Error::Malformed("too many cell metadata blocks".into()))
}

pub(crate) fn formula_cache(at: crate::CellIndex, result: CellValue) -> Result<crate::edit::FormulaCache> {
    use sml::CellValues;

    Ok(match result {
        CellValue::Empty => None,
        CellValue::Number(value) if value.is_finite() => Some((CellValues::Number, crate::worksheet::format_number(value))),
        CellValue::Number(value) => return Err(Error::InvalidNumber(value)),
        CellValue::String(value) => {
            let units = value.encode_utf16().count();
            if units > crate::worksheet::MAX_STRING_LEN {
                return Err(Error::StringTooLong(units));
            }
            let escaped = crate::escape::encode(&value).into_owned();
            if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(&escaped) {
                return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
            }
            Some((CellValues::String, escaped))
        }
        CellValue::Date(value) if crate::setters::parse_iso_date(&value).is_some() =>
            Some((CellValues::Date, value)),
        CellValue::Bool(value) => Some((CellValues::Boolean, u8::from(value).to_string())),
        CellValue::Error(value) if crate::setters::CELL_ERRORS.contains(&value.as_str()) => {
            Some((CellValues::Error, value))
        }
        _ => return Err(Error::InvalidValue {
            cell: at.to_string(),
            message: "unsupported formula result".into(),
        }),
    })
}

impl CellMut<'_, '_> {
    /// Writes an array formula over the given inclusive range. The cell
    /// must be its top-left anchor. Dynamic arrays also receive XLDAPR metadata.
    pub fn set_array_formula(self, text: &str, range: CellRange, dynamic: bool) -> Result<()> {
        self.set_array_formula_with_result(text, range, dynamic, CellValue::Empty)
    }

    /// Writes an array formula and a caller-supplied cached result on its anchor.
    /// Supported results are numbers, strings, booleans, UTC ISO dates and standard Excel errors.
    pub fn set_array_formula_with_result(
        self, text: &str, range: CellRange, dynamic: bool, result: CellValue,
    ) -> Result<()> {
        let cache = formula_cache(self.at, result)?;
        let wb = &mut *self.ws.wb;
        wb.check_editable()?;
        if wb.census.implicit_references.contains(&self.ws.sheet) {
            return Err(Error::MissingReference {
                part: wb.sheets[self.ws.sheet].part.clone().expect("worksheet"),
                what: "row or cell (an array requires explicit references)".into(),
            });
        }
        crate::CellIndex::new(range.last.row, range.last.col)?;
        if range.first != self.at || !range.contains(self.at) {
            return Err(Error::Malformed("array formula must start at its anchor".into()));
        }
        let text = text.strip_prefix('=').unwrap_or(text);
        if !dynamic {
            let plan = wb.plan_writes(vec![(
                self.ws.sheet, self.at, CellOp::ArrayFormula(text.into(), range, None, cache), Extra::default(),
            )])?;
            return wb.apply_writes(plan);
        }
        let rels = wb.pkg.relationships(&wb.workbook_part)?;
        let mut related = rels.by_type(REL_METADATA);
        let part = match related.next() {
            None => None,
            Some(rel) => Some(
                rels.resolve(rel)
                    .filter(|_| related.next().is_none())
                    .ok_or_else(|| Error::PartNotEditable {
                        part: wb.workbook_part.clone(),
                        reason: "ambiguous or external sheet metadata relationship".into(),
                    })?
                    .to_owned(),
            ),
        };
        let mut doc = if let Some(part) = &part {
            wb.pkg
                .xml_part(part)
                .map_err(|e| Error::PartNotEditable {
                    part: part.clone(),
                    reason: e.to_string(),
                })?
                .clone()
        } else {
            XmlDocument::parse(format!("<metadata xmlns=\"{SML_NS}\"/>").as_bytes())?
        };
        if !doc.root().is(sml::Metadata::QNAME) {
            return Err(Error::PartNotEditable {
                part: part.expect("only existing metadata can have an unexpected root"),
                reason: "expected a SpreadsheetML metadata root".into(),
            });
        }
        let cm = ensure_block(&mut doc)?;
        let plan = wb.plan_writes(vec![(
            self.ws.sheet,
            self.at,
            CellOp::ArrayFormula(text.into(), range, Some(cm), cache),
            Extra::default(),
        )])?;
        let mut registry = RegistryPlan::new(&wb.pkg);
        if let Some(part) = part {
            if doc.is_dirty() {
                registry.stage_part(&wb.pkg, &part, doc)?;
            }
        } else {
            let prefix = wb
                .workbook_part
                .rsplit_once('/')
                .map_or_else(|| "metadata".into(), |(dir, _)| format!("{dir}/metadata"));
            let part = match registry.add_part(
                &wb.pkg,
                PartNameRequest::Exact(format!("{prefix}.xml")),
                TYPE,
                doc.clone(),
            ) {
                Err(ooxml_core::Error::RegistryRefused {
                    reason: RegistryRefusal::NameInUse { .. },
                    ..
                }) => registry.add_part(
                    &wb.pkg,
                    PartNameRequest::Numbered(format!("{prefix}{{}}.xml")),
                    TYPE,
                    doc,
                )?,
                result => result?,
            };
            registry.add_relationship(
                &wb.pkg,
                Some(&wb.workbook_part),
                REL_METADATA,
                Target::Part(part),
            )?;
        }
        wb.apply_writes_with(plan, registry)
    }
}

/// What a dynamic-formula write does to the metadata part: the cloned doc
/// plus the create-or-stage choice. Built by the plan, installed by the apply.
#[derive(Debug)]
pub(crate) enum MetaAction {
    /// Stage the doc over an existing part (a no-op when it is unchanged).
    Stage { part: String, doc: XmlDocument },
    /// Create the part from the doc.
    Create { doc: XmlDocument },
}

impl MetaAction {
    /// The registry transaction installing the doc: staged over `part`, or a
    /// new part related from the workbook. Pure: reads the package only.
    pub(crate) fn into_registry(self, pkg: &Package, workbook_part: &str) -> Result<RegistryPlan> {
        let mut registry = RegistryPlan::new(pkg);
        match self {
            MetaAction::Stage { part, doc } => {
                if doc.is_dirty() {
                    registry.stage_part(pkg, &part, doc)?;
                }
            }
            MetaAction::Create { doc } => {
                let prefix = workbook_part
                    .rsplit_once('/')
                    .map_or_else(|| "metadata".into(), |(dir, _)| format!("{dir}/metadata"));
                let part = match registry.add_part(
                    pkg,
                    PartNameRequest::Exact(format!("{prefix}.xml")),
                    TYPE,
                    doc.clone(),
                ) {
                    Err(ooxml_core::Error::RegistryRefused {
                        reason: RegistryRefusal::NameInUse { .. },
                        ..
                    }) => registry.add_part(
                        pkg,
                        PartNameRequest::Numbered(format!("{prefix}{{}}.xml")),
                        TYPE,
                        doc,
                    )?,
                    result => result?,
                };
                registry.add_relationship(
                    pkg,
                    Some(workbook_part),
                    REL_METADATA,
                    Target::Part(part),
                )?;
            }
        }
        Ok(registry)
    }
}

impl Workbook {
    /// Plans a dynamic-formula write (C1's plan entry): the metadata work on
    /// a cloned doc plus the cell plan, validated without applying. The caller
    /// installs both (`MetaAction::into_registry` + `apply_writes_with`).
    pub(crate) fn plan_dynamic_formula(
        &mut self,
        sheet: usize,
        at: CellIndex,
        text: &str,
    ) -> Result<(MetaAction, CellWritePlan)> {
        self.check_editable()?;
        if self.census.implicit_references.contains(&sheet) {
            return Err(Error::MissingReference {
                part: self.sheets[sheet].part.clone().expect("worksheet"),
                what: "row or cell (a dynamic array requires explicit references)".into(),
            });
        }
        let rels = self.pkg.relationships(&self.workbook_part)?;
        let mut related = rels.by_type(REL_METADATA);
        let part = match related.next() {
            None => None,
            Some(rel) => Some(
                rels.resolve(rel)
                    .filter(|_| related.next().is_none())
                    .ok_or_else(|| Error::PartNotEditable {
                        part: self.workbook_part.clone(),
                        reason: "ambiguous or external sheet metadata relationship".into(),
                    })?
                    .to_owned(),
            ),
        };
        let mut doc = if let Some(part) = &part {
            self.pkg
                .xml_part(part)
                .map_err(|e| Error::PartNotEditable {
                    part: part.clone(),
                    reason: e.to_string(),
                })?
                .clone()
        } else {
            XmlDocument::parse(format!("<metadata xmlns=\"{SML_NS}\"/>").as_bytes())?
        };
        if !doc.root().is(sml::Metadata::QNAME) {
            return Err(Error::PartNotEditable {
                part: part.expect("only existing metadata can have an unexpected root"),
                reason: "expected a SpreadsheetML metadata root".into(),
            });
        }
        let cm = ensure_block(&mut doc)?;
        let text = text.strip_prefix('=').unwrap_or(text);
        let plan = self.plan_writes(vec![(
            sheet,
            at,
            CellOp::DynamicFormula(text.into(), cm),
            Extra::default(),
        )])?;
        let action = match part {
            Some(part) => MetaAction::Stage { part, doc },
            None => MetaAction::Create { doc },
            };
        Ok((action, plan))
        }
}

impl CellMut<'_, '_> {
    /// Writes an uncached dynamic-array formula in file form, with the
    /// anchor as its initial array reference. See [`Self::set_formula`]
    /// for formula syntax and future-function prefixes.
    ///
    /// Excel calculates the spill range. A value in the potential range can
    /// cause `#SPILL!` in Excel. Only an existing recorded result range
    /// restricts adjacent cell writes.
    pub fn set_dynamic_formula(self, text: &str) -> Result<()> {
        let wb = &mut *self.ws.wb;
        let (action, plan) = wb.plan_dynamic_formula(self.ws.sheet, self.at, text)?;
        let registry = action.into_registry(&wb.pkg, &wb.workbook_part)?;
        wb.apply_writes_with(plan, registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(a1: &str) -> CellIndex {
        CellIndex::parse(a1).unwrap()
    }

    fn assert_clean(wb: &Workbook) {
        for name in wb.pkg.part_names() {
            if let Some(doc) = wb.pkg.parsed_part(name) {
                assert!(!doc.is_dirty(), "{name} is dirty");
                assert!(!doc.root().subtree_dirty(), "{name} has a dirty subtree");
            }
        }
    }

    fn has_metadata(wb: &Workbook) -> bool {
        wb.pkg.part_names().any(|n| n.contains("metadata"))
    }

    #[test]
    fn plan_plus_registry_plus_apply_equals_set_dynamic_formula() {
        // create path (no metadata part yet), with `=`-stripping
        let wb = Workbook::new().unwrap();
        assert!(!has_metadata(&wb));
        let bytes = wb.to_bytes().unwrap();
        let mut planned = Workbook::from_bytes(bytes.clone()).unwrap();
        assert_clean(&planned);
        let (action, plan) = planned.plan_dynamic_formula(0, at("A1"), "=1+1").unwrap();
        assert!(matches!(action, MetaAction::Create { .. }));
        assert_eq!(planned.to_bytes().unwrap(), bytes, "a plan changes nothing");
        assert_clean(&planned);
        let registry = action
            .into_registry(&planned.pkg, &planned.workbook_part)
            .unwrap();
        planned.apply_writes_with(plan, registry).unwrap();
        let mut combined = Workbook::from_bytes(bytes).unwrap();
        combined
            .worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_dynamic_formula("=1+1")
            .unwrap();
        assert_eq!(planned.to_bytes().unwrap(), combined.to_bytes().unwrap());
        // reuse path: the block is reused, the cloned doc stays clean
        let bytes = planned.to_bytes().unwrap();
        let mut planned = Workbook::from_bytes(bytes.clone()).unwrap();
        let (action, plan) = planned.plan_dynamic_formula(0, at("A3"), "2+2").unwrap();
        assert!(matches!(action, MetaAction::Stage { .. }));
        assert_eq!(planned.to_bytes().unwrap(), bytes, "a plan changes nothing");
        assert_clean(&planned);
        let registry = action
            .into_registry(&planned.pkg, &planned.workbook_part)
            .unwrap();
        planned.apply_writes_with(plan, registry).unwrap();
        let mut combined = Workbook::from_bytes(bytes).unwrap();
        combined
            .worksheet()
            .unwrap()
            .cell_mut(2, 0)
            .unwrap()
            .set_dynamic_formula("2+2")
            .unwrap();
        assert_eq!(planned.to_bytes().unwrap(), combined.to_bytes().unwrap());
    }

    #[test]
    fn metadata_refusals_are_byte_identical() {
        // an unparsable formula needs no fixture
        let mut wb = Workbook::new().unwrap();
        let before = wb.to_bytes().unwrap();
        std::assert_matches!(
            wb.plan_dynamic_formula(0, at("A7"), "1+"),
            Err(Error::InvalidFormula { .. })
        );
        assert_eq!(wb.to_bytes().unwrap(), before);
        assert_clean(&wb);
        // a metadata part with the wrong root
        let bytes = Workbook::new().unwrap().to_bytes().unwrap();
        let mut pkg = ooxml_core::Package::from_bytes(bytes).unwrap();
        let mut reg = ooxml_core::RegistryPlan::new(&pkg);
        let part = reg
            .add_part(
                &pkg,
                ooxml_core::PartNameRequest::Exact("xl/metadata.xml".into()),
                TYPE,
                b"<foo/>".to_vec(),
            )
            .unwrap();
        reg.add_relationship(
            &pkg,
            Some("xl/workbook.xml"),
            REL_METADATA,
            ooxml_core::Target::Part(part),
        )
        .unwrap();
        pkg.apply(reg).unwrap();
        let bytes = pkg.to_bytes().unwrap();
        let mut wb = Workbook::from_bytes(bytes.clone()).unwrap();
        std::assert_matches!(
            wb.plan_dynamic_formula(0, at("A1"), "1+1"),
            Err(Error::PartNotEditable { .. })
        );
        assert_eq!(wb.to_bytes().unwrap(), bytes);
        assert_clean(&wb);
    }
}
