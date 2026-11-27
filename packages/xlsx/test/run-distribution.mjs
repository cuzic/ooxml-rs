// Run from the scratch test project after bundle.mjs and make-fixtures.py.
// Set OOXML_PROFILE_SETTERS=1 to time each Wasm phase of a scalar update.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

const require = createRequire(resolve(process.cwd(), 'package.json'));
const { chromium, firefox } = require('playwright');
const [engine, origin] = process.argv.slice(2);
assert.ok(engine === 'chromium' || engine === 'firefox');
const type = engine === 'firefox' ? firefox : chromium;
const browser = await type.launch({ executablePath: process.env.OOXML_BROWSER_BINARY, timeout: 15000 });

async function passing(page, path) {
  await page.goto(origin + path);
  await page.waitForFunction(() => document.body.innerText !== 'Running');
  assert.equal(await page.locator('body').innerText(), 'PASS');
}

try {
  const page = await browser.newPage();
  for (const adapter of ['exceljs-reference', 'exceljs', 'sheetjs-reference', 'sheetjs', 'both']) await passing(page, '/bundles.html?' + adapter);
  await passing(page, '/deployment/bundles.html?both');
  await passing(page, '/reference.html');
  assert.deepEqual(await page.evaluate(() => globalThis.xlsxReference), {
    sheetjs: '0.20.3',
    sheetOutputArrayBuffer: true,
    sheetOffset: 1,
    dimension: ['A1', 9, 'A1:C3', 9],
    excelOutputUint8Array: true,
    excelOutputArrayBuffer: false,
    excelPromises: true,
    excelLoadIdentity: true,
    excelExactName: true,
    excelCellIdentity: true,
    excelOldHandleValue: 1,
    excelNewWorksheet: true,
  });
  const parsedContainers = await page.evaluate(async () => {
    const SheetJS = await import('./node_modules/@ooxml-rs/xlsx/src/sheetjs-browser.js');
    const Reference = await import('./node_modules/xlsx/xlsx.mjs');
    if (SheetJS.default.CFB !== SheetJS.CFB || SheetJS.CFB.version !== Reference.CFB.version) {
      throw new Error('Browser CFB export mismatch');
    }
    for (const fileType of ['cfb', 'zip', 'mad']) {
      const archive = SheetJS.CFB.utils.cfb_new();
      SheetJS.CFB.utils.cfb_add(archive, 'data', new TextEncoder().encode('日本語'));
      const encoded = SheetJS.CFB.write(archive, { fileType, type: 'base64', compression: true });
      const actual = SheetJS.CFB.read(encoded, { type: 'base64' });
      const expected = Reference.CFB.read(encoded, { type: 'base64' });
      const decode = (api, container) => new TextDecoder().decode(Uint8Array.from(api.find(container, 'data').content));
      if (decode(SheetJS.CFB, actual) !== '日本語' || decode(Reference.CFB, expected) !== '日本語') {
        throw new Error('Browser CFB interoperability failed for ' + fileType);
      }
    }
    const workbook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[23, '値']]), 'Data');
    SheetJS.utils.book_append_sheet(workbook, SheetJS.utils.aoa_to_sheet([[42]]), 'Other');
    for (const api of [SheetJS, Reference]) {
      const addin = api.utils.book_new(api.utils.aoa_to_sheet([[23, '値']]), 'Data');
      api.utils.book_append_sheet(addin, api.utils.aoa_to_sheet([[42]]), 'Other');
      const xla = api.write(addin, { bookType: 'xla', type: 'binary' });
      const container = Reference.CFB.read(xla, { type: 'binary' });
      const bytes = Uint8Array.from(Reference.CFB.find(container, '/Workbook').content);
      const view = new DataView(bytes.buffer);
      const next = 4 + view.getUint16(2, true);
      if (view.getUint16(next, true) !== 0x0087 || view.getUint16(next + 2, true) !== 0 ||
          Reference.read(xla, { type: 'binary' }).Sheets.Other.A1.v !== 42) {
        throw new Error('XLA add-in marker or workbook content differs from the reference');
      }
    }
    const dbfBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([
      ['Name', 'Amount', 'Active', 'Date'], ['alpha', 23.5, true, new Date(2020, 2, 4)],
      ['beta', -4, false, new Date(2021, 2, 4)],
    ], { cellDates: true }));
    const dbfOptions = { bookType: 'dbf', type: 'binary' };
    const dbfOutput = SheetJS.write(dbfBook, dbfOptions);
    if (dbfOutput !== Reference.write(dbfBook, dbfOptions)) throw new Error('DBF bytes differ from reference');
    if (Reference.read(dbfOutput, { type: 'binary' }).Sheets.Sheet1.C3.v !== false) {
      throw new Error('DBF reference reread failed');
    }
    const dbfImported = SheetJS.read(dbfOutput, { type: 'binary', cellDates: true });
    const dbfExpected = Reference.read(dbfOutput, { type: 'binary', cellDates: true });
    if (dbfImported.Sheets.Sheet1.D2.v.getTime() !== dbfExpected.Sheets.Sheet1.D2.v.getTime() ||
        !dbfImported.Sheets.Sheet1['!cols'].every(({ DBF }, index) =>
          Object.keys(DBF).every(key => DBF[key] === dbfExpected.Sheets.Sheet1['!cols'][index].DBF[key]))) {
      throw new Error('DBF import differs from reference');
    }
    const dbfBytes = Uint8Array.from(dbfOutput, character => character.charCodeAt(0));
    const dbfHeader = new DataView(dbfBytes.buffer).getUint16(8, true);
    dbfBytes[dbfHeader] = 0x2a;
    const filteredDbf = SheetJS.read(dbfBytes, { sheetRows: 2, dense: true });
    if (filteredDbf.Sheets.Sheet1['!data'][1][0].v !== 'beta') throw new Error('DBF included a deleted row');
    const wk1Book = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([
      ['Name', 4], ['日本語', 42023.5, true], [new Date('2020-03-04T12:00Z')],
    ], { cellDates: true }), 'Original');
    const wk1Options = { bookType: 'wk1', type: 'binary' };
    const wk1 = SheetJS.write(wk1Book, wk1Options);
    if (wk1 !== Reference.write(wk1Book, wk1Options)) throw new Error('WK1 bytes differ from reference');
    const wk1Read = { type: 'binary', dense: true, cellDates: true };
    const values = api => api.read(wk1, { ...wk1Read }).Sheets.Sheet1['!data']
      .map(row => row.map(cell => [cell.t, cell.v, cell.z]));
    if (JSON.stringify(values(SheetJS)) !== JSON.stringify(values(Reference))) {
      throw new Error('WK1 read projection differs from reference');
    }
    const tokens = [1, 0, 0, 0, 0, 5, 2, 0, 9, 3];
    const formulaRecord = new Uint8Array(19 + tokens.length);
    const formulaView = new DataView(formulaRecord.buffer);
    formulaView.setUint16(0, 0x10, true);
    formulaView.setUint16(2, 15 + tokens.length, true);
    formulaView.setUint8(4, 0xff);
    formulaView.setUint16(7, 3, true);
    formulaView.setFloat64(9, 6, true);
    formulaView.setUint16(17, tokens.length, true);
    formulaRecord.set(tokens, 19);
    const formulaBytes = wk1.slice(0, -4) + String.fromCharCode(...formulaRecord) + wk1.slice(-4);
    const formula = SheetJS.read(formulaBytes, { type: 'binary' }).Sheets.Sheet1.A4;
    const expectedFormula = Reference.read(formulaBytes, { type: 'binary' }).Sheets.Sheet1.A4;
    if (formula.f !== '$A$1+2' || formula.f !== expectedFormula.f || formula.v !== expectedFormula.v ||
        SheetJS.read(formulaBytes, { type: 'binary', cellFormula: false }).Sheets.Sheet1.A4.f !== undefined) {
      throw new Error('WK1 formula reading differs from reference');
    }
    const wk3Book = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([['Name', 4], ['日本語', 42023.5]]), 'First');
    SheetJS.utils.book_append_sheet(wk3Book, SheetJS.utils.aoa_to_sheet([[0, 23.5, 'Second']]), '第二');
    const wk3Options = { bookType: 'wk3', type: 'binary' };
    const wk3 = SheetJS.write(wk3Book, wk3Options);
    if (wk3 !== Reference.write(wk3Book, wk3Options)) throw new Error('WK3 bytes differ from reference');
    const wk3Read = { type: 'binary', dense: true };
    const wk3Values = api => {
      const book = api.read(wk3, { ...wk3Read });
      return [book.SheetNames, book.SheetNames.map(name => book.Sheets[name]['!data'])];
    };
    if (JSON.stringify(wk3Values(SheetJS)) !== JSON.stringify(wk3Values(Reference))) {
      throw new Error('WK3 read projection differs from reference');
    }
    const fractions = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[0.1, -23.5, Number.MIN_VALUE]]));
    const reopenedFractions = SheetJS.read(SheetJS.write(fractions, wk3Options), wk3Read).Sheets.Sheet1['!data'][0];
    if (reopenedFractions[0].v !== 0.1 || reopenedFractions[1].v !== -23.5 || reopenedFractions[2].v !== Number.MIN_VALUE) {
      throw new Error('WK3 floating-point round trip changed a value');
    }
    const biffBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([
      ['Name', 4], ['é日本€', -23.5, true], [new Date('2020-03-04T12:00Z')],
    ], { cellDates: true }), 'Original');
    SheetJS.utils.cell_add_comment(biffBook.Sheets.Original.A1, 'A note');
    for (const bookType of ['biff2', 'biff3', 'biff4']) {
      const options = { bookType, type: 'binary' };
      const bytes = SheetJS.write(biffBook, options);
      if (bytes !== Reference.write(biffBook, { ...options })) throw new Error(bookType + ' bytes differ from reference');
      const readOptions = { type: 'binary', cellDates: true, cellNF: true };
      const actual = SheetJS.read(bytes, readOptions).Sheets.Sheet1;
      const expected = Reference.read(bytes, { ...readOptions }).Sheets.Sheet1;
      const cells = sheet => [sheet.A1.v, sheet.B1.v, sheet.A2.v, sheet.B2.v, sheet.C2.v, sheet.A3.v];
      if (JSON.stringify(cells(actual)) !== JSON.stringify(cells(expected)) || actual.A1.c[0].t !== 'A note') {
        throw new Error(bookType + ' cell or note mismatch');
      }
    }
    const propertyXls = Reference.utils.book_new(Reference.utils.aoa_to_sheet([[7]]), 'Metadata');
    propertyXls.Props = { Title: '売上', Author: 'Ada', Company: 'Research', CreatedDate: new Date('2026-01-01T00:00Z') };
    propertyXls.Custprops = { Count: 4, Published: true, When: new Date('2026-01-02T00:00Z') };
    const propertyBinary = Reference.write(propertyXls, { bookType: 'biff8', type: 'binary' });
    for (const bookSheets of [false, true]) {
      const options = { type: 'binary', bookProps: true, bookSheets, bookFiles: true };
      const actual = SheetJS.read(propertyBinary, { ...options });
      const expected = Reference.read(propertyBinary, { ...options });
      const properties = book => JSON.stringify(Object.entries(book.Props)
        .filter(([key]) => key !== 'Title').sort(([a], [b]) => a.localeCompare(b)));
      if (actual.Props !== actual.Custprops || actual.Props.Title !== '売上' || properties(actual) !== properties(expected) ||
          JSON.stringify(actual.SheetNames) !== JSON.stringify(expected.SheetNames) || actual.Sheets !== undefined ||
          !(actual.Props.When instanceof Date) || !actual.cfb) {
        throw new Error('BIFF8 document properties differ from reference');
      }
    }
    for (const bookType of ['biff8', 'xls']) {
      const bytes = SheetJS.write(propertyXls, { bookType, type: 'binary' });
      const exported = SheetJS.read(bytes, { type: 'binary', bookProps: true, WTF: true });
      const reopened = Reference.read(bytes, { type: 'binary', WTF: true });
      if (exported.Props.Title !== '売上' || exported.Props.When.toISOString() !== '2026-01-02T00:00:00.000Z' ||
          reopened.Props.Author !== 'Ada' || reopened.Props.Company !== 'Research' ||
          reopened.Props.CreatedDate !== '2026-01-01T00:00:00Z' || reopened.Props.Count !== 4 ||
          reopened.Props.Published !== true || reopened.Sheets.Metadata.A1.v !== 7) {
        throw new Error(bookType + ' document property export failed');
      }
    }
    const headingsBook = Reference.utils.book_new(Reference.utils.aoa_to_sheet([[1]]), 'Data');
    headingsBook.Props = {
      SheetNames: ['Alpha', 'Beta'], DefinedNames: ['Total'], ChartNames: ['Trend'],
      Worksheets: 99,
    };
    const headings = SheetJS.write(headingsBook, { bookType: 'biff8', type: 'binary' });
    for (const api of [SheetJS, Reference]) {
      const { Props } = api.read(headings, { type: 'binary', bookProps: true, WTF: true });
      const view = [Props.Worksheets, Props.SheetNames, Props.NamedRanges, Props.DefinedNames, Props.Chartsheets, Props.ChartNames];
      if (JSON.stringify(view) !== JSON.stringify([2, ['Alpha', 'Beta'], 1, ['Total'], 1, ['Trend']])) {
        throw new Error('BIFF8 document heading arrays did not round-trip');
      }
    }
    const badProperties = SheetJS.CFB.read(propertyBinary, { type: 'binary' });
    SheetJS.CFB.utils.cfb_del(badProperties, '/\u0005SummaryInformation');
    SheetJS.CFB.utils.cfb_add(badProperties, '\u0005SummaryInformation', Uint8Array.of(0, 0, 0, 0));
    const relaxedProperties = SheetJS.parse_xlscfb(badProperties, { bookProps: true });
    if (relaxedProperties.Props.Title !== undefined || relaxedProperties.Props.Company !== 'Research') {
      throw new Error('Invalid BIFF8 summary stream affected other document properties');
    }
    let propertyDiagnostic;
    try {
      SheetJS.parse_xlscfb(badProperties, { bookProps: true, WTF: true });
    } catch (error) {
      propertyDiagnostic = error;
    }
    if (propertyDiagnostic?.code !== 'INVALID_INPUT' || !propertyDiagnostic.message.includes('SummaryInformation')) {
      throw new Error('BIFF8 WTF did not report the invalid summary stream');
    }
    const xlsBook = Reference.utils.book_new(Reference.utils.aoa_to_sheet([
      ['Name', 42023.5, true, new Date('2020-03-04T12:00Z')],
    ], { cellDates: true }), 'First');
    Reference.utils.book_append_sheet(xlsBook, Reference.utils.aoa_to_sheet([[0, 'Other']]), 'Second');
    for (const bookType of ['biff5', 'biff8']) {
      const binary = Reference.write(xlsBook, { bookType, type: 'binary' });
      const options = { type: 'binary', cellDates: true, cellNF: true };
      const actual = SheetJS.read(binary, options);
      const expected = Reference.read(binary, { ...options });
      const values = book => [book.SheetNames, ['A1', 'B1', 'C1', 'D1'].map(address => book.Sheets.First[address].v)];
      if (JSON.stringify(values(actual)) !== JSON.stringify(values(expected))) {
        throw new Error(bookType + ' import differs from reference');
      }
      const cfb = SheetJS.CFB.read(binary, { type: 'binary' });
      const projected = SheetJS.parse_xlscfb(cfb, { sheets: 'Second', bookFiles: true });
      if (projected.Sheets.First || projected.Sheets.Second.B1.v !== 'Other' || projected.cfb !== cfb) {
        throw new Error(bookType + ' CFB sheet projection failed');
      }
      const rawOptions = { type: 'binary', bookFiles: true, bookSheets: true, cellHTML: false };
      const raw = SheetJS.read(binary, { ...rawOptions });
      const rawReference = Reference.read(binary, { ...rawOptions });
      if (JSON.stringify(raw.cfb.FullPaths) !== JSON.stringify(rawReference.cfb.FullPaths)) {
        throw new Error(bookType + ' compound paths differ from reference');
      }
      const exported = SheetJS.write(xlsBook, { bookType, type: 'binary', bookSST: bookType === 'biff8' });
      const reopened = Reference.read(exported, { ...options });
      if (JSON.stringify(values(reopened)) !== JSON.stringify(values(expected)) || reopened.Sheets.Second.B1.v !== 'Other') {
        throw new Error(bookType + ' export did not reopen in reference');
      }
    }
    for (const bookType of ['biff5', 'biff8']) {
      const wide = bookType === 'biff8';
      const { record, joinRecords } = await import('./node_modules/@ooxml-rs/xlsx/src/sheetjs-binary-records.js');
      const label = (name, scope, tokens, flags = 0) =>
        record(0x18, 14 + Number(wide) + name.length + tokens.length, (view, bytes) => {
          view.setUint16(4, flags, true);
          bytes[7] = name.length;
          view.setUint16(8, tokens.length, true);
          view.setUint16(wide ? 12 : 10, scope, true);
          bytes.set(new TextEncoder().encode(name), wide ? 19 : 18);
          bytes.set(tokens, bytes.length - tokens.length);
        });
      const nameToken = wide ? [0x43, 2, 0, 0, 0] : [0x43, 2, 0, ...new Array(12).fill(0)];
      const labels = joinRecords([
        ...(wide ? [record(0x01ae, 4, view => {
          view.setUint16(4, 2, true);
          view.setUint16(6, 0x0401, true);
        }), record(0x0017, 8, view => {
          view.setUint16(4, 1, true);
        })] : []),
        label('Constant', 0, [0x1e, 42, 0]),
        label('_xlfn.TOTAL', 0, [0x1e, 8, 0]),
        label('Alias', 0, nameToken),
        label('Local', 1, wide ? [0x44, 0, 0, 0, 0] : [0x44, 0, 0, 0]),
        label('\x06', 1, [0x1e, 7, 0], 0x20),
        ...(wide ? [label('\x0d', 1, [0x3b, 0, 0, 0, 0, 2, 0, 0, 0, 1, 0], 0x20)] : []),
      ]);
      const file = Reference.write(xlsBook, { bookType, type: 'binary' });
      const archive = SheetJS.CFB.read(file, { type: 'binary' });
      const name = wide ? 'Workbook' : 'Book';
      const data = Uint8Array.from(SheetJS.CFB.find(archive, '/' + name).content);
      const view = new DataView(data.buffer);
      let offset = 0;
      let lastSheet = 0;
      while (view.getUint16(offset, true) !== 0x0a) {
        if (view.getUint16(offset, true) === 0x85) {
          const position = view.getUint32(offset + 4, true);
          lastSheet = Math.max(lastSheet, position);
          view.setUint32(offset + 4, position + labels.length, true);
        }
        offset += 4 + view.getUint16(offset + 2, true);
      }
      let sheetEnd = lastSheet;
      while (view.getUint16(sheetEnd, true) !== 0x0a) {
        sheetEnd += 4 + view.getUint16(sheetEnd + 2, true);
      }
      const formula = record(0x06, 22 + nameToken.length, (recordView, output) => {
        recordView.setUint16(4, 1, true); // A2
        recordView.setFloat64(10, 8, true);
        recordView.setUint16(24, nameToken.length, true);
        output.set(nameToken, 26);
      });
      SheetJS.CFB.utils.cfb_del(archive, '/' + name);
      SheetJS.CFB.utils.cfb_add(archive, name, joinRecords([
        data.subarray(0, offset), labels, data.subarray(offset, sheetEnd), formula, data.subarray(sheetEnd),
      ]));
      const bytes = SheetJS.CFB.write(archive, { type: 'binary', fileType: 'cfb' });
      for (const xlfn of [false, true]) {
        const projected = api => api.read(bytes, { type: 'binary', xlfn }).Workbook.Names;
        const actual = projected(SheetJS);
        if (JSON.stringify(actual) !== JSON.stringify(projected(Reference)) ||
            actual.find(name => name.Name === 'Alias')?.Ref !== 'TOTAL') {
          throw new Error(bookType + ' defined names differ from reference');
        }
      }
      for (const cellFormula of [false, true]) {
        const settings = { type: 'binary', cellFormula };
        for (const api of [SheetJS, Reference]) {
          const cell = api.read(bytes, { ...settings }).Sheets.Second.A2;
          if (cell.v !== 8 || cell.f !== (cellFormula ? 'TOTAL' : undefined)) {
            throw new Error(bookType + ' named cell formula differs from reference');
          }
        }
      }
      if (wide) {
        const filter = api => api.read(bytes, { type: 'binary' }).Sheets.First['!autofilter']?.ref;
        if (filter(SheetJS) !== 'A1:B3' || filter(Reference) !== 'A1:B3' ||
            SheetJS.read(bytes, { type: 'binary', sheets: 'Second' }).Sheets.First !== undefined) {
          throw new Error('BIFF8 filter database scope differs from reference');
        }
        const first = SheetJS.read(bytes, { type: 'binary' });
        first.Workbook.Names.find(name => name.Name === 'Local').Comment = '日本語 & 😀';
        const rewritten = SheetJS.write(first, { bookType: 'biff8', type: 'binary' });
        for (const api of [SheetJS, Reference]) {
          const saved = api.read(rewritten, { type: 'binary' });
          const refs = Object.fromEntries(saved.Workbook.Names.map(name => [name.Name, name.Ref]));
          if (saved.Sheets.First['!autofilter']?.ref !== 'A1:B3' ||
              refs.Constant !== 42 || refs.Local !== '$A$1' || refs['_xlnm.Print_Area'] !== 7 ||
              saved.Workbook.Names.find(name => name.Name === 'Local')?.Comment !== '日本語 & 😀' ||
              saved.Workbook.Names.filter(name => name.Name === '_xlnm._FilterDatabase').length !== 1) {
            throw new Error('BIFF8 defined names or autofilter did not survive re-export');
          }
        }
      }
    }
    const namedCellBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([
      [{ t: 'n', v: 3, f: 'Rate+1' }, { t: 'n', v: 8, f: '合計' }],
    ]), 'Local');
    SheetJS.utils.book_append_sheet(namedCellBook, SheetJS.utils.aoa_to_sheet([[{ t: 'n', v: 1, f: 'Rate' }, { t: 'n', v: 2, f: 'Local!Rate' }]]), 'Global');
    namedCellBook.Workbook = { Names: [
      { Name: 'Unsupported', Ref: 'UNKNOWNFUNC(1)' },
      { Name: 'DroppedAlias', Ref: 'Unsupported+1' },
      { Name: 'ForwardAlias', Ref: 'Rate+1' },
      { Name: 'Rate', Ref: 1 },
      { Name: 'Rate', Ref: 2, Sheet: 0, Comment: 'local' },
      { Name: '合計', Ref: 8 },
    ] };
    const namedCells = SheetJS.write(namedCellBook, { bookType: 'biff8', type: 'binary' });
    for (const api of [SheetJS, Reference]) {
      const opened = api.read(namedCells, { type: 'binary' });
      if (opened.Sheets.Local.A1.f !== 'Rate+1' || opened.Sheets.Local.B1.f !== '合計' ||
          opened.Sheets.Global.A1.f !== 'Rate' ||
          opened.Sheets.Global.B1.f !== (api === SheetJS ? 'Local!Rate' : 'Rate') || opened.Workbook.Names.length !== 4 ||
          (api === SheetJS && opened.Workbook.Names[0].Ref !== 'Rate+1')) {
        throw new Error('BIFF8 named cell formulas did not survive re-export');
      }
    }
    const namedContainer = SheetJS.CFB.read(namedCells, { type: 'binary' });
    const namedStream = Uint8Array.from(SheetJS.CFB.find(namedContainer, '/Workbook').content);
    const namedView = new DataView(namedStream.buffer);
    const tokenIndices = [];
    for (let offset = 0; offset + 4 <= namedStream.length;) {
      if (namedView.getUint16(offset, true) === 0x06 && namedStream[offset + 26] === 0x43) {
        tokenIndices.push(namedView.getUint32(offset + 27, true));
      }
      offset += 4 + namedView.getUint16(offset + 2, true);
    }
    if (tokenIndices.join() !== '3,4,2,3') throw new Error('BIFF8 defined name scope or ordinal changed');
    const bookWithStrings = SheetJS.write(xlsBook, { bookType: 'biff8', type: 'binary', bookSST: true });
    const namesContainer = SheetJS.CFB.read(bookWithStrings, { type: 'binary' });
    const workbookEntry = SheetJS.CFB.find(namesContainer, '/Workbook');
    const workbookStream = Uint8Array.from(workbookEntry.content);
    const records = new DataView(workbookStream.buffer);
    let offset = 0;
    while (offset + 4 <= workbookStream.length && records.getUint16(offset, true) !== 0x00fc) {
      offset += 4 + records.getUint16(offset + 2, true);
    }
    records.setUint32(offset + 8, 0xffffffff, true);
    workbookEntry.content = workbookStream;
    const names = SheetJS.parse_xlscfb(namesContainer, { bookSheets: true });
    if (names.SheetNames.join() !== 'First,Second' || names.Sheets !== undefined) {
      throw new Error('BIFF8 names-only read decoded the shared strings');
    }
    let stringError;
    try {
      SheetJS.parse_xlscfb(namesContainer, { bookSheets: true, WTF: true });
    } catch (error) {
      stringError = error;
    }
    if (stringError?.code !== 'INVALID_INPUT') throw new Error('BIFF8 strict names-only read ignored invalid strings');
    for (const [rows, bookSST, expected] of [
      [[[1]], true, [0, 0]], [[['one']], false, [0, 0]], [[['']], true, [1, 1]],
    ]) {
      const book = Reference.utils.book_new(Reference.utils.aoa_to_sheet(rows));
      for (const api of [SheetJS, Reference]) {
        const bytes = api.write(book, { bookType: 'biff8', type: 'binary', bookSST });
        const archive = SheetJS.CFB.read(bytes, { type: 'binary' });
        const content = Uint8Array.from(SheetJS.CFB.find(archive, '/Workbook').content);
        const records = new DataView(content.buffer);
        let offset = 0;
        while (offset + 12 <= content.length && records.getUint16(offset, true) !== 0x00fc) {
          offset += 4 + records.getUint16(offset + 2, true);
        }
        const actual = offset + 12 <= content.length
          ? [records.getUint32(offset + 4, true), records.getUint32(offset + 8, true)] : undefined;
        if (JSON.stringify(actual) !== JSON.stringify(expected)) {
          throw new Error('BIFF8 shared string counts differ for empty and inline strings');
        }
      }
    }
    const continuedText = '日本😀'.repeat(4300);
    const continuedBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[continuedText], ['tail']]));
    const continued = SheetJS.write(continuedBook, { bookType: 'biff8', type: 'binary', bookSST: true });
    for (const api of [SheetJS, Reference]) {
      const sheet = api.read(continued, { type: 'binary' }).Sheets.Sheet1;
      if (sheet.A1.v !== continuedText || sheet.A2.v !== 'tail') {
        throw new Error('BIFF8 continued shared text did not round-trip');
      }
    }
    for (const version of [5, 8]) {
      const formulaSheet = Reference.utils.aoa_to_sheet([[4, 9, 13]]);
      const address = version === 8 ? [0x44, 0, 0, 0, 0xc0] : [0x44, 0, 0xc0, 0];
      const range = version === 8 ? [0x45, 0, 0, 0, 0, 0, 0xc0, 1, 0xc0] : [0x45, 0, 0xc0, 0, 0xc0, 0, 1];
      const addition = [...address, 0x1e, 5, 0, 3];
      const sum = [...range, 0x19, 0x10, 0, 0];
      formulaSheet.B1.bf = [addition.length, 0, ...addition];
      formulaSheet.C1.bf = [sum.length, 0, ...sum];
      const bytes = Reference.write(Reference.utils.book_new(formulaSheet), { bookType: 'biff' + version, type: 'binary' });
      const actual = SheetJS.read(bytes, { type: 'binary' }).Sheets.Sheet1;
      const expected = Reference.read(bytes, { type: 'binary' }).Sheets.Sheet1;
      for (const cell of ['B1', 'C1']) {
        if (!expected[cell].f || actual[cell].f !== expected[cell].f || actual[cell].v !== expected[cell].v) {
          throw new Error('BIFF formula differs from reference');
        }
      }
      if (SheetJS.read(bytes, { type: 'binary', cellFormula: false }).Sheets.Sheet1.B1.f !== undefined) {
        throw new Error('BIFF formula projection ignored cellFormula: false');
      }
      const formulaExport = SheetJS.utils.aoa_to_sheet([[4, 9, 13, 'ab', true]]);
      const formulas = { B1: '$A1+5', C1: 'SUM(A1:B1)', D1: '"a"&"b"', E1: 'A1<B1' };
      for (const [cell, formula] of Object.entries(formulas)) formulaExport[cell].f = formula;
      const exported = SheetJS.write(SheetJS.utils.book_new(formulaExport), { bookType: 'biff' + version, type: 'binary' });
      for (const api of [SheetJS, Reference]) {
        const result = api.read(exported, { type: 'binary' }).Sheets.Sheet1;
        for (const [cell, formula] of Object.entries(formulas)) {
          if (result[cell].f !== formula || result[cell].v !== formulaExport[cell].v) {
            throw new Error('BIFF formula or typed cache failed to round trip');
          }
        }
      }
    }
    for (const bookType of ['biff5', 'biff8', 'xlsb']) {
      const input = SheetJS.utils.aoa_to_sheet([[4], [7]]);
      SheetJS.utils.sheet_set_array_formula(input, 'B1:B2', 'A1:A2*2');
      input.B1.v = 8;
      input.B2.v = 14;
      const bytes = SheetJS.write(SheetJS.utils.book_new(input), { bookType, type: 'binary' });
      for (const api of [SheetJS, Reference]) {
        const sheet = api.read(bytes, { type: 'binary' }).Sheets.Sheet1;
        if (sheet.B1.f !== 'A1:A2*2' || sheet.B1.F !== 'B1:B2' || sheet.B2.F !== 'B1:B2' ||
            sheet.B1.v !== 8 || sheet.B2.v !== 14) throw new Error(bookType + ' array formula did not round trip');
      }
    }
    const catalogue = Reference.utils.book_new(Reference.utils.aoa_to_sheet([[1]]), '公開');
    Reference.utils.book_append_sheet(catalogue, Reference.utils.aoa_to_sheet([[2]]), '非表示');
    catalogue.Workbook = { WBProps: { date1904: true }, Sheets: [{ Hidden: 0 }, { Hidden: 2 }] };
    for (const writer of [Reference, SheetJS]) {
      const catalogueBytes = writer.write(catalogue, { bookType: 'xlsb', type: 'binary' });
      const info = SheetJS.read(catalogueBytes, { type: 'binary', bookSheets: true });
      const oracleInfo = Reference.read(catalogueBytes, { type: 'binary' });
      if (JSON.stringify(info.SheetNames) !== JSON.stringify(oracleInfo.SheetNames) ||
          info.Workbook?.Sheets[1]?.Hidden !== oracleInfo.Workbook.Sheets[1].Hidden ||
          info.Workbook?.WBProps.date1904 !== oracleInfo.Workbook.WBProps.date1904 || info.Sheets !== undefined) {
        throw new Error('XLSB catalogue projection differs from reference');
      }
    }
    const txtSheet = SheetJS.utils.aoa_to_sheet([['日本', 1]]);
    const plainTxt = '日本\t1';
    const binaryTxt = '\xff\xfe' + String.fromCharCode(0xe5, 0x65, 0x2c, 0x67, 9, 0, 49, 0);
    if (SheetJS.utils.sheet_to_txt(txtSheet) !== plainTxt) throw new Error('TXT utility changed without a codepage table');
    const txtCodec = { utils: {
      decode: (_page, bytes) => new TextDecoder('utf-16le').decode(bytes),
      encode(page, text, format) {
        if (page !== 1200 || format !== 'str') throw new Error('TXT utility requested the wrong codepage output mode');
        return Array.from({ length: text.length }, (_, index) => {
          const unit = text.charCodeAt(index);
          return String.fromCharCode(unit & 255, unit >>> 8);
        }).join('');
      },
    } };
    SheetJS.set_cptable(txtCodec);
    try {
      const options = { FS: ',', RS: '!' };
      const binary = SheetJS.utils.sheet_to_txt(txtSheet, options);
      const book = SheetJS.utils.book_new(txtSheet);
      if (binary !== binaryTxt || options.FS !== '\t' || options.RS !== '\n' ||
          SheetJS.utils.sheet_to_txt(txtSheet, { type: 'string' }) !== plainTxt ||
          SheetJS.write(book, { bookType: 'txt', type: 'binary' }) !== binaryTxt ||
          SheetJS.write(book, { bookType: 'txt', type: 'string' }) !== plainTxt) {
        throw new Error('TXT utility and workbook output disagree with their encoding contracts');
      }
    } finally {
      SheetJS.set_cptable(undefined);
    }
    const displayStyle = document.createElement('style');
    displayStyle.textContent = '.xlsx-visible-row { display: table-row !important } ' +
      '.xlsx-visible-cell { display: table-cell !important }';
    const displayTable = document.createElement('table');
    displayTable.innerHTML = '<tr style="display:none" class="xlsx-visible-row">' +
      '<td rowspan="2">A</td><td style="display:none" class="xlsx-visible-cell">B</td></tr>' +
      '<tr><td>C</td></tr><tr style="display:none"><td>Hidden</td></tr>';
    document.head.append(displayStyle);
    document.body.append(displayTable);
    try {
      const actual = SheetJS.utils.table_to_sheet(displayTable, { display: true });
      const expected = Reference.utils.table_to_sheet(displayTable, { display: true });
      if (JSON.stringify(actual) !== JSON.stringify(expected)) {
        throw new Error('Computed display overrides or row spans differ from the reference');
      }
    } finally {
      displayTable.remove();
      displayStyle.remove();
    }
    const typedTable = document.createElement('table');
    typedTable.innerHTML = '<tr><td data-t="e" data-v="7">#DIV/0!</td></tr>' +
      '<tr style="display:none"><td>100</td></tr><tr><td>2</td></tr><tr><td>3</td></tr>';
    for (const raw of [false, true]) {
      const options = { raw, display: true, sheetRows: 2, origin: 'C2' };
      const actual = SheetJS.utils.sheet_add_dom({}, typedTable, options);
      const expected = Reference.utils.sheet_add_dom({}, typedTable, options);
      for (const key of ['C2', 'C3', 'C4']) {
        const cell = actual[key];
        const ref = expected[key];
        if (cell?.t !== ref?.t || cell?.v !== ref?.v || cell?.w !== ref?.w) {
          throw new Error('HTML table typed cells or row selection differ from reference');
        }
      }
    }
    const dateTable = document.createElement('table');
    dateTable.innerHTML = '<tr><td data-z="0.00">2024-01-02</td><td data-z="0.00">7</td></tr>';
    for (const options of [{}, { cellDates: true, UTC: true, dateNF: 'yyyy-mm-dd' }, { raw: true }]) {
      const actual = SheetJS.utils.table_to_sheet(dateTable, options);
      const expected = Reference.utils.table_to_sheet(dateTable, options);
      for (const key of ['A1', 'B1']) {
        if (JSON.stringify(actual[key]) !== JSON.stringify(expected[key])) {
          throw new Error('HTML table date formats differ from reference: ' + JSON.stringify([actual[key], expected[key]]));
        }
      }
    }
    const linkTable = document.createElement('table');
    linkTable.innerHTML = '<tr><td><a href="#skip">Local</a><span><a href="https://example.com/a?x=1&amp;y=2">External</a></span></td>' +
      '<td><a href="javascript:void(0)">Ignored</a></td></tr><tr><td>Cut off</td></tr>';
    const appended = [SheetJS, Reference].map(api => {
      const sheet = api.utils.aoa_to_sheet([[1]]);
      return api.utils.sheet_add_dom(sheet, linkTable, { origin: -1, sheetRows: 1 });
    });
    if (appended.some(sheet => sheet.A2.l?.Target !== 'https://example.com/a?x=1&y=2' || sheet.B2.l !== undefined ||
        sheet['!ref'] !== 'A1:B2' || sheet['!fullref'] !== 'A1:B3' || sheet.A3 !== undefined)) {
      throw new Error('HTML table links or appended row cutoff differ from reference');
    }
    const xlsbStyled = SheetJS.utils.aoa_to_sheet([[{ t: 'n', v: 1, z: '0.00' }, { t: 'z' }]]);
    const xlsbStyledBytes = SheetJS.write(SheetJS.utils.book_new(xlsbStyled), { bookType: 'xlsb', type: 'array' });
    for (const dense of [false, true]) {
      for (const options of [{}, { cellNF: true, sheetStubs: true }, { cellStyles: true, cellNF: false, sheetStubs: false }]) {
        const settings = { ...options, dense };
        const actual = SheetJS.read(xlsbStyledBytes, settings).Sheets.Sheet1;
        const expected = Reference.read(xlsbStyledBytes, settings).Sheets.Sheet1;
        const row = sheet => dense ? sheet['!data'][0] : [sheet.A1, sheet.B1];
        const blank = row(actual)[1];
        if (JSON.stringify(row(actual)) !== JSON.stringify(row(expected)) ||
            (blank && (!Object.hasOwn(blank, 'v') || Object.hasOwn(blank, 'z')))) {
          throw new Error('XLSB styling did not preserve number formats or the blank cell shape');
        }
      }
    }
    const selectionBook = Reference.utils.book_new(Reference.utils.aoa_to_sheet([[1]]), 'First');
    Reference.utils.book_append_sheet(selectionBook, Reference.utils.aoa_to_sheet([[2]]), 'Second');
    Reference.utils.book_append_sheet(selectionBook, Reference.utils.aoa_to_sheet([[3]]), 'Third');
    for (const bookType of ['xlsx', 'xlsb', 'biff5', 'biff8']) {
      const bytes = Reference.write(selectionBook, { bookType, type: 'array' });
      for (const [sheets, sheetKeys] of [
        [[2, 'fIrSt', 'FIRST'], ['First', 'Third']], [[], []], ['sEcOnD', ['Second']], [0, ['First']],
      ]) {
        const selected = SheetJS.read(bytes, { sheets });
        const reference = Reference.read(bytes, { sheets });
        // SheetJS CE 0.20.3 always reads every sheet in compound BIFF files.
        const expectedKeys = bookType.startsWith('biff') ? sheetKeys : Object.keys(reference.Sheets);
        if (JSON.stringify(Object.keys(selected.Sheets)) !== JSON.stringify(expectedKeys) ||
            JSON.stringify(selected.SheetNames) !== JSON.stringify(reference.SheetNames)) {
          throw new Error(bookType + ' sheet selectors changed the selected sheets or tab order');
        }
      }
    }
    const odfBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[1]]), 'First');
    SheetJS.utils.book_append_sheet(odfBook, SheetJS.utils.aoa_to_sheet([[2]]), 'Second');
    SheetJS.utils.book_append_sheet(odfBook, SheetJS.utils.aoa_to_sheet([[3]]), 'Third');
    odfBook.Workbook = { Sheets: [{ Hidden: 0 }, { Hidden: 1 }, { Hidden: 0 }] };
    for (const bookType of ['ods', 'fods']) {
      const bytes = SheetJS.write(odfBook, { bookType, type: 'array' });
      const selected = SheetJS.read(bytes, { sheets: [2, 'fIrSt'] });
      const hidden = SheetJS.read(bytes, { sheets: 'sEcOnD' });
      const catalogue = SheetJS.read(bytes, { sheets: [], bookSheets: true });
      if (selected.SheetNames.join() !== 'First,Second,Third' || Object.keys(selected.Sheets).join() !== 'First,Third' ||
          hidden.Sheets.Second?.A1.v !== 2 || hidden.Workbook.Sheets[1].Hidden !== 1 ||
          catalogue.Sheets !== undefined || catalogue.SheetNames.join() !== 'First,Second,Third') {
        throw new Error(bookType + ' sheet selection changed tab order or visibility');
      }
      try {
        SheetJS.read(bytes, { sheets: [[0]], bookSheets: true });
        throw new Error(bookType + ' accepted nested selectors');
      } catch (error) {
        if (error.code !== 'UNSUPPORTED_OPTIONS' || error.message !== 'Expected sheet names or zero-based indexes') throw error;
      }
    }
    const withoutSpreadsheet = '<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"><office:body/></office:document>';
    for (const [options, code, message] of [
      [{ sheetRows: -1, sheets: [null] }, 'UNSUPPORTED_OPTIONS', 'Expected a nonnegative FODS row limit'],
      [{ sheetRows: 0, sheets: [null] }, 'INVALID_INPUT', 'Expected an OpenDocument spreadsheet'],
    ]) {
      try {
        SheetJS.read(withoutSpreadsheet, { type: 'string', ...options });
        throw new Error('FODS accepted a document without a spreadsheet');
      } catch (error) {
        if (error.code !== code || error.message !== message) throw error;
      }
    }
    const selectionBytes = Reference.write(selectionBook, { bookType: 'xlsb', type: 'array' });
    const lazySelectors = Object.defineProperty([], '0', { get() { throw new Error('Unexpected selector access'); } });
    if (SheetJS.read(selectionBytes, { sheets: lazySelectors, bookSheets: true }).SheetNames.length !== 3) {
      throw new Error('XLSB catalogue depended on worksheet selectors');
    }
    const namedBook = Reference.utils.book_new(Reference.utils.aoa_to_sheet([[1]]), 'Data');
    namedBook.Workbook = { Names: [
      { Name: 'Global', Ref: 'Data!$A$1', Comment: '日本語' },
      { Name: '_xlnm.Print_Area', Sheet: 0, Ref: 'Data!$A$1:$A$2' },
    ] };
    const namedBytes = Reference.write(namedBook, { bookType: 'xlsb', type: 'binary' });
    const expectedNames = Reference.read(namedBytes, { type: 'binary' }).Workbook.Names;
    const actualNames = SheetJS.read(namedBytes, { type: 'binary', cellFormula: false, xlfn: true }).Workbook.Names;
    if (JSON.stringify(actualNames) !== JSON.stringify(expectedNames) ||
        SheetJS.read(namedBytes, { type: 'binary', bookSheets: true }).Workbook.Names !== undefined) {
      throw new Error('XLSB defined names differ from reference: ' + JSON.stringify({ actualNames, expectedNames }));
    }
    const ownNamedBytes = SheetJS.write(namedBook, { bookType: 'xlsb', type: 'binary' });
    if (JSON.stringify(Reference.read(ownNamedBytes, { type: 'binary' }).Workbook.Names) !== JSON.stringify(expectedNames)) {
      throw new Error('Reference did not read the XLSB defined-name records written by Rust');
    }
    namedBook.Workbook.Names.unshift(
      { Name: 'Forward', Ref: 'Global' },
      { Name: 'Unsupported', Ref: 'UNSUPPORTED_FUNCTION()' },
      { Name: 'Dependent', Ref: 'Unsupported' },
    );
    namedBook.Sheets.Data.A1.f = 'Forward';
    const roundTrip = SheetJS.read(SheetJS.write(namedBook, { bookType: 'xlsb', type: 'array' }));
    if (roundTrip.Workbook.Names.map(name => name.Name).join(',') !== 'Forward,Global,_xlnm.Print_Area' ||
        roundTrip.Workbook.Names[0]?.Ref !== 'Global' || roundTrip.Sheets.Data.A1.f !== 'Forward') {
      throw new Error('XLSB defined-name indices did not survive forward references and omitted definitions');
    }
    const xlsbLinkSheet = Reference.utils.aoa_to_sheet([[1], [2]]);
    xlsbLinkSheet.A1.l = { Target: 'https://example.com/page#part', Tooltip: '日本語' };
    xlsbLinkSheet.A2.l = { Target: '#Data!A1' };
    const xlsbLinkBytes = Reference.write(Reference.utils.book_new(xlsbLinkSheet, 'Data'), { bookType: 'xlsb', type: 'binary' });
    for (const dense of [false, true]) {
      for (const sheetRows of [undefined, 1]) {
        const settings = { type: 'binary', dense, sheetRows };
        const actual = SheetJS.read(xlsbLinkBytes, settings).Sheets.Data;
        const expected = Reference.read(xlsbLinkBytes, settings).Sheets.Data;
        const links = sheet => dense ? [sheet['!data'][0]?.[0]?.l, sheet['!data'][1]?.[0]?.l] : [sheet.A1?.l, sheet.A2?.l];
        if (JSON.stringify(links(actual)) !== JSON.stringify(links(expected))) {
          throw new Error('XLSB hyperlinks differ from reference: ' + JSON.stringify([links(actual), links(expected)]));
        }
      }
    }
    for (const dense of [false, true]) {
      const linked = SheetJS.utils.aoa_to_sheet([[1], [2]], { dense });
      const first = dense ? linked['!data'][0][0] : linked.A1;
      const second = dense ? linked['!data'][1][0] : linked.A2;
      first.l = { Target: 'https://example.com/page?a=1&b=2#part', Tooltip: '日本語', display: 'Visit' };
      second.t = 'z';
      second.v = undefined;
      second.l = { Target: "#'Data'!A1" };
      const bytes = SheetJS.write(SheetJS.utils.book_new(linked, 'Data'), { bookType: 'xlsb', type: 'array' });
      for (const api of [SheetJS, Reference]) {
        const read = api.read(bytes).Sheets.Data;
        if (read.A1.l?.Target !== first.l.Target || read.A1.l?.Rel?.TargetMode !== 'External' ||
            read.A1.l?.Tooltip !== '日本語' || read.A1.l?.display !== 'Visit' ||
            read.A2?.t !== 'z' || read.A2.l?.Target !== second.l.Target || read.A2.l?.relId !== '') {
          throw new Error('XLSB hyperlinks did not survive writing: ' + JSON.stringify(read));
        }
      }
    }
    const rangeSheet = SheetJS.utils.aoa_to_sheet([[1, 2], [3, 4]]);
    const rangeLink = { Target: '#Data!A1', rfx: SheetJS.utils.decode_range('A1:B2') };
    for (const address of ['A1', 'B1', 'A2', 'B2']) rangeSheet[address].l = rangeLink;
    const writeLinkedBook = book => SheetJS.write(book, { bookType: 'xlsb', type: 'array' });
    const rangeBytes = writeLinkedBook(SheetJS.utils.book_new(rangeSheet, 'Data'));
    for (const api of [SheetJS, Reference]) {
      const read = api.read(rangeBytes).Sheets.Data;
      if (read.A1.l !== read.B2.l || SheetJS.utils.encode_range(read.B2.l.rfx) !== 'A1:B2') {
        throw new Error('XLSB did not preserve a shared hyperlink range');
      }
    }
    const editedLinks = SheetJS.read(rangeBytes);
    delete editedLinks.Sheets.Data.B1.l;
    editedLinks.Sheets.Data.A2.l = { Target: '#Data!B2' };
    const editedRange = Reference.read(writeLinkedBook(editedLinks)).Sheets.Data;
    if (editedRange.B1.l !== undefined || editedRange.A2.l?.Target !== '#Data!B2' ||
        editedRange.A1.l?.Target !== '#Data!A1' || editedRange.B2.l?.Target !== '#Data!A1') {
      throw new Error('XLSB hyperlink range overwrote a cell after an individual edit');
    }
    function xlsbProtectionBytes(api, settings) {
      const sheet = api.utils.aoa_to_sheet([[1]]);
      if (settings !== undefined) sheet['!protect'] = settings;
      const bytes = api.write(api.utils.book_new(sheet), { bookType: 'xlsb', type: 'array' });
      const archive = SheetJS.CFB.read(new Uint8Array(bytes), { type: 'array' });
      const records = Uint8Array.from(SheetJS.CFB.find(archive, '/xl/worksheets/sheet1.bin').content);
      const header = records.findIndex((value, index) => value === 0x97 && records[index + 1] === 4 && records[index + 2] === 66);
      return header < 0 ? undefined : records.slice(header + 3, header + 69);
    }
    for (const settings of [undefined, {}, { password: 'café設定', objects: true, formatCells: false,
      sort: false, autoFilter: true, selectLockedCells: true, selectUnlockedCells: false }]) {
      const actual = xlsbProtectionBytes(SheetJS, settings);
      const expected = xlsbProtectionBytes(Reference, settings);
      if ((settings !== undefined && (!actual || !expected)) ||
          JSON.stringify(actual) !== JSON.stringify(expected)) {
        throw new Error('XLSB sheet protection bytes differ from reference');
      }
    }
    const listedOptions = { type: 'binary', bookFiles: true, sheets: '公開' };
    const listedBytes = Reference.write(catalogue, { bookType: 'xlsb', type: 'binary' });
    const listed = SheetJS.read(listedBytes, listedOptions);
    const oracleFiles = Reference.read(listedBytes, { ...listedOptions });
    if (JSON.stringify(listed.keys) !== JSON.stringify(oracleFiles.keys)) {
      throw new Error('XLSB ZIP entry order differs from reference');
    }
    for (const key of ['xl/workbook.bin', 'xl/worksheets/sheet2.bin', '[Content_Types].xml']) {
      const file = listed.files[key];
      const oracle = oracleFiles.files[key];
      if (file.name !== oracle.name || file.size !== oracle.size ||
          file.content.some((byte, index) => byte !== oracle.content[index])) {
        throw new Error('XLSB ZIP entry differs from reference: ' + key);
      }
    }
    if (SheetJS.read(listedBytes, { type: 'binary', bookFiles: true, bookSheets: true }).files !== undefined) {
      throw new Error('XLSB names-only read unexpectedly projected ZIP entries');
    }
    const macroArchive = SheetJS.CFB.read(listedBytes, { type: 'binary' });
    const contentTypes = '[Content_Types].xml';
    const contentXml = new TextDecoder().decode(Uint8Array.from(SheetJS.CFB.find(macroArchive, '/' + contentTypes).content));
    const payload = Uint8Array.of(0, 43, 99, 255);
    SheetJS.CFB.utils.cfb_del(macroArchive, '/' + contentTypes);
    SheetJS.CFB.utils.cfb_add(macroArchive, contentTypes, new TextEncoder().encode(contentXml.replace('</Types>',
      '<Override PartName="/xl/testing-vba.bin" ContentType="application/vnd.ms-office.vbaProject"/></Types>')));
    SheetJS.CFB.utils.cfb_add(macroArchive, 'xl/testing-vba.bin', payload);
    const macroBytes = SheetJS.CFB.write(macroArchive, { type: 'binary', fileType: 'zip' });
    const macro = SheetJS.read(macroBytes, { type: 'binary', bookVBA: true });
    const oracleMacro = Reference.read(macroBytes, { type: 'binary', bookVBA: true });
    if (JSON.stringify([...macro.vbaraw]) !== JSON.stringify([...oracleMacro.vbaraw]) ||
        macro.vbaraw.length !== payload.length ||
        SheetJS.read(macroBytes, { type: 'binary' }).vbaraw !== undefined ||
        SheetJS.read(macroBytes, { type: 'binary', bookSheets: true, bookVBA: true }).vbaraw !== undefined ||
        SheetJS.read(listedBytes, { type: 'binary', bookVBA: true }).vbaraw !== undefined) {
      throw new Error('XLSB VBA byte projection differs from reference');
    }
    const detachedProject = macro.vbaraw;
    detachedProject[0] = 255;
    if (macro.vbaraw[0] !== 0) throw new Error('Mutating returned VBA bytes changed the stored snapshot');
    const chainArchive = SheetJS.CFB.read(listedBytes, { type: 'binary' });
    SheetJS.CFB.utils.cfb_del(chainArchive, '/' + contentTypes);
    SheetJS.CFB.utils.cfb_add(chainArchive, contentTypes, new TextEncoder().encode(contentXml.replace('</Types>',
      '<Override PartName="/xl/calcChain.bin" ContentType="application/vnd.ms-excel.calcChain"/></Types>')));
    const chain = Uint8Array.of(0x90, 4, 0,
      0x3f, 13, 2, 0, 0, 0, 6, 0, 0, 0, 3, 0, 0, 0, 10,
      0x3f, 13, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0x91, 4, 0);
    SheetJS.CFB.utils.cfb_add(chainArchive, 'xl/calcChain.bin', chain);
    const chainBytes = SheetJS.CFB.write(chainArchive, { type: 'binary', fileType: 'zip' });
    const chainOptions = { type: 'binary', bookDeps: true, sheets: '公開' };
    const actualChain = SheetJS.read(chainBytes, { ...chainOptions });
    const expectedChain = Reference.read(chainBytes, { ...chainOptions });
    if (JSON.stringify(actualChain.Deps) !== JSON.stringify(expectedChain.Deps) ||
        actualChain.Deps[0]?.r !== 'D7' || actualChain.Deps[0]?.l !== '1' || actualChain.Deps[0]?.a !== '1' ||
        Array.isArray(SheetJS.read(chainBytes, { type: 'binary' }).Deps) ||
        SheetJS.read(chainBytes, { type: 'binary', bookDeps: true, bookSheets: true }).Deps !== undefined) {
      throw new Error('XLSB calculation chain differs from reference');
    }
    const propertyBook = Reference.utils.book_new(Reference.utils.aoa_to_sheet([[1]]), 'Data');
    propertyBook.Props = { Title: '月報', Author: 'Ada', Company: 'Research', CreatedDate: new Date('2026-01-01T00:00Z') };
    propertyBook.Custprops = { Units: 4, Published: true };
    const propertyBytes = Reference.write(propertyBook, { bookType: 'xlsb', type: 'binary' });
    for (const bookSheets of [false, true]) {
      const options = { type: 'binary', bookProps: true, bookSheets };
      const actual = SheetJS.read(propertyBytes, { ...options });
      const expected = Reference.read(propertyBytes, { ...options });
      const sorted = record => JSON.stringify(Object.entries(record).sort(([a], [b]) => a.localeCompare(b)));
      if (actual.Sheets !== undefined || JSON.stringify(actual.SheetNames) !== JSON.stringify(expected.SheetNames) ||
          sorted(actual.Props) !== sorted(expected.Props) || sorted(actual.Custprops) !== sorted(expected.Custprops)) {
        throw new Error('XLSB document properties differ from reference');
      }
    }
    for (const sheets of [undefined, 'Data']) {
      const options = { type: 'binary', sheets };
      const actual = SheetJS.read(propertyBytes, { ...options });
      const expected = Reference.read(propertyBytes, { ...options });
      if (actual.Props.Title !== expected.Props.Title || actual.Custprops.Units !== expected.Custprops.Units ||
          actual.Props.CreatedDate.getTime() !== expected.Props.CreatedDate.getTime() ||
          actual.Props.Worksheets !== expected.Props.Worksheets ||
          JSON.stringify(actual.Props.SheetNames) !== JSON.stringify(expected.Props.SheetNames) ||
          actual.Sheets.Data.A1.v !== expected.Sheets.Data.A1.v) {
        throw new Error('XLSB cell read did not project document properties');
      }
    }
    const importedProperties = SheetJS.read(propertyBytes, { type: 'binary' });
    const exportedProperties = SheetJS.write(importedProperties, {
      bookType: 'xlsb', type: 'binary', compression: true,
      Props: { Title: '更新 & 確認', CreatedDate: new Date('2025-01-01T00:00Z') },
    });
    for (const api of [SheetJS, Reference]) {
      const { Props, Custprops, Sheets } = api.read(exportedProperties, { type: 'binary' });
      if (Props.Title !== '更新 & 確認' || Props.Author !== 'Ada' || Props.Company !== 'Research' ||
          Props.CreatedDate.getTime() !== Date.parse('2026-01-01T00:00Z') ||
          Props.ModifiedDate !== undefined || Custprops.Units !== 4 || Custprops.Published !== true ||
          Sheets.Data.A1.v !== 1) {
        throw new Error('XLSB export did not preserve document properties or apply the write options');
      }
    }
    const tabBook = Reference.utils.book_new(Reference.utils.aoa_to_sheet([[1]]), 'Summary');
    for (const name of ['Hidden', 'Very Hidden']) {
      Reference.utils.book_append_sheet(tabBook, Reference.utils.aoa_to_sheet([[2]]), name);
    }
    tabBook.Workbook = { Sheets: [{ Hidden: 0 }, { Hidden: 1 }, { Hidden: 2 }] };
    const tabBytes = SheetJS.write(tabBook, { bookType: 'xlsb', type: 'array' });
    const refTabs = Reference.write(tabBook, { bookType: 'xlsb', type: 'array' });
    for (const bookSheets of [false, true]) {
      const settings = { bookProps: true, bookSheets };
      const expected = Reference.read(refTabs, { ...settings });
      for (const api of [Reference, SheetJS]) {
        const actual = api.read(tabBytes, { ...settings });
        const view = book => JSON.stringify([book.Props.Worksheets, book.Props.SheetNames, book.SheetNames]);
        if (view(actual) !== view(expected)) throw new Error('XLSB worksheet properties differ from reference');
      }
    }
    const unicodeTab = Reference.utils.book_new(Reference.utils.aoa_to_sheet([[1]]), '売上 & 集計');
    const unicodeBytes = SheetJS.write(unicodeTab, { bookType: 'xlsb', type: 'array' });
    if (SheetJS.read(unicodeBytes, { bookProps: true }).Props.SheetNames?.[0] !== '売上 & 集計') {
      throw new Error('XLSB worksheet properties did not preserve the Unicode tab name');
    }
    const formatSheet = Reference.utils.aoa_to_sheet([[1.25, new Date('2020-03-04T12:00Z')]], { cellDates: true });
    formatSheet.A1.z = '0.000 "units"';
    formatSheet.B1.z = 'yyyy-mm-dd hh:mm';
    const formatBook = Reference.utils.book_new(formatSheet);
    for (const writer of [Reference, SheetJS]) {
      const bytes = writer.write(formatBook, { bookType: 'xlsb', type: 'binary' });
      const options = { type: 'binary', cellNF: true, cellDates: true };
      const actual = SheetJS.read(bytes, { ...options }).Sheets.Sheet1;
      const expected = Reference.read(bytes, { ...options }).Sheets.Sheet1;
      for (const key of ['A1', 'B1']) {
        const signature = cell => JSON.stringify([cell.t, cell.v, cell.z, cell.w]);
        if (signature(actual[key]) !== signature(expected[key])) {
          throw new Error('XLSB custom number format differs from reference: ' + key);
        }
      }
    }
    const dateFormatSheet = Reference.utils.aoa_to_sheet([[43894, 43894]]);
    dateFormatSheet.A1.z = 'm/d/yy';
    dateFormatSheet.B1.z = 'yyyy-mm-dd';
    const dateFormatBook = Reference.utils.book_new(dateFormatSheet);
    for (const bookType of ['xlsb', 'biff2', 'biff3', 'biff4', 'biff5', 'biff8']) {
      const bytes = Reference.write(dateFormatBook, { bookType, type: 'binary' });
      for (const cellText of [true, false]) {
        const options = { type: 'binary', dateNF: 'dd/mm/yyyy', cellText, cellNF: true, cellDates: true };
        const actual = SheetJS.read(bytes, { ...options }).Sheets.Sheet1;
        const expected = Reference.read(bytes, { ...options }).Sheets.Sheet1;
        for (const key of ['A1', 'B1']) {
          const signature = cell => JSON.stringify([cell.t, cell.v, cell.z, cell.w]);
          if (signature(actual[key]) !== signature(expected[key])) {
            throw new Error(bookType + ' dateNF differs from reference: ' + key);
          }
        }
      }
    }
    for (const bookType of ['xlsb', 'biff2', 'biff3', 'biff4', 'biff5', 'biff8']) {
      for (const date1904 of [false, true]) {
        const dateSerials = Reference.utils.aoa_to_sheet([[-1, 0, 59, 60, 60.123456, 61, 2958465, 2958466]]);
        for (const key of Object.keys(dateSerials)) {
          if (!key.startsWith('!')) dateSerials[key].z = 'm/d/yy';
        }
        const book = Reference.utils.book_new(dateSerials);
        book.Workbook = { WBProps: { date1904 } };
        const bytes = Reference.write(book, { bookType, type: 'array' });
        for (const cellDates of [false, true]) {
          const settings = { cellDates, cellNF: true };
          const actual = SheetJS.read(bytes, { ...settings }).Sheets.Sheet1;
          const expected = Reference.read(bytes, { ...settings }).Sheets.Sheet1;
          for (const key of Object.keys(dateSerials).filter(key => !key.startsWith('!'))) {
            const signature = cell => JSON.stringify([cell.t, cell.v, cell.z, cell.w]);
            if (signature(actual[key]) !== signature(expected[key])) {
              throw new Error(bookType + ' date serial boundary differs from reference: ' + key);
            }
          }
        }
      }
    }
    const scalarBook = Reference.utils.book_new(Reference.utils.aoa_to_sheet([['a& <b>', true, { t: 'e', v: 7 }]]));
    for (const bookType of ['xlsb', 'biff2', 'biff3', 'biff4', 'biff5', 'biff8']) {
      const bytes = Reference.write(scalarBook, { bookType, type: 'array' });
      for (const cellText of [false, true]) {
        const settings = { cellText, cellNF: false };
        const actual = SheetJS.read(bytes, { ...settings }).Sheets.Sheet1;
        const expected = Reference.read(bytes, { ...settings }).Sheets.Sheet1;
        const view = sheet => ['A1', 'B1', 'C1'].map(key => {
          const { t, v, w, z } = sheet[key];
          return [t, v, w, z];
        });
        if (JSON.stringify(view(actual)) !== JSON.stringify(view(expected))) {
          throw new Error(bookType + ' scalar formatting differs from reference');
        }
      }
    }
    const richBook = Reference.utils.book_new(Reference.utils.aoa_to_sheet([['日本語']]));
    richBook.Sheets.Sheet1['!ref'] = 'A1:Z10';
    const richBytes = Reference.write(richBook, { bookType: 'xlsb', type: 'binary', bookSST: true });
    const richContainer = SheetJS.CFB.read(richBytes, { type: 'binary' });
    const richPart = 'xl/sharedStrings.bin';
    const sharedStream = Uint8Array.from(SheetJS.CFB.find(richContainer, '/' + richPart).content);
    // BrtBeginSst (two-byte ID, one-byte size, eight-byte payload), then BrtSSTItem.
    if (sharedStream[11] !== 0x13 || sharedStream[12] !== 11) throw new Error('Unexpected XLSB shared string fixture');
    const richStream = new Uint8Array(sharedStream.length + 12);
    richStream.set(sharedStream.subarray(0, 24));
    richStream[12] += 12;
    richStream[13] = 1;
    richStream.set([2, 0, 0, 0, 0, 0, 5, 0, 1, 0, 7, 0], 24);
    richStream.set(sharedStream.subarray(24), 36);
    SheetJS.CFB.utils.cfb_del(richContainer, '/' + richPart);
    SheetJS.CFB.utils.cfb_add(richContainer, richPart, richStream);
    const richOutput = SheetJS.CFB.write(richContainer, { type: 'binary', fileType: 'zip' });
    for (const cellHTML of [true, false]) {
      const settings = { type: 'binary', cellHTML, nodim: true };
      const actual = SheetJS.read(richOutput, { ...settings }).Sheets.Sheet1;
      const expected = Reference.read(richOutput, { ...settings }).Sheets.Sheet1;
      if (actual['!ref'] !== expected['!ref'] || JSON.stringify(actual.A1.r) !== JSON.stringify(expected.A1.r) ||
          actual.A1.r?.length !== 2 || actual.A1.v !== '日本語' || actual.A1.h !== expected.A1.h) {
        throw new Error('XLSB shared rich text or read options differ from reference');
      }
    }
    for (const bookSST of [false, true]) {
      const binaryBook = Reference.utils.book_new(Reference.utils.aoa_to_sheet([
        ['日本語', 23.5, true, new Date('2020-03-04T12:00Z')], ['日本語', -4, false],
      ], { cellDates: true }), 'First');
      Reference.utils.book_append_sheet(binaryBook, Reference.utils.aoa_to_sheet([[0, 'Other'], ['日本語']]), 'Second');
      const bytes = Reference.write(binaryBook, { bookType: 'xlsb', type: 'binary', bookSST });
      const settings = { type: 'binary', cellDates: true, cellNF: true };
      const actual = SheetJS.read(bytes, settings);
      const expected = Reference.read(bytes, { ...settings });
      const values = book => [book.SheetNames, ['A1', 'B1', 'C1', 'D1', 'A2', 'B2', 'C2']
        .map(cell => book.Sheets.First[cell].v)];
      const selected = SheetJS.read(bytes, { type: 'binary', sheets: 'Second', dense: true });
      if (JSON.stringify(values(actual)) !== JSON.stringify(values(expected)) ||
          selected.Sheets.First || selected.Sheets.Second['!data'][0][1].v !== 'Other') {
        throw new Error('XLSB values or projection differ from reference');
      }
      const merges = [Reference.utils.decode_range('A3:B3')];
      binaryBook.Sheets.First['!merges'] = merges;
      const exported = SheetJS.write(binaryBook, { bookType: 'xlsb', type: 'binary', bookSST, compression: bookSST });
      const exportedZip = SheetJS.CFB.read(exported, { type: 'binary' });
      const table = SheetJS.CFB.find(exportedZip, '/xl/sharedStrings.bin');
      if (Boolean(table) !== bookSST) throw new Error('XLSB shared string table presence differs from requested storage');
      if (table) {
        const content = Uint8Array.from(table.content);
        const header = new DataView(content.buffer);
        if (header.getUint32(3, true) !== 4 || header.getUint32(7, true) !== 2) {
          throw new Error('XLSB shared string occurrence and unique counts are incorrect');
        }
      }
      for (const api of [SheetJS, Reference]) {
        const reopened = api.read(exported, { ...settings });
        if (JSON.stringify(values(reopened)) !== JSON.stringify(values(expected)) ||
            reopened.Sheets.First['!merges']?.map(Reference.utils.encode_range).join() !== 'A3:B3' ||
            reopened.Sheets.Second.B1.v !== 'Other' || reopened.Sheets.Second.A2.v !== '日本語') {
          throw new Error('XLSB export differs from reference');
        }
      }
    }
    const formulaSheet = SheetJS.utils.aoa_to_sheet([[7, '日本語', true]]);
    formulaSheet.D1 = { t: 'e', v: 7 };
    formulaSheet['!ref'] = 'A1:D1';
    const formulas = ['SUM($A$2:A3)+XFD1048576', '"日本"&"語"', 'A1>0', '1/0'];
    const formulaCells = ['A1', 'B1', 'C1', 'D1'];
    formulaCells.forEach((key, index) => { formulaSheet[key].f = formulas[index]; });
    const xlsbFormulaBytes = SheetJS.write(SheetJS.utils.book_new(formulaSheet), { bookType: 'xlsb', type: 'array' });
    for (const api of [SheetJS, Reference]) {
      const reopened = api.read(xlsbFormulaBytes).Sheets.Sheet1;
      const cached = api.read(xlsbFormulaBytes, { cellFormula: false }).Sheets.Sheet1;
      for (const [index, key] of formulaCells.entries()) {
        if (reopened[key].f !== formulas[index] || reopened[key].v !== formulaSheet[key].v ||
            cached[key].f !== undefined || cached[key].v !== formulaSheet[key].v) {
          throw new Error('XLSB formula or cached value did not round trip: ' + key);
        }
      }
    }
    // xlsb-formula-presence and the existing descriptor boundary.
    const explicitFormula = SheetJS.read(xlsbFormulaBytes, { cellFormula: undefined }).Sheets.Sheet1;
    if (explicitFormula.A1.f !== formulas[0]) throw new Error('Explicit undefined changed XLSB formula projection');
    const inherited = Object.create({ dense: true });
    if (SheetJS.read(xlsbFormulaBytes, inherited).Sheets.Sheet1['!data']) {
      throw new Error('XLSB options used an inherited key');
    }
    let called = false;
    const accessor = Object.defineProperty({}, 'dense', { get() { called = true; return true; } });
    for (const options of [accessor, { unknownXlsbOption: true }]) {
      try {
        SheetJS.read(xlsbFormulaBytes, options);
        throw new Error('XLSB accepted an invalid descriptor');
      } catch (error) {
        if (error.code !== 'UNSUPPORTED_OPTIONS' || called) throw error;
      }
    }
    const linked = SheetJS.utils.aoa_to_sheet([[7, 9]]);
    linked.A1.f = 'Data!$A$1+3';
    linked.B1.f = "SUM('Owner''s Data'!A1:A2)";
    const linkedBook = SheetJS.utils.book_new(linked, 'First');
    SheetJS.utils.book_append_sheet(linkedBook, SheetJS.utils.aoa_to_sheet([[4]]), 'Data');
    SheetJS.utils.book_append_sheet(linkedBook, SheetJS.utils.aoa_to_sheet([[4], [5]]), "Owner's Data");
    for (const bookType of ['biff8', 'xlsb']) {
      const linkedBytes = SheetJS.write(linkedBook, { bookType, type: 'binary' });
      for (const api of [SheetJS, Reference]) {
        const settings = bookType === 'xlsb' ? { type: 'binary', sheets: 'First' } : { type: 'binary' };
        const sheet = api.read(linkedBytes, settings).Sheets.First;
        if (sheet.A1.f !== linked.A1.f || sheet.B1.f !== linked.B1.f || sheet.A1.v !== 7 || sheet.B1.v !== 9) {
          throw new Error(bookType + ' cross-sheet formula did not round trip');
        }
      }
    }
    const grouped = Reference.utils.aoa_to_sheet([[4, 8], [7, 11]]);
    grouped.B1.bf = grouped.B2.bf = [5, 0, 1, 0, 0, 1, 0];
    const groupedBytes = Reference.write(Reference.utils.book_new(grouped), { type: 'binary', bookType: 'biff8' });
    for (const array of [false, true]) {
      const container = SheetJS.CFB.read(groupedBytes, { type: 'binary' });
      const source = Uint8Array.from(SheetJS.CFB.find(container, 'Workbook').content);
      const view = new DataView(source.buffer);
      let end = 0;
      while (end + 4 <= source.length) {
        const type = view.getUint16(end, true);
        end += 4 + view.getUint16(end + 2, true);
        if (type === 6) break;
      }
      const tokens = array ? [0x44, 0, 0, 0, 0] : [0x4c, 0, 0, 0xff, 0xff, 0x4c, 0, 0, 0, 0, 3];
      const header = array ? [0x21, 2, 14 + tokens.length, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0]
        : [0xbc, 4, 10 + tokens.length, 0, 0, 0, 1, 0, 1, 1, 0, 2];
      const record = Uint8Array.of(...header, tokens.length, 0, ...tokens);
      const stream = new Uint8Array(source.length + record.length);
      stream.set(source.subarray(0, end));
      stream.set(record, end);
      stream.set(source.subarray(end), end + record.length);
      SheetJS.CFB.utils.cfb_add(container, 'Workbook', stream);
      const bytes = SheetJS.CFB.write(container, { type: 'binary' });
      const actual = SheetJS.read(bytes, { type: 'binary', dense: true }).Sheets.Sheet1['!data'];
      const expected = Reference.read(bytes, { type: 'binary' }).Sheets.Sheet1;
      const formula = array ? '$A$1' : 'A1+$A$1';
      if (actual[0][1].f !== formula || expected.B1.f !== formula || actual[1][1].v !== 11 ||
          (array ? actual[1][1].F !== 'B1:B2' || actual[1][1].f !== undefined : actual[1][1].f !== 'A2+$A$1')) {
        throw new Error('BIFF grouped formula failed to expand');
      }
    }
    const sharedSheet = SheetJS.utils.aoa_to_sheet([[4], [7]]);
    SheetJS.utils.sheet_set_array_formula(sharedSheet, 'B1:B2', 'A1+$A$1');
    sharedSheet.B1.v = 8;
    sharedSheet.B2.v = 11;
    const sharedBook = SheetJS.utils.book_new(sharedSheet);
    const sharedContainer = SheetJS.CFB.read(SheetJS.write(sharedBook, { bookType: 'xlsb', type: 'binary' }), { type: 'binary' });
    const part = 'xl/worksheets/sheet1.bin';
    const source = Uint8Array.from(SheetJS.CFB.find(sharedContainer, '/' + part).content);
    let cursor = 0;
    function readRecordInteger() {
      let value = 0;
      for (let shift = 0; shift <= 21 && cursor < source.length; shift += 7) {
        const byte = source[cursor++];
        value |= (byte & 127) << shift;
        if (!(byte & 128)) return value;
      }
      throw new Error('Invalid XLSB fixture record');
    }
    let replaced = false;
    while (cursor < source.length) {
      const start = cursor;
      const id = readRecordInteger();
      const size = readRecordInteger();
      const end = cursor + size;
      if (id === 0x1aa) {
        // Replace BrtArrFmla with BrtShrFmla. PtgRefN encodes a same-row, previous-column reference.
        const tokens = [0x4c, 0, 0, 0, 0, 0xff, 0xff, 0x44, 0, 0, 0, 0, 0, 0, 3];
        const record = Uint8Array.of(0xab, 0x03, 24 + tokens.length,
          ...source.subarray(cursor, cursor + 16), tokens.length, 0, 0, 0, ...tokens, 0, 0, 0, 0);
        const stream = new Uint8Array(start + record.length + source.length - end);
        stream.set(source.subarray(0, start));
        stream.set(record, start);
        stream.set(source.subarray(end), start + record.length);
        SheetJS.CFB.utils.cfb_del(sharedContainer, '/' + part);
        SheetJS.CFB.utils.cfb_add(sharedContainer, part, stream);
        replaced = true;
        break;
      }
      cursor = end;
    }
    if (!replaced) throw new Error('XLSB grouped formula fixture is missing the source formula');
    const sharedBytes = SheetJS.CFB.write(sharedContainer, { type: 'binary', fileType: 'zip' });
    for (const api of [SheetJS, Reference]) {
      const actual = api.read(sharedBytes, { type: 'binary', dense: true }).Sheets.Sheet1['!data'];
      const limited = api.read(sharedBytes, { type: 'binary', sheetRows: 1 }).Sheets.Sheet1;
      const cache = api.read(sharedBytes, { type: 'binary', cellFormula: false }).Sheets.Sheet1;
      if (limited.B1.f !== 'A1+$A$1' || limited.B2 !== undefined || cache.B2.v !== 11 ||
          cache.B2.f !== undefined || actual[0][1].f !== 'A1+$A$1' || actual[1][1].f !== 'A2+$A$1' ||
          actual[0][1].v !== 8 || actual[1][1].v !== 11 || actual[1][1].F !== undefined) {
        throw new Error('XLSB shared formula failed to expand');
      }
    }
    xlsBook.Sheets.First['!merges'] = [{ s: { r: 1, c: 0 }, e: { r: 1, c: 1 } }];
    xlsBook.Sheets.First['!rows'] = [{ hidden: true, level: 2, hpt: 31 }];
    xlsBook.Sheets.First['!cols'] = [, { hidden: true, level: 3, width: 22.75 }];
    for (const bookType of ['biff8', 'xlsb']) {
      const layoutBytes = SheetJS.write(xlsBook, { bookType, type: 'binary' });
      for (const api of [SheetJS, Reference]) {
        const sheet = api.read(layoutBytes, { type: 'binary', cellStyles: true }).Sheets.First;
        if (sheet['!rows'][0].hpt !== 31 || !sheet['!rows'][0].hidden || sheet['!rows'][0].level !== 2 ||
            sheet['!cols'][1].width !== 22.75 || !sheet['!cols'][1].hidden || sheet['!cols'][1].level !== 3 ||
            sheet['!merges'][0].e.c !== 1) throw new Error(bookType + ' layout did not round trip');
      }
    }
    const spanSheet = {
      A2: { t: 'n', v: 1 }, ANM2: { t: 'n', v: 2 }, XFD2: { t: 'n', v: 3 },
      '!ref': 'A2:XFD2', '!rows': [{ hidden: true }, { hpt: 24, level: 2 }, , { hpt: 31 }],
    };
    const spanBytes = SheetJS.write(SheetJS.utils.book_new(spanSheet), { bookType: 'xlsb', type: 'array' });
    for (const api of [SheetJS, Reference]) {
      const sheet = api.read(spanBytes).Sheets.Sheet1;
      const rows = sheet['!rows'];
      if (!rows[0]?.hidden || rows[1]?.hpt !== 24 || rows[1]?.level !== 2 || rows[3]?.hpt !== 31 ||
          sheet.A2.v !== 1 || sheet.ANM2.v !== 2 || sheet.XFD2.v !== 3) {
        throw new Error('XLSB wide row or layout-only rows did not round trip');
      }
    }
    const widths = SheetJS.utils.aoa_to_sheet([[1, 2, 3]]);
    widths['!cols'] = [{ width: 22.75, MDW: 6 }, { wpx: 109 }, { wch: 13.25 }];
    const widthBook = SheetJS.utils.book_new(widths);
    const widthResults = [];
    for (const writer of [SheetJS, Reference]) {
      const bytes = writer.write(widthBook, { bookType: 'xlsb', type: 'binary' });
      const settings = { type: 'binary', cellStyles: true };
      const actual = SheetJS.read(bytes, { ...settings }).Sheets.Sheet1['!cols'];
      const expected = Reference.read(bytes, { ...settings }).Sheets.Sheet1['!cols'];
      const fields = ['width', 'wpx', 'wch', 'MDW'];
      for (const [index, column] of actual.entries()) {
        if (fields.some(field => column[field] !== expected[index][field])) {
          throw new Error('XLSB derived column widths differ from reference');
        }
      }
      widthResults.push(actual.map(column => column.width));
    }
    if (widthResults[0].some((width, index) => width !== widthResults[1][index])) {
      throw new Error('XLSB column width options encode differently from reference');
    }
    const cutoffSheet = SheetJS.utils.aoa_to_sheet([[1, 2, 3], [4], [5], [6], [7]]);
    cutoffSheet['!ref'] = 'A1:Z10';
    cutoffSheet['!rows'] = [{ hpt: 21 }, { hpt: 22, hidden: true, level: 2 }];
    const cutoffBytes = SheetJS.write(SheetJS.utils.book_new(cutoffSheet), { bookType: 'xlsb', type: 'array' });
    for (const sheetRows of [undefined, 0, 1, 7, 9, 10, 100, 1048577, 'abc']) {
      for (const dense of [false, true]) {
        const actual = SheetJS.read(cutoffBytes, { sheetRows, dense }).Sheets.Sheet1;
        const expected = Reference.read(cutoffBytes, { sheetRows, dense }).Sheets.Sheet1;
        for (const key of ['!ref', '!fullref']) {
          if (actual[key] !== expected[key]) throw new Error('XLSB row limit changed ' + key + ': ' + sheetRows);
        }
        const excluded = actual['!rows']?.[1];
        if (excluded?.hpt !== 22 || !excluded.hidden || excluded.level !== 2 ||
            (sheetRows === 1 && (dense ? actual['!data'][1] : actual.A2))) {
          throw new Error('XLSB row limit did not preserve the cutoff row header alone');
        }
      }
    }
    let rowLimitCoercions = 0;
    const lazyRowLimit = { valueOf() { ++rowLimitCoercions; return 1.2; } };
    SheetJS.read(cutoffBytes, { sheetRows: lazyRowLimit, bookSheets: true });
    SheetJS.read(cutoffBytes, { sheetRows: lazyRowLimit, sheets: 'Excluded' });
    if (rowLimitCoercions !== 0) throw new Error('XLSB converted the row limit without projecting a sheet');
    const fractionalCutoff = SheetJS.read(cutoffBytes, { sheetRows: lazyRowLimit }).Sheets.Sheet1;
    if (rowLimitCoercions !== 1 || fractionalCutoff.A2?.v !== 4 || fractionalCutoff.A3 !== undefined) {
      throw new Error('XLSB did not round up the row limit once per projected sheet');
    }
    const printSheet = SheetJS.utils.aoa_to_sheet([['A', 'B'], [1, 2], [3, 4]]);
    printSheet['!autofilter'] = { ref: 'A1:B1' };
    printSheet['!merges'] = [SheetJS.utils.decode_range('A2:B3')];
    printSheet['!margins'] = { left: 0.45, top: 0, footer: 0.225 };
    const printBook = SheetJS.utils.book_new(printSheet);
    const margins = { left: 0.45, right: 0.7, top: 0, bottom: 0.75, header: 0.3, footer: 0.225 };
    for (const writer of [SheetJS, Reference]) {
      const bytes = writer.write(printBook, { bookType: 'xlsb', type: 'array' });
      for (const reader of [SheetJS, Reference]) {
        const actual = reader.read(bytes).Sheets.Sheet1;
        const limited = reader.read(bytes, { sheetRows: 2 }).Sheets.Sheet1;
        if (limited['!autofilter'] || limited['!margins'] || limited['!merges'] ||
            !actual['!merges']?.length || actual['!autofilter']?.ref !== 'A1:B3' ||
            Object.entries(margins).some(([edge, value]) => actual['!margins']?.[edge] !== value)) {
          throw new Error('XLSB autofilter or print margins did not round trip');
        }
      }
    }
    const xlsxOptionBytes = SheetJS.write(workbook, { type: 'array' });
    for (const [options, message] of [
      [{ dense: null, dateNF: false }, 'Expected boolean worksheet projection options'],
      [{ dateNF: false, sheetRows: -1 }, 'Expected dateNF to be a number format string'],
      [{ sheetRows: -1, bookSheets: 0 }, 'Expected a nonnegative sheetRows limit'],
      [{ bookSheets: 0, sheets: [null] }, 'Expected a boolean bookSheets option'],
      [{ sheets: [null] }, 'Expected sheet names or zero-based indexes'],
    ]) {
      try {
        SheetJS.read(xlsxOptionBytes, options);
        throw new Error('XLSX read accepted invalid options');
      } catch (error) {
        if (error.code !== 'UNSUPPORTED_OPTIONS' || error.message !== message) throw error;
      }
    }
    let selectorReads = 0;
    const catalogueSelectors = Object.defineProperty([], '0', { get() { ++selectorReads; return 0; } });
    const xlsxCatalogue = SheetJS.read(xlsxOptionBytes, {
      sheets: catalogueSelectors, bookSheets: true, bookFiles: true, bookDeps: true, bookVBA: true,
    });
    if (selectorReads !== 1 || xlsxCatalogue.Sheets !== undefined ||
        ['files', 'keys', 'Deps', 'vbaraw'].some(key => Object.hasOwn(xlsxCatalogue, key))) {
      throw new Error('XLSX catalogue accessed selectors after validation or projected optional resources');
    }
    const xlsxFiles = SheetJS.read(xlsxOptionBytes, { bookFiles: true });
    if (!xlsxFiles.keys?.includes('xl/workbook.xml')) throw new Error('XLSX full read omitted requested ZIP entries');
    const noteOnly = Reference.utils.aoa_to_sheet([[1], [], [3, 4]]);
    noteOnly.B2 = { t: 'z', c: [{ a: 'A', t: 'note without a cell record' }] };
    const noteOnlyBytes = Reference.write(Reference.utils.book_new(noteOnly), { type: 'array' });
    for (const dense of [false, true]) {
      const sheet = SheetJS.read(noteOnlyBytes, { dense, sheetRows: 2 }).Sheets.Sheet1;
      const note = dense ? sheet['!data'][1]?.[1] : sheet.B2;
      if (note?.t !== 'z' || note.c?.[0]?.t !== 'note without a cell record' ||
          (dense ? sheet['!data'][2] : sheet.B3) !== undefined ||
          sheet['!ref'] !== 'A1:B2' || sheet['!fullref'] !== 'A1:B3') {
        throw new Error('XLSX numeric coordinate projection lost a comment-only cell or cutoff');
      }
    }
    const formulaBatchSheet = Reference.utils.aoa_to_sheet([
      [2, { t: 'n', v: 3, f: 'A1+1' }], [4, { t: 'n', v: 5, f: 'A2+1' }],
    ]);
    const formulaBatchBytes = Reference.write(Reference.utils.book_new(formulaBatchSheet), { type: 'array' });
    const formulaBatchArchive = SheetJS.CFB.read(new Uint8Array(formulaBatchBytes), { type: 'array' });
    const formulaBatchPart = 'xl/worksheets/sheet1.xml';
    const formulaBatchXml = new TextDecoder().decode(SheetJS.CFB.find(formulaBatchArchive, '/' + formulaBatchPart).content);
    const sharedFormulaXml = formulaBatchXml
      .replace('<f>A1+1</f>', '<f t="shared" si="0" ref="B1:B2">A1+1</f>')
      .replace('<f>A2+1</f>', '<f t="shared" si="0"/>');
    if (sharedFormulaXml === formulaBatchXml) throw new Error('Could not create the XLSX shared formula fixture');
    SheetJS.CFB.utils.cfb_del(formulaBatchArchive, '/' + formulaBatchPart);
    SheetJS.CFB.utils.cfb_add(formulaBatchArchive, formulaBatchPart, new TextEncoder().encode(sharedFormulaXml));
    const sharedFormulaBytes = new Uint8Array(SheetJS.CFB.write(formulaBatchArchive, { type: 'array', fileType: 'zip' }));
    for (const cellFormula of [true, false]) {
      const settings = { cellFormula };
      const actual = SheetJS.read(sharedFormulaBytes, settings).Sheets.Sheet1;
      const expected = Reference.read(sharedFormulaBytes, settings).Sheets.Sheet1;
      const results = sheet => ['B1', 'B2'].map(key => [sheet[key].v, sheet[key].f]);
      if (JSON.stringify(results(actual)) !== JSON.stringify(results(expected))) {
        throw new Error('XLSX formula batch changed shared formula results or translation');
      }
    }
    const literals = Reference.utils.aoa_to_sheet([[
      { t: 'e', v: 7 }, 5, { t: 'd', v: new Date('2024-01-02T00:00:00Z'), z: 'm/d/yy' },
      { t: 'e', v: 23, f: 'NA()' },
    ]]);
    const literalBytes = Reference.write(Reference.utils.book_new(literals), { type: 'array', cellDates: true });
    for (const dense of [false, true]) {
      const settings = { cellDates: true, dense };
      const actual = SheetJS.read(literalBytes, settings).Sheets.Sheet1;
      const expected = Reference.read(literalBytes, settings).Sheets.Sheet1;
      const row = sheet => dense ? sheet['!data'][0] : [sheet.A1, sheet.B1, sheet.C1, sheet.D1];
      const fields = sheet => row(sheet).map(({ t, v, w, f }) => [t, v, w, f]);
      if (JSON.stringify(fields(actual)) !== JSON.stringify(fields(expected))) {
        throw new Error('XLSX batch changed error, ISO date or formula values: ' + JSON.stringify(fields(actual)));
      }
    }
    const formatFixture = Reference.utils.aoa_to_sheet([[
      { t: 'n', v: 45293, z: 'm/d/yy' }, { t: 'n', v: 7, z: '0.00' }, { t: 'n', v: 1.25, z: '0.0000' },
    ], [45293, undefined, { t: 'n', v: 45293, f: 'DATE(2024,1,2)', z: 'm/d/yy' }], [undefined, 7]]);
    const formatFixtureBytes = Reference.write(Reference.utils.book_new(formatFixture), { type: 'array' });
    for (const dateNF of [undefined, 'yyyy-mm-dd']) {
      for (const cellText of [undefined, true, false]) {
        const settings = { cellNF: true, cellText, dateNF };
        const actual = SheetJS.read(formatFixtureBytes, settings).Sheets.Sheet1;
        const expected = Reference.read(formatFixtureBytes, settings).Sheets.Sheet1;
        for (const address of ['A1', 'B1', 'C1']) {
          if (actual[address].z !== expected[address].z || actual[address].w !== expected[address].w) {
            throw new Error('XLSX number format resolution or cellText default differs from reference');
          }
        }
      }
    }
    const inheritedArchive = SheetJS.CFB.read(new Uint8Array(formatFixtureBytes), { type: 'array' });
    const inheritedPart = 'xl/worksheets/sheet1.xml';
    const inheritedXml = new TextDecoder().decode(SheetJS.CFB.find(inheritedArchive, '/' + inheritedPart).content);
    const inheritedStyleXml = inheritedXml
      .replace('<sheetData>', '<cols><col min="2" max="2" style="2"/></cols><sheetData>')
      .replace('<row r="2">', '<row r="2" s="1" customFormat="1">');
    SheetJS.CFB.utils.cfb_del(inheritedArchive, '/' + inheritedPart);
    SheetJS.CFB.utils.cfb_add(inheritedArchive, inheritedPart, new TextEncoder().encode(inheritedStyleXml));
    const inheritedBytes = new Uint8Array(SheetJS.CFB.write(inheritedArchive, { type: 'array', fileType: 'zip' }));
    const inheritedCells = SheetJS.read(inheritedBytes, { cellNF: true, dateNF: 'yyyy-mm-dd' }).Sheets.Sheet1;
    if (inheritedCells.A2.z !== 'm/d/yy' || inheritedCells.A2.w !== '2024-01-02' ||
        inheritedCells.B3.z !== '0.00' || inheritedCells.B3.w !== '7.00') {
      throw new Error('XLSX format cache lost inherited row or column formats: ' +
        JSON.stringify([inheritedCells.A2, inheritedCells.B3]));
    }
    const dateOnly = { cellDates: true, cellNF: false, cellText: false };
    const dates = SheetJS.read(formatFixtureBytes, dateOnly).Sheets.Sheet1;
    const expectedDates = Reference.read(formatFixtureBytes, dateOnly).Sheets.Sheet1;
    for (const key of ['A1', 'B1', 'C2']) {
      const actual = dates[key];
      const expected = expectedDates[key];
      if (actual.t !== expected.t || JSON.stringify(actual.v) !== JSON.stringify(expected.v) ||
          actual.f !== expected.f || actual.z !== undefined || actual.w !== undefined) {
        throw new Error('XLSX date style cache changed scalar or formula typing');
      }
    }
    const styled = Reference.utils.aoa_to_sheet([[1, 2]]);
    styled.A1.z = '0.00';
    const styledBytes = Reference.write(Reference.utils.book_new(styled), { type: 'array' });
    const styledArchive = SheetJS.CFB.read(new Uint8Array(styledBytes), { type: 'array' });
    const stylePart = 'xl/worksheets/sheet1.xml';
    const styleXml = new TextDecoder().decode(SheetJS.CFB.find(styledArchive, '/' + stylePart).content);
    const explicitStub = styleXml.replace('<c r="B1"><v>2</v></c>', '<c r="B1"/>');
    if (styleXml === explicitStub) throw new Error('Could not create the XLSX stub fixture');
    SheetJS.CFB.utils.cfb_del(styledArchive, '/' + stylePart);
    SheetJS.CFB.utils.cfb_add(styledArchive, stylePart, new TextEncoder().encode(explicitStub));
    const projectionBytes = new Uint8Array(SheetJS.CFB.write(styledArchive, { type: 'array', fileType: 'zip' }));
    for (const [options, format, stub] of [
      [{}, undefined, undefined],
      [{ cellNF: true }, '0.00', undefined],
      [{ sheetStubs: true }, undefined, 'z'],
      [{ cellStyles: true, cellNF: false, sheetStubs: false }, '0.00', 'z'],
    ]) {
      const actual = SheetJS.read(projectionBytes, options).Sheets.Sheet1;
      const expected = Reference.read(projectionBytes, options).Sheets.Sheet1;
      if (actual.A1.z !== format || expected.A1.z !== format || actual.B1?.t !== stub || expected.B1?.t !== stub) {
        throw new Error('XLSX style projection options changed number formats or stubs');
      }
    }
    const defaultRead = SheetJS.read(xlsxOptionBytes);
    const explicitRead = SheetJS.read(xlsxOptionBytes, { cellFormula: undefined, dense: undefined, sheetRows: undefined });
    if (JSON.stringify(defaultRead.SheetNames) !== JSON.stringify(explicitRead.SheetNames) ||
        JSON.stringify(defaultRead.Sheets) !== JSON.stringify(explicitRead.Sheets)) {
      throw new Error('XLSX read defaults changed for explicit undefined options');
    }
    for (const writer of [SheetJS.write, SheetJS.writeXLSX]) {
      writer(workbook, { type: 'array', compression: undefined, codepage: undefined });
      for (const [options, message] of [
        [{ compression: 'yes', themeXLSX: false, codepage: -1 }, 'Expected a boolean compression option'],
        [{ themeXLSX: false, codepage: -1 }, 'Expected a themeXLSX XML string'],
        [{ codepage: -1, unknownOption: true }, 'Expected a nonnegative integer codepage'],
      ]) {
        try {
          writer(workbook, { type: 'array', ...options });
          throw new Error('XLSX write accepted invalid options');
        } catch (error) {
          if (error.code !== 'UNSUPPORTED_OPTIONS' || error.message !== message) throw error;
        }
      }
    }
    for (const [bookType, message] of [
      ['xlsb', 'Expected a boolean compression option'],
      ['ods', 'Unsupported XLSX options'],
    ]) {
      try {
        SheetJS.write(workbook, { bookType, type: 'array', compression: 'yes', unknownOption: true });
        throw new Error('Write accepted invalid options');
      } catch (error) {
        if (error.code !== 'UNSUPPORTED_OPTIONS' || error.message !== message) throw error;
      }
    }
    SheetJS.write(workbook, { bookType: 'ods', type: 'array', compression: undefined });
    try {
      SheetJS.write(workbook, { bookType: 'xlsb', type: 'array', compression: undefined });
      throw new Error('XLSB write accepted explicit undefined compression');
    } catch (error) {
      if (error.code !== 'UNSUPPORTED_OPTIONS') throw error;
    }
    return ['xlsx', 'ods'].map(bookType => {
      const bytes = SheetJS.write(workbook, { bookType, type: 'buffer' });
      const container = Reference.CFB.read(bytes, { type: 'array' });
      const selected = SheetJS.parse_zip(container, { sheets: 'Data' });
      const expected = Reference.parse_zip(container, { sheets: 'Data' });
      return [selected.SheetNames, selected.Sheets.Data.B1.v, expected.Sheets.Data.B1.v,
        SheetJS.parse_zip({ files: { 'Index.zip': { content: bytes } } }).Sheets.Other.A1.v];
    });
  });
  assert.deepEqual(parsedContainers, [
    [['Data', 'Other'], '値', '値', 42],
    [['Data', 'Other'], '値', '値', 42],
  ]);
  const zipModes = await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    function fileMethods(bytes) {
      const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      const end = bytes.length - 22;
      const count = view.getUint16(end + 10, true);
      const decoder = new TextDecoder();
      const methods = new Set();
      let offset = view.getUint32(end + 16, true);
      for (let index = 0; index < count; index += 1) {
        const nameLength = view.getUint16(offset + 28, true);
        const name = decoder.decode(bytes.subarray(offset + 46, offset + 46 + nameLength));
        if (!name.endsWith('/')) methods.add(view.getUint16(offset + 10, true));
        offset += 46 + nameLength + view.getUint16(offset + 30, true) + view.getUint16(offset + 32, true);
      }
      return [...methods];
    }
    const methods = [];
    for (const Type of [Workbook, globalThis.ExcelJS.Workbook]) {
      const workbook = new Type();
      const sheet = workbook.addWorksheet('Zip');
      sheet.getCell('A1').value = 'compressed value '.repeat(100);
      const baseline = await workbook.xlsx.writeBuffer();
      const store = await workbook.xlsx.writeBuffer({ zip: { compression: 'STORE' } });
      const deflate = await workbook.xlsx.writeBuffer({ zip: { compressionOptions: { level: 9 } } });
      const reopened = await new Workbook().xlsx.load(deflate);
      if (reopened.getWorksheet('Zip').getCell('A1').value !== sheet.getCell('A1').value) throw new Error('ZIP lost data');
      if (Type === Workbook) {
        const chunks = [];
        await workbook.xlsx.write(new WritableStream({ write: bytes => chunks.push(bytes) }), { zip: { compression: 'STORE' } });
        if (fileMethods(chunks[0]).join(',') !== '0') throw new Error('ZIP stream ignored STORE');
        const unchanged = await workbook.xlsx.writeBuffer();
        if (unchanged.some((byte, index) => byte !== baseline[index]) || unchanged.length !== baseline.length) {
          throw new Error('ZIP serialization changed the committed bytes');
        }
      }
      methods.push([fileMethods(store), fileMethods(deflate), store.length > deflate.length]);
    }
    return methods;
  });
  assert.deepEqual(zipModes, [[[0], [8], true], [[0], [8], true]]);
  await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const Reference = globalThis.ExcelJS.Workbook;
    const source = new Reference();
    const original = source.addWorksheet('Protected');
    original.getCell('A1').value = 1;
    await original.protect('', { formatRows: true, selectLockedCells: false });
    const bytes = await source.xlsx.writeBuffer();
    const model = (await new Reference().xlsx.load(bytes)).getWorksheet('Protected').sheetProtection;
    const workbook = await new Workbook().xlsx.load(new Uint8Array(bytes));
    const worksheet = workbook.getWorksheet('Protected');
    if (JSON.stringify(worksheet.sheetProtection) !== JSON.stringify(model)) {
      throw new Error('Loaded worksheet protection differs from reference');
    }
    const ignored = await new Workbook().xlsx.load(new Uint8Array(bytes), { ignoreNodes: ['sheetProtection'] });
    const hidden = ignored.getWorksheet('Protected');
    if (hidden.sheetProtection !== undefined) throw new Error('Ignored worksheet protection was projected');
    hidden.getCell('A1').value = 2;
    const reopened = await new Reference().xlsx.load(await ignored.xlsx.writeBuffer());
    if (reopened.getWorksheet('Protected').sheetProtection?.formatRows !== true) {
      throw new Error('Editing an ignored sheet discarded worksheet protection');
    }
    hidden.unprotect();
    if (hidden.sheetProtection !== null) throw new Error('Unprotect did not clear worksheet protection');
  });
  await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const observe = Type => {
      const sheet = new Type().addWorksheet('Merge');
      const master = sheet.getCell('A1');
      master.value = { formula: '1+1', result: 2 };
      master.addMergeRef();
      const before = master.isMerged;
      sheet.mergeCells('A1:B1');
      master.releaseMergeRef();
      const follower = sheet.getCell('B1');
      const result = [before, master.isMerged, master.text, follower.text, follower.toCsvString(), follower.master === master];
      sheet.unMergeCells('A1:B1');
      return [...result, master.isMerged, follower.isMerged];
    };
    if (JSON.stringify(observe(Workbook)) !== JSON.stringify(observe(globalThis.ExcelJS.Workbook))) {
      throw new Error('Merged cell text and references differ from ExcelJS');
    }
  });
  await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const SheetJS = await import('./node_modules/xlsx/xlsx.mjs');
    for (const Type of [Workbook, globalThis.ExcelJS.Workbook]) {
      const workbook = new Type();
      const sheet = workbook.addWorksheet('Print');
      sheet.getCell('A1').value = 1;
      sheet.getRow(2).addPageBreak(2, 4);
      if (JSON.stringify(sheet.rowBreaks) !== JSON.stringify([{ id: 2, max: 3, man: 1, min: 1 }])) {
        throw new Error('Row page break projection differs from reference');
      }
      const bytes = await workbook.xlsx.writeBuffer();
      const loaded = await new Workbook().xlsx.load(bytes, { ignoreNodes: ['rowBreaks'] });
      const projected = loaded.getWorksheet('Print');
      if (projected.rowBreaks.length !== 0) throw new Error('Imported row breaks were projected');
      projected.getCell('A1').value = 2;
      const archive = SheetJS.CFB.read(await loaded.xlsx.writeBuffer(), { type: 'array' });
      const xml = new TextDecoder().decode(SheetJS.CFB.find(archive, '/xl/worksheets/sheet1.xml').content);
      if (!xml.includes('<rowBreaks') || !/<brk[^>]+id="2"/.test(xml)) {
        throw new Error('Editing an imported sheet discarded row breaks');
      }
    }
  });
  await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const Reference = globalThis.ExcelJS.Workbook;
    const source = new Reference();
    const original = source.addWorksheet('Pictures');
    original.getCell('A1').value = 1;
    const image = source.addImage({
      base64: 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=',
      extension: 'png',
    });
    original.addImage(image, 'A1:B2');
    original.addBackgroundImage(image);
    const bytes = await source.xlsx.writeBuffer();
    for (const ignoreNodes of [['drawing'], ['picture'], ['drawing', 'picture']]) {
      const actual = await new Workbook().xlsx.load(bytes, { ignoreNodes });
      const expected = await new Reference().xlsx.load(bytes, { ignoreNodes });
      const sheet = actual.getWorksheet('Pictures');
      const oracle = expected.getWorksheet('Pictures');
      if (sheet.getImages().length !== oracle.getImages().length ||
          sheet.getBackgroundImageId() !== oracle.getBackgroundImageId() || !actual.getImage(0)) {
        throw new Error('Ignored worksheet images differ from reference');
      }
      sheet.getCell('A1').value = 2;
      const reopened = (await new Reference().xlsx.load(await actual.xlsx.writeBuffer())).getWorksheet('Pictures');
      if (reopened.getImages().length !== 1 || reopened.getBackgroundImageId() !== image) {
        throw new Error('An edit discarded ignored image relationships');
      }
    }
  });
  await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const SheetJS = await import('./node_modules/xlsx/xlsx.mjs');
    const Reference = globalThis.ExcelJS.Workbook;
    const source = new Reference();
    source.addWorksheet('Range').getCell('D6').value = 5;
    const archive = SheetJS.CFB.read(await source.xlsx.writeBuffer(), { type: 'array' });
    const part = 'xl/worksheets/sheet1.xml';
    const xml = new TextDecoder().decode(SheetJS.CFB.find(archive, '/' + part).content);
    const expanded = xml.replace(/<dimension ref="[^"]+"/, '<dimension ref="A1:Z20"');
    if (xml === expanded) throw new Error('Could not extend declared worksheet dimensions');
    SheetJS.CFB.utils.cfb_del(archive, '/' + part);
    SheetJS.CFB.utils.cfb_add(archive, part, new TextEncoder().encode(expanded));
    const bytes = Uint8Array.from(SheetJS.CFB.write(archive, { type: 'array', fileType: 'zip' }));
    for (const Type of [Workbook, Reference]) {
      const book = await new Type().xlsx.load(bytes, { ignoreNodes: ['dimension'] });
      const sheet = book.getWorksheet('Range');
      if (sheet.dimensions.range !== 'D6:D6' || sheet.getCell('D6').value !== 5) {
        throw new Error('Ignoring declared dimensions changed worksheet contents');
      }
    }
  });
  await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const Reference = globalThis.ExcelJS.Workbook;
    const source = new Reference();
    const sourceSheet = source.addWorksheet('Notes');
    sourceSheet.getCell('A1').note = 'Plain';
    sourceSheet.getCell('B2').note = {
      margins: { insetmode: 'custom', inset: [0.2, 0.3, 0.4, 0.5] },
      protection: { locked: 'False', lockText: 'True' },
      editAs: 'oneCells',
      texts: [{ text: 'Styled ', font: { name: 'Calibri', size: 11, bold: true, color: { argb: 'FF123456' } } },
        { text: 'plain' }],
    };
    const bytes = await source.xlsx.writeBuffer();
    const actual = await new Workbook().xlsx.load(bytes);
    const sheet = actual.getWorksheet('Notes');
    const note = sheet.getCell('B2').note;
    if (sheet.getCell('A1').note !== 'Plain' || note.texts.length !== 2 ||
        note.texts[0].text !== 'Styled ' || note.texts[0].font.bold !== true ||
        note.margins.insetmode !== 'custom' || JSON.stringify(note.margins.inset) !== '[0.2,0.3,0.4,0.5]' ||
        note.protection.locked !== 'False' || note.protection.lockText !== 'True' || note.editAs !== 'oneCells' ||
        note.texts[0].font.color.argb !== 'FF123456' || note.texts[1].text !== 'plain' ||
        JSON.stringify(sheet.getCell('B2').model.comment.note.texts) !== JSON.stringify(note.texts)) {
      throw new Error('Legacy note model lost rich text or font settings');
    }
  });
  await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const Reference = globalThis.ExcelJS.Workbook;
    const source = new Reference();
    const original = source.addWorksheet('DataBars');
    original.getCell('A1').value = 10;
    original.addConditionalFormatting({ ref: 'A1:A2', rules: [{
      type: 'dataBar', gradient: false, border: true, borderColor: { argb: 'FF112233' },
      color: { argb: 'FF668899' }, cfvo: [{ type: 'min' }, { type: 'max' }],
    }] });
    const bytes = await source.xlsx.writeBuffer();
    for (const ignoreNodes of [[], ['extLst']]) {
      const actual = await new Workbook().xlsx.load(bytes, { ignoreNodes });
      const expected = await new Reference().xlsx.load(bytes, { ignoreNodes });
      const sheet = actual.getWorksheet('DataBars');
      const rule = sheet.conditionalFormattings[0].rules[0];
      const oracle = expected.getWorksheet('DataBars').conditionalFormattings[0].rules[0];
      for (const key of ['x14Id', 'minLength', 'maxLength', 'gradient', 'border', 'borderColor', 'color', 'cfvo']) {
        if (JSON.stringify(rule[key]) !== JSON.stringify(oracle[key])) throw new Error('Data bar property differs: ' + key);
      }
      if (!ignoreNodes.length) {
        const copy = new Workbook();
        copy.model = actual.model;
        const copied = await new Reference().xlsx.load(await copy.xlsx.writeBuffer());
        const copiedRule = copied.getWorksheet('DataBars').conditionalFormattings[0].rules[0];
        for (const key of ['minLength', 'maxLength', 'gradient', 'border', 'borderColor', 'color', 'cfvo']) {
          if (JSON.stringify(copiedRule[key]) !== JSON.stringify(oracle[key])) {
            throw new Error('Workbook model copy changed the data bar option: ' + key);
          }
        }
      }
      sheet.getCell('A1').value = 11;
      const reopened = await new Reference().xlsx.load(await actual.xlsx.writeBuffer());
      if (reopened.getWorksheet('DataBars').conditionalFormattings[0].rules[0].gradient !== false) {
        throw new Error('Editing discarded the data bar extension');
      }
    }
    const { CFB } = await import('./node_modules/@ooxml-rs/xlsx/src/sheetjs-browser.js');
    const archive = CFB.read(bytes, { type: 'array' });
    const part = 'xl/worksheets/sheet1.xml';
    const xml = new TextDecoder().decode(CFB.find(archive, '/' + part).content);
    const extendedXml = xml.replace('<x14:dataBar ', '<x14:dataBar showValue="0" ')
      .replace('<x14:borderColor', '<x14:fillColor rgb="FFABCDEF"/><x14:borderColor');
    if (extendedXml === xml) throw new Error('Could not modify the data bar fixture');
    CFB.utils.cfb_del(archive, '/' + part);
    CFB.utils.cfb_add(archive, part, new TextEncoder().encode(extendedXml));
    const extendedBytes = CFB.write(archive, { fileType: 'zip', type: 'array' });
    const linked = await new Workbook().xlsx.load(new Uint8Array(extendedBytes));
    const extOnly = await new Workbook().xlsx.load(new Uint8Array(extendedBytes), { ignoreNodes: ['conditionalFormatting'] });
    const linkedRule = linked.getWorksheet('DataBars').conditionalFormattings[0].rules[0];
    const extRule = extOnly.getWorksheet('DataBars').conditionalFormattings[0].rules[0];
    if (linkedRule.color?.argb !== 'FF668899' || linkedRule.fillColor?.argb !== 'FFABCDEF' ||
        extRule.fillColor?.argb !== 'FFABCDEF' || extRule.showValue !== false) {
      throw new Error('Data bar projection lost the extended fill or visibility');
    }
  });
  await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const { ValueType } = await import('./node_modules/@ooxml-rs/xlsx/src/exceljs.js');
    const Reference = globalThis.ExcelJS.Workbook;
    const workbook = new Reference();
    const sheet = workbook.addWorksheet('Icons');
    sheet.getCell('A1').value = 1;
    sheet.addConditionalFormatting({ ref: 'A1:A3', rules: [
      { type: 'expression', formulae: ['A1>0'] },
      { type: 'iconSet', iconSet: '3Triangles', reverse: true, showValue: false,
        icons: [{ iconSet: 'NoIcons' }, { iconSet: '5Boxes' }, { iconSet: '3Stars' }],
        cfvo: [{ type: 'num', value: 0 }, { type: 'num', value: 1 }, { type: 'num', value: 2 }] },
    ] });
    sheet.addConditionalFormatting({ ref: 'B1', rules: [{
      type: 'iconSet', iconSet: '3Stars', cfvo: [{ type: 'num', value: 0 }, { type: 'num', value: 1 }, { type: 'num', value: 2 }],
    }] });
    const thresholdBook = new Reference();
    thresholdBook.addWorksheet('Threshold').addConditionalFormatting({ ref: 'A1:A3', rules: [{
      type: 'iconSet', iconSet: '3Stars', cfvo: [
        { type: 'num', value: 0 },
        { type: 'formula', value: 'AVERAGE(A1:A3)' },
        { type: 'formula', value: '2+A1' },
      ],
    }] });
    const thresholdWorkbook = await new Workbook().xlsx.load(await thresholdBook.xlsx.writeBuffer());
    const thresholdSheet = thresholdWorkbook.getWorksheet('Threshold');
    const [thresholdBlock] = thresholdSheet.model.conditionalFormattings;
    if (JSON.stringify(thresholdBlock.rules[0].cfvo.map(({ value }) => value)) !==
        JSON.stringify([0, 'AVERAGE(A1:A3)', '2+A1'])) {
      throw new Error('Extended icon thresholds did not retain their formula texts');
    }
    const bytes = await workbook.xlsx.writeBuffer();
    for (const ignoreNodes of [[], ['extLst'], ['conditionalFormatting']]) {
      const actual = await new Workbook().xlsx.load(bytes, { ignoreNodes });
      const expected = await new Reference().xlsx.load(bytes, { ignoreNodes });
      const current = actual.getWorksheet('Icons');
      const summarize = bookSheet => (bookSheet.conditionalFormattings ?? []).map(({ ref, rules }) => [
        ref, rules.map(rule => [rule.type, rule.iconSet, rule.reverse, rule.showValue, rule.cfvo, rule.icons]),
      ]);
      if (JSON.stringify(summarize(current)) !== JSON.stringify(summarize(expected.getWorksheet('Icons')))) {
        throw new Error('Extended icon set differs from reference: ' + ignoreNodes.join());
      }
      if (!ignoreNodes.length) {
        current.model = {
          rows: [{ number: 1, cells: [{ address: 'A1', type: ValueType.Number, value: 11 }] }],
          conditionalFormattings: current.model.conditionalFormattings,
        };
        const modelEdit = await new Reference().xlsx.load(await actual.xlsx.writeBuffer());
        if (modelEdit.getWorksheet('Icons').getCell('A1').value !== 11 ||
          JSON.stringify(summarize(modelEdit.getWorksheet('Icons'))) !== JSON.stringify(summarize(expected.getWorksheet('Icons')))) {
          throw new Error('Worksheet model assignment changed the existing extended icon rules');
        }
        const modelCopy = new Workbook();
        modelCopy.model = actual.model;
        const copied = await new Reference().xlsx.load(await modelCopy.xlsx.writeBuffer());
        if (JSON.stringify(summarize(copied.getWorksheet('Icons'))) !== JSON.stringify(summarize(expected.getWorksheet('Icons')))) {
          throw new Error('Workbook model copy lost its extended icon rules');
        }
        current.removeConditionalFormatting(0);
        const saved = await new Reference().xlsx.load(await actual.xlsx.writeBuffer());
        const remaining = saved.getWorksheet('Icons').conditionalFormattings ?? [];
        if (remaining.length !== 1 || remaining[0].ref !== 'B1' || remaining[0].rules[0].iconSet !== '3Stars') {
          throw new Error('Conditional formatting removal changed the wrong block');
        }
        current.removeConditionalFormatting(block => block.ref !== 'B1');
        const cleared = await new Reference().xlsx.load(await actual.xlsx.writeBuffer());
        if (cleared.getWorksheet('Icons').conditionalFormattings?.length) {
          throw new Error('Extension-only conditional formatting survived removal');
        }
      }
    }
    const SheetJS = await import('./node_modules/xlsx/xlsx.mjs');
    const archive = SheetJS.CFB.read(bytes, { type: 'array' });
    const part = 'xl/worksheets/sheet1.xml';
    const xml = new TextDecoder().decode(SheetJS.CFB.find(archive, '/' + part).content);
    const ids = [...xml.matchAll(/<x14:cfRule[^>]* id="([^"]+)"/g)].map(match => match[1]);
    if (ids.length !== 2) throw new Error('Ambiguous conditional format fixture lacks rule IDs');
    SheetJS.CFB.utils.cfb_del(archive, '/' + part);
    SheetJS.CFB.utils.cfb_add(archive, part, new TextEncoder().encode(xml.replaceAll(ids[1], ids[0])));
    const ambiguous = Uint8Array.from(SheetJS.CFB.write(archive, { type: 'array', fileType: 'zip' }));
    const bad = await new Workbook().xlsx.load(ambiguous);
    try {
      bad.getWorksheet('Icons').removeConditionalFormatting(0);
      throw new Error('Ambiguous extension rule ID was accepted');
    } catch (error) {
      if (error.code !== 'CONDITIONAL_FORMAT_EDIT_REFUSED') throw error;
    }
    const unchanged = await bad.xlsx.writeBuffer();
    if (unchanged.length !== ambiguous.length || unchanged.some((byte, index) => byte !== ambiguous[index])) {
      throw new Error('Refused conditional format removal changed the workbook');
    }
  });
  const styleFree = await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const Reference = globalThis.ExcelJS.Workbook;
    const pending = new Workbook();
    for (const [options, loaded, code, message] of [
      [{ unknownOption: true }, false, 'UNSUPPORTED_OPTIONS', 'Unsupported XLSX options'],
      [{ useStyles: 1 }, false, 'UNSUPPORTED_OPTIONS', 'Expected a boolean useStyles option'],
      [{ useSharedStrings: 0, useStyles: 1 }, true, 'UNSUPPORTED_OPTIONS', 'Expected a boolean useSharedStrings option'],
      [{ useStyles: 1 }, true, 'UNSUPPORTED_OPTIONS', 'Expected a boolean useStyles option'],
    ]) {
      if (loaded && !pending.worksheets.length) pending.addWorksheet('Data');
      let error;
      try {
        await pending.xlsx.writeBuffer(options);
      } catch (caught) {
        error = caught;
      }
      if (error?.code !== code || error.message !== message) {
        throw new Error('ExcelJS output option validation order changed');
      }
    }
    const empty = new Workbook();
    empty.creator = 'Packaged empty workbook';
    const emptyBytes = await empty.xlsx.writeBuffer({ useStyles: false, useSharedStrings: false });
    for (const Type of [Workbook, Reference]) {
      const reopened = await new Type().xlsx.load(emptyBytes);
      if (reopened.worksheets.length !== 0 || reopened.creator !== empty.creator) {
        throw new Error('Packaged ExcelJS zero-sheet workbook did not round-trip');
      }
    }
    const outputs = [];
    for (const Type of [Workbook, Reference]) {
      const workbook = new Type();
      const sheet = workbook.addWorksheet('Styles');
      sheet.getColumn(2).width = 23;
      sheet.getRow(1).height = 31;
      sheet.getCell('A1').value = 12.5;
      sheet.getCell('A1').numFmt = '0.00%';
      sheet.getCell('A1').font = { bold: true };
      sheet.getCell('B1').value = new Date('2020-03-04T12:30:00Z');
      sheet.getCell('B1').numFmt = 'yyyy-mm-dd';
      sheet.getCell('C1').value = 'text';
      const bytes = await workbook.xlsx.writeBuffer({
        useStyles: false, useSharedStrings: false, zip: { compression: 'STORE' },
      });
      const reopened = (await new Reference().xlsx.load(bytes)).getWorksheet('Styles');
      outputs.push([reopened.getCell('A1').value, Boolean(reopened.getCell('A1').font?.bold),
        reopened.getCell('A1').numFmt ?? 'General', reopened.getCell('B1').value.toISOString(),
        reopened.getCell('B1').numFmt, reopened.getCell('C1').value, reopened.getColumn(2).width, reopened.getRow(1).height]);
      if (Type === Workbook && !sheet.getCell('A1').font.bold) throw new Error('Export changed the open workbook');
    }
    return outputs;
  });
  assert.deepEqual(styleFree[0], styleFree[1]);
  const unsharedStrings = await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const SheetJS = await import('./dist/sheetjs/bundle.mjs');
    const workbook = new Workbook();
    const sheet = workbook.addWorksheet('Strings');
    sheet.getCell('A1').value = ' padded ';
    sheet.getCell('A1').font = { italic: true };
    sheet.getCell('A2').value = { richText: [{ text: 'name ', font: { bold: true } }, { text: 'value' }] };
    sheet.getCell('A3').value = ' padded ';
    sheet.getCell('A4').value = { formula: '"cached"', result: 'cached' };
    const before = await workbook.xlsx.writeBuffer();
    const excelOutput = await workbook.xlsx.writeBuffer({ useSharedStrings: false });
    const sheetOutput = SheetJS.write(SheetJS.read(before), { type: 'array', bookSST: false });
    const decoder = new TextDecoder();
    const results = [];
    for (const [bytes, cellType] of [[excelOutput, 'str'], [sheetOutput, 'inlineStr']]) {
      const zip = SheetJS.read(bytes, { bookFiles: true });
      const xml = decoder.decode(zip.files['xl/worksheets/sheet1.xml'].content);
      const document = new DOMParser().parseFromString(xml, 'application/xml');
      const cells = [...document.getElementsByTagNameNS('*', 'c')];
      if (zip.files['xl/sharedStrings.xml'] || cells[0].getAttribute('t') !== cellType ||
          cells[1].getAttribute('t') !== 'inlineStr') throw new Error('Unexpected string storage');
      const reopened = (await new globalThis.ExcelJS.Workbook().xlsx.load(new Uint8Array(bytes))).getWorksheet('Strings');
      results.push([reopened.getCell('A1').value, reopened.getCell('A1').font.italic,
        reopened.getCell('A2').value.richText.map(run => run.text).join(''),
        reopened.getCell('A2').value.richText[0].font.bold]);
    }
    const loaded = await new Workbook().xlsx.load(excelOutput);
    const sharedOutputs = [
      await loaded.xlsx.writeBuffer({ useSharedStrings: true }),
      SheetJS.write(SheetJS.read(sheetOutput), { type: 'array', bookSST: true }),
    ];
    for (const bytes of sharedOutputs) {
      const zip = SheetJS.read(bytes, { bookFiles: true });
      const xml = decoder.decode(zip.files['xl/sharedStrings.xml'].content);
      const stringTable = new DOMParser().parseFromString(xml, 'application/xml').documentElement;
      if (stringTable.getAttribute('count') !== '3' || stringTable.getAttribute('uniqueCount') !== '2') {
        throw new Error('Unexpected shared string counts');
      }
      const worksheet = (await new globalThis.ExcelJS.Workbook().xlsx.load(new Uint8Array(bytes))).getWorksheet('Strings');
      if (worksheet.getCell('A1').value !== ' padded ' ||
          worksheet.getCell('A3').value !== ' padded ' ||
          worksheet.getCell('A2').value.richText[0].font.bold !== true ||
          worksheet.getCell('A4').value.result !== 'cached') throw new Error('Shared string conversion lost values or formatting');
    }
    const after = await workbook.xlsx.writeBuffer();
    if (before.length !== after.length || before.some((byte, index) => after[index] !== byte)) {
      throw new Error('String conversion changed committed bytes');
    }
    return results;
  });
  assert.deepEqual(unsharedStrings, [[' padded ', true, 'name value', true], [' padded ', true, 'name value', true]]);
  const csvOutputs = await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    async function run(WorkbookType) {
      const workbook = new WorkbookType();
      const sheet = workbook.addWorksheet('Data');
      sheet.addRow([23, 'a;b', true]);
      sheet.getCell('A3').value = new Date('2020-03-04T12:34:56.000Z');
      sheet.getCell('C3').value = { error: '#REF!' };
      const sparse = workbook.addWorksheet('Sparse');
      sparse.getCell('A3').value = 'first';
      sparse.getCell('A6').value = 'last';
      const settings = [
        {},
        { sheetName: 'Sparse', map: false },
        { sheetName: 'Sparse', includeEmptyRows: false },
        { includeEmptyRows: false, formatterOptions: {
          headers: true, quoteColumns: { Amount: true },
          transform: row => [['Amount', row[0]], ['Text', row[1]]],
        } },
        { dateUTC: true, dateFormat: 'YYYY/MM/DD HH:mm:ss', formatterOptions: {
          delimiter: ';', rowDelimiter: '\r\n', writeBOM: true, quoteColumns: [true, false, true],
          headers: ['Amount', 'Text', 'Status'], includeEndRowDelimiter: true,
        } },
        { includeEmptyRows: false, formatterOptions: { headers: true, transform(row, callback) {
          callback(null, row.map(value => String(value ?? '').toUpperCase()));
        } } },
      ];
      return Promise.all(settings.map(async options =>
        Array.from(await workbook.csv.writeBuffer({ sheetName: 'Data', ...options }))));
    }
    return [await run(Workbook), await run(globalThis.ExcelJS.Workbook)];
  });
  assert.deepEqual(csvOutputs[0], csvOutputs[1]);
  const csvInput = await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const workbook = new Workbook();
    const input = '# skip\r\n\r\nskipped line\r\nignored\r\n 01 ;"日本;語";true;#REF!\r\n' +
      '\r\n04/03/2020;"line\n""two""";; tail \r\nignored';
    const bytes = new TextEncoder().encode(input);
    const stream = new ReadableStream({
      start(controller) {
        for (let position = 0; position < bytes.length; position += 1) controller.enqueue(bytes.subarray(position, position + 1));
        controller.close();
      },
    });
    const sheet = await workbook.csv.read(stream, {
      sheetName: 'Imported', dateFormats: ['DD/MM/YYYY'],
      parserOptions: { delimiter: ';', trim: true, comment: '#', skipLines: 1, skipRows: 1, ignoreEmpty: true, maxRows: 2 },
    });
    return [sheet.name, sheet.rowCount, sheet.getRow(1).values.slice(1),
      sheet.getCell('A2').value.getTime() === new Date(2020, 2, 4).getTime(),
      sheet.getCell('B2').value, sheet.getCell('C2').value, sheet.getCell('D2').value, stream.locked];
  });
  assert.deepEqual(csvInput, ['Imported', 2, [1, '日本;語', true, { error: '#REF!' }],
    true, 'line\n"two"', null, 'tail', false]);

  const formulas = await page.evaluate(async () => {
    const { Workbook } = await import('./dist/exceljs/bundle.mjs');
    const readFormulas = (book) => {
      const sheet = book.getWorksheet('Formulas');
      return ['B2', 'B3', 'D2', 'E2'].map((address) => {
        const cell = sheet.getCell(address);
        return [cell.value, cell.formula ?? null];
      });
    };
    const create = async (WorkbookType) => {
      const book = new WorkbookType();
      const sheet = book.addWorksheet('Formulas');
      sheet.fillFormula('B2:B3', '$A2+C$1', [8, 9]);
      sheet.fillFormula('D2:E2', '1+2', [[3, 'result']], 'array');
      return book.xlsx.writeBuffer();
    };
    const reference = globalThis.ExcelJS.Workbook;
    return {
      writtenByAdapter: readFormulas(await new reference().xlsx.load(await create(Workbook))),
      writtenByReference: readFormulas(await new Workbook().xlsx.load(await create(reference))),
    };
  });
  const expectedFormulas = [
    [{ formula: '$A2+C$1', shareType: 'shared', ref: 'B2:B3', result: 8 }, '$A2+C$1'],
    [{ sharedFormula: 'B2', result: 9 }, '$A3+C$1'],
    [{ formula: '1+2', shareType: 'array', ref: 'D2:E2', result: 3 }, '1+2'],
    ['result', null],
  ];
  assert.deepEqual(formulas.writtenByAdapter, expectedFormulas);
  assert.deepEqual(formulas.writtenByReference, expectedFormulas);
  const formatDifferences = await page.evaluate(async () => {
    const { SSF } = await import('./dist/ssf/bundle.mjs');
    const reference = (await import('./node_modules/xlsx/xlsx.mjs')).SSF;
    const formats = [
      ...Array.from({ length: 83 }, (_, id) => id),
      '0.0, "k"', '0.##%', '# ?/16', 'dddd, mmmm d, yyyy',
      '0.0;[Red](0.0);"zero";"label: "@', '0.0"x;y";(0.0)',
      '[>=100]0.0" kg";[<0](0.0);0.0',
    ];
    const cases = [];
    for (const format of formats) {
      for (const value of [-1234.5, 0, 0.5, 45000.62734, 'abc']) {
        const actual = SSF.format(format, value);
        const expected = reference.format(format, value);
        if (actual !== expected) cases.push({ format, value, actual, expected });
      }
    }
    return cases;
  });
  assert.deepEqual(formatDifferences, []);
  const spreadsheetMl = await page.evaluate(async () => {
    const adapter = await import('./dist/sheetjs/bundle.mjs');
    const reference = await import('./node_modules/xlsx/xlsx.mjs');
    const sheet = {
      '!ref': 'A1:D3',
      A1: { t: 's', v: '日本語', c: [{ a: 'Editor', t: 'Reviewed' }] },
      B2: { t: 'n', v: 12, f: '$D$3+1', F: 'B2:C2' },
      C2: { t: 'n', v: 13, F: 'B2:C2' },
      D3: { t: 'n', v: 11 },
      '!autofilter': { ref: 'A1:D3' },
    };
    const book = reference.utils.book_new(sheet, 'Data');
    book.Props = { Title: 'Interoperability' };
    book.Workbook = { Names: [{ Name: 'Input', Ref: 'Data!$D$3' }] };
    const describe = (workbook) => {
      const worksheet = workbook.Sheets.Data;
      const { a, t } = worksheet.A1.c[0];
      return {
        title: workbook.Props.Title,
        name: workbook.Workbook.Names[0].Ref,
        first: worksheet.A1.v,
        comment: { a, t },
        formula: worksheet.B2.f,
        array: [worksheet.B2.F, worksheet.C2.F],
        filter: worksheet['!autofilter'].ref,
      };
    };
    const options = { bookType: 'xlml', type: 'string' };
    const writtenByAdapter = adapter.write(book, options);
    adapter.stream.set_readable(() => ({ chunks: [], push(chunk) { this.chunks.push(chunk); } }));
    const stream = adapter.stream.to_xlml(book, { stride: 1 });
    while (stream.chunks.at(-1) !== null) stream._read();
    const writtenByReference = reference.write(book, options);
    return {
      writtenByAdapter: describe(reference.read(writtenByAdapter, { type: 'string' })),
      writtenByReference: describe(adapter.read(writtenByReference, { type: 'string' })),
      streamMatches: stream.chunks.join('') === writtenByAdapter,
    };
  });
  const expectedSpreadsheetMl = {
    title: 'Interoperability', name: 'Data!$D$3', first: '日本語',
    comment: { a: 'Editor', t: 'Reviewed' }, formula: '$D$3+1',
    array: ['B2:C2', 'B2:C2'], filter: 'A1:D3',
  };
  assert.deepEqual(spreadsheetMl.writtenByAdapter, expectedSpreadsheetMl);
  assert.deepEqual(spreadsheetMl.writtenByReference, expectedSpreadsheetMl);
  assert.equal(spreadsheetMl.streamMatches, true);
  const csp = "default-src 'none'; script-src 'self' 'nonce-browser-probe' 'wasm-unsafe-eval'; connect-src 'self'";
  const bundledPage = '**/bundles.html?both';
  await page.route(bundledPage, async (route) => {
    const response = await route.fetch();
    const html = (await response.text()).replace('<script type="module">', '<script type="module" nonce="browser-probe">');
    await route.fulfill({ response, body: html, headers: { ...response.headers(), 'content-security-policy': csp } });
  });
  await passing(page, '/bundles.html?both');
  await page.unroute(bundledPage);

  let requests = 0;
  await page.route('**/*.wasm', (route) => {
    requests++;
    return route.fulfill({ status: 404 });
  });
  await page.goto(origin + '/bundles.html?both');
  await page.waitForFunction(() => document.body.innerText !== 'Running');
  assert.ok(requests > 0, 'expected an attempted Wasm load');
  assert.match(await page.locator('body').innerText(), /^FAIL/);
  await page.unroute('**/*.wasm');

  for (const name of ['scalar', 'sparse']) {
    // A fresh document gives each fixture its own Wasm instance.
    await page.goto(origin + '/fixtures/');
    const measurement = await page.evaluate(async (name) => {
      const data = await (await fetch('./' + name + '.xlsx')).arrayBuffer();
      const memories = new Set();
      for (const method of ['instantiate', 'instantiateStreaming']) {
        const original = WebAssembly[method];
        if (!original) continue;
        WebAssembly[method] = async (...args) => {
          const result = await original(...args);
          const memory = (result.instance ?? result).exports.memory;
          if (memory) memories.add(memory);
          return result;
        };
      }
      const start = performance.now();
      const { Workbook, read, write } = await import('../dist/both/bundle.mjs');
      const initialized = performance.now();
      const excel = new Workbook();
      await excel.xlsx.load(data);
      const cell = excel.worksheets[0].getCell('A1');
      const loaded = performance.now();
      for (let value = 700; value < 710; value++) cell.value = value;
      const edited = performance.now();
      await excel.xlsx.writeBuffer();
      const excelSaved = performance.now();
      const book = read(data);
      book.Sheets[book.SheetNames[0]].A1.v = 800;
      const sheetLoaded = performance.now();
      write(book, { type: 'array', bookType: 'xlsx' });
      const sheetSaved = performance.now();
      return {
        fixture: name, bytes: data.byteLength,
        wasmMemoryHighWaterBytes: Math.max(0, ...[...memories].map((memory) => memory.buffer.byteLength)),
        initializationMs: initialized - start,
        excelLoadMs: loaded - initialized,
        excelTenSettersMs: edited - loaded,
        excelSaveMs: excelSaved - edited,
        sheetLoadAndProjectionMs: sheetLoaded - excelSaved,
        sheetSaveMs: sheetSaved - sheetLoaded,
      };
    }, name);
    console.log(engine, browser.version(), JSON.stringify(measurement));
  }
  if (process.env.OOXML_PROFILE_SETTERS === '1') {
    await page.goto(origin + '/fixtures/');
    const phases = await page.evaluate(async () => {
      const { default: init, XlsxDocument } = await import('../node_modules/@ooxml-rs/xlsx/wasm/ooxml_xlsx_wasm.js');
      await init();
      const input = new Uint8Array(await (await fetch('./sparse.xlsx')).arrayBuffer());
      const initial = new XlsxDocument(input);
      const [sheet] = initial.worksheet_names();
      initial.free();
      const samples = [];
      for (let trial = 0; trial < 7; trial++) {
        let bytes = input;
        const totals = { openMs: 0, clearLinkMs: 0, scalarWriteMs: 0, saveMs: 0 };
        for (let value = 700; value < 710; value++) {
          const start = performance.now();
          const candidate = new XlsxDocument(bytes);
          const opened = performance.now();
          try {
            candidate.clear_cell_hyperlink(sheet, 'A1');
            const cleared = performance.now();
            candidate.write_scalar(sheet, 0, 0, value, true, true);
            const edited = performance.now();
            bytes = candidate.to_bytes();
            const saved = performance.now();
            totals.openMs += opened - start;
            totals.clearLinkMs += cleared - opened;
            totals.scalarWriteMs += edited - cleared;
            totals.saveMs += saved - edited;
          } finally {
            candidate.free();
          }
        }
        if (trial >= 2) samples.push(totals);
      }
      const median = key => samples.map(sample => sample[key]).sort((a, b) => a - b)[2];
      return { fixture: 'sparse', inputBytes: input.byteLength, editsPerTrial: 10, trials: samples.length,
        openMs: median('openMs'), clearLinkMs: median('clearLinkMs'),
        scalarWriteMs: median('scalarWriteMs'), saveMs: median('saveMs') };
    });
    console.log(engine, browser.version(), 'ExcelJS scalar update phases', JSON.stringify(phases));
  }
  console.log(engine, browser.version(), 'distribution and reference checks PASS');
} finally {
  await browser.close();
}
