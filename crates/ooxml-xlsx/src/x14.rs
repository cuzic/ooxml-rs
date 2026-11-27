//! Hand-written lenses for the x14 forms inside a worksheet's `extLst`
//! (ADR-0014 §5, §6): data validations and conditional formatting are read
//! and preserved. Standalone icon rules can also be written.

use ooxml_core::{AttrSpec, ChildOf, Content, ElementSpec, LensMut, QName, RawNode, SimpleType, Slot, Wildcard};
use ooxml_schema::sml;

pub(crate) const NS_X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
pub(crate) const DV_RULE: QName = QName::new(NS_X14, "dataValidation");
pub(crate) const NS_XM: &str = "http://schemas.microsoft.com/office/excel/2006/main";
/// `ext/@uri` of x14 data validations.
pub(crate) const URI_DV: &str = "{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}";
/// `ext/@uri` of x14 conditional formatting.
pub(crate) const URI_CF: &str = "{78C0D931-6437-407d-A8EE-F0AAD7539E65}";
pub(crate) const URI_SPARKLINE: &str = "{05C60535-1F16-4FD2-B633-F4F36F0B64E0}";

const ANY: Slot = Slot { names: &[], wildcard: Some(Wildcard::Any), max_each: None, max_total: None };

macro_rules! x14_spec {
    ($t:ident, $local:literal, $attrs:expr) => {
        #[derive(Debug)]
        pub(crate) enum $t {}
        impl ElementSpec for $t {
            const QNAME: QName = QName::new(NS_X14, $local);
            const SDK_NAME: &'static str = concat!("x14:", $local);
            const ATTRS: &'static [AttrSpec] = $attrs;
            const CONTENT: Content = Content::Slots(&[ANY]);
        }
    };
}

x14_spec!(DataValidations, "dataValidations", &[AttrSpec { name: QName::new("", "count"), ty: SimpleType::String, default: None }]);
x14_spec!(ConditionalFormattings, "conditionalFormattings", &[]);
x14_spec!(ConditionalFormatting, "conditionalFormatting", &[]);
x14_spec!(CfRule, "cfRule", &[AttrSpec { name: QName::new("", "priority"), ty: SimpleType::String, default: None }]);
impl ChildOf<sml::WorksheetExtension> for DataValidations {}
impl ChildOf<sml::WorksheetExtension> for ConditionalFormattings {}
impl ChildOf<ConditionalFormattings> for ConditionalFormatting {}
impl ChildOf<ConditionalFormatting> for CfRule {}

x14_spec!(IconSet, "iconSet", &[]);
x14_spec!(Cfvo, "cfvo", &[]);
x14_spec!(CfIcon, "cfIcon", &[]);
x14_spec!(DataBar, "dataBar", &[]);
x14_spec!(FillColor, "fillColor", &[]);
x14_spec!(BorderColor, "borderColor", &[]);
x14_spec!(NegativeFillColor, "negativeFillColor", &[]);
x14_spec!(NegativeBorderColor, "negativeBorderColor", &[]);
x14_spec!(AxisColor, "axisColor", &[]);
impl ChildOf<CfRule> for DataBar {}
impl ChildOf<DataBar> for Cfvo {}
impl ChildOf<DataBar> for FillColor {}
impl ChildOf<DataBar> for BorderColor {}
impl ChildOf<DataBar> for NegativeFillColor {}
impl ChildOf<DataBar> for NegativeBorderColor {}
impl ChildOf<DataBar> for AxisColor {}
impl ChildOf<CfRule> for IconSet {}
impl ChildOf<IconSet> for Cfvo {}
impl ChildOf<IconSet> for CfIcon {}

macro_rules! xm_text_spec {
    ($t:ident, $local:literal, $parent:ident) => {
        pub(crate) enum $t {}
        impl ElementSpec for $t {
            const QNAME: QName = QName::new(NS_XM, $local);
            const SDK_NAME: &'static str = concat!("xm:", $local);
            const ATTRS: &'static [AttrSpec] = &[];
            const CONTENT: Content = Content::Text(SimpleType::String);
        }
        impl ChildOf<$parent> for $t {}
    };
}

pub(crate) enum RuleId {}
impl ElementSpec for RuleId {
    const QNAME: QName = QName::new(NS_X14, "id");
    const SDK_NAME: &'static str = "x14:id";
    const ATTRS: &'static [AttrSpec] = &[];
    const CONTENT: Content = Content::Text(SimpleType::String);
}
impl ChildOf<sml::ConditionalFormattingRuleExtension> for RuleId {}

xm_text_spec!(Formula, "f", Cfvo);
xm_text_spec!(Sqref, "sqref", ConditionalFormatting);

/// The worksheet's `ext` elements with `uri` (item indices in `extLst`).
pub(crate) fn exts<'n>(ws: &'n RawNode, uri: &str) -> Vec<(usize, &'n RawNode)> {
    ws.elements()
        .filter(|e| e.is(sml::WorksheetExtensionList::QNAME))
        .flat_map(|l| l.indexed_elements().filter(|(_, e)| e.is(sml::WorksheetExtension::QNAME) && e.attr(QName::new("", "uri")) == Some(uri)))
        .collect()
}

/// `xm:sqref` text of an x14 rule element.
pub(crate) fn sqref(rule: &RawNode) -> Option<String> {
    rule.elements().find(|e| e.is(QName::new(NS_XM, "sqref"))).map(|e| e.text().into_owned())
}

/// Item indices of the `ext` elements with `uri` in a worksheet `extLst`.
pub(crate) fn exts_in_list(list: &RawNode, uri: &str) -> Vec<usize> {
    list.indexed_elements().filter(|(_, e)| e.is(sml::WorksheetExtension::QNAME) && e.attr(QName::new("", "uri")) == Some(uri)).map(|(i, _)| i).collect()
}

/// A mutable lens on the `C` child at item index `i` (planned to exist).
pub(crate) fn child_at<'s, P: ElementSpec, C: ChildOf<P>>(l: &'s mut LensMut<'_, P>, i: usize) -> LensMut<'s, C> {
    let k = l.node().indexed_elements().filter(|(_, e)| e.is(C::QNAME)).position(|(j, _)| j == i).expect("planned child");
    l.nth_child_mut::<C>(k).expect("planned child")
}
