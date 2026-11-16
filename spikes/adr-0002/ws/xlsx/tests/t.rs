use core_spike::*;
use schema_spike::sml::*;
fn cell(r: &str) -> RawItem { let mut n = RawNode::new(Cell::QNAME); n.attrs.push((Cell::ATTRS[0].name, r.into())); n.own_dirty=false; n.subtree_dirty=false; RawItem::Elem(n) }
fn col(n: &RawNode) -> u32 { n.attr(Cell::ATTRS[0].name).unwrap().bytes().next().unwrap() as u32 }
fn refs(n: &RawNode) -> Vec<String> { n.children.iter().filter_map(|c| match c { RawItem::Elem(e) => Some(e.attr(Cell::ATTRS[0].name).map(String::from).unwrap_or(e.name.local.into())), _ => None }).collect() }

#[test]
fn defaults_and_accessors() {
    let n = RawNode::new(Cell::QNAME);
    let c = CellRef(Lens::new(&n));
    assert_eq!(c.style_index(), 0);
    assert_eq!(c.data_type(), "n");
}
#[test]
fn insert_within_band_by_column() {
    let mut row = RawNode::new(Row::QNAME);
    row.children = vec![cell("B1"), cell("D1"), RawItem::Elem(RawNode::new(QName{ns:NS,local:"extLst"}))];
    let mut m = LensMut::<Row>::new(&mut row);
    let new = m.insert_by::<Cell>(|e| col(e).cmp(&(b'A' as u32))).unwrap();
    new.attrs.push((Cell::ATTRS[0].name, "A1".into()));
    let mut m = LensMut::<Row>::new(&mut row);
    let new = m.insert_by::<Cell>(|e| col(e).cmp(&(b'C' as u32))).unwrap();
    new.attrs.push((Cell::ATTRS[0].name, "C1".into()));
    assert_eq!(refs(&row), ["A1", "B1", "C1", "D1", "extLst"]);
}
#[test]
fn max_occurs_one() {
    let mut c = RawNode::new(Cell::QNAME);
    LensMut::<Cell>::new(&mut c).insert::<CellValue>().unwrap();
    std::assert_matches!(LensMut::<Cell>::new(&mut c).insert::<CellValue>(), Err(Error::AlreadyPresent));
}
#[test]
fn ac_sibling_is_refused() {
    let mut row = RawNode::new(Row::QNAME);
    row.children = vec![RawItem::Elem(RawNode::new(QName{ns:MC_NS,local:"AlternateContent"}))];
    assert!(matches!(LensMut::<Row>::new(&mut row).insert::<Cell>(), Err(Error::UnresolvedSibling)));
}
#[test]
fn substr_range_gives_span() {
    let src = r#"<row r="1"><c r="A1"/></row>"#;
    let open = &src[src.find("<c").unwrap()..src.find("/>").unwrap() + 2];
    assert_eq!(src.substr_range(open), Some(std::range::Range::from(11..22)));
}
