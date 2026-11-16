// @generated shape (written by hand for the spike)
use core_spike::*;
pub const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const fn q(l: &'static str) -> QName { QName { ns: NS, local: l } }
pub const CELL_VALUES: &[&str] = &["b", "n", "e", "s", "str", "inlineStr", "d"];
macro_rules! one { ($n:expr) => { Slot { names: &[q($n)], max_each: Some(1), max_total: Some(1) } } }
macro_rules! many { ($n:expr) => { Slot { names: &[q($n)], max_each: None, max_total: None } } }

/// `x:CT_Row/x:row`
pub enum Row {}
impl ElementSpec for Row {
    const QNAME: QName = q("row");
    const ATTRS: &'static [AttrSpec] = &[AttrSpec { name: QName { ns: "", local: "r" }, ty: SimpleType::U32, default: None }];
    const CONTENT: Content = Content::Slots(&[many!("c"), one!("extLst")]);
}
/// `x:CT_Cell/x:c` (attributes merged from abstract `x:CT_Cell/` = CellType)
pub enum Cell {}
impl ElementSpec for Cell {
    const QNAME: QName = q("c");
    const ATTRS: &'static [AttrSpec] = &[
        AttrSpec { name: QName { ns: "", local: "r" }, ty: SimpleType::Str, default: None },
        AttrSpec { name: QName { ns: "", local: "s" }, ty: SimpleType::U32, default: Some("0") },
        AttrSpec { name: QName { ns: "", local: "t" }, ty: SimpleType::Enum(CELL_VALUES), default: Some("n") },
    ];
    const CONTENT: Content = Content::Slots(&[one!("f"), one!("v"), one!("is"), one!("extLst")]);
}
/// `x:CT_Xstring/x:v`
pub enum CellValue {}
impl ElementSpec for CellValue { const QNAME: QName = q("v"); const ATTRS: &'static [AttrSpec] = &[]; const CONTENT: Content = Content::Text; }

impl ChildOf<Row> for Cell {}
impl ChildOf<Cell> for CellValue {}
const _: () = assert!(content_contains(Row::CONTENT, Cell::QNAME));
const _: () = assert!(content_contains(Cell::CONTENT, CellValue::QNAME));
const _: () = assert!(no_duplicate_names(Cell::CONTENT));
const _: () = { let a = Cell::ATTRS; let mut i = 0; while i < a.len() { assert!(default_is_valid(a[i])); i += 1; } };

/// Local newtype views (B2): inherent methods are legal here.
pub struct CellView<'a>(pub Lens<'a, Cell>);
impl<'a> CellView<'a> {
    pub fn style_index(&self) -> Result<u32, AttrError> { self.0.attr_or_default("s").map(|v| v.unwrap_or(0)) }
}
pub struct RowViewMut<'a>(pub LensMut<'a, Row>);
pub struct CellViewMut<'a>(pub LensMut<'a, Cell>);
impl<'a> CellViewMut<'a> {
    /// Child views are reborrowed from the parent view, one at a time.
    pub fn value_mut(&mut self) -> Result<LensMut<'_, CellValue>, Error> { self.0.get_or_insert::<CellValue>() }
}
