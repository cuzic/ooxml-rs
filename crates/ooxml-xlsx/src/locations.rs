//! Sheet-name dispatch for the reference locations in ADR-0018 §2.2.

use crate::SML_NS as SML;
use crate::x14::{NS_X14 as X14, NS_XM as XM, URI_CF, URI_DV, URI_SPARKLINE};
use ooxml_schema::props::{NS_AP as AP, NS_VT as VT};

pub(crate) const CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
pub(crate) const C15_NS: &str = "http://schemas.microsoft.com/office/drawing/2012/chart";
const CX: &str = "http://schemas.microsoft.com/office/drawing/2014/chartex";
const VML: &str = "urn:schemas-microsoft-com:office:excel";
const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const CDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/chartDrawing";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const RICH_DATA: &str = "http://schemas.microsoft.com/office/spreadsheetml/2017/richdata2";
const TC: &str = "http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments";
const SORT_STATE: &str = "{0FC57C36-975A-43D2-8416-536B9B585D1B}";
const FILTER_COLUMN: &str = "{68452572-C921-441B-96FD-77FB3E787301}";
const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartKind {
    Worksheet,
    Workbook,
    NonReference,
    Relationships,
    Chart,
    ChartEx,
    Table,
    Connections,
    Control,
    Vml,
    Drawing,
    ChartShapes,
    AppProperties,
    CalcChain,
    Comments,
    PivotCache,
    PivotTable,
    SingleXmlCells,
    VolatileDependencies,
    ThreadedComments,
    QueryTable,
    Revisions,
    ActiveX,
    ExternalLink,
    Metadata,
    RichData,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    Refuse,
    Rewrite,
    Keep,
    Unaffected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Syntax {
    Formula,
    Location,
    PivotSource,
    RawName,
    Sqref,
    Cell,
    Range,
    Index,
    Marker,
    Count,
    Flags,
    ColumnSpans,
    ColumnOffset,
    Element,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    #[cfg_attr(not(test), expect(dead_code, reason = "structural dispatch is wired in P7a–d"))]
    rules: &'static OperationRules,
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
        matches!(self.scope, Scope::Any)
            && self.syntax != Syntax::RawName
            && self.rename != Rule::Unaffected
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
            PartKind::Chart
                | PartKind::ChartEx
                | PartKind::Vml
                | PartKind::Drawing
                | PartKind::ChartShapes
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
            syntax: Syntax::$syntax, scope: Scope::Any, rename: Rule::Rewrite, delete: Rule::$delete, rules: &OPERATION_RULES[$id - 1], extension: $uri }
    };
}

macro_rules! sqref {
    ($id:literal, $uri:expr; $($ns:ident : $name:literal),+) => {
        Location { id: $id, part: PartKind::Worksheet, path: &[$(($ns, $name)),+], attr: None,
            syntax: Syntax::Sqref, scope: Scope::Own, rename: Rule::Keep, delete: Rule::Keep,
            rules: &OPERATION_RULES[$id - 1], extension: Some($uri) }
    };
}

macro_rules! position {
    ($id:literal, $part:ident, $syntax:ident, $attr:literal; $($name:literal),+) => {
        Location { id: $id, part: PartKind::$part, path: &[$((SML, $name)),+], attr: Some($attr),
            syntax: Syntax::$syntax, scope: Scope::Own, rename: Rule::Unaffected, delete: Rule::Unaffected,
            rules: &OPERATION_RULES[$id - 1], extension: None }
    };
}

// Some rows describe an entire part or an element rather than an attribute.
macro_rules! site {
    ($id:literal, $part:ident, $syntax:ident, $attr:expr, $uri:expr; $($ns:ident : $name:literal),*) => {
        Location { id: $id, part: PartKind::$part, path: &[$(($ns, $name)),*], attr: $attr,
            syntax: Syntax::$syntax, scope: Scope::Own, rename: Rule::Unaffected, delete: Rule::Unaffected,
            rules: &OPERATION_RULES[$id - 1], extension: $uri }
    };
}

const X15: &str = "http://schemas.microsoft.com/office/spreadsheetml/2010/11/main";
const WEB_EXTENSIONS: &str = "{F7C9EE02-42E1-4005-9D12-6889AFFD525C}";
const PROTECTED_RANGES: &str = "{FC87AEE6-9EDD-4A0A-B7FB-166176984837}";
const IGNORED_ERRORS: &str = "{01252117-D84E-4E92-8308-4BE1C098FCBB}";

/// Worksheet extension identities in ADR-0018. Empty row lists document
/// extensions whose contents carry no positions.
const EXTENSIONS: &[(&str, &[u8])] = &[
    (URI_CF, &[20]),
    (URI_DV, &[20]),
    (URI_SPARKLINE, &[21]),
    (PROTECTED_RANGES, &[68]),
    (IGNORED_ERRORS, &[69]),
    (WEB_EXTENSIONS, &[70]),
    ("{A8765BA9-456A-4DAB-B4F3-ACF838C121DE}", &[]),
    ("{7E03D99C-DC04-49d9-9315-930204A7B6E9}", &[]),
    ("{B025F937-C7B1-47D3-B67F-A62EFF666E3E}", &[]),
    (SORT_STATE, &[74]),
    (FILTER_COLUMN, &[74]),
    (
        "http://schemas.microsoft.com/office/mac/excel/2008/main",
        &[],
    ),
    ("{64002731-A6B0-56B0-2670-7721B7C09600}", &[]),
];

#[cfg_attr(not(test), expect(dead_code, reason = "structural dispatch is wired in P7a–d"))]
struct OperationRules {
    g1: &'static str,
    copy: &'static str,
    table_rename: &'static str,
}

const OPERATION_RULES: [OperationRules; 74] = [
    OperationRules { g1: "D (row removed)", copy: "copied", table_rename: "–" }, // L1
    OperationRules { g1: "R (column edits)", copy: "copied", table_rename: "–" }, // L2
    OperationRules { g1: "D (cell removed)", copy: "copied", table_rename: "–" }, // L3
    OperationRules { g1: "R (§3.3)", copy: "R (§4.4)", table_rename: "R" }, // L4
    OperationRules { g1: "R; X when an array is split (§3.3.3)", copy: "copied", table_rename: "–" }, // L5
    OperationRules { g1: "R; X when deleted", copy: "copied", table_rename: "–" }, // L6
    OperationRules { g1: "R", copy: "copied", table_rename: "–" }, // L7
    OperationRules { g1: "R (§3.4)", copy: "copied", table_rename: "–" }, // L8
    OperationRules { g1: "R, clamped (§3.5)", copy: "copied", table_rename: "–" }, // L9
    OperationRules { g1: "R (§3.5)", copy: "copied", table_rename: "–" }, // L10
    OperationRules { g1: "R, clamped", copy: "copied", table_rename: "–" }, // L11
    OperationRules { g1: "D (§3.5)", copy: "copied", table_rename: "–" }, // L12
    OperationRules { g1: "D", copy: "copied", table_rename: "–" }, // L13
    OperationRules { g1: "R; X when its target is deleted (§3.5)", copy: "kept (§4.4)", table_rename: "–" }, // L14
    OperationRules { g1: "R; X header row or a filtered column deleted (§3.5)", copy: "copied", table_rename: "–" }, // L15
    OperationRules { g1: "D (rule removed when empty)", copy: "copied", table_rename: "–" }, // L16
    OperationRules { g1: "R (§3.3.4)", copy: "R", table_rename: "R" }, // L17
    OperationRules { g1: "D", copy: "copied", table_rename: "–" }, // L18
    OperationRules { g1: "R (§3.3.4)", copy: "R", table_rename: "R" }, // L19
    OperationRules { g1: "as L16–L19", copy: "R", table_rename: "R" }, // L20
    OperationRules { g1: "R; sparkline removed with its host; X when its data range is wholly deleted (§3.5)", copy: "R (retargeted, Q3)", table_rename: "R" }, // L21
    OperationRules { g1: "D; `@min`/`@max` by the range rule on the other axis, pinned at the edge (§3.2)", copy: "copied", table_rename: "–" }, // L22
    OperationRules { g1: "D", copy: "copied", table_rename: "–" }, // L23
    OperationRules { g1: "R; X when it would become empty", copy: "copied", table_rename: "–" }, // L24
    OperationRules { g1: "as there", copy: "copied", table_rename: "–" }, // L25
    OperationRules { g1: "D", copy: "copied", table_rename: "–" }, // L26
    OperationRules { g1: "XX", copy: "XX", table_rename: "–" }, // L27
    OperationRules { g1: "XX", copy: "XX", table_rename: "–" }, // L28
    OperationRules { g1: "XX", copy: "XX", table_rename: "–" }, // L29
    OperationRules { g1: "XX", copy: "XX", table_rename: "–" }, // L30
    OperationRules { g1: "XX; X inside a registry element", copy: "XX", table_rename: "X inside a registry element" }, // L31
    OperationRules { g1: "R (§3.3.5)", copy: "R + local names duplicated (§4.4)", table_rename: "R" }, // L32
    OperationRules { g1: "–", copy: "N (ADR-0009 §4)", table_rename: "–" }, // L33
    OperationRules { g1: "part removed (ADR-0003 §4.3)", copy: "part removed", table_rename: "–" }, // L34
    OperationRules { g1: "R / X (§3.6.1)", copy: "N (§4.3)", table_rename: "–" }, // L35
    OperationRules { g1: "R (§3.6.1)", copy: "copied", table_rename: "column name: R" }, // L36
    OperationRules { g1: "R", copy: "R", table_rename: "R" }, // L37
    OperationRules { g1: "–", copy: "N", table_rename: "R" }, // L38
    OperationRules { g1: "D (note removed with its VML shape and thread)", copy: "copied", table_rename: "–" }, // L39
    OperationRules { g1: "R with L39", copy: "copied", table_rename: "–" }, // L40
    OperationRules { g1: "R (§3.6.3)", copy: "copied", table_rename: "–" }, // L41
    OperationRules { g1: "X", copy: "XX (controls)", table_rename: "X" }, // L42
    OperationRules { g1: "D with L39", copy: "N (new GUIDs)", table_rename: "–" }, // L43
    OperationRules { g1: "R / X (§3.6.3)", copy: "copied", table_rename: "–" }, // L44
    OperationRules { g1: "X when they would change", copy: "XX", table_rename: "–" }, // L45
    OperationRules { g1: "R", copy: "R", table_rename: "R" }, // L46
    OperationRules { g1: "R; X when a series range is wholly deleted (§3.6.4)", copy: "R (§4.4)", table_rename: "R" }, // L47
    OperationRules { g1: "X", copy: "XX", table_rename: "X" }, // L48
    OperationRules { g1: "K (pivot rules apply to the pivot table)", copy: "XX", table_rename: "–" }, // L49
    OperationRules { g1: "R when moved; X when expanded or shrunk (§3.6.5)", copy: "–", table_rename: "–" }, // L50
    OperationRules { g1: "–", copy: "–", table_rename: "R (table rename)" }, // L51
    OperationRules { g1: "X if the edit crosses or moves it (§3.6.5)", copy: "XX", table_rename: "–" }, // L52
    OperationRules { g1: "X", copy: "–", table_rename: "–" }, // L53
    OperationRules { g1: "R; X when deleted", copy: "XX", table_rename: "–" }, // L54
    OperationRules { g1: "R", copy: "–", table_rename: "R" }, // L55
    OperationRules { g1: "X", copy: "XX", table_rename: "X" }, // L56
    OperationRules { g1: "X when its table's `ref` changes", copy: "XX", table_rename: "X (rename)" }, // L57
    OperationRules { g1: "XX (`SharedWorkbook`)", copy: "XX", table_rename: "XX" }, // L58
    OperationRules { g1: "XX (`BinaryReferencesPossible`, ADR-0009 §6)", copy: "XX", table_rename: "XX" }, // L59
    OperationRules { g1: "K", copy: "–", table_rename: "–" }, // L60
    OperationRules { g1: "– (move with the cell)", copy: "copied", table_rename: "–" }, // L61
    OperationRules { g1: "R when it names the edited sheet's area *(u: it has no sheet; assumed the first sheet)*; X when its area is changed in any other way", copy: "–", table_rename: "–" }, // L62
    OperationRules { g1: "X when it names the edited sheet", copy: "X", table_rename: "X" }, // L63
    OperationRules { g1: "–", copy: "–", table_rename: "X (table rename; review m13)" }, // L64
    OperationRules { g1: "XX when the edited sheet has any", copy: "XX", table_rename: "–" }, // L65
    OperationRules { g1: "–", copy: "removed on the copy (§4.2)", table_rename: "–" }, // L66
    OperationRules { g1: "refuse when locked (Q13)", copy: "refuse on `lockStructure` (Q13)", table_rename: "–" }, // L67
    OperationRules { g1: "as L24", copy: "copied", table_rename: "–" }, // L68
    OperationRules { g1: "as L23", copy: "copied", table_rename: "–" }, // L69
    OperationRules { g1: "X", copy: "XX", table_rename: "X" }, // L70
    OperationRules { g1: "R through `set_relationship_target` in the operation's `RegistryPlan`; X when the target is wholly deleted (as L14)", copy: "kept", table_rename: "–" }, // L71
    OperationRules { g1: "– (positions are L50/L53/L28)", copy: "–", table_rename: "–" }, // L72
    OperationRules { g1: "K", copy: "N (ADR-0009 §11)", table_rename: "–" }, // L73
    OperationRules { g1: "X while it holds a `ref`", copy: "copied", table_rename: "–" }, // L74
];

pub(crate) const LOCATIONS: &[Location] = &[
    position!(1, Worksheet, Index, "r"; "worksheet", "sheetData", "row"),
    position!(2, Worksheet, ColumnSpans, "spans"; "worksheet", "sheetData", "row"),
    position!(8, Worksheet, Index, "min"; "worksheet", "cols", "col"),
    position!(8, Worksheet, Index, "max"; "worksheet", "cols", "col"),
    position!(10, Worksheet, Count, "xSplit"; "worksheet", "sheetViews", "sheetView", "pane"),
    position!(10, Worksheet, Count, "ySplit"; "worksheet", "sheetViews", "sheetView", "pane"),
    position!(25, Worksheet, Count, "xSplit"; "worksheet", "customSheetViews", "customSheetView", "pane"),
    position!(25, Worksheet, Count, "ySplit"; "worksheet", "customSheetViews", "customSheetView", "pane"),
    position!(22, Worksheet, Index, "id"; "worksheet", "rowBreaks", "brk"),
    position!(22, Worksheet, Index, "min"; "worksheet", "rowBreaks", "brk"),
    position!(22, Worksheet, Index, "max"; "worksheet", "rowBreaks", "brk"),
    position!(22, Worksheet, Count, "count"; "worksheet", "rowBreaks"),
    position!(22, Worksheet, Count, "manualBreakCount"; "worksheet", "rowBreaks"),
    position!(22, Worksheet, Index, "id"; "worksheet", "colBreaks", "brk"),
    position!(22, Worksheet, Index, "min"; "worksheet", "colBreaks", "brk"),
    position!(22, Worksheet, Index, "max"; "worksheet", "colBreaks", "brk"),
    position!(22, Worksheet, Count, "count"; "worksheet", "colBreaks"),
    position!(22, Worksheet, Count, "manualBreakCount"; "worksheet", "colBreaks"),
    position!(25, Worksheet, Index, "id"; "worksheet", "customSheetViews", "customSheetView", "rowBreaks", "brk"),
    position!(25, Worksheet, Index, "min"; "worksheet", "customSheetViews", "customSheetView", "rowBreaks", "brk"),
    position!(25, Worksheet, Index, "max"; "worksheet", "customSheetViews", "customSheetView", "rowBreaks", "brk"),
    position!(25, Worksheet, Count, "count"; "worksheet", "customSheetViews", "customSheetView", "rowBreaks"),
    position!(25, Worksheet, Count, "manualBreakCount"; "worksheet", "customSheetViews", "customSheetView", "rowBreaks"),
    position!(25, Worksheet, Index, "id"; "worksheet", "customSheetViews", "customSheetView", "colBreaks", "brk"),
    position!(25, Worksheet, Index, "min"; "worksheet", "customSheetViews", "customSheetView", "colBreaks", "brk"),
    position!(25, Worksheet, Index, "max"; "worksheet", "customSheetViews", "customSheetView", "colBreaks", "brk"),
    position!(25, Worksheet, Count, "count"; "worksheet", "customSheetViews", "customSheetView", "colBreaks"),
    position!(25, Worksheet, Count, "manualBreakCount"; "worksheet", "customSheetViews", "customSheetView", "colBreaks"),
    position!(15, Worksheet, ColumnOffset, "colId"; "worksheet", "autoFilter", "filterColumn"),
    position!(25, Worksheet, ColumnOffset, "colId"; "worksheet", "customSheetViews", "customSheetView", "autoFilter", "filterColumn"),
    position!(36, Table, ColumnOffset, "colId"; "table", "autoFilter", "filterColumn"),
    site!(31, Worksheet, Element, None, None; SML:"worksheet", MC:"AlternateContent"),
    site!(31, Worksheet, Element, None, None; SML:"worksheet", SML:"sheetData", MC:"AlternateContent"),
    site!(31, Worksheet, Element, None, None; SML:"worksheet", SML:"sheetData", SML:"row", MC:"AlternateContent"),
    position!(33, Workbook, Index, "localSheetId"; "workbook", "definedNames", "definedName"),
    position!(33, Workbook, Index, "activeTab"; "workbook", "bookViews", "workbookView"),
    position!(33, Workbook, Index, "firstSheet"; "workbook", "bookViews", "workbookView"),
    position!(34, CalcChain, Index, "i"; "calcChain", "c"),
    site!(36, Table, Element, None, None; SML:"table", SML:"tableColumns", SML:"tableColumn"),
    position!(38, Table, RawName, "name"; "table"),
    position!(38, Table, RawName, "displayName"; "table"),
    site!(40, Vml, Index, None, None; VML:"ClientData", VML:"Row"),
    site!(40, Vml, Index, None, None; VML:"ClientData", VML:"Column"),
    site!(41, Vml, Marker, None, None; VML:"ClientData", VML:"Anchor"),
    site!(43, ThreadedComments, Cell, Some("ref"), None; TC:"ThreadedComments", TC:"threadedComment"),
    site!(44, Drawing, Marker, None, None; XDR:"wsDr", XDR:"twoCellAnchor", XDR:"from"),
    site!(44, Drawing, Marker, None, None; XDR:"wsDr", XDR:"twoCellAnchor", XDR:"to"),
    site!(44, Drawing, Marker, None, None; XDR:"wsDr", XDR:"oneCellAnchor", XDR:"from"),
    site!(45, Drawing, Marker, None, None; XDR:"wsDr", MC:"AlternateContent", MC:"Choice", XDR:"twoCellAnchor"),
    site!(45, Drawing, Marker, None, None; XDR:"wsDr", MC:"AlternateContent", MC:"Choice", XDR:"oneCellAnchor"),
    site!(45, Drawing, Marker, None, None; XDR:"wsDr", MC:"AlternateContent", MC:"Choice", XDR:"absoluteAnchor"),
    site!(45, Drawing, Marker, None, None; XDR:"wsDr", MC:"AlternateContent", MC:"Fallback", XDR:"twoCellAnchor"),
    site!(45, Drawing, Marker, None, None; XDR:"wsDr", MC:"AlternateContent", MC:"Fallback", XDR:"oneCellAnchor"),
    site!(45, Drawing, Marker, None, None; XDR:"wsDr", MC:"AlternateContent", MC:"Fallback", XDR:"absoluteAnchor"),
    position!(51, PivotCache, RawName, "name"; "pivotCacheDefinition", "cacheSource", "worksheetSource"),
    site!(57, QueryTable, Element, None, None; SML:"queryTable"),
    site!(58, Revisions, Element, None, None; ),
    site!(59, ActiveX, Element, Some("LinkedCell"), None; ),
    site!(59, ActiveX, Element, Some("ListFillRange"), None; ),
    Location { scope: Scope::Any, ..site!(60, ExternalLink, Element, None, None; SML:"externalLink") },
    site!(61, Metadata, Index, Some("cm"), None; SML:"worksheet", SML:"sheetData", SML:"row", SML:"c"),
    site!(61, RichData, Index, Some("cm"), None; SML:"worksheet", SML:"sheetData", SML:"row", SML:"c"),
    site!(61, Metadata, Index, Some("vm"), None; SML:"worksheet", SML:"sheetData", SML:"row", SML:"c"),
    site!(61, RichData, Index, Some("vm"), None; SML:"worksheet", SML:"sheetData", SML:"row", SML:"c"),
    position!(63, Workbook, RawName, "sourceObject"; "workbook", "webPublishObjects", "webPublishObject"),
    site!(64, Connections, RawName, Some("sourceName"), None; X15:"rangePr"),
    position!(64, Connections, RawName, "name"; "connections", "connection"),
    position!(66, Worksheet, RawName, "codeName"; "worksheet", "sheetPr"),
    position!(67, Worksheet, Flags, "insertRows"; "worksheet", "sheetProtection"),
    position!(67, Worksheet, Flags, "insertColumns"; "worksheet", "sheetProtection"),
    position!(67, Worksheet, Flags, "deleteRows"; "worksheet", "sheetProtection"),
    position!(67, Worksheet, Flags, "deleteColumns"; "worksheet", "sheetProtection"),
    position!(67, Workbook, Flags, "lockStructure"; "workbook", "workbookProtection"),
    site!(74, Worksheet, Range, Some("ref"), Some(SORT_STATE); SML:"worksheet", SML:"sortState", SML:"extLst", SML:"ext", X14:"sortCondition"),
    site!(74, Worksheet, Range, Some("ref"), Some(SORT_STATE); SML:"worksheet", SML:"sortState", SML:"extLst", SML:"ext", RICH_DATA:"richSortCondition"),
    site!(74, Worksheet, Range, Some("ref"), Some(SORT_STATE); SML:"worksheet", SML:"autoFilter", SML:"sortState", SML:"extLst", SML:"ext", X14:"sortCondition"),
    site!(74, Worksheet, Range, Some("ref"), Some(SORT_STATE); SML:"worksheet", SML:"autoFilter", SML:"sortState", SML:"extLst", SML:"ext", RICH_DATA:"richSortCondition"),
    site!(74, Worksheet, Range, Some("ref"), Some(FILTER_COLUMN); SML:"worksheet", SML:"autoFilter", SML:"filterColumn", SML:"extLst", SML:"ext", RICH_DATA:"filterColumn"),
    site!(74, Worksheet, Range, Some("ref"), Some(SORT_STATE); SML:"worksheet", SML:"customSheetViews", SML:"customSheetView", SML:"autoFilter", SML:"sortState", SML:"extLst", SML:"ext", X14:"sortCondition"),
    site!(74, Worksheet, Range, Some("ref"), Some(SORT_STATE); SML:"worksheet", SML:"customSheetViews", SML:"customSheetView", SML:"autoFilter", SML:"sortState", SML:"extLst", SML:"ext", RICH_DATA:"richSortCondition"),
    site!(74, Worksheet, Range, Some("ref"), Some(FILTER_COLUMN); SML:"worksheet", SML:"customSheetViews", SML:"customSheetView", SML:"autoFilter", SML:"filterColumn", SML:"extLst", SML:"ext", RICH_DATA:"filterColumn"),
    site!(74, Table, Range, Some("ref"), Some(SORT_STATE); SML:"table", SML:"sortState", SML:"extLst", SML:"ext", X14:"sortCondition"),
    site!(74, Table, Range, Some("ref"), Some(SORT_STATE); SML:"table", SML:"sortState", SML:"extLst", SML:"ext", RICH_DATA:"richSortCondition"),
    site!(74, Table, Range, Some("ref"), Some(SORT_STATE); SML:"table", SML:"autoFilter", SML:"sortState", SML:"extLst", SML:"ext", X14:"sortCondition"),
    site!(74, Table, Range, Some("ref"), Some(SORT_STATE); SML:"table", SML:"autoFilter", SML:"sortState", SML:"extLst", SML:"ext", RICH_DATA:"richSortCondition"),
    site!(74, Table, Range, Some("ref"), Some(FILTER_COLUMN); SML:"table", SML:"autoFilter", SML:"filterColumn", SML:"extLst", SML:"ext", RICH_DATA:"filterColumn"),
    site!(30, Worksheet, Marker, None, None; SML:"worksheet", SML:"oleObjects", SML:"oleObject", SML:"objectPr", SML:"anchor"),
    site!(30, Worksheet, Marker, None, None; SML:"worksheet", SML:"controls", SML:"control", SML:"controlPr", SML:"anchor"),
    Location { scope: Scope::Any, ..site!(47, Chart, Formula, None, None; C15_NS:"sqref") },
    Location { scope: Scope::Any, ..site!(47, Chart, Formula, None, None; C15_NS:"fullRef") },
    Location { scope: Scope::Any, ..site!(47, Chart, Formula, None, None; C15_NS:"formulaRef") },
    Location { scope: Scope::Any, ..site!(47, Chart, Formula, None, None; C15_NS:"datalabelsRange") },
    Location { scope: Scope::Any, ..site!(47, Chart, Formula, None, None; C15_NS:"filteredSeriesTitle") },
    Location { scope: Scope::Any, ..site!(47, Chart, Formula, None, None; C15_NS:"filteredCategoryTitle") },
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
    sqref!(21, URI_SPARKLINE; SML:"worksheet", SML:"extLst", SML:"ext", X14:"sparklineGroups", X14:"sparklineGroup", X14:"sparklines", X14:"sparkline", XM:"sqref"),
    sqref!(68, PROTECTED_RANGES; SML:"worksheet", SML:"extLst", SML:"ext", X14:"protectedRanges", X14:"protectedRange", XM:"sqref"),
    sqref!(69, IGNORED_ERRORS; SML:"worksheet", SML:"extLst", SML:"ext", X14:"ignoredErrors", X14:"ignoredError", XM:"sqref"),
    location!(21, Worksheet, Formula, Rewrite, None, Some(URI_SPARKLINE); SML:"worksheet", SML:"extLst", SML:"ext", X14:"sparklineGroups", X14:"sparklineGroup", X14:"sparklines", X14:"sparkline", XM:"f"),
    location!(21, Worksheet, Formula, Rewrite, None, Some(URI_SPARKLINE); SML:"worksheet", SML:"extLst", SML:"ext", X14:"sparklineGroups", X14:"sparklineGroup", XM:"f"),
    location!(30, Worksheet, Formula, Rewrite, Some("linkedCell"), None; SML:"worksheet", SML:"controls", SML:"control", SML:"controlPr"),
    location!(30, Worksheet, Formula, Rewrite, Some("listFillRange"), None; SML:"worksheet", SML:"controls", SML:"control", SML:"controlPr"),
    location!(30, Worksheet, Formula, Rewrite, Some("linkedCell"), None; SML:"worksheet", MC:"AlternateContent", MC:"Choice", SML:"controls", SML:"control", SML:"controlPr"),
    location!(30, Worksheet, Formula, Rewrite, Some("listFillRange"), None; SML:"worksheet", MC:"AlternateContent", MC:"Choice", SML:"controls", SML:"control", SML:"controlPr"),
    location!(30, Worksheet, Formula, Rewrite, Some("linkedCell"), None; SML:"worksheet", MC:"AlternateContent", MC:"Fallback", SML:"controls", SML:"control", SML:"controlPr"),
    location!(30, Worksheet, Formula, Rewrite, Some("listFillRange"), None; SML:"worksheet", MC:"AlternateContent", MC:"Fallback", SML:"controls", SML:"control", SML:"controlPr"),
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
    Location {
        rename: Rule::Refuse,
        ..location!(48, ChartEx, Formula, Refuse, None, None; CX:"f")
    },
    location!(49, Chart, PivotSource, Refuse, None, None; CHART_NS:"pivotSource", CHART_NS:"name"),
    location!(55, Connections, Formula, Rewrite, Some("cell"), None; SML:"connections", SML:"connection", SML:"parameters", SML:"parameter"),
    location!(56, Control, Formula, Rewrite, Some("fmlaLink"), None; X14:"formControlPr"),
    location!(56, Control, Formula, Rewrite, Some("fmlaRange"), None; X14:"formControlPr"),
    location!(56, Control, Formula, Rewrite, Some("fmlaTxbx"), None; X14:"formControlPr"),
    location!(56, Control, Formula, Rewrite, Some("fmlaGroup"), None; X14:"formControlPr"),
    location!(70, Worksheet, Formula, Rewrite, None, Some(WEB_EXTENSIONS); SML:"worksheet", SML:"extLst", SML:"ext", X15:"webExtensions", X15:"webExtension", XM:"f"),
    location!(71, Relationships, Location, Keep, Some("Target"), None; RELS:"Relationships", RELS:"Relationship"),
    location!(72, PivotCache, RawName, Refuse, Some("sheet"), None; SML:"pivotCacheDefinition", SML:"cacheSource", SML:"worksheetSource"),
    Location {
        rename: Rule::Refuse,
        ..location!(72, PivotCache, RawName, Refuse, Some("sheet"), None; SML:"pivotCacheDefinition", SML:"cacheSource", SML:"consolidation", SML:"rangeSets", SML:"rangeSet")
    },
    Location {
        rename: Rule::Refuse,
        ..location!(72, Worksheet, RawName, Refuse, Some("sheet"), None; SML:"worksheet", SML:"dataConsolidate", SML:"dataRefs", SML:"dataRef")
    },
    location!(73, AppProperties, Location, Rewrite, None, None; AP:"Properties", AP:"TitlesOfParts", VT:"vector", VT:"lpstr"),
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
        "application/vnd.ms-office.chartex+xml" => PartKind::ChartEx,
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

    const ADR: &str = include_str!("../../../docs/adr/0018-xlsx-phase7-structural-edits.md");

    #[test]
    fn extension_uris_and_rows_match_the_adr() {
        let table = ADR.split_once("| URI | Element | Rows / rule | Corpus |").unwrap().1;
        let expected: Vec<_> = table.lines().skip(2).take_while(|line| line.starts_with('|'))
            .flat_map(|line| {
                let columns: Vec<_> = line.split('|').map(str::trim).collect();
                let ids: Vec<u8> = columns[3].strip_prefix('L')
                    .and_then(|value| value.split(|c: char| !c.is_ascii_digit()).next())
                    .and_then(|id| id.parse().ok()).into_iter().collect();
                columns[1].split('`').skip(1).step_by(2)
                    .filter(|uri| uri.starts_with('{') || uri.starts_with("http://"))
                    .map(move |uri| (uri.to_ascii_lowercase(), ids.clone()))
            }).collect();
        let actual: Vec<_> = EXTENSIONS.iter().map(|(uri, ids)| (uri.to_ascii_lowercase(), ids.to_vec())).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn each_qualified_location_and_its_rules_match_the_adr() {
        let table = ADR.split_once("<!-- BEGIN QUALIFIED LOCATIONS -->\n").unwrap().1;
        let expected: Vec<_> = table.lines().skip(2).take_while(|line| line.starts_with('|')).collect();
        let namespace = |ns| match ns {
            SML => "sml",
            X14 => "x14",
            X15 => "x15",
            XM => "xm",
            MC => "mc",
            TC => "tc",
            RICH_DATA => "xlrd2",
            VML => "x",
            XDR => "xdr",
            CDR => "cdr",
            CHART_NS => "c",
            C15_NS => "c15",
            CX => "cx",
            RELS => "rels",
            AP => "ap",
            VT => "vt",
            _ => panic!("unknown namespace {ns}"),
        };
        let mut rows: Vec<_> = LOCATIONS.iter().collect();
        rows.sort_by_key(|row| row.id);
        let actual: Vec<_> = rows.into_iter().map(|row| {
            let path = row.path.iter().map(|&(ns, name)| format!("{}:{name}", namespace(ns)))
                .collect::<Vec<_>>().join("/");
            format!("| L{} | {:?} | {:?} | {} | {} | {:?} | {} | {} | {} | {:?} | {:?} | {} |",
                row.id, row.part, row.scope, path, row.attr.unwrap_or(""), row.syntax,
                row.rules.g1, row.rules.copy, row.rules.table_rename, row.rename, row.delete,
                row.extension.unwrap_or("").to_ascii_uppercase())
        }).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn operation_columns_match_the_location_summary() {
        let summary: Vec<_> = ADR.lines().filter(|line| line.starts_with("| L")
            && line.split('|').count() == 12).collect();
        assert_eq!(summary.len(), OPERATION_RULES.len());
        for (i, (line, rules)) in summary.into_iter().zip(&OPERATION_RULES).enumerate() {
            let fields: Vec<_> = line.split('|').map(str::trim).collect();
            assert_eq!(fields[1], format!("L{}", i + 1));
            assert_eq!(&fields[5..8], [rules.g1, rules.copy, rules.table_rename], "{}", fields[1]);
            assert!(LOCATIONS.iter().any(|row| usize::from(row.id) == i + 1), "{}", fields[1]);
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
