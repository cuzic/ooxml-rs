//! Opt-in sweep of the Apache POI test-data/spreadsheet corpus.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use ooxml_core::{Package, QName, XmlDocument};
use ooxml_xlsx::{CellIndex, CellRange, CellValue, MAX_ROWS, Table, Workbook};

const REL_TABLE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/table";
const TABLE_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml";

fn table_parts(wb: &Workbook) -> Vec<(u32, String, CellRange)> {
    let pkg = wb.package();
    pkg.part_names()
        .filter(|p| pkg.content_type(p) == Some(TABLE_TYPE))
        .filter_map(|p| {
            let bytes = pkg.read_part(p).ok()?;
            let doc = XmlDocument::parse_with(&bytes, &pkg.parse_options()).ok()?;
            let root = doc.root();
            let attr = |key| root.attr(QName::new("", key));
            Some((
                attr("id")?.parse().ok()?,
                attr("displayName")?.into(),
                CellRange::parse(attr("ref")?).ok()?,
            ))
        })
        .collect()
}

fn first_column_name(pkg: &Package, part: &str, at: CellIndex) -> Option<String> {
    let bytes = pkg.read_part(part).ok()?;
    let doc = XmlDocument::parse_with(&bytes, &pkg.parse_options()).ok()?;
    let root = doc.root();
    let range = CellRange::parse(root.attr(QName::new("", "ref"))?).ok()?;
    if range.first != at {
        return None;
    }
    root.elements()
        .find(|n| n.name().local() == "tableColumns")?
        .elements()
        .find(|n| n.name().local() == "tableColumn")?
        .attr(QName::new("", "name"))
        .map(str::to_owned)
}

#[test]
#[ignore = "set OOXML_POI_SPREADSHEET to the local POI corpus"]
fn poi_table_sweep() {
    let root = std::env::var("OOXML_POI_SPREADSHEET").expect("POI spreadsheet directory");
    let mut paths: Vec<_> = std::fs::read_dir(Path::new(&root))
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    let (
        mut opened,
        mut worksheets,
        mut created,
        mut synced,
        mut refused,
        mut skipped_sheets,
        mut reopened,
    ) = (0, 0, 0, 0, 0, 0, 0);
    let (mut duplicate_ids, mut duplicate_names, mut tried_headers) = (0, 0, 0);
    for path in &paths {
        let original: Arc<[u8]> = std::fs::read(path).unwrap().into();
        let Ok(mut wb) = Workbook::from_bytes(original) else {
            continue;
        };
        opened += 1;
        let initial = table_parts(&wb);
        let max_id = initial.iter().map(|t| t.0).max().unwrap_or_default();
        let mut edited = Vec::new();
        let names: Vec<_> = wb.sheet_names().map(str::to_owned).collect();
        for sheet in names {
            let before = wb.to_bytes().unwrap();
            let Ok(ws) = wb.worksheet_by_name(&sheet) else {
                skipped_sheets += 1;
                continue;
            };
            worksheets += 1;
            let part = ws.part_name().to_owned();
            // Use sheet relationships to pick existing table parts, independent
            // of the implementation's table inventory and id allocator.
            let rels = wb.package().relationships(&part).unwrap();
            let header_ranges: Vec<_> = rels
                .by_type(REL_TABLE)
                .filter_map(|r| {
                    let part = rels.resolve(r)?;
                    let pkg = wb.package();
                    let bytes = pkg.read_part(&part).ok()?;
                    let doc = XmlDocument::parse_with(&bytes, &pkg.parse_options()).ok()?;
                    let root = doc.root();
                    let count = root.attr(QName::new("", "headerRowCount")).unwrap_or("1");
                    let range = CellRange::parse(root.attr(QName::new("", "ref"))?).ok()?;
                    Some((count != "0", range.first))
                })
                .collect();
            let mut ws = wb.worksheet_by_name(&sheet).unwrap();
            if header_ranges.is_empty() {
                let last_row = ws
                    .cells()
                    .filter_map(Result::ok)
                    .filter_map(|c| CellIndex::parse(c.reference()).ok())
                    .map(|c| c.row)
                    .max();
                let row = last_row.map_or(0, |r| (r + 2).min(MAX_ROWS - 2));
                let at = CellIndex { row, col: 0 };
                let range = CellRange {
                    first: at,
                    last: CellIndex {
                        row: row + 1,
                        col: 1,
                    },
                };
                match ws.add_table(Table::new(range)) {
                    Ok(_) => {
                        created += 1;
                        edited.push((sheet, at, "Column1".to_owned()));
                    }
                    Err(e) => {
                        refused += 1;
                        eprintln!(
                            "refused creation in {}/{sheet}: {e}",
                            path.file_name().unwrap().to_string_lossy()
                        );
                        assert_eq!(wb.to_bytes().unwrap(), before);
                    }
                }
            } else {
                for (_, at) in header_ranges.into_iter().filter(|(header, _)| *header) {
                    let before = wb.to_bytes().unwrap();
                    let mut ws = wb.worksheet_by_name(&sheet).unwrap();
                    tried_headers += 1;
                    let name = format!("POI sweep {}", tried_headers);
                    match ws.write_string(at.row, at.col, &name) {
                        Ok(()) => {
                            synced += 1;
                            edited.push((sheet.clone(), at, name));
                        }
                        Err(e) => {
                            refused += 1;
                            eprintln!(
                                "refused header in {}/{sheet}: {e}",
                                path.file_name().unwrap().to_string_lossy()
                            );
                            assert_eq!(wb.to_bytes().unwrap(), before);
                        }
                    }
                }
            }
        }
        if edited.is_empty() {
            continue;
        }
        let mut wb = Workbook::from_bytes(wb.to_bytes().unwrap())
            .unwrap_or_else(|e| panic!("reopen {path:?}: {e}"));
        reopened += 1;
        let tables = table_parts(&wb);
        let (mut ids, mut names) = (HashSet::new(), HashSet::new());
        for (id, name, _) in tables {
            if !ids.insert(id) {
                duplicate_ids += 1;
                eprintln!("duplicate table id {id} in {path:?}");
            }
            if !names.insert(name.to_lowercase()) {
                duplicate_names += 1;
                eprintln!("duplicate table name {name} in {path:?}");
            }
            assert!(initial.iter().any(|t| t.0 == id) || id > max_id);
        }
        for (sheet, pos, value) in edited {
            let ws = wb.worksheet_by_name(&sheet).unwrap();
            let cell = ws.cell(pos.row, pos.col).unwrap().unwrap();
            assert_eq!(
                cell.value().unwrap(),
                CellValue::String(value.clone()),
                "{path:?} {sheet}!{pos}"
            );
            let sheet_part = ws.part_name().to_owned();
            let pkg = wb.package();
            let rels = pkg.relationships(&sheet_part).unwrap();
            let name = rels
                .by_type(REL_TABLE)
                .filter_map(|rel| rels.resolve(rel))
                .find_map(|part| first_column_name(pkg, &part, pos));
            assert_eq!(
                name.as_deref(),
                Some(value.as_str()),
                "{path:?} table at {sheet}!{pos}"
            );
        }
    }
    eprintln!(
        "files={} opened={opened} worksheets={worksheets} skipped_sheets={skipped_sheets} created={created} header_attempts={tried_headers} synced={synced} refused={refused} reopened={reopened} duplicate_ids={duplicate_ids} duplicate_names={duplicate_names}",
        paths.len()
    );
    assert_eq!((duplicate_ids, duplicate_names), (0, 0));
}
