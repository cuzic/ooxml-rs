use super::*;
use crate::formula::precedents::Env;
use crate::formula::structural::{rewrite_range, rewrite_sqref};
use crate::locations::{self, PartKind, Syntax};
use crate::x14::{NS_X14, NS_XM};
use ooxml_core::{Content, LensMut, SimpleType};

const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

#[derive(Default)]
pub(super) struct Features {
    rewrites: Vec<(Vec<usize>, QName, Update)>,
    removals: Vec<(Vec<usize>, QName, Vec<usize>)>,
    splits: Vec<ColumnSplit>,
}

struct ColumnSplit {
    path: Vec<usize>,
    index: usize,
    attrs: Vec<(&'static str, String)>,
}

enum Update {
    Attr(&'static str, String),
    Text(String),
}

macro_rules! elements {
    ($($key:ident: $ns:expr, $local:literal;)+) => {
        $(enum $key {}
        impl ElementSpec for $key {
            const QNAME: QName = QName::new($ns, $local);
            const SDK_NAME: &'static str = $local;
            const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
            const CONTENT: Content = Content::Text(SimpleType::String);
        })+

        fn update(doc: &mut XmlDocument, path: &[usize], q: QName, edit: &Update) -> Result<()> {
            match q {
                $($key::QNAME => apply_update(doc.lens_mut_at::<$key>(path)?, edit),)+
                _ => unreachable!("validated element"),
            }
        }

        fn drop_items(doc: &mut XmlDocument, path: &[usize], q: QName, indices: &[usize]) -> Result<()> {
            match q {
                $($key::QNAME => remove_indices(&mut doc.lens_mut_at::<$key>(path)?, indices, false),)+
                _ => unreachable!("validated parent"),
            }
            Ok(())
        }

        fn qualified(node: &RawNode) -> Option<QName> {
            $(if node.is($key::QNAME) { return Some($key::QNAME); })+
            None
        }
    };
}

elements! {
    WorksheetRoot: crate::SML_NS, "worksheet";
    Dimension: crate::SML_NS, "dimension";
    SheetView: crate::SML_NS, "sheetView";
    CustomSheetView: crate::SML_NS, "customSheetView";
    Pane: crate::SML_NS, "pane";
    Selection: crate::SML_NS, "selection";
    Col: crate::SML_NS, "col";
    Cols: crate::SML_NS, "cols";
    RowBreaks: crate::SML_NS, "rowBreaks";
    ColBreaks: crate::SML_NS, "colBreaks";
    BreakItem: crate::SML_NS, "brk";
    MergeCells: crate::SML_NS, "mergeCells";
    MergeCell: crate::SML_NS, "mergeCell";
    Hyperlinks: crate::SML_NS, "hyperlinks";
    Hyperlink: crate::SML_NS, "hyperlink";
    AutoFilter: crate::SML_NS, "autoFilter";
    FilterColumn: crate::SML_NS, "filterColumn";
    SortState: crate::SML_NS, "sortState";
    SortCondition: crate::SML_NS, "sortCondition";
    Cf: crate::SML_NS, "conditionalFormatting";
    Formula: crate::SML_NS, "formula";
    Cfvo: crate::SML_NS, "cfvo";
    Dvs: crate::SML_NS, "dataValidations";
    Dv: crate::SML_NS, "dataValidation";
    Formula1: crate::SML_NS, "formula1";
    Formula2: crate::SML_NS, "formula2";
    Errors: crate::SML_NS, "ignoredErrors";
    IgnoredError: crate::SML_NS, "ignoredError";
    ProtectedRange: crate::SML_NS, "protectedRange";
    Watches: crate::SML_NS, "cellWatches";
    Watch: crate::SML_NS, "cellWatch";
    SmartTags: crate::SML_NS, "smartTags";
    SmartTag: crate::SML_NS, "cellSmartTags";
    ExtList: crate::SML_NS, "extLst";
    Ext: crate::SML_NS, "ext";
    X14Cfs: NS_X14, "conditionalFormattings";
    X14Cf: NS_X14, "conditionalFormatting";
    X14Dvs: NS_X14, "dataValidations";
    X14Errors: NS_X14, "ignoredErrors";
    SparkGroups: NS_X14, "sparklineGroups";
    SparkGroup: NS_X14, "sparklineGroup";
    SparkList: NS_X14, "sparklines";
    XmFormula: NS_XM, "f";
    OleSize: crate::SML_NS, "oleSize";
    XmSqref: NS_XM, "sqref";
    TableFormula: crate::SML_NS, "calculatedColumnFormula";
    TotalsFormula: crate::SML_NS, "totalsRowFormula";
}

fn apply_update<E: ElementSpec>(mut lens: LensMut<'_, E>, edit: &Update) -> Result<()> {
    lens.preserve_attribute_spelling();
    match edit {
        Update::Attr(name, value) => lens.set_attr(attr(name), value)?,
        Update::Text(value) => lens.set_text(value)?,
    }
    Ok(())
}

impl Features {
    pub(super) fn is_empty(&self) -> bool {
        self.rewrites.is_empty() && self.removals.is_empty() && self.splits.is_empty()
    }

    pub(super) fn rewrite(&self, doc: &mut XmlDocument) -> Result<()> {
        for (path, name, edit) in &self.rewrites {
            update(doc, path, *name, edit)?;
        }
        Ok(())
    }

    pub(super) fn finish(mut self, doc: &mut XmlDocument) -> Result<()> {
        for split in &mut self.splits {
            for (path, _, indices) in &self.removals {
                if split.path == *path {
                    split.index -= indices.partition_point(|&index| index < split.index);
                }
            }
            for depth in (0..split.path.len()).rev() {
                for (path, _, indices) in &self.removals {
                    if path == &split.path[..depth] {
                        split.path[depth] -=
                            indices.partition_point(|&index| index < split.path[depth]);
                    }
                }
            }
        }
        for (path, name, indices) in self.removals {
            drop_items(doc, &path, name, &indices)?;
        }
        for split in self.splits.into_iter().rev() {
            let mut columns = doc.lens_mut_at::<sml::Columns>(&split.path)?;
            let mut column = columns.insert_at::<sml::Column>(split.index)?;
            for (name, value) in split.attrs {
                column.set_attr(attr(name), &value)?;
            }
        }
        Ok(())
    }
}

struct Planner<'a, 'b> {
    doc: &'a XmlDocument,
    part: &'a str,
    sheet: Option<usize>,
    kind: PartKind,
    ctx: &'a FormulaContext<'a>,
    env: &'a Env,
    report: &'b mut StructuralEditReport,
    edits: Features,
    removed_cf_links: HashSet<String>,
}

#[derive(Clone, Copy, Default)]
struct Site<'a> {
    extension: Option<&'a str>,
    area: Option<&'a str>,
    filter: Option<CellRange>,
    breaks: Option<Axis>,
}

impl Planner<'_, '_> {
    fn own(&self) -> bool {
        self.sheet == Some(self.ctx.edited) && self.kind == PartKind::Worksheet
    }

    fn blocked(&self, what: &str) -> Error {
        Error::StructureBlocked {
            part: self.part.into(),
            place: what.into(),
        }
    }

    fn add(
        &mut self,
        node: &RawNode,
        path: &[usize],
        name: Option<&'static str>,
        before: &str,
        after: String,
    ) -> Result<()> {
        if before == after {
            return Ok(());
        }
        let q = qualified(node).ok_or_else(|| self.blocked(node.name().local()))?;
        let location =
            name.map_or_else(|| q.local.to_owned(), |name| format!("{}@{name}", q.local));
        record(self.report, self.part, location, before, &after);
        let edit = match name {
            Some(name) => Update::Attr(name, after),
            None => Update::Text(after),
        };
        self.edits.rewrites.push((path.to_vec(), q, edit));
        Ok(())
    }

    fn scan<'n>(
        &mut self,
        node: &'n RawNode,
        path: &mut Vec<usize>,
        names: &mut Vec<(String, String)>,
        mut site: Site<'n>,
    ) -> Result<bool> {
        let local = node.name().local();
        let ns = node.name().ns();
        if node.is(tag("sheetData")) {
            return Ok(false);
        }
        names.push((ns.into(), local.into()));
        if node.is(tag("ext")) {
            site.extension = node.attr(attr("uri"));
        }
        if self.own() {
            if ns == MC && local == "AlternateContent"
                || ns == crate::SML_NS
                    && matches!(
                        local,
                        "scenarios"
                            | "dataConsolidate"
                            | "webPublishItems"
                            | "oleObjects"
                            | "controls"
                    )
            {
                return Err(self.blocked(local));
            }
            if local == "autoFilter"
                && let Some(value) = node.attr(attr("ref"))
            {
                site.filter = Some(CellRange::parse(value)?);
            }
            site.breaks = match local {
                "rowBreaks" => Some(Axis::Row),
                "colBreaks" => Some(Axis::Column),
                _ => site.breaks,
            };
        }
        let sqref = node
            .attr(attr("sqref"))
            .map(std::borrow::Cow::Borrowed)
            .or_else(|| {
                node.elements()
                    .find(|child| child.is(XmSqref::QNAME))
                    .map(RawNode::text)
            });
        // A borrowed area is sufficient for attributes. Text-backed x14 areas
        // can be owned, so the recursive walk keeps the Cow alive.
        let mut child_site = site;
        if let Some(sqref) = sqref.as_deref() {
            child_site.area = Some(sqref);
        }

        let mut remove_node = ns == NS_X14
            && local == "cfRule"
            && node
                .attr(attr("id"))
                .is_some_and(|id| self.removed_cf_links.contains(&id.to_ascii_lowercase()));
        let band = self.ctx.band;
        if self.own() && matches!(local, "col" | "brk") && ns == crate::SML_NS {
            remove_node = self.numeric(node, path, site)?;
        }
        if !remove_node
            && self.own()
            && let Some(sqref) = sqref.as_deref()
            && matches!(
                local,
                "conditionalFormatting"
                    | "dataValidation"
                    | "ignoredError"
                    | "protectedRange"
                    | "sparkline"
            )
            && (ns == crate::SML_NS || ns == NS_X14)
            && rewrite_sqref(sqref, band.axis, band.shift)
                .map_err(|e| rewrite_error(e, self.part, local))?
                == Rewrite::Remove
        {
            if local == "protectedRange" {
                return Err(self.blocked(local));
            }
            remove_node = true;
        }
        if !remove_node {
            for (name, before) in [
                ("ref", node.attr(attr("ref"))),
                ("r", node.attr(attr("r"))),
                ("topLeftCell", node.attr(attr("topLeftCell"))),
                ("activeCell", node.attr(attr("activeCell"))),
                ("sqref", node.attr(attr("sqref"))),
                ("location", node.attr(attr("location"))),
                ("val", node.attr(attr("val"))),
                ("xSplit", node.attr(attr("xSplit"))),
                ("ySplit", node.attr(attr("ySplit"))),
                ("colId", node.attr(attr("colId"))),
                ("linkedCell", node.attr(attr("linkedCell"))),
                ("listFillRange", node.attr(attr("listFillRange"))),
                ("textlink", node.attr(attr("textlink"))),
                ("cell", node.attr(attr("cell"))),
                ("fmlaLink", node.attr(attr("fmlaLink"))),
                ("fmlaRange", node.attr(attr("fmlaRange"))),
                ("fmlaTxbx", node.attr(attr("fmlaTxbx"))),
                ("fmlaGroup", node.attr(attr("fmlaGroup"))),
            ] {
                if let Some(before) = before
                    && let Some(row) = locations::find(self.kind, names, Some(name), site.extension)
                {
                    let result = self.value(node, row, Some(name), before, site)?;
                    remove_node |= self.apply_value(node, path, Some(name), before, result)?;
                }
            }
            if let Some(row) = locations::find(self.kind, names, None, site.extension)
                && matches!(row.syntax, Syntax::Formula | Syntax::Sqref)
            {
                let before = node.text();
                let result = self.value(node, row, None, &before, site)?;
                remove_node |= self.apply_value(node, path, None, &before, result)?;
            }
        }
        if !remove_node {
            let mut gone = Vec::new();
            let mut alive = 0;
            let mut manual = 0;
            for (i, child) in node.indexed_elements() {
                path.push(i);
                if self.scan(child, path, names, child_site)? {
                    gone.push(i);
                } else if !(local == "sparklineGroup" && child.is(XmFormula::QNAME)
                    || ns == NS_X14 && local == "conditionalFormatting" && child.is(XmSqref::QNAME))
                {
                    alive += 1;
                    manual += usize::from(matches!(child.attr(attr("man")), Some("1" | "true")));
                }
                path.pop();
            }
            if !gone.is_empty() {
                remove_node = alive == 0
                    && (ns == NS_X14 && local == "conditionalFormatting"
                        || matches!(
                            local,
                            "cols"
                                | "mergeCells"
                                | "hyperlinks"
                                | "dataValidations"
                                | "ignoredErrors"
                                | "conditionalFormattings"
                                | "sparklines"
                                | "sparklineGroup"
                                | "sparklineGroups"
                                | "ext"
                                | "extLst"
                                | "rowBreaks"
                                | "colBreaks"
                                | "cellWatches"
                                | "smartTags"
                        ));
                if !remove_node {
                    let q = qualified(node).ok_or_else(|| self.blocked(local))?;
                    self.edits.removals.push((path.clone(), q, gone));
                    if matches!(
                        local,
                        "dataValidations" | "mergeCells" | "rowBreaks" | "colBreaks"
                    ) {
                        if let Some(before) = node.attr(attr("count")) {
                            self.add(node, path, Some("count"), before, alive.to_string())?;
                        }
                        if let Some(before) = node.attr(attr("manualBreakCount")) {
                            self.add(
                                node,
                                path,
                                Some("manualBreakCount"),
                                before,
                                manual.to_string(),
                            )?;
                        }
                    }
                }
            }
        }
        names.pop();
        if remove_node {
            removed(
                self.report,
                self.part,
                format!(
                    "{local} {}",
                    node.attr(attr("ref"))
                        .or_else(|| node.attr(attr("sqref")))
                        .unwrap_or_default()
                ),
            );
        }
        Ok(remove_node)
    }

    fn apply_value(
        &mut self,
        node: &RawNode,
        path: &[usize],
        name: Option<&'static str>,
        before: &str,
        value: Rewrite,
    ) -> Result<bool> {
        match value {
            Rewrite::Keep => Ok(false),
            Rewrite::Remove => Ok(true),
            Rewrite::New(after) => {
                self.add(node, path, name, before, after)?;
                Ok(false)
            }
        }
    }

    fn value(
        &self,
        node: &RawNode,
        row: &locations::Location,
        name: Option<&str>,
        before: &str,
        site: Site<'_>,
    ) -> Result<Rewrite> {
        let local = node.name().local();
        let band = self.ctx.band;
        let map = |e| rewrite_error(e, self.part, local);
        if row.syntax == Syntax::Formula {
            if before.is_empty()
                || name == Some("val") && node.attr(attr("type")) != Some("formula")
            {
                return Ok(Rewrite::Keep);
            }
            let own = self.sheet.filter(|_| row.id != 21);
            let text = if self.own() && matches!(row.id, 17 | 19 | 20) {
                self.reanchor(before, site.area)?
            } else {
                before.to_owned()
            };
            let after = self
                .ctx
                .rewrite(&text, own, None, self.part, local)?
                .unwrap_or(text);
            if after == before {
                return Ok(Rewrite::Keep);
            }
            if matches!(row.id, 30 | 42 | 46 | 47 | 48 | 55 | 56 | 70) {
                return Err(self.blocked(local));
            }
            if row.id == 21 && after.contains("#REF!") && !before.contains("#REF!") {
                return Err(Error::ReferenceDeleted {
                    part: self.part.into(),
                    location: local.into(),
                });
            }
            return Ok(Rewrite::New(after));
        }
        if row.syntax == Syntax::Location && row.id == 14 {
            return self.hyperlink(before);
        }
        if !self.own() {
            return Ok(Rewrite::Keep);
        }
        match row.syntax {
            Syntax::Range => {
                if row.id == 74 {
                    return Err(self.blocked(local));
                }
                let outcome = rewrite_range(before, band.axis, band.shift).map_err(map)?;
                if matches!(row.id, 12 | 13) {
                    let range = CellRange::parse(before)?;
                    if matches!(band.shift, Shift::Insert { .. }) {
                        band.after(band.index(range.last), local)?;
                    }
                    if row.id == 12
                        && let Rewrite::New(ref value) = outcome
                    {
                        let new = CellRange::parse(value)?;
                        if new.first == new.last {
                            return Ok(Rewrite::Remove);
                        }
                        if band.cell(range.first)?.is_none() {
                            let old_anchor = band.with_index(
                                new.first,
                                band.shift.before(band.index(new.first)).expect("survivor"),
                            );
                            if has_value(self.doc, old_anchor) {
                                return Err(self.blocked("merge anchor"));
                            }
                        }
                    }
                }
                if local == "autoFilter"
                    && let Some(range) = site.filter
                    && matches!(band.axis, Axis::Row)
                    && band.cell(range.first)?.is_none()
                {
                    return Err(self.blocked("autofilter header"));
                }
                if local == "autoFilter"
                    && outcome == Rewrite::Remove
                    && node.elements().any(|child| child.is(tag("filterColumn")))
                {
                    return Err(self.blocked("filter column"));
                }
                if local == "dimension" && outcome == Rewrite::Remove {
                    return Ok(Rewrite::New("A1".into()));
                }
                Ok(outcome)
            }
            Syntax::Cell => {
                let old = CellIndex::parse(before)?;
                if matches!(
                    local,
                    "sheetView" | "customSheetView" | "pane" | "selection"
                ) {
                    let new = self.clamp(old);
                    return Ok(if new == old {
                        Rewrite::Keep
                    } else {
                        Rewrite::New(new.to_string())
                    });
                }
                Ok(match band.cell(old)? {
                    Some(new) if new != old => Rewrite::New(new.to_string()),
                    Some(_) => Rewrite::Keep,
                    None => Rewrite::Remove,
                })
            }
            Syntax::Sqref => {
                let result = rewrite_sqref(before, band.axis, band.shift).map_err(map)?;
                if local == "selection" && result == Rewrite::Remove {
                    let cell = CellIndex::parse(node.attr(attr("activeCell")).unwrap_or("A1"))?;
                    return Ok(Rewrite::New(self.clamp(cell).to_string()));
                }
                if local == "selection"
                    && let Rewrite::New(text) = &result
                {
                    let normalized = text
                        .split_ascii_whitespace()
                        .map(|item| match item.split_once(':') {
                            Some((first, last)) if first == last => first,
                            _ => item,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    return Ok(Rewrite::New(normalized));
                }
                Ok(result)
            }
            Syntax::Count
                if local == "pane"
                    && matches!(node.attr(attr("state")), Some("frozen" | "frozenSplit")) =>
            {
                if !matches!(
                    (band.axis, name),
                    (Axis::Row, Some("ySplit")) | (Axis::Column, Some("xSplit"))
                ) {
                    return Ok(Rewrite::Keep);
                }
                let old = before
                    .parse::<u32>()
                    .or_else(|_| before.strip_suffix(".0").unwrap_or(before).parse::<u32>())
                    .map_err(|_| self.blocked("frozen pane"))?;
                let new = match band.shift {
                    Shift::Insert { .. } if band.at < old => (old + band.count).min(band.limit - 1),
                    Shift::Delete { .. } if band.at < old => old - band.count.min(old - band.at),
                    _ => old,
                };
                Ok(if new == old {
                    Rewrite::Keep
                } else {
                    Rewrite::New(new.to_string())
                })
            }
            Syntax::ColumnOffset
                if matches!(band.axis, Axis::Column)
                    && let Some(filter) = site.filter =>
            {
                let old = before
                    .parse::<u32>()
                    .map_err(|_| self.blocked("filter column ID"))?;
                let column = filter
                    .first
                    .col
                    .checked_add(old)
                    .filter(|&column| column <= filter.last.col)
                    .ok_or_else(|| self.blocked("filter column ID"))?;
                let new = band
                    .after(column, "filter column")?
                    .ok_or_else(|| self.blocked("filter column"))?;
                let first = numeric_shift(band, filter.first.col, filter.last.col, || {
                    format!("autoFilter {filter}")
                })?
                    .expect("surviving filter")
                    .0;
                Ok(if new - first == old {
                    Rewrite::Keep
                } else {
                    Rewrite::New((new - first).to_string())
                })
            }
            _ => Ok(Rewrite::Keep),
        }
    }

    fn clamp(&self, old: CellIndex) -> CellIndex {
        let band = self.ctx.band;
        let coord = match band.shift.after(band.index(old), band.limit) {
            Ok(Some(coord)) => coord,
            Ok(None) => band.at.min(band.limit - 1),
            Err(_) => band.limit - 1,
        };
        band.with_index(old, coord)
    }

    fn hyperlink(&self, before: &str) -> Result<Rewrite> {
        let (hash, target) = before
            .strip_prefix('#')
            .map_or(("", before), |target| ("#", target));
        if !lex(target).is_ok_and(
            |tokens| matches!(tokens.as_slice(), [token] if matches!(token.kind, TokKind::Ref(_))),
        ) {
            return Ok(Rewrite::Keep);
        }
        match self
            .ctx
            .rewrite(target, self.sheet, None, self.part, "hyperlink target")?
        {
            Some(after) if after.ends_with("#REF!") => Err(Error::ReferenceDeleted {
                part: self.part.into(),
                location: format!("hyperlink {before}"),
            }),
            Some(after) => Ok(Rewrite::New(format!("{hash}{after}"))),
            None => Ok(Rewrite::Keep),
        }
    }

    fn reanchor(&self, before: &str, sqref: Option<&str>) -> Result<String> {
        let Some(sqref) = sqref else {
            return Ok(before.into());
        };
        let mut ranges = sqref.split_ascii_whitespace().map(CellRange::parse);
        let Some(first) = ranges.next().transpose()? else {
            return Ok(before.into());
        };
        let others = ranges.collect::<Result<Vec<_>>>()?;
        let band = self.ctx.band;
        let anchor = band.index(first.first);
        let bbox = others
            .iter()
            .map(|range| band.index(range.first))
            .fold(anchor, u32::min);
        let deleted = |index| {
            matches!(band.shift, Shift::Delete { .. })
                && (band.at..band.at + band.count).contains(&index)
        };
        if !others.is_empty() && (deleted(anchor) || deleted(bbox))
            && lex(before).is_ok_and(|tokens| tokens.iter().any(|token| matches!(&token.kind, TokKind::Ref(reference) if relative(reference.area))))
        {
            return Err(self.blocked("relative formula on a multi-area rule"));
        }
        if !deleted(anchor) || !others.is_empty() {
            return Ok(before.into());
        }
        let delta = i64::from(band.at + band.count - anchor);
        let (dr, dc) = match band.axis {
            Axis::Row => (delta, 0),
            Axis::Column => (0, delta),
        };
        translate(before, dr, dc, &|name| self.env.is_name(name))
            .map_err(|_| self.blocked("range formula translation"))
    }

    fn numeric(&mut self, node: &RawNode, path: &[usize], site: Site<'_>) -> Result<bool> {
        let band = self.ctx.band;
        if node.is(Col::QNAME) {
            if !matches!(band.axis, Axis::Column) {
                return Ok(false);
            }
            let Some((min, max)) = node.attr(attr("min")).zip(node.attr(attr("max"))) else {
                return Err(self.blocked("column bounds"));
            };
            let (lo, hi) = numeric_range(&format!("{min}:{max}"), ':', 1, MAX_COLS)?;
            if matches!(band.shift, Shift::Insert { .. }) && lo < band.at && band.at <= hi {
                let end = hi.saturating_add(band.count).min(MAX_COLS - 1);
                if band.at + band.count <= end {
                    if node.elements().next().is_some()
                        || node.attrs().iter().any(|attr| !attr.name().ns().is_empty())
                    {
                        return Err(self.blocked("extended column formatting"));
                    }
                    let attrs = [
                        "width",
                        "style",
                        "hidden",
                        "bestFit",
                        "customWidth",
                        "phonetic",
                        "outlineLevel",
                        "collapsed",
                    ]
                    .into_iter()
                    .filter_map(|name| node.attr(attr(name)).map(|value| (name, value.into())))
                    .chain([
                        ("min", (band.at + band.count + 1).to_string()),
                        ("max", (end + 1).to_string()),
                    ])
                    .collect();
                    let (&index, parent) = path.split_last().expect("column parent");
                    self.edits.splits.push(ColumnSplit {
                        path: parent.to_vec(),
                        index: index + 1,
                        attrs,
                    });
                }
                self.add(node, path, Some("max"), max, band.at.to_string())?;
                return Ok(false);
            }
            // Formatting shifted entirely past XFD has no surviving columns.
            if matches!(band.shift, Shift::Insert { .. })
                && band.at <= lo
                && lo + band.count >= MAX_COLS
            {
                return Ok(true);
            }
            // A full-width formatting record must still leave the new
            // leading columns at their default format.
            if matches!(band.shift, Shift::Insert { .. })
                && band.at == 0
                && lo == 0
                && hi == MAX_COLS - 1
            {
                let first = band.after(lo, "column formatting")?.expect("insertion");
                self.add(node, path, Some("min"), min, (first + 1).to_string())?;
                return Ok(false);
            }
            let Some((first, last)) = numeric_shift(band, lo, hi, || format!("col {min}:{max}"))? else {
                return Ok(true);
            };
            self.add(node, path, Some("min"), min, (first + 1).to_string())?;
            self.add(node, path, Some("max"), max, (last + 1).to_string())?;
        } else if let Some(axis) = site.breaks {
            if std::mem::discriminant(&axis) == std::mem::discriminant(&band.axis) {
                let Some(id) = node.attr(attr("id")) else {
                    return Ok(false);
                };
                let Some(new) = band.after(
                    id.parse().map_err(|_| self.blocked("break id"))?,
                    "break id",
                )?
                else {
                    return Ok(true);
                };
                self.add(node, path, Some("id"), id, new.to_string())?;
            } else {
                let end = (band.limit - 1).to_string();
                let min = node.attr(attr("min")).unwrap_or("0");
                let max = node.attr(attr("max")).unwrap_or(&end);
                let (lo, hi) = numeric_range(&format!("{min}:{max}"), ':', 0, band.limit)?;
                let Some((first, last)) = numeric_shift(band, lo, hi, || format!("brk {min}:{max}"))? else {
                    return Ok(true);
                };
                self.add(node, path, Some("min"), min, first.to_string())?;
                self.add(node, path, Some("max"), max, last.to_string())?;
            }
        }
        Ok(false)
    }
}

fn has_value(doc: &XmlDocument, at: CellIndex) -> bool {
    doc.root()
        .elements()
        .find(|node| node.is(tag("sheetData")))
        .into_iter()
        .flat_map(RawNode::elements)
        .filter(|row| xml_row(row, "").ok() == Some(at.row))
        .flat_map(RawNode::elements)
        .any(|cell| {
            cell.is(tag("c"))
                && xml_cell(cell, "").ok() == Some(at)
                && cell
                    .elements()
                    .any(|child| matches!(child.name().local(), "v" | "is" | "f"))
        })
}

pub(super) fn plan(
    doc: &XmlDocument,
    part: &str,
    sheet: usize,
    ctx: &FormulaContext<'_>,
    env: &Env,
    report: &mut StructuralEditReport,
) -> Result<Features> {
    let mut planner = Planner {
        doc,
        part,
        sheet: Some(sheet),
        kind: PartKind::Worksheet,
        ctx,
        env,
        report,
        edits: Features::default(),
        removed_cf_links: HashSet::new(),
    };
    if sheet == ctx.edited {
        let band = ctx.band;
        for cf in doc.root().elements().filter(|node| node.is(Cf::QNAME)) {
            let Some(sqref) = cf.attr(attr("sqref")) else {
                continue;
            };
            if rewrite_sqref(sqref, band.axis, band.shift)
                .map_err(|e| rewrite_error(e, part, "conditionalFormatting"))?
                == Rewrite::Remove
            {
                planner.removed_cf_links.extend(
                    cf.elements()
                        .filter(|rule| rule.is(tag("cfRule")))
                        .filter_map(crate::condfmt::x14_id)
                        .map(|id| id.to_ascii_lowercase()),
                );
            }
        }
    }
    planner.scan(
        doc.root(),
        &mut Vec::new(),
        &mut Vec::new(),
        Site::default(),
    )?;
    Ok(planner.edits)
}

pub(super) fn objects(
    wb: &mut Workbook,
    part: &str,
    ctx: &FormulaContext<'_>,
    change: &mut Change,
    report: &mut StructuralEditReport,
) -> Result<()> {
    let mut ownership = HashMap::new();
    for (sheet, entry) in wb.sheets.iter().enumerate() {
        if let Some(name) = &entry.part {
            let rels = wb.pkg.relationships(name)?;
            for rel in rels.iter() {
                if let Some(target) = rels.resolve(rel) {
                    ownership
                        .entry(target)
                        .and_modify(|owner| {
                            if sheet == ctx.edited {
                                *owner = sheet;
                            }
                        })
                        .or_insert(sheet);
                }
            }
        }
    }
    let parts: Vec<_> = wb.pkg.part_names().map(str::to_owned).collect();
    for name in parts {
        let content_type = wb
            .pkg
            .content_type(&name)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let kind = locations::additional_part(&content_type);
        let owner = ownership.get(&name).copied();
        let own = owner == Some(ctx.edited);
        let local_object = content_type.ends_with("comments+xml")
            || content_type.ends_with("threadedcomments+xml")
            || content_type.ends_with("pivottable+xml")
            || content_type.ends_with("tablesinglecells+xml");
        let pivot_cache = content_type.ends_with("pivotcachedefinition+xml");
        let chart = content_type.ends_with("chart+xml");
        if !(own && local_object
            || pivot_cache
            || chart
            || matches!(
                kind,
                PartKind::Drawing
                    | PartKind::Vml
                    | PartKind::Table
                    | PartKind::ChartEx
                    | PartKind::Connections
                    | PartKind::Control
                    | PartKind::ChartShapes
            ))
        {
            continue;
        }
        let doc = match wb.pkg.xml_part(&name) {
            Ok(doc) => doc,
            Err(_) if !own && kind == PartKind::Vml => continue,
            Err(error) => {
                return Err(Error::PartNotEditable {
                    part: name,
                    reason: error.to_string(),
                });
            }
        };
        let object_kind = if chart { PartKind::Chart } else { kind };
        if own || pivot_cache {
            check_object(doc.root(), own, ctx, &name)?;
        }
        let mut planner = Planner {
            doc,
            part: &name,
            sheet: owner,
            kind: object_kind,
            ctx,
            env: &wb.env,
            report,
            edits: Features::default(),
            removed_cf_links: HashSet::new(),
        };
        planner.scan(
            doc.root(),
            &mut Vec::new(),
            &mut Vec::new(),
            Site::default(),
        )?;
        if !planner.edits.is_empty() {
            planner.edits.rewrite(change.doc(&mut wb.pkg, &name)?)?;
        }
    }
    // The workbook context may expose pivot parts not listed under an ordinary location.
    if !wb.contexts[&ctx.edited].pivots.is_empty()
        && wb.pkg.relationships(part)?.iter().next().is_none()
    {
        return Err(Error::StructureBlocked {
            part: part.into(),
            place: "unresolved pivot table".into(),
        });
    }
    Ok(())
}

fn check_object(node: &RawNode, own: bool, ctx: &FormulaContext<'_>, part: &str) -> Result<()> {
    let local = node.name().local();
    let band = ctx.band;
    if ((own && matches!(local, "comment" | "threadedComment" | "location"))
        || (local == "worksheetSource"
            && node
                .attr(attr("sheet"))
                .is_some_and(|sheet| ctx.sheet(sheet) == Some(ctx.edited))))
        && let Some(reference) = node.attr(attr("ref"))
    {
        band.blocked(CellRange::parse(reference)?, part, local)?;
    }
    const DRAWING_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
    if own
        && node.name().ns() == DRAWING_NS
        && matches!(local, "oneCellAnchor" | "twoCellAnchor")
        && node.attr(attr("editAs")) != Some("absolute")
    {
        let coordinate = match band.axis {
            Axis::Row => "row",
            Axis::Column => "col",
        };
        let two_markers = local == "twoCellAnchor" && node.attr(attr("editAs")) != Some("oneCell");
        for marker in node.elements().filter(|child| {
            child.is(QName::new(DRAWING_NS, "from"))
                || two_markers && child.is(QName::new(DRAWING_NS, "to"))
        }) {
            if let Some(child) = marker
                .elements()
                .find(|child| child.is(QName::new(DRAWING_NS, coordinate)))
            {
                check_marker(band, &child.text(), part)?;
            }
        }
    }
    if own
        && node.name().ns() == "urn:schemas-microsoft-com:office:excel"
        && matches!(
            (band.axis, local),
            (Axis::Row, "Row") | (Axis::Column, "Column")
        )
    {
        check_marker(band, &node.text(), part)?;
    }
    if own && local == "Anchor" && node.name().ns() == "urn:schemas-microsoft-com:office:excel" {
        let text = node.text();
        let markers: Vec<_> = text.split(',').collect();
        let coordinate = match band.axis {
            Axis::Row => 2,
            Axis::Column => 0,
        };
        if markers.len() != 8 {
            return Err(Error::StructureBlocked {
                part: part.into(),
                place: "anchor".into(),
            });
        }
        for marker in [markers[coordinate], markers[coordinate + 4]] {
            check_marker(band, marker, part)?;
        }
    }
    if own
        && local == "singleXmlCell"
        && let Some(reference) = node.attr(attr("r"))
    {
        let cell = CellIndex::parse(reference)?;
        band.blocked(
            CellRange {
                first: cell,
                last: cell,
            },
            part,
            local,
        )?;
    }
    for child in node.elements() {
        check_object(child, own, ctx, part)?;
    }
    Ok(())
}

fn check_marker(band: Band, coordinate: &str, part: &str) -> Result<()> {
    let index = coordinate
        .trim()
        .parse::<u32>()
        .map_err(|_| Error::StructureBlocked {
            part: part.into(),
            place: "anchor".into(),
        })?;
    band.after(index, part)?;
    if index >= band.at {
        return Err(Error::StructureBlocked {
            part: part.into(),
            place: "anchor".into(),
        });
    }
    Ok(())
}

pub(super) fn relationships(
    wb: &Workbook,
    ctx: &FormulaContext<'_>,
    plan: &mut RegistryPlan,
    report: &mut StructuralEditReport,
) -> Result<()> {
    use ooxml_core::opc::TargetMode;
    let parts = std::iter::once(None).chain(
        wb.pkg
            .part_names()
            .filter(|name| !name.ends_with(".rels"))
            .map(Some),
    );
    for source in parts {
        let rels = match source {
            Some(source) => wb.pkg.relationships(source)?,
            None => wb.pkg.package_relationships().clone(),
        };
        for rel in rels.iter() {
            let target = rel.target.trim();
            let Some(reference) = target.strip_prefix('#') else {
                continue;
            };
            if !super::super::names::mentions_prefix(reference, &ctx.sheets[ctx.edited]) {
                continue;
            }
            let part = source.unwrap_or("[package]");
            let location = format!("relationship {}", rel.id);
            let Some(after) = ctx.rewrite(reference, None, None, part, &location)? else {
                continue;
            };
            if rel.target_mode != TargetMode::Internal {
                return Err(Error::UnrewritableReference {
                    part: part.into(),
                    location,
                });
            }
            if after.ends_with("#REF!") {
                return Err(Error::ReferenceDeleted {
                    part: part.into(),
                    location,
                });
            }
            let after = format!("#{after}");
            plan.set_relationship_target(&wb.pkg, source, &rel.id, &after)?;
            record(report, part, location, target, &after);
        }
    }
    Ok(())
}

pub(super) fn completeness(
    wb: &Workbook,
    ctx: &FormulaContext<'_>,
    edited_part: &str,
) -> Result<()> {
    let name = &ctx.sheets[ctx.edited];
    let worksheets: HashSet<_> = wb
        .sheets
        .iter()
        .filter(|sheet| sheet.kind == SheetKind::Worksheet)
        .filter_map(|sheet| sheet.part.as_deref())
        .collect();
    for part in wb.pkg.part_names() {
        let content_type = wb
            .pkg
            .content_type(part)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if Some(part) == wb.calc_chain.as_deref()
            || !(content_type.ends_with("/xml")
                || content_type.ends_with("+xml")
                || content_type.contains("vmldrawing")
                || part.ends_with(".rels"))
        {
            continue;
        }
        let kind = if worksheets.contains(part) {
            PartKind::Worksheet
        } else if part == wb.workbook_part {
            PartKind::Workbook
        } else if part.ends_with(".rels") {
            PartKind::Relationships
        } else if part.starts_with("docProps/")
            || content_type.ends_with("sharedstrings+xml")
            || content_type.contains("comments")
            || content_type.ends_with("person+xml")
            || content_type.ends_with("externallink+xml")
        {
            PartKind::NonReference
        } else if content_type.ends_with("chart+xml") {
            PartKind::Chart
        } else {
            locations::additional_part(&content_type)
        };
        if kind != PartKind::NonReference {
            super::super::scan::scan_grid_part(
                part,
                &wb.pkg.read_part(part)?,
                kind,
                name,
                part == edited_part,
            )?;
        }
    }
    Ok(())
}
