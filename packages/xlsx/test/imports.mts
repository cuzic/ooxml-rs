import * as ExcelJS from '@ooxml-rs/xlsx/exceljs';
import * as XLSX from '@ooxml-rs/xlsx/sheetjs';
const stringCodepage: XLSX.CodepageTable = { utils: { decode: () => '', encode: (_page, text) => text } };

const writer = new ExcelJS.stream.xlsx.WorkbookWriter({ filename: 'large.xlsx', useSharedStrings: true, useStyles: true, zip: { comment: "Archive comment", forceZip64: true, forceLocalTime: true, zlib: { level: 9 } } });
writer.properties.date1904 = true;
const writingSheet = writer.addWorksheet('Data', { useSharedStrings: false, properties: { outlineLevelRow: 1 } });
const streamStarted: boolean = writingSheet.startedData;
const streamHasComments: boolean = writingSheet.hasComments;
const rejectStreamDestroy: () => never = writingSheet.destroy;
const backgroundId = writer.addImage({ filename: 'background.png', extension: 'png' });
writingSheet.addBackgroundImage(backgroundId);
writingSheet.autoFilter = { from: 'A1', to: { row: 2, column: 2 } };
writingSheet.state = 'visible';
writingSheet.mergeCells([[['B2', 'A1']]]);
const committedRange: string = writingSheet.dimensions.shortRange;
writingSheet.properties.defaultRowHeight = 22;
const nextStreamingSheetId: number = writer.nextId;
writingSheet.views = [{ state: 'frozen', xSplit: 1 }];
writingSheet.pageSetup = { orientation: 'landscape' };
writingSheet.pageSetup.margins.left = 0.5;
writingSheet.headerFooter.oddFooter = '&RPage &P';
await writingSheet.protect('password', { spinCount: 1, formatRows: true });
writingSheet.unprotect();
writingSheet.addConditionalFormatting({ ref: 'A1', rules: [{ type: 'expression', formulae: ['A1>0'] }] });
writingSheet.removeConditionalFormatting(block => block.ref === 'A1');
const backgroundName: string | undefined = writer.getImage(backgroundId)?.name;
writingSheet.columns = [{ key: 'id', header: 'ID', style: { numFmt: '0' } }];
writingSheet.getColumn('id').font = { bold: true };
writingSheet.getColumn('id').header = ['ID', 'Number'];
const pendingColumnValues: ExcelJS.StreamingCellValue[] = writingSheet.getColumn('id').values;
writingSheet.getColumn('id').eachCell({ includeEmpty: true }, (cell, rowNumber) => { if (rowNumber === 2) cell.numFmt = '0'; });
const row = writingSheet.addRow({ id: 42 });
row.getCell(1).addMergeRef();
row.getCell(1).releaseMergeRef();
row.getCell(2).value = { formula: 'DATE(2024,2,29)', result: new Date() };
row.getCell(3).value = { richText: [{ text: 'Styled', font: { bold: true } }] };
row.getCell(1).style = { font: { bold: true }, numFmt: '0.00' };
row.height = 20;
row.getCell(1).names = ['Revenue', 'CurrentRevenue'];
const streamedType: typeof ExcelJS.ValueType[keyof typeof ExcelJS.ValueType] = row.getCell(1).type;
const streamedAddress: ExcelJS.Cell['fullAddress'] = row.getCell(1).fullAddress;
const streamedFormula: string | undefined = row.getCell(2).formula;
const streamedResult: ExcelJS.Cell['result'] = row.getCell(2).result;
const pendingCell: ExcelJS.StreamingCell | undefined = row.findCell(1);
const streamedText: string = row.getCell(1).text;
const streamedHtml: string = row.getCell(1).html;
const streamedCsv: string | number = row.getCell(1).toCsvString();
row.getCell(2).dataValidation = { type: "whole", operator: "between", formulae: [1, 5] };
row.getCell(1).note = { texts: [{ text: 'Cell note' }] };
const populatedColumns: number = row.actualCellCount;
const rowBounds: { min: number; max: number } | null = row.dimensions;
row.eachCell({ includeEmpty: true }, (cell, column) => { if (column === 2) cell.font = { bold: true }; });
row.addPageBreak(2, 4);
row.alignment = { horizontal: 'center' };
row.getCell(5).value = { text: 'Example', hyperlink: 'https://example.com/' };
row.getCell(6).value = { formula: 'A2:B2*2', shareType: 'array', ref: 'F2:G2', result: 2 };
row.outlineLevel = 1;
writingSheet.mergeCells('C2:D3');
row.commit();
row.worksheet.workbook.definedNames.add('Data!A2', 'WrittenValue');
writer.creator = 'Type fixture';
writer.calcProperties.fullCalcOnLoad = true;
writer.lastPrinted = new Date();
writer.views.push({ activeTab: 0, visibility: 'visible' });
await writer.commit();

const reader = new ExcelJS.stream.xlsx.WorkbookReader('large.xlsx', { styles: 'cache' });
reader.on('worksheet', sheet => {
  const id: number = sheet.id;
  sheet.once('row', row => {
    const number: number = row.number;
    // @ts-expect-error A row number is numeric.
    const wrong: string = row.number;
    void [number, wrong];
  });
  sheet.on('hyperlink', ({ ref }) => {
    const address: string = ref;
    void address;
  });
  // @ts-expect-error Worksheet events supply a numeric ID.
  const wrong: string = sheet.id;
  void [id, wrong];
});
reader.on('hyperlinks', links => {
  links.on('hyperlink', relationship => {
    const target: string = relationship.target;
    // @ts-expect-error A relationship target is a string.
    const wrong: number = relationship.target;
    void [target, wrong];
  });
});
reader.once('entry', entry => {
  if (entry.type === 'worksheet') {
    const id: string = entry.id;
    void id;
  }
});
for await (const worksheet of reader) {
  for await (const streamedRow of worksheet) {
    const address: string = streamedRow.getCell(1).address;
    const height: number | undefined = streamedRow.height;
    const columnWidth: number | undefined = worksheet.getColumn(1).width;
    void [address, height, columnWidth];
  }
}
const source = new Uint8Array();
const workbook = new ExcelJS.Workbook();
await workbook.xlsx.load(source);
workbook.getWorksheet('Data')!.getCell('B2').value = { formula: 'FALSE()', result: false };
const output: Uint8Array = await workbook.xlsx.writeBuffer();
const book = XLSX.read(output);
book.Sheets.Data!.B2 = { t: 'n', v: 1 };
const sheetOutput: ArrayBuffer = XLSX.write(book, { type: 'array', bookType: 'xlsx' });
void sheetOutput;

const formatId: number = XLSX.SSF.load("0.00", 200);
const formatted: string | number = XLSX.SSF.format(formatId, 42);
void formatted;

const xml: string = XLSX.write(book, { bookType: 'xlml', type: 'string' });
const fromXml = XLSX.read(xml, { type: 'string', dense: true });
const styledXml = XLSX.read(xml, { type: 'string', cellStyles: true });
const rightToLeft: boolean | undefined = styledXml.Workbook?.Views?.[0]?.RTL;
const ranges: string[] = [XLSX.utils.decode_range('A1:B2')].map(XLSX.utils.encode_range);
void [fromXml, rightToLeft, ranges];
const activeTab = book.Workbook?.WBView?.[0]?.activeTab;
const sheetId: string | undefined = book.Workbook?.Sheets?.[0]?.sheetId;
const columnWidth: number | null | undefined = book.Sheets[book.SheetNames[0]]?.['!cols']?.[0]?.width;
void [activeTab, sheetId, columnWidth];
XLSX.write({ SheetNames: ['S'], Sheets: { S: { '!ref': 'A1', '!cols': [{ width: null, hidden: true, customwidth: '1' }],
  A1: { t: 's', v: 'x', l: { Target: '#S!A1', ref: 'A1', location: 'S!A1', Rel: { Target: '#S!A1', TargetMode: 'Internal' } } } } },
  Workbook: { Views: [{ RTL: true, zoom: 80 }], WBProps: { CodeName: '', filterPrivacy: false }, Sheets: [{ name: 'S', CodeName: 'Sheet1' }] } }, { type: 'array' });
// @ts-expect-error SpreadsheetML style projection accepts a boolean.
XLSX.read(xml, { type: 'string', cellStyles: 'true' });

const addin: ArrayBuffer = XLSX.write(book, { bookType: 'xla', type: 'array' });
void addin;
XLSX.writeFile(book, 'book.xla', { bookType: 'xla', bookSST: true });
XLSX.writeFileAsync('book.dat', book, { bookType: 'socialcalc', sheet: 0 }, () => {});
// @ts-expect-error The explicit SocialCalc alias is available on the async file API.
XLSX.writeFile(book, 'book.dat', { bookType: 'socialcalc' });

const namedCells: ExcelJS.DefinedNameMatrixCell[] = [{ sheetName: 'Data', row: 1, col: 1 }];
const definedRanges = workbook.definedNames.getRanges('example', {
  forEach: callback => namedCells.forEach(callback),
  map: callback => namedCells.map(callback),
  findCellAt: (sheet, row, col) => namedCells.find(cell =>
    cell.sheetName === sheet && cell.row === row && cell.col === col),
});
void definedRanges;
workbook.definedNames.spliceRows('Data', 2, 1);
writer.definedNames.spliceColumns('Data', 3, 0, 2);
// @ts-expect-error The insertion count must be numeric.
workbook.definedNames.spliceColumns('Data', 3, 0, '2');

const worksheetModel: ExcelJS.WorksheetModel = workbook.getWorksheet('Data')!.model;
void worksheetModel.rows;
const workbookModel: ExcelJS.WorkbookModel = workbook.model;
const container = new ExcelJS.ModelContainer(workbookModel);
const loadedContainer: ExcelJS.ModelContainer = await container.xlsx.load(source);
void loadedContainer;
void workbookModel.worksheets;
workbook.model = { worksheets: [], sheets: [],
  media: [{ type: 'image', extension: 'png', filename: 'picture.png' }] };
workbook.getWorksheet('Data')!.getRow(1).model = {
  number: 1, cells: [{ address: 'A1', type: ExcelJS.ValueType.Number, value: 1 }],
};
workbook.getWorksheet('Data')!.model = {
  rows: [{ number: 1, cells: [{ address: 'A1', type: ExcelJS.ValueType.Number, value: 1 }] }],
  pageSetup: { orientation: 'landscape' },
  merges: ['A1:B1'],
  sheetProtection: worksheetModel.sheetProtection,
  tables: worksheetModel.tables,
};

function typecheckDestroyedHandles(
  row: ExcelJS.Row,
  cell: ExcelJS.Cell,
  streamRow: ExcelJS.StreamingRow,
  streamCell: ExcelJS.StreamingCell,
  readerRow: ExcelJS.StreamingReadRow,
  readerCell: ExcelJS.StreamingReadCell,
): void {
  for (const handle of [row, cell, streamRow, streamCell, readerRow, readerCell]) handle.destroy();
}
