use ooxml_xlsx::Workbook;
use sha2::{Digest, Sha256};

#[test]
#[ignore = "requires the ADR-0018 corpus"]
fn sheet_operation_corpus() {
    let root = std::env::var_os("OOXML_XLSX_CORPUS").expect("set OOXML_XLSX_CORPUS");
    let mut files: Vec<_> = [
        std::path::PathBuf::from(root),
        "../../reference/fixtures/xlsx/calamine".into(),
    ]
    .into_iter()
    .flat_map(|dir| std::fs::read_dir(dir).unwrap())
    .map(|entry| entry.unwrap().path())
    .filter(|path| {
        path.extension()
            .is_some_and(|ext| ext == "xlsx" || ext == "xlsm")
    })
    .collect();
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    for path in files {
        let input = std::fs::read(&path).unwrap();
        let Ok(book) = Workbook::from_bytes(input.clone()) else {
            continue;
        };
        if book.edit_refusal().is_some() {
            continue;
        }
        let before = book.to_bytes().unwrap();
        let file = path.file_name().unwrap().to_string_lossy();
        for (index, sheet) in book.sheet_names().enumerate() {
            for operation in ["rename", "delete"] {
                let mut edit = Workbook::from_bytes(input.clone()).unwrap();
                let result = if operation == "rename" {
                    edit.rename_sheet(sheet, &format!("PhaseSevenRenamed{index}"))
                } else {
                    edit.delete_sheet(sheet)
                };
                let out = edit.to_bytes().unwrap();
                match result {
                    Ok(rewritten) => {
                        let digest = Sha256::digest(&out);
                        let rewrites = Sha256::digest(format!("{rewritten:?}"));
                        let relationships = rewritten
                            .iter()
                            .filter(|row| row.part.ends_with(".rels"))
                            .count();
                        println!(
                            "P7\t{file}\t{index}\t{operation}\tOK\t{digest:02x?}\t{rewrites:02x?}\t{relationships}"
                        );
                    }
                    Err(error) => {
                        assert!(
                            out == before,
                            "{file} {sheet:?} {operation}: refusal changed output"
                        );
                        println!("P7\t{file}\t{index}\t{operation}\tERR\t{error:?}");
                    }
                }
            }
        }
    }
}
