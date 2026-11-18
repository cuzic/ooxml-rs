use ooxml_core::Package;
use ooxml_robustness::phase1_oracle::{Phase1Edits, assert_phase1_diff};
use ooxml_xlsx::Workbook;

#[test]
fn string_write_appends_to_existing_shared_strings() {
    const SST: &str = "xl/sharedStrings.xml";
    let mut wb = Workbook::new().unwrap();
    let mut sheet = wb.worksheet().unwrap();
    sheet.cell_mut(0, 0).unwrap().set_string("original").unwrap();
    sheet.cell_mut(0, 1).unwrap().set_string("second").unwrap();
    let base = wb.to_bytes().unwrap();

    let mut wb = Workbook::from_bytes(base.clone()).unwrap();
    let sheet_name = wb.sheet_name().to_owned();
    let sheet_part = wb.worksheet_part().to_owned();
    let workbook = wb.workbook_part().to_owned();
    let mut edits = Phase1Edits::default();
    let pending = edits.before_write(&mut wb, &sheet_name, 20, 2);
    wb.worksheet().unwrap().cell_mut(20, 2).unwrap().set_string("aaaaaaaaaa").unwrap();
    edits.wrote_string(pending);
    let out = wb.to_bytes().unwrap();

    let check = |wb: &mut Workbook, out: &[u8], edits: &Phase1Edits| {
        assert_phase1_diff(wb, &base, out, &[&sheet_part], &workbook, edits);
    };
    check(&mut wb, &out, &edits);

    let mut package = Package::from_bytes(out).unwrap();
    let sst = String::from_utf8(package.read_part(SST).unwrap()).unwrap();
    let original = "<si><t>original</t></si>";
    let second = "<si><t>second</t></si>";
    assert!(sst.contains(&format!("{original}{second}<si><t>aaaaaaaaaa</t></si>")));
    assert!(sst.contains("count=\"3\"") && sst.contains("uniqueCount=\"3\""));

    for changed in [
        sst.replace("original", "rewritten"),
        sst.replace(original, ""),
        sst.replace(&format!("{original}{second}"), &format!("{second}{original}")),
        sst.replace("uniqueCount=\"3\"", "uniqueCount=\"4\""),
    ] {
        assert_ne!(changed, sst);
        package.set_part(SST, changed.into_bytes()).unwrap();
        let altered = package.to_bytes().unwrap();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&mut wb, &altered, &edits))).unwrap_err();
        assert!(failure.downcast_ref::<String>().is_some_and(|message| message.starts_with(SST)));
    }
}
