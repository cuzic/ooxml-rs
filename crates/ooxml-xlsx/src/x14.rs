//! Hand-written lenses for the x14 forms inside a worksheet's `extLst`
//! (ADR-0014 §5, §6): data validations and conditional formatting are read
//! and preserved, and only removed or renumbered by Phase4.

use ooxml_core::{AttrSpec, ChildOf, Content, ElementSpec, QName, RawNode, SimpleType, Slot, Wildcard};
use ooxml_schema::sml;

pub(crate) const NS_X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
pub(crate) const NS_XM: &str = "http://schemas.microsoft.com/office/excel/2006/main";
/// `ext/@uri` of x14 data validations.
pub(crate) const URI_DV: &str = "{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}";
/// `ext/@uri` of x14 conditional formatting.
pub(crate) const URI_CF: &str = "{78C0D931-6437-407d-A8EE-F0AAD7539E65}";

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
