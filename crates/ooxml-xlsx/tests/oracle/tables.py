"""Read the P5b and P5d manual-check workbooks with openpyxl 3.1.5."""

from pathlib import Path
import sys
import xml.etree.ElementTree as ET
from zipfile import ZipFile

from openpyxl import load_workbook
from openpyxl.worksheet.formula import ArrayFormula


def main(directory: Path) -> None:
    workbook = load_workbook(directory / "39-p5b-table.xlsx")
    try:
        worksheet = workbook.active
        table = worksheet.tables["Sales"]
        columns = table.tableColumns
        assert (table.ref, table.autoFilter.ref, table.totalsRowCount) == ("A1:D4", "A1:D3", 1)
        assert [column.name for column in columns] == ["Market", "Units, shipped", "Notes", "Doubled"]
        assert table.tableStyleInfo.name == "TableStyleMedium9"
        assert table.tableStyleInfo.showRowStripes
        assert worksheet["A1"].value == "Market"
        assert worksheet["A4"].value == columns[0].totalsRowLabel == "Total"
        assert worksheet["B4"].value == "=SUBTOTAL(109,Sales[[Units, shipped]])"
        assert columns[1].totalsRowFunction == "sum"
        assert [worksheet[cell].value for cell in ("B2", "B3")] == [2, 5]
        formula = "Sales[[#This Row],[Units, shipped]]*2"
        assert columns[3].calculatedColumnFormula.text == formula
        assert worksheet["D2"].value == worksheet["D3"].value == "=" + formula
    finally:
        workbook.close()
    print("openpyxl table read-back passed")


def check_phase5d(directory: Path) -> None:
    dynamic = load_workbook(directory / "43-p5d-dynamic-array.xlsx")
    try:
        formula = dynamic.active["C1"].value
        assert isinstance(formula, ArrayFormula)
        assert (formula.ref, formula.text) == ("C1", "=_xlfn._xlws.SORT(A1:A3)")
    finally:
        dynamic.close()

    path = directory / "42-p5d-sparklines.xlsx"
    sparklines = load_workbook(path)
    sparklines.close()
    ns = {
        "x14": "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main",
        "xm": "http://schemas.microsoft.com/office/excel/2006/main",
    }
    with ZipFile(path) as archive:
        xml = ET.fromstring(archive.read("xl/worksheets/sheet1.xml"))
    groups = xml.findall(".//x14:sparklineGroup", ns)
    assert [group.attrib["type"] for group in groups] == ["line", "column"]
    assert [group.findtext(".//xm:f", namespaces=ns) for group in groups] == ["Sheet1!A1:C1"] * 2
    assert [group.findtext(".//xm:sqref", namespaces=ns) for group in groups] == ["D1", "E1"]
    print("openpyxl P5d read-back passed")



if __name__ == "__main__":
    directory = Path(sys.argv[1])
    main(directory)
    check_phase5d(directory)
