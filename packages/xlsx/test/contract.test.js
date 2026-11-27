import { excelDateMillis, formulaErrorCode } from '../src/exceljs-cell-value.js';
import { tabColorAttributes } from '../src/exceljs-rich-text.js';
import { formulaA1 } from '../src/sheetjs-xlml-read.js';
import { format as ssfFormat, is_date as ssfIsDate, parse_date_code as ssfParseDateCode, get_table as ssfTable, load as ssfLoad, load_table as ssfLoadTable } from '../src/sheetjs-ssf.js';
import { formulaR1C1 } from '../src/sheetjs-xlml.js';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { address, copyInput, decodeBase64, optionsMatch, scalar } from '../src/contract.js';
import { createCsv } from '../src/exceljs-csv.js';
import { readCsvRows } from '../src/exceljs-csv-input.js';
import { streamingReaderOptions, streamingWorkbookReaderSettings, cellNoteMarginDefaults, cellNoteMarginsSettings, cellNoteSettings, csvWriteFileSettings, dataValidationSettings, gradientCenterSettings, gradientStopSettings, headerFooterSettings, isWorksheetState, pageMarginNames, pageMarginPatch, printMarginDefaults as excelMargins, pageSetupEntries, pageSetupSettings, printOptionEntries, printerSettingDefaults, printerSettingEntries, printerSettingNames, tableColumnSettings, streamingZipSettings, worksheetViewReadDefaults, worksheetViewSettings, zipCompressionSettings, zipDeflateSettings, zipLevelSettings, zipOutputSettings } from '../src/exceljs-options.generated.js';
import { compoundContainerSettings, compoundReadSettings, dbfReadSettings, earlyBiffReadSettings, isWorkbookVisibilityMetadata, isWorksheetVisibility, isWorksheetVisibilityEntry, validateWorksheetColumnLayout, workbookDateSystemSettings, xlsxWorkbookViewSettings, lotusReadSettings, marginKeys, printMarginDefaults as sheetMargins, textReadSettings, worksheetPrintMarginPatch, worksheetVisibilityCodes, xlsbReadSettings, xlsbWorkbookResourceSettings, xlsxReadSettings, xlsxSheetProjectionSettings, xlsxWorkbookResourceSettings } from '../src/sheetjs-options.generated.js';

import { set_readable, to_csv, to_html, to_json, to_xlml } from '../src/sheetjs-stream.js';
import { aoa_to_sheet, sheet_add_aoa, sheet_add_json } from '../src/sheetjs-input.js';
import { cell_add_comment, cell_set_hyperlink, cell_set_internal_link, cell_set_number_format, format_cell, sheet_get_cell, sheet_new, sheet_to_csv, sheet_to_formulae, sheet_to_html, sheet_to_json } from '../src/sheetjs-utils.js';
import { consts } from '../src/sheetjs-public-utils.js';
import { differentialPatternFillSettings, patternFillSettings } from '../src/exceljs-options.generated.js';
test('input views keep their window and are copied independently', () => {
  const source = Uint8Array.of(1, 2, 3, 4);
  const output = copyInput(source.subarray(1, 3));
  source[1] = 9;
  assert.deepEqual(output, Uint8Array.of(2, 3));
  assert.deepEqual(copyInput(output.buffer), output);
  assert.throws(() => copyInput(new DataView(output.buffer)), { code: 'INVALID_INPUT' });
});

test('addresses use canonical identity and XLSX limits', () => {
  assert.deepEqual(address('xfd1048576'), ['XFD1048576', 1048575, 16383]);
  assert.deepEqual(address('A1'), ['A1', 0, 0]);
  for (const value of ['XFE1', 'A1048577', 'A0', 'A01', 1]) {
    assert.throws(() => address(value), { code: 'INVALID_ADDRESS' });
  }
});

test('scalars preserve valid strings and reject lossy input before Wasm', () => {
  for (const value of ['', '日本語', '𠮷']) assert.equal(scalar(value, false), value);
  assert.ok(Object.is(scalar(-0, false), 0));
  assert.equal(scalar(false, true), false);
  for (const value of [NaN, Infinity, -Infinity, '\uD800', '\uDC00', null, false, {}]) {
    assert.throws(() => scalar(value, false), { code: 'INVALID_VALUE' });
  }
});

test('options are exact and getter properties are rejected without evaluation', () => {
  optionsMatch(undefined, {});
  optionsMatch({ type: 'array' }, { type: 'array' });
  assert.throws(() => optionsMatch(undefined, { type: 'array' }, false), {
    code: 'UNSUPPORTED_OPTIONS',
  });
  assert.throws(() => optionsMatch({
    get type() { assert.fail('getter invoked'); },
  }, { type: 'array' }), { code: 'UNSUPPORTED_OPTIONS' });
});

test('Base64 decoding preserves bytes across alphabets and whitespace', () => {
  const expected = Uint8Array.of(0xfb, 0xff, 0xfe, 0x00);
  assert.deepEqual(decodeBase64('+//+AA=='), expected);
  assert.deepEqual(decodeBase64('-__-AA'), expected);
  assert.deepEqual(decodeBase64('+//+\nAA=='), expected);
  assert.throws(() => decodeBase64('*'), { code: 'INVALID_INPUT' });
});

test('browser CSV reads the destination before resolving parser options', async () => {
  const events = [];
  const sheet = {
    addRows(rows) { assert.deepEqual(rows, [['A', '1:B']]); },
  };
  const csv = createCsv({
    addWorksheet(name) {
      assert.equal(name, 'Imported');
      events.push('addWorksheet');
      return sheet;
    },
  });
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode('A;B'));
      controller.close();
    },
  });
  const options = Object.create({
    get map() {
      events.push('map');
      return (value, index) => index ? `${index}:${value}` : value;
    },
    get sheetName() {
      events.push('sheetName');
      return 'Imported';
    },
    get parserOptions() {
      events.push('parserOptions');
      return { delimiter: ';' };
    },
  });
  assert.equal(await csv.read(stream, options), sheet);
  assert.deepEqual(events, ['map', 'sheetName', 'addWorksheet', 'parserOptions']);
});

test('browser CSV accepts a full weekday in a custom date format', async () => {
  let rows;
  const csv = createCsv({ addWorksheet: () => ({ addRows: values => { rows = values; } }) });
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode('2025-05-01 Thursday\n2025-05-01 Friday'));
      controller.close();
    },
  });
  await csv.read(stream, { dateFormats: ['YYYY-MM-DD dddd'] });
  assert.deepEqual(rows, [[new Date(2025, 4, 1)], ['2025-05-01 Friday']]);
});

test('browser CSV accepts whitespace around quoted fields across chunks', async () => {
  let rows;
  const csv = createCsv({ addWorksheet: () => ({ addRows: values => { rows = values; } }) });
  const stream = new ReadableStream({
    start(controller) {
      const bytes = new TextEncoder().encode('\u00a0"A"\u000b,B\r\n\u00a0plain,C');
      for (const byte of bytes) controller.enqueue(Uint8Array.of(byte));
      controller.close();
    },
  });
  await csv.read(stream);
  assert.deepEqual(rows, [['A', 'B'], ['\u00a0plain', 'C']]);
});

test('browser CSV distinguishes an empty leading field from later whitespace', async () => {
  let rows;
  const csv = createCsv({ addWorksheet: () => ({ addRows: values => { rows = values; } }) });
  const stream = new ReadableStream({
    start(controller) {
      const bytes = new TextEncoder().encode(' ,text, \r\n  \r\n  ');
      for (const byte of bytes) controller.enqueue(Uint8Array.of(byte));
      controller.close();
    },
  });
  await csv.read(stream, { map: value => value });
  assert.deepEqual(rows, [['', 'text', ' '], []]);
});

test('browser CSV custom dialect works at every byte split', async () => {
  const text = "#comment\r\n  'skip;me';ignore\r\n'日本\\'😀';'a\r\nb'\r\nlast;value";
  const bytes = new TextEncoder().encode(text);
  const options = { delimiter: ';', quote: "'", escape: "\\", comment: '#', skipRows: 1, maxRows: 1 };
  for (let split = 0; split <= bytes.length; split += 1) {
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(bytes.slice(0, split));
        controller.enqueue(bytes.slice(split));
        controller.close();
      },
    });
    assert.deepEqual(await readCsvRows(stream, options, value => value), [["日本'😀", 'a\r\nb']], `byte split ${split}`);
  }
});

test('CSV file encoding defaults only for nullish values', () => {
  for (const options of [undefined, {}, { encoding: undefined }, { encoding: null }]) {
    assert.deepEqual(csvWriteFileSettings(options), { encoding: 'utf8' });
  }
  for (const encoding of ['', 'latin1']) {
    assert.deepEqual(csvWriteFileSettings(Object.create({ encoding })), { encoding });
  }
});

test('printer read defaults preserve the field list and do not add write edits', () => {
  assert.deepEqual(Object.keys(printerSettingDefaults), printerSettingNames);
  assert.ok(Object.hasOwn(printerSettingDefaults, 'firstPageNumber'));
  assert.equal(printerSettingDefaults.firstPageNumber, undefined);
  assert.equal(printerSettingDefaults.horizontalDpi, 0xffffffff);
  assert.deepEqual(pageSetupSettings({ draft: true }, (_key, value) => value), { draft: true });
});

test('worksheet view read defaults cover view fields and leave tabSelected undeclared', () => {
  const fields = Object.keys(worksheetViewSettings({}));
  for (const key of Object.keys(worksheetViewReadDefaults)) assert.ok(fields.includes(key), key);
  assert.ok(fields.includes('tabSelected'));
  assert.equal(Object.hasOwn(worksheetViewReadDefaults, 'tabSelected'), false);
  assert.ok(Object.isFrozen(worksheetViewReadDefaults));
  assert.deepEqual(worksheetViewReadDefaults, {
    workbookViewId: 0, zoomScale: 100, showGridLines: true, rightToLeft: false,
    zoomScaleNormal: 100, showRuler: true, showRowColHeaders: true,
  });
});

test('differential pattern fills default an omitted pattern to solid; cell fills require one', () => {
  const color = { argb: 'FF00FF00' };
  for (const fill of [{ type: 'pattern', bgColor: color }, { type: 'pattern', pattern: undefined, bgColor: color }]) {
    assert.deepEqual(differentialPatternFillSettings(fill), { type: 'pattern', pattern: 'solid', fgColor: undefined, bgColor: color });
    assert.throws(() => patternFillSettings(fill), { code: 'UNSUPPORTED_OPTIONS' });
  }
  assert.equal(differentialPatternFillSettings({ type: 'pattern', pattern: 'gray125' }).pattern, 'gray125');
  assert.throws(() => differentialPatternFillSettings({ type: 'pattern', pattern: 'bogus' }), { code: 'UNSUPPORTED_OPTIONS' });
  assert.throws(() => differentialPatternFillSettings({ type: 'pattern', extra: 1 }), { code: 'UNSUPPORTED_OPTIONS' });
});

test('both adapters generate the same print-margin order and defaults', () => {
  assert.deepEqual(pageMarginNames, ['left', 'right', 'top', 'bottom', 'header', 'footer']);
  assert.deepEqual(pageMarginNames, marginKeys);
  assert.deepEqual(excelMargins, sheetMargins);
  assert.deepEqual(pageMarginNames.map(key => excelMargins[key]), [0.7, 0.7, 0.75, 0.75, 0.3, 0.3]);
});

test('SpreadsheetML stream validates stride lazily and batches rows', () => {
  set_readable(() => ({ chunks: [], push(value) { this.chunks.push(value); } }));
  const book = { SheetNames: ['Data'], Sheets: { Data: { '!ref': 'A1:A11' } } };
  const rowCounts = options => {
    const stream = to_xlml(book, options);
    const counts = [];
    while (stream.chunks.at(-1) !== null) {
      const start = stream.chunks.length;
      stream._read();
      const rows = stream.chunks.slice(start).filter(chunk => chunk?.startsWith('<Row'));
      for (const row of rows) assert.equal(row.match(/<Row\b/g).length, 1);
      if (rows.length) counts.push(rows.length);
    }
    return counts;
  };
  assert.deepEqual(rowCounts(undefined), [10, 1]);
  assert.deepEqual(rowCounts({ stride: null }), [10, 1]);
  assert.deepEqual(rowCounts({ stride: 0 }), [10, 1]);
  assert.deepEqual(rowCounts(Object.create({ stride: 5 })), [5, 5, 1]);
  for (const stride of [-1, 1.5, '2', Number.MAX_SAFE_INTEGER + 1]) {
    const stream = to_xlml(book, { stride });
    assert.throws(() => stream._read(), { code: 'INVALID_VALUE' });
  }
});

test('both worksheet visibility APIs share the persisted state codes', () => {
  assert.deepEqual(worksheetVisibilityCodes, { visible: 0, hidden: 1, veryHidden: 2 });
  assert.deepEqual(consts, { SHEET_VISIBLE: 0, SHEET_HIDDEN: 1, SHEET_VERY_HIDDEN: 2 });
  for (const [state, code] of Object.entries(worksheetVisibilityCodes)) {
    assert.ok(isWorksheetState(state));
    assert.ok(isWorksheetVisibility(code));
    assert.ok(!isWorksheetVisibility(state));
    assert.ok(!isWorksheetState(code));
  }
  for (const value of [-1, 3, 1.5, '1', null, NaN]) assert.ok(!isWorksheetVisibility(value));
});

test('fit-to-page is inferred only when creating a worksheet', () => {
  const project = (_key, value) => value;
  for (const [options, expected] of [
    [{ fitToWidth: 2 }, { fitToWidth: 2, fitToPage: true }],
    [{ fitToHeight: 2 }, { fitToHeight: 2, fitToPage: true }],
    [{ fitToWidth: 0 }, { fitToWidth: 0 }],
    [{ fitToHeight: 2, scale: 80 }, { fitToHeight: 2, scale: 80 }],
    [{ fitToWidth: 2, fitToPage: false }, { fitToWidth: 2, fitToPage: false }],
  ]) {
    assert.deepEqual(pageSetupSettings(options, project, true), expected);
    assert.deepEqual(pageSetupSettings(options, project), options);
  }
});

test('printer entries append an inferred page-number flag only when omitted', () => {
  assert.deepEqual(printerSettingEntries({ cellComments: 'asDisplyed', firstPageNumber: 3, copies: 2 }),
    ['cellComments', 'asDisplayed', 'firstPageNumber', '3', 'copies', '2', 'useFirstPageNumber', '1']);
  assert.deepEqual(printerSettingEntries({ firstPageNumber: 0 }),
    ['firstPageNumber', '0', 'useFirstPageNumber', '0']);
  assert.deepEqual(printerSettingEntries({ firstPageNumber: 3, useFirstPageNumber: false }),
    ['useFirstPageNumber', '0', 'firstPageNumber', '3']);
  assert.deepEqual(printerSettingEntries({ scale: 80 }), []);
});

test('numeric print patches preserve explicit false and leave omitted fields untouched', () => {
  assert.deepEqual(pageSetupEntries({ orientation: 'landscape', fitToHeight: 0, fitToPage: false }),
    Float64Array.of(1, NaN, NaN, NaN, 0, 0));
  assert.deepEqual(printOptionEntries({ showGridLines: false, verticalCentered: true }),
    Uint8Array.of(0, 2, 2, 1));
  assert.equal(pageSetupEntries({ copies: 2 }), undefined);
  assert.equal(printOptionEntries({ copies: 2 }), undefined);
});

test('header-footer patch distinguishes clears, false flags and omitted fields', () => {
  assert.deepEqual(headerFooterSettings({ oddHeader: 'Title', oddFooter: null, differentOddEven: false }), {
    values: ['Title', '', '', '', '', '', 'false', ''],
    changed: (1 << 0) | (1 << 1) | (1 << 6),
  });
  assert.equal(headerFooterSettings({
    get oddHeader() { assert.fail('getter invoked'); },
    oddFooter: undefined,
  }).changed, 0);
});

test('page margin patches preserve prior values and skip no-op updates', () => {
  const defaults = Float64Array.of(0.7, 0.7, 0.75, 0.75, 0.3, 0.3);
  assert.equal(pageMarginPatch({ left: 0.7 }, defaults), undefined);
  assert.deepEqual(pageMarginPatch(Object.defineProperty({ top: 0 }, 'right', { value: 1.5 }), defaults),
    Float64Array.of(0.7, 1.5, 0, 0.75, 0.3, 0.3));
  assert.throws(() => pageMarginPatch({
    get left() { assert.fail('getter invoked'); },
  }, defaults), { code: 'INVALID_VALUE' });
  assert.equal(defaults[1], 0.7);
});

test('SheetJS margin patches prefer original values and then declared defaults', () => {
  assert.deepEqual(worksheetPrintMarginPatch({ left: 0, top: null }),
    Float64Array.of(0, 0.7, 0.75, 0.75, 0.3, 0.3));
  const previous = Float64Array.of(1, 2, 3, 4, 5, 6);
  assert.equal(worksheetPrintMarginPatch({ left: null }, previous), undefined);
  assert.deepEqual(worksheetPrintMarginPatch({ top: 0 }, previous), Float64Array.of(1, 2, 0, 4, 5, 6));
  assert.throws(() => worksheetPrintMarginPatch({ top: -1 }, previous), { code: 'INVALID_VALUE' });
});

test('note field defaults preserve omission and validate explicit values', () => {
  assert.equal(cellNoteSettings({ texts: [{ text: 'note' }] }).editAs, undefined);
  assert.equal(cellNoteMarginsSettings({}).inset, undefined);
  assert.deepEqual(cellNoteMarginDefaults.inset, [0.13, 0.13, 0.25, 0.25]);
  assert.ok(Object.isFrozen(cellNoteMarginDefaults.inset));
  assert.throws(() => cellNoteMarginsSettings({ inset: [0, 0, 0, Infinity] }), { code: 'INVALID_VALUE' });
  assert.throws(() => cellNoteMarginsSettings({ insetmode: 'manual' }), { code: 'UNSUPPORTED_OPTIONS' });
});

test('input rules validate field values without evaluating accessors', () => {
  const formulae = [1, 10];
  assert.equal(dataValidationSettings({ type: 'whole', formulae, allowBlank: false }).formulae, formulae);
  const defaults = dataValidationSettings({ type: 'whole', formulae });
  assert.equal(defaults.operator, 'between');
  assert.equal(defaults.errorStyle, 'stop');
  assert.equal(defaults.showInputMessage, false);
  assert.equal(defaults.prompt, '');
  assert.throws(() => dataValidationSettings({ type: 'whole', errorStyle: 'invalid' }), { code: 'INVALID_VALUE' });
  assert.throws(() => dataValidationSettings({
    type: 'whole',
    get allowBlank() { assert.fail('the validator evaluated an accessor'); },
    formulae,
  }), { code: 'INVALID_VALUE' });
});

test('table columns require a formula for custom totals', () => {
  assert.equal(tableColumnSettings({ name: 'Amount' }).totalsRowFunction, 'none');
  assert.throws(() => tableColumnSettings({ name: 'Amount', totalsRowFunction: 'custom' }), { code: 'INVALID_VALUE' });
  const formula = 'SUM(Items[Amount])';
  assert.equal(tableColumnSettings({ name: 'Amount', totalsRowFunction: 'custom', totalsRowFormula: formula }).totalsRowFormula, formula);
});

test('gradient center defaults and gradient stop ranges are validated', () => {
  assert.deepEqual(gradientCenterSettings({ left: 0.2, top: 0.3 }),
    { left: 0.2, top: 0.3, right: 0.2, bottom: 0.3 });
  assert.throws(() => gradientCenterSettings({ left: 0.2, top: 0.3, right: 1.1 }), { code: 'INVALID_VALUE' });
  assert.throws(() => gradientStopSettings({
    get position() { assert.fail('the validator evaluated an accessor'); },
  }), { code: 'INVALID_VALUE' });
});

test('SheetJS array and JSON input preserve append origin and sparse fields', () => {
  const sheet = aoa_to_sheet([[1, undefined, 3]], { dense: true, origin: 'B2' });
  sheet_add_aoa(sheet, [[null, undefined, 4]], { origin: -1, sheetStubs: true });
  assert.equal(sheet['!ref'], 'A1:D3');
  assert.deepEqual(sheet['!data'][2], [{ t: 'z', v: null }, , { t: 'n', v: 4 }]);
  const header = ['c', 'c', 'a'];
  sheet_add_json(sheet, [{ a: 5, c: null, b: undefined }], {
    origin: -1, header, skipHeader: true, nullError: true,
  });
  assert.deepEqual(header, ['c', 'c', 'a', 'b']);
  assert.equal(sheet['!ref'], 'A1:D4');
  assert.deepEqual(sheet['!data'][3], [{ t: 'e', v: 0 }, , { t: 'n', v: 5 }, { t: 'z', v: undefined }]);
});

test('SheetJS JSON export combines duplicate headers, hidden cells and blank defaults', () => {
  const sheet = {
    '!ref': 'A1:D4',
    '!rows': [{}, {}, { hidden: true }],
    '!cols': [{}, { hidden: true }],
    A1: { t: 's', v: 'Name' }, B1: { t: 's', v: 'Hidden' },
    C1: { t: 's', v: 'Name' }, D1: { t: 's', v: 'Note' },
    A2: { t: 'n', v: 0, w: 'zero' }, D2: { t: 's', v: '' },
    A3: { t: 's', v: 'Skipped' },
  };
  const rows = sheet_to_json(sheet, { raw: false, skipHidden: true, defval: null });
  assert.deepEqual(rows, [{ Name: 'zero', Name_1: null, Note: '' }]);
  assert.equal(rows[0].__rowNum__, 1);
  assert.equal(Object.keys(rows[0]).includes('__rowNum__'), false);
});

test('SheetJS JSON export keeps a reserved header as an own data property', () => {
  const sheet = {
    '!ref': 'A1:A2', A1: { t: 's', v: '__proto__' }, A2: { t: 's', v: 'cell value' },
  };
  const [row] = sheet_to_json(sheet);
  assert.equal(Object.getPrototypeOf(row), Object.prototype);
  assert.equal(Object.hasOwn(row, '__proto__'), true);
  assert.equal(row.__proto__, 'cell value');
});

test('SheetJS CSV stream and utility agree with quoting, strip and hidden columns', () => {
  const sheet = {
    '!ref': 'A1:C4',
    '!cols': [{}, { hidden: true }],
    A1: { t: 's', v: 'ID' }, B1: { t: 's', v: 'omit' },
    A2: { t: 'n', v: 3, w: 'three' }, C2: { t: 's', v: 'semi;colon' },
    A4: { t: 'n', f: 'SUM(1,2)' }, C4: { t: 'z', v: null },
  };
  const options = { FS: ';', RS: '\r\n', skipHidden: true, strip: true, blankrows: false, rawNumbers: true };
  const expected = '"ID"\r\n3;"semi;colon"\r\n"=SUM(1,2)"';
  assert.equal(sheet_to_csv(sheet, options), expected);
  set_readable(() => ({ chunks: [], push(chunk) { this.chunks.push(chunk); } }));
  const stream = to_csv(sheet, options);
  while (stream.chunks.at(-1) !== null) stream._read();
  assert.equal(stream.chunks.join(''), '\uFEFF' + expected);
});

test('SheetJS formula export skips missing dense rows and emits array anchors', () => {
  const data = [];
  data[0] = [{ t: 'n', f: 'SUM(B1:B2)', F: 'A1:A2' }];
  data[1] = [{ t: 'n', F: 'A1:A2' }];
  data[9999] = [{ t: 's', w: 'Display', v: 'value' }];
  const sheet = { '!ref': 'A1:CV10000', '!data': data };
  assert.deepEqual(sheet_to_formulae(sheet), ['A1:A2=SUM(B1:B2)', "A10000='Display"]);
  assert.deepEqual(sheet_to_formulae(sheet, { values: false }), ['A1:A2=SUM(B1:B2)']);
});

test('SheetJS JSON and CSV preserve the requested dense blank rows', () => {
  const data = [[{ t: 's', v: 'Name' }]];
  data[3] = [{ t: 's', v: 'Ada' }];
  const sheet = { '!ref': 'A1:B4', '!data': data };
  assert.deepEqual(sheet_to_json(sheet, { defval: null }), [{ Name: 'Ada', __EMPTY: null }]);
  assert.deepEqual(sheet_to_json(sheet, { blankrows: true, defval: null }), [{}, {}, { Name: 'Ada', __EMPTY: null }]);
  assert.deepEqual(sheet_to_json(sheet, { header: 1, blankrows: false }), [['Name'], ['Ada']]);
  assert.equal(sheet_to_csv(sheet, { blankrows: false }), 'Name,\nAda,');
  assert.equal(sheet_to_csv(sheet), 'Name,\n,\n,\nAda,');
});

test('SheetJS JSON exports early spreadsheet dates and the leap-day serial', () => {
  const sheet = {
    '!ref': 'A1:A4',
    A1: { t: 'n', v: 59, z: 'yyyy-mm-dd hh:mm' },
    A2: { t: 'n', v: 59.5, z: 'yyyy-mm-dd hh:mm' },
    A3: { t: 'n', v: 60, z: 'yyyy-mm-dd hh:mm' },
    A4: { t: 'n', v: 61, z: 'yyyy-mm-dd hh:mm' },
  };
  assert.deepEqual(sheet_to_json(structuredClone(sheet), { header: 1, UTC: true }), [
    [new Date('1900-02-28T00:00:00Z')],
    [new Date('1900-02-28T12:00:00Z')],
    [60],
    [new Date('1900-03-01T00:00:00Z')],
  ]);
  assert.deepEqual(sheet_to_json(sheet, { header: 1, raw: false, UTC: true }), [
    ['1900-02-29 00:00'], ['1900-02-29 12:00'], ['1900-02-29 00:00'], ['1900-03-01 00:00'],
  ]);
});

test('SheetJS JSON applies defval to a missing starting dense row', () => {
  const sheet = { '!ref': 'B2:C4', '!data': [] };
  sheet['!data'][3] = [, { t: 's', v: 'last' }];
  assert.deepEqual(sheet_to_json(sheet, { header: 1, defval: null }), [
    [null, null],
    [],
    ['last', null],
  ]);
  assert.deepEqual(sheet['!data'][1], []);
  assert.equal(2 in sheet['!data'], false);
  assert.deepEqual(sheet_to_json(sheet, { header: 1, range: 2, defval: null }), [
    [null, null],
    ['last', null],
  ]);
});

test('SheetJS JSON keeps raw serials when the date format is a numeric ID', () => {
  const sheet = {
    '!ref': 'A1:B1',
    A1: { t: 'n', v: 25, z: 14 },
    B1: { t: 'n', v: 25, z: 'm/d/yy' },
  };
  assert.deepEqual(sheet_to_json(sheet, { header: 1, UTC: true }), [
    [25, new Date('1900-01-25T00:00:00Z')],
  ]);
  assert.deepEqual(sheet_to_json(sheet, { header: 1, raw: false, UTC: true }), [['1/25/00', '1/25/00']]);
});

test('SheetJS format_cell reuses a cached date display before applying dateNF', () => {
  const value = new Date('2024-01-02T00:00:00Z');
  const options = { dateNF: 'yyyy-mm-dd' };
  for (const w of ['cached', '']) {
    const cell = { t: 'd', v: value, w };
    assert.equal(format_cell(cell, undefined, options), w);
    assert.deepEqual(cell, { t: 'd', v: value, w });
  }
  const cell = { t: 'd', v: value };
  assert.equal(format_cell(cell, undefined, options), '2024-01-02');
  assert.deepEqual(cell, { t: 'd', v: value, z: 'yyyy-mm-dd', w: '2024-01-02' });
});

test('SheetJS unquoted calendar periods fall back to the default date display', () => {
  const cell = { t: 'd', v: new Date('2024-02-03T00:00:00Z') };
  assert.equal(format_cell(cell, undefined, { dateNF: 'dd.mm.yyyy' }), '2/3/24');
  assert.equal(cell.w, '2/3/24');
});

test('SheetJS format_cell applies a numeric display format to a Date serial', () => {
  const v = new Date('2024-01-02T12:00:00Z');
  for (const [style, expected] of [
    [{ z: '0.00' }, '45293.50'],
    [{ z: 'General' }, '45293.5'],
    [{ XF: { numFmtId: 2 } }, '45293.50'],
    [{ z: 'yyyy-mm-dd' }, '2024-01-02'],
  ]) {
    const cell = { t: 'd', v, ...style };
    assert.equal(format_cell(cell), expected);
    assert.equal(cell.w, expected);
    assert.equal(cell.v, v);
  }
});

test('SheetJS Date displays follow the 1900 leap-day boundary', () => {
  for (const [day, serial, calendar] of [
    ['1900-02-27', '58.5', '1900-02-27'],
    ['1900-02-28', '60.5', '1900-02-29'],
    ['1900-03-01', '61.5', '1900-03-01'],
  ]) {
    const v = new Date(day + 'T12:00:00Z');
    assert.equal(format_cell({ t: 'd', v, z: '0.0' }), serial);
    assert.equal(format_cell({ t: 'd', v, z: 'yyyy-mm-dd' }), calendar);
  }
  const v = new Date('1904-01-01T12:00:00Z');
  assert.equal(format_cell({ t: 'd', v, z: '0.0' }, undefined, { date1904: true }), '0.5');
});

test('SheetJS SSF formats Date serials and keeps General as a date display', () => {
  const value = new Date('1900-02-28T12:00:00Z');
  assert.equal(ssfFormat('0.0', value), '60.5');
  assert.equal(ssfFormat('@', value), '60.5');
  assert.equal(ssfFormat('yyyy-mm-dd', value), '1900-02-29');
  assert.equal(ssfFormat('General', value), '2/29/00');
  const epoch = new Date('1904-01-01T12:00:00Z');
  assert.equal(ssfFormat('0.0', epoch, { date1904: true }), '0.5');
});

test('SheetJS SSF uses General prefixes and empty table formats', () => {
  assert.equal(ssfFormat('General;0', -12.345), '-12.345');
  assert.equal(ssfFormat('GENERALxyz', new Date('2024-01-02T12:00:00Z')), '1/2/24');
  const date = new Date('2024-01-02T12:00:00Z');
  assert.equal(ssfFormat('General', date, { table: { 14: 'yyyy' } }), '2024');
  assert.equal(ssfFormat('General', date, { table: {} }), '45293.5');
  const options = { table: { 2: '', 60: '0.000' } };
  assert.equal(ssfFormat(2, 12.345, options), '');
  assert.equal(ssfFormat(2, true, options), 'TRUE');
  assert.equal(ssfFormat(2, 'text', options), 'text');
  assert.equal(ssfFormat(60, 12.345, options), '12.345');
  assert.equal(ssfFormat(27, 12.345, options), '1/12/00');
});

test('SheetJS conditional number formats select sections before taking the magnitude', () => {
  const values = [-12, -2, 0, 2, 12];
  assert.deepEqual(values.map(value => ssfFormat('[>10]"A"0;[<-10]"B"0', value)),
    ['B12', 'A2', 'A0', 'A2', 'A12']);
  assert.deepEqual(values.map(value => ssfFormat('[>10]"A"0;[<-10]"B"0;"C"0', value)),
    ['B12', 'C2', 'C0', 'C2', 'A12']);
  assert.equal(ssfFormat('[>=10]0.0;[Red]0', -1.25), '1');
  assert.equal(ssfFormat('[<0]0', -2), '-2');
});

test('SheetJS ignores fill spacing in numeric display while preserving literal stars', () => {
  assert.equal(ssfFormat('* 0', -12.3), '-12');
  assert.equal(ssfFormat('**0', 12.3), '12');
  assert.equal(ssfFormat('0* ', 12.3), '12');
  assert.equal(ssfFormat('0"* "', 12.3), '12* ');
  const cell = { t: 'n', v: 12.3, z: '* 0.00' };
  assert.equal(sheet_to_csv({ '!ref': 'A1', A1: cell }), '12.30');
  assert.equal(cell.w, '12.30');
});

test('SheetJS General sections keep the original signed value', () => {
  const format = '0;General;"zero"';
  assert.deepEqual([-1.25, 0, 1.25, 'text'].map(value => ssfFormat(format, value)),
    ['-1.25', 'zero', '1', 'text']);
  assert.equal(ssfFormat('0;0;General', 0), '0');
  const cell = { t: 'n', v: -1.25, z: format };
  assert.equal(format_cell(cell), '-1.25');
  assert.equal(cell.w, '-1.25');
});

test('SheetJS applies explicit currency symbols in locale-tagged number formats', () => {
  assert.equal(ssfFormat('[$€-407]#,##0.00', -12.34), '-€12.34');
  assert.equal(ssfFormat('[$USD-409]0.00', 12.34), 'USD12.34');
  assert.equal(ssfFormat('[$-409]0.00', 12.34), '$12.34');
  assert.equal(ssfFormat('[$€-407]0.00;([$€-407]0.00)', -12.34), '(€12.34)');
  const cell = { t: 'n', v: 12.34, z: '[$¥-411]#,##0' };
  assert.equal(format_cell(cell), '¥12');
  assert.equal(cell.w, '¥12');
});

test('SheetJS dates skip locale directives and retain quoted tokens', () => {
  const serial = 45293.5;
  assert.equal(ssfFormat('[$-409]yyyy-mm-dd', serial), '2024-01-02');
  assert.equal(ssfFormat('[$€-407]dd/mm/yyyy', serial), '02/01/2024');
  assert.equal(ssfFormat('[$-411]yyyy"年"m"月"d"日"', serial), '2024年1月2日');
  assert.equal(ssfFormat('[$-409]h:mm', serial), '12:00');
  assert.equal(ssfFormat('"[$-409]" yyyy-mm-dd', serial), '[$-409] 2024-01-02');
  const cell = { t: 'n', v: serial, z: '[$-F800]dddd, mmmm dd, yyyy' };
  assert.equal(format_cell(cell), 'Tuesday, January 02, 2024');
  assert.equal(cell.w, 'Tuesday, January 02, 2024');
});

test('SheetJS formats elapsed minutes, seconds, and standalone hours', () => {
  assert.equal(ssfFormat('[m]:ss', 1.5), '2160:00');
  assert.equal(ssfFormat('[s]', 0.9999999), '86400');
  assert.equal(ssfFormat('[hh]:mm', 1.5), '36:00');
  assert.equal(ssfFormat('h AM/PM', 0.123456), '2 AM');
  assert.equal(ssfFormat('h AM/PM', 0.9999999), '12 PM');
});

test('SheetJS clocks round to the requested fraction and carry into the next day', () => {
  const serial = seconds => 45293 + seconds / 86400;
  assert.equal(ssfFormat('h:mm:ss.0', serial(1.26)), '0:00:01.3');
  assert.equal(ssfFormat('h:mm:ss.0', serial(59.96)), '0:01:00.0');
  assert.equal(ssfFormat('h:mm:ss.00', serial(86399.96)), '23:59:59.96');
  assert.equal(ssfFormat('yyyy-mm-dd h:mm:ss.0', serial(86399.96)), '2024-01-03 0:00:00.0');
  assert.equal(ssfFormat('yyyy-mm-dd h:mm:ss.00', serial(86399.96)), '2024-01-02 23:59:59.96');
});

test('SheetJS elapsed time rounds fractions without wrapping at 24 hours', () => {
  const value = (86400 + 59.96) / 86400;
  assert.equal(ssfFormat('[h]:mm:ss.0', value), '24:01:00.0');
  assert.equal(ssfFormat('[m]:ss.00', value), '1440:59.96');
  assert.equal(ssfFormat('mm:ss.000', value), '00:59.960');
  assert.equal(ssfFormat('[s].0', value), '86460.0');
  assert.equal(ssfFormat('MMSS.0', 1.26 / 86400), '0001.3');
});

test('SheetJS short meridiem format preserves marker case and spacing', () => {
  assert.equal(ssfFormat('h:mm A/P', 0.125), '3:00 A');
  assert.equal(ssfFormat('h:mm a/p', 0.625), '3:00 p');
  assert.equal(ssfFormat('h:mmA/P', 0.625), '3:00P');
  assert.equal(ssfFormat('h:mm am/pm', 0.625), '3:00 PM');
  assert.equal(ssfFormat('yyyy-mm-dd h:mm A/P', 45293.625), '2024-01-02 3:00 P');
});

test('SheetJS date-time formats preserve quoted and escaped ISO separators', () => {
  const serial = 45293.625;
  assert.equal(ssfFormat('yyyy-mm-dd"T"hh:mm:ss', serial), '2024-01-02T15:00:00');
  assert.equal(ssfFormat('yyyy-mm-dd\\Thh:mm:ss', serial), '2024-01-02T15:00:00');
  assert.equal(ssfFormat('yyyy-mm-dd"T"hh:mm:ss.000"Z"', serial), '2024-01-02T15:00:00.000Z');
  assert.equal(ssfFormat('yyyy-mm-dd"T"hh:mm:ss.0"Z"', 45293 + 86399.96 / 86400), '2024-01-03T00:00:00.0Z');
});

test('SheetJS time fields respect one- and two-digit minute and second tokens', () => {
  const serial = (3600 + 4 * 60 + 5.26) / 86400;
  assert.equal(ssfFormat('m:ss', serial), '4:05');
  assert.equal(ssfFormat('m:s', serial), '4:5');
  assert.equal(ssfFormat('ss.0', serial), '05.3');
  assert.equal(ssfFormat('[h]:m:s', serial), '1:4:5');
  assert.equal(ssfFormat('[m]:s', serial), '64:5');
});

test('SheetJS is_date distinguishes scientific E from date tokens', () => {
  for (const format of ['g', '[ms]', '0.00e+00', 'general', '[$-409]h:mm', 'a/p']) {
    assert.equal(ssfIsDate(format), true, format);
  }
  for (const format of ['0.00E+00', 'General', 'GeNeRaL', '"g"', '[dh]', '[$USD-409]0.00']) {
    assert.equal(ssfIsDate(format), false, format);
  }
});

test('SheetJS parse_date_code preserves serial fields and calendar boundary results', () => {
  assert.deepEqual(ssfParseDateCode(60), {
    D: 60, T: 0, u: 0, y: 1900, m: 2, d: 29, H: 0, M: 0, S: 0, q: 3,
  });
  assert.deepEqual(ssfParseDateCode(60, undefined, true), {
    D: 60, T: 0, u: 0, y: 1317, m: 10, d: 29, H: 0, M: 0, S: 0, q: 3,
  });
  assert.deepEqual(ssfParseDateCode(0.9999999999, { date1904: true }), {
    D: 1, T: 0, u: 0, y: 1904, m: 1, d: 2, H: 0, M: 0, S: 0, q: 6,
  });
  assert.deepEqual(ssfParseDateCode(59.99999 / 86400), {
    D: 0, T: 59, u: 0, y: 1900, m: 1, d: 0, H: 0, M: 1, S: 0, q: 6,
  });
  assert.equal(ssfParseDateCode(2958465).y, 9999);
  assert.equal(ssfParseDateCode(2958465.0001), null);
  assert.equal(ssfParseDateCode(-1), null);
});

test('SheetJS SSF load reuses public registry holes and deduplicates automatic IDs', () => {
  const table = ssfTable();
  const previous = table[1];
  const outOfRange = 392;
  try {
    table[1] = null;
    assert.equal(ssfLoad('0.00000'), 1);
    ssfLoadTable({ 391: '0.000000', [outOfRange]: '0.0000000' });
    assert.equal(ssfLoad('0.000000'), 391);
    assert.equal(ssfFormat(391, 1.25), '1.250000');
    assert.equal(table[outOfRange], undefined);
  } finally {
    table[1] = previous;
    delete table[391];
  }
});

test('SheetJS formats fixed digit groups with literal and escaped separators', () => {
  assert.equal(ssfFormat('00000-0000', 12345), '00001-2345');
  assert.equal(ssfFormat('000-00-0000', 1234567890), '234-56-7890');
  assert.equal(ssfFormat('000\\-00\\-0000', -12345), '-000-01-2345');
  assert.equal(ssfFormat('00 00', 123456), '1234 56');
  assert.equal(ssfFormat('00-00', -1.6), '00-02');
  const cell = { t: 'n', v: 12345, z: '00000-0000' };
  assert.equal(sheet_to_csv({ '!ref': 'A1', A1: cell }), '00001-2345');
});

test('SheetJS phone formats support required and optional digit groups', () => {
  assert.equal(ssfFormat('(000) 000-0000', 12), '(000) 000-0012');
  assert.equal(ssfFormat('(000) 000-0000', 12345678901), '(234) 567-8901');
  assert.equal(ssfFormat('(000) 000-0000', -1234567890), '-(23) 456-7890');
  assert.equal(ssfFormat('(###) ###-####', 12), '() -12');
  assert.equal(ssfFormat('(###) ###-####', -1.5), '-() -1');
  assert.equal(ssfFormat('(###) ###-####', -0.2), '-() ');
});

test('SheetJS number formats support optional integer digits and zero suppression', () => {
  assert.equal(ssfFormat('#', -1.5), '-1');
  assert.equal(ssfFormat('###', 0), '');
  assert.equal(ssfFormat('##0', -0.125), '-0');
  assert.equal(ssfFormat('#00', 1), '01');
  assert.equal(ssfFormat('##0.00', -0.125), '-0.12');
  assert.equal(ssfFormat('#.##', 0), '.');
  assert.equal(ssfFormat('#.00', 0.125), '.13');
  assert.equal(ssfFormat('#.00', -0.125), '-0.12');
  assert.equal(ssfFormat('#,###', 1234.125), '1,234');
  assert.equal(ssfFormat('#,###', -0.125), '');
});

test('SheetJS optional digits work in positive, negative and zero sections', () => {
  assert.deepEqual([12.345, -12.345, 0].map(value => ssfFormat('##0.00;[Red](##0.00);"zero"', value)),
    ['12.35', '(12.35)', 'zero']);
  assert.deepEqual([1234, -1234, 0].map(value => ssfFormat('#,###;(#,###);"-"', value)),
    ['1,234', '(1,234)', '-']);
  assert.equal(ssfFormat('"Qty "###', 12), 'Qty 12');
});

test('SheetJS format_cell uses the cell style or General when an explicit number format fails', () => {
  const styled = { t: 'n', v: 12.345, z: 'QQQ', XF: { numFmtId: 2 } };
  const general = { t: 'n', v: 12.345, z: 'QQQ' };
  assert.equal(format_cell(styled), '12.35');
  assert.equal(format_cell(general), '12.345');
  assert.equal(styled.w, '12.35');
  assert.equal(general.w, '12.345');
  assert.equal(styled.z, 'QQQ');
});

test('SheetJS caches General output for an unknown numeric format ID', () => {
  const styled = { t: 'n', v: 1234567890123, XF: { numFmtId: 999 } };
  const explicit = { t: 'n', v: 1234567890123, z: 999 };
  const sheet = { '!ref': 'A1:B1', A1: styled, B1: explicit };
  assert.equal(sheet_to_csv(sheet), '1.23457E+12,1.23457E+12');
  assert.equal(styled.w, '1.23457E+12');
  assert.equal(explicit.w, '1.23457E+12');
  assert.deepEqual(sheet_to_json(sheet, { header: 1, raw: false }), [['1.23457E+12', '1.23457E+12']]);
});

test('SheetJS JSON caches formatted headers and values on their cells', () => {
  const header = { t: 's', v: 'Date' };
  const date = { t: 'd', v: new Date('2024-01-02T00:00:00Z') };
  const sheet = { '!ref': 'A1:A2', A1: header, A2: date };
  assert.deepEqual(sheet_to_json(sheet, { raw: false, dateNF: 'yyyy-mm-dd' }), [{ Date: '2024-01-02' }]);
  assert.equal(header.w, 'Date');
  assert.equal(date.z, 'yyyy-mm-dd');
  assert.equal(date.w, '2024-01-02');
  assert.deepEqual(sheet_to_json(sheet, { raw: false, dateNF: 'dd/mm/yyyy' }), [{ Date: '2024-01-02' }]);
});

test('SheetJS CSV caches displayed values and bypasses the cache for raw numbers', () => {
  const date = { t: 'd', v: new Date('2024-01-02T00:00:00Z') };
  const number = { t: 'n', v: 12, z: '0.00' };
  const sheet = { '!ref': 'A1:B1', A1: date, B1: number };
  assert.equal(sheet_to_csv(sheet, { dateNF: 'yyyy-mm-dd', rawNumbers: true }), '2024-01-02,12');
  assert.equal(date.z, 'yyyy-mm-dd');
  assert.equal(date.w, '2024-01-02');
  assert.equal(number.w, undefined);
  assert.equal(sheet_to_csv(sheet, { dateNF: 'dd/mm/yyyy' }), '2024-01-02,12.00');
  assert.equal(number.w, '12.00');
  assert.equal(sheet_to_csv(sheet, { rawNumbers: true }), '2024-01-02,12');
});

test('SheetJS HTML caches formatted numeric displays on their cells', () => {
  const cell = { t: 'n', v: 12, z: '0.00' };
  const sheet = { '!ref': 'A1', A1: cell };
  assert.match(sheet_to_html(sheet), />12\.00<\/td>/);
  assert.equal(cell.w, '12.00');
  cell.z = '0.0';
  assert.match(sheet_to_html(sheet), />12\.00<\/td>/);
});

test('SheetJS encode_range accepts the index supplied by Array.map', async () => {
  const { encode_range } = await import('../src/sheetjs-public-utils.js');
  const ranges = [
    { s: { r: 0, c: 0 }, e: { r: 0, c: 0 } },
    { s: { r: 1, c: 1 }, e: { r: 2, c: 2 } },
  ];
  assert.deepEqual(ranges.map(encode_range), ['A1', 'B2:C3']);
  assert.equal(encode_range('B2', 'C3'), 'B2:C3');
});

test('SheetJS JSON streams capture headers before reading current row values', () => {
  set_readable(() => ({ chunks: [], push(chunk) { this.chunks.push(chunk); } }));
  const header = { t: 's', v: 'Before' };
  const sheet = { '!ref': 'A1:A2', '!data': [[header], [{ t: 'n', v: 1 }]] };
  const readable = to_json(sheet);
  assert.equal(header.w, 'Before');
  sheet['!data'][0] = [{ t: 's', v: 'After' }];
  sheet['!data'][1] = [{ t: 'n', v: 2 }];
  readable._read();
  assert.deepEqual(readable.chunks, [{ Before: 2 }]);

  const missing = { '!ref': 'A1', '!data': [] };
  to_json(missing, { header: 1 });
  assert.deepEqual(missing['!data'], [[]]);
});

test('SheetJS JSON leaves dense rows untouched if the Readable factory fails', () => {
  const failure = new Error('Readable construction failed');
  const sheet = { '!ref': 'A1', '!data': [] };
  set_readable(() => { throw failure; });
  try {
    assert.throws(() => to_json(sheet), error => error === failure);
    assert.deepEqual(sheet['!data'], []);
  } finally {
    set_readable(() => ({ chunks: [], push(chunk) { this.chunks.push(chunk); } }));
  }
});

test('SheetJS CSV streams capture the export range before reading cell values', () => {
  set_readable(() => ({ chunks: [], push(chunk) { this.chunks.push(chunk); } }));
  const sheet = { '!ref': 'A1:B1', '!cols': [{ hidden: true }], B1: { t: 'n', v: 1 } };
  const readable = to_csv(sheet, { skipHidden: true });
  sheet['!ref'] = 'A1:A1';
  sheet['!cols'][0].hidden = false;
  sheet.B1.v = 2;
  readable._read();
  readable._read();
  readable._read();
  assert.deepEqual(readable.chunks, ['\uFEFF', '2', null]);
});

test('SheetJS HTML streams capture their range and wrapper before reading cells', () => {
  set_readable(() => ({ chunks: [], push(chunk) { this.chunks.push(chunk); } }));
  const sheet = { '!ref': 'A1:A1', A1: { t: 'n', v: 1 }, A2: { t: 'n', v: 3 } };
  const options = { header: '<main>', footer: '</main>', id: 'before' };
  const readable = to_html(sheet, options);
  assert.deepEqual(readable.chunks, ['<main>', '<table id="before">']);
  assert.equal(sheet.A1.w, undefined);
  sheet['!ref'] = 'A1:A2';
  sheet.A1.v = 2;
  options.header = options.footer = options.id = 'after';
  for (let i = 0; i < 5; i += 1) readable._read();
  const html = readable.chunks.join('');
  assert.match(html, /^<main><table id="before">/);
  assert.match(html, />2<\/td><\/tr><\/table><\/main>$/);
  assert.equal(html.match(/<tr>/g).length, 1);
});

test('SheetJS CSV and JSON streams signal empty input during construction', () => {
  set_readable(() => ({ chunks: [], push(chunk) { this.chunks.push(chunk); } }));
  for (const convert of [to_csv, to_json]) {
    for (const sheet of [null, {}, { '!ref': null }]) {
      const readable = convert(sheet);
      assert.deepEqual(readable.chunks, [null]);
      readable._read();
      assert.deepEqual(readable.chunks, [null]);
    }
  }
});

test('SpreadsheetML quantizes character and pixel column widths', () => {
  set_readable(() => ({ chunks: [], push(chunk) { this.chunks.push(chunk); } }));
  const sheet = { '!ref': 'A1:C1', '!cols': [{ width: 10.1 }, { wch: 10.2 }, { wpx: 30.2 }] };
  const readable = to_xlml({ SheetNames: ['Data'], Sheets: { Data: sheet } });
  while (readable.chunks.at(-1) !== null) readable._read();
  assert.deepEqual(readable.chunks.filter(chunk => chunk?.startsWith('<Column')), [
    '<Column ss:Index="1" ss:Width="61"/>',
    '<Column ss:Index="2" ss:Width="66"/>',
    '<Column ss:Index="3" ss:Width="30"/>',
  ]);
  assert.deepEqual(sheet['!cols'], [
    { width: 10.1, wpx: 61, wch: 9.33, MDW: 6 },
    { wch: 10.2, width: 11.03515625, wpx: 66, MDW: 6 },
    { wpx: 30.2, wch: 4.2, width: 5.03515625, MDW: 6 },
  ]);
});

test('SpreadsheetML caches the pixel height when writing row metadata', () => {
  set_readable(() => ({ chunks: [], push(chunk) { this.chunks.push(chunk); } }));
  const rows = [{ hpt: 14.5, hidden: true }, { hpt: 13, hpx: 18 }];
  const sheet = { '!ref': 'A1:A2', '!rows': rows };
  const output = to_xlml({ SheetNames: ['Data'], Sheets: { Data: sheet } });
  while (output.chunks.at(-1) !== null) output._read();
  assert.deepEqual(rows, [{ hpt: 14.5, hpx: 14.5, hidden: true }, { hpt: 13, hpx: 18 }]);
  assert.deepEqual(output.chunks.filter(chunk => chunk?.startsWith('<Row')), [
    '<Row ss:Index="1" ss:AutoFitHeight="0" ss:Height="14.5" ss:Hidden="1"></Row>',
    '<Row ss:Index="2" ss:AutoFitHeight="0" ss:Height="18"></Row>',
  ]);
});

test('SpreadsheetML formula conversion preserves structured reference names', () => {
  const formula = 'SUM(Table1[R1C1],Table2[A1],$A$1,A3,"R1C1")';
  const wire = 'SUM(Table1[R1C1],Table2[A1],R1C1,R[1]C[-1],"R1C1")';
  assert.equal(formulaR1C1(formula, 1, 1), wire);
  assert.equal(formulaA1(wire, 1, 1), formula);
});

test('ExcelJS tab colors validate only the selected color representation', () => {
  assert.deepEqual(tabColorAttributes({ argb: 'ff112233', theme: -1, tint: 2 }), ['rgb', 'FF112233']);
  assert.deepEqual(tabColorAttributes({ theme: 2, indexed: -1, tint: 0.5 }), ['theme', '2', 'tint', '0.5']);
  assert.deepEqual(tabColorAttributes({ indexed: 4, tint: 2 }), ['indexed', '4']);
  assert.deepEqual(tabColorAttributes({ tint: 2 }), ['auto', '1']);
  assert.throws(() => tabColorAttributes({ theme: 2, tint: 2 }), { code: 'INVALID_VALUE' });
});

test('ExcelJS formula error results are exact data properties', () => {
  assert.equal(formulaErrorCode({ error: '#DIV/0!' }), '#DIV/0!');
  assert.equal(formulaErrorCode({ error: '#UNKNOWN!' }), undefined);
  assert.equal(formulaErrorCode({ error: '#DIV/0!', extra: true }), undefined);
  assert.equal(formulaErrorCode(Object.create({ error: '#DIV/0!' })), undefined);
  assert.equal(formulaErrorCode({ get error() { throw new Error('getter was called'); } }), undefined);
});

test('ExcelJS formula dates use the native Date value', () => {
  const date = new Date('2025-03-15T00:00:00Z');
  date.getTime = () => { throw new Error('overridden method was called'); };
  assert.equal(excelDateMillis(date), Date.UTC(2025, 2, 15));
  assert.throws(() => excelDateMillis(new Date(NaN)), { code: 'INVALID_VALUE' });
});


test('streaming reader DSL validates modes without reading option accessors', () => {
  const options = Object.create({ styles: 'cache' }, {
    worksheets: { get() { assert.fail('Option accessor was called'); } },
  });
  assert.deepEqual(streamingWorkbookReaderSettings(options), {
    worksheets: 'emit', sharedStrings: 'cache', hyperlinks: 'ignore', styles: 'ignore', entries: 'ignore',
  });
  for (const [key, choices] of Object.entries(streamingReaderOptions)) {
    for (const value of choices) assert.equal(streamingWorkbookReaderSettings({ [key]: value })[key], value);
    assert.throws(() => streamingWorkbookReaderSettings({ [key]: null }), { code: 'INVALID_VALUE' });
  }
  assert.throws(() => streamingWorkbookReaderSettings({ [Symbol('unknown')]: true }), { code: 'UNSUPPORTED_OPTIONS' });
});

test('SheetJS cell edit helpers preserve the cell handle and unrelated fields', () => {
  for (const dense of [false, true]) {
    const sheet = sheet_new({ dense });
    const cell = sheet_get_cell(sheet, { r: 1, c: 2 });
    assert.equal(sheet_get_cell(sheet, 'C2'), cell);
    assert.equal(sheet_get_cell(sheet, 1, 2), cell);
    Object.assign(cell, { t: 'n', v: 12, w: 'cached' });
    assert.equal(cell_set_number_format(cell, '0.00'), cell);
    assert.equal(cell.w, 'cached');
    assert.equal(cell_set_hyperlink(cell, 'https://example.com/', 'External'), cell);
    assert.deepEqual(cell.l, { Target: 'https://example.com/', Tooltip: 'External' });
    assert.equal(cell_set_internal_link(cell, "'Other sheet'!A1", ''), cell);
    assert.deepEqual(cell.l, { Target: "#'Other sheet'!A1" });
    assert.equal(cell_set_hyperlink(cell, ''), cell);
    assert.equal(Object.hasOwn(cell, 'l'), false);
    assert.equal(cell_add_comment(cell, 'First', ''), undefined);
    const comments = cell.c;
    cell_add_comment(cell, 'Second', 'Reviewer');
    assert.equal(cell.c, comments);
    assert.deepEqual(comments, [{ t: 'First', a: 'SheetJS' }, { t: 'Second', a: 'Reviewer' }]);
    assert.equal(cell.v, 12);
    assert.equal(sheet['!ref'], undefined);
  }
});

test('SheetJS workbook resource projections respect bookSheets in both ZIP formats', () => {
  for (const [read, project] of [
    [xlsxReadSettings, xlsxWorkbookResourceSettings],
    [xlsbReadSettings, xlsbWorkbookResourceSettings],
  ]) {
    const requested = { bookFiles: true, bookVBA: true, bookDeps: true };
    assert.deepEqual(project(read(requested)), { files: true, vba: true, dependencies: true });
    assert.deepEqual(project(read({ ...requested, bookSheets: true })), {
      files: false, vba: false, dependencies: false,
    });
  }
  assert.equal(xlsbWorkbookResourceSettings(xlsbReadSettings({ bookFiles: 1 })).files, true);
  assert.throws(() => xlsxReadSettings({ bookFiles: 1 }), /Expected boolean worksheet projection options/);
});

test('SheetJS read accepts raw and WTF with any value in every format', () => {
  const values = [true, false, 1, 0, '', 'yes', null, undefined, {}];
  for (const value of values) {
    for (const read of [xlsxReadSettings, xlsbReadSettings, compoundReadSettings, earlyBiffReadSettings, lotusReadSettings, dbfReadSettings]) {
      assert.doesNotThrow(() => read({ raw: value, WTF: value }), read.name);
    }
    assert.doesNotThrow(() => textReadSettings({ raw: value, WTF: value }, 'plain'));
    for (const read of [xlsxReadSettings, xlsbReadSettings]) {
      assert.doesNotThrow(() => read({ password: value, PRN: value, FS: value, UTC: value }), read.name);
    }
    assert.equal(xlsxSheetProjectionSettings(xlsxReadSettings({ WTF: value })).WTF, Boolean(value));
  }
  assert.equal(xlsxSheetProjectionSettings(xlsxReadSettings({})).WTF, false);
  for (const [bookFiles, raw, expected] of [[true, 'yes', true], [true, 0, false], [false, true, false], [undefined, true, false]]) {
    assert.equal(compoundContainerSettings(compoundReadSettings({ bookFiles, raw })).raw, expected);
  }
  assert.throws(() => compoundReadSettings({ password: 'x' }), /Unsupported XLSX options/);
  assert.throws(() => xlsxReadSettings({ dense: 1, WTF: 1 }), /Expected boolean worksheet projection options/);
});

test('SheetJS write accepts the options a format does not read and documents them', async () => {
  const { fodsWriteSettings, odsWriteSettings, textWriteSettings, xlmlWriteSettings, xlsbWriteSettings, xlsxWriteSettings } =
    await import('../src/sheetjs-options.generated.js');
  const { writeOptionsByFormat } = await import('../sheetjs-option-contracts.mjs');
  const values = [true, false, 1, 0, '', 'x', null, undefined, {}];
  const accepts = (write, keys, seed = () => ({})) => {
    for (const value of values) {
      for (const key of keys) {
        const settings = seed();
        assert.doesNotThrow(() => write({ ...settings, [key]: value }, settings), `${write.name} ${key} ${String(value)}`);
      }
    }
  };
  accepts(xlsxWriteSettings, ['sheet', 'WTF', 'numbers', 'bookVBA', 'FS', 'RS']);
  accepts(xlsbWriteSettings, ['sheet', 'WTF', 'cellDates', 'cellStyles', 'sheetStubs', 'numbers', 'codepage', 'bookVBA', 'FS', 'RS']);
  const sheetOptions = ['sheet', 'bookSST', 'cellDates', 'Props', 'cellStyles', 'sheetStubs', 'ignoreEC', 'themeXLSX', 'WTF',
    'numbers', 'codepage', 'bookVBA', 'FS', 'RS'];
  accepts(odsWriteSettings, sheetOptions);
  accepts(fodsWriteSettings, [...sheetOptions, 'compression', 'password']);
  accepts(xlmlWriteSettings, [...sheetOptions.filter(key => key !== 'Props'), 'compression', 'password']);
  const unused = ['compression', 'bookSST', 'cellDates', 'Props', 'cellStyles', 'sheetStubs', 'ignoreEC', 'themeXLSX', 'WTF',
    'numbers', 'password', 'bookVBA'];
  for (const variant of ['csv', 'txt', 'html', 'prn', 'dif', 'sylk', 'eth', 'rtf', 'dbf', 'wk1', 'wk3', 'biff2', 'biff8', 'xls',
    'csv:file', 'html:file', 'biff5:file']) {
    const keys = ['csv', 'txt', 'csv:file'].includes(variant) ? unused : [...unused, 'FS', 'RS'];
    for (const key of keys) {
      assert.doesNotThrow(() => textWriteSettings({ [key]: 'x' }, variant), `${variant} ${key}`);
    }
  }
  for (const [write, key] of [[xlsxWriteSettings, 'password'], [xlsbWriteSettings, 'password'], [xlsbWriteSettings, 'ignoreEC'],
    [xlsbWriteSettings, 'themeXLSX'], [odsWriteSettings, 'password']]) {
    assert.throws(() => write({ [key]: 'x' }, {}), { code: 'UNSUPPORTED_OPTIONS' }, `${write.name} ${key}`);
  }
  assert.throws(() => textWriteSettings({ type: 'x' }, 'csv:file'), { code: 'UNSUPPORTED_OPTIONS' });
  // Declared options without effect are exactly those the format contracts accept beyond their own keys.
  assert.deepEqual(Object.keys(writeOptionsByFormat.xlsb), ['sheet', 'WTF', 'cellDates', 'cellStyles', 'sheetStubs', 'numbers',
    'codepage', 'bookVBA', 'FS', 'RS']);
  const generator = fileURLToPath(new URL('../generate-sheetjs-options.mjs', import.meta.url));
  const reference = execFileSync(process.execPath, [generator, '--reference'], { encoding: 'utf8' });
  const table = format => reference.split(`### Write options without effect for ${format}\n`)[1]?.split('\n\n#')[0];
  assert.ok(table('xlsb').includes('| `cellDates` | — | — | Accepted, any value. No effect: only the 0.20.3 XLSX writer reads it. |'));
  assert.ok(table('ods').includes('| `sheet` | — | — | Every sheet is written. A truthy value that names no sheet or indexes past SheetNames throws "Sheet not found", as in 0.20.3. |'));
  assert.match(table('xlsx / xlsm / xlam'), /\| `sheet` \|.*writeXLSX and writeFileXLSX do not check it\. \|/);
  assert.match(table('xlsx / xlsm / xlam'), /\| `WTF` \| — \| — \| Accepted, any value\. With a truthy value 0\.20\.3 throws on a worksheet range beyond A1:XFD1048576/);
  assert.ok(table('fods').includes('| `password` | — | — | Accepted, any value. No effect: 0.20.3 does not encrypt output. |'));
  assert.ok(table('csv / txt').includes('| `Props` | — | — | Accepted, any value. No effect: 0.20.3 merges options.Props only into XLSX, XLSB and SpreadsheetML properties.'));
  assert.ok(!table('csv / txt').includes('| `FS` |'));
  assert.ok(table('biff5 / biff8 / xls / xla').includes('| `compression` | — | — | Accepted, any value. No effect: 0.20.3 compresses only ZIP output (XLSX, XLSB, ODS). |'));
  assert.ok(!table('xlsx / xlsm / xlam').includes('| `password` |'));
});

test('ExcelJS streaming ZIP store uses a generated boolean default', () => {
  assert.equal(streamingZipSettings({}).store, false);
  assert.equal(streamingZipSettings({ store: undefined }).store, false);
  assert.equal(streamingZipSettings({ store: true }).store, true);
  assert.throws(() => streamingZipSettings({ store: null }), { code: 'INVALID_VALUE' });
});

test('ExcelJS ZIP compression projection handles mixed case and defaults', () => {
  for (const [compression, compressed] of [[undefined, true], ['Deflate', true], ['sToRe', false]]) {
    assert.deepEqual(zipCompressionSettings(zipOutputSettings({ compression })), { compressed });
  }
  assert.throws(() => zipOutputSettings({ compression: 'GZIP' }), /Expected ZIP compression DEFLATE or STORE/);
});

test('ExcelJS ZIP projection preserves the default DEFLATE level for legacy sentinels', () => {
  for (const [level, deflateLevel] of [[undefined, undefined], [-1, 6], [0, 6], [1, 1], [9, 9]]) {
    assert.deepEqual(zipDeflateSettings(zipLevelSettings({ level })), { deflateLevel });
  }
  assert.throws(() => zipLevelSettings({ level: 10 }), /Expected a DEFLATE level/);
});

test('generated references describe format-specific validation and reader event timing', () => {
  const generator = fileURLToPath(new URL('../generate-sheetjs-options.mjs', import.meta.url));
  const reference = execFileSync(process.execPath, [generator, '--reference'], { encoding: 'utf8' });
  const table = reference.split('### Write option validation for text and legacy formats\n')[1]?.split('\n\n##')[0];
  assert.ok(table, 'Missing text and legacy write validation table');
  assert.ok(table.includes('| `dateNF` | Optional string | — | Accepted for `csv`, `txt` |'));
  const xlsxRead = reference.split('## SheetJS `read(data, options)` for XLSX\n')[1]?.split('\n\n##')[0];
  assert.ok(xlsxRead, 'Missing XLSX read validation table');
  assert.match(xlsxRead, /first failed check in this order is reported:\n`dense` → `cellNF` → `cellText`/);
  assert.ok(xlsxRead.includes('`dateNF` → `sheetRows` → `bookSheets` → `sheets`.'));
  const xlsbRead = reference.split('## SheetJS `read(data, options)` for XLSB\n')[1]?.split('\n\n##')[0];
  assert.ok(xlsbRead, 'Missing XLSB read validation table');
  for (const readTable of [xlsxRead, xlsbRead]) {
    const selectorRow = readTable.split('\n').find(line => line.startsWith('| `sheets` |'));
    assert.ok(selectorRow?.includes('case-insensitive name or zero-based index'));
    assert.ok(selectorRow.includes('SheetNames keeps the full list'));
  }
  assert.match(xlsbRead, /\| `bookSheets` .*With bookProps, returns property records and sheet names/);
  assert.match(xlsbRead, /\| `bookProps` .*SheetNames is included when bookSheets is also truthy/);
  assert.ok(xlsbRead.includes('| `nodim` | — | — | Accepted. The declared worksheet dimensions are retained. |'));
  assert.ok(xlsbRead.includes('| `cellHTML` | — | — | Accepted. XLSB rich-text runs are exposed without generating HTML. |'));
  assert.match(xlsxRead, /\| `WTF` \| — \| — \| enabled for truthy values, With cellStyles, a theme fill color also reports its untinted theme color as raw_rgb\. With a truthy value 0\.20\.3 throws/);
  for (const readTable of [xlsxRead, xlsbRead]) {
    assert.ok(readTable.includes('| `raw` | — | — | Accepted, any value. No effect: 0.20.3 uses it for text input and compound-file headers. |'));
    assert.ok(readTable.includes('| `password` | — | — | Accepted, any value. No effect: 0.20.3 uses it only to decrypt compound files. |'));
  }
  assert.match(xlsbRead, /\| `WTF` \| — \| — \| Accepted, any value\. With a truthy value 0\.20\.3 throws/);
  const xlsRead = reference.split('## SheetJS `read(data, options)` for compound XLS (BIFF5/8)\n')[1]?.split('\n\n##')[0];
  assert.match(xlsRead, /\| `raw` \| — \| — \| Accepted, any value\. With a truthy bookFiles, a truthy value adds the compound-file header and sectors as cfb\.raw\. \|/);
  assert.match(xlsRead, /\| `WTF` \| — \| — \| Accepted, any value\./);
  assert.ok(reference.includes('The workbook notification precedes publication of model and properties.'));
  assert.ok(reference.includes(
    '| `workbook.xlsx.write options.zip` | `compression` (optional string, choices ["DEFLATE","STORE"] (case-insensitive), default "DEFLATE" when omitted or undefined)'));
  assert.ok(reference.includes(
    '| `workbook.xlsx.write options.zip.compressionOptions` | `level` (optional integer, range -1 to 9, -1 and 0 use DEFLATE level 6) |'));
  assert.ok(reference.includes('`reader.properties.model.date1904`'));
  assert.ok(reference.includes('`XLSX cell.l`'));
  assert.ok(reference.includes('an own `display: undefined` removes it'));
});

test('XLSX comments and cells accept the read projection fields r, h and T:false', async () => {
  const { isXlsxCellShape, xlsxCellCommentSettings } = await import('../src/sheetjs-options.generated.js');
  const { plainStringHtml, richStringHtml } = await import('../src/sheetjs-cell-html.js');
  assert.deepEqual(xlsxCellCommentSettings({ a: 'A', t: 'x', r: '<t>x</t>', h: 'x', T: false }),
    { a: 'A', t: 'x', r: '<t>x</t>', h: 'x', T: false });
  for (const T of [true, 1, 'yes']) {
    assert.throws(() => xlsxCellCommentSettings({ a: 'A', t: 'x', T }), { code: 'UNSUPPORTED_CELL' });
  }
  assert.throws(() => xlsxCellCommentSettings({ a: 'A', t: 'x', ID: 'id' }), { code: 'INVALID_VALUE' });
  assert.ok(isXlsxCellShape({ t: 's', v: 'x', r: '<t>x</t>', h: 'x' }));
  assert.equal(plainStringHtml('a&b<c>"\'\n\t\r'), 'a&amp;b&lt;c&gt;&quot;&apos;<br/>&#x0009;&#x000d;');
  const plainRun = ['p', ...Array(13).fill('')];
  const boldRun = ['b', 'Calibri', '11', 'true', 'false', 'false', '', '', '', '', '', '', '', '0'];
  assert.equal(richStringHtml([...boldRun, ...plainRun, '', ...Array(13).fill('')]), '<span style=""><b>b</b></span>p');
});

test('SheetJS XLSX write accepts 0.20.3 read-back workbook metadata and raw column keys', () => {
  assert.ok(isWorkbookVisibilityMetadata({ Sheets: [], AppVersion: {}, WBView: [], CalcPr: {}, xmlns: '', Views: [] }));
  assert.equal(isWorkbookVisibilityMetadata({ Unknown: 1 }), false);
  assert.ok(isWorksheetVisibilityEntry({ name: 'A', sheetId: '1', sheetid: '1', id: 'rId1', Hidden: 0, CodeName: 'Sheet1' }));
  assert.equal(workbookDateSystemSettings({ date1904: true, filterPrivacy: false, CodeName: '', showObjects: 'all' }).date1904, true);
  for (const [key, value] of [['filterPrivacy', true], ['CodeName', 'X'], ['showObjects', 'none'], ['defaultThemeVersion', 1]]) {
    assert.throws(() => workbookDateSystemSettings({ [key]: value }), { code: 'UNSUPPORTED_STRUCTURE' }, key);
  }
  assert.deepEqual(xlsxWorkbookViewSettings({ RTL: true, zoom: 80 }), { RTL: true, zoom: 80 });
  assert.throws(() => xlsxWorkbookViewSettings({ RTL: 'yes' }), { code: 'INVALID_VALUE' });
  validateWorksheetColumnLayout({ width: null, customwidth: '1', style: '1', bestFit: '1', bestfit: '1', outlineLevel: '1', outlinelevel: '1', collapsed: '1', phonetic: '1' });
  assert.throws(() => validateWorksheetColumnLayout({ customWidth: '1' }), { code: 'UNSUPPORTED_STRUCTURE' });
});
