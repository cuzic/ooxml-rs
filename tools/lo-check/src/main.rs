use std::path::PathBuf;

use calamine::Reader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::args_os().nth(1).ok_or("expected a workbook path")?);
    let mut workbook = calamine::open_workbook_auto(&path)?;
    let sheets = workbook.sheet_names();
    if sheets.is_empty() {
        return Err("workbook has no sheets".into());
    }
    for name in sheets {
        workbook.worksheet_range(&name)?;
    }
    Ok(())
}
