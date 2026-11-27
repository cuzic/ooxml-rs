import { Workbook } from '../src/exceljs.js';
import * as SheetJS from '../src/sheetjs.js';
import { XlsxDocument } from '../src/runtime.js';

const writeOptions = { bookType: 'xlsx', type: 'array' };

function assert(value, message) {
  if (!value) throw new Error(message);
}

function throws(action, message) {
  try { action(); } catch (error) { return error; }
  throw new Error(message);
}

function bytesEqual(left, right) {
  const a = new Uint8Array(left);
  const b = new Uint8Array(right);
  return a.length === b.length && a.every((byte, index) => byte === b[index]);
}

async function fixture(name) {
  const response = await fetch('../fixtures/' + name + '.xlsx');
  assert(response.ok, 'missing fixture ' + name);
  return new Uint8Array(await response.arrayBuffer());
}

export async function properties() {
  const isoFormula = await fixture('iso-formula');
  const datedFormula = SheetJS.read(isoFormula, { cellDates: true });
  const dateResult = datedFormula.Sheets.Sheet1.A1;
  assert(dateResult.f === 'DATE(2024,2,3)' && dateResult.t === 'd' &&
    dateResult.v.toISOString() === '2024-02-03T04:05:06.789Z', 'ISO formula date result');
  assert(SheetJS.read(isoFormula).Sheets.Sheet1.A1.t === 'n', 'ISO formula numeric result');
  assert(bytesEqual(isoFormula, SheetJS.write(datedFormula, writeOptions)), 'ISO formula result no-op bytes');
  const ids = await new Workbook().xlsx.load(await fixture('sheet-ids'));
  assert(ids.worksheets.map(({ id }) => id).join(',') === '9,4', 'worksheet IDs and order');
  assert(ids.getWorksheet() === ids.worksheets[1], 'default worksheet lookup uses lowest ID');
  assert(ids.getWorksheet(9) === ids.getWorksheet('Sheet1'), 'worksheet lookup identity across ID and name');
  assert(ids.getWorksheet(1) === undefined && ids.getWorksheet('9') === undefined, 'missing ID and name');
  const visited = [];
  ids.eachSheet((sheet, id) => visited.push([sheet.name, id]));
  assert(JSON.stringify(visited) === '[["Sheet1",9],["Other",4]]', 'eachSheet order and IDs');
  const input = await fixture('scalar');
  const wrapped = new Uint8Array(input.length + 8);
  wrapped.set(input, 4);
  const view = wrapped.subarray(4, 4 + input.length);
  const excel = new Workbook();
  const load = excel.xlsx.load(view);
  assert(load instanceof Promise && await load === excel, 'ExcelJS load contract');
  assert(excel.getWorksheet('sheet1') === undefined, 'name lookup must be exact');
  assert(excel.worksheets.map(({ name }) => name).join(',') === 'Sheet1,Other', 'worksheet order');
  const sheet = excel.getWorksheet('Sheet1');
  assert(excel.worksheets !== excel.worksheets && sheet === excel.worksheets[0], 'worksheet identity');
  const a1 = sheet.getCell('A1');
  assert(a1 === sheet.getCell('a1') && a1.value === 1, 'cell identity and read');
  const row = sheet.findRow(1);
  assert(row === sheet.getRow(1) && row.number === 1, 'loaded row identity');
  assert(row.getCell('A') === a1 && sheet.getCell(1, 1) === a1, 'cell overload identity');
  assert(row.findCell(2) === sheet.getCell('B1'), 'find loaded cell');
  assert(sheet.findRow(7) === undefined && sheet.findCell(7, 2) === undefined, 'find does not create');
  assert(sheet.getRows(7, 2)[0] === sheet.findRow(7), 'getRows creates row handles');
  assert(sheet.getRows(7, 0) === undefined, 'zero-length getRows');
  const otherSheet = excel.getWorksheet('Other');
  assert(otherSheet.rowCount === 3 && otherSheet.actualRowCount === 2, 'sparse row counts');
  const rowNumbers = [];
  otherSheet.eachRow({ includeEmpty: true }, (_, number) => rowNumbers.push(number));
  assert(rowNumbers.join(',') === '1,2,3', 'empty row iteration');
  const lastRow = otherSheet.lastRow;
  assert(lastRow.cellCount === 3 && lastRow.actualCellCount === 1, 'sparse cell counts');
  const columns = [];
  lastRow.eachCell({ includeEmpty: true }, (_, column) => columns.push(column));
  assert(columns.join(',') === '1,2,3' && lastRow.values[3] === 9, 'sparse row values');
  assert(otherSheet.getSheetValues()[3][3] === 9, 'sparse sheet values');


  assert(sheet.getCell('B1').value === false && sheet.getCell('C1').value === 'plain', 'scalar reads');
  assert(sheet.getCell('J8').value === null, 'empty read');
  assert(bytesEqual(input, await excel.xlsx.writeBuffer()), 'ExcelJS no-op bytes');

  const book = SheetJS.read(view, { type: 'array' });
  assert(!(book instanceof Promise), 'SheetJS read must be synchronous');
  const projected = book.Sheets.Sheet1;
  assert(projected['!ref'] === 'A1:J8' && projected.J8 === undefined, 'SheetJS range and empty');
  assert(projected.B1.v === false && projected.C1.v === 'plain', 'SheetJS scalar reads');
  assert(bytesEqual(input, SheetJS.write(book, writeOptions)), 'SheetJS no-op bytes');

  for (const cellDates of [false, true]) {
    const dateCell = SheetJS.read(view, { cellDates, cellText: true, cellNF: true, dateNF: 'yyyy/mm/dd' })
      .Sheets.Sheet1.J1;
    assert(dateCell.z === 'm/d/yy' && dateCell.w === '2020/01/01', 'dateNF built-in format override');
  }
  for (const [coordinate, text] of [['D1', 'rich'], ['I2', 'inline rich']]) {
    assert(projected[coordinate].v === text && projected[coordinate].r.includes('<r>'), 'SheetJS rich text ' + coordinate);
    assert(sheet.getCell(coordinate).value.richText[0].text === text, 'ExcelJS rich text ' + coordinate);
  }
  assert(projected.E1.f === 'A1+1' && projected.E1.v === 2, 'SheetJS formula and result');
  const formula = sheet.getCell('E1');
  assert(formula.value.formula === 'A1+1' && formula.result === 2, 'ExcelJS formula and result');
  assert(projected.F1.F === 'F1:F3' && projected.F1.f === 'ROW(F1:F3)' &&
    projected.F2.F === 'F1:F3' && projected.F2.v === 2, 'SheetJS array formula projection');
  assert(sheet.getCell('F1').value.shareType === 'array' && sheet.getCell('F2').value === 2,
    'ExcelJS array formula projection');
  for (const coordinate of ['F1', 'F2']) {
    throws(() => { sheet.getCell(coordinate).value = 9; }, 'unguarded array edit ' + coordinate);
  }
  assert(projected.H1.v === 43831 && sheet.getCell('H1').value.toISOString() === '2020-01-01T00:00:00.000Z',
    'ISO date projection');
  assert(projected.I1.t === 'e' && projected.I1.v === 42 && sheet.getCell('I1').value.error === '#N/A',
    'error cell projection');
  assert(projected.J1.t === 'n' && projected.J1.v === 43831, 'SheetJS date serial');
  assert(sheet.getCell('J1').value.toISOString() === '2020-01-01T00:00:00.000Z', 'ExcelJS date-formatted cell');
  throws(() => { sheet.getCell('J1').value = 9; }, 'ExcelJS date-formatted cell accepted an edit');
  assert(({ ...projected }).F2.v === 2, 'spread lost an array follower');
  for (const coordinate of ['F3', 'G1', 'G2']) {
    assert(!Object.keys(projected).includes(coordinate), 'virtual cell materialized');
    throws(() => projected[coordinate], 'unguarded virtual read ' + coordinate);
    if (coordinate === 'F3') {
      throws(() => { sheet.getCell(coordinate).value = 9; }, 'unguarded virtual edit ' + coordinate);
    } else {
      assert(sheet.getCell(coordinate).master === sheet.getCell('G1'), 'merged cell did not resolve master');
    }
  }
  assert(bytesEqual(input, await excel.xlsx.writeBuffer()), 'rejected advanced writes changed bytes');

  for (const name of ['absent', 'undersized', 'empty']) {
    const original = await fixture(name);
    const other = SheetJS.read(original);
    const grid = other.Sheets.Other;
    assert(grid.C3 === undefined, name + ' projected a cell outside its range');
    assert(grid['!ref'] === ({ absent: undefined, undersized: 'A1', empty: 'A1:C3' })[name], name + ' extent');
    assert(bytesEqual(original, SheetJS.write(other, writeOptions)), name + ' changed original bytes');
  }
  const spillInput = await fixture('dynamic');
  const spill = await new Workbook().xlsx.load(spillInput);
  const spilledCell = spill.worksheets[0].getCell('D2');
  assert(spilledCell.value === 2, 'dynamic-array cached result was lost');
  throws(() => { spilledCell.value = 9; }, 'dynamic-array result accepted an edit');
  assert(bytesEqual(spillInput, await spill.xlsx.writeBuffer()), 'spill refusal changed bytes');

  const incomplete = await fixture('incomplete');
  const incompleteBook = SheetJS.read(incomplete);
  throws(() => incompleteBook.Sheets.Other, 'incomplete classification opened');
  assert(bytesEqual(incomplete, SheetJS.write(incompleteBook, writeOptions)), 'incomplete no-op bytes');

  for (const address of ['A0', 'XFE1', 'A1048577']) {
    throws(() => sheet.getCell(address), 'invalid address was accepted');
  }
  projected.A1.v = 15;
  const other = book.Sheets.Other;
  other.Z99 = { t: 'n', v: 4 };
  throws(() => SheetJS.write(book, writeOptions), 'cross-sheet refusal');
  delete other.Z99;
  projected.A1.v = 1;
  assert(bytesEqual(input, SheetJS.write(book, writeOptions)), 'reverted pending changes');
  projected.A1.v = 15;
  other.B2 = { t: 's', v: '𠮷' };
  const sheetOutput = SheetJS.write(book, writeOptions);
  const sheetReopened = SheetJS.read(sheetOutput);
  assert(sheetReopened.Sheets.Sheet1.A1.v === 15 && sheetReopened.Sheets.Other.B2.v === '𠮷', 'cross-sheet retry');
  assert(sheetReopened.Sheets.Other['!ref'] === 'A1:C3', 'write expanded !ref');

  a1.value = 16;
  const committed = await excel.xlsx.writeBuffer();
  const originalWrite = XlsxDocument.prototype.write_scalar;
  const originalSave = XlsxDocument.prototype.to_bytes;
  try {
    for (const step of ['registry', 'target']) {
      XlsxDocument.prototype.write_scalar = function (...args) {
        this.inject_apply_failure(step);
        return originalWrite.apply(this, args);
      };
      const error = throws(() => { a1.value = 'new shared string'; }, 'apply failure was ignored');
      assert(/poison/i.test(error.message), 'expected poisoned apply failure: ' + error);
      assert(bytesEqual(committed, await excel.xlsx.writeBuffer()) && a1.value === 16, 'apply failure changed commit');
    }
    XlsxDocument.prototype.write_scalar = originalWrite;
    XlsxDocument.prototype.to_bytes = () => { throw Error('injected serialization failure'); };
    throws(() => { a1.value = 17; }, 'serialization failure was ignored');
    assert(bytesEqual(committed, await excel.xlsx.writeBuffer()) && a1.value === 16, 'serialization failure changed commit');
  } finally {
    XlsxDocument.prototype.write_scalar = originalWrite;
    XlsxDocument.prototype.to_bytes = originalSave;
  }

  a1.value = '';
  assert(a1.value === '', 'empty string was lost');
  a1.value = '𠮷';
  assert(a1.value === '𠮷', 'supplementary character was lost');
  a1.value = -0;
  assert(Object.is(a1.value, 0), 'signed zero was not normalized');
  for (const value of [NaN, Infinity, '\uD800']) {
    throws(() => { a1.value = value; }, 'invalid scalar was accepted');
  }
  await excel.xlsx.load(input);
  throws(() => a1.value, 'old handle remained live');
  assert(excel.getWorksheet('Sheet1').getCell('A1').value === 1, 'reload did not reset generation');
  const { AtomicXlsxDocument } = await import('../wasm/ooxml_xlsx_wasm.js');
  if (AtomicXlsxDocument) {
    const candidate = new AtomicXlsxDocument(input);
    try {
      candidate.write_scalar('Sheet1', 0, 0, 16);
      assert(bytesEqual(candidate.to_bytes(), committed), 'Rust candidate differs from reference commit');
      for (const step of ['registry', 'target', 'serialize']) {
        const error = throws(() => candidate.write_with_failure('Sheet1', 0, 0, 'staged text', step),
          'Rust candidate ignored ' + step + ' failure');
        assert(step === 'serialize' ? /serialization failure/.test(error.message) : /poison/i.test(error.message),
          'wrong Rust candidate failure: ' + error);
        assert(bytesEqual(candidate.to_bytes(), committed), 'Rust candidate changed committed bytes after ' + step);
        assert(candidate.read_scalar('Sheet1', 0, 0) === 16, 'Rust candidate changed committed read view');
      }
      candidate.write_scalar('Sheet1', 0, 0, 22);
      assert(candidate.read_scalar('Sheet1', 0, 0) === 22, 'Rust candidate could not recover');
    } finally {
      candidate.free();
    }
  }
  return { sheetOutput: [...new Uint8Array(sheetOutput)], committed: [...committed] };
}
