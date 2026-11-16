use core_spike::*;
use schema_spike::sml::*;
const R: QName = QName { ns: "", local: "r" };
fn cell(r: &str) -> RawItem { let mut n = RawNode::parsed(Cell::QNAME, true); n.attrs.push((R, r.into())); RawItem::Elem(n) }
fn col(n: &RawNode) -> u8 { n.attr(R).unwrap().as_bytes()[0] }
fn refs(n: &RawNode) -> Vec<String> {
    n.children.iter().filter_map(|c| match c { RawItem::Elem(e) => Some(e.attr(R).map(String::from).unwrap_or(e.name.local.into())), _ => None }).collect()
}

#[test]
fn defaults_and_accessors() {
    let mut n = RawNode::parsed(Cell::QNAME, true);
    assert_eq!(CellView(Lens::new(&n)).style_index(), Ok(0));
    n.attrs.push((QName { ns: "", local: "s" }, "x".into()));
    assert_eq!(CellView(Lens::new(&n)).style_index(), Err(AttrError { raw: "x".into() })); // never silently 0
}
#[test]
fn insert_within_slot_by_column_and_dirty_path() {
    let mut row = RawNode::parsed(Row::QNAME, false); // <row r="1"> ... </row>
    row.children = vec![cell("B1"), cell("D1"), RawItem::Elem(RawNode::parsed(QName { ns: NS, local: "extLst" }, false))];
    let mut m = LensMut::<Row>::root(&mut row);
    m.insert_by::<Cell>(|_, e| col(e) > b'A').unwrap().set_attr(R, "A1");
    m.insert_by::<Cell>(|_, e| col(e) > b'C').unwrap().set_attr(R, "C1");
    assert_eq!(refs(&row), ["A1", "B1", "C1", "D1", "extLst"]);
    assert!(row.subtree_dirty && !row.own_dirty, "parent with a close tag keeps its own tags verbatim");
}
#[test]
fn self_closing_parent_gains_first_child_and_ancestors_marked() {
    let mut row = RawNode::parsed(Row::QNAME, false);
    row.children = vec![cell("D2")]; // <c r="D2" s="1"/>
    let mut r = RowViewMut(LensMut::root(&mut row));
    let mut c = CellViewMut(r.0.get_or_insert::<Cell>().unwrap());
    { c.value_mut().unwrap(); }
    { c.value_mut().unwrap(); } // second call reuses (get_or_insert), sequential reborrow
    let RawItem::Elem(d2) = &row.children[0] else { panic!() };
    assert!(d2.own_dirty && d2.subtree_dirty && row.subtree_dirty && !row.own_dirty);
    assert_eq!(d2.children.len(), 1);
}
#[test]
fn max_occurs_one() {
    let mut c = RawNode::parsed(Cell::QNAME, true);
    let mut m = LensMut::<Cell>::root(&mut c);
    m.insert::<CellValue>().unwrap();
    std::assert_matches!(m.insert::<CellValue>(), Err(Error::AlreadyPresent));
}
#[test]
fn ac_sibling_is_refused() {
    let mut row = RawNode::parsed(Row::QNAME, false);
    row.children = vec![RawItem::Elem(RawNode::parsed(QName { ns: MC_NS, local: "AlternateContent" }, false))];
    std::assert_matches!(LensMut::<Row>::root(&mut row).insert::<Cell>(), Err(Error::UnresolvedSibling));
}
#[test]
fn substr_range_gives_span() {
    let src = r#"<row r="1"><c r="A1"/></row>"#;
    let open = &src[src.find("<c").unwrap()..src.find("/>").unwrap() + 2];
    assert_eq!(src.substr_range(open), Some(std::range::Range::from(11..22)));
}
