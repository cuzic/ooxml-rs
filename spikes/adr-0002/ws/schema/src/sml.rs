// @generated (by hand for the spike)
use core_spike::*;
pub const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const fn q(l: &'static str) -> QName { QName { ns: NS, local: l } }
pub const CELL_VALUES: &[&str] = &["b", "n", "e", "s", "str", "inlineStr", "d"];

/// `x:CT_Row/x:row`
pub enum Row {}
impl ElementSpec for Row {
    const QNAME: QName = q("row");
    const ATTRS: &'static [AttrSpec] = &[AttrSpec { name: q("r"), ty: SimpleType::U32, default: None }];
    const CONTENT: Content = Content::Sequence(&[Item { name: q("c"), max: None }, Item { name: q("extLst"), max: Some(1) }]);
}
/// `x:CT_Cell/x:c`
pub enum Cell {}
impl ElementSpec for Cell {
    const QNAME: QName = q("c");
    const ATTRS: &'static [AttrSpec] = &[
        AttrSpec { name: q("r"), ty: SimpleType::Str, default: None },
        AttrSpec { name: q("s"), ty: SimpleType::U32, default: Some("0") },
        AttrSpec { name: q("t"), ty: SimpleType::Enum(CELL_VALUES), default: Some("n") },
    ];
    const CONTENT: Content = Content::Sequence(&[
        Item { name: q("f"), max: Some(1) }, Item { name: q("v"), max: Some(1) },
        Item { name: q("is"), max: Some(1) }, Item { name: q("extLst"), max: Some(1) },
    ]);
}
/// `x:CT_Xstring/x:v`
pub enum CellValue {}
impl ElementSpec for CellValue { const QNAME: QName = q("v"); const ATTRS: &'static [AttrSpec] = &[]; const CONTENT: Content = Content::Text; }

impl ChildOf<Row> for Cell {}
impl ChildOf<Cell> for CellValue {}
// generated consistency checks — module-level, always evaluated
const _: () = assert!(content_contains(Row::CONTENT, Cell::QNAME));
const _: () = assert!(content_contains(Cell::CONTENT, CellValue::QNAME));
const _: () = assert!(no_duplicate_items(Cell::CONTENT));
const _: () = { let a = Cell::ATTRS; let mut i = 0; while i < a.len() { assert!(default_is_valid(a[i])); i += 1; } };

// B2: accessors on a LOCAL newtype, not inherent impl on core's Lens
pub struct CellRef<'a>(pub Lens<'a, Cell>);
impl<'a> CellRef<'a> {
    pub fn style_index(&self) -> u32 { self.0.attr_str("s").and_then(|v| v.parse().ok()).unwrap_or(0) }
    pub fn data_type(&self) -> &'a str { self.0.attr_str("t").unwrap_or("n") }
}
