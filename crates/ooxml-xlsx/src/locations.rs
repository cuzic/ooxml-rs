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
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule {
    Rewrite,
    Keep,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Syntax {
    Formula,
    Location,
    PivotSource,
    Sqref,
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
    location!(49, Chart, PivotSource, Keep, None, None; CHART_NS:"pivotSource", CHART_NS:"name"),
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
}
