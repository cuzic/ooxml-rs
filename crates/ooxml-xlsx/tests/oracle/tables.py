"""Read xlsx_manual_check's generated table with openpyxl 3.1.5."""

from pathlib import Path
import sys

from openpyxl import load_workbook


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


if __name__ == "__main__":
    main(Path(sys.argv[1]))
