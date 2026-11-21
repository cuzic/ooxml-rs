//! Sheet-name dispatch for the reference locations in ADR-0018 §2.2.

use crate::SML_NS as SML;
use crate::x14::{NS_X14 as X14, NS_XM as XM, URI_CF, URI_DV};

pub(crate) const CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
pub(crate) const C15_NS: &str = "http://schemas.microsoft.com/office/drawing/2012/chart";
const VML: &str = "urn:schemas-microsoft-com:office:excel";
const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const CDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/chartDrawing";
const SPARKLINE: &str = "{05C60535-1F16-4FD2-B633-F4F36F0B64E0}";
const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartKind {
    Worksheet,
    Workbook,
    NonReference,
    Relationships,
    Chart,
    Table,
    Connections,
    Control,
    Vml,
    Drawing,
    ChartShapes,
    CalcChain,
    Comments,
    PivotCache,
    PivotTable,
    SingleXmlCells,
    VolatileDependencies,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule {
    Refuse,
    Rewrite,
    Keep,
    Unaffected,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Syntax {
    Formula,
    Location,
    PivotSource,
    Sqref,
    Cell,
    Range,
}

#[derive(Clone, Copy)]
enum Scope {
    Any,
    Own,
}

pub(crate) struct Location {
    pub(crate) id: u8,
    pub(crate) part: PartKind,
    path: &'static [(&'static str, &'static str)],
    attr: Option<&'static str>,
    pub(crate) syntax: Syntax,
    scope: Scope,
    pub(crate) rename: Rule,
    pub(crate) delete: Rule,
    extension: Option<&'static str>,
}

impl Location {
    pub(crate) fn sheet_rule(&self, deleting: bool) -> Rule {
        if deleting { self.delete } else { self.rename }
    }

    pub(crate) fn matches_chart_element(&self, name: &ooxml_core::XName) -> bool {
        matches!(self.path, [(ns, local)] if *ns == name.ns() && *local == name.local())
    }

    pub(crate) fn accepts_sheet_prefix(&self) -> bool {
        matches!(self.scope, Scope::Any) && self.rename != Rule::Unaffected
    }

    pub(crate) fn matches(
        &self,
        kind: PartKind,
        path: &[(String, String)],
        attr: Option<&str>,
        uri: Option<&str>,
    ) -> bool {
        let anchored = !matches!(
            self.part,
            PartKind::Chart | PartKind::Vml | PartKind::Drawing | PartKind::ChartShapes
        );
        self.part == kind
            && self.attr == attr
            && (!anchored || path.len() == self.path.len())
            && path.len() >= self.path.len()
            && path[path.len() - self.path.len()..]
                .iter()
                .zip(self.path)
                .all(|((ns, name), (n, l))| ns == n && name == l)
            && match (self.extension, uri) {
                (Some(expected), Some(actual)) => expected.eq_ignore_ascii_case(actual),
                (None, _) => true,
                _ => false,
            }
    }
}

macro_rules! location {
    ($id:literal, $part:ident, $syntax:ident, $delete:ident, $attr:expr, $uri:expr; $($ns:ident : $name:literal),+) => {
        Location { id: $id, part: PartKind::$part, path: &[$(($ns, $name)),+], attr: $attr,
            syntax: Syntax::$syntax, scope: Scope::Any, rename: Rule::Rewrite, delete: Rule::$delete, extension: $uri }
    };
}

macro_rules! sqref {
    ($id:literal, $uri:expr; $($ns:ident : $name:literal),+) => {
        Location { id: $id, part: PartKind::Worksheet, path: &[$(($ns, $name)),+], attr: None,
            syntax: Syntax::Sqref, scope: Scope::Own, rename: Rule::Keep, delete: Rule::Keep,
            extension: Some($uri) }
    };
}

macro_rules! position {
    ($id:literal, $part:ident, $syntax:ident, $attr:literal; $($name:literal),+) => {
        Location { id: $id, part: PartKind::$part, path: &[$((SML, $name)),+], attr: Some($attr),
            syntax: Syntax::$syntax, scope: Scope::Own, rename: Rule::Unaffected, delete: Rule::Unaffected,
            extension: None }
    };
}

const PROTECTED_RANGES: &str = "{FC87AEE6-9EDD-4A0A-B7FB-166176984837}";
const IGNORED_ERRORS: &str = "{01252117-D84E-4E92-8308-4BE1C098FCBB}";

/// Worksheet extension identities in ADR-0018. Empty row lists document
/// extensions whose contents carry no positions.
const EXTENSIONS: &[(&str, &[u8])] = &[
    (URI_CF, &[20]),
    (URI_DV, &[20]),
    (SPARKLINE, &[21]),
    (PROTECTED_RANGES, &[68]),
    (IGNORED_ERRORS, &[69]),
    ("{F7C9EE02-42E1-4005-9D12-6889AFFD525C}", &[70]),
    ("{A8765BA9-456A-4DAB-B4F3-ACF838C121DE}", &[]),
    ("{7E03D99C-DC04-49d9-9315-930204A7B6E9}", &[]),
    ("{B025F937-C7B1-47D3-B67F-A62EFF666E3E}", &[]),
    ("{0FC57C36-975A-43D2-8416-536B9B585D1B}", &[74]),
    ("{68452572-C921-441B-96FD-77FB3E787301}", &[74]),
    (
        "http://schemas.microsoft.com/office/mac/excel/2008/main",
        &[],
    ),
    ("{64002731-A6B0-56B0-2670-7721B7C09600}", &[]),
];

pub(crate) const LOCATIONS: &[Location] = &[
    position!(15, Worksheet, Range, "ref"; "worksheet", "autoFilter"),
    position!(15, Worksheet, Range, "ref"; "worksheet", "autoFilter", "sortState"),
    position!(15, Worksheet, Range, "ref"; "worksheet", "autoFilter", "sortState", "sortCondition"),
    position!(15, Worksheet, Range, "ref"; "worksheet", "sortState"),
    position!(15, Worksheet, Range, "ref"; "worksheet", "sortState", "sortCondition"),
    position!(25, Worksheet, Range, "ref"; "worksheet", "customSheetViews", "customSheetView", "autoFilter"),
    position!(25, Worksheet, Range, "ref"; "worksheet", "customSheetViews", "customSheetView", "autoFilter", "sortState"),
    position!(25, Worksheet, Range, "ref"; "worksheet", "customSheetViews", "customSheetView", "autoFilter", "sortState", "sortCondition"),
    position!(35, Table, Range, "ref"; "table", "autoFilter"),
    position!(35, Table, Range, "ref"; "table", "autoFilter", "sortState"),
    position!(35, Table, Range, "ref"; "table", "autoFilter", "sortState", "sortCondition"),
    position!(35, Table, Range, "ref"; "table", "sortState"),
    position!(35, Table, Range, "ref"; "table", "sortState", "sortCondition"),
    position!(34, CalcChain, Cell, "r"; "calcChain", "c"),
    position!(34, CalcChain, Cell, "ref"; "calcChain", "c"),
    position!(39, Comments, Cell, "ref"; "comments", "commentList", "comment"),
    position!(50, PivotCache, Range, "ref"; "pivotCacheDefinition", "cacheSource", "worksheetSource"),
    position!(53, PivotCache, Range, "ref"; "pivotCacheDefinition", "cacheSource", "consolidation", "rangeSets", "rangeSet"),
    position!(52, PivotTable, Range, "ref"; "pivotTableDefinition", "location"),
    position!(3, Worksheet, Cell, "r"; "worksheet", "sheetData", "row", "c"),
    position!(7, Worksheet, Range, "ref"; "worksheet", "dimension"),
    position!(9, Worksheet, Cell, "topLeftCell"; "worksheet", "sheetViews", "sheetView"),
    position!(9, Worksheet, Cell, "topLeftCell"; "worksheet", "sheetViews", "sheetView", "pane"),
    position!(9, Worksheet, Cell, "topLeftCell"; "worksheet", "customSheetViews", "customSheetView"),
    position!(25, Worksheet, Cell, "topLeftCell"; "worksheet", "customSheetViews", "customSheetView", "pane"),
    position!(11, Worksheet, Cell, "activeCell"; "worksheet", "sheetViews", "sheetView", "selection"),
    position!(11, Worksheet, Sqref, "sqref"; "worksheet", "sheetViews", "sheetView", "selection"),
    position!(25, Worksheet, Cell, "activeCell"; "worksheet", "customSheetViews", "customSheetView", "selection"),
    position!(25, Worksheet, Sqref, "sqref"; "worksheet", "customSheetViews", "customSheetView", "selection"),
    position!(28, Worksheet, Range, "ref"; "worksheet", "dataConsolidate", "dataRefs", "dataRef"),
    position!(12, Worksheet, Range, "ref"; "worksheet", "mergeCells", "mergeCell"),
    position!(26, Worksheet, Cell, "r"; "worksheet", "smartTags", "cellSmartTags"),
    position!(26, Worksheet, Cell, "r"; "worksheet", "cellWatches", "cellWatch"),
    position!(18, Worksheet, Sqref, "sqref"; "worksheet", "dataValidations", "dataValidation"),
    position!(16, Worksheet, Sqref, "sqref"; "worksheet", "conditionalFormatting"),
    position!(13, Worksheet, Range, "ref"; "worksheet", "hyperlinks", "hyperlink"),
    position!(5, Worksheet, Range, "ref"; "worksheet", "sheetData", "row", "c", "f"),
    position!(6, Worksheet, Cell, "r1"; "worksheet", "sheetData", "row", "c", "f"),
    position!(6, Worksheet, Cell, "r2"; "worksheet", "sheetData", "row", "c", "f"),
    position!(27, Worksheet, Sqref, "sqref"; "worksheet", "scenarios"),
    position!(27, Worksheet, Cell, "r"; "worksheet", "scenarios", "scenario", "inputCells"),
    position!(24, Worksheet, Sqref, "sqref"; "worksheet", "protectedRanges", "protectedRange"),
    position!(29, Worksheet, Range, "sourceRef"; "worksheet", "webPublishItems", "webPublishItem"),
    position!(23, Worksheet, Sqref, "sqref"; "worksheet", "ignoredErrors", "ignoredError"),
    position!(54, SingleXmlCells, Cell, "r"; "singleXmlCells", "singleXmlCell"),
    position!(35, Table, Range, "ref"; "table"),
    position!(65, VolatileDependencies, Cell, "r"; "volTypes", "volType", "main", "tp", "tr"),
    position!(62, Workbook, Range, "ref"; "workbook", "oleSize"),
    location!(4, Worksheet, Formula, Rewrite, None, None; SML:"worksheet", SML:"sheetData", SML:"row", SML:"c", SML:"f"),
    location!(14, Worksheet, Location, Keep, Some("location"), None; SML:"worksheet", SML:"hyperlinks", SML:"hyperlink"),
    location!(17, Worksheet, Formula, Rewrite, None, None; SML:"worksheet", SML:"conditionalFormatting", SML:"cfRule", SML:"formula"),
    location!(17, Worksheet, Formula, Rewrite, Some("val"), None; SML:"worksheet", SML:"conditionalFormatting", SML:"cfRule", SML:"colorScale", SML:"cfvo"),
    location!(17, Worksheet, Formula, Rewrite, Some("val"), None; SML:"worksheet", SML:"conditionalFormatting", SML:"cfRule", SML:"dataBar", SML:"cfvo"),
    location!(17, Worksheet, Formula, Rewrite, Some("val"), None; SML:"worksheet", SML:"conditionalFormatting", SML:"cfRule", SML:"iconSet", SML:"cfvo"),
    location!(19, Worksheet, Formula, Rewrite, None, None; SML:"worksheet", SML:"dataValidations", SML:"dataValidation", SML:"formula1"),
    location!(19, Worksheet, Formula, Rewrite, None, None; SML:"worksheet", SML:"dataValidations", SML:"dataValidation", SML:"formula2"),
    location!(20, Worksheet, Formula, Rewrite, None, Some(URI_CF); SML:"worksheet", SML:"extLst", SML:"ext", X14:"conditionalFormattings", X14:"conditionalFormatting", X14:"cfRule", XM:"f"),
    location!(20, Worksheet, Formula, Rewrite, None, Some(URI_DV); SML:"worksheet", SML:"extLst", SML:"ext", X14:"dataValidations", X14:"dataValidation", X14:"formula1", XM:"f"),
    location!(20, Worksheet, Formula, Rewrite, None, Some(URI_DV); SML:"worksheet", SML:"extLst", SML:"ext", X14:"dataValidations", X14:"dataValidation", X14:"formula2", XM:"f"),
    location!(20, Worksheet, Formula, Rewrite, None, Some(URI_CF); SML:"worksheet", SML:"extLst", SML:"ext", X14:"conditionalFormattings", X14:"conditionalFormatting", X14:"cfRule", X14:"colorScale", X14:"cfvo", XM:"f"),
    location!(20, Worksheet, Formula, Rewrite, None, Some(URI_CF); SML:"worksheet", SML:"extLst", SML:"ext", X14:"conditionalFormattings", X14:"conditionalFormatting", X14:"cfRule", X14:"dataBar", X14:"cfvo", XM:"f"),
    location!(20, Worksheet, Formula, Rewrite, None, Some(URI_CF); SML:"worksheet", SML:"extLst", SML:"ext", X14:"conditionalFormattings", X14:"conditionalFormatting", X14:"cfRule", X14:"iconSet", X14:"cfvo", XM:"f"),
    sqref!(20, URI_CF; SML:"worksheet", SML:"extLst", SML:"ext", X14:"conditionalFormattings", X14:"conditionalFormatting", XM:"sqref"),
    sqref!(20, URI_DV; SML:"worksheet", SML:"extLst", SML:"ext", X14:"dataValidations", X14:"dataValidation", XM:"sqref"),
    sqref!(21, SPARKLINE; SML:"worksheet", SML:"extLst", SML:"ext", X14:"sparklineGroups", X14:"sparklineGroup", X14:"sparklines", X14:"sparkline", XM:"sqref"),
    sqref!(68, PROTECTED_RANGES; SML:"worksheet", SML:"extLst", SML:"ext", X14:"protectedRanges", X14:"protectedRange", XM:"sqref"),
    sqref!(69, IGNORED_ERRORS; SML:"worksheet", SML:"extLst", SML:"ext", X14:"ignoredErrors", X14:"ignoredError", XM:"sqref"),
    location!(21, Worksheet, Formula, Rewrite, None, Some(SPARKLINE); SML:"worksheet", SML:"extLst", SML:"ext", X14:"sparklineGroups", X14:"sparklineGroup", X14:"sparklines", X14:"sparkline", XM:"f"),
    location!(21, Worksheet, Formula, Rewrite, None, Some(SPARKLINE); SML:"worksheet", SML:"extLst", SML:"ext", X14:"sparklineGroups", X14:"sparklineGroup", XM:"f"),
    location!(30, Worksheet, Formula, Rewrite, Some("linkedCell"), None; SML:"worksheet", SML:"controls", SML:"control", SML:"controlPr"),
    location!(30, Worksheet, Formula, Rewrite, Some("listFillRange"), None; SML:"worksheet", SML:"controls", SML:"control", SML:"controlPr"),
    location!(32, Workbook, Formula, Rewrite, None, None; SML:"workbook", SML:"definedNames", SML:"definedName"),
    location!(37, Table, Formula, Rewrite, None, None; SML:"table", SML:"tableColumns", SML:"tableColumn", SML:"calculatedColumnFormula"),
    location!(37, Table, Formula, Rewrite, None, None; SML:"table", SML:"tableColumns", SML:"tableColumn", SML:"totalsRowFormula"),
    location!(42, Vml, Formula, Rewrite, None, None; VML:"ClientData", VML:"FmlaLink"),
    location!(42, Vml, Formula, Rewrite, None, None; VML:"ClientData", VML:"FmlaRange"),
    location!(42, Vml, Formula, Rewrite, None, None; VML:"ClientData", VML:"FmlaTxbx"),
    location!(42, Vml, Formula, Rewrite, None, None; VML:"ClientData", VML:"FmlaPict"),
    location!(42, Vml, Formula, Rewrite, None, None; VML:"ClientData", VML:"FmlaGroup"),
    location!(46, Drawing, Formula, Rewrite, Some("textlink"), None; XDR:"sp"),
    location!(46, ChartShapes, Formula, Rewrite, Some("textlink"), None; CDR:"sp"),
    location!(47, Chart, Formula, Rewrite, None, None; CHART_NS:"f"),
    location!(47, Chart, Formula, Rewrite, None, None; C15_NS:"f"),
    location!(49, Chart, PivotSource, Refuse, None, None; CHART_NS:"pivotSource", CHART_NS:"name"),
    location!(55, Connections, Formula, Rewrite, Some("cell"), None; SML:"connections", SML:"connection", SML:"parameters", SML:"parameter"),
    location!(56, Control, Formula, Rewrite, Some("fmlaLink"), None; X14:"formControlPr"),
    location!(56, Control, Formula, Rewrite, Some("fmlaRange"), None; X14:"formControlPr"),
    location!(56, Control, Formula, Rewrite, Some("fmlaTxbx"), None; X14:"formControlPr"),
    location!(56, Control, Formula, Rewrite, Some("fmlaGroup"), None; X14:"formControlPr"),
    location!(71, Relationships, Location, Keep, Some("Target"), None; RELS:"Relationships", RELS:"Relationship"),
];

const _: () = {
    let mut index = 0;
    while index < LOCATIONS.len() {
        let row = &LOCATIONS[index];
        if matches!(row.syntax, Syntax::Formula | Syntax::Location) {
            assert!(matches!(row.scope, Scope::Any));
        }
        index += 1;
    }
};

pub(crate) fn sheet_rewrites(id: u8, deleting: bool) -> impl Iterator<Item = &'static Location> {
    LOCATIONS
        .iter()
        .filter(move |row| row.id == id && row.sheet_rule(deleting) == Rule::Rewrite)
}

pub(crate) fn find(
    kind: PartKind,
    path: &[(String, String)],
    attr: Option<&str>,
    uri: Option<&str>,
) -> Option<&'static Location> {
    LOCATIONS.iter().find(|row| {
        row.matches(kind, path, attr, uri)
            && row
                .extension
                .is_none_or(|_| extension_rows(uri).contains(&row.id))
    })
}

pub(crate) fn additional_part(content_type: &str) -> PartKind {
    match content_type {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml" => PartKind::Table,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml" => {
            PartKind::Connections
        }
        "application/vnd.ms-excel.controlproperties+xml" => PartKind::Control,
        "application/vnd.openxmlformats-officedocument.vmldrawing" => PartKind::Vml,
        "application/vnd.openxmlformats-officedocument.drawing+xml" => PartKind::Drawing,
        "application/vnd.openxmlformats-officedocument.drawingml.chartshapes+xml" => {
            PartKind::ChartShapes
        }
        _ => PartKind::Other,
    }
}

fn extension_rows(uri: Option<&str>) -> &'static [u8] {
    uri.and_then(|uri| {
        EXTENSIONS
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(uri))
    })
    .map_or(&[], |(_, ids)| ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_uris_match_the_adr() {
        let adr = include_str!("../../../docs/adr/0018-xlsx-phase7-structural-edits.md");
        let table = adr
            .split_once("| URI | Element | Rows / rule | Corpus |")
            .unwrap()
            .1;
        let uris: Vec<_> = table
            .lines()
            .skip(2)
            .take_while(|line| line.starts_with('|'))
            .flat_map(|line| {
                line.split('|')
                    .nth(1)
                    .unwrap()
                    .split('`')
                    .skip(1)
                    .step_by(2)
            })
            .filter(|value| value.starts_with('{') || value.starts_with("http://"))
            .collect();
        assert_eq!(uris.len(), EXTENSIONS.len());
        for uri in uris {
            assert!(
                EXTENSIONS
                    .iter()
                    .any(|(registered, _)| registered.eq_ignore_ascii_case(uri)),
                "{uri}"
            );
        }
    }

    #[test]
    fn registry_accepts_legacy_locations_for_rename() {
        for (id, count) in [(4, 1), (14, 1), (32, 1), (47, 2), (71, 1)] {
            assert_eq!(
                LOCATIONS
                    .iter()
                    .filter(|row| row.id == id && row.rename == Rule::Rewrite)
                    .count(),
                count
            );
        }
    }

    #[test]
    fn spreadsheet_schema_reference_types_are_classified() {
        use ooxml_core::{QName, RawNode, XmlDocument};

        const XSD: &str = "http://www.w3.org/2001/XMLSchema";
        const NAME: QName = QName::new("", "name");
        const TYPE: QName = QName::new("", "type");
        const BASE: QName = QName::new("", "base");
        const TYPES: &[&str] = &["ST_Ref", "ST_CellRef", "ST_Sqref", "ST_RefA", "ST_Formula"];
        const NOT_APPLICABLE: &[(&str, &str, &str)] = &[
            (
                "CT_PivotArea",
                "offset",
                "pivot internals are handled by the L52 pivot refusal",
            ),
            (
                "CT_UndoInfo",
                "dr",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_UndoInfo",
                "r",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_RevisionRowColumn",
                "ref",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_RevisionMove",
                "source",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_RevisionMove",
                "destination",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_RevisionFormatting",
                "sqref",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_RevisionAutoFormatting",
                "ref",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_RevisionComment",
                "cell",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_RevisionDefinedName",
                "formula",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_RevisionDefinedName",
                "oldFormula",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_RevisionQueryTableField",
                "ref",
                "revision history is refused as a shared workbook",
            ),
            (
                "CT_SheetPr",
                "syncRef",
                "shared workbook synchronization is covered by L58",
            ),
            ("CT_ObjectPr", "macro", "macro name, not a cell position"),
            ("CT_ControlPr", "macro", "macro name, not a cell position"),
            (
                "CT_ExternalCell",
                "r",
                "another workbook's cells are covered by L60",
            ),
        ];
        fn descendants(node: &RawNode) -> Vec<&RawNode> {
            let mut nodes = Vec::new();
            let mut stack = vec![node];
            while let Some(node) = stack.pop() {
                nodes.push(node);
                stack.extend(node.elements());
            }
            nodes
        }
        let doc = XmlDocument::parse(include_bytes!(
            "../../../reference/ecma-376-transitional-xsd/sml.xsd"
        ))
        .unwrap();
        let all = descendants(doc.root());
        let formula_types: Vec<_> = doc
            .root()
            .elements()
            .filter(|node| node.is(QName::new(XSD, "complexType")))
            .filter(|node| {
                descendants(node)
                    .iter()
                    .any(|node| node.attr(BASE) == Some("ST_Formula"))
            })
            .filter_map(|node| node.attr(NAME))
            .collect();
        let mut direct = 0;
        let mut derived = 0;
        let mut excluded = 0;
        for owner in doc.root().elements() {
            let Some(owner_name) = owner.attr(NAME) else {
                continue;
            };
            for node in descendants(owner) {
                let (Some(name), Some(ty)) = (node.attr(NAME), node.attr(TYPE)) else {
                    continue;
                };
                let is_attr = node.is(QName::new(XSD, "attribute"));
                if !is_attr && !node.is(QName::new(XSD, "element")) {
                    continue;
                }
                if TYPES.contains(&ty) {
                    direct += 1;
                } else if !is_attr && formula_types.contains(&ty) {
                    derived += 1;
                } else {
                    continue;
                }
                if NOT_APPLICABLE
                    .iter()
                    .any(|&(o, n, reason)| o == owner_name && n == name && !reason.is_empty())
                {
                    excluded += 1;
                    continue;
                }
                let matches = LOCATIONS.iter().any(|row| {
                    let Some((SML, local)) = row.path.last().copied() else {
                        return false;
                    };
                    if is_attr {
                        row.attr == Some(name)
                            && all.iter().any(|element| {
                                element.is(QName::new(XSD, "element"))
                                    && element.attr(TYPE) == Some(owner_name)
                                    && element.attr(NAME) == Some(local)
                            })
                    } else {
                        row.attr.is_none() && row.syntax == Syntax::Formula && local == name
                    }
                });
                assert!(matches, "unclassified {owner_name}/{name} ({ty})");
            }
        }
        assert_eq!(direct, 56);
        assert_eq!(derived, 4);
        assert_eq!(excluded, NOT_APPLICABLE.len());
    }
}
