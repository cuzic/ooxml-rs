//! Worksheet sparkline extension (ADR-0017 §8).

use ooxml_core::{
    AttrSpec, ChildOf, Content, ElementSpec, LensMut, NsDecl, QName, SimpleType, Slot, Wildcard,
};
use ooxml_schema::sml;

use crate::formula::lexer::{Area, Prefix, TokKind, Token, lex};
use crate::sheet_ops::quote_sheet;
use crate::workbook::ApplyStepHook;
use crate::x14::{NS_X14, NS_XM, child_at, exts_in_list};
use crate::{CellIndex, Error, Result, Worksheet};

const URI: &str = "{05C60535-1F16-4fd2-B633-F4F36F0B64E0}";
const X14: [NsDecl<'static>; 1] = [NsDecl::new("x14", NS_X14)];
const XM: [NsDecl<'static>; 1] = [NsDecl::new("xm", NS_XM)];
const ANY: Slot = Slot {
    names: &[],
    wildcard: Some(Wildcard::Any),
    max_each: None,
    max_total: None,
};

macro_rules! spec {
    ($ty:ident, $ns:ident, $local:literal, $parent:ty, $content:expr $(, $attr:literal)*) => {
        enum $ty {}
        impl ElementSpec for $ty {
            const QNAME: QName = QName::new($ns, $local);
            const SDK_NAME: &'static str = $local;
            const ATTRS: &'static [AttrSpec] = &[$(
                AttrSpec { name: QName::new("", $attr), ty: SimpleType::String, default: None },
            )*];
            const CONTENT: Content = $content;
        }
        impl ChildOf<$parent> for $ty {}
    };
}
spec!(
    Groups,
    NS_X14,
    "sparklineGroups",
    sml::WorksheetExtension,
    Content::Slots(&[ANY])
);
spec!(
    Group,
    NS_X14,
    "sparklineGroup",
    Groups,
    Content::Slots(&[ANY]),
    "type",
    "displayEmptyCellsAs",
    "markers",
    "high",
    "low",
    "first",
    "last",
    "negative",
    "displayXAxis",
    "minAxisType",
    "maxAxisType",
    "manualMin",
    "manualMax"
);
spec!(Series, NS_X14, "colorSeries", Group, Content::Empty, "rgb");
spec!(
    Negative,
    NS_X14,
    "colorNegative",
    Group,
    Content::Empty,
    "rgb"
);
spec!(Axis, NS_X14, "colorAxis", Group, Content::Empty, "rgb");
spec!(
    Markers,
    NS_X14,
    "colorMarkers",
    Group,
    Content::Empty,
    "rgb"
);
spec!(First, NS_X14, "colorFirst", Group, Content::Empty, "rgb");
spec!(Last, NS_X14, "colorLast", Group, Content::Empty, "rgb");
spec!(High, NS_X14, "colorHigh", Group, Content::Empty, "rgb");
spec!(Low, NS_X14, "colorLow", Group, Content::Empty, "rgb");
spec!(
    Sparklines,
    NS_X14,
    "sparklines",
    Group,
    Content::Slots(&[ANY])
);
spec!(
    Sparkline,
    NS_X14,
    "sparkline",
    Sparklines,
    Content::Slots(&[ANY])
);
spec!(
    Formula,
    NS_XM,
    "f",
    Sparkline,
    Content::Text(SimpleType::String)
);
spec!(
    Destination,
    NS_XM,
    "sqref",
    Sparkline,
    Content::Text(SimpleType::String)
);

/// Sparkline plot type.
#[derive(Clone, Copy, Debug)]
pub enum SparklineType {
    Line,
    Column,
    Stacked,
}

/// How the plot displays empty cells.
#[derive(Clone, Copy, Debug, Default)]
pub enum SparklineEmptyCells {
    #[default]
    Gap,
    Zero,
    Span,
}

/// Scaling for one end of the vertical axis.
#[derive(Clone, Copy, Debug, Default)]
pub enum SparklineAxis {
    #[default]
    Individual,
    Group,
    Custom(f64),
}

/// Optional 32-bit ARGB colors for a sparkline group.
#[derive(Clone, Copy, Debug, Default)]
pub struct SparklineColors {
    pub series: Option<u32>,
    pub negative: Option<u32>,
    pub axis: Option<u32>,
    pub markers: Option<u32>,
    pub first: Option<u32>,
    pub last: Option<u32>,
    pub high: Option<u32>,
    pub low: Option<u32>,
}

/// A set of sparklines with shared display options.
#[derive(Clone, Debug)]
pub struct SparklineGroup {
    pub kind: SparklineType,
    pub markers: bool,
    pub high: bool,
    pub low: bool,
    pub first: bool,
    pub last: bool,
    pub negative: bool,
    pub colors: SparklineColors,
    pub display_x_axis: bool,
    pub min_axis: SparklineAxis,
    pub max_axis: SparklineAxis,
    pub empty_cells: SparklineEmptyCells,
    entries: Vec<(String, String)>,
}

impl SparklineGroup {
    /// Creates an empty group of the given type.
    pub fn new(kind: SparklineType) -> Self {
        Self {
            kind,
            markers: false,
            high: false,
            low: false,
            first: false,
            last: false,
            negative: false,
            colors: SparklineColors::default(),
            display_x_axis: false,
            min_axis: SparklineAxis::Individual,
            max_axis: SparklineAxis::Individual,
            empty_cells: SparklineEmptyCells::Gap,
            entries: Vec::new(),
        }
    }

    /// Adds a sparkline at one A1 cell with a sheet-qualified source range.
    pub fn add(mut self, destination: impl Into<String>, source: impl Into<String>) -> Self {
        self.entries.push((destination.into(), source.into()));
        self
    }
}

fn color<C: ChildOf<Group>>(group: &mut LensMut<'_, Group>, value: Option<u32>) -> Result<()> {
    if let Some(argb) = value {
        group
            .insert::<C>()?
            .set_attr(QName::new("", "rgb"), &format!("{argb:08X}"))?;
    }
    Ok(())
}

fn write_group(
    groups: &mut LensMut<'_, Groups>,
    opts: SparklineGroup,
    entries: &[(CellIndex, String)],
) -> Result<()> {
    let mut group = groups.insert::<Group>()?;
    group.set_attr(
        QName::new("", "type"),
        match opts.kind {
            SparklineType::Line => "line",
            SparklineType::Column => "column",
            SparklineType::Stacked => "stacked",
        },
    )?;
    group.set_attr(
        QName::new("", "displayEmptyCellsAs"),
        match opts.empty_cells {
            SparklineEmptyCells::Gap => "gap",
            SparklineEmptyCells::Zero => "zero",
            SparklineEmptyCells::Span => "span",
        },
    )?;
    for (name, enabled) in [
        ("markers", opts.markers),
        ("high", opts.high),
        ("low", opts.low),
        ("first", opts.first),
        ("last", opts.last),
        ("negative", opts.negative),
        ("displayXAxis", opts.display_x_axis),
    ] {
        if enabled {
            group.set_attr(QName::new("", name), "1")?;
        }
    }
    for (attr, bound, axis) in [
        ("minAxisType", "manualMin", opts.min_axis),
        ("maxAxisType", "manualMax", opts.max_axis),
    ] {
        match axis {
            SparklineAxis::Individual => {}
            SparklineAxis::Group => group.set_attr(QName::new("", attr), "group")?,
            SparklineAxis::Custom(value) => {
                group.set_attr(QName::new("", attr), "custom")?;
                group.set_attr(QName::new("", bound), &value.to_string())?;
            }
        }
    }
    color::<Series>(&mut group, opts.colors.series)?;
    color::<Negative>(&mut group, opts.colors.negative)?;
    color::<Axis>(&mut group, opts.colors.axis)?;
    color::<Markers>(&mut group, opts.colors.markers)?;
    color::<First>(&mut group, opts.colors.first)?;
    color::<Last>(&mut group, opts.colors.last)?;
    color::<High>(&mut group, opts.colors.high)?;
    color::<Low>(&mut group, opts.colors.low)?;
    let mut list = group.insert::<Sparklines>()?;
    for (dest, source) in entries {
        let mut spark = list.insert::<Sparkline>()?;
        spark.insert_with_ns::<Formula>(&XM)?.set_text(source)?;
        spark
            .insert_with_ns::<Destination>(&XM)?
            .set_text(&dest.to_string())?;
    }
    Ok(())
}

impl Worksheet<'_> {
    /// Appends a sparkline group to the worksheet extension.
    pub fn add_sparklines(&mut self, group: SparklineGroup) -> Result<()> {
        let part = self.part_name().to_owned();
        let wb = &mut *self.wb;
        wb.check_editable()?;
        if group.entries.is_empty()
            || [group.min_axis, group.max_axis]
                .into_iter()
                .any(|a| matches!(a, SparklineAxis::Custom(v) if !v.is_finite()))
        {
            return Err(Error::InvalidSparklineGroup);
        }
        let mut entries = Vec::with_capacity(group.entries.len());
        for (dest, source) in &group.entries {
            let bad_range = || Error::InvalidFormula {
                text: source.clone(),
                reason: "expected a sheet-qualified A1 range".into(),
            };
            let trimmed = source.trim();
            let tokens = lex(trimmed).map_err(|_| bad_range())?;
            let [
                Token {
                    kind: TokKind::Ref(reference),
                    ..
                },
            ] = tokens.as_slice()
            else {
                return Err(bad_range());
            };
            let Prefix::Sheet(name) = &reference.prefix else {
                return Err(bad_range());
            };
            if !matches!(reference.area, Area::Cell { .. } | Area::Range { .. }) {
                return Err(bad_range());
            }
            let sheet = &wb.sheets[wb.sheet_index(name)?].name;
            let text = format!(
                "{}!{}",
                quote_sheet(sheet),
                &trimmed[reference.area_offset..]
            );
            if ooxml_core::value::first_invalid_xml_char(&text).is_some() {
                return Err(bad_range());
            }
            entries.push((CellIndex::parse(dest)?, text));
        }

        let doc = wb
            .pkg
            .xml_part_mut(&part)
            .map_err(|e| Error::PartNotEditable {
                part: part.clone(),
                reason: e.to_string(),
            })?;
        if doc.max_depth() < 8 {
            return Err(Error::PartNotEditable {
                part,
                reason: "sparkline group exceeds XML depth limit".into(),
            });
        }
        let mut sheet = doc.lens_mut::<sml::Worksheet>()?;
        let index = if let Some(mut list) = sheet.child_mut::<sml::WorksheetExtensionList>() {
            let exts = exts_in_list(list.node(), URI);
            if let Some(&index) = exts.first() {
                let mut ext = child_at::<_, sml::WorksheetExtension>(&mut list, index);
                if let Some(groups) = ext.child_mut::<Groups>() {
                    groups.check_insert::<Group>()?;
                } else {
                    ext.check_insert_with_ns::<Groups>(&[X14[0], XM[0]])?;
                }
            } else {
                list.check_insert_with_ns::<sml::WorksheetExtension>(&X14)?;
            }
            exts.first().copied()
        } else {
            sheet.check_insert::<sml::WorksheetExtensionList>()?;
            None
        };

        let registry = ooxml_core::RegistryPlan::new(&wb.pkg);
        wb.apply_registry_edit(registry, |wb| {
            wb.fault_point(ApplyStepHook::Sparklines)?;
            let mut sheet = wb.pkg.xml_part_mut(&part)?.lens_mut::<sml::Worksheet>()?;
            let mut list = sheet.get_or_insert::<sml::WorksheetExtensionList>()?;
            let mut ext = if let Some(index) = index {
                child_at::<_, sml::WorksheetExtension>(&mut list, index)
            } else {
                let mut ext = list.insert_with_ns::<sml::WorksheetExtension>(&X14)?;
                ext.set_attr(QName::new("", "uri"), URI)?;
                ext
            };
            let mut groups = ext.get_or_insert_with_ns::<Groups>(&[X14[0], XM[0]])?;
            write_group(&mut groups, group, &entries)
        })
    }
}
