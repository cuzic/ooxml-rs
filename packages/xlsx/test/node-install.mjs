// Copy this file into the scratch project after installing the packed tarball.
import assert from 'node:assert/strict';
import * as fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable, Writable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { FormulaType, ModelContainer, ValueType, Workbook, stream } from '@ooxml-rs/xlsx/exceljs';
import * as SheetJS from '@ooxml-rs/xlsx/sheetjs';

const directory = fs.mkdtempSync(join(tmpdir(), 'ooxml-xlsx-test-'));
try {
  const { XlsbSharedStrings, XlsxDocument, XlsxRowStream, XlsxZipReader, XlsxZipStream, encode_xlsx_row } = await import(new URL('../wasm/ooxml_xlsx_wasm.js',
    import.meta.resolve('@ooxml-rs/xlsx/exceljs')));
  const rowLifecycleBook = new Workbook();
  const rowLifecycleSheet = rowLifecycleBook.addWorksheet('Row lifecycle');
  const destroyedRow = rowLifecycleSheet.getRow(1);
  assert.equal(destroyedRow.destroy(), undefined);
  assert.equal(rowLifecycleSheet.getRow(1), destroyedRow);
  assert.equal(destroyedRow.number, 1);
  assert.equal(destroyedRow.worksheet, undefined);
  assert.equal(destroyedRow.style, undefined);
  assert.throws(() => destroyedRow.getCell(1), TypeError);
  assert.throws(() => destroyedRow.commit(), TypeError);
  assert.equal(destroyedRow.destroy(), undefined);

  await assert.rejects(rowLifecycleBook.xlsx.writeBuffer(), TypeError);
  rowLifecycleBook.removeWorksheet(rowLifecycleSheet.id);
  assert.equal((await new Workbook().xlsx.load(await rowLifecycleBook.xlsx.writeBuffer())).worksheets.length, 0);

  const rowDestroyFile = join(directory, 'destroyed-streaming-row.xlsx');
  const rowDestroyWriter = new stream.xlsx.WorkbookWriter({ filename: rowDestroyFile });
  const rowDestroySheet = rowDestroyWriter.addWorksheet('Row lifecycle');
  rowDestroySheet.getCell('A1').value = 1;
  const discardedRow = rowDestroySheet.getRow(2);
  discardedRow.getCell(1).value = 2;
  discardedRow.getCell(1).note = 'Discarded with the row';
  discardedRow.destroy();
  assert.equal(rowDestroySheet.getRow(2), discardedRow);
  assert.equal(discardedRow.worksheet, undefined);
  assert.equal(discardedRow.style, undefined);
  assert.equal(discardedRow.hasValues, false);
  assert.throws(() => discardedRow.values, TypeError);
  assert.throws(() => discardedRow.getCell(1), TypeError);
  assert.throws(() => discardedRow.commit(), TypeError);
  rowDestroySheet.getCell('A3').value = 3;
  await rowDestroyWriter.commit();
  const rowDestroyRead = (await new Workbook().xlsx.readFile(rowDestroyFile)).getWorksheet(1);
  assert.deepEqual(['A1', 'A2', 'A3'].map(key => rowDestroyRead.getCell(key).value), [1, null, 3]);
  assert.equal(rowDestroyRead.getCell('A2').note, undefined);

  const rowDestroyReader = new stream.xlsx.WorkbookReader(rowDestroyFile);
  const visitedRows = [];
  for await (const worksheet of rowDestroyReader) {
    for await (const row of worksheet) {
      const cell = row.getCell(1);
      visitedRows.push([row.number, cell.value]);
      assert.equal(cell.worksheet, worksheet);
      assert.equal(cell.workbook, rowDestroyReader);
      assert.deepEqual(cell.fullAddress, {
        sheetName: worksheet.name, address: 'A' + row.number, row: row.number, col: 1,
      });
      assert.equal(cell.$col$row, '$A$' + row.number);
      assert.equal(row.hasValues, true);
      assert.equal(row.destroy(), undefined);
      assert.equal(row.worksheet, undefined);
      assert.equal(row.style, undefined);
      assert.equal(row.hasValues, false);
      assert.throws(() => row.values, TypeError);
      assert.throws(() => row.dimensions, TypeError);
      assert.throws(() => row.getCell(1), TypeError);
      assert.throws(() => row.eachCell(() => {}), TypeError);
      assert.equal(cell.value, visitedRows.at(-1)[1]);
      assert.equal(cell.worksheet, undefined);
      assert.equal(cell.row, row.number);
      assert.equal(cell.col, 1);
      assert.throws(() => cell.fullAddress, TypeError);
    }
  }
  assert.deepEqual(visitedRows, [[1, 1], [3, 3]]);

  const cellDestroyReader = new stream.xlsx.WorkbookReader(rowDestroyFile);
  const cellDestroyRows = [];
  for await (const worksheet of cellDestroyReader) {
    for await (const row of worksheet) {
      const cell = row.getCell(1);
      cellDestroyRows.push([row.number, cell.value]);
      assert.equal(cell.destroy(), undefined);
      assert.equal(row.getCell(1), cell);
      assert.equal(cell.address, undefined);
      assert.equal(cell.style, undefined);
      assert.throws(() => cell.value, TypeError);
      assert.throws(() => row.values, TypeError);
      assert.throws(() => row.dimensions, TypeError);
      assert.throws(() => row.hasValues, TypeError);
    }
  }
  assert.deepEqual(cellDestroyRows, [[1, 1], [3, 3]]);

  const readerMerge = new stream.xlsx.WorkbookReader(rowDestroyFile);
  const readerMergeRows = [];
  for await (const worksheet of readerMerge) {
    for await (const row of worksheet) {
      readerMergeRows.push(row.number);
      const master = row.getCell(1);
      const child = row.getCell(2);
      assert.equal(child.merge, master.merge);
      assert.equal(child.destroy, master.destroy);
      const value = master.value;
      child.merge(master);
      assert.equal(child.master, master);
      assert.equal(child.model.master, master.address);
      assert.equal(child.model.type, ValueType.Merge);
      assert.equal(child.type, ValueType.Merge);
      assert.equal(child.effectiveType, ValueType.Number);
      assert.equal(child.text, String(value));
      assert.equal(child.toCsvString(), '');
      assert.equal(child.formulaType, undefined);
      assert.equal(child.isMerged, true);
      assert.equal(master.isMerged, true);
      assert.equal(child.isMergedTo(master), true);
      assert.equal(master.isMergedTo(child), false);
      assert.equal(child.value, value);
      child.value = 7;
      assert.equal(master.value, 7);
      assert.deepEqual(row.dimensions, { min: 1, max: 2 });
      assert.throws(() => master.merge(child), { code: 'MERGE_EDIT_REFUSED' });
      child.unmerge();
      assert.equal(child.master, child);
      assert.equal(child.type, ValueType.Null);
      assert.equal(child.effectiveType, ValueType.Null);
      assert.equal(child.isMerged, false);
      assert.equal(master.isMerged, false);
      assert.equal(child.isMergedTo(master), false);
      master.addMergeRef();
      assert.equal(master.isMerged, true);
      master.releaseMergeRef();
      assert.equal(master.isMerged, false);
      assert.equal(child.value, null);
      assert.deepEqual(row.dimensions, { min: 1, max: 1 });
    }
  }
  assert.deepEqual(readerMergeRows, [1, 3]);

  const sparseReadBook = new Workbook();
  const sparseReadSheet = sparseReadBook.addWorksheet('Sparse read');
  sparseReadSheet.getCell('A1').value = 1;
  sparseReadSheet.getCell('D1').value = 4;
  const sparseReadBytes = await sparseReadBook.xlsx.writeBuffer();
  for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([sparseReadBytes]))) {
    for await (const row of worksheet) {
      assert.equal(row.cellCount, 4);
      assert.equal(row.actualCellCount, 2);
      assert.equal(row.findCell(2), undefined);
      const cells = [];
      row.eachCell({ includeEmpty: true }, (cell, column) => cells.push([column, cell.value]));
      assert.deepEqual(cells, [[1, 1], [2, null], [3, null], [4, 4]]);
      assert.equal(row.findCell(2), row.getCellEx({ address: 'B1', row: 1, col: 2 }));
      row.getCell(6);
      assert.equal(row.cellCount, 6);
      assert.equal(row.actualCellCount, 2);
      row.getCell(2).value = 2;
      const populated = [];
      row.eachCell((cell, column) => populated.push([column, cell.value]));
      assert.deepEqual(populated, [[1, 1], [2, 2], [4, 4]]);
      assert.deepEqual(row.model.cells.map(cell => [cell.address, cell.type, cell.value]), [
        ['A1', ValueType.Number, 1], ['B1', ValueType.Number, 2], ['C1', ValueType.Null, undefined],
        ['D1', ValueType.Number, 4], ['F1', ValueType.Null, undefined],
      ]);
      assert.equal(row.model.min, 1);
      assert.equal(row.model.max, 6);
    }
  }

  for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([sparseReadBytes]))) {
    for await (const row of worksheet) {
      const cell = row.getCell('A');
      cell.model = {
        address: 'A1', type: ValueType.Formula, formula: '2+3', result: 5, style: { numFmt: '0.00' },
      };
      assert.deepEqual(cell.value, { formula: '2+3', result: 5 });
      assert.equal(cell.formulaType, FormulaType.Master);
      assert.equal(cell.numFmt, '0.00');
      assert.throws(() => { cell.model = { address: 'B1', type: ValueType.Number, value: 6 }; },
        { code: 'INVALID_ADDRESS' });
      assert.equal(cell.result, 5);
      cell.model = { type: ValueType.Null };
      assert.equal(cell.value, null);
      assert.equal(cell.numFmt, undefined);
      assert.deepEqual(row.dimensions, { min: 4, max: 4 });
    }
  }

  for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([sparseReadBytes]))) {
    for await (const row of worksheet) {
      const oldCell = row.getCell(1);
      row.model = {
        number: 1, height: 23, hidden: true, outlineLevel: 1,
        cells: [
          { address: 'B1', type: ValueType.Number, value: 2 },
          { type: ValueType.String, value: 'next' },
        ],
      };
      assert.notEqual(row.getCell(1), oldCell);
      assert.equal(oldCell.value, 1);
      assert.deepEqual(row.values, [, , 2, 'next']);
      assert.equal(row.height, 23);
      assert.equal(row.hidden, true);
      assert.equal(row.outlineLevel, 1);
      assert.equal(row.getCell(3).worksheet, worksheet);
      assert.throws(() => {
        row.model = { number: 1, cells: [
          { address: 'A1', type: ValueType.Number, value: 10 },
          { address: 'B1', type: ValueType.Number, value: 'wrong' },
        ] };
      }, { code: 'INVALID_VALUE' });
      oldCell.destroy();
      assert.deepEqual(row.dimensions, { min: 2, max: 3 });
      assert.deepEqual(row.values, [, , 2, 'next']);
    }
  }

  for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([sparseReadBytes]))) {
    for await (const row of worksheet) {
      const previous = row.getCell(1);
      row.values = [, 11, , 33];
      assert.deepEqual(row.values, [, 11, , 33]);
      assert.equal(row.cellCount, 3);
      assert.equal(previous.value, 1);
      worksheet.setColumnKey('entry', worksheet.getColumn('D'));
      row.values = { entry: { formula: '2+3', result: 5 }, ignored: 9 };
      assert.deepEqual(row.values, [, , , , { formula: '2+3', result: 5 }]);
      assert.equal(row.getCell(4).formula, '2+3');
      const tooWide = [];
      tooWide[16385] = 1;
      assert.throws(() => { row.values = tooWide; }, { code: 'INVALID_ADDRESS' });
      assert.equal(row.getCell(4).result, 5);
      row.values = null;
      assert.deepEqual(row.values, []);
      assert.equal(row.cellCount, 0);
    }
  }

  for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([sparseReadBytes]))) {
    for await (const row of worksheet) {
      const cell = row.getCell(1);
      cell.value = { formula: '1+2', result: 3 };
      assert.equal(cell.formula, '1+2');
      assert.equal(cell.result, 3);
      assert.equal(cell.hyperlink, undefined);
      cell.value = { text: 'Link', hyperlink: 'https://example.test' };
      assert.equal(cell.hyperlink, 'https://example.test');
      assert.equal(cell.formula, undefined);
      assert.equal(cell.result, undefined);
      cell.value = null;
      assert.deepEqual(row.dimensions, { min: 4, max: 4 });
      row.getCell(6).value = 6;
      assert.deepEqual(row.dimensions, { min: 4, max: 6 });
    }
  }

  for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([sparseReadBytes]))) {
    for await (const row of worksheet) {
      const fourth = row.getCell(4);
      fourth.style.numFmt = '0.00';
      row.splice(2, 0, 'new');
      assert.deepEqual(row.values, [, 1, 'new', , , 4]);
      assert.equal(row.getCell(5).numFmt, '0.00');
      assert.equal(row.findCell(4), undefined);
      assert.equal(fourth.value, 4);
      row.splice(2, 1);
      assert.deepEqual(row.values, [, 1, , , 4]);
      assert.equal(row.cellCount, 5);
      assert.equal(row.getCell(4).numFmt, '0.00');
      row.getCell(16384).value = 1;
      assert.throws(() => row.splice(1, 0, 5), { code: 'INVALID_ADDRESS' });
      assert.equal(row.getCell(16384).value, 1);
      assert.equal(row.getCell(1).value, 1);
    }
  }

  const tallRowWriter = new stream.xlsx.WorkbookWriter({ filename: join(directory, 'destroyed-tall-row.xlsx') });
  const tallRow = tallRowWriter.addWorksheet('Tall row').getRow(1);
  tallRow.height = 23;
  tallRow.destroy();
  assert.equal(tallRow.height, 23);
  await assert.rejects(tallRowWriter.commit(), TypeError);

  const streamCellWriter = new stream.xlsx.WorkbookWriter({ filename: join(directory, 'destroyed-streaming-cell.xlsx') });
  const streamCellSheet = streamCellWriter.addWorksheet('Cell lifecycle');
  const streamCell = streamCellSheet.getCell('A1');
  streamCell.value = 42;
  assert.equal(streamCell.destroy(), undefined);
  assert.equal(streamCellSheet.getCell('A1'), streamCell);
  assert.equal(streamCell.address, undefined);
  assert.equal(streamCell.style, undefined);
  for (const key of ['row', 'col', 'worksheet', 'value', 'master', 'type']) {
    assert.throws(() => streamCell[key], TypeError);
  }
  await assert.rejects(streamCellWriter.commit(), TypeError);

  const cellLifecycleBook = new Workbook();
  const cellLifecycleSheet = cellLifecycleBook.addWorksheet('Cell lifecycle');
  const destroyedCell = cellLifecycleSheet.getCell('A1');
  destroyedCell.value = 42;
  assert.equal(destroyedCell.destroy(), undefined);
  assert.equal(cellLifecycleSheet.getCell('A1'), destroyedCell);
  assert.equal(destroyedCell.address, undefined);
  assert.equal(destroyedCell.style, undefined);
  for (const key of ['row', 'col', 'worksheet', 'workbook', 'model', 'value', 'type']) {
    assert.throws(() => destroyedCell[key], TypeError);
  }
  assert.throws(() => { destroyedCell.value = 7; }, TypeError);
  assert.equal(destroyedCell.destroy(), undefined);
  await assert.rejects(cellLifecycleBook.xlsx.writeBuffer(), TypeError);
  cellLifecycleBook.removeWorksheet(cellLifecycleSheet.id);
  assert.equal((await new Workbook().xlsx.load(await cellLifecycleBook.xlsx.writeBuffer())).worksheets.length, 0);

  const zipLevelBook = new Workbook();
  const zipLevelSheet = zipLevelBook.addWorksheet('Compression');
  const zipCellValue = 'compressed value '.repeat(100);
  zipLevelSheet.getCell('A1').value = zipCellValue;
  const writeAtLevel = level => zipLevelBook.xlsx.writeBuffer({ zip: { compressionOptions: { level } } });
  const normalLevelBytes = await writeAtLevel(6);
  for (const level of [-1, 0]) {
    assert.deepEqual(await writeAtLevel(level), normalLevelBytes);
  }
  const fastLevelBytes = await writeAtLevel(1);
  assert.notDeepEqual(fastLevelBytes, normalLevelBytes);
  assert.equal((await new Workbook().xlsx.load(fastLevelBytes)).getWorksheet('Compression').getCell('A1').value,
    zipCellValue);
  await assert.rejects(() => zipLevelBook.xlsx.writeBuffer({
    zip: { compression: 'STORE', compressionOptions: { level: 10 } },
  }), /Expected a DEFLATE level between -1 and 9/);
  for (const [compression, mixedCase, zipMethod] of [['STORE', 'sToRe', 0], ['DEFLATE', 'Deflate', 8]]) {
    const bytes = await zipLevelBook.xlsx.writeBuffer({ zip: { compression } });
    assert.deepEqual(await zipLevelBook.xlsx.writeBuffer({ zip: { compression: mixedCase } }), bytes);
    const header = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    assert.equal(header.getUint32(0, true), 0x04034b50);
    assert.equal(header.getUint16(8, true), zipMethod);
  }
  const emptyBook = new Workbook();
  emptyBook.creator = 'Empty author';
  emptyBook.properties.date1904 = true;
  emptyBook.calcProperties.fullCalcOnLoad = true;
  const emptyWriter = new stream.xlsx.WorkbookWriter({ creator: 'Empty author', zip: { store: true } });
  await emptyWriter.commit();
  for (const bytes of [await emptyBook.xlsx.writeBuffer({ zip: { compression: 'STORE' } }), emptyWriter.stream.read()]) {
    const emptyReader = new stream.xlsx.WorkbookReader(Readable.from([bytes]));
    const streamedSheets = [];
    for await (const sheet of emptyReader) streamedSheets.push(sheet);
    assert.deepEqual(streamedSheets, []);
    const emptyReopened = await new Workbook().xlsx.load(bytes);
    assert.deepEqual(emptyReopened.worksheets, []);
    assert.equal(emptyReopened.creator, 'Empty author');
    assert.equal(emptyReopened.addWorksheet('New first').id, 1);
    assert.equal((await new Workbook().xlsx.load(await emptyReopened.xlsx.writeBuffer())).getWorksheet(1).name, 'New first');
    const archive = new XlsxZipReader(bytes.length, (offset, length) => bytes.subarray(offset, offset + length));
    try {
      assert.ok(!archive.entry_names().some(name => name.startsWith('xl/worksheets/')));
      assert.match(Buffer.from(archive.read_metadata('xl/workbook.xml', 65536)).toString(), /<sheets(?:\s*\/>|><\/sheets>)/);
      assert.match(Buffer.from(archive.read_metadata('docProps/core.xml', 65536)).toString(), /Empty author/);
    } finally {
      archive.free();
    }
  }
  const emptyWithoutStrings = await emptyBook.xlsx.writeBuffer({ useStyles: false, useSharedStrings: false });
  const emptyWithoutStringsArchive = new XlsxZipReader(emptyWithoutStrings.length,
    (offset, length) => emptyWithoutStrings.subarray(offset, offset + length));
  try {
    assert.ok(!emptyWithoutStringsArchive.entry_names().includes('xl/sharedStrings.xml'));
  } finally {
    emptyWithoutStringsArchive.free();
  }
  const reloadedEmpty = await new Workbook().xlsx.load(await emptyBook.xlsx.writeBuffer());
  assert.equal(reloadedEmpty.properties.date1904, true);
  assert.equal(reloadedEmpty.calcProperties.fullCalcOnLoad, true);
  assert.equal(emptyBook.addWorksheet('First').id, 1);
  assert.equal(emptyBook.properties.date1904, true);
  assert.equal(emptyBook.calcProperties.fullCalcOnLoad, true);
  const removedLastBook = new Workbook();
  removedLastBook.creator = 'Still here';
  const removedLastSheet = removedLastBook.addWorksheet('Remove this');
  removedLastSheet.getCell('A1').value = 123;
  removedLastSheet.getCell('A1').note = 'Remove this note';
  removedLastBook.removeWorksheet(removedLastSheet.id);
  assert.throws(() => removedLastSheet.getCell('A1'), { code: 'STALE_HANDLE' });
  const removedLastBytes = await removedLastBook.xlsx.writeBuffer();
  const removedLastRead = await new Workbook().xlsx.load(removedLastBytes);
  assert.equal(removedLastRead.creator, 'Still here');
  assert.deepEqual(removedLastRead.worksheets, []);
  const removedLastArchive = new XlsxZipReader(removedLastBytes.length,
    (offset, length) => removedLastBytes.subarray(offset, offset + length));
  try {
    const names = removedLastArchive.entry_names();
    assert.ok(!names.some(name => name.startsWith('xl/worksheets/') || /comments|vmlDrawing/i.test(name)));
  } finally {
    removedLastArchive.free();
  }
  const streamedRow = encode_xlsx_row(1, Uint32Array.of(0, 2), ['a<&', false]);
  const failedRowSource = new XlsxRowStream(() => { throw new Error('Injected XML source failure'); });
  try {
    assert.throws(() => failedRowSource.next_row(), /Injected XML source failure/);
  } finally {
    failedRowSource.free();
  }
  assert.match(streamedRow, /r="A2".*a&lt;&amp;.*r="C2" t="b"><v>0/);
  assert.throws(() => encode_xlsx_row(0, Uint32Array.of(0), []), /one value per column/);
  const archive = new XlsxZipStream();
  const seedBook = new Workbook();
  seedBook.addWorksheet('Stream').getCell('A1').value = 1;
  const seed = new XlsxDocument(await seedBook.xlsx.writeBuffer());
  try {
    const encoder = new TextEncoder();
    const decoder = new TextDecoder();
    const parts = seed.zip_entry_names();
    const worksheetPart = parts.find(part => /xl\/worksheets\/sheet1\.xml$/.test(part));
    const worksheetXml = decoder.decode(seed.zip_entry_bytes(worksheetPart));
    const [before, after] = worksheetXml.split(/<sheetData>.*?<\/sheetData>/s);
    assert.ok(after);
    archive.start_entry(worksheetPart.replace(/^\//, ''), false);
    assert.throws(() => archive.write_stored_chunk(encoder.encode('early')), /Drain ZIP output/);
    const header = archive.drain();
    assert.ok(header.length > 0);
    const rowPart = `${before}<sheetData>${streamedRow}</sheetData>${after}`;
    const first = encoder.encode(rowPart);
    archive.write_stored_chunk(first);
    assert.deepEqual(archive.drain(), new Uint8Array());
    for (const part of parts) {
      if (part === worksheetPart) continue;
      archive.start_entry(part.replace(/^\//, ''), true);
      archive.write_chunk(seed.zip_entry_bytes(part));
    }
    const tail = archive.finish();
    const streamedZip = Buffer.concat([header, first, tail]);
    const reopened = await new Workbook().xlsx.load(streamedZip);
    const seekable = new XlsxZipReader(streamedZip.length, (offset, length) => streamedZip.subarray(offset, offset + length));
    try {
      const name = worksheetPart.replace(/^\//, '');
      assert.ok(seekable.entry_names().includes(name));
      assert.throws(() => seekable.read_metadata('xl/workbook.xml', 1), /byte limit/);
      assert.match(decoder.decode(seekable.read_metadata('xl/workbook.xml', 65536)), /Stream/);
      const rows = seekable.open_rows(name);
      try {
        assert.deepEqual(rows.next_row(), [
          2, 1, 'inlineStr', undefined, 'a<&', undefined, undefined,
          3, 'b', undefined, '0', undefined, undefined,
        ]);
        assert.deepEqual(rows.next_row(), []);
      } finally {
        rows.free();
      }
      streamedZip[streamedZip.indexOf('a&lt;&amp;')] = 'b'.charCodeAt(0);
      const damaged = seekable.open_rows(name);
      try {
        assert.throws(() => { while (damaged.next_row().length) {} }, /checksum/);
      } finally {
        damaged.free();
      }
    } finally {
      seekable.free();
    }
    assert.equal(reopened.getWorksheet('Stream').getCell('A2').value, 'a<&');
    assert.equal(reopened.getWorksheet('Stream').getCell('C2').value, false);
    assert.throws(() => archive.write_chunk(first), /already finalized/);
  } finally {
    seed.free();
    archive.free();
  }
  const writerFile = join(directory, 'stream-writer.xlsx');
  const writer = new stream.xlsx.WorkbookWriter({ filename: writerFile });
  const alpha = writer.addWorksheet('Alpha');
  const beta = writer.addWorksheet('Beta');
  const streamMergeReference = alpha.getCell('A1');
  assert.equal(alpha.getRow(1).getCellEx({ address: 'A1', row: 1, col: 1 }), streamMergeReference);
  assert.throws(() => alpha.getRow(1).getCellEx({ address: 'B1', row: 1, col: 1 }), { code: 'INVALID_ADDRESS' });
  streamMergeReference.addMergeRef();
  assert.equal(streamMergeReference.isMerged, true);
  assert.equal(streamMergeReference.master, streamMergeReference);
  streamMergeReference.releaseMergeRef();
  assert.equal(streamMergeReference.isMerged, false);
  alpha.getRow(1).values = [1, 'a<&', false];
  alpha.getRow(1).commit();
  beta.getCell('A3').value = 'third';
  alpha.getRow(4).values = [true];
  alpha.getRow(4).commit();
  assert.throws(() => alpha.getRow(1), /been committed/);
  alpha.commit();
  assert.equal(writer.getWorksheet('Beta'), beta);
  const written = writer.commit();
  assert.equal(writer.commit(), written);
  await written;
  const streamedWorkbook = await new Workbook().xlsx.readFile(writerFile);
  assert.equal(streamedWorkbook.getWorksheet('Alpha').getCell('B1').value, 'a<&');
  assert.equal(streamedWorkbook.getWorksheet('Alpha').getCell('C1').value, false);
  assert.equal(streamedWorkbook.getWorksheet('Alpha').getCell('A4').value, true);
  assert.equal(streamedWorkbook.getWorksheet('Beta').getCell('A3').value, 'third');

  const namedWriterFile = join(directory, 'stream-named-splice.xlsx');
  const namedWriter = new stream.xlsx.WorkbookWriter({ filename: namedWriterFile });
  const namedWriterSheet = namedWriter.addWorksheet('Data');
  namedWriterSheet.getCell('A3').value = 'unchanged';
  namedWriter.definedNames.add('Data!A2:B4', 'Region');
  namedWriter.definedNames.spliceRows('Data', 2, 1);
  namedWriter.definedNames.spliceColumns('Data', 2, 0, 1);
  await namedWriter.commit();
  const namedStreamed = await new Workbook().xlsx.readFile(namedWriterFile);
  assert.deepEqual(namedStreamed.definedNames.getNames('Data!A2'), ['Region']);
  assert.deepEqual(namedStreamed.definedNames.getNames('Data!B2'), []);
  assert.deepEqual(namedStreamed.definedNames.getNames('Data!C3'), ['Region']);
  assert.equal(namedStreamed.getWorksheet('Data').getCell('A3').value, 'unchanged');

  const columnDefinitionFile = join(directory, 'stream-column-definition.xlsx');
  const columnDefinitionWriter = new stream.xlsx.WorkbookWriter({ filename: columnDefinitionFile, useStyles: true });
  const columnDefinitionSheet = columnDefinitionWriter.addWorksheet('Data');
  const streamingDefinition = columnDefinitionSheet.getColumn(2);
  streamingDefinition.defn = { key: 'amount', header: 'Amount', width: 16, style: { numFmt: '0.00' } };
  assert.equal(columnDefinitionSheet.getColumn('amount'), streamingDefinition);
  assert.equal(streamingDefinition.defn.style.numFmt, '0.00');
  assert.equal(columnDefinitionSheet.getCell('B1').value, 'Amount');
  columnDefinitionSheet.getCell('B2').value = 4;
  await columnDefinitionWriter.commit();
  const columnDefinitionRead = (await new Workbook().xlsx.readFile(columnDefinitionFile)).getWorksheet('Data');
  assert.equal(columnDefinitionRead.getColumn(2).width, 16);
  assert.equal(columnDefinitionRead.getCell('B1').value, 'Amount');
  assert.equal(columnDefinitionRead.getCell('B2').numFmt, '0.00');

  const rowSpliceFile = join(directory, 'stream-row-splice.xlsx');
  const rowSpliceWriter = new stream.xlsx.WorkbookWriter({ filename: rowSpliceFile, useStyles: true });
  const rowSpliceSheet = rowSpliceWriter.addWorksheet('Data');
  const rowSplice = rowSpliceSheet.addRow([1, , 3, 4]);
  rowSplice.getCell(3).numFmt = '0.00';
  rowSplice.getCell(3).note = 'Move me';
  const regularSpliceRow = new Workbook().addWorksheet('Bounds').getRow(1);
  const streamingSpliceRow = rowSpliceWriter.addWorksheet('Bounds').getRow(1);
  for (const row of [regularSpliceRow, streamingSpliceRow]) {
    row.getCell(16384).value = 42;
    for (const count of [-1, 0.5, Number.MAX_SAFE_INTEGER + 1]) {
      assert.throws(() => row.splice(16384, count), { code: 'INVALID_ADDRESS' });
    }
    assert.throws(() => row.splice(16384, 0, 9), { code: 'INVALID_ADDRESS' });
    assert.equal(row.getCell(16384).value, 42);
    row.splice(16384, 1, 9);
    assert.equal(row.getCell(16384).value, 9);
  }
  assert.deepEqual(XlsxDocument.infer_merge_ranges(Uint32Array.of(
    0, 4, 1, 5,
    0, 0, 1, 3,
    0, 0, 3, 1,
  )), ['E1:F2', 'A1:D4']);
  assert.throws(() => XlsxDocument.infer_merge_ranges(Uint32Array.of(0, 0, 0)), /four coordinates/);
  assert.throws(() => XlsxDocument.infer_merge_ranges(Uint32Array.of(0, 0, 1, 16384)));
  const implicitMergeBook = new Workbook();
  const implicitMergeSheet = implicitMergeBook.addWorksheet('Implicit merge');
  implicitMergeSheet.model = { rows: [
    { number: 1, cells: [{ address: 'A1', type: ValueType.Number, value: 7 }] },
    { number: 2, cells: [{
      address: 'D2', type: ValueType.Merge, master: 'A1', style: { numFmt: '0.000' },
      comment: { type: 'note', note: { texts: [{ text: 'Follower note' }] } },
    }] },
    { number: 4, cells: [{ address: 'B4', type: ValueType.Merge, master: 'A1' }] },
  ] };
  const implicitMergeRead = (await new Workbook().xlsx.load(await implicitMergeBook.xlsx.writeBuffer()))
    .getWorksheet('Implicit merge');
  assert.deepEqual(implicitMergeRead.model.merges, ['A1:D4']);
  assert.equal(implicitMergeRead.getCell('D4').master.address, 'A1');
  assert.equal(implicitMergeRead.getCell('D4').value, 7);
  assert.equal(implicitMergeRead.getCell('D2').numFmt, '0.000');
  assert.equal(implicitMergeRead.getCell('D2').note, 'Follower note');
  assert.equal(implicitMergeRead.getCell('A1').note, undefined);
  const explicitMergeSheet = implicitMergeBook.addWorksheet('Explicit merge');
  explicitMergeSheet.model = {
    merges: ['A1:D4'],
    rows: [
      { number: 1, cells: [{ address: 'A1', type: ValueType.Number, value: 7 }] },
      { number: 2, cells: [{
        address: 'D2', type: ValueType.Merge, master: 'A1', style: { numFmt: '0.000' },
        comment: { type: 'note', note: { texts: [{ text: 'Explicit follower note' }] } },
      }] },
    ],
  };
  const explicitMergeRead = (await new Workbook().xlsx.load(await implicitMergeBook.xlsx.writeBuffer()))
    .getWorksheet('Explicit merge');
  assert.equal(explicitMergeRead.getCell('D2').numFmt, '0.000');
  assert.equal(explicitMergeRead.getCell('D2').note, 'Explicit follower note');
  const linkedModelSheet = implicitMergeBook.addWorksheet('Linked merge');
  linkedModelSheet.getCell('A1').value = 7;
  linkedModelSheet.mergeCells('A1:B1');
  linkedModelSheet.model = { merges: ['A1:B1'], rows: [
    { number: 1, cells: [
      { address: 'A1', type: ValueType.Number, value: 7 },
      { address: 'B1', type: ValueType.Merge, master: 'A1', style: { numFmt: '0.000' } },
    ] },
  ] };
  linkedModelSheet.getCell('A1').numFmt = '0.0';
  assert.equal(linkedModelSheet.getCell('B1').numFmt, '0.000');
  linkedModelSheet.model = { merges: ['A1:B1'], rows: [
    { number: 1, cells: [
      { address: 'A1', type: ValueType.Number, value: 7 },
      { address: 'B1', type: ValueType.Merge, master: 'A1',
        comment: { type: 'note', note: { texts: [{ text: 'Note only update' }] } } },
    ] },
  ] };
  assert.equal(linkedModelSheet.getCell('B1').numFmt, '0.000');
  assert.equal(linkedModelSheet.getCell('B1').note, 'Note only update');
  assert.throws(() => {
    linkedModelSheet.model = { merges: ['A1:B1'], rows: [
      { number: 1, cells: [
        { address: 'A1', type: ValueType.Number, value: 99 },
        { address: 'B1', type: ValueType.Merge, master: 'A2', style: { numFmt: '0' } },
      ] },
    ] };
  }, { code: 'INVALID_ADDRESS' });
  assert.equal(linkedModelSheet.getCell('A1').value, 7);
  assert.equal(linkedModelSheet.getCell('B1').numFmt, '0.000');
  linkedModelSheet.getCell('A4').value = 13;
  linkedModelSheet.mergeCells('A4:B5');
  linkedModelSheet.getRow(1).model = { number: 1, cells: [
    { address: 'A1', type: ValueType.Number, value: 8 },
    { address: 'B1', type: ValueType.Merge, master: 'A1', style: { numFmt: '0.00' },
      comment: { type: 'note', note: { texts: [{ text: 'Row model follower note' }] } } },
  ] };
  assert.equal(linkedModelSheet.getCell('B1').master.address, 'A1');
  assert.equal(linkedModelSheet.getCell('B1').value, 8);
  assert.equal(linkedModelSheet.getCell('B1').numFmt, '0.00');
  assert.equal(linkedModelSheet.getCell('B1').note, 'Row model follower note');
  assert.equal(linkedModelSheet.getCell('B5').master.address, 'A4');
  assert.equal(linkedModelSheet.getCell('B5').value, 13);
  const invalidMergeSheet = implicitMergeBook.addWorksheet('Invalid merge');
  assert.throws(() => {
    invalidMergeSheet.model = { rows: [
      { number: 1, cells: [{ address: 'A1', type: ValueType.Number, value: 7 }] },
      { number: 2, cells: [{ address: 'D4', type: ValueType.Merge, master: 'A1' }] },
    ] };
  }, { code: 'INVALID_ADDRESS' });
  assert.deepEqual(invalidMergeSheet.model.merges, []);
  assert.equal(invalidMergeSheet.getCell('A1').value, null);
  assert.throws(() => {
    invalidMergeSheet.model = { rows: [
      { number: 1, cells: [{ address: 'B1', type: ValueType.Number, value: 9 }] },
      { number: 2, cells: [{ address: 'A2', type: ValueType.Number, value: 7 }] },
      { number: 3, cells: [{ address: 'D3', type: ValueType.Merge, master: 'A2' }] },
      { number: 4, cells: [{ address: 'C4', type: ValueType.Merge, master: 'B1' }] },
    ] };
  }, { code: 'MERGE_EDIT_REFUSED' });
  assert.deepEqual(invalidMergeSheet.model.merges, []);
  assert.equal(invalidMergeSheet.getCell('B1').value, null);
  assert.throws(() => {
    invalidMergeSheet.model = { rows: [
      { number: 1, cells: [{ address: 'A1', type: ValueType.Merge, master: 'B2' }] },
      { number: 2, cells: [{ address: 'B2', type: ValueType.Number, value: 7 }] },
    ] };
  }, { code: 'INVALID_ADDRESS' });
  assert.equal(invalidMergeSheet.getCell('B2').value, null);
  const validationSpliceBook = new Workbook();
  const validationSpliceSheet = validationSpliceBook.addWorksheet('Data');
  const validationSpliceRow = validationSpliceSheet.addRow([1, 2, 3]);
  const spliceValidation = { type: 'whole', operator: 'between', formulae: [1, 5] };
  validationSpliceSheet.getCell('C1').dataValidation = spliceValidation;
  validationSpliceSheet.getCell('D1').note = {
    texts: [{ text: 'Splice note', font: { bold: true } }],
    editAs: 'absolute',
  };
  const spliceNoteModel = validationSpliceSheet.getCell('D1').model.comment;
  validationSpliceSheet.getCell('D2').note = 'Different row';
  const spliceFormulas = [
    { formula: '2-2', result: 0 },
    { formula: 'FALSE()', result: false },
    { formula: '""', result: '' },
    { formula: '1+1' },
  ];
  spliceFormulas.forEach((value, index) => { validationSpliceRow.getCell(6 + index).value = value; });
  const beforeInjectedNoteSplice = await validationSpliceBook.xlsx.writeBuffer();
  const originalNoteSplice = XlsxDocument.prototype.splice_row_notes;
  try {
    XlsxDocument.prototype.splice_row_notes = function (...args) {
      originalNoteSplice.apply(this, args);
      throw new Error('simulated failure after moving notes');
    };
    assert.throws(() => validationSpliceRow.splice(2, 0, 9), { code: 'CELL_EDIT_REFUSED' });
  } finally {
    XlsxDocument.prototype.splice_row_notes = originalNoteSplice;
  }
  assert.deepEqual(await validationSpliceBook.xlsx.writeBuffer(), beforeInjectedNoteSplice);
  assert.deepEqual(validationSpliceRow.values, [, 1, 2, 3, , , ...spliceFormulas]);
  assert.deepEqual(validationSpliceSheet.getCell('D1').model.comment, spliceNoteModel);
  assert.equal(validationSpliceSheet.getCell('E1').note, undefined);
  assert.deepEqual(validationSpliceSheet.getCell('C1').dataValidation, spliceValidation);
  validationSpliceRow.splice(2, 0, 9);
  const validationSpliceRead = (await new Workbook().xlsx.load(
    await validationSpliceBook.xlsx.writeBuffer(),
  )).getWorksheet('Data');
  assert.deepEqual(validationSpliceRead.getRow(1).values, [, 1, 9, 2, 3, , , ...spliceFormulas]);
  assert.deepEqual(validationSpliceRead.getCell('C1').dataValidation, spliceValidation);
  assert.equal(validationSpliceRead.getCell('D1').dataValidation, undefined);
  assert.equal(validationSpliceRead.getCell('D1').note, undefined);
  assert.deepEqual(validationSpliceRead.getCell('E1').model.comment, spliceNoteModel);
  assert.equal(validationSpliceRead.getCell('D2').note, 'Different row');
  assert.equal(validationSpliceRead.getCell('E2').note, undefined);
  const formulaModeSheet = new Workbook().addWorksheet('Formula mode');
  assert.equal(formulaModeSheet.fillFormula.length, 3);
  formulaModeSheet.fillFormula('B2:B2', '1', [1]);
  formulaModeSheet.fillFormula({ top: 3, left: 2, bottom: 3, right: 2 }, '1', [1], undefined);
  assert.equal(formulaModeSheet.getCell('B2').value.shareType, 'shared');
  assert.equal(formulaModeSheet.getCell('B3').value.shareType, 'shared');
  assert.throws(() => formulaModeSheet.fillFormula({ top: 3, left: 16385, bottom: 3, right: 16385 }, '1'),
    { code: 'INVALID_ADDRESS' });
  const formulaCoordinates = [];
  formulaModeSheet.fillFormula({ top: 6, left: 3, bottom: 5, right: 2 }, '1', (row, column) => {
    formulaCoordinates.push([row, column]);
    return row * 10 + column;
  });
  assert.deepEqual(formulaCoordinates, [[5, 2], [5, 3], [6, 2], [6, 3]]);
  assert.equal(formulaModeSheet.getCell('B5').value.ref, 'B5:C6');
  assert.deepEqual(formulaModeSheet.getCell('C6').value, { sharedFormula: 'B5', result: 63 });
  formulaModeSheet.fillFormula({ top: 1048576, left: 16384, bottom: 1048576, right: 16384 }, '1', [1]);
  assert.equal(formulaModeSheet.getCell('XFD1048576').value.ref, 'XFD1048576');
  assert.throws(() => formulaModeSheet.fillFormula('B4:B4', '1', [1], null), { code: 'UNSUPPORTED_OPTIONS' });
  for (const shareType of ['shared', 'array']) {
    const source = new Workbook().addWorksheet('Formula source');
    source.fillFormula('B2:B2', '1', [1], shareType);
    const value = source.getCell('B2').value;
    assert.equal(value.ref, 'B2');
    const targetBook = new Workbook();
    const target = targetBook.addWorksheet('Formula target');
    target.getCell('B2').value = value;
    assert.deepEqual(target.getCell('B2').value, value);
    const modelTarget = targetBook.addWorksheet('Model target');
    modelTarget.getCell('B2').model = source.getCell('B2').model;
    const reopened = await new Workbook().xlsx.load(await targetBook.xlsx.writeBuffer());
    assert.deepEqual(reopened.getWorksheet('Formula target').getCell('B2').value, value);
    assert.deepEqual(reopened.getWorksheet('Model target').getCell('B2').value, value);
    const streamTarget = rowSpliceWriter.addWorksheet('Single ' + shareType);
    streamTarget.getCell('B2').model = source.getCell('B2').model;
  }
  const styledSpliceBook = new Workbook();
  const styledSpliceSheet = styledSpliceBook.addWorksheet('Styled splice');
  styledSpliceSheet.getCell('A1').value = 0;
  styledSpliceSheet.getCell('A1').numFmt = '0.000';
  styledSpliceSheet.getCell('B1').value = false;
  styledSpliceSheet.getCell('C1').numFmt = '0.00';
  styledSpliceSheet.getCell('D1').value = 'move';
  styledSpliceSheet.getRow(1).splice(1, 0, 'insert');
  const styledSpliceRead = (await new Workbook().xlsx.load(await styledSpliceBook.xlsx.writeBuffer())).getWorksheet(1);
  assert.equal(styledSpliceRead.getCell('B1').value, 0);
  assert.equal(styledSpliceRead.getCell('B1').numFmt, '0.000');
  assert.equal(styledSpliceRead.getCell('C1').value, false);
  assert.equal(styledSpliceRead.getCell('D1').value, null);
  assert.equal(styledSpliceRead.getCell('D1').numFmt, '0.00');
  assert.equal(styledSpliceRead.getCell('E1').value, 'move');

  const rangeFormulaBook = new Workbook();
  const rangeFormulaSheet = rangeFormulaBook.addWorksheet('Static array');
  rangeFormulaSheet.fillFormula('B1:C1', 'B2+1', [2, 3], 'array');
  rangeFormulaSheet.getCell('C1').numFmt = '0.000';
  const rangeFormulaRow = rangeFormulaSheet.getRow(1);
  const beforePartialArray = await rangeFormulaBook.xlsx.writeBuffer();
  assert.throws(() => rangeFormulaRow.splice(3, 0, 7), { code: 'CELL_EDIT_REFUSED' });
  assert.deepEqual(await rangeFormulaBook.xlsx.writeBuffer(), beforePartialArray);
  rangeFormulaRow.splice(1, 0, 7);
  assert.deepEqual(rangeFormulaSheet.getCell('C1').value,
    { formula: 'B2+1', shareType: 'array', ref: 'C1:D1', result: 2 });
  assert.equal(rangeFormulaSheet.getCell('D1').value, 3);
  const arraySpliceRead = (await new Workbook().xlsx.load(await rangeFormulaBook.xlsx.writeBuffer())).getWorksheet(1);
  assert.equal(arraySpliceRead.getCell('D1').numFmt, '0.000');
  assert.equal(arraySpliceRead.getCell('D1').value, 3);
  arraySpliceRead.getRow(1).splice(3, 2, 'replacement');
  assert.equal(arraySpliceRead.getCell('C1').value, 'replacement');
  assert.equal(arraySpliceRead.getCell('D1').value, null);

  const ordinarySharedSpliceBook = new Workbook();
  const ordinarySharedSpliceSheet = ordinarySharedSpliceBook.addWorksheet('Shared splice');
  ordinarySharedSpliceSheet.fillFormula('B1:C1', 'B2+1', [2, 3], 'shared');
  ordinarySharedSpliceSheet.getCell('C1').numFmt = '0.000';
  const ordinarySharedSpliceRow = ordinarySharedSpliceSheet.getRow(1);
  assert.throws(() => ordinarySharedSpliceRow.splice(3, 0, 7), { code: 'CELL_EDIT_REFUSED' });
  ordinarySharedSpliceRow.splice(1, 0, 7);
  assert.deepEqual(ordinarySharedSpliceSheet.getCell('C1').value,
    { formula: 'B2+1', shareType: 'shared', ref: 'C1:D1', result: 2 });
  assert.deepEqual(ordinarySharedSpliceSheet.getCell('D1').value, { sharedFormula: 'C1', result: 3 });
  const ordinarySharedSpliceRead =
    (await new Workbook().xlsx.load(await ordinarySharedSpliceBook.xlsx.writeBuffer())).getWorksheet(1);
  assert.equal(ordinarySharedSpliceRead.getCell('D1').formula, 'C2+1');
  assert.equal(ordinarySharedSpliceRead.getCell('D1').numFmt, '0.000');

  ordinarySharedSpliceRead.getRow(1).splice(1, 1);
  assert.deepEqual(ordinarySharedSpliceRead.getCell('B1').value,
    { formula: 'B2+1', shareType: 'shared', ref: 'B1:C1', result: 2 });
  assert.deepEqual(ordinarySharedSpliceRead.getCell('C1').value, { sharedFormula: 'B1', result: 3 });

  ordinarySharedSpliceRead.getCell('E1').value = 25;
  const beforePartialShared = await ordinarySharedSpliceRead.workbook.xlsx.writeBuffer();
  assert.throws(() => ordinarySharedSpliceRead.getRow(1).splice(2, 1, 'split'), { code: 'CELL_EDIT_REFUSED' });
  assert.deepEqual(await ordinarySharedSpliceRead.workbook.xlsx.writeBuffer(), beforePartialShared);
  ordinarySharedSpliceRead.getRow(1).splice(2, 2, 'replacement');
  assert.equal(ordinarySharedSpliceRead.getCell('B1').value, 'replacement');
  assert.equal(ordinarySharedSpliceRead.getCell('C1').value, null);
  assert.equal(ordinarySharedSpliceRead.getCell('D1').value, 25);
  const sharedRemovalRead = (await new Workbook().xlsx.load(await ordinarySharedSpliceRead.workbook.xlsx.writeBuffer())).getWorksheet(1);
  assert.equal(sharedRemovalRead.getCell('B1').formula, undefined);
  assert.equal(sharedRemovalRead.getCell('C1').formula, undefined);
  assert.equal(sharedRemovalRead.getCell('D1').value, 25);

  const sparseSharedBase = new Workbook();
  sparseSharedBase.addWorksheet('Sparse shared').getCell('C1').value = 'independent';
  const sparseSharedDocument = new XlsxDocument(await sparseSharedBase.xlsx.writeBuffer());
  let sparseSharedBytes;
  try {
    sparseSharedDocument.write_shared_formula_master('Sparse shared', 'B1:E1', 'B2+1', 0, undefined);
    sparseSharedDocument.write_shared_formula_member('Sparse shared', 0, 3, 'B1', '#N/A', 'e');
    const cachedDate = new Date('2025-06-01T00:00:00Z');
    sparseSharedDocument.write_shared_formula_member(
      'Sparse shared', 0, 4, 'B1', sparseSharedDocument.excel_date_serial(cachedDate.getTime()), undefined);
    sparseSharedDocument.set_number_format('Sparse shared', 0, 4, 'mm-dd-yy');
    sparseSharedBytes = sparseSharedDocument.to_bytes();
  } finally {
    sparseSharedDocument.free();
  }
  const sparseSharedBook = await new Workbook().xlsx.load(sparseSharedBytes);
  sparseSharedBook.getWorksheet(1).getRow(1).splice(1, 0, 'first');
  const sparseSharedRead = (await new Workbook().xlsx.load(await sparseSharedBook.xlsx.writeBuffer())).getWorksheet(1);
  assert.deepEqual(sparseSharedRead.getCell('C1').value, {
    formula: 'B2+1', shareType: 'shared', ref: 'C1:F1', result: 0,
  });
  assert.equal(sparseSharedRead.getCell('D1').value, 'independent');
  assert.deepEqual(sparseSharedRead.getCell('E1').value, { sharedFormula: 'C1', result: { error: '#N/A' } });
  assert.deepEqual(sparseSharedRead.getCell('F1').value, {
    sharedFormula: 'C1', result: new Date('2025-06-01T00:00:00Z'),
  });

  const multiRowSharedBook = new Workbook();
  const multiRowSharedSheet = multiRowSharedBook.addWorksheet('Multi-row shared splice');
  multiRowSharedSheet.fillFormula('B1:C2', 'E1+1', [2, 3, 4, 5], 'shared');
  const multiRowSharedBytes = await multiRowSharedBook.xlsx.writeBuffer();
  assert.throws(() => multiRowSharedSheet.getRow(1).splice(1, 0, 7), { code: 'CELL_EDIT_REFUSED' });
  assert.deepEqual(await multiRowSharedBook.xlsx.writeBuffer(), multiRowSharedBytes);

  const authoredNoteSeed = new XlsxDocument(await validationSpliceBook.xlsx.writeBuffer());
  let authoredNoteBytes;
  try {
    authoredNoteSeed.add_cell_note('Data', 2, 2, '作成者', 'Authored note');
    authoredNoteSeed.add_cell_note('Data', 2, 3, '別の作成者', 'Adjacent note');
    authoredNoteBytes = authoredNoteSeed.to_bytes();
  } finally {
    authoredNoteSeed.free();
  }
  const authoredNoteBook = await new Workbook().xlsx.load(authoredNoteBytes);
  authoredNoteBook.getWorksheet('Data').getRow(3).splice(1, 0, 7);
  const authoredNoteResult = new XlsxDocument(await authoredNoteBook.xlsx.writeBuffer());
  try {
    assert.deepEqual(authoredNoteResult.row_notes('Data', 2), [
      'D3', '作成者', 'Authored note',
      'E3', '別の作成者', 'Adjacent note',
    ]);
  } finally {
    authoredNoteResult.free();
  }
  authoredNoteBook.getWorksheet('Data').getRow(3).splice(4, 1);
  const deletedNoteResult = new XlsxDocument(await authoredNoteBook.xlsx.writeBuffer());
  try {
    assert.deepEqual(deletedNoteResult.row_notes('Data', 2), ['D3', '別の作成者', 'Adjacent note']);
  } finally {
    deletedNoteResult.free();
  }
  const rangeSpliceSheet = rowSpliceWriter.addWorksheet('Formula bounds');
  const sharedSpliceRow = rangeSpliceSheet.addRow([
    1, { formula: 'A1+1', shareType: 'shared', ref: 'B1:C1', result: 2 },
    { sharedFormula: 'B1', result: 3 },
  ]);
  assert.throws(() => sharedSpliceRow.splice(3, 0, 9), { code: 'UNSUPPORTED_OPTIONS' });
  const sharedBeforeSplice = sharedSpliceRow.getCell(2).value;
  sharedBeforeSplice.ref = 'A1:C1';
  assert.throws(() => sharedSpliceRow.splice(2, 0, 9), { code: 'INVALID_ADDRESS' });
  assert.equal(sharedSpliceRow.getCell(2).value, sharedBeforeSplice);
  sharedBeforeSplice.ref = 'B1:C1';
  let formulaGetterCalls = 0;
  Object.defineProperty(sharedBeforeSplice, 'unexpected', {
    configurable: true, enumerable: true,
    get() { formulaGetterCalls++; throw new Error('unexpected getter'); },
  });
  assert.throws(() => sharedSpliceRow.splice(2, 0, 9), { code: 'UNSUPPORTED_OPTIONS' });
  assert.equal(formulaGetterCalls, 0);
  assert.equal(sharedSpliceRow.getCell(2).value, sharedBeforeSplice);
  delete sharedBeforeSplice.unexpected;
  sharedSpliceRow.getCell(4).value = { sharedFormula: 'B1', result: 4 };
  assert.throws(() => sharedSpliceRow.splice(2, 0, 9), { code: 'UNSUPPORTED_OPTIONS' });
  assert.equal(sharedSpliceRow.getCell(4).value.sharedFormula, 'B1');
  sharedSpliceRow.getCell(4).value = null;

  sharedSpliceRow.splice(2, 0, 9);
  assert.equal(sharedBeforeSplice.ref, 'B1:C1');
  const arraySpliceRow = rangeSpliceSheet.addRow([
    1, { formula: 'COLUMN(B2:C2)', shareType: 'array', ref: 'B2:C2', result: 2 }, 3,
  ]);
  arraySpliceRow.splice(2, 0, 9);
  const deletedSpliceRow = rangeSpliceSheet.addRow([
    1, { formula: 'A3+1', shareType: 'shared', ref: 'B3:C3', result: 2 },
    { sharedFormula: 'B3', result: 3 }, 4,
  ]);
  assert.throws(() => deletedSpliceRow.splice(2, 1), { code: 'UNSUPPORTED_OPTIONS' });
  deletedSpliceRow.splice(2, 2);
  const formulaEdge = rowSpliceWriter.addWorksheet('Formula edge').getRow(1);
  const edgeFormula = { formula: '1+1', shareType: 'array', ref: 'XFC1:XFD1', result: 2 };
  formulaEdge.getCell(16383).value = edgeFormula;
  assert.throws(() => formulaEdge.splice(16383, 0, 9), { code: 'INVALID_ADDRESS' });
  assert.equal(formulaEdge.getCell(16383).value, edgeFormula);
  formulaEdge.splice(16382, 1);
  const verticalFormulaSheet = rowSpliceWriter.addWorksheet('Committed formula');
  const verticalMaster = verticalFormulaSheet.addRow([1, 2]);
  verticalMaster.getCell(1).value = {
    formula: 'COLUMN(A1:B2)', shareType: 'array', ref: 'A1:B2', result: 1,
  };
  const verticalMember = verticalFormulaSheet.addRow([1, 2, 3]);
  assert.throws(() => verticalMember.splice(2, 0, 9), { code: 'UNSUPPORTED_OPTIONS' });
  assert.deepEqual(verticalMember.values, [, 1, 2, 3]);
  verticalMaster.commit();
  assert.throws(() => verticalMember.splice(2, 0, 9), { code: 'UNSUPPORTED_OPTIONS' });
  assert.throws(() => verticalMember.splice(1, 1, 9), { code: 'UNSUPPORTED_OPTIONS' });
  assert.deepEqual(verticalMember.values, [, 1, 2, 3]);
  verticalMember.splice(3, 1, 9);
  const originalSetNote = XlsxDocument.prototype.set_cell_note;
  let noteWrites = 0;
  try {
    XlsxDocument.prototype.set_cell_note = function (...args) {
      if (++noteWrites === 2) throw new Error('simulated note relocation failure');
      return originalSetNote.apply(this, args);
    };
    assert.throws(() => rowSplice.splice(2, 1, 8, 9), { code: 'CELL_EDIT_REFUSED' });
  } finally {
    XlsxDocument.prototype.set_cell_note = originalSetNote;
  }
  assert.deepEqual(rowSplice.values, [, 1, , 3, 4]);
  assert.equal(rowSplice.getCell(3).note, 'Move me');
  assert.equal(rowSplice.getCell(4).note, undefined);
  rowSplice.splice(2, 1, 8, 9);
  assert.equal(rowSplice.getCell(4).value, 3);
  assert.equal(rowSplice.getCell(4).numFmt, '0.00');
  assert.equal(rowSplice.getCell(4).note, 'Move me');
  const streamCellModel = rowSplice.getCell(4).model;
  assert.equal(streamCellModel.address, 'D1');
  assert.equal(streamCellModel.type, ValueType.Number);
  assert.equal(streamCellModel.value, 3);
  assert.equal(streamCellModel.style.numFmt, '0.00');
  assert.equal(streamCellModel.comment.note.texts[0].text, 'Move me');
  rowSplice.splice(2, 2);
  const replacedRow = rowSpliceSheet.addRow([1, 2, 3]);
  replacedRow.getCell(3).note = 'Retained note';
  replacedRow.values = [4, 5];
  replacedRow.splice(2, 0, 7);
  assert.deepEqual(replacedRow.values, [, 4, 7, 5]);
  assert.equal(replacedRow.getCell(4).note, 'Retained note');
  rowSplice.commit();
  await rowSpliceWriter.commit();
  const rowSpliceBook = await new Workbook().xlsx.readFile(rowSpliceFile);
  assert.equal(rowSpliceBook.getWorksheet('Bounds').getCell('XFD1').value, 9);
  const sharedSpliceRead = rowSpliceBook.getWorksheet('Formula bounds');
  assert.deepEqual(sharedSpliceRead.getCell('C1').value,
    { formula: 'A1+1', shareType: 'shared', ref: 'C1:D1', result: 2 });
  assert.deepEqual(sharedSpliceRead.getCell('D1').value, { sharedFormula: 'C1', result: 3 });
  assert.deepEqual(sharedSpliceRead.getCell('C2').value,
    { formula: 'COLUMN(B2:C2)', shareType: 'array', ref: 'C2:D2', result: 2 });
  assert.equal(sharedSpliceRead.getCell('D2').value, 3);
  assert.deepEqual(sharedSpliceRead.getRow(3).values, [, 1, 4]);
  const formulaEdgeRead = rowSpliceBook.getWorksheet('Formula edge');
  assert.deepEqual(formulaEdgeRead.getCell('XFB1').value,
    { formula: '1+1', shareType: 'array', ref: 'XFB1:XFC1', result: 2 });
  const committedFormula = rowSpliceBook.getWorksheet('Committed formula');
  assert.equal(committedFormula.getCell('A1').value.ref, 'A1:B2');
  assert.deepEqual(committedFormula.getRow(2).values, [, 1, 2, 9]);
  for (const shareType of ['shared', 'array']) {
    assert.deepEqual(rowSpliceBook.getWorksheet('Single ' + shareType).getCell('B2').value,
      { formula: '1', result: 1, ref: 'B2', shareType });
  }
  const rowSpliceRead = rowSpliceBook.getWorksheet('Data');
  assert.deepEqual(rowSpliceRead.getRow(1).values, [, 1, 3, 4]);
  assert.equal(rowSpliceRead.getCell('B1').numFmt, '0.00');
  assert.equal(rowSpliceRead.getCell('B1').note, 'Move me');
  assert.deepEqual(rowSpliceRead.getRow(2).values, [, 4, 7, 5]);
  assert.equal(rowSpliceRead.getCell('C2').note, undefined);
  assert.equal(rowSpliceRead.getCell('D2').note, 'Retained note');

  const cellModelWriterPath = join(directory, 'stream-cell-model.xlsx');
  const cellModelWriter = new stream.xlsx.WorkbookWriter({ filename: cellModelWriterPath, useStyles: true });
  const streamedModelSheet = cellModelWriter.addWorksheet('Model');
  const streamedModelCell = streamedModelSheet.getCell('A1');
  assert.equal(streamedModelSheet.getRow(2).model, null);
  streamedModelCell.model = { type: ValueType.Formula, formula: '1+1', result: 2, style: { numFmt: '0.00' } };
  assert.throws(() => {
    streamedModelCell.model = { type: ValueType.Number, value: 'invalid' };
  }, { code: 'INVALID_VALUE' });
  assert.equal(streamedModelCell.model.formula, '1+1');
  const targetModelCell = streamedModelSheet.getCell('B1');
  targetModelCell.model = { ...streamCellModel, address: 'B1' };
  assert.equal(targetModelCell.note, 'Move me');
  const streamedRowModel = streamedModelSheet.getRow(1).model;
  assert.deepEqual([streamedRowModel.number, streamedRowModel.min, streamedRowModel.max], [1, 1, 2]);
  assert.deepEqual(streamedRowModel.cells.map(cell => cell.address), ['A1', 'B1']);
  const streamModelTargetRow = streamedModelSheet.getRow(2);
  streamModelTargetRow.model = { ...streamedRowModel, number: 2, height: 18, cells: [
    { ...streamedRowModel.cells[0], address: 'A2' },
    { ...streamedRowModel.cells[1], address: undefined },
  ] };
  assert.equal(streamModelTargetRow.getCell(2).note, 'Move me');
  assert.throws(() => {
    streamModelTargetRow.model = { number: 2, cells: [
      { address: 'A2', type: ValueType.Number, value: 9 },
      { address: 'B2', type: ValueType.Number, value: 'invalid' },
    ] };
  }, { code: 'INVALID_VALUE' });
  assert.throws(() => {
    streamModelTargetRow.model = {
      number: 2, style: { font: { bold: 'invalid' } },
      cells: [{ address: 'A2', type: ValueType.Number, value: 9 }],
    };
  }, { code: 'INVALID_VALUE' });
  assert.throws(() => {
    streamModelTargetRow.model = { number: 2, height: 21, cells: [
      { address: 'A2', type: ValueType.Number, value: 9 },
      { address: 'B2', type: ValueType.Number, value: 10, style: { font: { bold: 'invalid' } } },
    ] };
  }, { code: 'INVALID_VALUE' });
  assert.throws(() => {
    streamModelTargetRow.model = { number: 2, height: 21, cells: [
      { address: 'XFD2', type: ValueType.Number, value: 9 },
      { type: ValueType.Number, value: 10 },
    ] };
  }, { code: 'INVALID_ADDRESS' });
  assert.equal(streamModelTargetRow.cellCount, 2);
  assert.equal(streamModelTargetRow.height, 18);
  assert.deepEqual(streamModelTargetRow.getCell(1).value, { formula: '1+1', result: 2 });
  assert.equal(streamModelTargetRow.getCell(2).note, 'Move me');
  await cellModelWriter.commit();
  const modelFromFile = (await new Workbook().xlsx.readFile(cellModelWriterPath)).getWorksheet('Model');
  assert.deepEqual(modelFromFile.getCell('A1').value, { formula: '1+1', result: 2 });
  assert.equal(modelFromFile.getCell('B1').note, 'Move me');
  assert.equal(modelFromFile.getCell('B2').note, 'Move me');
  assert.equal(modelFromFile.getRow(2).height, 18);

  const writerChunks = [];
  const destination = new Writable({
    highWaterMark: 128,
    write(chunk, _encoding, done) {
      writerChunks.push(chunk);
      setImmediate(done);
    },
  });
  const sinkWriter = new stream.xlsx.WorkbookWriter({ stream: destination, zip: { store: true } });
  sinkWriter.addWorksheet().addRow([42]).commit();
  await sinkWriter.commit();
  assert.equal((await new Workbook().xlsx.load(Buffer.concat(writerChunks))).getWorksheet('sheet1').getCell('A1').value, 42);

  const brokenWriter = new stream.xlsx.WorkbookWriter({
    stream: new Writable({
      write(_chunk, _encoding, done) { done(new Error('Injected stream failure')); },
    }),
  });
  const retryRow = brokenWriter.addWorksheet().addRow([1]);
  retryRow.getCell('B').value = Infinity;
  assert.throws(() => retryRow.commit(), { code: 'INVALID_VALUE' });
  retryRow.getCell('B').value = 2;
  retryRow.commit();
  await assert.rejects(brokenWriter.commit(), /Injected stream failure/);

  const typedFile = join(directory, 'typed-stream-writer.xlsx');
  const typedWriter = new stream.xlsx.WorkbookWriter({ filename: typedFile });
  typedWriter.addWorksheet('Empty');
  const typedSheet = typedWriter.addWorksheet('Typed');
  const timestamp = new Date('2024-02-29T07:08:09.123Z');
  typedSheet.addRow([
    timestamp, { error: '#N/A' }, { formula: '1<0', result: false },
    { formula: '"a<&"', result: 'a<&' }, { formula: 'A1', result: timestamp },
    { formula: 'NA()', result: { error: '#N/A' } }, { formula: '1+2' },
  ]).commit();
  const invalidFormulaRow = typedSheet.addRow([{ formula: 'SUM((' }]);
  assert.throws(() => invalidFormulaRow.commit(), /SUM|formula/i);
  invalidFormulaRow.getCell(1).value = { formula: '1+3', result: 4 };
  invalidFormulaRow.commit();
  await typedWriter.commit();
  assert.equal(SheetJS.read(fs.readFileSync(typedFile)).Sheets.Typed['!ref'], 'A1:G2');
  const typedFd = fs.openSync(typedFile, 'r');
  try {
    const typedSize = fs.fstatSync(typedFd).size;
    const buffer = Buffer.allocUnsafe(65536);
    const seekable = new XlsxZipReader(typedSize, (offset, length) => (
      buffer.subarray(0, fs.readSync(typedFd, buffer, 0, length, offset))
    ));
    try {
      const rows = seekable.open_rows('xl/worksheets/sheet2.xml');
      try {
        const first = rows.next_row();
        assert.equal(first[0], 1);
        assert.deepEqual(first.slice(13, 19), [3, 'b', undefined, '0', '1<0', undefined]);
        assert.equal(rows.next_row()[0], 2);
        assert.deepEqual(rows.next_row(), []);
      } finally {
        rows.free();
      }
    } finally {
      seekable.free();
    }
  } finally {
    fs.closeSync(typedFd);
  }
  const typedRoundTrip = await new Workbook().xlsx.readFile(typedFile);
  assert.equal(typedRoundTrip.getWorksheet('Empty').getCell('A1').value, null);
  const typed = typedRoundTrip.getWorksheet('Typed');
  assert.deepEqual(typed.getCell('A1').value, timestamp);
  assert.deepEqual(typed.getCell('B1').value, { error: '#N/A' });
  assert.deepEqual(typed.getCell('C1').value, { formula: '1<0', result: false });
  assert.deepEqual(typed.getCell('D1').value, { formula: '"a<&"', result: 'a<&' });
  assert.deepEqual(typed.getCell('E1').value, { formula: 'A1', result: timestamp });
  assert.deepEqual(typed.getCell('F1').value, { formula: 'NA()', result: { error: '#N/A' } });
  assert.equal(typed.getCell('G1').value.formula, '1+2');
  assert.deepEqual(typed.getCell('A2').value, { formula: '1+3', result: 4 });

  const streamingRead = new stream.xlsx.WorkbookReader(typedFile, { styles: 'cache' });
  const names = [];
  for await (const sheet of streamingRead) {
    names.push(sheet.name);
    for await (const row of sheet) {
      if (row.number === 2) {
        assert.deepEqual(row.getCell(1).value, { formula: '1+3', result: 4 });
        assert.equal(row.getCell(1).type, ValueType.Formula);
        assert.equal(row.getCell(1).effectiveType, ValueType.Number);
        assert.equal(row.getCell(1).formulaType, FormulaType.Master);
        assert.equal(row.getCell(1).text, '4');
        assert.equal(row.getCell(1).toCsvString(), '4');
        continue;
      }
      assert.equal(sheet.name, 'Typed');
      assert.deepEqual(row.getCell(1).value, timestamp);
      assert.deepEqual(row.getCell('B').value, { error: '#N/A' });
      assert.deepEqual(row.getCell('C').value, { formula: '1<0', result: false });
    }
  }
  assert.deepEqual(names, ['Empty', 'Typed']);

  const sharedBook = new Workbook();
  sharedBook.addWorksheet('Shared').getCell('B2').value = 'shared < string';
  const sharedBytes = await sharedBook.xlsx.writeBuffer({ useSharedStrings: true });
  const displayReader = new stream.xlsx.WorkbookReader(Readable.from([sharedBytes]));
  for await (const sheet of displayReader) {
    for await (const row of sheet) {
      const cell = row.getCell('B');
      assert.equal(cell.toString(), 'shared < string');
      assert.equal(cell.html, 'shared &lt; string');
      assert.equal(cell.toCsvString(), '"shared < string"');
      cell.value = "O'Reilly";
      assert.equal(cell.html, 'O&apos;Reilly');
    }
  }
  const tracedReader = new stream.xlsx.WorkbookReader(Readable.from([sharedBytes]), {
    entries: 'emit', sharedStrings: 'emit',
  });
  const readEvents = [];
  tracedReader.on('entry', entry => {
    if (entry.type === 'workbook') {
      assert.equal(tracedReader.model, undefined);
      assert.equal(tracedReader.properties, undefined);
    }
    readEvents.push(entry.type);
  });
  tracedReader.on('shared-strings', entry => readEvents.push(entry));
  tracedReader.on('worksheet', sheet => {
    assert.equal(sheet.workbook, tracedReader);
    assert.deepEqual(tracedReader.properties.model, { date1904: false });
    assert.ok(tracedReader.model.sheets.some(({ id, name }) => id === sheet.id && name === sheet.name));
  });
  tracedReader.on('worksheet', sheet => sheet.on('row', row => {
    const cell = row.getCell('B');
    assert.equal(cell.type, ValueType.String);
    assert.equal(cell.effectiveType, ValueType.String);
    assert.equal(cell.text, '{"sharedString":0}');
    assert.equal(cell.html, '{&quot;sharedString&quot;:0}');
    assert.equal(cell.toCsvString(), cell.text);
    assert.deepEqual(cell.model, {
      address: 'B2', type: ValueType.String, value: '{"sharedString":0}', rawValue: { sharedString: 0 }, style: {},
    });
    readEvents.push(cell.value);
  }));
  tracedReader.on('end', () => readEvents.push('end'));
  tracedReader.on('finished', () => readEvents.push('finished'));
  await tracedReader.read();
  assert.deepEqual(readEvents, [
    'workbook', 'shared-strings', { index: 0, text: 'shared < string' }, 'styles',
    'worksheet', { sharedString: 0 }, 'end', 'finished',
  ]);
  const brokenReader = new stream.xlsx.WorkbookReader(Readable.from([Buffer.from('not a ZIP')]));
  await assert.rejects(brokenReader.read());

  const readerErrors = [];
  const observedBrokenReader = new stream.xlsx.WorkbookReader(Readable.from([Buffer.from('not a ZIP')]));
  observedBrokenReader.on('error', error => readerErrors.push(error));
  observedBrokenReader.on('end', () => assert.fail('Invalid ZIP emitted end'));
  observedBrokenReader.on('finished', () => assert.fail('Invalid ZIP emitted finished'));
  await observedBrokenReader.read();
  assert.equal(readerErrors.length, 1);
  assert.ok(readerErrors[0] instanceof Error);

  const brokenIterator = new stream.xlsx.WorkbookReader(Readable.from([Buffer.from('not a ZIP')]));
  brokenIterator.on('error', () => assert.fail('Async iteration emitted error'));
  await assert.rejects(async () => {
    for await (const sheet of brokenIterator) assert.fail(`Invalid ZIP yielded ${sheet.name}`);
  });

  const originalMkdtemp = fs.default.mkdtempSync;
  let readerDirectory;
  fs.default.mkdtempSync = (...args) => {
    const path = originalMkdtemp(...args);
    if (args[0] === join(tmpdir(), 'ooxml-xlsx-reader-')) readerDirectory = path;
    return path;
  };
  syncBuiltinESMExports();
  try {
    const earlyReader = new stream.xlsx.WorkbookReader(Readable.from([sharedBytes]));
    let seenSheet = false;
    for await (const sheet of earlyReader) {
      seenSheet = true;
      assert.ok(fs.existsSync(join(readerDirectory, 'source.xlsx')));
      for await (const row of sheet) {
        assert.equal(row.getCell('B').value, 'shared < string');
        break;
      }
      break;
    }
    assert.ok(seenSheet);
    assert.equal(fs.existsSync(readerDirectory), false);

    const originalArchiveFree = XlsxZipReader.prototype.free;
    const cleanupError = new Error('Archive disposal failed');
    XlsxZipReader.prototype.free = function () {
      originalArchiveFree.call(this);
      throw cleanupError;
    };
    try {
      await assert.rejects(earlyReader.read(Readable.from([sharedBytes])), error => error === cleanupError);
      assert.equal(fs.existsSync(readerDirectory), false);
    } finally {
      XlsxZipReader.prototype.free = originalArchiveFree;
    }
  } finally {
    fs.default.mkdtempSync = originalMkdtemp;
    syncBuiltinESMExports();
  }

  const reusedReader = new stream.xlsx.WorkbookReader(Readable.from([sharedBytes]));
  await reusedReader.read();
  assert.ok(reusedReader.model.sheets.length);
  assert.equal(reusedReader.properties.model.date1904, false);
  assert.deepEqual(reusedReader.sharedStrings, ['shared < string']);
  const previousOptions = reusedReader.options;
  const previousModel = reusedReader.model;
  let suppliedChunks = 0;
  const retryInput = Readable.from((function* () {
    suppliedChunks++;
    yield sharedBytes;
  })());
  await assert.rejects(
    reusedReader.read(retryInput, { worksheets: 'cache' }),
    { code: 'INVALID_VALUE' },
  );
  assert.equal(reusedReader.options, previousOptions);
  assert.equal(reusedReader.model, previousModel);
  assert.equal(suppliedChunks, 0);
  await reusedReader.read(retryInput, { worksheets: 'emit' });
  assert.equal(suppliedChunks, 1);
  assert.deepEqual(reusedReader.sharedStrings, ['shared < string']);
  await assert.rejects(reusedReader.read(Readable.from([Buffer.from('not a ZIP')])));
  assert.equal(reusedReader.model, undefined);
  assert.equal(reusedReader.properties, undefined);
  assert.equal(reusedReader.sharedStrings, undefined);

  const replacement = new Workbook();
  replacement.properties.date1904 = true;
  replacement.addWorksheet('Replacement');
  const replacementBytes = await replacement.xlsx.writeBuffer();
  await reusedReader.read(Readable.from([replacementBytes]), { sharedStrings: 'ignore' });
  assert.deepEqual(reusedReader.model.sheets.map(({ name }) => name), ['Replacement']);
  assert.equal(reusedReader.properties.model.date1904, true);
  assert.equal(reusedReader.sharedStrings, undefined);

  const hyperlinkBook = new Workbook();
  const hyperlinkSheet = hyperlinkBook.addWorksheet('Links');
  hyperlinkBook.addWorksheet('Target');
  hyperlinkSheet.getCell('A1').value = {
    text: 'external', hyperlink: 'https://example.com/', tooltip: 'External target',
  };
  hyperlinkSheet.getCell('B1').value = { text: 'internal', hyperlink: '#Target!A1' };
  const hyperlinkBytes = await hyperlinkBook.xlsx.writeBuffer();
  const linkReader = new stream.xlsx.WorkbookReader(Readable.from([hyperlinkBytes]), { hyperlinks: 'cache' });
  for await (const sheet of linkReader) {
    for await (const row of sheet) {
      assert.deepEqual(row.getCell('A').value, {
        text: 'external', hyperlink: 'https://example.com/', tooltip: 'External target',
      });
      assert.deepEqual(row.getCell('B').value, { text: 'internal', hyperlink: '#Target!A1' });
    }
  }
  const linkEvents = [];
  const linkEmitter = new stream.xlsx.WorkbookReader(Readable.from([hyperlinkBytes]), { hyperlinks: 'emit' });
  linkEmitter.on('hyperlinks', links => {
    links.on('hyperlink', record => linkEvents.push(record));
    void links.read();
  });
  linkEmitter.on('worksheet', sheet => sheet.on('hyperlink', ref => linkEvents.push(ref)));
  await linkEmitter.read();
  assert.equal(linkEvents[0].target, 'https://example.com/');
  assert.equal(linkEvents[0].targetMode, 'External');
  assert.deepEqual(linkEvents.slice(1), [
    { ref: 'A1', rId: linkEvents[0].rId }, { ref: 'B1', rId: undefined },
  ]);

  const columnBook = new Workbook();
  const columnSheet = columnBook.addWorksheet('Columns');
  columnSheet.columns = [
    {}, { width: 21, hidden: true, outlineLevel: 2, style: { numFmt: '0.00' } },
  ];
  columnSheet.getCell('B4').value = 3;
  const layoutRow = columnSheet.getRow(4);
  layoutRow.height = 25.5;
  layoutRow.hidden = true;
  layoutRow.outlineLevel = 2;
  layoutRow.numFmt = '0.0';
  columnSheet.getColumn(2).font = { name: 'Calibri', size: 11, strike: true };
  layoutRow.font = { name: 'Calibri', size: 12, italic: true };
  const testFill = { type: 'pattern', pattern: 'solid', fgColor: { argb: 'FF3300FF' } };
  const testBorder = { bottom: { style: 'thin', color: { argb: 'FF335533' } } };
  columnSheet.getColumn(2).fill = testFill;
  layoutRow.alignment = { horizontal: 'center', wrapText: true };
  columnSheet.getCell('B4').border = testBorder;
  columnSheet.getCell('B4').protection = { locked: false, hidden: true };
  columnSheet.getCell('B4').font = { name: 'Calibri', size: 13, bold: true, color: { argb: 'FF123456' } };
  columnSheet.getCell('E7').value = 9;
  const columnBytes = await columnBook.xlsx.writeBuffer();
  const columnReader = new stream.xlsx.WorkbookReader(Readable.from([columnBytes]), { styles: 'cache' });
  let streamedColumns;
  for await (const sheet of columnReader) {
    streamedColumns = sheet;
    assert.equal(sheet.columns, null);
    for await (const row of sheet) {
      if (row.number === 4) {
        assert.deepEqual([row.height, row.hidden, row.outlineLevel, row.numFmt], [25.5, true, 2, '0.0']);
        assert.equal(row.style.numFmt, '0.0');
        assert.equal(row.font.italic, true);
        assert.equal(row.style.font.size, 12);
        assert.deepEqual(row.alignment, { horizontal: 'center', wrapText: true });
        assert.deepEqual(row.getCell('B').border, testBorder);
        assert.deepEqual(row.getCell('B').protection, { locked: false, hidden: true });
        assert.equal(row.getCell('B').font.bold, true);
        assert.deepEqual(row.getCell('B').style.font.color, { argb: 'FF123456' });
        const column = sheet.getColumn('B');
        assert.deepEqual([column.number, column.letter, column.width, column.hidden, column.outlineLevel, column.numFmt],
          [2, 'B', 21, true, 2, '0.00']);
        assert.equal(sheet.columns[1], column);
        assert.equal(column.font.strike, true);
        assert.equal(column.style.font.size, 11);
        assert.deepEqual(column.fill, { ...testFill, bgColor: { indexed: 64 } });
        column.key = 'amount';
        assert.equal(row.getCell('amount').value, 3);
        assert.deepEqual(sheet.dimensions.model, { top: 4, left: 2, bottom: 4, right: 2 });
      }
    }
  }
  assert.equal(streamedColumns.getColumnKey('amount').letter, 'B');
  assert.equal(streamedColumns.dimensions.range, 'B4:E7');
  const keys = [];
  streamedColumns.eachColumnKey((column, key) => keys.push([key, column.number]));
  assert.deepEqual(keys, [['amount', 2]]);
  streamedColumns.getColumn('amount').key = 'total';
  assert.equal(streamedColumns.getColumnKey('amount'), undefined);
  assert.equal(streamedColumns.getColumn('total').number, 2);

  const rekeyedWriter = new stream.xlsx.WorkbookWriter();
  const rekeyedStreamSheet = rekeyedWriter.addWorksheet('Rekeyed');
  const rekeyedDocumentSheet = new Workbook().addWorksheet('Rekeyed');
  for (const sheet of [rekeyedStreamSheet, rekeyedDocumentSheet]) {
    sheet.columns = [{ key: 'old' }, { key: 'keep' }];
    const original = sheet.getColumn('old');
    sheet.setColumnKey('staleAlias', original);
    assert.throws(() => { sheet.columns = [null]; });
    assert.equal(sheet.getColumnKey('staleAlias'), original);
    sheet.columns = [{ key: 'keep' }];
    assert.equal(sheet.getColumnKey('old'), undefined);
    assert.equal(sheet.getColumnKey('staleAlias'), undefined);
    assert.equal(sheet.getColumnKey('keep'), sheet.getColumn(1));
    const row = sheet.addRow({ keep: 7, old: 80, staleAlias: 90 });
    assert.equal(row.getCell(1).value, 7);
    assert.equal(row.getCell(2).value, null);
  }
  await rekeyedWriter.commit();

  const keyedFile = join(directory, 'keyed-stream-writer.xlsx');
  const keyedWriter = new stream.xlsx.WorkbookWriter({ filename: keyedFile });
  const keyedSheet = keyedWriter.addWorksheet('Keyed');
  keyedSheet.columns = [
    { key: 'name', header: ['Details', 'Name'], width: 23 },
    { key: 'total', header: 'Total', hidden: true, outlineLevel: 2 },
  ];
  const keyedRow = keyedSheet.addRow({ ignored: 'not a column', total: false, name: 'First' });
  assert.equal(keyedRow.number, 3);
  assert.equal(keyedRow.getCell('name').address, 'A3');
  assert.equal(keyedRow.getCell('name').value, 'First');
  keyedRow.getCell('name').value = 'First revised';
  keyedRow.commit();
  assert.throws(() => {
    keyedSheet.columns = [{ key: 'changed', header: 'Too late' }];
  }, /header row has been committed/);
  keyedSheet.addRow({ name: 'Second', total: 0 }).commit();
  const clearedSheet = keyedWriter.addWorksheet('Cleared');
  clearedSheet.columns = [{ header: 'Clear this' }];
  clearedSheet.getRow(1).values = [];
  clearedSheet.getRow(1).commit();
  await keyedWriter.commit();
  const keyedRoundTrip = await new Workbook().xlsx.readFile(keyedFile);
  assert.equal(keyedRoundTrip.getWorksheet('Cleared').getCell('A1').value, null);
  const keyed = keyedRoundTrip.getWorksheet('Keyed');
  assert.equal(keyed.getCell('A1').value, 'Details');
  assert.equal(keyed.getCell('A2').value, 'Name');
  assert.equal(keyed.getCell('B1').value, 'Total');
  assert.equal(keyed.getCell('A3').value, 'First revised');
  assert.equal(keyed.getCell('B3').value, false);
  assert.equal(keyed.getCell('A4').value, 'Second');
  assert.equal(keyed.getCell('B4').value, 0);
  assert.equal(keyed.getCell('C3').value, null);
  assert.equal(keyed.getColumn(1).width, 23);
  assert.equal(keyed.getColumn(2).hidden, true);
  assert.equal(keyed.getColumn(2).outlineLevel, 2);

  const layoutFile = join(directory, 'layout-stream-writer.xlsx');
  const layoutWriter = new stream.xlsx.WorkbookWriter({ filename: layoutFile });
  const layoutSheet = layoutWriter.addWorksheet('Layout', { properties: { outlineLevelRow: 2 } });
  const formattedRow = layoutSheet.addRow([{ formula: '1+2', result: 3 } ]);
  formattedRow.height = 24.5;
  formattedRow.outlineLevel = 1;
  assert.equal(formattedRow.collapsed, false);
  formattedRow.commit();
  const emptyLayoutRow = layoutSheet.getRow(8);
  assert.throws(() => { emptyLayoutRow.height = 410; }, /row height/);
  assert.throws(() => { emptyLayoutRow.outlineLevel = 8; }, /outline level/);
  emptyLayoutRow.height = 18;
  emptyLayoutRow.hidden = true;
  emptyLayoutRow.outlineLevel = 2;
  assert.equal(emptyLayoutRow.collapsed, true);
  emptyLayoutRow.commit();
  await layoutWriter.commit();
  const layoutRoundTrip = await new Workbook().xlsx.readFile(layoutFile);
  const reopenedLayout = layoutRoundTrip.getWorksheet('Layout');
  assert.deepEqual(reopenedLayout.getCell('A1').value, { formula: '1+2', result: 3 });
  assert.equal(reopenedLayout.getRow(1).height, 24.5);
  assert.equal(reopenedLayout.getRow(8).height, 18);
  assert.equal(reopenedLayout.getRow(8).hidden, true);
  assert.equal(reopenedLayout.getRow(8).outlineLevel, 2);
  // `dimension ref="A1"` is not a bound in CE 0.20.3; the empty row 8 element still counts.
  assert.equal(SheetJS.read(fs.readFileSync(layoutFile)).Sheets.Layout['!ref'], 'A1:A8');
  const layoutReader = new stream.xlsx.WorkbookReader(layoutFile);
  for await (const sheet of layoutReader) {
    const rowLayouts = [];
    for await (const row of sheet) rowLayouts.push([row.number, row.height, row.hidden, row.outlineLevel, row.collapsed]);
    assert.deepEqual(rowLayouts, [[1, 24.5, false, 1, false], [8, 18, true, 2, true]]);
  }

  const mergeFile = join(directory, 'merged-stream-writer.xlsx');
  const mergeWriter = new stream.xlsx.WorkbookWriter({ filename: mergeFile });
  const mergeSheet = mergeWriter.addWorksheet('Merged');
  const anchor = mergeSheet.getCell('B2');
  const oldMember = mergeSheet.getCell(3, 3);
  anchor.value = 'master';
  oldMember.value = 'discarded';
  mergeSheet.mergeCells('C3', 'B2');
  assert.equal(oldMember.master, anchor);
  assert.equal(oldMember.value, 'master');
  assert.equal(oldMember.isMerged, true);
  assert.throws(() => mergeSheet.mergeCells('C2:D3'));
  mergeSheet.getRow(2).values = [, , , 'assigned through member'];
  assert.equal(oldMember.value, 'assigned through member');
  mergeSheet.getRow(2).values = [, , 'reset'];
  assert.equal(oldMember.value, 'reset');
  oldMember.value = 'through member';
  mergeSheet.getRow(2).commit();
  assert.throws(() => { oldMember.value = 'too late'; }, /been committed/);
  assert.throws(() => mergeSheet.mergeCells('E2:F2'), /been committed/);
  assert.equal(mergeSheet.addRow(['after']).number, 4);
  await mergeWriter.commit();
  const mergeDocument = new XlsxDocument(fs.readFileSync(mergeFile));
  try {
    const sheetXml = Buffer.from(mergeDocument.zip_entry_bytes('xl/worksheets/sheet1.xml')).toString();
    assert.match(sheetXml, /<mergeCell ref="B2:C3"/);
    assert.doesNotMatch(sheetXml, /<c r="C3"/);
  } finally {
    mergeDocument.free();
  }
  const mergedRoundTrip = await new Workbook().xlsx.readFile(mergeFile);
  const persistedMerge = mergedRoundTrip.getWorksheet('Merged');
  assert.equal(persistedMerge.getCell('B2').value, 'through member');
  assert.equal(persistedMerge.getCell('C3').master.address, 'B2');
  assert.equal(persistedMerge.getCell('C3').value, 'through member');
  assert.equal(persistedMerge.getCell('A4').value, 'after');

  const sstFile = join(directory, 'shared-stream-writer.xlsx');
  const sstWriter = new stream.xlsx.WorkbookWriter({ filename: sstFile, useSharedStrings: true });
  const sharedOne = sstWriter.addWorksheet('First');
  assert.throws(() => sstWriter.addWorksheet('Invalid', { useSharedStrings: 'true' }), /boolean useSharedStrings/);
  let readStreamingSharedStrings = false;
  assert.throws(() => sstWriter.addWorksheet('Accessor', {
    get useSharedStrings() {
      readStreamingSharedStrings = true;
      return false;
    },
  }), { code: 'UNSUPPORTED_OPTIONS' });
  assert.equal(readStreamingSharedStrings, false);
  const sharedTwo = sstWriter.addWorksheet('Second', { useSharedStrings: undefined });
  assert.equal(sharedTwo.useSharedStrings, true);
  const inlineOnly = sstWriter.addWorksheet('Inline', { useSharedStrings: false });
  const repeated = ' repeated <&> ';
  sharedOne.addRow([repeated, repeated, { formula: '"cached"', result: 'cached' }, timestamp]).commit();
  sharedTwo.addRow([repeated, 'second']).commit();
  inlineOnly.addRow([repeated]).commit();
  inlineOnly.useSharedStrings = true;
  inlineOnly.addRow([repeated]).commit();
  await sstWriter.commit();
  const sstBytes = fs.readFileSync(sstFile);
  const sstZip = new XlsxZipReader(sstBytes.length, (offset, length) => sstBytes.subarray(offset, offset + length));
  try {
    assert.deepEqual(sstZip.shared_text('xl/sharedStrings.xml'), [repeated, 'second']);
    const first = Buffer.from(sstZip.read_metadata('xl/worksheets/sheet1.xml', 65536)).toString();
    assert.match(first, /r="A1" t="s"><v>0<\/v>/);
    assert.match(first, /r="B1" t="s"><v>0<\/v>/);
    assert.match(first, /r="C1" t="str"/);
    const mixed = Buffer.from(sstZip.read_metadata('xl/worksheets/sheet3.xml', 65536)).toString();
    assert.match(mixed, /r="A1" t="inlineStr"/);
    assert.match(mixed, /r="A2" t="s"/);
  } finally {
    sstZip.free();
  }
  const sstRoundTrip = await new Workbook().xlsx.load(sstBytes);
  assert.equal(sstRoundTrip.getWorksheet('First').getCell('A1').value, repeated);
  assert.deepEqual(sstRoundTrip.getWorksheet('First').getCell('D1').value, timestamp);
  assert.equal(sstRoundTrip.getWorksheet('Second').getCell('B1').value, 'second');
  assert.equal(sstRoundTrip.getWorksheet('Inline').getCell('A2').value, repeated);

  const richBook = new Workbook();
  const authoredRich = {
    richText: [
      { text: 'Styled', font: { name: 'Calibri', size: 12, bold: true, italic: true, color: { argb: 'FF123456' } } },
      { text: ' plain' },
    ],
  };
  richBook.addWorksheet('Rich').getCell('A1').value = authoredRich;
  for (const useSharedStrings of [true, false]) {
    const richBytes = await richBook.xlsx.writeBuffer({ useSharedStrings });
    const richZip = new XlsxZipReader(richBytes.length, (offset, length) => richBytes.subarray(offset, offset + length));
    try {
      const xml = Buffer.from(richZip.read_metadata('xl/worksheets/sheet1.xml', 65536)).toString();
      assert.match(xml, useSharedStrings ? /t="s"/ : /t="inlineStr"/);
    } finally {
      richZip.free();
    }
    const richReader = new stream.xlsx.WorkbookReader(Readable.from([richBytes]));
    let richRows = 0;
    for await (const sheet of richReader) {
      for await (const row of sheet) {
        richRows++;
        assert.deepEqual(row.getCell(1).value, {
          richText: [
            { text: 'Styled', font: { name: 'Calibri', size: 12, bold: true, italic: true, color: { argb: 'FF123456' } } },
            { text: ' plain' },
          ],
        });
      }
    }
    assert.equal(richRows, 1);
  }

  const richEmitBook = new Workbook();
  richEmitBook.addWorksheet('Rich emit').getRow(1).values = ['plain', {
    richText: [{ text: 'bold', font: { bold: true } }, { text: ' unstyled' }],
  }];
  const richEmitBytes = await richEmitBook.xlsx.writeBuffer({ useSharedStrings: true });
  const richEmitReader = new stream.xlsx.WorkbookReader(Readable.from([richEmitBytes]), { sharedStrings: 'emit' });
  const richEmitEntries = [];
  for await (const entry of richEmitReader.parse()) {
    if (entry.eventType === 'shared-strings') richEmitEntries.push(entry.value);
    if (entry.eventType === 'worksheet') {
      for await (const row of entry.value) {
        assert.deepEqual(row.values, [, { sharedString: 0 }, { sharedString: 1 }]);
      }
    }
  }
  assert.equal(richEmitReader.sharedStrings, undefined);
  assert.deepEqual(richEmitEntries, [
    { index: 0, text: 'plain' },
    { index: 1, text: { richText: [{ text: 'bold', font: { name: 'Calibri', size: 11, bold: true } }, { text: ' unstyled', font: null }] } },
  ]);

  const originalSharedText = XlsxZipReader.prototype.shared_text;
  XlsxZipReader.prototype.shared_text = function (...args) {
    const strings = originalSharedText.apply(this, args);
    Object.defineProperty(strings, 1, {
      get() { assert.fail('Read the unconsumed shared-string entry'); },
    });
    return strings;
  };
  try {
    const earlyEmit = new stream.xlsx.WorkbookReader(Readable.from([richEmitBytes]), { sharedStrings: 'emit' });
    const events = earlyEmit.parse();
    try {
      assert.deepEqual(await events.next(), {
        done: false, value: { eventType: 'shared-strings', value: { index: 0, text: 'plain' } },
      });
    } finally {
      await events.return();
    }
  } finally {
    XlsxZipReader.prototype.shared_text = originalSharedText;
  }

  const sharedPart = '/xl/sharedStrings.xml';
  for (const [stripFields, expectedFont] of [
    [['rFont', 'sz'], { bold: true }],
    [['rFont'], { size: 11, bold: true }],
    [['sz'], { name: 'Calibri', bold: true }],
    [['rFont', 'sz', 'b'], undefined],
  ]) {
    const archive = SheetJS.CFB.read(new Uint8Array(richEmitBytes), { type: 'array' });
    const sharedXml = Buffer.from(SheetJS.CFB.find(archive, sharedPart).content).toString();
    const editedXml = stripFields.reduce((xml, field) => xml.replace(new RegExp('<' + field + '\\b[^>]*\\/>'), ''), sharedXml);
    assert.notEqual(editedXml, sharedXml);
    assert.ok(SheetJS.CFB.utils.cfb_del(archive, sharedPart));
    SheetJS.CFB.utils.cfb_add(archive, sharedPart.slice(1), Buffer.from(editedXml));
    const partialFontBytes = Uint8Array.from(SheetJS.CFB.write(archive, { type: 'array', fileType: 'zip' }));
    const partialReader = new stream.xlsx.WorkbookReader(Readable.from([partialFontBytes]), { sharedStrings: 'emit' });
    const partialStrings = [];
    for await (const entry of partialReader.parse()) {
      if (entry.eventType === 'shared-strings') partialStrings.push(entry.value.text);
    }
    const formatted = { text: 'bold', font: expectedFont ?? null };
    assert.deepEqual(partialStrings, [
      'plain', { richText: [formatted, { text: ' unstyled', font: null }] },
    ]);

    const partialCache = new stream.xlsx.WorkbookReader(Readable.from([partialFontBytes]));
    const cached = { richText: [expectedFont ? formatted : { text: 'bold' }, { text: ' unstyled' }] };
    for await (const worksheet of partialCache) {
      for await (const row of worksheet) assert.deepEqual(row.getCell(2).value, cached);
    }
    assert.deepEqual(partialCache.sharedStrings[1], cached);
  }

  // Cell-format fonts as ExcelJS 4.4.0 writes them may omit name and size.
  // Expected values are those 4.4.0 returns for the same package through
  // Workbook.xlsx.load and WorkbookReader({ styles: 'cache' }).
  {
    const sml = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
    const fontStyles = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="${sml}">`
      + '<numFmts count="1"><numFmt numFmtId="164" formatCode="0.0"/></numFmts><fonts count="5">'
      + '<font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font>'
      + '<font><b/><color rgb="FFFF0000"/></font><font><i/><sz val="14"/></font><font><u/><name val="Arial"/></font><font/>'
      + '<font><b val="0"/><outline/><shadow val="0"/><condense/><extend val="1"/></font></fonts>'
      + '<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>'
      + '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>'
      + '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="7">'
      + '<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>'
      + '<xf numFmtId="10" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/>'
      + '<xf numFmtId="0" fontId="2" fillId="0" borderId="0" xfId="0" applyFont="1"/>'
      + '<xf numFmtId="164" fontId="3" fillId="0" borderId="0" xfId="0" applyFont="1"/>'
      + '<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>'
      + '<xf numFmtId="0" fontId="4" fillId="0" borderId="0" xfId="0" applyFont="1"/>'
      + '<xf numFmtId="0" fontId="5" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs>'
      + '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>';
    const fontSheet = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="${sml}"><dimension ref="A1:H2"/>`
      + '<cols><col min="3" max="3" width="9" style="3" customWidth="1"/></cols><sheetData><row r="1">'
      + '<c r="A1" s="1"><v>1</v></c><c r="B1" s="2"><v>2</v></c><c r="C1" s="3"><v>3</v></c><c r="D1" s="4"><v>4</v></c>'
      + '<c r="E1" s="0"><v>5</v></c><c r="F1"><v>6</v></c><c r="G1" s="5"><v>7</v></c><c r="H1" s="6"><v>9</v></c></row>'
      + '<row r="2" s="2" customFormat="1"><c r="A2" s="2"><v>8</v></c></row></sheetData></worksheet>';
    const fontBase = new Workbook();
    fontBase.addWorksheet('Fonts').getCell('A1').value = 1;
    const fontArchive = SheetJS.CFB.read(new Uint8Array(await fontBase.xlsx.writeBuffer()), { type: 'array' });
    for (const [part, xml] of [['/xl/styles.xml', fontStyles], ['/xl/worksheets/sheet1.xml', fontSheet]]) {
      assert.ok(SheetJS.CFB.utils.cfb_del(fontArchive, part));
      SheetJS.CFB.utils.cfb_add(fontArchive, part.slice(1), Buffer.from(xml));
    }
    const fontBytes = Uint8Array.from(SheetJS.CFB.write(fontArchive, { type: 'array', fileType: 'zip' }));
    const defaultFont = { size: 11, color: { theme: 1 }, name: 'Calibri', family: 2, scheme: 'minor' };
    const sizeOnly = { italic: true, size: 14 };
    const expectedFonts = {
      A1: [{ bold: true, color: { argb: 'FFFF0000' } }, '0.00%'], B1: [sizeOnly, undefined],
      C1: [{ underline: true, name: 'Arial' }, '0.0'], D1: [defaultFont, undefined], E1: [undefined, undefined],
      F1: [undefined, undefined], G1: [{}, undefined], A2: [sizeOnly, undefined],
      // 4.4.0 also reports bold for <b val="0"/>; the adapter keeps the declared false.
      H1: [{ outline: true, shadow: true, condense: true, extend: true }, undefined],
    };
    const fontBook = await new Workbook().xlsx.load(fontBytes);
    const fontWorksheet = fontBook.getWorksheet('Fonts');
    for (const [cellAddress, [font, numFmt]] of Object.entries(expectedFonts)) {
      assert.deepEqual(fontWorksheet.getCell(cellAddress).font, font, cellAddress);
      assert.equal(fontWorksheet.getCell(cellAddress).numFmt, numFmt, cellAddress);
    }
    assert.deepEqual(fontWorksheet.getRow(2).font, sizeOnly);
    assert.deepEqual(fontWorksheet.getColumn(3).font, { underline: true, name: 'Arial' });
    const fontReader = new stream.xlsx.WorkbookReader(Readable.from([fontBytes]), { styles: 'cache' });
    const streamedFonts = {};
    for await (const worksheet of fontReader) {
      for await (const row of worksheet) {
        row.eachCell((cell) => { streamedFonts[cell.address] = [cell.font, cell.numFmt]; });
        if (row.number === 2) assert.deepEqual(row.font, sizeOnly);
      }
    }
    assert.deepEqual(streamedFonts, expectedFonts);
  }

  // Rich-text run fonts with presence-only elements, as ExcelJS 4.4.0 writes
  // them. 4.4.0's Workbook.xlsx.load returns these runs; the adapter used to
  // refuse the cell value. The streaming reader projects the same fonts.
  {
    const presenceBook = new Workbook();
    const presenceSheet = presenceBook.addWorksheet('Runs');
    presenceSheet.getCell('A1').value = { richText: [{ text: 'o', font: { bold: true } }, { text: 's' }, { text: 'p' }] };
    presenceSheet.getCell('A2').value = { richText: [{ text: 'q', font: { italic: true } }] };
    const presenceArchive = SheetJS.CFB.read(new Uint8Array(await presenceBook.xlsx.writeBuffer({ useSharedStrings: true })), { type: 'array' });
    const presenceStrings = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
      + '<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">'
      + '<si><r><rPr><b/><outline/></rPr><t>o</t></r><r><rPr><condense/><extend/><shadow/></rPr><t>s</t></r><r><t>p</t></r></si>'
      + '<si><r><rPr><i/><sz val="12"/><rFont val="Arial"/></rPr><t>q</t></r></si></sst>';
    assert.ok(SheetJS.CFB.utils.cfb_del(presenceArchive, '/xl/sharedStrings.xml'));
    SheetJS.CFB.utils.cfb_add(presenceArchive, 'xl/sharedStrings.xml', Buffer.from(presenceStrings));
    const presenceBytes = Uint8Array.from(SheetJS.CFB.write(presenceArchive, { type: 'array', fileType: 'zip' }));
    const expectedRuns = [
      { richText: [{ font: { bold: true, outline: true }, text: 'o' }, { font: { condense: true, extend: true, shadow: true }, text: 's' }, { text: 'p' }] },
      { richText: [{ font: { italic: true, size: 12, name: 'Arial' }, text: 'q' }] },
    ];
    const presenceLoaded = (await new Workbook().xlsx.load(presenceBytes)).getWorksheet('Runs');
    assert.deepEqual([presenceLoaded.getCell('A1').value, presenceLoaded.getCell('A2').value], expectedRuns);
    const presenceStreamed = [];
    for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([presenceBytes]))) {
      for await (const row of worksheet) presenceStreamed.push(row.getCell(1).value);
    }
    assert.deepEqual(presenceStreamed, expectedRuns);
  }

  // Inline rich text: 4.4.0's Workbook.xlsx.load returns these runs (its
  // streaming reader keeps only the last run's text). The streaming reader
  // returns the same runs as the normal load, without defaulted name/size.
  {
    const inlineBook = new Workbook();
    inlineBook.addWorksheet('Inline').getCell('A1').value = 1;
    const inlineArchive = SheetJS.CFB.read(new Uint8Array(await inlineBook.xlsx.writeBuffer()), { type: 'array' });
    const inlineSheet = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
      + '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>'
      + '<row r="1"><c r="A1" t="inlineStr"><is><r><rPr><b/><outline/></rPr><t>o</t></r>'
      + '<r><rPr><shadow/><condense/><extend/></rPr><t>s</t></r><r><t>p</t></r></is></c></row>'
      + '<row r="2"><c r="A2" t="inlineStr"><is><r><rPr><i/><sz val="12"/><rFont val="Arial"/></rPr><t>q</t></r>'
      + '<r><rPr><b/></rPr><t>b</t></r></is></c></row></sheetData></worksheet>';
    assert.ok(SheetJS.CFB.utils.cfb_del(inlineArchive, '/xl/worksheets/sheet1.xml'));
    SheetJS.CFB.utils.cfb_add(inlineArchive, 'xl/worksheets/sheet1.xml', Buffer.from(inlineSheet));
    const inlineBytes = Uint8Array.from(SheetJS.CFB.write(inlineArchive, { type: 'array', fileType: 'zip' }));
    const expectedInline = [
      { richText: [{ font: { bold: true, outline: true }, text: 'o' }, { font: { shadow: true, condense: true, extend: true }, text: 's' }, { text: 'p' }] },
      { richText: [{ font: { italic: true, size: 12, name: 'Arial' }, text: 'q' }, { font: { bold: true }, text: 'b' }] },
    ];
    const inlineLoaded = (await new Workbook().xlsx.load(inlineBytes)).getWorksheet('Inline');
    assert.deepEqual([inlineLoaded.getCell('A1').value, inlineLoaded.getCell('A2').value], expectedInline);
    const inlineStreamed = [];
    for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([inlineBytes]))) {
      for await (const row of worksheet) inlineStreamed.push(row.getCell(1).value);
    }
    assert.deepEqual(inlineStreamed, expectedInline);
  }

  // Hyperlinks as ExcelJS 4.4.0 writes them: an in-document link (and any
  // target matching its Sheet!A1 pattern) goes both into the relationship
  // Target and into location. 4.4.0 reads the Target alone; a location that
  // repeats the Target is one link, not 'target#target'. A different location
  // (Excel's URL fragment, E1) is still appended: 4.4.0 drops it, and its tooltip.
  {
    const linkBook = new Workbook();
    linkBook.addWorksheet('Main').getCell('A1').value = 1;
    const linkArchive = SheetJS.CFB.read(new Uint8Array(await linkBook.xlsx.writeBuffer()), { type: 'array' });
    const cell = (ref, text) => `<row r="${ref.slice(1)}"><c r="${ref}" t="inlineStr"><is><t>${text}</t></is></c></row>`;
    const linkSheet = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
      + '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
      + 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData>'
      + cell('A1', 'h') + cell('A2', 'p') + cell('A3', 'q') + cell('A4', 'e') + cell('A5', 'f') + '</sheetData><hyperlinks>'
      + '<hyperlink ref="A1" r:id="rId1" location="#Other!B2"/><hyperlink ref="A2" r:id="rId2" location="Other!B2"/>'
      + '<hyperlink ref="A3" r:id="rId3" tooltip="tt" location="&apos;Other&apos;!C3"/>'
      + '<hyperlink ref="A4" r:id="rId4" location="https://e.com/#Other!B2"/>'
      + '<hyperlink ref="A5" r:id="rId5" location="frag"/></hyperlinks></worksheet>';
    const relationship = (id, target) => `<Relationship Id="${id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="${target}" TargetMode="External"/>`;
    const linkRels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
      + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
      + relationship('rId1', '#Other!B2') + relationship('rId2', 'Other!B2') + relationship('rId3', '&apos;Other&apos;!C3')
      + relationship('rId4', 'https://e.com/#Other!B2') + relationship('rId5', 'https://e.com/p') + '</Relationships>';
    assert.ok(SheetJS.CFB.utils.cfb_del(linkArchive, '/xl/worksheets/sheet1.xml'));
    SheetJS.CFB.utils.cfb_add(linkArchive, 'xl/worksheets/sheet1.xml', Buffer.from(linkSheet));
    if (SheetJS.CFB.find(linkArchive, '/xl/worksheets/_rels/sheet1.xml.rels')) {
      SheetJS.CFB.utils.cfb_del(linkArchive, '/xl/worksheets/_rels/sheet1.xml.rels');
    }
    SheetJS.CFB.utils.cfb_add(linkArchive, 'xl/worksheets/_rels/sheet1.xml.rels', Buffer.from(linkRels));
    const linkBytes = Uint8Array.from(SheetJS.CFB.write(linkArchive, { type: 'array', fileType: 'zip' }));
    const expectedLinks = [
      { text: 'h', hyperlink: '#Other!B2' }, { text: 'p', hyperlink: 'Other!B2' },
      { text: 'q', hyperlink: "'Other'!C3", tooltip: 'tt' }, { text: 'e', hyperlink: 'https://e.com/#Other!B2' },
      { text: 'f', hyperlink: 'https://e.com/p#frag' },
    ];
    const addresses = ['A1', 'A2', 'A3', 'A4', 'A5'];
    const linkLoaded = await new Workbook().xlsx.load(linkBytes);
    const linkSheetLoaded = linkLoaded.getWorksheet('Main');
    assert.deepEqual(addresses.map(address => linkSheetLoaded.getCell(address).value), expectedLinks);
    assert.deepEqual(addresses.map(address => linkSheetLoaded.getCell(address).hyperlink), expectedLinks.map(link => link.hyperlink));
    assert.equal((await linkLoaded.csv.writeBuffer({ sheetName: 'Main' })).toString(),
      expectedLinks.map(link => link.hyperlink).join('\n'));
    const streamedLinks = [];
    for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([linkBytes]), { hyperlinks: 'cache' })) {
      for await (const row of worksheet) streamedLinks.push(row.getCell(1).value);
    }
    assert.deepEqual(streamedLinks, expectedLinks);
  }

  // Note runs as ExcelJS 4.4.0 writes them (rPr without rFont/sz) read like
  // 4.4.0: no name:'' or default size, and outline/shadow runs keep their font.
  {
    const noteBook = new Workbook();
    const noteSheet = noteBook.addWorksheet('Notes');
    for (const address of ['A1', 'A2', 'A3']) {
      noteSheet.getCell(address).value = address;
      noteSheet.getCell(address).note = address;
    }
    const noteArchive = SheetJS.CFB.read(new Uint8Array(await noteBook.xlsx.writeBuffer()), { type: 'array' });
    const noteComments = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
      + '<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>Author</author></authors><commentList>'
      + '<comment ref="A1" authorId="0"><text><r><rPr><b/><sz val="9"/></rPr><t xml:space="preserve">b </t></r><r><t>p</t></r></text></comment>'
      + '<comment ref="A2" authorId="0"><text><r><rPr><i/><u/><color rgb="FFFF0000"/><family val="2"/><vertAlign val="superscript"/>'
      + '<scheme val="minor"/><strike/><sz val="12"/><rFont val="Arial"/></rPr><t>full</t></r><r><rPr><color theme="1"/></rPr><t>c</t></r></text></comment>'
      + '<comment ref="A3" authorId="0"><text><r><rPr><b/><outline/><shadow/></rPr><t>x</t></r></text></comment>'
      + '</commentList></comments>';
    assert.ok(SheetJS.CFB.utils.cfb_del(noteArchive, '/xl/comments1.xml'));
    SheetJS.CFB.utils.cfb_add(noteArchive, 'xl/comments1.xml', Buffer.from(noteComments));
    const noteBytes = Uint8Array.from(SheetJS.CFB.write(noteArchive, { type: 'array', fileType: 'zip' }));
    const noteLoaded = (await new Workbook().xlsx.load(noteBytes)).getWorksheet('Notes');
    const expectedNoteTexts = [
      [{ font: { bold: true, size: 9 }, text: 'b ' }, { text: 'p' }],
      [{ font: { italic: true, underline: true, color: { argb: 'FFFF0000' }, family: 2, vertAlign: 'superscript',
        scheme: 'minor', strike: true, size: 12, name: 'Arial' }, text: 'full' }, { font: { color: { theme: 1 } }, text: 'c' }],
      [{ font: { bold: true, outline: true, shadow: true }, text: 'x' }],
    ];
    assert.deepEqual(['A1', 'A2', 'A3'].map(address => noteLoaded.getCell(address).note.texts), expectedNoteTexts);
    assert.deepEqual(['A1', 'A2', 'A3'].map(address => noteLoaded.getCell(address).model.comment.note.texts), expectedNoteTexts);
  }

  // Sheet properties as ExcelJS 4.4.0 writes them (sheets made with frozen views,
  // tabSelected and outlineProperties; with nothing; with a zoomed view). 4.4.0
  // reports no tabSelected for a loaded view, no outlineProperties when outlinePr
  // is absent, and an empty calcProperties. A declared tabSelected="1" stays true
  // here (4.4.0 drops it). Missing views/headerFooter (null in 4.4.0) are open.
  {
    const propertyBook = new Workbook();
    for (const name of ['Set', 'Plain', 'View', 'Zero']) propertyBook.addWorksheet(name).getCell('A1').value = 0;
    const propertyArchive = SheetJS.CFB.read(new Uint8Array(await propertyBook.xlsx.writeBuffer()), { type: 'array' });
    const sheetXml = (head, value) => '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
      + '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
      + 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" '
      + 'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="x14ac" '
      + 'xmlns:x14ac="http://schemas.microsoft.com/office/spreadsheetml/2009/9/ac">' + head
      + '<sheetFormatPr defaultRowHeight="15" outlineLevelRow="0" outlineLevelCol="0" x14ac:dyDescent="55"/>'
      + `<sheetData><row r="1" spans="1:1" x14ac:dyDescent="0.25"><c r="A1"><v>${value}</v></c></row></sheetData>`
      + '<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>'
      + '<pageSetup orientation="portrait" horizontalDpi="4294967295" verticalDpi="4294967295" scale="100" fitToWidth="1" fitToHeight="1"/></worksheet>';
    const propertySheets = [
      '<sheetPr><outlinePr summaryBelow="0"/></sheetPr><dimension ref="A1:A1"/><sheetViews><sheetView workbookViewId="0" tabSelected="1">'
        + '<pane xSplit="1" ySplit="1" topLeftCell="B2" activePane="bottomRight" state="frozen"/><selection pane="bottomRight"/></sheetView></sheetViews>',
      '<dimension ref="A1:A1"/>',
      '<dimension ref="A1:A1"/><sheetViews><sheetView workbookViewId="0" zoomScale="80"/></sheetViews>',
      // Hand-edited (not written by 4.4.0, which only writes "1"): 4.4.0 omits it as well.
      '<dimension ref="A1:A1"/><sheetViews><sheetView workbookViewId="0" zoomScale="80" tabSelected="0"/></sheetViews>',
    ];
    for (const [index, head] of propertySheets.entries()) {
      assert.ok(SheetJS.CFB.utils.cfb_del(propertyArchive, `/xl/worksheets/sheet${index + 1}.xml`));
      SheetJS.CFB.utils.cfb_add(propertyArchive, `xl/worksheets/sheet${index + 1}.xml`, Buffer.from(sheetXml(head, index + 1)));
    }
    const propertyBytes = Uint8Array.from(SheetJS.CFB.write(propertyArchive, { type: 'array', fileType: 'zip' }));
    const propertyLoaded = await new Workbook().xlsx.load(propertyBytes);
    const [setSheet, plainSheet, viewSheet, zeroSheet] = ['Set', 'Plain', 'View', 'Zero'].map(name => propertyLoaded.getWorksheet(name));
    const viewBase = { workbookViewId: 0, rightToLeft: false, showRuler: true, showRowColHeaders: true,
      showGridLines: true, zoomScale: 100, zoomScaleNormal: 100 };
    assert.deepEqual(setSheet.views, [{ ...viewBase, state: 'frozen', xSplit: 1, ySplit: 1, topLeftCell: 'B2', tabSelected: true }]);
    assert.deepEqual(viewSheet.views, [{ ...viewBase, state: 'normal', zoomScale: 80 }]);
    assert.deepEqual(zeroSheet.views, viewSheet.views);
    assert.equal(Object.hasOwn(viewSheet.views[0], 'tabSelected'), false);
    assert.equal(Object.hasOwn(zeroSheet.views[0], 'tabSelected'), false);
    assert.deepEqual(setSheet.properties.outlineProperties, { summaryBelow: false, summaryRight: undefined });
    for (const sheet of [plainSheet, viewSheet]) {
      assert.equal(sheet.properties.outlineProperties, undefined);
      assert.equal(sheet.model.properties.outlineProperties, undefined);
    }
    assert.deepEqual(setSheet.model.properties.outlineProperties, { summaryBelow: false, summaryRight: undefined });
    assert.equal(propertyLoaded.calcProperties.fullCalcOnLoad, undefined);
    assert.equal(JSON.stringify(propertyLoaded.calcProperties), '{}');
    // The same in session: fresh sheets and workbooks.
    const freshBook = new Workbook();
    const freshSheet = freshBook.addWorksheet('Fresh', { views: [{ state: 'frozen', xSplit: 1, ySplit: 1 }] });
    assert.equal(freshSheet.properties.outlineProperties, undefined);
    assert.equal(Object.hasOwn(freshSheet.views[0], 'tabSelected'), false);
    assert.equal(JSON.stringify(freshBook.calcProperties), '{}');
    assert.equal(JSON.stringify(new Workbook().calcProperties), '{}');
    freshSheet.properties.outlineProperties = { summaryRight: false };
    assert.deepEqual(freshSheet.properties.outlineProperties, { summaryBelow: undefined, summaryRight: false });
    freshSheet.views = [{ tabSelected: true }];
    assert.equal(freshSheet.views[0].tabSelected, true);
  }

  // Conditional formatting as ExcelJS 4.4.0 writes it (dxfs and cfRules copied
  // from its output). Like 4.4.0, rules carry no dxfId and empty dxf border
  // edges (<left/>, <diagonal/>) are dropped. 4.4.0's null for unset style keys,
  // {id, formatCode} numFmt and operator:'containsText' appear only after load
  // (in session it echoes the input) and are left open.
  {
    const cfBook = new Workbook();
    cfBook.addWorksheet('C').getCell('A1').value = 1;
    const cfArchive = SheetJS.CFB.read(new Uint8Array(await cfBook.xlsx.writeBuffer()), { type: 'array' });
    const readEntry = path => Buffer.from(SheetJS.CFB.find(cfArchive, path).content).toString();
    const replaceEntry = (path, text) => {
      assert.ok(SheetJS.CFB.utils.cfb_del(cfArchive, `/${path}`));
      SheetJS.CFB.utils.cfb_add(cfArchive, path, Buffer.from(text));
    };
    const cfStyles = readEntry('/xl/styles.xml');
    assert.ok(cfStyles.includes('<dxfs count="0"/>'));
    replaceEntry('xl/styles.xml', cfStyles.replace('<dxfs count="0"/>', '<dxfs count="4">'
      + '<dxf><font><b/></font></dxf>'
      + '<dxf><fill><patternFill patternType="solid"><bgColor rgb="FFFF0000"/></patternFill></fill></dxf>'
      + '<dxf><numFmt numFmtId="2" formatCode="0.00"/><border><left/><right/><top style="thin"><color rgb="FF00FF00"/></top><bottom/><diagonal/></border></dxf>'
      + '<dxf><font><i/><color rgb="FF0000FF"/></font><alignment horizontal="center"/></dxf></dxfs>'));
    const cfSheet = readEntry('/xl/worksheets/sheet1.xml');
    assert.ok(cfSheet.includes('</sheetData>'));
    replaceEntry('xl/worksheets/sheet1.xml', cfSheet.replace('</sheetData>', '</sheetData>'
      + '<conditionalFormatting sqref="A1:A5"><cfRule type="expression" dxfId="0" priority="1"><formula>A1&gt;0</formula></cfRule>'
      + '<cfRule type="cellIs" dxfId="1" priority="2" operator="greaterThan"><formula>3</formula></cfRule>'
      + '<cfRule type="cellIs" dxfId="2" priority="3" operator="lessThan"><formula>0</formula></cfRule>'
      + '<cfRule type="containsText" dxfId="3" priority="4"><formula>NOT(ISERROR(SEARCH(&quot;x&quot;,A1)))</formula></cfRule>'
      + '<cfRule type="top10" priority="5" rank="2"/></conditionalFormatting>'));
    const cfLoaded = (await new Workbook().xlsx.load(Uint8Array.from(SheetJS.CFB.write(cfArchive, { type: 'array', fileType: 'zip' }))))
      .getWorksheet('C');
    const expectedCfRules = [
      { type: 'expression', priority: 1, formulae: ['A1>0'], style: { font: { bold: true } } },
      { type: 'cellIs', operator: 'greaterThan', priority: 2, formulae: ['3'],
        style: { fill: { type: 'pattern', pattern: 'solid', bgColor: { argb: 'FFFF0000' } } } },
      { type: 'cellIs', operator: 'lessThan', priority: 3, formulae: ['0'],
        style: { numFmt: '0.00', border: { top: { style: 'thin', color: { argb: 'FF00FF00' } } } } },
      { type: 'containsText', priority: 4, formulae: ['NOT(ISERROR(SEARCH("x",A1)))'],
        style: { font: { italic: true, color: { argb: 'FF0000FF' } }, alignment: { horizontal: 'center' } } },
      { type: 'top10', priority: 5, rank: 2 },
    ];
    assert.deepEqual(cfLoaded.conditionalFormattings, [{ ref: 'A1:A5', rules: expectedCfRules }]);
    assert.deepEqual(cfLoaded.model.conditionalFormattings, [{ ref: 'A1:A5', rules: expectedCfRules }]);
    assert.ok(cfLoaded.conditionalFormattings[0].rules.every(rule => !Object.hasOwn(rule, 'dxfId')));
  }

  // Standard data bars report only the declared minLength/maxLength/showValue, as
  // 4.4.0 does in session; the writer leaves the undeclared attributes out, so a
  // reload reports the same. 4.4.0 never reads <dataBar> attributes: declared ones
  // in a file stay reported here (deliberate difference).
  {
    const barBook = new Workbook();
    const barSheet = barBook.addWorksheet('Bars');
    const barBase = { type: 'dataBar', cfvo: [{ type: 'min' }, { type: 'max' }], color: { argb: 'FF638EC6' } };
    const barRules = [{}, { minLength: 5 }, { showValue: false }, { maxLength: 70, showValue: true }]
      .map((extra, index) => ({ ...barBase, priority: index + 1, ...extra }));
    barRules.forEach((rule, index) => barSheet.addConditionalFormatting({ ref: `A${index + 1}`, rules: [rule] }));
    const barBlocks = barRules.map((rule, index) => ({ ref: `A${index + 1}`, rules: [rule] }));
    assert.deepEqual(barSheet.conditionalFormattings, barBlocks);
    assert.deepEqual(barSheet.model.conditionalFormattings, barBlocks);
    const barBuffer = await barBook.xlsx.writeBuffer();
    const barXml = Buffer.from(SheetJS.CFB.find(SheetJS.CFB.read(new Uint8Array(barBuffer), { type: 'array' }),
      '/xl/worksheets/sheet1.xml').content).toString();
    assert.deepEqual(barXml.match(/<dataBar[^>]*>/g),
      ['<dataBar>', '<dataBar minLength="5">', '<dataBar showValue="0">', '<dataBar maxLength="70" showValue="1">']);
    const barReloaded = (await new Workbook().xlsx.load(barBuffer)).getWorksheet('Bars');
    assert.deepEqual(barReloaded.conditionalFormattings, barBlocks);
    assert.deepEqual(barReloaded.model.conditionalFormattings, barBlocks);
    // Omitted limits are still checked against the schema defaults (10 and 90).
    assert.throws(() => barSheet.addConditionalFormatting({ ref: 'B1', rules: [{ ...barBase, priority: 9, minLength: 95 }] }),
      { code: 'INVALID_VALUE' });
    const barArchive = SheetJS.CFB.read(new Uint8Array(barBuffer), { type: 'array' });
    const declaredXml = barXml.replace('<dataBar>', '<dataBar minLength="3" maxLength="70" showValue="0">');
    assert.ok(SheetJS.CFB.utils.cfb_del(barArchive, '/xl/worksheets/sheet1.xml'));
    SheetJS.CFB.utils.cfb_add(barArchive, 'xl/worksheets/sheet1.xml', Buffer.from(declaredXml));
    const declaredBars = (await new Workbook().xlsx.load(Uint8Array.from(SheetJS.CFB.write(barArchive, { type: 'array', fileType: 'zip' }))))
      .getWorksheet('Bars');
    assert.deepEqual(declaredBars.conditionalFormattings[0].rules,
      [{ ...barBase, priority: 1, minLength: 3, maxLength: 70, showValue: false }]);
  }

  // Text rules as Excel writes them (text attribute plus formula). 4.4.0 never
  // reads the text attribute (it echoes text only in session and never writes it);
  // it is file data, so it stays reported here (deliberate difference).
  {
    const textBook = new Workbook();
    textBook.addWorksheet('T').getCell('A1').value = 'x';
    const textArchive = SheetJS.CFB.read(new Uint8Array(await textBook.xlsx.writeBuffer()), { type: 'array' });
    const textSheet = Buffer.from(SheetJS.CFB.find(textArchive, '/xl/worksheets/sheet1.xml').content).toString();
    assert.ok(SheetJS.CFB.utils.cfb_del(textArchive, '/xl/worksheets/sheet1.xml'));
    SheetJS.CFB.utils.cfb_add(textArchive, 'xl/worksheets/sheet1.xml', Buffer.from(textSheet.replace('</sheetData>', '</sheetData>'
      + '<conditionalFormatting sqref="A1"><cfRule type="containsText" priority="1" operator="containsText" text="x">'
      + '<formula>NOT(ISERROR(SEARCH("x",A1)))</formula></cfRule>'
      + '<cfRule type="beginsWith" priority="2" operator="beginsWith" text="z"><formula>LEFT(A1,LEN("z"))="z"</formula></cfRule>'
      + '</conditionalFormatting>')));
    const textLoaded = (await new Workbook().xlsx.load(Uint8Array.from(SheetJS.CFB.write(textArchive, { type: 'array', fileType: 'zip' }))))
      .getWorksheet('T');
    assert.deepEqual(textLoaded.conditionalFormattings[0].rules, [
      { type: 'containsText', operator: 'containsText', priority: 1, text: 'x', formulae: ['NOT(ISERROR(SEARCH("x",A1)))'] },
      { type: 'beginsWith', operator: 'beginsWith', priority: 2, text: 'z', formulae: ['LEFT(A1,LEN("z"))="z"'] },
    ]);
  }

  // A conditional (dxf) pattern fill may omit the pattern, as 4.4.0 accepts it and
  // writes <patternFill> without patternType (solid in a dxf); the dxf written here
  // is the same. Cell fills still need a pattern. Reading reports 'solid' (4.4.0:
  // no pattern key), since the writer omits patternType for 'solid' too.
  {
    const dxfFillBook = new Workbook();
    const dxfFillSheet = dxfFillBook.addWorksheet('F');
    const greenFill = { type: 'pattern', bgColor: { argb: 'FF00FF00' } };
    dxfFillSheet.addConditionalFormatting({ ref: 'A1', rules: [
      { type: 'expression', formulae: ['TRUE'], priority: 1, style: { fill: greenFill } },
      { type: 'expression', formulae: ['FALSE'], priority: 2, style: { fill: { ...greenFill, pattern: undefined } } },
    ] });
    const solidGreen = { type: 'pattern', pattern: 'solid', bgColor: { argb: 'FF00FF00' } };
    assert.deepEqual(dxfFillSheet.conditionalFormattings[0].rules.map(rule => rule.style), [{ fill: solidGreen }, { fill: solidGreen }]);
    const dxfFillBuffer = await dxfFillBook.xlsx.writeBuffer();
    const dxfFillStyles = Buffer.from(SheetJS.CFB.find(SheetJS.CFB.read(new Uint8Array(dxfFillBuffer), { type: 'array' }),
      '/xl/styles.xml').content).toString();
    assert.equal(dxfFillStyles.match(/<dxfs.*<\/dxfs>/s)?.[0], '<dxfs count="1"><dxf><fill><patternFill><bgColor rgb="FF00FF00"/>'
      + '</patternFill></fill></dxf></dxfs>');
    const dxfFillReloaded = (await new Workbook().xlsx.load(dxfFillBuffer)).getWorksheet('F');
    assert.deepEqual(dxfFillReloaded.model.conditionalFormattings[0].rules.map(rule => rule.style), [{ fill: solidGreen }, { fill: solidGreen }]);
    // The shape 4.4.0 loads from such a dxf (no pattern key) copies through the model.
    const dxfFillCopy = new Workbook().addWorksheet('Copy');
    dxfFillCopy.model = { rows: [], conditionalFormattings: [{ ref: 'B1', rules: [
      { type: 'expression', formulae: ['TRUE'], priority: 1, style: { fill: greenFill } }] }] };
    assert.deepEqual(dxfFillCopy.conditionalFormattings[0].rules[0].style, { fill: solidGreen });
    assert.throws(() => dxfFillSheet.addConditionalFormatting({ ref: 'C1', rules: [
      { type: 'expression', formulae: ['TRUE'], priority: 3, style: { fill: { ...greenFill, pattern: 'bogus' } } }] }),
    { code: 'UNSUPPORTED_OPTIONS' });
    assert.throws(() => { dxfFillSheet.getCell('A1').fill = greenFill; }, { code: 'UNSUPPORTED_OPTIONS' });
  }

  // worksheet.dimensions.model as 4.4.0's Range reports it: the bounds of cells and
  // merges, all zero (while top/left/... read 1 and the range is A1:A1) without them.
  {
    const dimensionSource = new Workbook();
    const dimensionCells = dimensionSource.addWorksheet('Cells');
    dimensionCells.getCell('B2').value = 1;
    dimensionCells.getCell('D5').value = 2;
    dimensionSource.addWorksheet('Empty');
    const dimensionLoaded = await new Workbook().xlsx.load(await dimensionSource.xlsx.writeBuffer());
    assert.deepEqual(dimensionLoaded.getWorksheet('Cells').dimensions.model, { top: 2, left: 2, bottom: 5, right: 4 });
    const emptyDimensions = dimensionLoaded.getWorksheet('Empty').dimensions;
    assert.deepEqual(emptyDimensions.model, { top: 0, left: 0, bottom: 0, right: 0 });
    assert.deepEqual([emptyDimensions.top, emptyDimensions.right, emptyDimensions.range], [1, 1, 'A1:A1']);
    const dimensionSheet = new Workbook().addWorksheet('Fresh');
    assert.deepEqual(dimensionSheet.dimensions.model, { top: 0, left: 0, bottom: 0, right: 0 });
    dimensionSheet.mergeCells('B2:C3');
    assert.deepEqual(dimensionSheet.dimensions.model, { top: 2, left: 2, bottom: 3, right: 3 });
    dimensionSheet.getCell('E9').value = 1;
    assert.deepEqual(dimensionSheet.dimensions.model, { top: 2, left: 2, bottom: 9, right: 5 });
    dimensionSheet.unMergeCells('B2:C3');
    dimensionSheet.getCell('E9').value = null;
    assert.deepEqual(dimensionSheet.dimensions.model, { top: 0, left: 0, bottom: 0, right: 0 });
  }

  const richWriter = new stream.xlsx.WorkbookWriter({ useSharedStrings: true });
  const differentlyStyled = { richText: [
    { text: 'Styled', font: { name: 'Calibri', size: 12, italic: true } }, { text: ' plain' },
  ] };
  const richShared = richWriter.addWorksheet('Shared');
  richShared.addRow([authoredRich, authoredRich, differentlyStyled, 'Styled plain']).commit();
  const richInline = richWriter.addWorksheet('Inline', { useSharedStrings: false });
  richInline.addRow([authoredRich]).commit();
  await richWriter.commit();
  const streamedRichBytes = richWriter.stream.read();
  const streamedRich = await new Workbook().xlsx.load(streamedRichBytes);
  assert.deepEqual(streamedRich.getWorksheet('Shared').getCell('A1').value, authoredRich);
  assert.deepEqual(streamedRich.getWorksheet('Shared').getCell('C1').value, differentlyStyled);
  assert.equal(streamedRich.getWorksheet('Shared').getCell('D1').value, 'Styled plain');
  assert.deepEqual(streamedRich.getWorksheet('Inline').getCell('A1').value, authoredRich);
  const streamedRichZip = new XlsxZipReader(streamedRichBytes.length, (offset, length) => streamedRichBytes.subarray(offset, offset + length));
  try {
    assert.equal(streamedRichZip.shared_text('xl/sharedStrings.xml').length, 3);
    const inlineXml = Buffer.from(streamedRichZip.read_metadata('xl/worksheets/sheet2.xml', 65536)).toString();
    assert.match(inlineXml, /t="inlineStr"><is><r><rPr>/);
  } finally {
    streamedRichZip.free();
  }

  const styledFile = join(directory, 'styled-stream-writer.xlsx');
  const styledWriter = new stream.xlsx.WorkbookWriter({ filename: styledFile, useStyles: true, useSharedStrings: true });
  const styledSheet = styledWriter.addWorksheet('Styled');
  const style = {
    numFmt: '0.00',
    font: { name: 'Calibri', size: 13, bold: true },
    fill: { type: 'pattern', pattern: 'solid', fgColor: { argb: 'FF998877' } },
    border: { bottom: { style: 'thin', color: { argb: 'FF443322' } } },
    alignment: { horizontal: 'center', wrapText: true },
    protection: { locked: false, hidden: true },
  };
  const styledRow = styledSheet.addRow([2.5, 4.5, timestamp, { formula: 'A1+B1', result: 7 }]);
  styledRow.getCell('A').style = style;
  styledRow.getCell('B').style = structuredClone(style);
  styledRow.getCell('C').font = { name: 'Calibri', size: 11, italic: true };
  styledRow.getCell('D').numFmt = '0.000';
  styledRow.getCell('E').fill = style.fill;
  styledRow.getCell('F').value = 'with shared strings';
  styledRow.getCell('F').font = style.font;
  styledRow.commit();
  await styledWriter.commit();
  const styledRoundTrip = await new Workbook().xlsx.readFile(styledFile);
  const persistedStyled = styledRoundTrip.getWorksheet('Styled');
  assert.equal(persistedStyled.getCell('A1').numFmt, style.numFmt);
  assert.equal(persistedStyled.getCell('A1').font.bold, true);
  assert.deepEqual(persistedStyled.getCell('A1').fill.fgColor, style.fill.fgColor);
  assert.deepEqual(persistedStyled.getCell('A1').border, style.border);
  assert.deepEqual(persistedStyled.getCell('A1').alignment, style.alignment);
  assert.deepEqual(persistedStyled.getCell('A1').protection, style.protection);
  assert.deepEqual(persistedStyled.getCell('C1').value, timestamp);
  assert.equal(persistedStyled.getCell('C1').font.italic, true);
  assert.equal(persistedStyled.getCell('D1').numFmt, '0.000');
  assert.deepEqual(persistedStyled.getCell('E1').fill.fgColor, style.fill.fgColor);
  assert.equal(persistedStyled.getCell('E1').value, null);
  assert.equal(persistedStyled.getCell('F1').font.bold, true);
  assert.equal(persistedStyled.getCell('F1').value, 'with shared strings');
  const styledDocument = new XlsxDocument(fs.readFileSync(styledFile));
  try {
    const firstStyle = styledDocument.cell_style_index('Styled', 0, 0);
    assert.ok(firstStyle > 0);
    assert.equal(styledDocument.cell_style_index('Styled', 0, 1), firstStyle);
  } finally {
    styledDocument.free();
  }

  for (const useStyles of [true, false]) {
    const formatWriter = new stream.xlsx.WorkbookWriter({ useStyles });
    const formatSheet = formatWriter.addWorksheet('Formats');
    const formatCell = formatSheet.getCell('A1');
    formatCell.value = 3;
    formatCell.numFmt = '0.000';
    await formatWriter.commit();
    const formatRoundTrip = await new Workbook().xlsx.load(formatWriter.stream.read());
    assert.equal(formatRoundTrip.getWorksheet('Formats').getCell('A1').numFmt,
      useStyles ? '0.000' : undefined);
  }

  const inheritedWriter = new stream.xlsx.WorkbookWriter({ useStyles: true });
  const inheritedSheet = inheritedWriter.addWorksheet('Inheritance');
  inheritedSheet.columns = [{ key: 'amount', style: { numFmt: '0.000', font: { name: 'Calibri', size: 11, bold: true } } }];
  inheritedSheet.getColumn('amount').alignment = { horizontal: 'right' };
  const inheritedRow = inheritedSheet.addRow({ amount: 10 });
  inheritedRow.font = { name: 'Calibri', size: 12, italic: true };
  inheritedRow.getCell('amount').alignment = { horizontal: 'center' };
  inheritedRow.commit();
  const formattedEmpty = inheritedSheet.getRow(5);
  formattedEmpty.numFmt = '0.00';
  formattedEmpty.commit();
  await inheritedWriter.commit();
  const inheritedBytes = inheritedWriter.stream.read();
  const inheritedResult = (await new Workbook().xlsx.load(inheritedBytes)).getWorksheet('Inheritance');
  assert.equal(inheritedResult.getColumn(1).numFmt, '0.000');
  assert.equal(inheritedResult.getColumn(1).font.bold, true);
  assert.deepEqual(inheritedResult.getColumn(1).alignment, { horizontal: 'right' });
  assert.equal(inheritedResult.getRow(1).font.italic, true);
  assert.equal(inheritedResult.getRow(5).numFmt, '0.00');
  assert.equal(inheritedResult.getCell('A1').numFmt, '0.000');
  assert.equal(inheritedResult.getCell('A1').font.italic, true);
  assert.equal(inheritedResult.getCell('A1').font.bold, undefined);
  assert.deepEqual(inheritedResult.getCell('A1').alignment, { horizontal: 'center' });
  // CE 0.20.3 ignores `dimension ref="A1"`; the formatted empty row 5 counts.
  assert.equal(SheetJS.read(inheritedBytes).Sheets.Inheritance['!ref'], 'A1:A5');

  const arrayWriter = new stream.xlsx.WorkbookWriter();
  const arraySheet = arrayWriter.addWorksheet('Array');
  const arrayRow = arraySheet.addRow([{ formula: 'C1:D2*2', shareType: 'array', ref: 'A1:B2', result: 2 }, 4, 1, 2]);
  arrayRow.commit();
  const arrayMemberRow = arraySheet.addRow([6, { formula: '3+5', result: 8 }, 3, 4]);
  assert.throws(() => arrayMemberRow.commit(), /Array formula members/);
  arrayMemberRow.getCell(2).value = { formula: '1+2', shareType: 'array', ref: 'B2:C3', result: 3 };
  assert.throws(() => arrayMemberRow.commit(), /Array formulas must not overlap/);
  arrayMemberRow.getCell(2).value = 8;
  assert.throws(() => arraySheet.mergeCells('B2:B3'), /overlap an array formula/);
  arrayMemberRow.commit();
  const misalignedArray = arraySheet.addRow([null, { formula: '1+2', shareType: 'array', ref: 'A3:B3', result: 3 }]);
  assert.throws(() => misalignedArray.commit(), /array formula must start at its anchor cell/);
  misalignedArray.getCell(2).value = null;
  await arrayWriter.commit();
  const arrayBytes = arrayWriter.stream.read();
  const arrayDocument = new XlsxDocument(arrayBytes);
  try {
    const xml = Buffer.from(arrayDocument.zip_entry_bytes('xl/worksheets/sheet1.xml')).toString();
    assert.match(xml, /<f t="array" ref="A1:B2">C1:D2\*2<\/f><v>2<\/v>/);
  } finally {
    arrayDocument.free();
  }
  const arrayResult = (await new Workbook().xlsx.load(arrayBytes)).getWorksheet('Array');
  assert.deepEqual(arrayResult.getCell('A1').value,
    { formula: 'C1:D2*2', shareType: 'array', ref: 'A1:B2', result: 2 });
  assert.equal(arrayResult.getCell('B2').value, 8);
  const readArray = new stream.xlsx.WorkbookReader(Readable.from([arrayBytes]));
  const observedArray = [];
  for await (const sheet of readArray) {
    for await (const row of sheet) observedArray.push(row.getCell('A').value);
  }
  assert.deepEqual(observedArray, [
    { formula: 'C1:D2*2', shareType: 'array', ref: 'A1:B2', result: 2 }, 6,
  ]);

  const sharedFormulaWriter = new stream.xlsx.WorkbookWriter();
  const formulaSheet = sharedFormulaWriter.addWorksheet('Shared formulas');
  const masterRow = formulaSheet.addRow([
    { formula: 'C1*2', shareType: 'shared', ref: 'A1:B2', result: 6 },
    { sharedFormula: 'A2', result: 8 }, 3, 4,
  ]);
  assert.throws(() => masterRow.commit(), /Shared formula master must precede/);
  masterRow.getCell('B').value = { sharedFormula: '$A$1', result: 8 };
  assert.equal(masterRow.getCell('B').formulaType, FormulaType.Shared);
  assert.equal(masterRow.getCell('A').formula, 'C1*2');
  assert.equal(masterRow.getCell('B').formula, 'D1*2');
  assert.equal(masterRow.getCell('B').result, 8);
  masterRow.commit();
  const sharedMemberRow = formulaSheet.addRow([
    { sharedFormula: 'A1', result: 10 }, { formula: 'D2*2', result: 12 }, 5, 6,
  ]);
  assert.throws(() => sharedMemberRow.commit(), /Shared formula range requires members/);
  sharedMemberRow.getCell('B').value = { sharedFormula: 'A1', result: 12 };
  assert.equal(sharedMemberRow.getCell('A').formula, 'C2*2');
  assert.equal(sharedMemberRow.getCell('B').formula, 'D2*2');
  assert.equal(sharedMemberRow.getCell('B').result, 12);
  assert.equal(sharedMemberRow.getCell('C').formula, undefined);
  assert.equal(sharedMemberRow.getCell('C').result, undefined);
  assert.throws(() => formulaSheet.mergeCells('B2:C2'), /overlap a shared formula/);
  sharedMemberRow.commit();
  const secondMaster = formulaSheet.addRow([
    { formula: 'C3*2', shareType: 'shared', ref: 'A3:B3', result: 14 },
    { sharedFormula: 'A3', result: 16 }, 7, 8,
  ]);
  secondMaster.commit();
  await sharedFormulaWriter.commit();
  const sharedFormulaBytes = sharedFormulaWriter.stream.read();
  const sharedFormulaDocument = new XlsxDocument(sharedFormulaBytes);
  try {
    const xml = Buffer.from(sharedFormulaDocument.zip_entry_bytes('xl/worksheets/sheet1.xml')).toString();
    assert.match(xml, /<f t="shared" ref="A1:B2" si="0">C1\*2<\/f>/);
    assert.match(xml, /<f t="shared" si="0"\/><v>12<\/v>/);
    assert.match(xml, /<f t="shared" ref="A3:B3" si="1">C3\*2<\/f>/);
  } finally {
    sharedFormulaDocument.free();
  }
  const sharedFormulaResult = (await new Workbook().xlsx.load(sharedFormulaBytes)).getWorksheet('Shared formulas');
  assert.deepEqual(sharedFormulaResult.getCell('A1').value,
    { formula: 'C1*2', shareType: 'shared', ref: 'A1:B2', result: 6 });
  assert.deepEqual(sharedFormulaResult.getCell('B2').value, { sharedFormula: 'A1', result: 12 });
  assert.equal(sharedFormulaResult.getCell('B2').formula, 'D2*2');
  const sharedFormulaRead = new stream.xlsx.WorkbookReader(Readable.from([sharedFormulaBytes]));
  const sharedFormulaRows = [];
  const sharedFormulaMembers = [];
  let sharedAnchorRow;
  for await (const sheet of sharedFormulaRead) {
    for await (const row of sheet) {
      sharedFormulaRows.push(row.values.slice(1, 3));
      sharedFormulaMembers.push(row.getCell('B'));
      if (row.number === 1) sharedAnchorRow = row;
    }
  }
  assert.deepEqual(sharedFormulaMembers.map(cell => cell.formula), ['D1*2', 'D2*2', 'D3*2']);
  assert.deepEqual(sharedFormulaMembers.map(cell => cell.formulaType), Array(3).fill(FormulaType.Shared));
  const translateDetachedFormula = XlsxDocument.translate_detached_formula;
  let translations = 0;
  XlsxDocument.translate_detached_formula = (...args) => {
    translations++;
    return translateDetachedFormula(...args);
  };
  try {
    assert.equal(sharedFormulaMembers[1].formula, 'D2*2');
    assert.equal(sharedFormulaMembers[1].formula, 'D2*2');
    assert.equal(translations, 0);
    sharedAnchorRow.getCell('A').value = { formula: 'E1+1', result: 20 };
    assert.equal(sharedFormulaMembers[1].formula, 'F2+1');
    assert.equal(sharedFormulaMembers[1].formula, 'F2+1');
    assert.equal(translations, 1);
  } finally {
    XlsxDocument.translate_detached_formula = translateDetachedFormula;
  }
  const sharedAnchor = sharedAnchorRow.getCell('A');
  sharedAnchor.value = { formula: 'E1+2', result: 21 };
  assert.deepEqual(sharedFormulaMembers.map(cell => cell.formula), ['F1+2', 'F2+2', 'D3*2']);
  sharedAnchor.model = { type: ValueType.Formula, formula: '$G1+3', result: 31 };
  assert.equal(sharedFormulaMembers[1].formula, '$G2+3');
  sharedAnchorRow.values = [{ formula: 'H1+4', result: 41 }];
  assert.equal(sharedFormulaMembers[1].formula, 'I2+4');
  sharedAnchor.value = { formula: 'J1+5', result: 51 };
  assert.equal(sharedFormulaMembers[1].formula, 'I2+4');
  sharedAnchorRow.getCell('A').value = null;
  assert.equal(sharedFormulaMembers[1].formula, undefined);
  let spliceFormulaRow;
  let spliceFormulaMember;
  for await (const sheet of new stream.xlsx.WorkbookReader(Readable.from([sharedFormulaBytes]))) {
    for await (const row of sheet) {
      if (row.number === 1) spliceFormulaRow = row;
      if (row.number === 2) spliceFormulaMember = row.getCell('B');
    }
  }
  assert.equal(spliceFormulaMember.formula, 'D2*2');
  spliceFormulaRow.splice(1, 0, 'Inserted');
  assert.equal(spliceFormulaRow.getCell('C').formula, 'D1*2');
  assert.equal(spliceFormulaMember.formula, 'C2*2');
  spliceFormulaRow.getCell('B').value = { formula: '$E1+3', result: 12 };
  assert.equal(spliceFormulaMember.formula, '$E2+3');
  spliceFormulaRow.splice(2, 1);
  assert.equal(spliceFormulaMember.formula, undefined);
  assert.equal(XlsxDocument.translate_detached_formula('$C1+D$1+"A1"', 'A1', 'B2'), '$C2+E$1+"A1"');
  assert.deepEqual(sharedFormulaRows, [
    [{ formula: 'C1*2', shareType: 'shared', ref: 'A1:B2', result: 6 }, { sharedFormula: 'A1', result: 8 }],
    [{ sharedFormula: 'A1', result: 10 }, { sharedFormula: 'A1', result: 12 }],
    [{ formula: 'C3*2', shareType: 'shared', ref: 'A3:B3', result: 14 }, { sharedFormula: 'A3', result: 16 }],
  ]);

  assert.equal(XlsxDocument.translate_detached_formula('C1+Budget', 'A1', 'B2', ['Budget']), 'D2+Budget');
  assert.equal(XlsxDocument.translate_detached_formula('C1+Budget', 'A1', 'B2', ['c1']), undefined);
  const formulaNameBook = new Workbook();
  formulaNameBook.addWorksheet('Formula names').fillFormula('A1:B1', 'C1+Budget', [1, 2]);
  formulaNameBook.definedNames.add("'Formula names'!$A$1", 'Budget');
  const formulaNameBytes = await formulaNameBook.xlsx.writeBuffer();
  const formulaNameArchive = new XlsxZipReader(formulaNameBytes.length,
    (offset, length) => formulaNameBytes.subarray(offset, offset + length));
  try {
    assert.deepEqual(formulaNameArchive.workbook_manifest(true)[3], ['Budget']);
    assert.equal(formulaNameArchive.workbook_manifest()[3], 1);
  } finally {
    formulaNameArchive.free();
  }

  const formulaCollisionSource = new XlsxZipReader(formulaNameBytes.length,
    (offset, length) => formulaNameBytes.subarray(offset, offset + length));
  const formulaCollisionZip = new XlsxZipStream();
  let formulaCollisionBytes;
  try {
    const encoder = new TextEncoder();
    const decoder = new TextDecoder();
    for (const part of formulaCollisionSource.entry_names()) {
      const bytes = formulaCollisionSource.read_metadata(part, 65536);
      formulaCollisionZip.start_entry(part, false);
      formulaCollisionZip.write_chunk(part === 'xl/workbook.xml'
        ? encoder.encode(decoder.decode(bytes).replace('name="Budget"', 'name="c1"')) : bytes);
    }
    formulaCollisionBytes = formulaCollisionZip.finish();
  } finally {
    formulaCollisionZip.free();
    formulaCollisionSource.free();
  }
  for (const [bytes, expected] of [[formulaNameBytes, 'D1+Budget'], [formulaCollisionBytes, undefined]]) {
    for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([bytes]))) {
      for await (const row of worksheet) {
        assert.equal(row.getCell('B').formulaType, FormulaType.Shared);
        assert.equal(row.getCell('B').formula, expected);
      }
    }
  }

  async function writeWithZipSettings(zip) {
    const writer = new stream.xlsx.WorkbookWriter({ zip, created: new Date('2025-01-01T00:00:00Z') });
    const sheet = writer.addWorksheet('Compression options');
    for (let index = 0; index < 256; index += 1) {
      sheet.addRow([`Group ${index % 16}: ${'spreadsheet streaming '.repeat(7)}`, index]).commit();
    }
    await writer.commit();
    const bytes = writer.stream.read();
    const result = await new Workbook().xlsx.load(bytes);
    assert.equal(result.getWorksheet('Compression options').getCell('B256').value, 255);
    return bytes;
  }
  function zipHeaderTime(bytes) {
    const time = bytes.readUInt16LE(10);
    const date = bytes.readUInt16LE(12);
    return Date.UTC(1980 + (date >> 9), ((date >> 5) & 15) - 1, date & 31,
      time >> 11, (time >> 5) & 63, (time & 31) * 2);
  }
  const originalTimeZone = process.env.TZ;
  try {
    process.env.TZ = 'Asia/Tokyo';
    const before = Date.now();
    const localTimeZip = await writeWithZipSettings({ forceLocalTime: true });
    const utcTimeZip = await writeWithZipSettings({ forceLocalTime: false });
    const after = Date.now();
    const offset = -new Date(before).getTimezoneOffset() * 60000;
    assert.equal(offset, 9 * 3600000);
    for (const time of [zipHeaderTime(localTimeZip) - offset, zipHeaderTime(utcTimeZip)]) {
      assert.ok(time >= before - 2000 && time <= after);
    }
    const localTimeBook = await new Workbook().xlsx.load(localTimeZip);
    assert.equal(localTimeBook.created.toISOString(), '2025-01-01T00:00:00.000Z');
  } finally {
    if (originalTimeZone === undefined) delete process.env.TZ;
    else process.env.TZ = originalTimeZone;
  }

  const archiveComment = '月次レポート 📊';
  const commentedArchive = await writeWithZipSettings({ comment: archiveComment });
  const commentBytes = Buffer.from(archiveComment);
  const commentRecordStart = commentedArchive.length - 22 - commentBytes.length;
  assert.equal(commentedArchive.readUInt32LE(commentRecordStart), 0x06054b50);
  assert.equal(commentedArchive.readUInt16LE(commentRecordStart + 20), commentBytes.length);
  assert.deepEqual(commentedArchive.subarray(commentRecordStart + 22), commentBytes);
  const maxZipComment = '表'.repeat(21845);
  const maxCommentArchive = await writeWithZipSettings({ comment: maxZipComment });
  const maxCommentRecordStart = maxCommentArchive.length - 22 - 65535;
  assert.equal(maxCommentArchive.readUInt32LE(maxCommentRecordStart), 0x06054b50);
  assert.equal(maxCommentArchive.readUInt16LE(maxCommentRecordStart + 20), 65535);
  assert.deepEqual(maxCommentArchive.subarray(maxCommentRecordStart + 22), Buffer.from(maxZipComment));
  assert.throws(() => new stream.xlsx.WorkbookWriter({ zip: { comment: '表'.repeat(21846) } }),
    { code: 'INVALID_VALUE' });
  assert.throws(() => new stream.xlsx.WorkbookWriter({ zip: { comment: maxZipComment + 'x' } }),
    { code: 'INVALID_VALUE', message: /65535 UTF-8 bytes/ });
  assert.throws(() => new stream.xlsx.WorkbookWriter({ zip: { comment: maxZipComment + 'x', zlib: { level: 10 } } }),
    { code: 'INVALID_VALUE', message: /ZIP level/ });
  assert.throws(() => new stream.xlsx.WorkbookWriter({ zip: { comment: '\uD800' } }),
    { code: 'INVALID_VALUE' });

  const forcedZip64 = await writeWithZipSettings({ forceZip64: true, store: true });
  const zip64Locator = forcedZip64.length - 42;
  assert.equal(forcedZip64.readUInt32LE(zip64Locator), 0x07064b50);
  const zip64End = Number(forcedZip64.readBigUInt64LE(zip64Locator + 8));
  assert.equal(forcedZip64.readUInt32LE(zip64End), 0x06064b50);
  assert.equal(forcedZip64.readUInt32LE(zip64Locator + 20), 0x06054b50);
  const zip64Reader = new stream.xlsx.WorkbookReader(Readable.from([forcedZip64]));
  let zip64LastRow;
  for await (const sheet of zip64Reader) {
    for await (const row of sheet) zip64LastRow = row;
  }
  assert.equal(zip64LastRow.number, 256);
  assert.equal(zip64LastRow.getCell(2).value, 255);
  assert.throws(() => new stream.xlsx.WorkbookWriter({ zip: { forceZip64: 1 } }),
    { code: 'INVALID_VALUE' });

  const levelZeroArchive = await writeWithZipSettings({ zlib: { level: 0 } });
  const levelOneArchive = await writeWithZipSettings({ level: 1, zlib: { level: 0 } });
  const compressionSettings = new XlsxZipStream();
  try {
    assert.equal(compressionSettings.configure_compression(false, -1, 0), false);
    assert.equal(compressionSettings.configure_compression(false, 1, 0), true);
    assert.equal(compressionSettings.configure_compression(true, 9), false);
    assert.throws(() => compressionSettings.configure_compression(false, 10), /ZIP level/);
  } finally {
    compressionSettings.free();
  }
  const fallbackLevelArchive = await writeWithZipSettings({ level: -1, zlib: { level: 0 } });
  assert.deepEqual(fallbackLevelArchive, levelZeroArchive);
  assert.deepEqual(await writeWithZipSettings({ level: 0, zlib: { level: 9 } }), levelZeroArchive);
  const levelNineArchive = await writeWithZipSettings({ zlib: { level: 9 } });
  const explicitlyStoredArchive = await writeWithZipSettings({ store: true, level: 9 });
  const defaultArchive = await writeWithZipSettings({});
  const defaultLevelArchive = await writeWithZipSettings({ zlib: { level: -1 } });
  assert.deepEqual(explicitlyStoredArchive, levelZeroArchive);
  assert.deepEqual(defaultArchive, defaultLevelArchive);
  assert.equal(levelZeroArchive.readUInt16LE(8), 0);
  assert.equal(levelOneArchive.readUInt16LE(8), 8);
  assert.equal(levelNineArchive.readUInt16LE(8), 8);
  assert.ok(levelNineArchive.length < levelOneArchive.length);
  assert.throws(() => new stream.xlsx.WorkbookWriter({ zip: { zlib: { level: 10 } } }),
    { code: 'INVALID_VALUE' });

  const spoolWriter = new stream.xlsx.WorkbookWriter({ zip: { store: true } });
  const spoolOne = spoolWriter.addWorksheet('Spool One');
  const spoolTwo = spoolWriter.addWorksheet('Spool Two');
  spoolOne.addRow(['first row']).commit();
  const switchedRow = spoolTwo.addRow(['after switch']);
  const originalAppend = fs.default.appendFileSync;
  try {
    fs.default.appendFileSync = (path, bytes) => {
      originalAppend(path, bytes.subarray(0, 12));
      throw new Error('Injected partial spool write');
    };
    syncBuiltinESMExports();
    assert.throws(() => switchedRow.commit(), /Injected partial spool write/);
  } finally {
    fs.default.appendFileSync = originalAppend;
    syncBuiltinESMExports();
  }
  switchedRow.commit();
  const wideRow = '表'.repeat(24000);
  spoolOne.addRow([wideRow, wideRow]).commit();
  const bufferedRow = '編'.repeat(10000);
  spoolTwo.addRow([bufferedRow]).commit();
  spoolTwo.addRow([bufferedRow]).commit();
  spoolTwo.addRow([bufferedRow]).commit();
  await spoolWriter.commit();
  const spoolResult = await new Workbook().xlsx.load(spoolWriter.stream.read());
  assert.equal(spoolResult.getWorksheet('Spool One').getCell('A1').value, 'first row');
  assert.equal(spoolResult.getWorksheet('Spool One').getCell('B2').value, wideRow);
  assert.equal(spoolResult.getWorksheet('Spool Two').getCell('A1').value, 'after switch');
  assert.equal(spoolResult.getWorksheet('Spool Two').getCell('A4').value, bufferedRow);

  const hyperlinkWriter = new stream.xlsx.WorkbookWriter({ useSharedStrings: true });
  const streamedLinks = hyperlinkWriter.addWorksheet('Streamed Links');
  const external = {
    text: 'External link', hyperlink: 'https://example.com/?x=1&y=2', tooltip: 'Open page',
  };
  const internal = { text: 'Internal link', hyperlink: '#Target!B2' };
  const hyperlinkRow = streamedLinks.addRow([external, internal]);
  assert.equal(hyperlinkRow.getCell(1).hyperlink, external.hyperlink);
  hyperlinkRow.commit();
  streamedLinks.commit();
  hyperlinkWriter.addWorksheet('Target');
  await hyperlinkWriter.commit();
  const streamedLinkBytes = hyperlinkWriter.stream.read();
  const hyperlinkZip = new XlsxZipReader(streamedLinkBytes.length,
    (offset, length) => streamedLinkBytes.subarray(offset, offset + length));
  try {
    const sheetPath = 'xl/worksheets/sheet1.xml';
    const sheetXml = Buffer.from(hyperlinkZip.read_metadata(sheetPath, 65536)).toString();
    const relationshipXml = Buffer.from(hyperlinkZip.read_metadata(
      'xl/worksheets/_rels/sheet1.xml.rels', 65536)).toString();
    assert.match(sheetXml, /<hyperlink\b[^>]*\bref="A1"[^>]*\btooltip="Open page"/);
    assert.match(sheetXml, /<hyperlink\b[^>]*\bref="B1"[^>]*\blocation="Target!B2"/);
    assert.match(relationshipXml, /Target="https:\/\/example\.com\/\?x=1&amp;y=2"/);
  } finally {
    hyperlinkZip.free();
  }
  const hyperlinkResult = (await new Workbook().xlsx.load(streamedLinkBytes)).getWorksheet('Streamed Links');
  assert.deepEqual(hyperlinkResult.getCell(1, 1).value, external);
  assert.deepEqual(hyperlinkResult.getCell(1, 2).value, internal);
  const streamedLinkReader = new stream.xlsx.WorkbookReader(Readable.from([streamedLinkBytes]), { hyperlinks: 'cache' });
  const streamedLinkRows = [];
  for await (const sheet of streamedLinkReader) {
    for await (const row of sheet) streamedLinkRows.push(row.values.slice(1));
  }
  assert.deepEqual(streamedLinkRows, [[external, internal]]);
  const badLinkPath = join(directory, 'bad-stream-link.xlsx');
  const invalidLinkWriter = new stream.xlsx.WorkbookWriter({ filename: badLinkPath });
  invalidLinkWriter.addWorksheet('Links').addRow([{ text: 'Missing sheet', hyperlink: '#Missing!A1' }]).commit();
  await assert.rejects(invalidLinkWriter.commit(), { code: 'HYPERLINK_EDIT_REFUSED' });
  assert.equal(fs.existsSync(badLinkPath), false);

  const modelBook = new Workbook();
  modelBook.addWorksheet('Model').getCell('A1').value = 41;
  const inputModel = modelBook.model;
  const modelContainer = new ModelContainer(inputModel);
  assert.equal(modelContainer.model, inputModel);
  assert.equal(modelContainer.xlsx, modelContainer.xlsx);
  const modelFile = join(directory, 'container.xlsx');
  await modelContainer.xlsx.writeFile(modelFile);
  const readContainer = new ModelContainer(new Workbook().model);
  assert.equal(await readContainer.xlsx.readFile(modelFile), readContainer);
  assert.equal(readContainer.model.worksheets[0].name, 'Model');
  const modelSnapshot = readContainer.model;
  await assert.rejects(readContainer.xlsx.load(Buffer.from('invalid XLSX')));
  assert.equal(readContainer.model, modelSnapshot);
  modelBook.getWorksheet('Model').getCell('A1').value = 42;
  modelContainer.model = modelBook.model;
  const containerBytes = await modelContainer.xlsx.writeBuffer();
  assert.ok(Buffer.isBuffer(containerBytes));
  assert.equal(await readContainer.xlsx.read(Readable.from([containerBytes])), readContainer);
  assert.equal((await new Workbook().xlsx.load(containerBytes)).getWorksheet('Model').getCell('A1').value, 42);
  const modelChunks = [];
  await modelContainer.xlsx.write(new Writable({
    write(chunk, _encoding, callback) {
      modelChunks.push(chunk);
      callback();
    },
  }));
  assert.equal(await readContainer.xlsx.load(Buffer.concat(modelChunks).toString('base64'), { base64: true }), readContainer);
  assert.throws(() => modelContainer.xlsx.createInputStream(), /XLSX#createInputStream.*deprecated/);
  const forkWorkbook = new Workbook();
  const forkCell = forkWorkbook.addWorksheet('Candidate').getCell('A1');
  forkCell.value = { text: 'Keep the link', hyperlink: 'https://example.com/' };
  const beforeInvalidFork = await forkWorkbook.xlsx.writeBuffer();
  assert.throws(() => { forkCell.value = { formula: 'SUM((' }; }, { code: 'CELL_EDIT_REFUSED' });
  assert.equal(forkCell.hyperlink, 'https://example.com/');
  assert.deepEqual(await forkWorkbook.xlsx.writeBuffer(), beforeInvalidFork);
  forkCell.value = 42;
  assert.equal(forkCell.value, 42);
  const forkReloaded = await new Workbook().xlsx.load(await forkWorkbook.xlsx.writeBuffer());
  const reloadedForkCell = forkReloaded.getWorksheet('Candidate').getCell('A1');
  reloadedForkCell.font = { bold: true };
  const beforeInvalidForkStyle = await forkReloaded.xlsx.writeBuffer();
  assert.throws(() => { reloadedForkCell.font = { name: 'x'.repeat(32) }; }, { code: 'CELL_EDIT_REFUSED' });
  assert.equal(reloadedForkCell.font.bold, true);
  assert.deepEqual(await forkReloaded.xlsx.writeBuffer(), beforeInvalidForkStyle);
  const saveDocument = XlsxDocument.prototype.to_bytes;
  XlsxDocument.prototype.to_bytes = function () { throw new Error('Injected save failure'); };
  try {
    assert.throws(() => { reloadedForkCell.value = 43; }, {
      code: 'CELL_EDIT_REFUSED', message: 'Candidate!A1: Injected save failure',
    });
    assert.throws(() => { reloadedForkCell.font = { italic: true }; }, { code: 'WORKBOOK_SAVE_REFUSED' });
  } finally {
    XlsxDocument.prototype.to_bytes = saveDocument;
  }
  assert.equal(reloadedForkCell.value, 42);
  assert.equal(reloadedForkCell.font.bold, true);
  assert.deepEqual(await forkReloaded.xlsx.writeBuffer(), beforeInvalidForkStyle);
  forkReloaded.addWorksheet('After edit');
  assert.equal(forkReloaded.getWorksheet('Candidate').getCell('A1').font.bold, true);

  forkReloaded.getWorksheet('Candidate').getCell('B1').value = 'Shared text';
  const beforeExport = await forkReloaded.xlsx.writeBuffer();
  const sharedExport = await forkReloaded.xlsx.writeBuffer({ useStyles: false, useSharedStrings: true });
  const exported = SheetJS.read(sharedExport, { bookFiles: true });
  assert.equal(exported.Sheets.Candidate.B1.v, 'Shared text');
  assert.ok(exported.files['xl/sharedStrings.xml']);
  const reopenedExport = await new Workbook().xlsx.load(sharedExport);
  assert.equal(reopenedExport.getWorksheet('Candidate').getCell('A1').font?.bold ?? false, false);
  assert.deepEqual(await forkReloaded.xlsx.writeBuffer(), beforeExport);

  const richExportValue = { richText: [{ text: 'Review ', font: { bold: true } }, { text: 'status' }] };
  forkReloaded.getWorksheet('Candidate').getCell('C1').value = richExportValue;
  const sharedStyledBytes = await forkReloaded.xlsx.writeBuffer({ useSharedStrings: true });
  const sharedStyledBook = await new Workbook().xlsx.load(sharedStyledBytes);
  const inlineBytes = await sharedStyledBook.xlsx.writeBuffer({ useStyles: false, useSharedStrings: false });
  const inlineSheetJs = SheetJS.read(inlineBytes, { bookFiles: true });
  assert.equal(inlineSheetJs.files['xl/sharedStrings.xml'], undefined);
  assert.equal(inlineSheetJs.Sheets.Candidate.B1.v, 'Shared text');
  const inlineBook = await new Workbook().xlsx.load(inlineBytes);
  const inlineSheet = inlineBook.getWorksheet('Candidate');
  assert.equal(inlineSheet.getCell('A1').font?.bold ?? false, false);
  assert.equal(inlineSheet.getCell('C1').value.richText[0].font.bold, true);
  assert.equal(inlineSheet.getCell('C1').text, 'Review status');
  assert.deepEqual(await sharedStyledBook.xlsx.writeBuffer(), sharedStyledBytes);

  const modelWorkbook = new Workbook();
  const modelSheet = modelWorkbook.addWorksheet('Model');
  const modelCell = modelSheet.getCell('A1');
  modelCell.value = { formula: '1=0', result: false };
  modelCell.note = 'Review';
  const model = modelCell.model;
  assert.equal(model.formula, '1=0');
  assert.equal(model.result, false);
  assert.deepEqual(model.comment.note.texts, [{ text: 'Review' }]);
  assert.deepEqual(model.style, modelCell.style);
  modelSheet.mergeCells('A2:B2');
  assert.equal(modelSheet.getCell('B2').model.master, 'A2');
  assert.equal(modelSheet.getRow(3).model, null);
  const modelRow = modelSheet.getRow(4);
  modelRow.height = 20;
  assert.deepEqual(modelRow.model.cells, []);
  modelRow.getCell(2);
  modelRow.getCell(5).value = 'sparse';
  const rowModel = modelRow.model;
  assert.deepEqual([rowModel.min, rowModel.max, rowModel.height], [2, 5, 20]);
  assert.deepEqual(rowModel.cells.map(({ address }) => address), ['B4', 'E4']);
  assert.equal(modelRow.findCell(3), undefined);
  const assigned = modelSheet.getCell('C6');
  assigned.model = {
    ...model, address: 'C6', style: { numFmt: '0.00' },
  };
  assert.equal(assigned.model.result, false);
  assert.equal(assigned.numFmt, '0.00');
  assert.equal(assigned.note, 'Review');
  assigned.model = { address: 'C6', type: ValueType.Date, value: new Date('2024-02-03T00:00:00Z') };
  assert.equal(assigned.value.toISOString(), '2024-02-03T00:00:00.000Z');
  const beforeInvalidModel = await modelWorkbook.xlsx.writeBuffer();
  assert.throws(() => {
    assigned.model = { address: 'C6', type: ValueType.Formula, formula: 'SUM((', style: { numFmt: '0%' } };
  }, { code: 'CELL_EDIT_REFUSED' });
  assert.deepEqual(await modelWorkbook.xlsx.writeBuffer(), beforeInvalidModel);
  const modelBytes = await modelWorkbook.xlsx.writeBuffer();
  const reopenedModel = await new Workbook().xlsx.load(modelBytes);
  assert.equal(reopenedModel.getWorksheet('Model').getCell('A1').model.result, false);
  const workbookModel = reopenedModel.model;
  assert.equal(workbookModel.creator, 'Unknown');
  assert.deepEqual(workbookModel.worksheets.map(sheet => sheet.name), ['Model']);
  assert.deepEqual(workbookModel.sheets.map(sheet => sheet.name), ['Model']);
  assert.match(workbookModel.themes.theme1, /<a:theme\b/);
  assert.deepEqual(await reopenedModel.xlsx.writeBuffer(), modelBytes);
  const clearedThemeModel = await new Workbook().xlsx.load(modelBytes);
  clearedThemeModel.clearThemes();
  assert.equal(clearedThemeModel.model.themes, undefined);
  assert.deepEqual(reopenedModel.getWorksheet('Model').getRow(4).model.cells.map(({ address }) => address), ['E4']);
  assert.deepEqual(await reopenedModel.xlsx.writeBuffer(), modelBytes);
  const validationBook = new Workbook();
  const validationSheet = validationBook.addWorksheet('Validate');
  validationSheet.dataValidations.add('A1:A3', { type: 'list', formulae: ['"yes,no"'] });
  assert.deepEqual(Object.keys(validationSheet.dataValidations.model), ['A1:A3']);
  const validationBytes = await validationBook.xlsx.writeBuffer();
  const reopenedValidationBook = await new Workbook().xlsx.load(validationBytes);
  const validationModel = reopenedValidationBook.getWorksheet('Validate').dataValidations.model;
  assert.deepEqual(Object.keys(validationModel), ['A1', 'A2', 'A3']);
  assert.deepEqual(validationModel.A2.formulae, ['"yes,no"']);
  assert.equal(validationModel.A1, validationModel.A2);
  assert.deepEqual(await reopenedValidationBook.xlsx.writeBuffer(), validationBytes);
  const worksheetModel = reopenedValidationBook.getWorksheet('Validate').model;
  assert.deepEqual(Object.keys(worksheetModel.dataValidations), ['A1', 'A2', 'A3']);
  assert.deepEqual(worksheetModel.rows, []);
  assert.deepEqual(worksheetModel.dimensions.model, { top: 0, left: 0, bottom: 0, right: 0 });
  assert.deepEqual(await reopenedValidationBook.xlsx.writeBuffer(), validationBytes);
  const validations = reopenedValidationBook.getWorksheet('Validate').dataValidations;
  validations.model = {
    'B1:B2': { type: 'whole', operator: 'between', formulae: [1, 3] },
    D3: { type: 'list', formulae: ['"a,b"'] },
  };
  assert.deepEqual(Object.keys(validations.model), ['B1:B2', 'D3']);
  assert.equal(validations.find('A1:A3'), undefined);
  const replacementValidationBytes = await reopenedValidationBook.xlsx.writeBuffer();
  assert.throws(() => {
    validations.model = {
      'A1:A3': { type: 'whole', formulae: [1, 3] },
      A2: { type: 'whole', formulae: [1, 4] },
    };
  }, { code: 'WORKSHEET_EDIT_REFUSED' });
  assert.deepEqual(await reopenedValidationBook.xlsx.writeBuffer(), replacementValidationBytes);
  const persistedValidations = (await new Workbook().xlsx.load(replacementValidationBytes))
    .getWorksheet('Validate').dataValidations;
  assert.deepEqual(Object.keys(persistedValidations.model), ['B1', 'B2', 'D3']);
  validations.model = {};
  assert.deepEqual(validations.model, {});
  const validationArchive = SheetJS.CFB.read(validationBytes, { type: 'array' });
  const validationPart = '/xl/worksheets/sheet1.xml';
  const validationXml = Buffer.from(SheetJS.CFB.find(validationArchive, validationPart).content).toString('utf8');
  const x14Validation = '<extLst><ext uri="{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}" ' +
    'xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:dataValidations count="1" ' +
    'xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:dataValidation type="list">' +
    '<x14:formula1><xm:f>"x,y"</xm:f></x14:formula1><xm:sqref>D1:D3</xm:sqref>' +
    '</x14:dataValidation></x14:dataValidations></ext></extLst>';
  assert.ok(SheetJS.CFB.utils.cfb_del(validationArchive, validationPart));
  SheetJS.CFB.utils.cfb_add(validationArchive, validationPart.slice(1),
    Buffer.from(validationXml.replace('</worksheet>', x14Validation + '</worksheet>')));
  const x14Bytes = SheetJS.CFB.write(validationArchive, { type: 'array', fileType: 'zip' });
  const x14Book = await new Workbook().xlsx.load(Uint8Array.from(x14Bytes));
  const x14Sheet = x14Book.getWorksheet('Validate');
  assert.deepEqual(Object.keys(x14Sheet.dataValidations.model), ['A1', 'A2', 'A3']);
  x14Sheet.dataValidations.model = { B1: { type: 'list', formulae: ['"b,c"'] } };
  const updatedX14Bytes = await x14Book.xlsx.writeBuffer();
  const updatedX14Archive = SheetJS.CFB.read(Uint8Array.from(updatedX14Bytes), { type: 'array' });
  const updatedX14Xml = Buffer.from(SheetJS.CFB.find(updatedX14Archive, validationPart).content).toString('utf8');
  assert.ok(updatedX14Xml.includes(x14Validation));
  assert.throws(() => {
    x14Sheet.dataValidations.model = { D2: { type: 'list', formulae: ['"z"'] } };
  }, { code: 'WORKSHEET_EDIT_REFUSED' });
  assert.deepEqual(await x14Book.xlsx.writeBuffer(), updatedX14Bytes);
  modelSheet.columns = [{ width: 12 }, { width: 12 }];
  const sheetModel = modelSheet.model;
  assert.deepEqual(sheetModel.merges, ['A2:B2']);
  assert.deepEqual(sheetModel.rows.map(row => row.number), [1, 2, 4, 6]);
  assert.deepEqual(sheetModel.cols.map(({ min, max, width }) => [min, max, width]), [[1, 2, 12]]);
  const withColumnModelBytes = await modelWorkbook.xlsx.writeBuffer();
  const withColumnModel = await new Workbook().xlsx.load(withColumnModelBytes);
  const reloadedColumns = withColumnModel.getWorksheet('Model').model.cols;
  assert.deepEqual(reloadedColumns.map(({ min, max, width }) => [min, max, width]), [[1, 2, 12]]);
  assert.deepEqual(await withColumnModel.xlsx.writeBuffer(), withColumnModelBytes);
  const ignoredColumnModel = await new Workbook().xlsx.load(withColumnModelBytes, { ignoreNodes: ['cols'] });
  assert.equal(ignoredColumnModel.getWorksheet('Model').model.cols, undefined);
  assert.deepEqual(sheetModel.dimensions.model, { top: 1, left: 1, bottom: 6, right: 5 });
  const replacementBook = new Workbook();
  const replacementSheet = replacementBook.addWorksheet('Replacement');
  const replacementRow = replacementSheet.getRow(3);
  replacementRow.height = 22;
  replacementRow.getCell(1).value = 'remove';
  replacementRow.getCell(1).note = 'remove note';
  replacementRow.getCell(3).value = 'replace';
  replacementRow.model = {
    number: 3, hidden: true, outlineLevel: 1,
    cells: [
      { address: 'C3', type: ValueType.Number, value: 42, style: { numFmt: '0.00' } },
      { type: ValueType.Boolean, value: false },
    ],
  };
  assert.equal(replacementRow.height, undefined);
  assert.equal(replacementRow.hidden, true);
  assert.deepEqual(replacementRow.model.cells.map(({ address }) => address), ['C3', 'D3']);
  assert.equal(replacementSheet.getCell('A3').note, undefined);
  assert.equal(replacementSheet.getCell('C3').numFmt, '0.00');
  assert.equal(replacementSheet.getCell('D3').value, false);
  const beforeBadRow = await replacementBook.xlsx.writeBuffer();
  assert.throws(() => {
    replacementRow.model = {
      number: 3, height: 28,
      cells: [
        { address: 'C3', type: ValueType.String, value: 'invalidates old value' },
        { address: 'D3', type: ValueType.Formula, formula: 'SUM((' },
      ],
    };
  }, { code: 'ROW_EDIT_REFUSED' });
  assert.deepEqual(await replacementBook.xlsx.writeBuffer(), beforeBadRow);
  const restoredRow = (await new Workbook().xlsx.load(beforeBadRow)).getWorksheet('Replacement').getRow(3);
  assert.deepEqual(restoredRow.model.cells.map(({ address }) => address), ['C3', 'D3']);
  assert.equal(restoredRow.getCell(3).value, 42);
  assert.equal(restoredRow.getCell(4).value, false);
  replacementSheet.addConditionalFormatting({
    ref: 'A1', rules: [{ type: 'expression', formulae: ['A1>0'] }],
  });
  replacementSheet.model = {
    name: 'Replacement',
    rows: [{ number: 2, cells: [{ address: 'B2', type: ValueType.Number, value: 7 }] }],
    pageSetup: { orientation: 'landscape' }, headerFooter: { oddHeader: '&CModel' },
    conditionalFormattings: [
      { ref: 'B2', rules: [{ type: 'expression', formulae: ['B2>0'] }] },
      { ref: 'B2', rules: [{ type: 'expression', formulae: ['B2<10'] }] },
    ],
    dataValidations: { B2: { type: 'whole', operator: 'between', formulae: [1, 10] } },
  };
  assert.deepEqual(replacementSheet.model.rows.map(row => row.number), [2]);
  assert.equal(replacementSheet.getCell('B2').value, 7);
  assert.equal(replacementSheet.pageSetup.orientation, 'landscape');
  assert.equal(replacementSheet.headerFooter.oddHeader, '&CModel');
  assert.deepEqual(replacementSheet.dataValidations.model.B2.formulae, [1, 10]);
  await replacementSheet.protect('password', { spinCount: 1, formatRows: true, selectLockedCells: false });
  const storedProtection = replacementSheet.model.sheetProtection;
  assert.equal(storedProtection.algorithmName, 'SHA-512');
  assert.equal(storedProtection.spinCount, 1);
  assert.equal(storedProtection.formatRows, true);
  assert.equal(storedProtection.selectLockedCells, false);
  const protectionTarget = replacementBook.addWorksheet('TransferredProtection');
  protectionTarget.model = { rows: [], sheetProtection: storedProtection };
  assert.deepEqual(protectionTarget.sheetProtection, storedProtection);
  const protectedBytes = await replacementBook.xlsx.writeBuffer();
  const reloadedProtection = (await new Workbook().xlsx.load(protectedBytes))
    .getWorksheet('TransferredProtection');
  assert.deepEqual(reloadedProtection.sheetProtection, storedProtection);
  assert.throws(() => {
    replacementSheet.model = {
      rows: [],
      sheetProtection: { ...storedProtection, hashValue: '\u0000' },
    };
  }, { code: 'WORKSHEET_EDIT_REFUSED' });
  assert.deepEqual(await replacementBook.xlsx.writeBuffer(), protectedBytes);
  protectionTarget.model = { rows: [], sheetProtection: null };
  assert.equal(protectionTarget.sheetProtection, null);
  protectionTarget.model = { rows: [], sheetProtection: { sheet: false, formatRows: true, objects: false } };
  assert.equal(protectionTarget.sheetProtection.sheet, undefined);
  assert.equal(protectionTarget.sheetProtection.formatRows, true);
  assert.equal(protectionTarget.sheetProtection.objects, undefined);
  replacementSheet.getRow(6).addPageBreak();
  const pageBreaks = replacementSheet.rowBreaks;
  assert.equal(pageBreaks.length, 1);
  replacementSheet.model = {
    rows: [{ number: 2, cells: [{ address: 'B2', type: ValueType.Number, value: 7 }] }],
    rowBreaks: [],
    dimensions: protectionTarget.model.dimensions,
  };
  assert.deepEqual(replacementSheet.rowBreaks, pageBreaks);
  assert.deepEqual(replacementSheet.model.dimensions.model, { top: 2, left: 2, bottom: 2, right: 2 });
  const beforeBadWorksheetModel = await replacementBook.xlsx.writeBuffer();
  const conditionFormulae = sheet => sheet.conditionalFormattings.map(({ rules }) => rules[0].formulae);
  assert.deepEqual(conditionFormulae(replacementSheet), [['B2>0'], ['B2<10']]);
  const reopenedConditions = (await new Workbook().xlsx.load(beforeBadWorksheetModel)).getWorksheet('Replacement');
  assert.deepEqual(conditionFormulae(reopenedConditions), [['B2>0'], ['B2<10']]);
  assert.throws(() => {
    replacementSheet.model = {
      rows: [],
      conditionalFormattings: [{ ref: 'invalid', rules: [{ type: 'expression', formulae: ['B2>0'] }] }],
    };
  }, { code: 'CONDITIONAL_FORMAT_EDIT_REFUSED' });
  assert.deepEqual(await replacementBook.xlsx.writeBuffer(), beforeBadWorksheetModel);
  assert.throws(() => {
    replacementSheet.model = { rows: [
      { number: 2, cells: [{ address: 'B2', type: ValueType.Number, value: 9 }] },
      { number: 4, cells: [{ address: 'A4', type: ValueType.Formula, formula: 'SUM((' }] },
    ] };
  }, { code: 'ROW_EDIT_REFUSED' });
  assert.deepEqual(await replacementBook.xlsx.writeBuffer(), beforeBadWorksheetModel);
  assert.throws(() => {
    replacementSheet.model = {
      rows: [{ number: 2, cells: [{ address: 'B2', type: ValueType.Number, value: 11 }] }],
      dataValidations: {
        'A1:A2': { type: 'list', formulae: ['"x"'] },
        A2: { type: 'list', formulae: ['"y"'] },
      },
    };
  }, { code: 'WORKSHEET_EDIT_REFUSED' });
  assert.equal(replacementSheet.getCell('B2').value, 7);
  assert.deepEqual(await replacementBook.xlsx.writeBuffer(), beforeBadWorksheetModel);
  replacementSheet.model = {
    rows: [{ number: 2, cells: [{ address: 'B2', type: ValueType.Number, value: 7 }] }],
    mergeCells: ['B2:C2'],
    conditionalFormattings: [],
  };
  assert.deepEqual(replacementSheet.conditionalFormattings, []);
  assert.equal(replacementSheet.getCell('C2').master.address, 'B2');
  replacementSheet.model = {
    rows: [{ number: 2, cells: [
      { address: 'C2', type: ValueType.Number, value: 8 },
      { address: 'D2', type: ValueType.Number, value: 9 },
    ] }],
    merges: ['D2:E2'],
  };
  assert.deepEqual(replacementSheet.model.merges, ['D2:E2']);
  assert.equal(replacementSheet.getCell('C2').value, 8);
  assert.equal(replacementSheet.getCell('E2').master.address, 'D2');
  const beforeOverlappingMerges = await replacementBook.xlsx.writeBuffer();
  assert.throws(() => {
    replacementSheet.model = {
      rows: [{ number: 2, cells: [{ address: 'B2', type: ValueType.Number, value: 1 }] }],
      merges: ['B2:C2', 'C2:D2'],
    };
  }, { code: 'MERGE_EDIT_REFUSED' });
  assert.deepEqual(await replacementBook.xlsx.writeBuffer(), beforeOverlappingMerges);
  replacementSheet.columns = [{ width: 21, hidden: true }];
  replacementSheet.model = {
    rows: [{ number: 2, cells: [{ address: 'D2', type: ValueType.Number, value: 9 }] }],
    cols: [{ min: 3, max: 5, width: 18, hidden: true, style: { numFmt: '0.00' } }],
    properties: {
      defaultRowHeight: 27, defaultColWidth: 16, dyDescent: 45, outlineLevelRow: 2,
      outlineProperties: { summaryBelow: false }, tabColor: { argb: 'FF339966' },
    },
  };
  assert.equal(replacementSheet.getColumn(1).hidden, false);
  assert.equal(replacementSheet.getColumn(1).width, undefined);
  assert.equal(replacementSheet.getColumn(5).numFmt, '0.00');
  assert.equal(replacementSheet.properties.defaultRowHeight, 27);
  assert.equal(replacementSheet.properties.defaultColWidth, 16);
  assert.equal(replacementSheet.properties.dyDescent, 45);
  assert.equal(replacementSheet.properties.outlineLevelRow, 2);
  assert.equal(replacementSheet.properties.outlineProperties.summaryBelow, false);
  assert.deepEqual(replacementSheet.properties.tabColor, { argb: 'FF339966' });
  const automaticColorBook = new Workbook();
  const automaticColorSheet = automaticColorBook.addWorksheet('Automatic tab color');
  automaticColorSheet.properties.tabColor = {};
  assert.equal(automaticColorSheet.properties.tabColor, undefined);
  assert.deepEqual(automaticColorSheet.model.properties.tabColor, {});
  const automaticColorCopy = new Workbook();
  automaticColorCopy.model = automaticColorBook.model;
  const automaticColorBytes = await automaticColorCopy.xlsx.writeBuffer();
  const automaticColorReloaded = (await new Workbook().xlsx.load(automaticColorBytes)).worksheets[0];
  assert.deepEqual(automaticColorReloaded.model.properties.tabColor, {});
  assert.equal(automaticColorReloaded.properties.tabColor, undefined);
  assert.deepEqual(replacementSheet.model.cols.map(({ min, max, width, hidden }) => [min, max, width, hidden]),
    [[3, 5, 18, true]]);
  const beforeBadColumns = await replacementBook.xlsx.writeBuffer();
  assert.throws(() => {
    replacementSheet.model = {
      rows: [{ number: 8, cells: [{ address: 'A8', type: ValueType.Formula, formula: 'SUM((' }] }],
      cols: [{ min: 1, max: 1, width: 11 }],
    };
  }, { code: 'ROW_EDIT_REFUSED' });
  assert.deepEqual(await replacementBook.xlsx.writeBuffer(), beforeBadColumns);
  const reloadedColumnRanges = (await new Workbook().xlsx.load(beforeBadColumns)).getWorksheet('Replacement').model.cols;
  assert.deepEqual(reloadedColumnRanges.map(({ min, max, width, hidden }) => [min, max, width, hidden]),
    [[3, 5, 18, true]]);
  const dependentSheet = replacementBook.addWorksheet('Dependent');
  dependentSheet.getCell('A1').value = { formula: 'Replacement!D2' };
  const renamedHandle = replacementSheet.getCell('D2');
  const beforeInvalidRename = await replacementBook.xlsx.writeBuffer();
  assert.throws(() => {
    replacementSheet.model = { name: 'Dependent', rows: [] };
  }, { code: 'DUPLICATE_SHEET_NAME' });
  assert.deepEqual(await replacementBook.xlsx.writeBuffer(), beforeInvalidRename);
  replacementSheet.model = {
    name: 'Updated',
    rows: [{ number: 2, cells: [{ address: 'D2', type: ValueType.Number, value: 15 }] }],
  };
  assert.equal(replacementBook.getWorksheet('Replacement'), undefined);
  assert.equal(replacementBook.getWorksheet('Updated'), replacementSheet);
  assert.equal(replacementSheet.getCell('D2'), renamedHandle);
  assert.equal(renamedHandle.value, 15);
  assert.match(dependentSheet.getCell('A1').formula, /Updated.*D2/);
  const stateWorkbook = new Workbook();
  const visibleSheet = stateWorkbook.addWorksheet('Visible');
  const stateSheet = stateWorkbook.addWorksheet('State', { state: 'veryHidden' });
  assert.equal(stateSheet.state, 'veryHidden');
  const legacyTabWriter = new stream.xlsx.WorkbookWriter();
  const legacyTab = legacyTabWriter.addWorksheet('Legacy', {
    tabColor: { argb: 'FF123456' },
    properties: { defaultRowHeight: 24 },
  });
  assert.deepEqual(legacyTab.properties.tabColor, { argb: 'FF123456' });
  assert.equal(legacyTab.properties.defaultRowHeight, 24);
  const preferredTab = legacyTabWriter.addWorksheet('Preferred', {
    tabColor: { argb: 'FF123456' },
    properties: { tabColor: { argb: 'FF654321' } },
  });
  assert.deepEqual(preferredTab.properties.tabColor, { argb: 'FF654321' });
  await legacyTabWriter.commit();
  const legacyTabRead = await new Workbook().xlsx.load(legacyTabWriter.stream.read());
  assert.deepEqual(legacyTabRead.getWorksheet('Legacy').properties.tabColor, { argb: 'FF123456' });
  assert.deepEqual(legacyTabRead.getWorksheet('Preferred').properties.tabColor, { argb: 'FF654321' });

  const streamingStateBook = new stream.xlsx.WorkbookWriter();
  const streamingVisible = streamingStateBook.addWorksheet('Visible', { state: undefined });
  assert.equal(streamingVisible.state, 'visible');
  assert.throws(() => streamingStateBook.addWorksheet('Invalid', { state: null }),
    { code: 'UNSUPPORTED_OPTIONS', message: 'Invalid worksheet state' });
  assert.equal(streamingStateBook.getWorksheet('Invalid'), undefined);
  const streamingHidden = streamingStateBook.addWorksheet('Hidden', { state: 'veryHidden' });
  assert.equal(streamingHidden.state, 'veryHidden');
  await streamingStateBook.commit();
  const streamingStateRead = await new Workbook().xlsx.load(streamingStateBook.stream.read());
  assert.equal(streamingStateRead.getWorksheet('Hidden').state, 'veryHidden');

  const beforeInvalidState = await stateWorkbook.xlsx.writeBuffer();
  assert.throws(() => stateWorkbook.addWorksheet('Invalid', { state: 'hidden-ish' }),
    { code: 'UNSUPPORTED_OPTIONS', message: 'Invalid worksheet state' });
  assert.throws(() => { visibleSheet.state = 'hidden-ish'; },
    { code: 'INVALID_VALUE', message: 'Invalid worksheet state' });
  assert.deepEqual(await stateWorkbook.xlsx.writeBuffer(), beforeInvalidState);
  const stateProjection = SheetJS.read(beforeInvalidState);
  assert.equal(stateProjection.Workbook.Sheets[1].Hidden, SheetJS.utils.consts.SHEET_VERY_HIDDEN);
  SheetJS.utils.book_set_sheet_visibility(stateProjection, 'State', SheetJS.utils.consts.SHEET_HIDDEN);
  const hiddenWorkbook = await new Workbook().xlsx.load(SheetJS.write(stateProjection, { type: 'array' }));
  assert.equal(hiddenWorkbook.getWorksheet('State').state, 'hidden');
  assert.equal(hiddenWorkbook.getWorksheet('Visible').state, 'visible');
  const printWorkbook = new Workbook();
  const printSheet = printWorkbook.addWorksheet('Print', { pageSetup: { fitToWidth: 2 } });
  assert.equal(printSheet.pageSetup.fitToPage, true);
  printSheet.pageSetup = { cellComments: 'None' };
  assert.equal(printSheet.pageSetup.cellComments, 'None');
  printSheet.pageSetup = {
    orientation: 'landscape', paperSize: 9, scale: 80, fitToHeight: 3, cellComments: 'asDisplyed',
  };
  assert.equal(printSheet.pageSetup.cellComments, 'asDisplayed');
  const printBytes = await printWorkbook.xlsx.writeBuffer();
  const ignoredPage = await new Workbook().xlsx.load(printBytes, { ignoreNodes: ['pageSetup'] });
  const page = ignoredPage.getWorksheet('Print').pageSetup;
  assert.deepEqual([page.orientation, page.paperSize, page.scale, page.fitToWidth, page.fitToHeight, page.fitToPage],
    ['portrait', undefined, 100, 1, 1, true]);
  const ignoredProperties = await new Workbook().xlsx.load(printBytes, { ignoreNodes: ['sheetPr'] });
  assert.equal(ignoredProperties.getWorksheet('Print').pageSetup.fitToPage, false);
  const matrixCells = [
    [undefined, 1, 1], ['Plain', 2, 2], ['Plain', 1, 2],
    ['Plain', 1, 3], ['Plain', 2, 3], ['Plain', 3, 3],
    ["O'Brien", 3, 3],
  ].map(([sheetName, row, col]) => ({ sheetName, row, col }));
  const nameWorkbook = new Workbook();
  nameWorkbook.definedNames.add('Saved!A1', 'sample');
  const matrix = {
    forEach(callback) { matrixCells.forEach(callback); },
    map(callback) { return matrixCells.map(callback); },
    findCellAt(sheetName, row, col) {
      return matrixCells.find(cell => cell.sheetName === sheetName && cell.row === row && cell.col === col);
    },
  };
  assert.deepEqual(nameWorkbook.definedNames.getRanges('sample', matrix), {
    name: 'sample', ranges: ['$A$1', 'Plain!$B$1:$C$2', 'Plain!$C$3', "'O'Brien'!$C$3"],
  });
  assert.ok(matrixCells.every(cell => cell.mark === false));
  assert.deepEqual(nameWorkbook.definedNames.getRanges('sample'), {
    name: 'sample', ranges: ['Saved!$A$1'],
  });
  SheetJS.set_fs(fs);
  const source = join(directory, 'sheet.xlsx');
  const draft = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[23]]));
  const propertyBytes = SheetJS.write(draft, {
    type: 'array',
    Props: { Author: 'Editor', CreatedDate: new Date('2024-02-03T04:05:06Z') },
  });
  const writtenProps = SheetJS.read(propertyBytes, { bookProps: true }).Props;
  assert.equal(writtenProps.Author, 'Editor');
  assert.equal(new Date(writtenProps.CreatedDate).toISOString(), '2024-02-03T04:05:06.000Z');
  const invalidDocumentDate = new Date(NaN);
  invalidDocumentDate.getTime = () => 0;
  assert.throws(() => SheetJS.write(draft, { type: 'array', Props: { CreatedDate: invalidDocumentDate } }),
    { code: 'INVALID_VALUE', message: 'Expected a valid document property date' });
  assert.throws(() => SheetJS.write(draft, { type: 'array', Props: { constructor: 'unexpected' } }),
    { code: 'UNSUPPORTED_STRUCTURE', message: 'Unsupported document property' });
  for (const bookType of ['xlsx', 'xlsb']) {
    const propertyBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[23]]));
    propertyBook.Props = { Company: 'Original', CreatedDate: new Date('2024-02-03T04:05:06Z') };
    const result = SheetJS.write(propertyBook, {
      bookType, type: 'array',
      Props: { Title: 'New title', Company: 'Override', CreatedDate: new Date('2025-01-01T00:00:00Z') },
    });
    const resources = { bookFiles: true, bookVBA: true, bookDeps: true };
    const loaded = SheetJS.read(result, resources);
    const workbookPart = bookType === 'xlsb' ? 'xl/workbook.bin' : 'xl/workbook.xml';
    assert.ok(loaded.keys.includes(workbookPart));
    assert.ok(Object.hasOwn(loaded.files, workbookPart));
    assert.equal(loaded.Sheets.Sheet1.A1.v, 23);
    const withoutResources = SheetJS.read(result);
    assert.equal(Object.hasOwn(withoutResources, 'keys'), false);
    assert.equal(Object.hasOwn(withoutResources, 'files'), false);
    const namesOnly = SheetJS.read(result, { ...resources, bookSheets: true });
    assert.deepEqual(namesOnly.SheetNames, ['Sheet1']);
    for (const key of ['Sheets', 'keys', 'files', 'vbaraw', 'Deps']) {
      assert.equal(Object.hasOwn(namesOnly, key), false, bookType + ' bookSheets exposes ' + key);
    }
    const { Props } = SheetJS.read(result, { bookProps: true });
    assert.equal(Props.Title, 'New title');
    assert.equal(Props.Company, 'Original');
    assert.equal(new Date(Props.CreatedDate).toISOString(), '2024-02-03T04:05:06.000Z');
  }
  const xlsbBytes = SheetJS.write(draft, { bookType: 'xlsb', type: 'array' });
  const xlsbCatalogue = SheetJS.read(xlsbBytes, { bookProps: 1, bookSheets: 1 });
  assert.deepEqual(xlsbCatalogue.SheetNames, ['Sheet1']);
  assert.ok(xlsbCatalogue.Props);
  assert.equal(Object.hasOwn(xlsbCatalogue, 'Workbook'), false);
  assert.equal(Object.hasOwn(xlsbCatalogue, 'Sheets'), false);
  for (const dense of [true, 1, false, undefined]) {
    const sheet = SheetJS.read(xlsbBytes, { dense }).Sheets.Sheet1;
    const cell = dense ? sheet['!data'][0][0] : sheet.A1;
    assert.equal(cell.v, 23);
    assert.equal(Object.hasOwn(sheet, '!data'), Boolean(dense));
  }
  for (const cellText of [false, 0, null, undefined]) {
    assert.equal(SheetJS.read(xlsbBytes, { cellText }).Sheets.Sheet1.A1.w, cellText === false ? undefined : '23');
  }
  const styledXlsbSheet = SheetJS.utils.aoa_to_sheet([[{ t: 'n', v: 1, z: '0.00' }, { t: 'z' }]]);
  const styledXlsbBytes = SheetJS.write(SheetJS.utils.book_new(styledXlsbSheet), { bookType: 'xlsb', type: 'array' });
  for (const dense of [false, true]) {
    for (const [options, format, stub] of [
      [{}, undefined, undefined],
      [{ cellNF: true }, '0.00', undefined],
      [{ sheetStubs: true }, undefined, 'z'],
      [{ cellStyles: true, cellNF: false, sheetStubs: false }, '0.00', 'z'],
    ]) {
      const sheet = SheetJS.read(styledXlsbBytes, { ...options, dense }).Sheets.Sheet1;
      const [first, second] = dense ? sheet['!data'][0] : [sheet.A1, sheet.B1];
      assert.equal(first.z, format);
      assert.equal(second?.t, stub);
    }
  }
  const sharedXlsbBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([['unused']]), 'Unselected');
  SheetJS.utils.book_append_sheet(sharedXlsbBook,
    SheetJS.utils.aoa_to_sheet([['繰り返し', '繰り返し', '別の値']]), 'Selected');
  const sharedXlsbBytes = SheetJS.write(sharedXlsbBook, { bookType: 'xlsb', type: 'array', bookSST: true });
  const selectedShared = SheetJS.read(sharedXlsbBytes, { sheets: 'Selected' }).Sheets.Selected;
  assert.deepEqual([selectedShared.A1.v, selectedShared.B1.v, selectedShared.C1.v], ['繰り返し', '繰り返し', '別の値']);
  assert.equal(selectedShared.A1.r, selectedShared.B1.r);
  assert.notEqual(selectedShared.A1.r, selectedShared.C1.r);
  const selectorError = new Error('XLSB selector failed');
  const failingSelectors = Object.defineProperty([], '0', { get() { throw selectorError; } });
  const { take_lengths: takeLengths, take_units: takeUnits, free: freeSharedStrings } = XlsbSharedStrings.prototype;
  let sharedStringsFreed = 0;
  XlsbSharedStrings.prototype.take_lengths = () => assert.fail('XLSB transferred shared string lengths before selecting sheets');
  XlsbSharedStrings.prototype.take_units = () => assert.fail('XLSB transferred shared string units before selecting sheets');
  XlsbSharedStrings.prototype.free = function () {
    sharedStringsFreed++;
    return freeSharedStrings.call(this);
  };
  try {
    assert.deepEqual(Object.keys(SheetJS.read(sharedXlsbBytes, { sheets: [] }).Sheets), []);
    assert.equal(sharedStringsFreed, 1);
    assert.throws(() => SheetJS.read(sharedXlsbBytes, { sheets: failingSelectors }), error => error === selectorError);
    assert.equal(sharedStringsFreed, 2);
  } finally {
    XlsbSharedStrings.prototype.take_lengths = takeLengths;
    XlsbSharedStrings.prototype.take_units = takeUnits;
    XlsbSharedStrings.prototype.free = freeSharedStrings;
  }
  const brokenStringsArchive = SheetJS.CFB.read(new Uint8Array(sharedXlsbBytes), { type: 'array' });
  const sharedStringsPart = 'xl/sharedStrings.bin';
  assert.ok(SheetJS.CFB.utils.cfb_del(brokenStringsArchive, '/' + sharedStringsPart));
  SheetJS.CFB.utils.cfb_add(brokenStringsArchive, sharedStringsPart, Uint8Array.of(0x80));
  const brokenStringsBytes = SheetJS.CFB.write(brokenStringsArchive, { type: 'array', fileType: 'zip' });
  let selectorReads = 0;
  const selectors = Object.defineProperty([], '0', {
    get() { selectorReads++; throw new Error('Unexpected selector evaluation'); },
  });
  assert.throws(() => SheetJS.read(brokenStringsBytes, { sheets: selectors }), { code: 'INVALID_INPUT' });
  assert.equal(selectorReads, 0);
  assert.deepEqual(SheetJS.read(brokenStringsBytes, { sheets: selectors, bookSheets: true }).SheetNames,
    ['Unselected', 'Selected']);
  assert.equal(selectorReads, 0);
  const rowLimitBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[1], [2]]));
  SheetJS.utils.book_append_sheet(rowLimitBook, SheetJS.utils.aoa_to_sheet([[3], [4]]), 'Second');
  const rowLimitBytes = SheetJS.write(rowLimitBook, { bookType: 'xlsb', type: 'array' });
  const xlsxSelectionBytes = SheetJS.write(rowLimitBook, { bookType: 'xlsx', type: 'array' });
  for (const selectionBytes of [xlsxSelectionBytes, rowLimitBytes]) {
    for (const [sheets, selected] of [
      ['sEcOnD', ['Second']],
      [0, ['Sheet1']],
      [['SECOND', 0, 'missing', 0], ['Sheet1', 'Second']],
      [[], []],
    ]) {
      const workbook = SheetJS.read(selectionBytes, { sheets });
      assert.deepEqual(workbook.SheetNames, ['Sheet1', 'Second']);
      assert.deepEqual(Object.keys(workbook.Sheets), selected);
    }
  }
  const brokenSheetArchive = SheetJS.CFB.read(new Uint8Array(rowLimitBytes), { type: 'array' });
  const secondSheetPart = 'xl/worksheets/sheet2.bin';
  assert.ok(SheetJS.CFB.utils.cfb_del(brokenSheetArchive, '/' + secondSheetPart));
  SheetJS.CFB.utils.cfb_add(brokenSheetArchive, secondSheetPart, Uint8Array.of(0x80));
  const brokenSheetBytes = SheetJS.CFB.write(brokenSheetArchive, { type: 'array', fileType: 'zip' });
  const firstOnly = SheetJS.read(brokenSheetBytes, { sheets: 'Sheet1' });
  assert.deepEqual(firstOnly.SheetNames, ['Sheet1', 'Second']);
  assert.equal(firstOnly.Sheets.Sheet1.A2.v, 2);
  assert.equal(firstOnly.Sheets.Second, undefined);
  assert.deepEqual(SheetJS.read(brokenSheetBytes, { bookSheets: true }).SheetNames, firstOnly.SheetNames);
  assert.throws(() => SheetJS.read(brokenSheetBytes, { sheets: 'Second' }), { code: 'INVALID_INPUT' });
  let rowLimitConversions = 0;
  const sheetRows = { valueOf() { return ++rowLimitConversions; } };
  assert.deepEqual(Object.keys(SheetJS.read(rowLimitBytes, { sheets: [], sheetRows }).Sheets), []);
  assert.equal(rowLimitConversions, 0);
  const limitedSheets = SheetJS.read(rowLimitBytes, { sheetRows }).Sheets;
  assert.equal(rowLimitConversions, 2);
  assert.equal(limitedSheets.Sheet1.A2, undefined);
  assert.equal(limitedSheets.Second.A2.v, 4);
  const xlsbDateBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[{ t: 'n', v: 45293, z: 'm/d/yy' }]]));
  const xlsbDateBytes = SheetJS.write(xlsbDateBook, { bookType: 'xlsb', type: 'array' });
  const xlsbDateCell = SheetJS.read(xlsbDateBytes, { cellDates: 1, dateNF: 'yyyy-mm-dd' }).Sheets.Sheet1.A1;
  assert.equal(xlsbDateCell.v.toISOString(), '2024-01-02T00:00:00.000Z');
  assert.equal(xlsbDateCell.w, '2024-01-02');
  const xlfnBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[{ t: 'n', v: 23, f: '_xlfn.Total' }]]));
  xlfnBook.Workbook = { Names: [{ Name: '_xlfn.Total', Ref: '23' }] };
  const xlfnBytes = SheetJS.write(xlfnBook, { bookType: 'xlsb', type: 'array' });
  for (const xlfn of [undefined, false, true]) {
    assert.equal(SheetJS.read(xlfnBytes, { xlfn }).Sheets.Sheet1.A1.f, xlfn ? '_xlfn.Total' : 'Total');
  }
  const xlsbResourceArchive = SheetJS.CFB.read(new Uint8Array(xlsbBytes), { type: 'array' });
  const contentTypesPath = '/[Content_Types].xml';
  const resourceTypes = Buffer.from(SheetJS.CFB.find(xlsbResourceArchive, contentTypesPath).content).toString('utf8');
  assert.ok(SheetJS.CFB.utils.cfb_del(xlsbResourceArchive, contentTypesPath));
  SheetJS.CFB.utils.cfb_add(xlsbResourceArchive, contentTypesPath.slice(1), Buffer.from(resourceTypes.replace('</Types>',
    '<Override PartName="/xl/vbaProject.bin" ContentType="application/vnd.ms-office.vbaProject"/>' +
    '<Override PartName="/xl/calcChain.bin" ContentType="application/vnd.ms-excel.calcChain"/></Types>')));
  const vbaPayload = Uint8Array.of(0, 43, 99, 255);
  SheetJS.CFB.utils.cfb_add(xlsbResourceArchive, 'xl/vbaProject.bin', vbaPayload);
  // BrtBeginCalcChain, BrtCalcChainItem (sheet 2, D7, flags 10), BrtEndCalcChain.
  SheetJS.CFB.utils.cfb_add(xlsbResourceArchive, 'xl/calcChain.bin', Uint8Array.of(
    0x90, 4, 0, 0x3f, 13, 2, 0, 0, 0, 6, 0, 0, 0, 3, 0, 0, 0, 10, 0x91, 4, 0));
  const xlsbResources = SheetJS.CFB.write(xlsbResourceArchive, { type: 'array', fileType: 'zip' });
  const vbaOnly = SheetJS.read(xlsbResources, { bookVBA: true });
  assert.deepEqual(vbaOnly.vbaraw, vbaPayload);
  assert.deepEqual(vbaOnly.Deps, {});
  const depsOnly = SheetJS.read(xlsbResources, { bookDeps: true });
  assert.equal(depsOnly.vbaraw, undefined);
  assert.deepEqual(depsOnly.Deps, [{ i: 2, r: 'D7', l: '1', a: '1' }]);
  const xlsbNamesOnly = SheetJS.read(xlsbResources, { bookVBA: true, bookDeps: true, bookSheets: true });
  assert.equal(Object.hasOwn(xlsbNamesOnly, 'vbaraw'), false);
  assert.equal(Object.hasOwn(xlsbNamesOnly, 'Deps'), false);
  const biffBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[23]]));
  biffBook.Props = { CreatedDate: '2024-02-03T04:05:06Z', SheetNames: ['First', 'Second'], Worksheets: 99 };
  const biffBytes = SheetJS.write(biffBook, { type: 'array', bookType: 'biff8' });
  const biffProps = SheetJS.read(biffBytes, { bookProps: true }).Props;
  assert.equal(new Date(biffProps.CreatedDate).toISOString(), '2024-02-03T04:05:06.000Z');
  assert.deepEqual(biffProps.SheetNames, ['First', 'Second']);
  assert.equal(biffProps.Worksheets, 2);
  const htmlPath = join(directory, 'sheet.htm');
  SheetJS.writeFile(draft, htmlPath, { bookType: 'htm' });
  assert.equal(fs.readFileSync(htmlPath, 'utf8'), SheetJS.write(draft, { bookType: 'html', type: 'string' }));
  const socialcalc = join(directory, 'sheet.dat');
  await new Promise((resolve, reject) => {
    SheetJS.writeFileAsync(socialcalc, draft, { bookType: 'socialcalc' },
      error => error ? reject(error) : resolve());
  });
  assert.deepEqual(Uint8Array.from(fs.readFileSync(socialcalc)),
    SheetJS.write(draft, { bookType: 'eth', type: 'buffer' }));
  SheetJS.writeFile(draft, source);
  assert.equal(SheetJS.readFile(source).Sheets.Sheet1.A1.v, 23);
  assert.equal(SheetJS.readFile(source).Sheets.Sheet1.A1.w, '23');
  assert.equal(SheetJS.readFile(source, { cellText: false }).Sheets.Sheet1.A1.w, undefined);
  const projectedRead = SheetJS.readFile(source, Object.defineProperty({
    get type() { assert.fail('Overridden readFile type getter must not run'); },
  }, 'cellText', { value: false }));
  assert.equal(projectedRead.Sheets.Sheet1.A1.w, undefined);
  SheetJS.default.stream.set_readable(Readable);
  const codecSheet = SheetJS.utils.aoa_to_sheet([['Name'], ['café']]);
  const codepageCalls = [];
  SheetJS.set_cptable({ utils: {
    decode: (_page, bytes) => new TextDecoder('windows-1252').decode(bytes),
    encode(page, value, format) {
      codepageCalls.push([page, format]);
      return Buffer.from(value, page === 1200 ? 'utf16le' : 'latin1').toString('latin1');
    },
  } });
  try {
    const binaryText = SheetJS.utils.sheet_to_txt(codecSheet);
    assert.deepEqual(Buffer.from(binaryText, 'latin1'),
      Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from('Name\ncafé', 'utf16le')]));
    assert.deepEqual(codepageCalls, [[1200, 'str']]);
    assert.equal(SheetJS.utils.sheet_to_txt(codecSheet, { type: 'string' }), 'Name\ncafé');
    assert.equal(codepageCalls.length, 1);
    const encodedBook = SheetJS.utils.book_new();
    SheetJS.utils.book_append_sheet(encodedBook, codecSheet, 'Names');
    const dbf = SheetJS.write(encodedBook, { bookType: 'dbf', type: 'buffer', codepage: 1252 });
    const decodedBook = SheetJS.read(dbf, { type: 'buffer' });
    assert.equal(decodedBook.Sheets[decodedBook.SheetNames[0]].A2.v, 'café');
  } finally {
    SheetJS.set_cptable(undefined);
  }
  const nullableDbfSheet = SheetJS.utils.aoa_to_sheet([['Name', 'Day', 'Active'], ['Stored']]);
  nullableDbfSheet['!cols'] = [
    undefined, { DBF: { name: 'Day', type: 'D', len: 8 } }, { DBF: { name: 'Active', type: 'L', len: 1 } },
  ];
  const nullableDbfBook = SheetJS.utils.book_new();
  SheetJS.utils.book_append_sheet(nullableDbfBook, nullableDbfSheet, 'Missing values');
  const nullableDbfOptions = { bookType: 'dbf', type: 'buffer' };
  const nullableDbfBytes = Buffer.from(SheetJS.write(nullableDbfBook, nullableDbfOptions));
  assert.equal(nullableDbfBytes.toString('ascii', 32 + 32 + 11, 32 + 32 + 12), 'D');
  assert.equal(nullableDbfBytes.toString('ascii', 32 + 64 + 11, 32 + 64 + 12), 'L');
  const nullableDbfRead = SheetJS.read(nullableDbfBytes, { type: 'buffer', cellDates: true });
  const nullableReadSheet = nullableDbfRead.Sheets[nullableDbfRead.SheetNames[0]];
  assert.equal(nullableReadSheet.B2, undefined);
  assert.equal(nullableReadSheet.C2, undefined);
  assert.deepEqual(nullableReadSheet['!cols'].slice(1).map(({ DBF }) => [DBF.type, DBF.len]), [['D', 8], ['L', 1]]);
  assert.deepEqual(Buffer.from(SheetJS.write(nullableDbfRead, nullableDbfOptions)), nullableDbfBytes);

  const numericDbf = SheetJS.utils.aoa_to_sheet([['Amount'], [-9.5], [99.9]]);
  numericDbf['!cols'] = [{ DBF: { type: 'N', len: 4, dec: 1 } }];
  const numericDbfBook = SheetJS.utils.book_new();
  SheetJS.utils.book_append_sheet(numericDbfBook, numericDbf, 'Amounts');
  const numericDbfOptions = { bookType: 'dbf', type: 'buffer' };
  const numericDbfBytes = SheetJS.write(numericDbfBook, numericDbfOptions);
  const numericDbfRead = SheetJS.read(numericDbfBytes, { type: 'buffer' });
  assert.deepEqual(SheetJS.utils.sheet_to_json(numericDbfRead.Sheets[numericDbfRead.SheetNames[0]], { header: 1 }),
    [['Amount'], [-9.5], [99.9]]);
  numericDbf.A3.v = 99.99;
  assert.throws(() => SheetJS.write(numericDbfBook, numericDbfOptions),
    { code: 'INVALID_VALUE', message: 'DBF numeric value does not fit field Amount' });
  numericDbf.A3.v = 1e21;
  numericDbf['!cols'][0].DBF.len = 20;
  assert.throws(() => SheetJS.write(numericDbfBook, numericDbfOptions), { code: 'INVALID_VALUE' });
  numericDbf.A2.v = -1e21;
  numericDbf['!cols'][0].DBF.len = 25;
  const wideNumericDbf = Buffer.from(SheetJS.write(numericDbfBook, numericDbfOptions));
  const firstDbfRecord = wideNumericDbf.readUInt16LE(8);
  assert.equal(wideNumericDbf.toString('ascii', firstDbfRecord + 1, firstDbfRecord + 26),
    '-1000000000000000000000.0');
  const wideNumericDbfBook = SheetJS.read(wideNumericDbf, { type: 'buffer' });
  assert.deepEqual(SheetJS.utils.sheet_to_json(wideNumericDbfBook.Sheets[wideNumericDbfBook.SheetNames[0]], { header: 1 }),
    [['Amount'], [-1e21], [1e21]]);

  const savedDbfTimeZone = process.env.TZ;
  try {
    process.env.TZ = 'America/Los_Angeles';
    const dbfDateBook = SheetJS.utils.book_new();
    const dbfDateSheet = SheetJS.utils.aoa_to_sheet([['Day'], [new Date('2024-07-01T12:00:00Z')]], {
      UTC: true, cellDates: true,
    });
    SheetJS.utils.book_append_sheet(dbfDateBook, dbfDateSheet, 'Dates');
    const dateDbf = SheetJS.write(dbfDateBook, { bookType: 'dbf', type: 'buffer' });
    const readDbfDate = options => {
      const book = SheetJS.read(dateDbf, { type: 'buffer', ...options });
      return book.Sheets[book.SheetNames[0]].A2;
    };
    assert.equal(readDbfDate({ cellDates: true }).v.toISOString(), '2024-07-01T00:00:00.000Z');
    assert.equal(readDbfDate({ cellDates: true, UTC: true }).v.toISOString(), '2024-07-01T00:00:00.000Z');
    assert.equal(readDbfDate({ cellDates: false }).v, readDbfDate({ cellDates: false, UTC: true }).v);
    assert.equal(readDbfDate({ cellDates: true }).w, '20240701');
    const serialDbf = SheetJS.read(dateDbf, { type: 'buffer' });
    const serialDbfSheet = serialDbf.Sheets[serialDbf.SheetNames[0]];
    assert.equal(serialDbfSheet.A2.t, 'n');
    assert.deepEqual(Buffer.from(SheetJS.write(serialDbf, { bookType: 'dbf', type: 'buffer' })), Buffer.from(dateDbf));
    serialDbfSheet.A2.v = 0;
    serialDbf.Workbook = { WBProps: { date1904: true } };
    const base1904Dbf = Buffer.from(SheetJS.write(serialDbf, { bookType: 'dbf', type: 'buffer' }));
    const base1904Offset = base1904Dbf.readUInt16LE(8) + 1;
    assert.equal(base1904Dbf.toString('ascii', base1904Offset, base1904Offset + 8), '19040101');
    const base1904Book = SheetJS.read(base1904Dbf, { type: 'buffer', cellDates: true });
    assert.equal(base1904Book.Sheets[base1904Book.SheetNames[0]].A2.v.toISOString(), '1904-01-01T00:00:00.000Z');
    const shiftedDbfSheet = {
      '!ref': 'C3:D4',
      '!cols': [{ DBF: { name: 'Outside', type: 'C', len: 10 } }, undefined,
        { DBF: { name: 'ShiftedDay', type: 'D', len: 8 } }],
      C3: { t: 's', v: 'Day' }, D3: { t: 's', v: 'Label' },
      C4: { t: 'n', v: 0, z: 'yyyymmdd' }, D4: { t: 's', v: 'kept' },
    };
    const shiftedDbfBook = SheetJS.utils.book_new(shiftedDbfSheet, 'Shifted');
    shiftedDbfBook.Workbook = { WBProps: { date1904: true } };
    const shiftedDbfBytes = SheetJS.write(shiftedDbfBook, { bookType: 'dbf', type: 'buffer' });
    const shiftedReadBook = SheetJS.read(shiftedDbfBytes, { type: 'buffer', cellDates: true });
    const shiftedRead = shiftedReadBook.Sheets[shiftedReadBook.SheetNames[0]];
    assert.equal(shiftedRead['!ref'], 'A1:B2');
    assert.equal(shiftedRead.A1.v, 'ShiftedDay');
    assert.equal(shiftedRead.A2.v.toISOString(), '1904-01-01T00:00:00.000Z');
    assert.equal(shiftedRead.B2.v, 'kept');
    serialDbf.Workbook.WBProps.date1904 = false;
    serialDbfSheet.A2.v = 60;
    assert.throws(() => SheetJS.write(serialDbf, { bookType: 'dbf', type: 'buffer' }), { code: 'INVALID_VALUE' });
    const historicalDbf = Buffer.from(dateDbf);
    const dateOffset = historicalDbf.readUInt16LE(8) + 1;
    historicalDbf.write('00040229', dateOffset, 'ascii');
    const historicalBook = SheetJS.read(historicalDbf, { type: 'buffer', cellDates: true });
    assert.equal(historicalBook.Sheets[historicalBook.SheetNames[0]].A2.v.toISOString(), '0004-02-29T00:00:00.000Z');
    historicalDbf.write('20230229', dateOffset, 'ascii');
    const invalidDateBook = SheetJS.read(historicalDbf, { type: 'buffer', cellDates: true });
    assert.equal(invalidDateBook.Sheets[invalidDateBook.SheetNames[0]].A2, undefined);
    dbfDateSheet.A2.v = new Date('0004-02-29T12:00:00Z');
    const earlyDateDbf = Buffer.from(SheetJS.write(dbfDateBook, { bookType: 'dbf', type: 'buffer' }));
    assert.equal(earlyDateDbf.toString('ascii', dateOffset, dateOffset + 8), '00040229');
    for (const invalidDate of [new Date('+010000-01-02T12:00:00Z'), new Date(NaN)]) {
      dbfDateSheet.A2.v = invalidDate;
      assert.throws(() => SheetJS.write(dbfDateBook, { bookType: 'dbf', type: 'buffer' }), { code: 'INVALID_VALUE' });
    }
  } finally {
    if (savedDbfTimeZone === undefined) delete process.env.TZ;
    else process.env.TZ = savedDbfTimeZone;
  }

  const multibyteBook = SheetJS.utils.book_new();
  const dbfPrefix = 'x'.repeat(249);
  const multibyteSheet = SheetJS.utils.aoa_to_sheet([
    ['x' + 'あ'.repeat(5), 'あNext'], [dbfPrefix + 'あ', 'next'], ['あ', 'end'],
  ]);
  SheetJS.utils.book_append_sheet(multibyteBook, multibyteSheet, 'Multibyte');
  SheetJS.set_cptable({ utils: {
    decode: (_page, bytes) => new TextDecoder('shift_jis').decode(bytes),
    encode: (_page, value) => Uint8Array.from([...value].flatMap(character =>
      character === 'あ' ? [0x82, 0xa0] : [character.charCodeAt(0)])),
  } });
  try {
    const dbfBytes = SheetJS.write(multibyteBook, { bookType: 'dbf', type: 'buffer', codepage: 932 });
    const recordStart = Buffer.from(dbfBytes).readUInt16LE(8);
    assert.equal(dbfBytes[recordStart + 250], 0x20);
    assert.deepEqual(Array.from(dbfBytes.subarray(32, 43)), [0x82, 0xa0, 0x82, 0xa0, 0x82, 0xa0, 0x82, 0xa0, 0x82, 0xa0, 0]);
    const rereadDbf = SheetJS.read(dbfBytes, { type: 'buffer' });
    const readDbfSheet = rereadDbf.Sheets[rereadDbf.SheetNames[0]];
    assert.equal(readDbfSheet.A1.v, 'あ'.repeat(5));
    assert.equal(readDbfSheet.B1.v, 'あNext');
    assert.equal(readDbfSheet.A2.v, dbfPrefix);
    assert.equal(readDbfSheet.B2.v, 'next');
    assert.equal(readDbfSheet.A3.v, 'あ');
    const inheritedCodepageDbf = SheetJS.write(rereadDbf, { bookType: 'dbf', type: 'buffer' });
    assert.equal(inheritedCodepageDbf[29], 0x7b);
    const overrideCodepageDbf = SheetJS.write(rereadDbf, { bookType: 'dbf', type: 'buffer', codepage: 1252 });
    assert.equal(overrideCodepageDbf[29], 0x03);
    const inheritedDbfBook = SheetJS.read(inheritedCodepageDbf, { type: 'buffer' });
    const inheritedDbfSheet = inheritedDbfBook.Sheets[inheritedDbfBook.SheetNames[0]];
    assert.equal(inheritedDbfSheet.A1.v, 'あ'.repeat(5));
    assert.equal(inheritedDbfSheet.A3.v, 'あ');
    const collisionBook = SheetJS.utils.book_new();
    const collisionSheet = SheetJS.utils.aoa_to_sheet([
      ['x' + 'あ'.repeat(5), 'y' + 'あ'.repeat(5), 'explicit'], ['first', 'second', 'third'],
    ]);
    collisionSheet['!cols'] = [,, { DBF: { name: 'あ'.repeat(5), type: 'C', len: 8 } }];
    SheetJS.utils.book_append_sheet(collisionBook, collisionSheet, 'Names');
    const collisionBytes = SheetJS.write(collisionBook, { bookType: 'dbf', type: 'buffer', codepage: 932 });
    const decodedCollision = SheetJS.read(collisionBytes, { type: 'buffer' });
    const collisionRows = SheetJS.utils.sheet_to_json(decodedCollision.Sheets[decodedCollision.SheetNames[0]], { header: 1 });
    assert.deepEqual(collisionRows, [
      ['あ'.repeat(4) + '_0', 'あ'.repeat(4) + '_1', 'あ'.repeat(5)], ['first', 'second', 'third'],
    ]);
  } finally {
    SheetJS.set_cptable(undefined);
  }
  const csvChunks = [];
  await pipeline(SheetJS.stream.to_csv(draft.Sheets.Sheet1), new Writable({
    write(chunk, _encoding, callback) {
      csvChunks.push(chunk);
      callback();
    },
  }));
  assert.equal(Buffer.concat(csvChunks).toString(), '\uFEFF23');
  const csvRows = [];
  for await (const row of SheetJS.stream.to_json(draft.Sheets.Sheet1, { header: 1 })) csvRows.push(row);
  assert.deepEqual(csvRows, [[23]]);
  let streamDestroyCalls = 0;
  class TrackedSheetReadable extends Readable {
    _destroy(error, callback) {
      streamDestroyCalls += 1;
      super._destroy(error, callback);
    }
  }
  const generatorPrototype = Object.getPrototypeOf(Object.getPrototypeOf((function* () {})()));
  const closeIterator = generatorPrototype.return;
  let iteratorCloseCalls = 0;
  generatorPrototype.return = function (...args) {
    iteratorCloseCalls += 1;
    return closeIterator.apply(this, args);
  };
  SheetJS.stream.set_readable(TrackedSheetReadable);
  try {
    const largeSheet = { '!ref': 'A1:A200' };
    let accessed = 0;
    for (let row = 1; row <= 200; row += 1) {
      Object.defineProperty(largeSheet, 'A' + row, { get() { accessed += 1; return { t: 'n', v: row }; } });
    }
    const partial = SheetJS.stream.to_json(largeSheet, { header: 1 });
    const closed = new Promise(resolve => partial.once('close', resolve));
    for await (const row of partial) {
      assert.deepEqual(row, [1]);
      break;
    }
    await closed;
    assert.ok(accessed < 200);
    assert.equal(streamDestroyCalls, 1);
    assert.equal(iteratorCloseCalls, 1);
    const pushFailure = new Error('Rejected stream chunk');
    class RejectingSheetReadable extends TrackedSheetReadable {
      push(chunk, encoding) {
        if (chunk !== null) throw pushFailure;
        return super.push(chunk, encoding);
      }
    }
    SheetJS.stream.set_readable(RejectingSheetReadable);
    const failing = SheetJS.stream.to_csv({ '!ref': 'A1', A1: { t: 's', v: 'unread' } });
    await assert.rejects(async () => { for await (const chunk of failing) assert.fail(String(chunk)); },
      error => error === pushFailure);
    assert.equal(iteratorCloseCalls, 2);
    assert.equal(streamDestroyCalls, 2);
  } finally {
    generatorPrototype.return = closeIterator;
    SheetJS.stream.set_readable(Readable);
  }
  const styled = SheetJS.utils.aoa_to_sheet([[{ t: 'n', v: 1.5, z: '0.00' }, 'text']]);
  for (const [options, expected] of [
    [{}, [[1.5, 'text']]],
    [{ raw: undefined }, [['1.50', 'text']]],
    [{ raw: false, rawNumbers: true }, [[1.5, 'text']]],
    [{ rawNumbers: false }, [['1.50', 'text']]],
    [Object.create({ raw: false }), [[1.5, 'text']]],
  ]) {
    options.header = 1;
    assert.deepEqual(SheetJS.utils.sheet_to_json(styled, options), expected);
    const rows = [];
    for await (const row of SheetJS.stream.to_json(styled, options)) rows.push(row);
    assert.deepEqual(rows, expected);
  }
  const formulaCells = [
    [{ t: 'n', v: 3, f: '1+2', F: 'C2' }, { t: 'b', v: false }, { t: 'n', v: 5, w: '5.00' }],
    [{ t: 'n', v: 7, f: '3+4', F: 'C3:C4' }, { t: 's', v: 'text' }, undefined],
    [{ t: 'n', v: 8, F: 'C3:C4' }, { t: 'z' }, { t: 'e', v: 7, w: '#DIV/0!' }],
  ];
  const formulaRange = { '!ref': 'C2:E4' };
  const sparseFormulae = { ...formulaRange };
  const denseFormulae = { ...formulaRange, '!data': [] };
  formulaCells.forEach((row, r) => row.forEach((cell, c) => {
    if (!cell) return;
    sparseFormulae[SheetJS.utils.encode_cell({ r: r + 1, c: c + 2 })] = cell;
    (denseFormulae['!data'][r + 1] ??= [])[c + 2] = cell;
  }));
  for (const sheet of [sparseFormulae, denseFormulae]) {
    const formulas = ['C2:C2=1+2', 'C3:C4=3+4'];
    const all = ['C2:C2=1+2', 'D2=FALSE', 'E2=5', 'C3:C4=3+4', "D3='text", "E4='#DIV/0!"];
    assert.deepEqual(SheetJS.utils.sheet_to_formulae(sheet, { values: false }), formulas);
    assert.deepEqual(SheetJS.utils.sheet_to_formulae(sheet, { values: undefined }), all);
  }
  const htmlSheet = SheetJS.utils.aoa_to_sheet([['<text>']]);
  const defaultHtml = SheetJS.utils.sheet_to_html(htmlSheet);
  for (const options of [
    { header: null, footer: null },
    { header: '', footer: '', id: 'quoted"id', editable: true },
  ]) {
    const html = SheetJS.utils.sheet_to_html(htmlSheet, options);
    if (options.editable) {
      assert.ok(html.startsWith('<table id="quoted&quot;id">'));
      assert.ok(html.includes('<span contenteditable="true">&lt;text&gt;</span>'));
      assert.ok(html.endsWith('</table>'));
    } else {
      assert.equal(html, defaultHtml);
    }
    let streamed = '';
    for await (const chunk of SheetJS.stream.to_html(htmlSheet, options)) streamed += chunk;
    assert.equal(streamed, html);
  }
  const orderedHtmlOpts = { header: '', footer: '' };
  const plainHtml = SheetJS.utils.sheet_to_html({}, orderedHtmlOpts);
  let streamedEmptyHtml = '';
  for await (const chunk of SheetJS.stream.to_html({}, orderedHtmlOpts)) streamedEmptyHtml += chunk;
  assert.equal(plainHtml, '<table></table>');
  assert.equal(streamedEmptyHtml, plainHtml);
  const changingHtmlMerges = { '!ref': 'A1:G3', '!merges': ['B2:D3', 'A1:C2', 'F1:G3']
    .map(SheetJS.utils.decode_range) };
  const mergedHtml = SheetJS.utils.sheet_to_html(changingHtmlMerges, orderedHtmlOpts);
  assert.match(mergedHtml, /rowspan="2" colspan="3" id="sjs-A1"/);
  assert.match(mergedHtml, /rowspan="2" colspan="3" id="sjs-B2"/);
  assert.match(mergedHtml, /rowspan="3" colspan="2" id="sjs-F1"/);
  assert.doesNotMatch(mergedHtml, /id="sjs-C2"/);
  assert.match(mergedHtml, /id="sjs-A3"/);
  const wideHtmlMerges = { '!ref': 'B2:G3', G2: { t: 's', v: 'After covered cells' },
    '!merges': ['A1:D3', 'E2:F2'].map(SheetJS.utils.decode_range) };
  const wideMergedHtml = SheetJS.utils.sheet_to_html(wideHtmlMerges, orderedHtmlOpts);
  assert.match(wideMergedHtml, /colspan="2" id="sjs-E2"/);
  assert.match(wideMergedHtml, /id="sjs-G2">After covered cells<\/td>/);
  assert.doesNotMatch(wideMergedHtml, /id="sjs-[BCDF]2"/);
  let wideMergedStream = '';
  for await (const chunk of SheetJS.stream.to_html(wideHtmlMerges, orderedHtmlOpts)) wideMergedStream += chunk;
  assert.equal(wideMergedStream, wideMergedHtml);
  const malformedHtmlMerges = {
    ...wideHtmlMerges,
    '!merges': [
      null, { s: { r: 1, c: 1 }, e: { r: 2, c: 4.5 } },
      { s: { r: 1, c: 5 }, e: { r: 2, c: 2 } },
      { s: { r: NaN, c: 0 }, e: { r: 4, c: 6 } },
      ...wideHtmlMerges['!merges'],
    ],
  };
  assert.equal(SheetJS.utils.sheet_to_html(malformedHtmlMerges, orderedHtmlOpts), wideMergedHtml);
  let malformedMergedStream = '';
  for await (const chunk of SheetJS.stream.to_html(malformedHtmlMerges, orderedHtmlOpts)) malformedMergedStream += chunk;
  assert.equal(malformedMergedStream, wideMergedHtml);

  for (const dense of [false, true]) {
    const worksheet = SheetJS.utils.sheet_new({ dense });
    const stub = SheetJS.utils.sheet_get_cell(worksheet, { r: 1, c: 2 });
    assert.equal(stub.t, 'z');
    assert.equal(SheetJS.utils.sheet_get_cell(worksheet, 'C2'), stub);
    assert.equal(SheetJS.utils.sheet_get_cell(worksheet, 1, 2), stub);
    stub.t = 'n';
    stub.v = 10;
    for (const invalidRange of [42, false, 'C2:B1']) {
      worksheet['!ref'] = invalidRange;
      assert.throws(() => SheetJS.utils.sheet_set_array_formula(worksheet, 'C2:D2', '1+2'));
      assert.equal(worksheet['!ref'], invalidRange);
      assert.deepEqual(stub, { t: 'n', v: 10 });
      if (dense) assert.equal(worksheet['!data'][1][3], undefined);
      else assert.equal(worksheet.D2, undefined);
    }
    worksheet['!ref'] = 'C2';
    for (const badFormula of ['   ', '\uD800']) {
      assert.throws(() => SheetJS.utils.sheet_set_array_formula(worksheet, 'C2:D2', badFormula),
        { code: 'INVALID_VALUE' });
      assert.deepEqual(stub, { t: 'n', v: 10 });
      assert.equal(worksheet['!ref'], 'C2');
    }
    SheetJS.utils.sheet_set_array_formula(worksheet, 'C2:D2', '1+2');
    assert.equal(worksheet['!ref'], 'C2:D2');
    assert.equal(stub.f, '1+2');
  }
  const shortDenseFormulaSheet = { '!ref': 'C2:XFD1048576', '!data': [[], [,, { f: '1+2', t: 'n' }]] };
  assert.deepEqual(SheetJS.utils.sheet_to_formulae(shortDenseFormulaSheet), ['C2=1+2']);
  shortDenseFormulaSheet['!data'][1][16383] = { v: 2, t: 'n' };
  shortDenseFormulaSheet['!data'][3] = [,, { f: '3+4', t: 'n' }];
  assert.deepEqual(SheetJS.utils.sheet_to_formulae(shortDenseFormulaSheet), ['C2=1+2', 'XFD2=2', 'C4=3+4']);
  assert.deepEqual(SheetJS.utils.sheet_to_formulae(shortDenseFormulaSheet, { values: false }), ['C2=1+2', 'C4=3+4']);
  const distantDenseRows = [];
  distantDenseRows[5] = [,, { f: '2+3' }];
  Object.defineProperty(distantDenseRows, 1048575, { value: [,, { t: 'n', v: 8 }] });
  assert.deepEqual(SheetJS.utils.sheet_to_formulae({ '!ref': 'C2:XFD1048576', '!data': distantDenseRows }),
    ['C6=2+3', 'C1048576=8']);
  const accessedDenseRows = [];
  Object.defineProperty(accessedDenseRows, 1, { get() {
    accessedDenseRows[2] = [,, { f: '3+4' }];
    return [,, { f: '1+2' }];
  } });
  accessedDenseRows[5000] = [];
  assert.deepEqual(SheetJS.utils.sheet_to_formulae({ '!ref': 'C1:C6000', '!data': accessedDenseRows }),
    ['C2=1+2', 'C3=3+4']);
  const wideFormulaRow = [];
  wideFormulaRow[2] = { f: '2+3' };
  Object.defineProperty(wideFormulaRow, 16383, { value: { f: '6+7' } });
  const wideFormulaSheet = { '!ref': 'C1:XFD1', '!data': [wideFormulaRow] };
  assert.deepEqual(SheetJS.utils.sheet_to_formulae(wideFormulaSheet), ['C1=2+3', 'XFD1=6+7']);
  Object.defineProperty(wideFormulaRow, 3, { get() {
    wideFormulaRow[4] = { f: '4+5' };
    return { f: '3+4' };
  } });
  assert.deepEqual(SheetJS.utils.sheet_to_formulae(wideFormulaSheet), ['C1=2+3', 'D1=3+4', 'E1=4+5', 'XFD1=6+7']);
  const changingDenseRows = [];
  const changingDenseColumns = [];
  Object.defineProperty(changingDenseRows, 0, { get() {
    Object.setPrototypeOf(changingDenseRows,
      Object.create(Array.prototype, { 4: { value: [ { f: '6+7' } ] } }));
    return changingDenseColumns;
  } });
  Object.defineProperty(changingDenseColumns, 0, { get() {
    Object.setPrototypeOf(changingDenseColumns,
      Object.create(Array.prototype, { 4: { value: { f: '4+5' } } }));
    return { f: '2+3' };
  } });
  assert.deepEqual(SheetJS.utils.sheet_to_formulae({ '!ref': 'A1:E5', '!data': changingDenseRows }),
    ['A1=2+3', 'E1=4+5', 'A5=6+7']);
  const inheritedDenseRow = [];
  const inheritedColumns = Object.create(Array.prototype, { 3: { value: { f: '4+5', t: 'n' } } });
  Object.setPrototypeOf(inheritedDenseRow, inheritedColumns);
  assert.deepEqual(SheetJS.utils.sheet_to_formulae({ '!ref': 'D1:D1', '!data': [inheritedDenseRow] }), ['D1=4+5']);

  const htmlErrorSheet = { '!ref': 'A1:D1', A1: { t: 'e', v: 7 }, B1: { t: 'e', v: 7, w: '#DIV/0!' },
    C1: { t: 'n', v: NaN }, D1: { t: 'n', v: Infinity } };
  const htmlErrorOptions = { header: '', footer: '' };
  const htmlErrors = SheetJS.utils.sheet_to_html(htmlErrorSheet, htmlErrorOptions);
  assert.match(htmlErrors, /data-t="e" data-v="7" id="sjs-A1"><\/td>/);
  assert.match(htmlErrors, /id="sjs-B1">#DIV\/0!<\/td>/);
  assert.match(htmlErrors, /id="sjs-C1">#NUM!<\/td>/);
  assert.match(htmlErrors, /id="sjs-D1">#DIV\/0!<\/td>/);
  let streamedErrors = '';
  for await (const chunk of SheetJS.stream.to_html(htmlErrorSheet, htmlErrorOptions)) streamedErrors += chunk;
  assert.equal(streamedErrors, htmlErrors);

  const inputLink = SheetJS.read('<table><tr><td><a href="#local">local</a><strong><a href="https://example.com/a?x=1&amp;y=2">external<br>next</a></strong></td><td><a href="data:text/plain,test">ignored</a></td></tr></table>', { type: 'string' });
  const linkSheet = inputLink.Sheets[inputLink.SheetNames[0]];
  assert.equal(linkSheet.A1.l.Target, 'https://example.com/a?x=1&y=2');
  assert.equal(linkSheet.A1.v, 'localexternal\nnext');
  assert.equal(linkSheet.B1.l, undefined);
  assert.ok(SheetJS.utils.sheet_to_html(linkSheet).includes('href="https://example.com/a?x=1&amp;y=2"'));
  const date = new Date('2024-01-02T13:04:00Z');
  for (const cellDates of [false, true]) {
    const sheet = SheetJS.utils.aoa_to_sheet([[date]], { UTC: true, cellDates, dateNF: 'dd-mmm-yy', dense: true });
    const cell = sheet['!data'][0][0];
    assert.equal(cell.t, cellDates ? 'd' : 'n');
    assert.equal(cell.z, 'dd-mmm-yy');
    assert.equal(cell.w, '02-Jan-24');
  }
  const localizedFormat = 'yyyy"年"m"月"d"日"';
  const localized = SheetJS.utils.aoa_to_sheet([[date]], { UTC: true, dateNF: localizedFormat });
  assert.equal(localized.A1.w, '2024年1月2日');
  assert.equal(SheetJS.SSF.format('yyyy\\年m\\月d\\日', 60), '1900年2月29日');
  assert.equal(SheetJS.SSF.format(localizedFormat, 0, { date1904: true }), '1904年1月1日');
  const dateHeader = ['date'];
  const jsonDates = SheetJS.utils.json_to_sheet([{ date, note: 'sample' }], {
    header: dateHeader, skipHeader: true, UTC: true, origin: 'C2', dateNF: 'yyyy-mm-dd',
  });
  assert.deepEqual(dateHeader, ['date', 'note']);
  assert.equal(jsonDates.C2.z, 'yyyy-mm-dd');
  jsonDates.C2.z = 14;
  SheetJS.utils.sheet_add_json(jsonDates, [{ date }], { skipHeader: true, UTC: true, origin: 'C2' });
  assert.equal(jsonDates.C2.z, 14);
  const cutoffSheet = SheetJS.utils.aoa_to_sheet([[1], [2, 3], [], [4, 5, 6]]);
  const cutoffBytes = SheetJS.write(SheetJS.utils.book_new(cutoffSheet), { type: 'array' });
  for (const dense of [false, true]) {
    const book = SheetJS.read(cutoffBytes, { sheetRows: 2, dense });
    const view = book.Sheets.Sheet1;
    assert.equal(view['!ref'], 'A1:B2');
    assert.equal(view['!fullref'], 'A1:C4');
    assert.equal(dense ? view['!data'][1][1].v : view.B2.v, 3);
    assert.equal(dense ? view['!data'][3] : view.C4, undefined);
    const reopened = SheetJS.read(SheetJS.write(book, { type: 'array' })).Sheets.Sheet1;
    assert.equal(reopened.C4.v, 6);
  }
  assert.equal(SheetJS.read(cutoffBytes, { sheetRows: 2 ** 32 + 1 }).Sheets.Sheet1.C4.v, 6);
  assert.equal(SheetJS.default.utils, SheetJS.utils);
  const { CFB } = SheetJS;
  assert.equal(CFB, SheetJS.default.CFB);
  const container = CFB.utils.cfb_new({ root: 'Workbook' });
  CFB.utils.cfb_add(container, 'folder/entry', Buffer.from('content'));
  CFB.utils.cfb_add(container, 'removed', Buffer.from('obsolete'));
  assert.ok(CFB.utils.cfb_del(container, 'removed'));
  const packed = join(directory, 'container.cfb');
  CFB.writeFile(container, packed);
  const reopened = CFB.read(packed, { type: 'file', raw: true });
  assert.equal(CFB.find(reopened, '/folder/entry').content.toString(), 'content');
  assert.equal(CFB.find(reopened, 'removed'), null);
  assert.ok(reopened.raw.header.length > 0);
  const zipped = CFB.write(container, { fileType: 'zip', type: 'base64', compression: true });
  const zip = CFB.read(zipped, { type: 'base64' });
  assert.equal(CFB.find(zip, '/folder/entry').content.toString(), 'content');



  const workbookZip = CFB.read(source, { type: 'file' });
  assert.equal(SheetJS.default.parse_zip(workbookZip).Sheets.Sheet1.A1.v, 23);
  assert.deepEqual(SheetJS.parse_zip(workbookZip, { bookSheets: true }).SheetNames, ['Sheet1']);

  for (const bookType of ['ods', 'fods', 'xlml']) {
    const bytes = SheetJS.write(draft, { bookType, type: 'buffer' });
    assert.equal(SheetJS.read(bytes).Sheets.Sheet1.A1.v, 23);
    if (bookType === 'ods') {
      assert.equal(SheetJS.parse_zip(CFB.read(bytes, { type: 'buffer' })).Sheets.Sheet1.A1.v, 23);
    }
  }
  const directMergeBook = new Workbook();
  const directMergeSheet = directMergeBook.addWorksheet('Direct cell merges');
  const directMaster = directMergeSheet.getCell('A1');
  directMaster.value = 7;
  directMaster.numFmt = '0.0';
  directMergeSheet.getCell('C3').value = 23;
  const directFollower = directMergeSheet.getCell('D4');
  directFollower.value = 42;
  directFollower.merge(directMaster);
  assert.deepEqual(directMergeSheet.model.merges, []);
  assert.equal(directFollower.master, directMaster);
  assert.equal(directFollower.type, ValueType.Merge);
  assert.equal(directFollower.value, 7);
  assert.equal(directFollower.numFmt, '0.0');
  directFollower.numFmt = '0.00';
  assert.equal(directMaster.numFmt, '0.00');
  directFollower.style = { numFmt: '0.0000' };
  assert.equal(directMaster.numFmt, '0.00');
  assert.equal(directFollower.numFmt, '0.0000');
  directFollower.value = 9;
  assert.equal(directMaster.value, 9);
  const detachedFollower = directMergeSheet.getCell('B2');
  detachedFollower.numFmt = '0.000';
  detachedFollower.merge(directMaster, true);
  assert.equal(detachedFollower.numFmt, '0.000');
  const directMergeBytes = await directMergeBook.xlsx.writeBuffer();
  const directMergeDocument = new XlsxDocument(directMergeBytes);
  try {
    const part = directMergeDocument.zip_entry_names().find(name => name.endsWith('/worksheets/sheet1.xml'));
    const xml = new TextDecoder().decode(directMergeDocument.zip_entry_bytes(part));
    assert.match(xml, /<c r="C3"[^>]*><v>23<\/v><\/c>/);
  } finally {
    directMergeDocument.free();
  }
  const directMergeRead = (await new Workbook().xlsx.load(directMergeBytes)).getWorksheet(1);
  assert.deepEqual(directMergeRead.model.merges, ['A1:D4']);
  assert.equal(directMergeRead.getCell('D4').value, 9);
  assert.equal(directMergeRead.getCell('D4').numFmt, '0.0000');
  assert.equal(directMergeRead.getCell('B2').numFmt, '0.000');
  assert.deepEqual(directMergeSheet.model.merges, []);
  directFollower.unmerge();
  assert.equal(directFollower.value, null);
  assert.equal(directFollower.master, directFollower);
  detachedFollower.unmerge();
  assert.equal(directMaster.isMerged, false);
  assert.deepEqual((await new Workbook().xlsx.load(await directMergeBook.xlsx.writeBuffer())).getWorksheet(1).model.merges, []);

  const chainMergeBook = new Workbook();
  const chainMergeSheet = chainMergeBook.addWorksheet('Chained merges');
  const chainRoot = chainMergeSheet.getCell('A1');
  const chainMiddle = chainMergeSheet.getCell('B2');
  const chainTail = chainMergeSheet.getCell('C3');
  chainRoot.value = 7;
  chainMiddle.merge(chainRoot);
  chainTail.merge(chainMiddle);
  assert.equal(chainTail.master, chainMiddle);
  assert.equal(chainTail.value, 7);
  chainTail.value = 8;
  assert.equal(chainRoot.value, 8);
  assert.throws(() => chainRoot.merge(chainTail), { code: 'MERGE_EDIT_REFUSED' });
  assert.equal(chainRoot.value, 8);
  await assert.rejects(chainMergeBook.xlsx.writeBuffer(), { code: 'MERGE_EDIT_REFUSED' });
  assert.equal(chainTail.master, chainMiddle);
  chainTail.unmerge();
  assert.equal(chainTail.value, null);
  const chainMergeRead = (await new Workbook().xlsx.load(await chainMergeBook.xlsx.writeBuffer())).getWorksheet(1);
  assert.equal(chainMergeRead.getCell('B2').value, 8);
  assert.equal(chainMergeRead.getCell('C3').value, null);

  const reverseMergeBook = new Workbook();
  const reverseMergeSheet = reverseMergeBook.addWorksheet('Reverse cell merge');
  const reverseMaster = reverseMergeSheet.getCell('D4');
  const reverseFollower = reverseMergeSheet.getCell('A1');
  reverseMaster.value = 23;
  reverseFollower.value = 9;
  reverseFollower.merge(reverseMaster);
  assert.equal(reverseFollower.value, 23);
  assert.equal(reverseFollower.master, reverseMaster);
  const reverseMergeBytes = await reverseMergeBook.xlsx.writeBuffer();
  const reverseMergeDocument = new XlsxDocument(reverseMergeBytes);
  try {
    const part = reverseMergeDocument.zip_entry_names().find(name => name.endsWith('/worksheets/sheet1.xml'));
    const xml = new TextDecoder().decode(reverseMergeDocument.zip_entry_bytes(part));
    assert.match(xml, /<c r="D4"[^>]*><v>23<\/v><\/c>/);
  } finally {
    reverseMergeDocument.free();
  }
  const reverseMergeRead = (await new Workbook().xlsx.load(reverseMergeBytes)).getWorksheet(1);
  assert.deepEqual(reverseMergeRead.model.merges, ['A1:D4']);
  assert.equal(reverseMergeRead.getCell('D4').master.address, 'A1');
  assert.equal(reverseMergeRead.getCell('D4').value, null);
  assert.equal(reverseFollower.value, 23);

  const streamDirectPath = join(directory, 'direct-streaming-merge.xlsx');
  const streamDirectWriter = new stream.xlsx.WorkbookWriter({ filename: streamDirectPath, useStyles: true });
  const streamDirectSheet = streamDirectWriter.addWorksheet('Direct merge');
  const streamDirectMaster = streamDirectSheet.getCell('A1');
  const streamDirectFollower = streamDirectSheet.getCell('D4');
  streamDirectMaster.value = 7;
  streamDirectMaster.numFmt = '0.0';
  streamDirectFollower.value = 23;
  streamDirectFollower.merge(streamDirectMaster);
  assert.equal(streamDirectFollower.master, streamDirectMaster);
  assert.equal(streamDirectFollower.type, ValueType.Merge);
  assert.equal(streamDirectFollower.value, 7);
  streamDirectFollower.value = 8;
  assert.equal(streamDirectMaster.value, 8);
  streamDirectFollower.numFmt = '0.00';
  assert.equal(streamDirectMaster.numFmt, '0.00');
  streamDirectFollower.style = { numFmt: '0.000' };
  assert.equal(streamDirectMaster.numFmt, '0.00');
  assert.equal(streamDirectFollower.numFmt, '0.000');
  streamDirectFollower.note = 'Covered note';
  const streamDirectDetached = streamDirectSheet.getCell('B2');
  streamDirectDetached.merge(streamDirectMaster);
  streamDirectDetached.unmerge();
  assert.equal(streamDirectDetached.value, null);
  assert.equal(streamDirectDetached.master, streamDirectDetached);
  assert.throws(() => streamDirectMaster.merge(streamDirectFollower), { code: 'MERGE_EDIT_REFUSED' });
  await streamDirectWriter.commit();
  const streamDirectRead = (await new Workbook().xlsx.readFile(streamDirectPath)).getWorksheet(1);
  assert.deepEqual(streamDirectRead.model.merges, []);
  assert.equal(streamDirectRead.getCell('A1').value, 8);
  assert.equal(streamDirectRead.getCell('D4').value, null);
  assert.equal(streamDirectRead.getCell('D4').numFmt, '0.000');
  assert.equal(streamDirectRead.getCell('D4').note, 'Covered note');

  const streamBlankPath = join(directory, 'blank-streaming-merge.xlsx');
  const streamBlankWriter = new stream.xlsx.WorkbookWriter({ filename: streamBlankPath });
  const streamBlankSheet = streamBlankWriter.addWorksheet('Blank follower');
  const streamBlankMaster = streamBlankSheet.getCell('A1');
  streamBlankMaster.value = 23;
  streamBlankSheet.getCell('C3').merge(streamBlankMaster);
  await streamBlankWriter.commit();
  const streamBlankDocument = new XlsxDocument(fs.readFileSync(streamBlankPath));
  try {
    const part = streamBlankDocument.zip_entry_names().find(name => name.endsWith('/worksheets/sheet1.xml'));
    const xml = new TextDecoder().decode(streamBlankDocument.zip_entry_bytes(part));
    assert.match(xml, /<c r="C3"[^>]*(?:\/>|><\/c>)/);
    assert.doesNotMatch(xml, /<mergeCells/);
  } finally {
    streamBlankDocument.free();
  }

  const nestedMergeBook = new Workbook();
  const nestedMergeSheet = nestedMergeBook.addWorksheet('Nested ranges');
  nestedMergeSheet.getCell('A1').value = 5;
  nestedMergeSheet.mergeCells([[['B2', 'A1']]]);
  assert.equal(nestedMergeSheet.getCell('B2').master.address, 'A1');
  nestedMergeSheet.unMergeCells([[['B2']]]);
  assert.equal(nestedMergeSheet.getCell('B2').master.address, 'B2');

  const nestedMergePath = join(directory, 'nested-merge.xlsx');
  const nestedMergeWriter = new stream.xlsx.WorkbookWriter({ filename: nestedMergePath });
  const nestedMergeStreaming = nestedMergeWriter.addWorksheet('Nested ranges');
  nestedMergeStreaming.getCell('A1').value = 5;
  nestedMergeStreaming.mergeCells([[['B2:A1']]]);
  assert.equal(nestedMergeStreaming.getCell('B2').master.address, 'A1');
  const circularRange = [];
  circularRange.push(circularRange);
  assert.throws(() => nestedMergeStreaming.mergeCells(circularRange), { code: 'INVALID_ADDRESS' });
  await nestedMergeWriter.commit();
  const nestedMergeRead = (await new Workbook().xlsx.readFile(nestedMergePath)).getWorksheet(1);
  assert.equal(nestedMergeRead.getCell('B2').master.address, 'A1');
  assert.equal(nestedMergeRead.getCell('B2').value, 5);

  const mergeReferenceBook = new Workbook();
  const mergeReferenceSheet = mergeReferenceBook.addWorksheet('References');
  const mergeReferenceCell = mergeReferenceSheet.getCell('A1');
  mergeReferenceCell.value = 42;
  const mergeReferenceBaseline = await mergeReferenceBook.xlsx.writeBuffer();
  mergeReferenceCell.addMergeRef();
  mergeReferenceCell.addMergeRef();
  mergeReferenceCell.releaseMergeRef();
  assert.equal(mergeReferenceCell.isMerged, true);
  assert.equal(mergeReferenceCell.master, mergeReferenceCell);
  assert.deepEqual(await mergeReferenceBook.xlsx.writeBuffer(), mergeReferenceBaseline);
  mergeReferenceCell.releaseMergeRef();
  assert.equal(mergeReferenceCell.isMerged, false);
  mergeReferenceCell.addMergeRef();
  mergeReferenceSheet.mergeCells('A1:B1');
  const mergeReferenceFollower = mergeReferenceSheet.getCell('B1');
  assert.equal(mergeReferenceFollower.master, mergeReferenceCell);
  mergeReferenceCell.releaseMergeRef();
  assert.equal(mergeReferenceCell.isMerged, true);
  assert.equal(mergeReferenceFollower.isMerged, true);
  mergeReferenceSheet.unMergeCells('A1:B1');
  assert.equal(mergeReferenceCell.isMerged, false);
  assert.equal(mergeReferenceFollower.isMerged, false);
  for (const Type of [Workbook, stream.xlsx.WorkbookWriter]) {
    const textBook = new Type();
    const textSheet = textBook.addWorksheet('Merged text');
    textSheet.getCell('A1').value = { formula: '1+1', result: 2 };
    textSheet.mergeCells('A1:B1');
    assert.equal(textSheet.getCell('A1').text, '2');
    assert.equal(textSheet.getCell('B1').text, '[object Object]');
    assert.equal(textSheet.getCell('B1').toCsvString(), '');
    await textBook.commit?.();
  }
  const reusedIdsBook = new Workbook();
  const deletedSheet = reusedIdsBook.addWorksheet('Deleted');
  reusedIdsBook.addWorksheet('Retained');
  reusedIdsBook.removeWorksheet(deletedSheet.id);
  const reusedIdReplacement = reusedIdsBook.addWorksheet('Replacement');
  const followingSheet = reusedIdsBook.addWorksheet('Following');
  assert.equal(reusedIdReplacement.id, 1);
  assert.equal(followingSheet.id, 3);
  assert.equal(reusedIdsBook.getWorksheet(1), reusedIdReplacement);
  const reusedIdsReloaded = new Workbook();
  await reusedIdsReloaded.xlsx.load(await reusedIdsBook.xlsx.writeBuffer());
  assert.deepEqual(reusedIdsReloaded.worksheets.map(sheet => [sheet.name, sheet.id]),
    [['Retained', 2], ['Replacement', 3], ['Following', 4]]);
  const matrixCell = { row: 1, col: 1, address: 'A1' };
  const unchangedMatrixCell = { row: 3, col: 2, address: 'B3' };
  new Workbook().definedNames.normaliseMatrix({
    forEachInSheet(sheetName, visit) {
      assert.equal(sheetName, 'Data');
      visit(matrixCell, 4, 3);
      visit(unchangedMatrixCell, 3, 2);
    },
  }, 'Data');
  assert.deepEqual(matrixCell, { row: 4, col: 3, address: 'C4' });
  assert.deepEqual(unchangedMatrixCell, { row: 3, col: 2, address: 'B3' });
  const matrixBook = new Workbook();
  matrixBook.addWorksheet('Data');
  const namedMatrix = matrixBook.definedNames.getMatrix('MatrixName');
  assert.equal(matrixBook.definedNames.getMatrix('MatrixName'), namedMatrix);
  namedMatrix.addCell('Data!A1');
  const namedMatrixCell = namedMatrix.getCellAt('Data', 2, 1);
  assert.equal(namedMatrix.findCell('Data!A2'), namedMatrixCell);
  assert.deepEqual(matrixBook.definedNames.getRanges('MatrixName', namedMatrix).ranges, ['Data!$A$1:$A$2']);
  namedMatrix.removeCellEx({ sheetName: 'Data', row: 1, col: 1 });
  assert.equal(namedMatrix.findCell('Data!A1'), undefined);
  assert.deepEqual(matrixBook.definedNames.getNames('Data!A2'), ['MatrixName']);
  matrixBook.definedNames.add('Data!A2', 'IndependentName');
  namedMatrix.spliceRows('Data', 1, 0, 2);
  namedMatrix.spliceColumns('Data', 1, 0, 1);
  assert.deepEqual(matrixBook.definedNames.getNames('Data!B4'), ['MatrixName']);
  assert.deepEqual(matrixBook.definedNames.getNames('Data!A2'), ['IndependentName']);
  assert.equal(namedMatrix.findCell('Data!A2'), undefined);
  matrixBook.addWorksheet("Data's sheet");
  matrixBook.definedNames.add("'Data''s sheet'!A1:B2", 'QuotedMatrix');
  const quotedRanges = matrixBook.definedNames.getRanges('QuotedMatrix', matrixBook.definedNames.getMatrix('QuotedMatrix'));
  assert.deepEqual(quotedRanges.ranges, ["'Data''s sheet'!$A$1:$B$2"]);
  matrixBook.definedNames.model = [quotedRanges];
  assert.deepEqual(matrixBook.definedNames.getNames("'Data''s sheet'!B2"), ['QuotedMatrix']);
  const namedSpliceWorkbook = new Workbook();
  const namedSpliceSheet = namedSpliceWorkbook.addWorksheet('Data');
  namedSpliceSheet.getCell('A3').value = 'unchanged';
  namedSpliceWorkbook.definedNames.add('Data!A1:C4', 'Region');
  namedSpliceWorkbook.definedNames.spliceRows('Data', 2, 1, 2);
  namedSpliceWorkbook.definedNames.spliceColumns('Data', 2, 1, 0);
  assert.deepEqual(namedSpliceWorkbook.definedNames.getNames('Data!B1'), ['Region']);
  assert.deepEqual(namedSpliceWorkbook.definedNames.getNames('Data!B2'), []);
  assert.deepEqual(namedSpliceWorkbook.definedNames.getNames('Data!B4'), ['Region']);
  assert.equal(namedSpliceSheet.getCell('A3').value, 'unchanged');
  const namedSpliceReloaded = new Workbook();
  await namedSpliceReloaded.xlsx.load(await namedSpliceWorkbook.xlsx.writeBuffer());
  assert.deepEqual(namedSpliceReloaded.definedNames.model, namedSpliceWorkbook.definedNames.model);
  namedSpliceWorkbook.addWorksheet('Other');
  namedSpliceWorkbook.definedNames.add('Other!B2', 'Untouched');
  const namedArchive = SheetJS.CFB.read(await namedSpliceWorkbook.xlsx.writeBuffer(), { type: 'buffer' });
  const namedXmlPath = 'xl/workbook.xml';
  const namedXml = Buffer.from(SheetJS.CFB.find(namedArchive, '/' + namedXmlPath).content).toString();
  const originalName = 'name="Untouched">Other!B2</definedName>';
  const modifiedNamedXml = namedXml
    .replace(/name="Untouched">[^<]+<\/definedName>/, originalName)
    .replace(/(name="Region">[^<]+)<\/definedName>/, '$1,Other!C3</definedName>');
  assert.notEqual(modifiedNamedXml, namedXml);
  SheetJS.CFB.utils.cfb_add(namedArchive, namedXmlPath, Buffer.from(modifiedNamedXml));
  const namedLocalityBook = new Workbook();
  await namedLocalityBook.xlsx.load(SheetJS.CFB.write(namedArchive, { fileType: 'zip', type: 'buffer' }));
  namedLocalityBook.definedNames.spliceRows('Data', 1, 0, 1);
  const namedLocalityArchive = SheetJS.CFB.read(await namedLocalityBook.xlsx.writeBuffer(), { type: 'buffer' });
  const namedLocalityXml = Buffer.from(SheetJS.CFB.find(namedLocalityArchive, '/' + namedXmlPath).content).toString();
  assert.ok(namedLocalityXml.includes(originalName));
  assert.match(namedLocalityXml, /name="Region">[^<]+,Other!C3<\/definedName>/);
  namedLocalityBook.definedNames.add('Data!A1048576', 'Boundary');
  const namedBeforeFailedSplice = await namedLocalityBook.xlsx.writeBuffer();
  assert.throws(() => namedLocalityBook.definedNames.spliceRows('Data', 1, 0, 1), { code: 'INVALID_ADDRESS' });
  assert.deepEqual(await namedLocalityBook.xlsx.writeBuffer(), namedBeforeFailedSplice);
  assert.throws(() => namedLocalityBook.definedNames.spliceRows('Data', 1, -1, 0), { code: 'INVALID_ADDRESS' });
  const xlmlColumns = '<Workbook xmlns="urn:schemas-microsoft-com:office:spreadsheet" ' +
    'xmlns:ss="urn:schemas-microsoft-com:office:spreadsheet"><Worksheet ss:Name="Data"><Table>' +
    '<Column ss:Width="8"/><Column ss:Width="30.2" ss:Span="1"/>' +
    '<Row><Cell><Data ss:Type="Number">1</Data></Cell></Row></Table></Worksheet></Workbook>';
  const xlmlLayout = SheetJS.read(xlmlColumns, { type: 'string' }).Sheets.Data['!cols'];
  assert.deepEqual(xlmlLayout, [
    { wpx: 8, wch: 0.5, width: 1.33203125, MDW: 6 },
    { wpx: 30, wch: 4.17, width: 5.00390625, MDW: 6 },
    { wpx: 30, wch: 4.17, width: 5.00390625, MDW: 6 },
  ]);
  assert.notEqual(xlmlLayout[1], xlmlLayout[2]);
  const xlmlStyles = xlmlColumns.replace('<Worksheet', '<Styles>' +
    '<Style ss:ID="Base"><Interior ss:Pattern="Gray50"/></Style>' +
    '<Style ss:ID="Child" ss:Parent="Base"/></Styles><Worksheet')
    .replace('<Row><Cell>', '<Row ss:StyleID="Child"><Cell>');
  assert.deepEqual(SheetJS.read(xlmlStyles, { type: 'string', cellStyles: true }).Sheets.Data.A1.s,
    { patternType: 'mediumGray' });
  assert.equal(SheetJS.read(xlmlStyles, { type: 'string', cellStyles: false }).Sheets.Data.A1.s, undefined);
  assert.throws(() => SheetJS.read(xlmlStyles, { type: 'string', cellStyles: 'true' }),
    { code: 'UNSUPPORTED_OPTIONS' });
  const xlmlNumberFormats = xlmlColumns.replace('<Worksheet', '<Styles>' +
    '<Style ss:ID="ParentFormat"><NumberFormat ss:Format="0.00"/></Style>' +
    '<Style ss:ID="RowFormat" ss:Parent="ParentFormat"><NumberFormat ss:Format="0.000"/></Style>' +
    '<Style ss:ID="ColumnFormat"><NumberFormat ss:Format="0%"/></Style>' +
    '<Style ss:ID="CellFormat"><NumberFormat ss:Format="0.0"/></Style></Styles><Worksheet')
    .replace('<Column ss:Width="8"/>', '<Column ss:StyleID="ColumnFormat" ss:Width="8"/>')
    .replace('<Row><Cell><Data ss:Type="Number">1</Data></Cell></Row>',
      '<Row ss:StyleID="RowFormat"><Cell><Data ss:Type="Number">1</Data></Cell>' +
      '<Cell ss:StyleID="CellFormat"><Data ss:Type="Number">1</Data></Cell></Row>' +
      '<Row><Cell><Data ss:Type="Number">1</Data></Cell></Row>');
  {
    const formats = SheetJS.SSF.get_table();
    const styleId = 200;
    const previous = Object.getOwnPropertyDescriptor(formats, styleId);
    try {
      SheetJS.SSF.load('0.00000', styleId);
      const draft = SheetJS.utils.book_new();
      const sheet = SheetJS.utils.aoa_to_sheet([[1.25]]);
      SheetJS.utils.cell_set_number_format(sheet.A1, styleId);
      SheetJS.utils.book_append_sheet(draft, sheet, 'Registered format');
      const output = { bookType: 'xlsx', type: 'array' };
      const loaded = SheetJS.read(SheetJS.write(draft, output), { cellNF: true });
      const cell = loaded.Sheets['Registered format'].A1;
      assert.equal(cell.z, '0.00000');
      assert.equal(cell.w, '1.25000');

      SheetJS.SSF.load('0.000000', styleId);
      SheetJS.utils.cell_set_number_format(cell, styleId);
      const updated = SheetJS.read(SheetJS.write(loaded, output), { cellNF: true });
      assert.deepEqual(updated.Sheets['Registered format'].A1, { t: 'n', v: 1.25, z: '0.000000', w: '1.250000' });
    } finally {
      if (previous) Object.defineProperty(formats, styleId, previous);
      else delete formats[styleId];
    }
  }

  {
    const book = new Workbook();
    const sheet = book.addWorksheet('Date formats');
    sheet.getCell('A1').value = 45293;
    sheet.getCell('A1').numFmt = 'm/d/yy';
    sheet.getCell('B1').value = 45293.625;
    sheet.getCell('B1').numFmt = 'yyyy-mm-dd"T"hh:mm:ss.000"Z"';
    sheet.getCell('C1').value = 12.34;
    sheet.getCell('C1').numFmt = 'QQQ';
    sheet.getCell('D1').value = { formula: '1+2', result: 3 };
    sheet.getCell('D1').numFmt = 'QQQ';
    const bytes = await book.xlsx.writeBuffer();
    const numeric = SheetJS.read(bytes, { cellNF: true, dateNF: 'yyyy-mm-dd' }).Sheets['Date formats'];
    assert.deepEqual(numeric.A1, { t: 'n', v: 45293, z: 'm/d/yy', w: '1/2/24' });
    assert.equal(numeric.B1.w, '2024-01-02T15:00:00.000Z');
    assert.deepEqual(numeric.C1, { t: 'n', v: 12.34, z: 'QQQ' });
    assert.deepEqual(numeric.D1, { t: 'n', v: 3, z: 'QQQ', f: '1+2' });
    assert.equal(SheetJS.utils.format_cell(numeric.C1), '12.34');

    const dates = SheetJS.read(bytes, { cellDates: true, cellNF: true, cellText: false }).Sheets['Date formats'];
    assert.deepEqual(dates.B1, {
      t: 'd', v: new Date('2024-01-02T15:00:00Z'), z: 'yyyy-mm-dd"T"hh:mm:ss.000"Z"',
    });
    assert.equal(SheetJS.utils.format_cell(dates.B1), '2024-01-02T15:00:00.000Z');
  }

  // Author and text only: reads also carry 0.20.3's r, T and h (checked in the h/r projection case).
  const commentTexts = (comments) => comments.map(({ a, t }) => ({ a, t }));

  {
    const draft = SheetJS.utils.book_new();
    const sheet = SheetJS.utils.aoa_to_sheet([[1, 2]]);
    SheetJS.utils.cell_add_comment(sheet.A1, 'First', 'Alice');
    SheetJS.utils.cell_add_comment(sheet.A1, 'Second', 'Bob');
    SheetJS.utils.sheet_add_aoa(sheet, [[{ t: 'n', v: 3, f: '1+2' }]], { origin: 'C1' });
    SheetJS.utils.cell_add_comment(sheet.C1, 'Formula review', 'Alice');
    SheetJS.utils.cell_set_internal_link(sheet.C1, 'Comments!A1');
    SheetJS.utils.book_append_sheet(draft, sheet, 'Comments');
    const output = { bookType: 'xlsx', type: 'array' };
    const bytes = new Uint8Array(SheetJS.write(draft, output));
    const workbook = SheetJS.read(bytes);
    const projected = workbook.Sheets.Comments;
    const comments = [{ a: 'Alice', t: 'First' }, { a: 'Bob', t: 'Second' }];
    assert.deepEqual(commentTexts(projected.A1.c), comments);
    assert.deepEqual(commentTexts(projected.C1.c), [{ a: 'Alice', t: 'Formula review' }]);
    assert.equal(projected.C1.l.Target, '#Comments!A1');
    assert.deepEqual(new Uint8Array(SheetJS.write(workbook, output)), bytes);
    projected.B1.v = 3;
    SheetJS.utils.cell_add_comment(projected.C1, 'Approved', 'Bob');
    const formula = SheetJS.read(SheetJS.write(workbook, output)).Sheets.Comments.C1;
    assert.equal(formula.f, '1+2');
    assert.equal(formula.v, 3);
    assert.deepEqual(commentTexts(formula.c), [{ a: 'Alice', t: 'Formula review' }, { a: 'Bob', t: 'Approved' }]);
    const hidden = SheetJS.read(bytes, { cellFormula: false });
    const hiddenCell = hidden.Sheets.Comments.C1;
    assert.equal(hiddenCell.f, undefined);
    assert.throws(() => { hiddenCell.v = 42; }, TypeError);
    hiddenCell.c[0].t = 'Edited while hidden';
    const visible = SheetJS.read(SheetJS.write(hidden, output)).Sheets.Comments.C1;
    assert.equal(visible.f, '1+2');
    assert.equal(visible.v, 3);
    assert.deepEqual(commentTexts(visible.c), [{ a: 'Alice', t: 'Edited while hidden' }]);
    assert.deepEqual(commentTexts(SheetJS.read(SheetJS.write(workbook, output)).Sheets.Comments.A1.c), comments);
    projected.A1.c[1].t = 42;
    assert.throws(() => SheetJS.write(workbook, output), { code: 'INVALID_VALUE' });
    projected.A1.c[1].t = 'Second';
    projected.A1.c.reverse();
    projected.A1.c[0].t = 'Revised';
    const replaced = SheetJS.read(SheetJS.write(workbook, output));
    assert.deepEqual(commentTexts(replaced.Sheets.Comments.A1.c), [
      { a: 'Bob', t: 'Revised' }, { a: 'Alice', t: 'First' },
    ]);
    delete replaced.Sheets.Comments.A1.c;
    SheetJS.utils.cell_add_comment(replaced.Sheets.Comments.B1, 'Added', 'Charlie');
    SheetJS.utils.cell_add_comment(replaced.Sheets.Comments.B1, 'Also added', 'Alice');
    const moved = SheetJS.read(SheetJS.write(replaced, output)).Sheets.Comments;
    assert.equal(moved.A1.c, undefined);
    assert.deepEqual(commentTexts(moved.B1.c), [{ a: 'Charlie', t: 'Added' }, { a: 'Alice', t: 'Also added' }]);

    const richBook = new Workbook();
    const richCell = richBook.addWorksheet('Rich comments').getCell('A1');
    richCell.value = 1;
    richCell.note = { texts: [{ text: 'Formatted', font: { bold: true } }] };
    const withRich = SheetJS.read(await richBook.xlsx.writeBuffer());
    const withRichCell = withRich.Sheets['Rich comments'].A1;
    SheetJS.utils.cell_add_comment(withRichCell, 'Plain', 'Editor');
    withRichCell.c.reverse();
    const richRoundTrip = SheetJS.read(SheetJS.write(withRich, output));
    assert.deepEqual(richRoundTrip.Sheets['Rich comments'].A1.c.map(comment => comment.t), ['Plain', 'Formatted']);
    richRoundTrip.Sheets['Rich comments'].A1.c.shift();
    const reloaded = await new Workbook().xlsx.load(SheetJS.write(richRoundTrip, output));
    assert.equal(reloaded.getWorksheet('Rich comments').getCell('A1').note.texts[0].font.bold, true);
  }

  {
    const sheet = SheetJS.utils.aoa_to_sheet([[2, 3]]);
    SheetJS.utils.sheet_set_array_formula(sheet, 'A1:B1', '{2,3}');
    sheet.A1.v = 2;
    sheet.B1.v = 3;
    SheetJS.utils.cell_add_comment(sheet.B1, 'Follower', 'Alice');
    const output = { bookType: 'xlsx', type: 'array' };
    const bytes = SheetJS.write(SheetJS.utils.book_new(sheet, 'Array comments'), output);
    for (const dense of [false, true]) {
      const book = SheetJS.read(bytes, { dense });
      const projected = book.Sheets['Array comments'];
      const anchor = SheetJS.utils.sheet_get_cell(projected, 'A1');
      const follower = SheetJS.utils.sheet_get_cell(projected, 'B1');
      assert.deepEqual(commentTexts(follower.c), [{ a: 'Alice', t: 'Follower' }]);
      assert.throws(() => { anchor.f = '42'; }, TypeError);
      assert.throws(() => { follower.v = 42; }, TypeError);
      SheetJS.utils.cell_add_comment(anchor, 'Anchor', 'Bob');
      SheetJS.utils.cell_set_hyperlink(anchor, 'https://example.com/array');
      delete follower.c;
      const updated = SheetJS.read(SheetJS.write(book, output)).Sheets['Array comments'];
      assert.equal(updated.A1.f, '{2,3}');
      assert.equal(updated.A1.F, 'A1:B1');
      assert.equal(updated.A1.v, 2);
      assert.equal(updated.B1.v, 3);
      assert.deepEqual(commentTexts(updated.A1.c), [{ a: 'Bob', t: 'Anchor' }]);
      assert.equal(updated.A1.l.Target, 'https://example.com/array');
      assert.equal(updated.B1.c, undefined);
    }
  }

  {
    const workbook = new Workbook();
    const cell = workbook.addWorksheet('Rich cell comments').getCell('A1');
    cell.value = { richText: [{ text: 'Bold', font: { bold: true } }, { text: ' plain' }] };
    cell.note = 'Original';
    for (const useSharedStrings of [false, true]) {
      const bytes = await workbook.xlsx.writeBuffer({ useSharedStrings });
      const output = { bookType: 'xlsx', type: 'array' };
      const loaded = SheetJS.read(bytes);
      const projected = loaded.Sheets['Rich cell comments'].A1;
      assert.equal(projected.c[0].t, 'Original');
      assert.throws(() => { projected.v = 'Edited text'; }, TypeError);
      SheetJS.utils.cell_set_hyperlink(projected, 'https://example.com/rich', 'Rich');
      SheetJS.utils.cell_add_comment(projected, 'Added', 'Reviewer');
      const saved = SheetJS.write(loaded, output);
      const updated = SheetJS.read(saved).Sheets['Rich cell comments'].A1;
      assert.deepEqual(updated.l, { ref: 'A1', Target: 'https://example.com/rich', Tooltip: 'Rich' });
      assert.equal(updated.r, projected.r);
      assert.equal(updated.h, projected.h);
      assert.deepEqual(updated.c.map(({ t }) => t), ['Original', 'Added']);
    }
  }

  {
    const sheet = SheetJS.utils.aoa_to_sheet([[1, 'First'], [2, 'Second']]);
    SheetJS.utils.cell_set_hyperlink(sheet.B1, 'https://example.com/first');
    SheetJS.utils.cell_set_hyperlink(sheet.B2, 'https://example.com/second');
    const output = { bookType: 'xlsx', type: 'array' };
    const origin = SheetJS.write(SheetJS.utils.book_new(sheet, 'Link stubs'), output);
    sheet.A1.l = { Target: '#A2', display: 'Jump to details' };
    const labeled = SheetJS.read(SheetJS.write(SheetJS.utils.book_new(sheet, 'Link stubs'), output));
    assert.equal(labeled.Sheets['Link stubs'].A1.v, 1);
    assert.deepEqual(labeled.Sheets['Link stubs'].A1.l, { ref: 'A1', location: 'A2', display: 'Jump to details',
      Target: '#A2', Rel: { Target: '#A2', TargetMode: 'Internal' } });
    const archive = SheetJS.CFB.read(new Uint8Array(origin), { type: 'array' });
    const path = '/xl/worksheets/sheet1.xml';
    const xml = Buffer.from(SheetJS.CFB.find(archive, path).content).toString();
    const sparseXml = xml.replace(/<c r="B[12]"[^>]*>[\s\S]*?<\/c>/g, '')
      .replace(/(<hyperlink ref="B1")/, '$1 tooltip="" display="Original label"');
    assert.notEqual(sparseXml, xml);
    assert.ok(SheetJS.CFB.utils.cfb_del(archive, path));
    SheetJS.CFB.utils.cfb_add(archive, path.slice(1), Buffer.from(sparseXml));
    const bytes = Uint8Array.from(SheetJS.CFB.write(archive, { fileType: 'zip', type: 'array' }));
    const book = SheetJS.read(bytes, { sheetRows: 1 });
    assert.deepEqual(book.Sheets['Link stubs'].B1, { t: 'z', l: { ref: 'B1', Target: 'https://example.com/first', display: 'Original label' } });
    assert.deepEqual(new Uint8Array(SheetJS.write(book, output)), bytes);
    const tooltip = ' Open & review <明細> ';
    const target = 'https://example.com/first';
    SheetJS.utils.cell_set_hyperlink(book.Sheets['Link stubs'].B1, target, tooltip);
    const tooltipBytes = new Uint8Array(SheetJS.write(book, output));
    const withTooltip = SheetJS.read(tooltipBytes).Sheets['Link stubs'];
    assert.deepEqual(withTooltip.B1.l, { ref: 'B1', Target: target, Tooltip: tooltip, display: 'Original label' });
    const tooltipArchive = SheetJS.CFB.read(tooltipBytes, { type: 'array' });
    const relationships = '/xl/worksheets/_rels/sheet1.xml.rels';
    assert.deepEqual(Buffer.from(SheetJS.CFB.find(tooltipArchive, relationships).content),
      Buffer.from(SheetJS.CFB.find(archive, relationships).content));
    const tooltipXml = Buffer.from(SheetJS.CFB.find(tooltipArchive, path).content).toString();
    assert.match(tooltipXml, /<hyperlink\b(?:[^">]|"[^"]*")*\bdisplay="Original label"/);
    const withoutTooltip = SheetJS.read(tooltipBytes);
    delete withoutTooltip.Sheets['Link stubs'].B1.l.Tooltip;
    const withoutTooltipBytes = new Uint8Array(SheetJS.write(withoutTooltip, output));
    assert.deepEqual(SheetJS.read(withoutTooltipBytes).Sheets['Link stubs'].B1.l, { ref: 'B1', Target: target, display: 'Original label' });
    const renamed = SheetJS.read(withoutTooltipBytes);
    const renamedLink = renamed.Sheets['Link stubs'].B1.l;
    Object.defineProperty(renamedLink, 'display', {
      configurable: true,
      get() { assert.fail('Hyperlink display getter was called'); },
    });
    assert.throws(() => SheetJS.write(renamed, output), { code: 'INVALID_VALUE' });
    Object.defineProperty(renamedLink, 'display', { value: 'Original label', writable: true });
    assert.deepEqual(new Uint8Array(SheetJS.write(renamed, output)), withoutTooltipBytes);
    renamedLink.display = 'Invalid\uD800';
    assert.throws(() => SheetJS.write(renamed, output), { code: 'INVALID_VALUE' });
    renamedLink.display = '';
    const withEmptyDisplay = SheetJS.read(SheetJS.write(renamed, output));
    assert.equal(withEmptyDisplay.Sheets['Link stubs'].B1.l.display, '');
    withEmptyDisplay.Sheets['Link stubs'].B1.l.display = undefined;
    const noDisplay = SheetJS.read(SheetJS.write(withEmptyDisplay, output));
    assert.equal(Object.hasOwn(noDisplay.Sheets['Link stubs'].B1.l, 'display'), false);
    const clearedTooltipArchive = SheetJS.CFB.read(withoutTooltipBytes, { type: 'array' });
    assert.deepEqual(Buffer.from(SheetJS.CFB.find(clearedTooltipArchive, relationships).content),
      Buffer.from(SheetJS.CFB.find(archive, relationships).content));
    assert.equal(book.Sheets['Link stubs'].B2, undefined);
    const withFormats = SheetJS.read(bytes, { sheetRows: 1, cellNF: true, cellStyles: true }).Sheets['Link stubs'];
    assert.equal(withFormats.A1.z, 'General');
    assert.equal(Object.hasOwn(withFormats.B1, 'z'), false);
    assert.equal(Object.hasOwn(withFormats.B1, 's'), false);
    assert.equal(book.Sheets['Link stubs']['!ref'], 'A1:B1');
    delete book.Sheets['Link stubs'].B1.l;
    const updated = SheetJS.read(SheetJS.write(book, output), { dense: true }).Sheets['Link stubs']['!data'];
    assert.equal(updated[0][1], undefined);
    assert.equal(updated[1][1].l.Target, 'https://example.com/second');

    const overlapXml = sparseXml.replace(/(<hyperlink ref=")B1(")/, '$1B1:B2$2');
    assert.notEqual(overlapXml, sparseXml);
    assert.ok(SheetJS.CFB.utils.cfb_del(archive, path));
    SheetJS.CFB.utils.cfb_add(archive, path.slice(1), Buffer.from(overlapXml));
    const overlapBytes = Uint8Array.from(SheetJS.CFB.write(archive, { fileType: 'zip', type: 'array' }));
    const overlap = SheetJS.read(overlapBytes);
    assert.equal(overlap.Sheets['Link stubs'].B1.l.Target, 'https://example.com/first');
    assert.equal(overlap.Sheets['Link stubs'].B2.l.Target, 'https://example.com/second');
    assert.deepEqual(new Uint8Array(SheetJS.write(overlap, output)), overlapBytes);

    const rangeXml = sparseXml.replace(/(<hyperlink ref=")B1(")/, '$1B1:B2$2')
      .replace(/<hyperlink ref="B2"[^>]*\/>/, '');
    assert.notEqual(rangeXml, sparseXml);
    assert.ok(SheetJS.CFB.utils.cfb_del(archive, path));
    SheetJS.CFB.utils.cfb_add(archive, path.slice(1), Buffer.from(rangeXml));
    const rangeBytes = Uint8Array.from(SheetJS.CFB.write(archive, { fileType: 'zip', type: 'array' }));
    const rangeBook = SheetJS.read(rangeBytes);
    assert.equal(rangeBook.Sheets['Link stubs'].B2.l.Target, 'https://example.com/first');
    assert.equal(SheetJS.read(rangeBytes, { sheetRows: 1 }).Sheets['Link stubs'].B2, undefined);
    assert.equal(SheetJS.read(rangeBytes, { dense: true }).Sheets['Link stubs']['!data'][1][1].t, 'z');
    assert.deepEqual(new Uint8Array(SheetJS.write(rangeBook, output)), rangeBytes);
    delete rangeBook.Sheets['Link stubs'].B2.l;
    assert.throws(() => SheetJS.write(rangeBook, output), { code: 'UNSUPPORTED_CELL' });
  }

  {
    // Expected values are CE 0.20.3's for the same patched packages.
    const output = { bookType: 'xlsx', type: 'array' };
    const patch = (bytes, edits) => {
      const archive = SheetJS.CFB.read(new Uint8Array(bytes), { type: 'array' });
      for (const [path, edit] of Object.entries(edits)) {
        const entry = SheetJS.CFB.find(archive, '/' + path);
        const text = edit(entry ? Buffer.from(entry.content).toString() : '');
        if (entry) assert.ok(SheetJS.CFB.utils.cfb_del(archive, '/' + path));
        SheetJS.CFB.utils.cfb_add(archive, path, Buffer.from(text));
      }
      return Uint8Array.from(SheetJS.CFB.write(archive, { fileType: 'zip', type: 'array' }));
    };
    const at = (sheet, key) => {
      const { r, c } = SheetJS.utils.decode_cell(key);
      return sheet['!data'] ? sheet['!data'][r]?.[c] : sheet[key];
    };

    // cellNF projects z on shared rich, inline rich and phonetic string cells.
    const strings = patch(SheetJS.write(SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([['x']]), 'R'), output), {
      'xl/sharedStrings.xml': () => '<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3">' +
        '<si><t>plain</t></si><si><r><rPr><b/></rPr><t>Bo</t></r><r><t>ld</t></r></si><si><t>漢</t><rPh sb="0" eb="1"><t>カン</t></rPh></si></sst>',
      'xl/worksheets/sheet1.xml': (xml) => xml.replace(/<sheetData>.*<\/sheetData>/, '<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c>' +
        '<c r="B1" t="s"><v>1</v></c><c r="C1" t="inlineStr"><is><r><rPr><i/></rPr><t>It</t></r></is></c><c r="D1" t="s"><v>2</v></c></row></sheetData>')
        .replace(/<dimension ref="[^"]*"/, '<dimension ref="A1:D1"'),
      '[Content_Types].xml': (xml) => xml.includes('sharedStrings') ? xml : xml.replace('</Types>',
        '<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>'),
      'xl/_rels/workbook.xml.rels': (xml) => xml.includes('sharedStrings') ? xml : xml.replace('</Relationships>',
        '<Relationship Id="rId99" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>'),
    });
    for (const dense of [false, true]) {
      const sheet = SheetJS.read(strings, { cellNF: true, dense }).Sheets.R;
      assert.deepEqual(['A1', 'B1', 'C1', 'D1'].map((key) => [at(sheet, key).v, at(sheet, key).z]),
        [['plain', 'General'], ['Bold', 'General'], ['It', 'General'], ['漢', 'General']]);
    }
    assert.equal(Object.hasOwn(SheetJS.read(strings).Sheets.R.B1, 'z'), false);

    // Hyperlinks: ref, internal location/Rel, one object per element, nodim keeps the data extent.
    const links = patch(SheetJS.write(SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([['a', 'b'], [1, 2], [3, 4]]), 'H'), output), {
      'xl/worksheets/sheet1.xml': (xml) => xml.replace('</sheetData>', '</sheetData><hyperlinks><hyperlink ref="A1:B2" r:id="rId9"/>' +
        '<hyperlink ref="D5" location="H!A1" display="go"/><hyperlink ref="A3" location="H!B1" tooltip="tip"/></hyperlinks>'),
      'xl/worksheets/_rels/sheet1.xml.rels': () => '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
        '<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://e.com/a" TargetMode="External"/></Relationships>',
    });
    const internal = { ref: 'A3', location: 'H!B1', Target: '#H!B1', Rel: { Target: '#H!B1', TargetMode: 'Internal' }, Tooltip: 'tip' };
    const stub = { t: 'z', l: { ref: 'D5', location: 'H!A1', display: 'go', Target: '#H!A1', Rel: { Target: '#H!A1', TargetMode: 'Internal' } } };
    for (const options of [{}, { dense: true }, { nodim: true }]) {
      const sheet = SheetJS.read(links, options).Sheets.H;
      assert.equal(sheet['!ref'], 'A1:B3');
      assert.equal(at(sheet, 'A1').l, at(sheet, 'B2').l);
      assert.equal(at(sheet, 'A1').l.ref, 'A1:B2');
      assert.equal(at(sheet, 'A1').l.Target, 'https://e.com/a');
      assert.deepEqual(at(sheet, 'A3').l, internal);
      // CE 0.20.3 also stubs D5 outside !ref without options; this adapter clips link stubs to !ref.
      if (options.nodim) assert.deepEqual(at(sheet, 'D5'), stub);
      else assert.equal(at(sheet, 'D5'), undefined);
    }
    const linkBook = SheetJS.read(links);
    assert.deepEqual(new Uint8Array(SheetJS.write(linkBook, output)), links);
    linkBook.Sheets.H.A1.l.Target = 'https://e.com/b';
    assert.throws(() => SheetJS.write(linkBook, output), { code: 'UNSUPPORTED_CELL' });
    const shaped = SheetJS.utils.aoa_to_sheet([['x', 'y']]);
    shaped.A1.l = { ref: 'A1:B2', id: 'rId9', Target: 'https://e.com/a', uid: '{U}',
      Rel: { Type: 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink', Target: 'https://e.com/a', Id: 'rId9', TargetMode: 'External' } };
    shaped.B1.l = { ...internal, location: 'W!A1', Target: '#W!A1' };
    const written = SheetJS.read(SheetJS.write(SheetJS.utils.book_new(shaped, 'W'), output)).Sheets.W;
    assert.deepEqual(written.A1.l, { ref: 'A1', Target: 'https://e.com/a' });
    assert.deepEqual(written.B1.l, { ref: 'B1', location: 'W!A1', Target: '#W!A1', Rel: { Target: '#W!A1', TargetMode: 'Internal' }, Tooltip: 'tip' });

    // A comment on a missing cell widens !ref; VML-only Note shapes stub without widening it.
    const noted = SheetJS.utils.aoa_to_sheet([['a', 'b'], [1, null], [3, 4]]);
    SheetJS.utils.cell_add_comment(noted.A1, 'near', 'X');
    const shape = (xml, row, column, id) => xml.match(/<v:shape [\s\S]*?<\/v:shape>/)[0]
      .replace('<x:Row>0</x:Row><x:Column>0</x:Column>', `<x:Row>${row}</x:Row><x:Column>${column}</x:Column>`).replaceAll('s1025', id);
    const notes = patch(SheetJS.write(SheetJS.utils.book_new(noted, 'N'), output), {
      'xl/comments1.xml': (xml) => xml.replace('</commentList>', '<comment ref="D10" authorId="0"><text><t>far</t></text></comment></commentList>'),
      'xl/drawings/vmlDrawing1.vml': (xml) => xml.replace('</xml>',
        `${shape(xml, 9, 3, 's1026')}${shape(xml, 1, 5, 's1027')}${shape(xml, 1, 1, 's1028')}${shape(xml, 2, 0, 's1029')}</xml>`),
    });
    const far = { t: 'z', c: [{ a: 'X', t: 'far', r: '<t>far</t>', T: false, h: 'far' }] };
    far.c.hidden = true;
    for (const options of [{}, { dense: true }, { nodim: true }]) {
      const sheet = SheetJS.read(notes, options).Sheets.N;
      assert.equal(sheet['!ref'], 'A1:D10');
      assert.deepEqual(at(sheet, 'D10'), far);
      assert.equal(at(sheet, 'D10').c.hidden, true);
      assert.deepEqual(at(sheet, 'B2'), { t: 'z' });
      // CE 0.20.3 also stubs F2 (outside the dimension) without options; clipped here like links.
      if (options.nodim) assert.deepEqual(at(sheet, 'F2'), { t: 'z' });
      else assert.equal(at(sheet, 'F2'), undefined);
    }
    const limited = SheetJS.read(notes, { sheetRows: 2 }).Sheets.N;
    assert.deepEqual([limited['!ref'], limited['!fullref'], limited.D10, limited.B2], ['A1:B2', 'A1:B3', undefined, { t: 'z' }]);
    const noteBook = SheetJS.read(notes);
    assert.deepEqual(new Uint8Array(SheetJS.write(noteBook, output)), notes);
    SheetJS.utils.cell_add_comment(noteBook.Sheets.N.D10, 'more', 'Y');
    assert.throws(() => SheetJS.write(noteBook, output), { code: 'UNSUPPORTED_RANGE' });
  }

  const xlmlFormatted = SheetJS.read(xlmlNumberFormats, { type: 'string', cellNF: true }).Sheets.Data;
  assert.deepEqual(['A1', 'B1', 'A2'].map(key => [xlmlFormatted[key].z, xlmlFormatted[key].w]),
    [['0.00', '1.00'], ['0.0', '1.0'], ['0%', '100%']]);
  const xlmlVisibility = xlmlColumns
    .replace('<Column ss:Width="8"/>', '<Column ss:Width="8" ss:Hidden="true"/>')
    .replace('<Row><Cell>', '<Row ss:Height="14.5" ss:Hidden="1"><Cell>');
  const xlmlVisibleLayout = SheetJS.read(xlmlVisibility, { type: 'string' }).Sheets.Data;
  assert.equal(xlmlVisibleLayout['!cols'][0].hidden, true);
  assert.deepEqual(xlmlVisibleLayout['!rows'][0], { hpt: 14, hpx: 14, hidden: true });
  const xlmlDates = xlmlColumns.replace('<Data ss:Type="Number">1</Data>',
    '<Data ss:Type="DateTime">2024-01-02T12:30:00</Data>');
  const xlmlDate = SheetJS.read(xlmlDates, { type: 'string', cellDates: true, cellNF: true }).Sheets.Data.A1;
  assert.equal(xlmlDate.v.toISOString(), '2024-01-02T12:30:00.000Z');
  assert.equal(xlmlDate.z, 'yyyy-mm-dd');
  assert.equal(xlmlDate.w, '2024-01-02');
  const xlml1904 = xlmlDates
    .replace('<Worksheet', '<ExcelWorkbook xmlns="urn:schemas-microsoft-com:office:excel"><Date1904/></ExcelWorkbook>' +
      '<Styles><Style ss:ID="Numeric"><NumberFormat ss:Format="0.00"/></Style></Styles><Worksheet')
    .replace('<Row><Cell>', '<Row><Cell ss:StyleID="Numeric">')
    .replace('2024-01-02T12:30:00', '1904-01-02T12:00:00');
  const xlmlNumericDate = SheetJS.read(xlml1904, { type: 'string', cellDates: true, cellNF: true }).Sheets.Data.A1;
  assert.deepEqual([xlmlNumericDate.t, xlmlNumericDate.v, xlmlNumericDate.w, xlmlNumericDate.z],
    ['n', 1.5, '1.50', '0.00']);
  const xlmlSingleArray = xlmlColumns
    .replace('<Cell>', '<Cell ss:Formula="=1+2" ss:ArrayRange="RC">');
  const xlmlArrayCell = SheetJS.read(xlmlSingleArray, { type: 'string' }).Sheets.Data.A1;
  assert.equal(xlmlArrayCell.f, '1+2');
  assert.equal(xlmlArrayCell.F, 'A1:A1');
  const xlmlComments = xlmlColumns.replace('</Cell>',
    '<Comment ss:Author="Ada" ss:ShowAlways="1"><ss:Data>Visible</ss:Data></Comment>' +
    '<Comment ss:Author="Ben"><ss:Data>Hidden</ss:Data></Comment></Cell>');
  const xlmlCommentBook = SheetJS.read(xlmlComments, { type: 'string' });
  const xlmlNotes = xlmlCommentBook.Sheets.Data.A1.c;
  assert.deepEqual(Array.from(xlmlNotes), [{ a: 'Ada', t: 'Visible' }, { a: 'Ben', t: 'Hidden' }]);
  assert.equal(xlmlNotes.hidden, true);
  const xlmlCommentOutput = SheetJS.write(xlmlCommentBook, { bookType: 'xlml', type: 'string' });
  assert.equal(xlmlCommentOutput.includes('ss:ShowAlways'), false);
  const xlmlPrintNames = xlmlColumns.replace('<Worksheet', '<Names>' +
    '<NamedRange ss:Name="Global" ss:RefersTo="=Data!R2C1"/></Names><Worksheet')
    .replace('<Table>', '<Names>' +
    '<NamedRange ss:Name="Print_Area" ss:RefersTo="=Data!R1C1:R2C2"/>' +
    '<NamedRange ss:Name="Print_Titles" ss:RefersTo="=Data!$1:$2,Data!$A:$B"/>' +
    '<NamedRange ss:Name="Custom" ss:RefersTo="=Data!R1C1"/></Names><Table>');
  const xlmlNamedBook = SheetJS.read(xlmlPrintNames, { type: 'string' });
  assert.deepEqual(xlmlNamedBook.Workbook.Names, [
    { Name: 'Global', Ref: 'Data!$A$2' },
    { Name: '_xlnm.Print_Area', Ref: 'Data!$A$1:$B$2', Sheet: 0 },
    { Name: '_xlnm.Print_Titles', Ref: 'Data!$1:$2,Data!$A:$B', Sheet: 0 },
    { Name: 'Custom', Ref: 'Data!$A$1', Sheet: 0 },
  ]);
  const xlmlNamedOutput = SheetJS.write(xlmlNamedBook, { bookType: 'xlml', type: 'string' });
  assert.match(xlmlNamedOutput, /ss:Name="Print_Area"/);
  assert.deepEqual(SheetJS.read(xlmlNamedOutput, { type: 'string' }).Workbook.Names, xlmlNamedBook.Workbook.Names);
  const xlmlMargins = xlmlColumns.replace('</Worksheet>',
    '<WorksheetOptions xmlns="urn:schemas-microsoft-com:office:excel"><PageSetup>' +
    '<Header Margin="0"/><PageMargins Left="0.25"/></PageSetup></WorksheetOptions></Worksheet>');
  assert.deepEqual(SheetJS.read(xlmlMargins, { type: 'string' }).Sheets.Data['!margins'],
    { left: 0.25, right: 1, top: 1, bottom: 1, header: 0, footer: 0.5 });
  const xlmlRtl = xlmlColumns.replace('</Worksheet>',
    '<WorksheetOptions xmlns="urn:schemas-microsoft-com:office:excel">' +
    '<DisplayRightToLeft/></WorksheetOptions></Worksheet>');
  const xlmlRtlBook = SheetJS.read(xlmlRtl, { type: 'string' });
  assert.deepEqual(xlmlRtlBook.Workbook.Views, [{ RTL: true }]);
  const xlmlRtlOutput = SheetJS.write(xlmlRtlBook, { bookType: 'xlml', type: 'string' });
  assert.match(xlmlRtlOutput, /<DisplayRightToLeft\/>/);
  assert.deepEqual(SheetJS.read(xlmlRtlOutput, { type: 'string' }).Workbook.Views, [{ RTL: true }]);
  const xlmlCustom = xlmlColumns.replace('<Worksheet', '<CustomDocumentProperties ' +
    'xmlns="urn:schemas-microsoft-com:office:office" xmlns:dt="uuid:C2F41010-65B3-11d1-A29F-00AA00C14882">' +
    '<Small dt:dt="i2">-12</Small><Fraction dt:dt="r4">1.25</Fraction>' +
    '<Calendar dt:dt="date">2024-01-02</Calendar><Large dt:dt="i8">9007199254740993</Large>' +
    '</CustomDocumentProperties><Worksheet');
  assert.deepEqual(SheetJS.read(xlmlCustom, { type: 'string' }).Custprops, {
    Small: -12, Fraction: 1.25, Calendar: new Date('2024-01-02T00:00:00Z'), Large: '9007199254740993',
  });
  const xlmlCustomNames = SheetJS.read(xlmlColumns, { type: 'string' });
  xlmlCustomNames.Custprops = { 'A B': 'space', A_x0020_B: 'literal escape', '😀': 'emoji' };
  const xlmlCustomNameOutput = SheetJS.write(xlmlCustomNames, { bookType: 'xlml', type: 'string' });
  assert.match(xlmlCustomNameOutput, /<A_x005f_x0020_B /);
  assert.deepEqual(SheetJS.read(xlmlCustomNameOutput, { type: 'string' }).Custprops, xlmlCustomNames.Custprops);
  const xlmlProps = SheetJS.read(xlmlColumns, { type: 'string' });
  xlmlProps.Props = { Title: 'Keep', Subject: 'Also keep', Author: 'Old' };
  const xlmlPropsOutput = SheetJS.write(xlmlProps, {
    bookType: 'xlml', type: 'string', Props: { Title: undefined, Subject: null, Author: 'New' },
  });
  assert.deepEqual(SheetJS.read(xlmlPropsOutput, { type: 'string' }).Props,
    { Title: 'Keep', Subject: 'Also keep', Author: 'New' });
  assert.deepEqual(xlmlProps.Props, { Title: 'Keep', Subject: 'Also keep', Author: 'Old' });
  const xlmlLeadingRows = xlmlColumns.replace('<Row><Cell>',
    '<Row ss:Index="2"></Row><Row ss:Index="4" ss:Height="14" ss:Hidden="1"><Cell ss:Index="2">');
  const xlmlLimited = SheetJS.read(xlmlLeadingRows, { type: 'string', sheetRows: 3 }).Sheets.Data;
  assert.equal(xlmlLimited['!ref'], 'B2:B3');
  assert.equal(xlmlLimited['!fullref'], 'B2:B4');
  assert.equal(xlmlLimited.B4, undefined);
  assert.deepEqual(xlmlLimited['!rows'][3], { hpx: 14, hpt: 14, hidden: true });
  assert.equal(SheetJS.read(xlmlLeadingRows, { type: 'string' }).Sheets.Data['!ref'], 'B2:B4');
  const xlmlMerged = xlmlColumns.replace('<Cell>', '<Cell ss:MergeAcross="1" ss:MergeDown="1">');
  const xlmlMergedSheet = SheetJS.read(xlmlMerged, { type: 'string', sheetStubs: true }).Sheets.Data;
  assert.equal(xlmlMergedSheet.A1.v, 1);
  for (const key of ['B1', 'A2', 'B2']) assert.deepEqual(xlmlMergedSheet[key], { t: 'z' });
  assert.equal(xlmlMergedSheet['!ref'], 'A1');
  const xlmlDenseMerged = SheetJS.read(xlmlMerged, {
    type: 'string', dense: true, sheetRows: 1, sheetStubs: true,
  }).Sheets.Data;
  assert.deepEqual(xlmlDenseMerged['!data'][0][1], { t: 'z' });
  assert.equal(xlmlDenseMerged['!data'][1], undefined);
  assert.equal(SheetJS.read(xlmlMerged, { type: 'string' }).Sheets.Data.B1, undefined);
  const xlmlEmptyCells = xlmlColumns.replace('</Row>',
    '<Cell ss:Index="4"/><Cell ss:Index="5"></Cell></Row>');
  for (const sheetStubs of [false, true]) {
    const xlmlEmptySheet = SheetJS.read(xlmlEmptyCells, { type: 'string', sheetStubs }).Sheets.Data;
    assert.equal(xlmlEmptySheet['!ref'], 'A1:E1');
    assert.equal(xlmlEmptySheet.D1, undefined);
    assert.equal(xlmlEmptySheet.E1, undefined);
  }
  const xlmlSelection = xlmlColumns.replace('</Workbook>',
    '<Worksheet ss:Name="Other"><Table><Row><Cell><Data ss:Type="Number">2</Data></Cell></Row></Table>' +
    '</Worksheet></Workbook>');
  for (const sheets of ['Other', 1, ['other', 0], [], 99]) {
    const selected = SheetJS.read(xlmlSelection, { type: 'string', sheets });
    assert.deepEqual(selected.SheetNames, ['Data', 'Other']);
    assert.deepEqual(Object.values(selected.Sheets).map(sheet => sheet.A1.v), [1, 2]);
  }
  const html = '<table><tr><td rowspan="2">日本語 &amp; tea<td data-v="23">ignored' +
    '<tr><td>a<br>b</table><table><tr><td>42</table>';
  const hiddenHtml = SheetJS.read('<table><tr hidden><td>Hidden</td></tr>' +
    '<tr hidden style="display:table-row"><td>Visible</td></tr></table>', { type: 'string' });
  assert.equal(hiddenHtml.Sheets.Sheet1['!rows'][0].hidden, true);
  assert.equal(hiddenHtml.Sheets.Sheet1['!rows'][1], undefined);
  assert.equal(hiddenHtml.Sheets.Sheet1.A1.v, 'Hidden');
  const whitespaceHtml = SheetJS.read(
    '<table><tr><td>  A  B\nC \n<br>  D&nbsp;&nbsp;E  </td><td data-v="  literal  ">ignored</td></tr></table>',
    { type: 'string' },
  );
  assert.equal(whitespaceHtml.Sheets.Sheet1.A1.v, 'A B C\nD  E');
  assert.equal(whitespaceHtml.Sheets.Sheet1.B1.v, '  literal  ');
  const htmlBook = SheetJS.read(html, { type: 'string' });
  assert.deepEqual(htmlBook.SheetNames, ['Sheet1', 'Sheet2']);
  assert.equal(htmlBook.Sheets.Sheet1.A1.v, '日本語 & tea');
  assert.equal(htmlBook.Sheets.Sheet1.B1.v, 23);
  assert.equal(htmlBook.Sheets.Sheet1.B2.v, 'a\nb');
  assert.deepEqual(htmlBook.Sheets.Sheet1['!merges'], [{ s: { r: 0, c: 0 }, e: { r: 1, c: 0 } }]);
  assert.equal(htmlBook.Sheets.Sheet2.A1.v, 42);

  const iteration = new Workbook().addWorksheet('Iteration');
  iteration.getCell('C3').value = 7;
  const visited = visit => {
    const result = [];
    visit((_handle, index) => result.push(index));
    return result;
  };
  assert.deepEqual(visited(callback => iteration.eachRow(callback)), [3]);
  assert.deepEqual(visited(callback => iteration.eachRow({ includeEmpty: true }, callback)), [1, 2, 3]);
  assert.deepEqual(visited(callback => iteration.getRow(3).eachCell(callback)), [3]);
  assert.deepEqual(visited(callback => iteration.getRow(3).eachCell({ includeEmpty: true }, callback)), [1, 2, 3]);
  assert.deepEqual(visited(callback => iteration.getColumn(3).eachCell({ includeEmpty: undefined }, callback)), [3]);
  assert.deepEqual(visited(callback => iteration.getColumn(3).eachCell({ includeEmpty: true }, callback)), [1, 2, 3]);

  const mergedIteration = new Workbook().addWorksheet('Merged iteration');
  mergedIteration.getCell('A2').numFmt = '0.00';
  mergedIteration.mergeCells('B4:C5');
  mergedIteration.mergeCells('D7:D8');
  assert.equal(mergedIteration.getRow(2).hasValues, false);
  assert.deepEqual(visited(callback => mergedIteration.eachRow(callback)), [4, 5, 8]);
  mergedIteration.getCell('D7').value = 9;
  assert.deepEqual(visited(callback => mergedIteration.eachRow(callback)), [4, 5, 7, 8]);

  const censusBook = new Workbook();
  const census = censusBook.addWorksheet('Stored');
  census.getCell('A2').numFmt = '0.00';
  census.getCell('D3').value = { formula: '1+1' };
  census.getRow(5).height = 21;
  const stored = (await new Workbook().xlsx.load(await censusBook.xlsx.writeBuffer())).getWorksheet('Stored');
  assert.deepEqual(visited(callback => stored.eachRow(callback)), [3]);
  assert.equal(stored.getRow(2).hasValues, false);
  assert.equal(stored.getRow(2).cellCount, 1);
  assert.equal(stored.findRow(5).height, 21);
  assert.deepEqual(visited(callback => stored.getRow(3).eachCell(callback)), [4]);

  const fillBook = new Workbook();
  const fillSource = fillBook.addWorksheet('Fill batch');
  fillSource.getCell('A1').value = 1;
  fillSource.getCell('B1').value = 2;
  fillSource.getCell('C1').value = 3;
  fillSource.getCell('B1').fill = { type: 'pattern', pattern: 'solid', fgColor: { argb: 'FF336699' } };
  fillSource.getCell('C1').fill = { type: 'pattern', pattern: 'solid', fgColor: { argb: 'FF993366' } };
  const fillBytes = await fillBook.xlsx.writeBuffer();
  for (const dense of [false, true]) {
    const filled = SheetJS.read(fillBytes, { dense, cellStyles: true }).Sheets['Fill batch'];
    const cells = dense ? filled['!data'][0] : [filled.A1, filled.B1, filled.C1];
    assert.deepEqual(cells.map(cell => cell.s.patternType), ['none', 'solid', 'solid']);
    assert.equal(cells[1].s.fgColor.rgb, '336699');
    assert.equal(cells[2].s.fgColor.rgb, '993366');
  }

  const workbook = new Workbook();
  assert.equal(await workbook.xlsx.readFile(source), workbook);
  workbook.getWorksheet('Sheet1').getCell('B1').value = 'Node';
  const edited = join(directory, 'excel.xlsx');
  const zipOptions = { zip: { compression: 'STORE' } };
  await workbook.xlsx.writeFile(edited, zipOptions);
  assert.equal(fs.readFileSync(edited).readUInt16LE(8), 0);
  assert.ok(Buffer.isBuffer(await workbook.xlsx.writeBuffer()));
  const chunks = [];
  await workbook.xlsx.write(new Writable({
    write(chunk, _encoding, callback) {
      chunks.push(chunk);
      callback();
    },
  }), zipOptions);
  assert.deepEqual(Buffer.concat(chunks), fs.readFileSync(edited));
  await assert.rejects(workbook.xlsx.readFile(join(directory, 'missing.xlsx')));
  assert.equal(workbook.getWorksheet('Sheet1').getCell('B1').value, 'Node');
  const sheetBook = SheetJS.read(edited, { type: 'file' });
  assert.equal(sheetBook.Sheets.Sheet1.B1.v, 'Node');

  const pictureBook = new Workbook();
  const pictures = pictureBook.addWorksheet('Pictures');
  const filename = join(directory, 'picture.png');
  for (const invalid of [
    { filename, extension: 'bmp' }, { filename: '', extension: 'png' },
    { filename, extension: 'png', buffer: Buffer.alloc(0) },
    { filename, extension: 'png', base64: '' },
  ]) {
    assert.throws(() => pictureBook.addImage(invalid), {
      code: 'INVALID_VALUE', message: 'Expected a PNG, JPEG or GIF filename as the only image source',
    });
  }
  const imageId = pictureBook.addImage({ filename, extension: 'png' });
  assert.equal(imageId, 0);
  assert.deepEqual(pictureBook.getImage(imageId), { type: 'image', extension: 'png', filename });
  pictures.addImage(imageId, 'A1:B2');
  const duplicateImage = pictureBook.addImage({ filename, extension: 'png' });
  pictures.addImage(duplicateImage, 'C1:D2');
  pictures.addBackgroundImage(imageId);
  await assert.rejects(pictureBook.xlsx.writeBuffer(), { code: 'ENOENT' });
  const fileModelBook = new Workbook();
  fileModelBook.model = pictureBook.model;
  const fileModelSheet = fileModelBook.getWorksheet('Pictures');
  assert.deepEqual(fileModelSheet.getImages().map(image => image.imageId), [imageId, duplicateImage]);
  assert.equal(fileModelSheet.getBackgroundImageId(), imageId);
  assert.deepEqual(fileModelBook.getImage(imageId), pictureBook.getImage(imageId));
  await assert.rejects(fileModelBook.xlsx.writeBuffer(), { code: 'ENOENT' });
  const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=', 'base64');
  fs.writeFileSync(filename, png);
  const snapshotCell = pictures.getCell('G1');
  snapshotCell.value = 'At save';
  const pendingImageSave = pictureBook.xlsx.writeBuffer();
  snapshotCell.value = 'After save started';
  const imageSnapshot = await new Workbook().xlsx.load(await pendingImageSave);
  assert.equal(imageSnapshot.worksheets[0].getCell('G1').value, 'At save');
  assert.equal(snapshotCell.value, 'After save started');
  for (const bytes of [png, Buffer.concat([png, Buffer.of(0)])]) {
    fs.writeFileSync(filename, bytes);
    const modelReloaded = await new Workbook().xlsx.load(await fileModelBook.xlsx.writeBuffer());
    const modelPictures = modelReloaded.getWorksheet('Pictures');
    for (const id of [...modelPictures.getImages().map(image => image.imageId), modelPictures.getBackgroundImageId()]) {
      assert.deepEqual(Buffer.from(modelReloaded.getImage(id).buffer), bytes);
    }
    const output = join(directory, 'pictures.xlsx');
    await pictureBook.xlsx.writeFile(output, { zip: { compression: 'STORE' } });
    const reopened = await new Workbook().xlsx.readFile(output);
    const sheet = reopened.getWorksheet('Pictures');
    assert.equal(sheet.getImages().length, 2);
    for (const id of [...sheet.getImages().map(image => image.imageId), sheet.getBackgroundImageId()]) {
      assert.deepEqual(Buffer.from(reopened.getImage(id).buffer), bytes);
    }
  }

  const backgroundWriter = new stream.xlsx.WorkbookWriter();
  const backgroundFile = join(directory, 'stream-background.png');
  fs.writeFileSync(backgroundFile, png);
  const backgroundSheet = backgroundWriter.addWorksheet('Background');
  const fileBackground = backgroundWriter.addImage({ filename: backgroundFile, extension: 'png' });
  assert.equal(backgroundWriter.getImage(fileBackground).filename, backgroundFile);
  backgroundSheet.addBackgroundImage(fileBackground);
  assert.equal(backgroundSheet.getBackgroundImageId(), fileBackground);
  backgroundSheet.addRow(['Retained streamed row']).commit();
  backgroundSheet.commit();

  const otherSheet = backgroundWriter.addWorksheet('Second background');
  const base64Background = backgroundWriter.addImage({ base64: png.toString('base64'), extension: 'png' });
  assert.equal(backgroundWriter.getImage(base64Background).name, 'image1.png');
  otherSheet.addBackgroundImage(base64Background);
  await backgroundWriter.commit();
  const backgroundResult = await new Workbook().xlsx.load(backgroundWriter.stream.read());
  assert.equal(backgroundResult.getWorksheet('Background').getCell('A1').value, 'Retained streamed row');
  for (const sheet of backgroundResult.worksheets) {
    assert.deepEqual(Buffer.from(backgroundResult.getImage(sheet.getBackgroundImageId()).buffer), png);
  }

  const validationWriter = new stream.xlsx.WorkbookWriter();
  const streamValidationSheet = validationWriter.addWorksheet('Stream validations');
  const validationRow = streamValidationSheet.addRow([3]);
  const constrainedCell = validationRow.getCell(1);
  validationRow.commit();
  const wholeNumbers = { type: 'whole', operator: 'between', formulae: [1, 5], showErrorMessage: true };
  constrainedCell.dataValidation = wholeNumbers;
  assert.deepEqual(constrainedCell.dataValidation.formulae, [1, 5]);
  assert.throws(() => { constrainedCell.dataValidation = { type: 'invalid' }; });
  assert.deepEqual(constrainedCell.dataValidation.formulae, [1, 5]);
  streamValidationSheet.getCell('D2').dataValidation = wholeNumbers;
  streamValidationSheet.getCell('D2').dataValidation = null;
  streamValidationSheet.dataValidations.add('B2:B4', { type: 'list', formulae: ['"Open,Closed"'] });
  streamValidationSheet.commit();
  assert.throws(() => { constrainedCell.dataValidation = null; }, /already committed/);
  await validationWriter.commit();
  const streamedValidationBytes = validationWriter.stream.read();
  const validationReadBack = await new Workbook().xlsx.load(streamedValidationBytes);
  const readValidationSheet = validationReadBack.getWorksheet('Stream validations');
  assert.equal(readValidationSheet.getCell('A1').value, 3);
  assert.deepEqual(readValidationSheet.getCell('A1').dataValidation.formulae, [1, 5]);
  assert.deepEqual(readValidationSheet.getCell('B4').dataValidation.formulae, ['"Open,Closed"']);
  assert.equal(readValidationSheet.getCell('D2').dataValidation, undefined);

  const createdAt = new Date('2024-02-03T04:05:06Z');
  const modifiedAt = new Date('2024-02-04T05:06:07Z');
  const printedAt = new Date('2024-02-05T06:07:08Z');
  const noteWriter = new stream.xlsx.WorkbookWriter({ useSharedStrings: true });
  const streamedNoteSheet = noteWriter.addWorksheet('Streamed notes');
  const blankStatusSheet = noteWriter.addWorksheet('Blank status');
  blankStatusSheet.getRow(1).commit();
  assert.equal(blankStatusSheet.startedData, true);
  assert.equal(blankStatusSheet.hasComments, false);
  const clearedStatusSheet = noteWriter.addWorksheet('Cleared status');
  const clearedStatusCell = clearedStatusSheet.getCell('A1');
  clearedStatusCell.value = 1;
  clearedStatusCell.note = 'Pending';
  clearedStatusCell.note = null;
  clearedStatusSheet.getRow(1).commit();
  assert.equal(clearedStatusSheet.startedData, true);
  assert.equal(clearedStatusSheet.hasComments, false);
  const standaloneNoteWriter = new stream.xlsx.WorkbookWriter();
  const standaloneNoteSheet = standaloneNoteWriter.addWorksheet('Only note');
  standaloneNoteSheet.getCell('A1').note = 'Retained';
  const invalidStatusCell = standaloneNoteSheet.getCell('B1');
  invalidStatusCell.value = new Date(NaN);
  assert.throws(() => standaloneNoteSheet.getRow(1).commit(), { code: 'INVALID_VALUE' });
  assert.equal(standaloneNoteSheet.startedData, false);
  assert.equal(standaloneNoteSheet.hasComments, false);
  invalidStatusCell.value = null;
  standaloneNoteSheet.getRow(1).commit();
  assert.equal(standaloneNoteSheet.startedData, true);
  assert.equal(standaloneNoteSheet.hasComments, true);
  await standaloneNoteWriter.commit();
  const standaloneNoteRead = await new Workbook().xlsx.load(standaloneNoteWriter.stream.read());
  assert.equal(standaloneNoteRead.getWorksheet('Only note').getCell('A1').note, 'Retained');
  const noteDate = new Date('2025-03-15T00:00:00Z');
  const noteAtEdgeBook = new Workbook();
  const noteAtEdgeSheet = noteAtEdgeBook.addWorksheet('Note at edge');
  noteAtEdgeSheet.getCell('XFD1').note = 'Last column';
  const noteAtEdgeBytes = await noteAtEdgeBook.xlsx.writeBuffer();
  assert.throws(() => noteAtEdgeSheet.getRow(1).splice(1, 0, 7), { code: 'INVALID_ADDRESS' });
  assert.equal(noteAtEdgeSheet.getCell('XFD1').note, 'Last column');
  assert.deepEqual(await noteAtEdgeBook.xlsx.writeBuffer(), noteAtEdgeBytes);
  noteAtEdgeSheet.getRow(1).splice(1, 1);
  assert.equal(noteAtEdgeSheet.getCell('XFC1').note, 'Last column');
  assert.equal(noteAtEdgeSheet.getCell('XFD1').note, undefined);

  const noteRow = streamedNoteSheet.addRow(['Cell with note', 'Cell with rich note', null, null, noteDate,
    { formula: 'DATE(2025,3,15)', result: noteDate }]);
  noteRow.getCell(1).note = 'Check amount';
  noteRow.getCell(2).note = { texts: [{ text: 'Rich note', font: { bold: true } }] };
  noteRow.getCell(3).note = 'Discarded';
  noteRow.getCell(3).note = null;
  noteRow.getCell(4).note = 'Note without a value';
  const noteOnlyRow = streamedNoteSheet.getRow(3);
  noteOnlyRow.getCell(2).note = 'Note on an otherwise empty row';
  noteOnlyRow.values = [];
  assert.equal(noteRow.getCell(1).note, 'Check amount');
  assert.equal(streamedNoteSheet.startedData, false);
  assert.equal(streamedNoteSheet.hasComments, false);
  assert.throws(() => streamedNoteSheet.destroy(), /Invalid Operation: destroy/);
  noteRow.commit();
  assert.equal(streamedNoteSheet.startedData, true);
  assert.equal(streamedNoteSheet.hasComments, true);
  assert.throws(() => { noteRow.getCell(1).note = 'Too late'; }, /has been committed/);
  await noteWriter.commit();
  const streamedNoteBytes = noteWriter.stream.read();
  const streamedNoteZip = new XlsxZipReader(streamedNoteBytes.length,
    (offset, length) => streamedNoteBytes.subarray(offset, offset + length));
  try {
    const xml = Buffer.from(streamedNoteZip.read_metadata('xl/worksheets/sheet1.xml', 65536)).toString();
    assert.match(xml, /<c r="D1"[^>]* s="[1-9][0-9]*"/);
    assert.match(xml, /<c r="B3"[^>]* s="[1-9][0-9]*"/);
    assert.doesNotMatch(xml, /<c r="C1"/);
    assert.match(xml, /<dimension ref="A1:F3"/);
  } finally {
    streamedNoteZip.free();
  }
  const streamedNoteBook = await new Workbook().xlsx.load(streamedNoteBytes);
  const loadedNoteSheet = streamedNoteBook.getWorksheet('Streamed notes');
  assert.equal(loadedNoteSheet.getCell('A1').value, 'Cell with note');
  assert.equal(loadedNoteSheet.getCell('A1').note, 'Check amount');
  assert.equal(loadedNoteSheet.getCell('B1').note.texts[0].text, 'Rich note');
  assert.equal(loadedNoteSheet.getCell('B1').note.texts[0].font.bold, true);
  assert.equal(loadedNoteSheet.getCell('C1').note, undefined);
  assert.equal(loadedNoteSheet.getCell('D1').note, 'Note without a value');
  assert.equal(loadedNoteSheet.getCell('B3').note, 'Note on an otherwise empty row');
  assert.equal(loadedNoteSheet.getCell('D1').value, null);
  assert.equal(loadedNoteSheet.getCell('B3').value, null);
  assert.deepEqual(loadedNoteSheet.getCell('E1').value, noteDate);
  assert.deepEqual(loadedNoteSheet.getCell('F1').value.result, noteDate);

  const metadataWriter = new stream.xlsx.WorkbookWriter({ creator: 'Initial author', created: createdAt });
  assert.equal(metadataWriter.creator, 'Initial author');
  assert.equal(metadataWriter.lastModifiedBy, 'ExcelJS');
  assert.deepEqual(metadataWriter.created, createdAt);
  assert.deepEqual(metadataWriter.modified, createdAt);
  const metadataSheet = metadataWriter.addWorksheet('Core properties');
  metadataSheet.addRow(['Saved with metadata']).commit();
  metadataSheet.commit();
  metadataWriter.creator = 'Final author';
  metadataWriter.lastModifiedBy = 'Final editor';
  metadataWriter.modified = modifiedAt;
  metadataWriter.lastPrinted = printedAt;
  assert.throws(() => { metadataWriter.modified = new Date(NaN); }, { code: 'INVALID_VALUE' });
  await metadataWriter.commit();
  const streamedMetadataBytes = metadataWriter.stream.read();
  const metadataReadBack = await new Workbook().xlsx.load(streamedMetadataBytes);
  assert.equal(metadataReadBack.creator, 'Final author');
  assert.equal(metadataReadBack.lastModifiedBy, 'Final editor');
  assert.deepEqual(metadataReadBack.created, createdAt);
  assert.deepEqual(metadataReadBack.modified, modifiedAt);
  assert.deepEqual(metadataReadBack.lastPrinted, printedAt);
  assert.equal(metadataReadBack.getWorksheet('Core properties').getCell('A1').value, 'Saved with metadata');

  const workbookViewWriter = new stream.xlsx.WorkbookWriter();
  workbookViewWriter.addWorksheet('First').addRow(['First row']).commit();
  const selectedStreamSheet = workbookViewWriter.addWorksheet('Selected');
  selectedStreamSheet.addRow(['Selected row']).commit();
  selectedStreamSheet.commit();
  assert.deepEqual(workbookViewWriter.views, []);
  workbookViewWriter.views.push({ x: 25, y: 50, width: 14000, height: 22000,
    firstSheet: 1, activeTab: 0, visibility: 'visible' });
  workbookViewWriter.views[0].activeTab = 1;
  await workbookViewWriter.commit();
  const workbookViewBytes = workbookViewWriter.stream.read();
  const streamedWorkbookView = await new Workbook().xlsx.load(workbookViewBytes);
  assert.deepEqual(streamedWorkbookView.views, [
    { x: 25, y: 50, width: 14000, height: 22000, firstSheet: 1, activeTab: 1, visibility: 'visible' },
  ]);
  assert.equal(streamedWorkbookView.getWorksheet('Selected').getCell('A1').value, 'Selected row');
  const invalidViewPath = join(directory, 'invalid-stream-view.xlsx');
  const invalidViewWriter = new stream.xlsx.WorkbookWriter({ filename: invalidViewPath });
  invalidViewWriter.addWorksheet('Invalid view');
  invalidViewWriter.views = [{ visibility: 'invalid' }];
  await assert.rejects(invalidViewWriter.commit(), { code: 'INVALID_VALUE' });
  assert.equal(fs.existsSync(invalidViewPath), false);

  for (const useStyles of [false, true]) {
    const dateSystemWriter = new stream.xlsx.WorkbookWriter({ useSharedStrings: useStyles, useStyles });
    const dateSystemSheet = dateSystemWriter.addWorksheet('1904 dates');
    dateSystemSheet.addRow(['Dates']).commit();
    dateSystemWriter.properties.date1904 = true;
    const date1904Value = new Date('2024-02-29T12:00:00Z');
    dateSystemSheet.addRow([date1904Value, { formula: 'A2', result: date1904Value }]).commit();
    assert.throws(() => { dateSystemWriter.properties = { date1904: false }; }, { code: 'INVALID_VALUE' });
    assert.equal(dateSystemWriter.properties.date1904, true);
    await dateSystemWriter.commit();
    const dateSystemBytes = dateSystemWriter.stream.read();
    const dateSystemBook = await new Workbook().xlsx.load(dateSystemBytes);
    assert.equal(dateSystemBook.properties.date1904, true);
    const dates = dateSystemBook.getWorksheet('1904 dates');
    assert.deepEqual(dates.getCell('A2').value, date1904Value);
    assert.deepEqual(dates.getCell('B2').value.result, date1904Value);
    const dateSystemReader = new stream.xlsx.WorkbookReader(Readable.from([dateSystemBytes]), { styles: 'cache' });
    let dateRowCount = 0;
    for await (const sheet of dateSystemReader) {
      for await (const row of sheet) {
        if (row.number !== 2) continue;
        dateRowCount += 1;
        assert.deepEqual(row.worksheet.workbook.properties.model, { date1904: true });
        assert.deepEqual(row.getCell(1).value, date1904Value);
        assert.deepEqual(row.getCell(2).value.result, date1904Value);
      }
    }
    assert.equal(dateRowCount, 1);
  }

  const definedNameWriter = new stream.xlsx.WorkbookWriter();
  definedNameWriter.calcProperties = { fullCalcOnLoad: false };
  const namedStreamSheet = definedNameWriter.addWorksheet('Named data');
  const firstNamedStreamRow = namedStreamSheet.addRow([12]);
  assert.equal(firstNamedStreamRow.worksheet, namedStreamSheet);
  firstNamedStreamRow.commit();
  namedStreamSheet.addRow([20]).commit();
  namedStreamSheet.commit();
  assert.equal(firstNamedStreamRow.worksheet.workbook, definedNameWriter);
  assert.equal(namedStreamSheet.workbook.getWorksheet(namedStreamSheet.id), namedStreamSheet);
  const streamedNames = firstNamedStreamRow.worksheet.workbook.definedNames;
  streamedNames.add("'Named data'!A1:A2", 'ReportValues');
  streamedNames.add("'Named data'!B1", 'DiscardedName');
  streamedNames.remove("'Named data'!B1", 'DiscardedName');
  assert.deepEqual(streamedNames.getRanges('ReportValues').ranges, ["'Named data'!$A$1:$A$2"]);
  const quotedStreamSheet = definedNameWriter.addWorksheet("O'Brien data");
  const quotedCell = quotedStreamSheet.addRow([9]).getCell(1);
  quotedCell.name = 'QuotedName';
  quotedCell.addName('TemporaryName');
  assert.equal(quotedCell.name, 'QuotedName');
  assert.deepEqual(quotedCell.names, ['QuotedName', 'TemporaryName']);
  assert.throws(() => { quotedCell.names = ['OtherName', 'invalid name']; });
  assert.deepEqual(quotedCell.names, ['QuotedName', 'TemporaryName']);
  quotedCell.removeName('TemporaryName');
  const otherNamedCell = quotedStreamSheet.getCell('B1');
  otherNamedCell.addName('DiscardedCellName');
  otherNamedCell.removeAllNames();
  assert.deepEqual(otherNamedCell.names, []);
  const namedStreamSummary = definedNameWriter.addWorksheet('Summary');
  namedStreamSummary.addRow([{ formula: 'SUM(ReportValues)', result: 32 }]).commit();
  definedNameWriter.calcProperties.fullCalcOnLoad = true;
  assert.throws(() => { definedNameWriter.calcProperties = { fullCalcOnLoad: 'true' }; }, { code: 'INVALID_VALUE' });
  await definedNameWriter.commit();
  const streamedNameBytes = definedNameWriter.stream.read();
  const calculationZip = new XlsxZipReader(streamedNameBytes.length,
    (offset, length) => streamedNameBytes.subarray(offset, offset + length));
  try {
    const workbookXml = Buffer.from(calculationZip.read_metadata('xl/workbook.xml', 65536)).toString();
    assert.match(workbookXml, /<calcPr\b[^>]*\bfullCalcOnLoad="1"/);
  } finally {
    calculationZip.free();
  }
  const streamedNameBook = await new Workbook().xlsx.load(streamedNameBytes);
  assert.equal(streamedNameBook.calcProperties.fullCalcOnLoad, true);
  assert.deepEqual(streamedNameBook.definedNames.getRanges('ReportValues').ranges, ["'Named data'!$A$1:$A$2"]);
  assert.deepEqual(streamedNameBook.definedNames.getRanges('DiscardedName').ranges, []);
  assert.deepEqual(streamedNameBook.definedNames.getRanges('QuotedName').ranges, ["'O''Brien data'!$A$1"]);
  assert.deepEqual(streamedNameBook.definedNames.getRanges('DiscardedCellName').ranges, []);
  assert.deepEqual(streamedNameBook.getWorksheet('Summary').getCell('A1').value,
    { formula: 'SUM(ReportValues)', result: 32 });

  const headerWriter = new stream.xlsx.WorkbookWriter();
  const headerSheet = headerWriter.addWorksheet('Updated headers');
  headerSheet.columns = [{ key: 'label' }, { key: 'amount', header: 'Initial' }];
  const headerColumn = headerSheet.getColumn('amount');
  const noHeaderColumn = headerSheet.getColumn('label');
  assert.deepEqual(noHeaderColumn.headers, [undefined]);
  const compareColumn = headerSheet.getColumn('D');
  compareColumn.width = noHeaderColumn.width;
  assert.equal(noHeaderColumn.isDefault, true);
  assert.equal(noHeaderColumn.equivalentTo(compareColumn), true);
  noHeaderColumn.font = { bold: true, italic: true };
  assert.equal(noHeaderColumn.isDefault, false);
  assert.equal(noHeaderColumn.equivalentTo(compareColumn), false);
  compareColumn.font = { italic: true, bold: true };
  assert.equal(noHeaderColumn.equivalentTo(compareColumn), true);
  compareColumn.outlineLevel = 1;
  assert.equal(noHeaderColumn.equivalentTo(compareColumn), false);
  assert.equal(noHeaderColumn.headerCount, 1);
  assert.deepEqual(headerColumn.headers, ['Initial']);
  assert.equal(headerColumn.headerCount, 1);
  headerColumn.width = 9;
  assert.equal(headerColumn.isCustomWidth, false);
  headerColumn.width = 14;
  assert.equal(headerColumn.isCustomWidth, true);
  assert.equal(headerColumn.worksheet, headerSheet);
  assert.equal(headerColumn.collapsed, false);
  headerSheet.properties.outlineLevelCol = 2;
  headerColumn.outlineLevel = 1;
  assert.equal(headerColumn.collapsed, false);
  headerColumn.outlineLevel = 2;
  assert.equal(headerColumn.collapsed, true);
  headerColumn.header = ['Amount', 'USD'];
  assert.deepEqual(headerColumn.header, ['Amount', 'USD']);
  assert.deepEqual(headerColumn.headers, ['Amount', 'USD']);
  assert.equal(headerColumn.headerCount, 2);
  assert.deepEqual(JSON.parse(headerColumn.toString()), { key: 'amount', width: 14, headers: ['Amount', 'USD'] });
  assert.equal(headerSheet.getCell('B2').value, 'USD');
  headerSheet.getRow(1).commit();
  assert.throws(() => { headerColumn.header = ['Too late']; }, /header row has been committed/);
  assert.deepEqual(headerColumn.header, ['Amount', 'USD']);
  await headerWriter.commit();
  const headerBytes = headerWriter.stream.read();
  const headerReadBack = await new Workbook().xlsx.load(headerBytes);
  assert.equal(headerReadBack.getWorksheet('Updated headers').getCell('B1').value, 'Amount');
  assert.equal(headerReadBack.getWorksheet('Updated headers').getCell('B2').value, 'USD');
  assert.equal(headerReadBack.getWorksheet('Updated headers').getColumn('B').outlineLevel, 2);
  assert.equal(headerReadBack.getWorksheet('Updated headers').getColumn('B').collapsed, true);

  const columnIterationWriter = new stream.xlsx.WorkbookWriter();
  const columnIterationSheet = columnIterationWriter.addWorksheet('Column iteration');
  columnIterationSheet.columns = [{ key: 'label' }, { key: 'amount' }];
  columnIterationSheet.addRow({ label: 'Committed', amount: 1 }).commit();
  columnIterationSheet.getRow(3).values = { label: 'Other column only' };
  columnIterationSheet.getRow(5).values = { amount: 0 };
  const amountColumn = columnIterationSheet.getColumn('amount');
  assert.equal(columnIterationSheet.getColumnKey('amount'), amountColumn);
  const otherKeySheet = columnIterationWriter.addWorksheet('Other keys');
  assert.throws(() => columnIterationSheet.setColumnKey('foreign', otherKeySheet.getColumn(1)),
    { code: 'INVALID_VALUE' });
  columnIterationSheet.setColumnKey('balance', amountColumn);
  const enumeratedKeys = new Map();
  columnIterationSheet.eachColumnKey((column, key) => enumeratedKeys.set(key, column));
  assert.equal(enumeratedKeys.get('balance'), amountColumn);
  columnIterationSheet.deleteColumnKey('amount');
  assert.equal(columnIterationSheet.getColumnKey('amount'), undefined);
  const aliasRow = columnIterationSheet.getRow(5);
  aliasRow.values = { balance: 0, amount: 99 };
  const rejectedColumnValues = ['would overwrite row 1'];
  rejectedColumnValues[3] = 99;
  assert.throws(() => { amountColumn.values = rejectedColumnValues; }, /has been committed/);
  assert.deepEqual(Object.keys(amountColumn.values), ['5']);
  assert.equal(amountColumn.values[5], 0);
  const pendingColumnValues = [];
  pendingColumnValues[3] = false;
  pendingColumnValues[5] = 0;
  amountColumn.values = pendingColumnValues;
  assert.deepEqual(Object.keys(amountColumn.values), ['3', '5']);
  assert.equal(amountColumn.values[3], false);
  const sparseColumn = [];
  amountColumn.eachCell((cell, number) => sparseColumn.push([number, cell.value]));
  assert.deepEqual(sparseColumn, [[3, false], [5, 0]]);
  const denseColumn = [];
  amountColumn.eachCell({ includeEmpty: true }, (cell, number) => {
    denseColumn.push([number, cell.value]);
    if (number === 4) cell.value = 7;
  });
  assert.deepEqual(denseColumn, [[2, null], [3, false], [4, null], [5, 0]]);
  await columnIterationWriter.commit();
  const columnIterationResult = await new Workbook().xlsx.load(columnIterationWriter.stream.read());
  assert.equal(columnIterationResult.getWorksheet('Column iteration').getCell('B4').value, 7);

  const displayWriter = new stream.xlsx.WorkbookWriter();
  const displaySheet = displayWriter.addWorksheet('Cell display');
  const displayRow = displaySheet.addRow([42, false, { text: 'Link text', hyperlink: 'https://example.com' },
    { richText: [{ text: 'Rich ' }, { text: 'text' }] }, { formula: '2+2', result: 4 },
    { formula: '1-1', result: 0 }, { error: '#DIV/0!' }]);
  displaySheet.mergeCells('A1:A2');
  const displayTexts = ['42', 'false', 'Link text', 'Rich text', '4', '', '#DIV/0!', ''];
  for (const [index, expected] of displayTexts.entries()) {
    const cell = displayRow.getCell(index + 1);
    assert.equal(cell.text, expected);
    assert.equal(String(cell), expected);
  }
  assert.equal(displaySheet.getCell('A2').text, '42');
  const coveredDisplayCell = displaySheet.getCell('A2');
  assert.equal(coveredDisplayCell.worksheet, displaySheet);
  assert.equal(coveredDisplayCell.workbook, displayWriter);
  assert.equal(coveredDisplayCell.row, 2);
  assert.equal(coveredDisplayCell.col, 1);
  assert.equal(coveredDisplayCell.master.address, 'A1');
  assert.equal(coveredDisplayCell.$col$row, '$A$2');
  assert.equal(coveredDisplayCell.isMergedTo(displayRow.getCell(1)), true);
  assert.equal(displayRow.getCell(1).isMergedTo(displayRow.getCell(1)), false);
  const sameAddressOnOtherSheet = displayWriter.addWorksheet('Other address').getCell('A1');
  assert.equal(coveredDisplayCell.isMergedTo(sameAddressOnOtherSheet), false);
  assert.deepEqual(coveredDisplayCell.fullAddress, { sheetName: 'Cell display', address: 'A2', row: 2, col: 1 });
  const displayTypes = [ValueType.Number, ValueType.Boolean, ValueType.Hyperlink,
    ValueType.RichText, ValueType.Formula, ValueType.Formula, ValueType.Error, ValueType.Null];
  assert.deepEqual(displayTypes.map((_, index) => displayRow.getCell(index + 1).type), displayTypes);
  assert.equal(displaySheet.getCell('A2').type, ValueType.Merge);
  assert.equal(displaySheet.getCell('A2').effectiveType, ValueType.Number);
  assert.equal(displayRow.getCell(5).effectiveType, ValueType.Number);
  assert.equal(displayRow.getCell(5).formulaType, FormulaType.Master);
  assert.equal(displayRow.getCell(8).formulaType, undefined);
  const boolFormula = displayRow.getCell(9);
  boolFormula.value = { formula: 'TRUE()', result: true };
  assert.equal(boolFormula.effectiveType, ValueType.Null);
  const htmlLink = displayRow.getCell(10);
  htmlLink.value = { text: `'A' & <B> "C"`, hyperlink: 'https://example.com/' };
  assert.equal(htmlLink.html, `'A' &amp; &lt;B&gt; &quot;C&quot;`);
  assert.equal(htmlLink.isHyperlink, true);
  assert.equal(coveredDisplayCell.isHyperlink, false);
  assert.equal(displayRow.getCell(8).html, '');
  assert.equal(displayRow.getCell(2).toCsvString(), 0);
  assert.equal(htmlLink.toCsvString(), 'https://example.com/');
  assert.equal(displayRow.getCell(4).toCsvString(), '"Rich text"');
  assert.equal(displayRow.getCell(5).toCsvString(), '4');
  assert.equal(displayRow.getCell(6).toCsvString(), '');
  assert.equal(coveredDisplayCell.toCsvString(), '');
  displayRow.getCell(11).value = 'A,"B"';
  assert.equal(displayRow.getCell(11).toCsvString(), '"A,""B"""');
  displayRow.getCell(12).value = new Date('2025-03-15T00:00:00Z');
  assert.equal(displayRow.getCell(12).toCsvString(), '2025-03-15T00:00:00.000Z');
  await displayWriter.commit();
  const displayBook = await new Workbook().xlsx.load(displayWriter.stream.read());
  assert.deepEqual(displayTexts.map((_, index) => displayBook.getWorksheet('Cell display').getCell(1, index + 1).text),
    displayTexts);

  const lookupWriter = new stream.xlsx.WorkbookWriter();
  const lookupSheet = lookupWriter.addWorksheet('Lookup');
  assert.equal(lookupSheet.findRow(4), undefined);
  assert.equal(lookupSheet.findCell('A4'), undefined);
  assert.equal(lookupSheet.addRow(['First']).number, 1);
  const fourth = lookupSheet.getRow(4);
  fourth.values = [false, 0, ''];
  assert.equal(lookupSheet.findRow(4), fourth);
  assert.equal(fourth.findCell(2).value, 0);
  assert.equal(fourth.findCell(4), undefined);
  assert.equal(fourth.cellCount, 3);
  assert.equal(lookupSheet.findCell('B4'), fourth.findCell(2));
  assert.equal(lookupSheet.findCell(4, 2).value, 0);
  assert.equal(lookupSheet.findCell('D4'), undefined);
  fourth.getCell(5).font = { bold: true };
  const sparseCells = [];
  fourth.eachCell((cell, column) => sparseCells.push([column, cell.value]));
  assert.deepEqual(sparseCells, [[1, false], [2, 0], [3, '']]);
  assert.equal(fourth.cellCount, 5);
  assert.equal(fourth.actualCellCount, 3);
  assert.deepEqual(fourth.dimensions, { min: 1, max: 3 });
  assert.equal(lookupSheet.findCell('D4'), undefined);
  const denseCells = [];
  fourth.eachCell({ includeEmpty: true }, (cell, column) => denseCells.push([column, cell.value ?? null]));
  assert.deepEqual(denseCells, [[1, false], [2, 0], [3, ''], [4, null], [5, null]]);
  const mergeIterationSheet = lookupWriter.addWorksheet('Iteration merge');
  mergeIterationSheet.mergeCells('A1:C2');
  const mergedRow = mergeIterationSheet.getRow(2);
  const mergedColumns = [];
  mergedRow.eachCell((cell, column) => {
    assert.equal(cell.isMerged, true);
    mergedColumns.push(column);
  });
  assert.deepEqual(mergedColumns, [1, 2, 3]);
  assert.equal(mergedRow.findCell(2).master, mergeIterationSheet.getCell('A1'));
  assert.equal(mergedRow.hasValues, true);
  assert.equal(mergedRow.actualCellCount, 3);
  assert.deepEqual(mergedRow.dimensions, { min: 1, max: 3 });
  lookupSheet.getRow(1).commit();
  assert.equal(lookupSheet.findRow(1), undefined);
  assert.equal(lookupSheet.findCell('A1'), undefined);
  const blankStreamingRow = lookupSheet.getRow(3);
  blankStreamingRow.getCell(1);
  assert.equal(blankStreamingRow.actualCellCount, 0);
  assert.equal(blankStreamingRow.dimensions, null);
  const nonemptyRows = [];
  lookupSheet.eachRow((row, number) => nonemptyRows.push([number, row.getCell(1).value]));
  assert.deepEqual(nonemptyRows, [[4, false]]);
  const pendingRows = [];
  lookupSheet.eachRow({ includeEmpty: true }, (row, number) => {
    assert.equal(lookupSheet.findRow(number), row);
    pendingRows.push(number);
    row.commit();
  });
  assert.deepEqual(pendingRows, [2, 3, 4]);
  assert.equal(lookupSheet.findCell('B4'), undefined);
  await lookupWriter.commit();
  const lookupResult = await new Workbook().xlsx.load(lookupWriter.stream.read());
  assert.equal(lookupResult.getWorksheet('Lookup').getCell('B4').value, 0);

  const viewWriter = new stream.xlsx.WorkbookWriter();
  assert.equal(viewWriter.nextId, 1);
  const defaultNamedStreamSheet = viewWriter.addWorksheet();
  assert.equal(defaultNamedStreamSheet.name, 'sheet1');
  assert.equal(viewWriter.nextId, 2);
  const activeStreamSheet = viewWriter.addWorksheet('Active stream');
  const hiddenStreamSheet = viewWriter.addWorksheet('Hidden stream', { state: 'hidden' });
  assert.equal(hiddenStreamSheet.state, 'hidden');
  assert.equal(viewWriter.nextId, 4);
  defaultNamedStreamSheet.state = 'hidden';
  hiddenStreamSheet.state = 'visible';
  activeStreamSheet.state = 'hidden';
  hiddenStreamSheet.addRow(['Frozen']).commit();
  hiddenStreamSheet.properties.defaultRowHeight = 28;
  hiddenStreamSheet.properties.outlineLevelRow = 2;
  hiddenStreamSheet.views = [{ state: 'frozen', xSplit: 1, ySplit: 1, topLeftCell: 'B2' }];
  assert.equal(hiddenStreamSheet.views[0].topLeftCell, 'B2');
  assert.throws(() => { hiddenStreamSheet.state = 'veryHidden'; }, { code: 'WORKSHEET_STATE_REFUSED' });
  assert.equal(hiddenStreamSheet.state, 'visible');
  hiddenStreamSheet.commit();
  assert.throws(() => { hiddenStreamSheet.views = []; }, /already committed/);
  activeStreamSheet.commit();
  activeStreamSheet.state = 'veryHidden';
  await viewWriter.commit();
  const streamedViewBytes = viewWriter.stream.read();
  const streamViewBook = await new Workbook().xlsx.load(streamedViewBytes);
  assert.equal(streamViewBook.getWorksheet('Active stream').state, 'veryHidden');
  const streamViewResult = streamViewBook.getWorksheet('Hidden stream');
  assert.equal(streamViewResult.state, 'visible');
  assert.equal(streamViewResult.properties.defaultRowHeight, 28);
  assert.equal(streamViewResult.properties.outlineLevelRow, 2);
  assert.equal(streamViewResult.getCell('A1').value, 'Frozen');
  const [streamedView] = streamViewResult.views;
  assert.deepEqual([streamedView.state, streamedView.xSplit, streamedView.ySplit, streamedView.topLeftCell],
    ['frozen', 1, 1, 'B2']);

  const dimensionsWriter = new stream.xlsx.WorkbookWriter();
  const dimensionsSheet = dimensionsWriter.addWorksheet('Committed dimensions');
  assert.equal(dimensionsSheet.dimensions.shortRange, 'A1');
  assert.deepEqual(dimensionsSheet.dimensions.model, { top: 0, left: 0, bottom: 0, right: 0 });
  dimensionsSheet.getCell('C4').value = 5;
  assert.equal(dimensionsSheet.dimensions.shortRange, 'A1');
  dimensionsSheet.getRow(4).commit();
  assert.equal(dimensionsSheet.dimensions.shortRange, 'C4');
  dimensionsSheet.getCell('E6').value = 7;
  dimensionsSheet.getRow(6).commit();
  assert.deepEqual(dimensionsSheet.dimensions, {
    top: 4, left: 3, bottom: 6, right: 5, tl: 'C4', br: 'E6', $t$l: '$C$4', $b$r: '$E$6',
    range: 'C4:E6', $range: '$C$4:$E$6', count: 9, shortRange: 'C4:E6', $shortRange: '$C$4:$E$6',
    model: { top: 4, left: 3, bottom: 6, right: 5 },
  });
  dimensionsSheet.mergeCells('B8:G10');
  const emptyMergedSheet = dimensionsWriter.addWorksheet('Only a merge');
  emptyMergedSheet.mergeCells('D4:F6');
  const sparseArraySheet = dimensionsWriter.addWorksheet('Sparse array dimensions');
  sparseArraySheet.getCell('C3').value = { formula: '1+2', shareType: 'array', ref: 'C3:F12', result: 3 };
  sparseArraySheet.getRow(3).commit();
  assert.equal(sparseArraySheet.dimensions.shortRange, 'C3:F12');
  await dimensionsWriter.commit();
  assert.equal(dimensionsSheet.dimensions.shortRange, 'B4:G10');
  assert.equal(emptyMergedSheet.dimensions.shortRange, 'D4:F6');
  const dimensionsBytes = dimensionsWriter.stream.read();
  const dimensionArchive = new XlsxDocument(dimensionsBytes);
  try {
    const xml = Buffer.from(dimensionArchive.zip_entry_bytes('xl/worksheets/sheet1.xml')).toString();
    assert.match(xml, /<dimension ref="B4:G10"\/>/);
    const emptyMergeXml = Buffer.from(dimensionArchive.zip_entry_bytes('xl/worksheets/sheet2.xml')).toString();
    assert.match(emptyMergeXml, /<dimension ref="D4:F6"\/>/);
    const sparseArrayXml = Buffer.from(dimensionArchive.zip_entry_bytes('xl/worksheets/sheet3.xml')).toString();
    assert.match(sparseArrayXml, /<dimension ref="C3:F12"\/>/);
    assert.match(sparseArrayXml, /<f t="array" ref="C3:F12">1\+2<\/f>/);
    assert.doesNotMatch(sparseArrayXml, /<c r="F12"/);
  } finally {
    dimensionArchive.free();
  }

  const printWriter = new stream.xlsx.WorkbookWriter();
  const printStreamSheet = printWriter.addWorksheet('Stream print', {
    pageSetup: { orientation: 'portrait' },
    headerFooter: { oddHeader: '&LOriginal' },
  });
  assert.equal(printStreamSheet.headerFooter.oddHeader, '&LOriginal');
  const printStreamRow = printStreamSheet.addRow(['Print me']);
  printStreamRow.commit();
  printStreamRow.addPageBreak(2, 4);
  printStreamRow.addPageBreak(3, 5);
  assert.throws(() => printStreamRow.addPageBreak(-1), { code: 'INVALID_VALUE' });
  assert.deepEqual(printStreamSheet.rowBreaks, [{ id: 1, min: 1, max: 3, man: 1 }]);
  printStreamSheet.pageSetup = { orientation: 'landscape', paperSize: 9 };
  printStreamSheet.pageSetup.margins.left = 0.5;
  printStreamSheet.headerFooter = { oddHeader: '&CUpdated' };
  printStreamSheet.headerFooter.oddFooter = '&RPage &P';
  await printWriter.commit();
  const streamedPrintBytes = printWriter.stream.read();
  const streamPrintBook = await new Workbook().xlsx.load(streamedPrintBytes);
  const streamPrintResult = streamPrintBook.getWorksheet('Stream print');
  assert.equal(streamPrintResult.getCell('A1').value, 'Print me');
  assert.deepEqual(streamPrintResult.rowBreaks, [{ id: 1, min: 1, max: 3, man: 1 }]);
  const ignoredBreaksBook = await new Workbook().xlsx.load(streamedPrintBytes, { ignoreNodes: ['rowBreaks'] });
  assert.deepEqual(ignoredBreaksBook.getWorksheet('Stream print').rowBreaks, []);
  const preservedBreaksBook = await new Workbook().xlsx.load(await ignoredBreaksBook.xlsx.writeBuffer());
  assert.deepEqual(preservedBreaksBook.getWorksheet('Stream print').rowBreaks, streamPrintResult.rowBreaks);
  const printZip = new XlsxZipReader(streamedPrintBytes.length,
    (offset, length) => streamedPrintBytes.subarray(offset, offset + length));
  try {
    const printXml = Buffer.from(printZip.read_metadata('xl/worksheets/sheet1.xml', 65536)).toString();
    assert.match(printXml, /<rowBreaks\b[^>]*><brk\b(?=[^>]*\bid="1")(?=[^>]*\bmin="1")(?=[^>]*\bmax="3")(?=[^>]*\bman="1")/);
  } finally {
    printZip.free();
  }
  assert.throws(() => printStreamRow.addPageBreak(), /already committed/);
  assert.equal(streamPrintResult.pageSetup.orientation, 'landscape');
  assert.equal(streamPrintResult.pageSetup.paperSize, 9);
  assert.equal(streamPrintResult.pageSetup.margins.left, 0.5);
  assert.equal(streamPrintResult.headerFooter.oddHeader, '&CUpdated');
  assert.equal(streamPrintResult.headerFooter.oddFooter, '&RPage &P');

  const filterWriter = new stream.xlsx.WorkbookWriter();
  const filteredStreamSheet = filterWriter.addWorksheet('Filtered stream', { autoFilter: 'A1:A2' });
  assert.equal(filteredStreamSheet.autoFilter, 'A1:A2');
  filteredStreamSheet.addRow(['Name', 'Count']).commit();
  filteredStreamSheet.addRow(['Apple', 3]).commit();
  filteredStreamSheet.autoFilter = { from: { row: 1, column: 1 }, to: 'B2' };
  assert.equal(filteredStreamSheet.autoFilter, 'A1:B2');
  assert.throws(() => { filteredStreamSheet.autoFilter = 'invalid'; }, { code: 'AUTOFILTER_EDIT_REFUSED' });
  assert.equal(filteredStreamSheet.autoFilter, 'A1:B2');
  filteredStreamSheet.commit();
  assert.throws(() => { filteredStreamSheet.autoFilter = null; }, /already committed/);
  const clearedFilterSheet = filterWriter.addWorksheet('Cleared filter', { autoFilter: 'A1:A2' });
  clearedFilterSheet.autoFilter = null;
  await filterWriter.commit();
  const streamedFilterBytes = filterWriter.stream.read();
  const streamedFilterBook = await new Workbook().xlsx.load(streamedFilterBytes);
  const reloadedStreamFilter = streamedFilterBook.getWorksheet('Filtered stream');
  assert.equal(reloadedStreamFilter.autoFilter, 'A1:B2');
  assert.equal(reloadedStreamFilter.getCell('B2').value, 3);
  assert.equal(streamedFilterBook.getWorksheet('Cleared filter').autoFilter, undefined);

  const conditionalWriter = new stream.xlsx.WorkbookWriter({ useStyles: true });
  const conditionalSheet = conditionalWriter.addWorksheet('Conditional');
  conditionalSheet.addRow([12]).commit();
  conditionalSheet.addConditionalFormatting({ ref: 'A1:A4', rules: [{
    type: 'expression', formulae: ['A1>10'], style: { font: { bold: true } },
  }] });
  conditionalSheet.addConditionalFormatting({ ref: 'B1', rules: [{ type: 'expression', formulae: ['B1>3'] }] });
  conditionalSheet.addConditionalFormatting({ ref: 'C1', rules: [{ type: 'expression', formulae: ['C1>4'] }] });
  conditionalSheet.removeConditionalFormatting(-1);
  conditionalSheet.removeConditionalFormatting(block => block.ref === 'A1:A4');
  assert.throws(() => conditionalSheet.addConditionalFormatting({ ref: 'invalid', rules: [
    { type: 'expression', formulae: ['A1>1'] },
  ] }));
  assert.deepEqual(conditionalSheet.conditionalFormatting.map(block => block.ref), ['A1:A4']);
  const clearedConditionalSheet = conditionalWriter.addWorksheet('Cleared');
  clearedConditionalSheet.addConditionalFormatting({ ref: 'A1', rules: [{ type: 'expression', formulae: ['A1>0'] }] });
  clearedConditionalSheet.removeConditionalFormatting();
  conditionalSheet.commit();
  assert.throws(() => conditionalSheet.removeConditionalFormatting(), /already committed/);

  await conditionalWriter.commit();
  const conditionalBytes = conditionalWriter.stream.read();
  const conditionalBook = await new Workbook().xlsx.load(conditionalBytes);
  const conditionalResult = conditionalBook.getWorksheet('Conditional');
  assert.equal(conditionalResult.getCell('A1').value, 12);
  assert.deepEqual(conditionalResult.conditionalFormattings.map(block => block.ref), ['A1:A4']);
  assert.deepEqual(conditionalResult.conditionalFormattings[0].rules[0].formulae, ['A1>10']);
  assert.equal(conditionalResult.conditionalFormattings[0].rules[0].style.font.bold, true);
  assert.deepEqual(conditionalBook.getWorksheet('Cleared').conditionalFormattings, []);

  const protectionWriter = new stream.xlsx.WorkbookWriter();
  const secured = protectionWriter.addWorksheet('Secured');
  secured.addRow(['Committed before protection']).commit();
  await secured.protect('password', { spinCount: 1, formatRows: true, selectLockedCells: false });
  const streamProtection = secured.sheetProtection;
  assert.equal(streamProtection.algorithmName, 'SHA-512');
  assert.equal(streamProtection.spinCount, 1);
  assert.equal(streamProtection.formatRows, true);
  assert.equal(streamProtection.selectLockedCells, false);
  await assert.rejects(secured.protect('password', { spinCount: 10_000_001 }), { code: 'INVALID_VALUE' });
  assert.deepEqual(secured.sheetProtection, streamProtection);
  secured.commit();
  await assert.rejects(secured.protect(), /already committed/);
  assert.throws(() => secured.unprotect(), /already committed/);

  const unlocked = protectionWriter.addWorksheet('Unlocked');
  await unlocked.protect(null, { insertRows: true });
  assert.equal(unlocked.sheetProtection.insertRows, true);
  unlocked.unprotect();
  assert.equal(unlocked.sheetProtection, null);
  await protectionWriter.commit();
  const protectionBytes = protectionWriter.stream.read();
  const protectionResult = await new Workbook().xlsx.load(protectionBytes);
  assert.equal(protectionResult.getWorksheet('Secured').getCell('A1').value, 'Committed before protection');
  assert.deepEqual(protectionResult.getWorksheet('Secured').sheetProtection, streamProtection);
  assert.equal(protectionResult.getWorksheet('Unlocked').sheetProtection, undefined);

  const missingBackgroundPath = join(directory, 'missing-background-output.xlsx');
  const missingBackgroundWriter = new stream.xlsx.WorkbookWriter({ filename: missingBackgroundPath });
  const removedBackground = missingBackgroundWriter.addImage({ filename: backgroundFile, extension: 'png' });
  missingBackgroundWriter.addWorksheet('Background').addBackgroundImage(removedBackground);
  fs.unlinkSync(backgroundFile);
  await assert.rejects(missingBackgroundWriter.commit(), { code: 'ENOENT' });
  assert.equal(fs.existsSync(missingBackgroundPath), false);

  const mediaBook = new Workbook();
  const mediaSource = mediaBook.addWorksheet('Source');
  const mediaTarget = mediaBook.addWorksheet('Target');
  assert.deepEqual(mediaBook.media, []);
  const mediaId = mediaBook.addImage({ buffer: png, extension: 'png' });
  assert.deepEqual(mediaBook.media[mediaId], mediaBook.getImage(mediaId));
  mediaBook.media[mediaId].buffer[0] = 0;
  assert.equal(mediaBook.getImage(mediaId).buffer[0], png[0]);
  const mediaWriter = new stream.xlsx.WorkbookWriter();
  const streamMediaId = mediaWriter.addImage({ buffer: png, extension: 'png' });
  assert.equal(mediaWriter.media[streamMediaId].extension, 'png');
  assert.deepEqual(Buffer.from(mediaWriter.media[streamMediaId].buffer), png);
  mediaWriter.addWorksheet('Stream media').addBackgroundImage(streamMediaId);
  await mediaWriter.commit();
  mediaSource.addBackgroundImage(mediaId);
  mediaSource.addImage(mediaId, { tl: { col: 1, row: 2 }, ext: { width: 20, height: 10 },
    hyperlinks: { hyperlink: 'https://example.com/', tooltip: 'Picture' } });
  mediaTarget.addImage(mediaId, 'A1:B2');
  const media = mediaSource.model.media;
  mediaTarget.model = { rows: [], media };
  assert.deepEqual(mediaTarget.model.media, media);
  const mediaBytes = await mediaBook.xlsx.writeBuffer();
  const mediaReloaded = await new Workbook().xlsx.load(mediaBytes);
  assert.deepEqual(mediaReloaded.media[0], mediaReloaded.getImage(0));
  const readMedia = sheet => sheet.model.media.map(({ imageId, ...entry }) => ({
    ...entry, buffer: Buffer.from(mediaReloaded.getImage(imageId).buffer),
  }));
  assert.deepEqual(readMedia(mediaReloaded.getWorksheet('Target')), readMedia(mediaReloaded.getWorksheet('Source')));
  assert.throws(() => {
    mediaTarget.model = { rows: [{ number: 1, cells: [{ address: 'A1', type: ValueType.Number, value: 42 }] }],
      media: [media[0], { ...media[1], hyperlinks: { hyperlink: 'invalid\u0000link' } }] };
  }, { code: 'IMAGE_EDIT_REFUSED' });
  assert.deepEqual(mediaTarget.model.media, media);
  assert.deepEqual(await mediaBook.xlsx.writeBuffer(), mediaBytes);
  mediaTarget.model = { rows: [], media: [] };
  assert.deepEqual(mediaTarget.model.media, []);
  assert.deepEqual(mediaSource.model.media, media);
  const clearedMedia = await new Workbook().xlsx.load(await mediaBook.xlsx.writeBuffer());
  assert.deepEqual(clearedMedia.getWorksheet('Target').model.media, []);
  assert.equal(clearedMedia.getWorksheet('Source').model.media.length, 2);

  const tableModelBook = new Workbook();
  const tableSource = tableModelBook.addWorksheet('TableSource');
  const tableTarget = tableModelBook.addWorksheet('TableTarget');
  const emptyTableBook = new Workbook();
  const emptyTableSheet = emptyTableBook.addWorksheet('Empty table');
  const emptyTable = emptyTableSheet.addTable({
    name: 'EmptyTable', ref: 'A1', columns: [{ name: 'Amount' }], rows: [],
  });
  assert.equal(emptyTableSheet.getCell('A1').value, 'Amount');
  const emptyTableCopy = new Workbook();
  emptyTableCopy.model = emptyTableBook.model;
  const reloadedEmptySheet = (await new Workbook().xlsx.load(await emptyTableCopy.xlsx.writeBuffer())).worksheets[0];
  assert.equal(reloadedEmptySheet.getTable('EmptyTable').columns[0].name, 'Amount');
  emptyTable.addRow([42]);
  emptyTable.commit();
  assert.equal(emptyTableSheet.getCell('A2').value, 42);
  emptyTable.removeRows(0);
  emptyTable.commit();
  assert.equal(emptyTableSheet.getCell('A2').value, null);
  const emptiedTableSheet = (await new Workbook().xlsx.load(await emptyTableBook.xlsx.writeBuffer())).worksheets[0];
  assert.equal(emptiedTableSheet.getTable('EmptyTable').columns[0].name, 'Amount');
  assert.equal(emptiedTableSheet.getCell('A2').value, null);

  const emptyWithTotals = emptyTableSheet.addTable({
    name: 'EmptyWithTotals', ref: 'A5', columns: [{ name: 'Amount' }], rows: [[12]], totalsRow: true,
  });
  emptyWithTotals.removeRows(0);
  emptyWithTotals.commit();
  assert.equal(emptyTableSheet.getCell('A6').value, 'Total');
  assert.equal(emptyTableSheet.getCell('A7').value, null);

  emptyWithTotals.addColumn({ name: 'Tax', totalsRowLabel: 'Tax total' }, [], 0);
  emptyWithTotals.commit();
  assert.equal(emptyTableSheet.getCell('A5').value, 'Tax');
  assert.equal(emptyTableSheet.getCell('B5').value, 'Amount');
  assert.equal(emptyTableSheet.getCell('A6').value, 'Tax total');
  emptyWithTotals.removeColumns(0);
  emptyWithTotals.commit();
  assert.equal(emptyTableSheet.getCell('A5').value, 'Amount');
  assert.equal(emptyTableSheet.getCell('A6').value, 'Total');
  assert.equal(emptyTableSheet.getCell('B5').value, null);

  tableSource.addTable({
    name: 'SourceTable', ref: 'A1', columns: [{ name: 'Value', filterButton: true }],
    rows: [[3]], style: { theme: 'TableStyleLight9', showRowStripes: true },
  });
  tableTarget.addTable({ name: 'OldTable', ref: 'C1', columns: [{ name: 'Old' }], rows: [[9]] });
  const [tableModel] = tableSource.model.tables;
  tableTarget.model = {
    rows: [
      { number: 1, cells: [{ address: 'A1', type: ValueType.String, value: 'Independent header' }] },
      { number: 2, cells: [{ address: 'A2', type: ValueType.Number, value: 12 }] },
    ],
    tables: [{ ...tableModel, name: 'TargetTable', displayName: 'TargetTable',
      columns: [{ ...tableModel.columns[0], calculatedColumnFormula: '1+1', totalsRowFunction: 'none' }] }],
  };
  assert.equal(tableTarget.getTable('OldTable'), undefined);
  assert.equal(tableTarget.getTable('TargetTable').columns[0].filterButton, true);
  assert.equal(tableTarget.getTable('TargetTable').columns[0].totalsRowFunction, 'none');
  assert.equal(tableTarget.getTable('TargetTable').columns[0].calculatedColumnFormula, '1+1');
  assert.equal(tableTarget.getCell('A1').value, 'Independent header');
  assert.equal(tableTarget.getCell('A2').value, 12);
  const tableBorder = { top: { style: 'thin', color: { argb: 'FF123456' } } };
  tableSource.addConditionalFormatting({ ref: 'A2', rules: [
    { type: 'expression', formulae: ['TRUE'], style: { font: { bold: true } } },
    { type: 'expression', formulae: ['TRUE'], style: { border: tableBorder } },
  ] });
  const tableModelBytes = await tableModelBook.xlsx.writeBuffer();
  const tableReopened = (await new Workbook().xlsx.load(tableModelBytes)).getWorksheet('TableTarget');
  assert.deepEqual(tableReopened.model.tables, tableTarget.model.tables);
  assert.equal(tableReopened.getCell('A1').value, 'Independent header');
  assert.throws(() => {
    tableTarget.model = { rows: [], tables: [tableModel] };
  }, { code: 'TABLE_EDIT_REFUSED' });
  assert.deepEqual(await tableModelBook.xlsx.writeBuffer(), tableModelBytes);
  const styledTableArchive = SheetJS.CFB.read(Buffer.from(tableModelBytes), { type: 'array' });
  const styledTablePart = '/xl/tables/table1.xml';
  const styledTableXml = Buffer.from(SheetJS.CFB.find(styledTableArchive, styledTablePart).content).toString('utf8')
    .replace(/<table(?=[ >])/, '<table headerRowDxfId="0" headerRowBorderDxfId="1" tableBorderDxfId="1" totalsRowBorderDxfId="1"')
    .replace(/<tableColumn(?=[ >])/, '<tableColumn dataDxfId="0"');
  assert.ok(SheetJS.CFB.utils.cfb_del(styledTableArchive, styledTablePart));
  SheetJS.CFB.utils.cfb_add(styledTableArchive, styledTablePart.slice(1), Buffer.from(styledTableXml));
  const styledTableBytes = SheetJS.CFB.write(styledTableArchive, { type: 'array', fileType: 'zip' });
  const styledTableBook = await new Workbook().xlsx.load(styledTableBytes);
  const [styledTable] = styledTableBook.getWorksheet('TableSource').model.tables;
  assert.equal(styledTable.headerRowDxfId, 0);
  assert.equal(styledTable.columns[0].dataDxfId, 0);
  assert.deepEqual(styledTable.differentialStyles.headerRowDxfId, { font: { bold: true } });
  for (const key of ['headerRowBorderDxfId', 'tableBorderDxfId', 'totalsRowBorderDxfId']) {
    assert.equal(styledTable[key], 1);
    assert.deepEqual(styledTable.differentialStyles[key], { border: tableBorder });
  }
  assert.deepEqual(styledTable.columns[0].differentialStyles.dataDxfId, { font: { bold: true } });
  assert.throws(() => {
    const differentialStyles = {
      ...styledTable.differentialStyles,
      get tableBorderDxfId() { assert.fail('getter must be rejected'); },
    };
    tableTarget.model = { rows: [], tables: [{ ...styledTable, differentialStyles }] };
  }, { code: 'UNSUPPORTED_OPTIONS' });
  const incompleteTable = { ...styledTable, differentialStyles: undefined };
  assert.throws(() => { tableTarget.model = { rows: [], tables: [incompleteTable] }; }, { code: 'UNSUPPORTED_OPTIONS' });
  assert.deepEqual(await tableModelBook.xlsx.writeBuffer(), tableModelBytes);
  const dxfCopy = new Workbook();
  const dxfCopySheet = dxfCopy.addWorksheet('Copy');
  dxfCopySheet.addConditionalFormatting({ ref: 'C1', rules: [
    { type: 'expression', formulae: ['TRUE'], style: { font: { italic: true } } },
  ] });
  dxfCopySheet.model = { rows: [], tables: [styledTable] };
  const [copiedStyledTable] = (await new Workbook().xlsx.load(await dxfCopy.xlsx.writeBuffer()))
    .getWorksheet('Copy').model.tables;
  assert.notEqual(copiedStyledTable.headerRowDxfId, styledTable.headerRowDxfId);
  assert.equal(copiedStyledTable.headerRowDxfId, copiedStyledTable.columns[0].dataDxfId);
  assert.notEqual(copiedStyledTable.tableBorderDxfId, styledTable.tableBorderDxfId);
  assert.equal(copiedStyledTable.tableBorderDxfId, copiedStyledTable.headerRowBorderDxfId);
  assert.equal(copiedStyledTable.tableBorderDxfId, copiedStyledTable.totalsRowBorderDxfId);
  assert.deepEqual(copiedStyledTable.differentialStyles, styledTable.differentialStyles);
  assert.deepEqual(copiedStyledTable.columns[0].differentialStyles, styledTable.columns[0].differentialStyles);
  tableTarget.model = { rows: tableTarget.model.rows, tables: [] };
  assert.deepEqual(tableTarget.model.tables, []);
  assert.equal(tableTarget.getCell('A2').value, 12);

  const modelTarget = new Workbook();
  const staleSheet = modelTarget.addWorksheet('Old');
  const encodeDocument = XlsxDocument.prototype.to_bytes;
  const writeScalarBatch = XlsxDocument.prototype.write_excel_scalar_batch;
  const setRowLayout = XlsxDocument.prototype.set_row_layout;
  let encodes = 0;
  const rowLayouts = [];
  const sizes = [];
  XlsxDocument.prototype.to_bytes = function () {
    encodes += 1;
    return encodeDocument.call(this);
  };
  XlsxDocument.prototype.write_excel_scalar_batch = function (name, positions, values) {
    sizes.push(values.length);
    return writeScalarBatch.call(this, name, positions, values);
  };
  XlsxDocument.prototype.set_row_layout = function (name, row, ...settings) {
    rowLayouts.push([name, row + 1]);
    return setRowLayout.call(this, name, row, ...settings);
  };
  try {
    modelTarget.model = {
      creator: 'Model author',
      properties: { date1904: true },
      calcProperties: { fullCalcOnLoad: true },
      worksheets: [
        { id: 1, name: 'One', rows: [
          { number: 1, cells: [
            { address: 'A1', type: ValueType.Number, value: 4 },
            { address: 'B1', type: ValueType.Formula, formula: 'A1+1', result: 5 },
            { address: 'C1', type: ValueType.Number, value: 6, style: { numFmt: '0.00' } },
          ] },
          { number: 2, cells: [{ address: 'A2', type: ValueType.Boolean, value: true }] },
          { number: 3, cells: [{ address: 'A3', type: ValueType.Formula,
            shareType: 'array', ref: 'A3:A4', formula: 'ROW(A3:A4)' }] },
          { number: 4, cells: [{ address: 'B4', type: ValueType.Number, value: 8 }] },
        ], pageSetup: { rowBreaks: null, colBreaks: null } },
        { id: 7, name: 'Seven', rows: [
          { number: 1, cells: [{ address: 'A1', type: ValueType.Number, value: 7 }] },
          { number: 2, hidden: true, height: 24, cells: [{ address: 'A2', type: ValueType.Boolean, value: false }] },
          { number: 3, cells: [{ address: 'A3', type: ValueType.String, value: 'text' }] },
          { number: 5, cells: [] },
        ],
          media: [{ type: 'background', imageId: 1 }] },
      ],
      sheets: [{ id: 7 }, { id: 1 }],
      definedNames: [{ name: 'SevenCell', ranges: ['Seven!$A$1'] }],
      media: [undefined, { type: 'image', extension: 'png', buffer: png }],
    };
  } finally {
    XlsxDocument.prototype.to_bytes = encodeDocument;
    XlsxDocument.prototype.set_row_layout = setRowLayout;
    XlsxDocument.prototype.write_excel_scalar_batch = writeScalarBatch;
  }
  assert.equal(encodes, 1, 'Workbook model assignment should serialize only on publication');
  assert.deepEqual(sizes, [3, 1, 1], 'Plain cells are batched across rows and on either side of complex cells');
  assert.deepEqual(rowLayouts, [['Seven', 2], ['Seven', 5], ['One', 3], ['One', 4]]);
  assert.deepEqual(modelTarget.worksheets.map(sheet => [sheet.id, sheet.name]), [[7, 'Seven'], [1, 'One']]);
  assert.equal(modelTarget.getWorksheet(7).getCell('A1').value, 7);
  assert.equal(modelTarget.getWorksheet(7).getCell('A2').value, false);
  assert.equal(modelTarget.getWorksheet(7).getCell('A3').value, 'text');
  assert.deepEqual(modelTarget.getWorksheet(1).getCell('B1').value, { formula: 'A1+1', result: 5 });
  assert.equal(modelTarget.getWorksheet(1).getCell('C1').numFmt, '0.00');
  assert.equal(modelTarget.getWorksheet(1).getCell('A2').value, true);
  assert.equal(modelTarget.getWorksheet(1).getCell('A3').value.ref, 'A3:A4');
  assert.equal(modelTarget.getWorksheet(1).getCell('B4').value, 8);
  assert.equal(modelTarget.getWorksheet(7).getBackgroundImageId(), 1);
  assert.throws(() => staleSheet.getCell('A1'), { code: 'STALE_HANDLE' });
  assert.equal(modelTarget.creator, 'Model author');
  assert.equal(modelTarget.properties.date1904, true);
  assert.equal(modelTarget.calcProperties.fullCalcOnLoad, true);
  assert.deepEqual(modelTarget.definedNames.getRanges('SevenCell').ranges, ['Seven!$A$1']);
  const assignedBytes = await modelTarget.xlsx.writeBuffer();
  const assignedReloaded = await new Workbook().xlsx.load(assignedBytes);
  assert.deepEqual(assignedReloaded.worksheets.map(sheet => sheet.id), [7, 1]);
  assert.equal(assignedReloaded.getWorksheet(7).getRow(2).hidden, true);
  assert.equal(assignedReloaded.getWorksheet(7).getRow(2).height, 24);
  assert.throws(() => {
    modelTarget.model = { worksheets: [{ id: 9, name: 'Invalid', rows: [{
      number: 1, cells: [{ address: 'A1', type: ValueType.Formula, formula: 'SUM((' }],
    }] }] };
  }, { code: 'ROW_EDIT_REFUSED' });
  assert.deepEqual(await modelTarget.xlsx.writeBuffer(), assignedBytes);
  modelTarget.model = { worksheets: [{ id: 9, name: 'Nine', rows: [] }] };
  assert.deepEqual(modelTarget.worksheets.map(sheet => [sheet.id, sheet.name]), [[9, 'Nine']]);
  modelTarget.model = { worksheets: [], creator: 'Empty author' };
  assert.deepEqual(modelTarget.worksheets, []);
  assert.equal(modelTarget.creator, 'Empty author');
  assert.equal(modelTarget.addWorksheet('Fresh').id, 1);

  const dxfFont = { bold: true, vertAlign: 'superscript', scheme: 'minor', family: 2, charset: 128 };
  const dxfSource = new Workbook().addWorksheet('Source');
  assert.throws(() => {
    dxfSource.getCell('A1').fill = { get type() { assert.fail('getter must be rejected'); } };
  }, { code: 'UNSUPPORTED_OPTIONS' });
  dxfSource.getCell('A1').font = { bold: true };
  assert.deepEqual(dxfSource.getCell('A1').font, { name: 'Calibri', size: 11, bold: true });
  dxfSource.addConditionalFormatting({ ref: 'A1:A3', rules: [
    { type: 'expression', formulae: ['A1>1'], style: { font: dxfFont } },
  ] });
  const dxfTargetBook = new Workbook();
  const dxfTarget = dxfTargetBook.addWorksheet('Target');
  dxfTarget.addConditionalFormatting({ ref: 'B1', rules: [
    { type: 'expression', formulae: ['B1>1'], style: { font: { italic: true } } },
  ] });
  const copiedConditions = dxfSource.model.conditionalFormattings;
  dxfTarget.model = { rows: [], conditionalFormattings: copiedConditions };
  const [copiedRule] = dxfTarget.conditionalFormattings[0].rules;
  assert.deepEqual(copiedRule.style, { font: dxfFont });
  assert.equal(Object.hasOwn(copiedRule, 'dxfId'), false);
  assert.equal(Object.hasOwn(copiedConditions[0].rules[0], 'dxfId'), false);
  const dxfBytes = await dxfTargetBook.xlsx.writeBuffer();
  assert.throws(() => {
    dxfTarget.model = { rows: [], conditionalFormattings: [{ ref: 'B1', rules: [{ ...copiedRule, dxfId: 0, style: undefined }] }] };
  }, { code: 'UNSUPPORTED_OPTIONS' });
  assert.deepEqual(await dxfTargetBook.xlsx.writeBuffer(), dxfBytes);
  const dxfReloaded = (await new Workbook().xlsx.load(dxfBytes)).getWorksheet('Target');
  assert.deepEqual(dxfReloaded.conditionalFormattings[0].rules[0].style, copiedRule.style);

  const predicateSource = new Workbook().addWorksheet('Predicates');
  predicateSource.addConditionalFormatting({ ref: 'A1:A3', rules: [
    { type: 'containsText', operator: 'notContains', text: 'absent' },
    { type: 'containsText', operator: 'beginsWith', text: 'prefix' },
    { type: 'containsText', operator: 'endsWith', text: 'suffix' },
    { type: 'containsText', operator: 'containsBlanks' },
    { type: 'containsText', operator: 'containsErrors' },
  ] });
  const predicateBook = new Workbook();
  const predicates = predicateBook.addWorksheet('Predicates');
  const sourceBlock = predicateSource.model.conditionalFormattings[0];
  const expectedRules = sourceBlock.rules.map((rule, index) => index === 0
    ? { ...rule, formulae: ['ISERROR(SEARCH("absent",A2))'] } : rule);
  expectedRules.push({ type: 'containsText', priority: 6, formulae: ['NOT(ISERROR(SEARCH("needle",A1)))'] });
  predicates.model = { rows: [], conditionalFormattings: [{ ...sourceBlock, rules: expectedRules }] };
  assert.deepEqual(predicates.conditionalFormattings[0].rules, expectedRules);
  const predicateBytes = await predicateBook.xlsx.writeBuffer();
  const predicateReloaded = (await new Workbook().xlsx.load(predicateBytes)).getWorksheet('Predicates');
  assert.deepEqual(predicateReloaded.conditionalFormattings[0].rules, expectedRules);
  assert.throws(() => {
    predicates.model = { rows: [], conditionalFormattings: [{
      ref: 'A1', rules: [{ ...expectedRules[0], formulae: ['SUM(('] }],
    }] };
  }, { code: 'CONDITIONAL_FORMAT_EDIT_REFUSED' });
  assert.deepEqual(await predicateBook.xlsx.writeBuffer(), predicateBytes);

  predicates.addConditionalFormatting({ ref: 'C1:C3', rules: [
    { type: 'duplicateValues', style: { font: { bold: true } }, stopIfTrue: true },
    { type: 'uniqueValues', style: { font: { italic: true } } },
  ] });
  assert.throws(() => predicates.addConditionalFormatting({ ref: 'C1', rules: [
    { type: 'uniqueValues', formulae: ['1=1'] },
  ] }), { code: 'UNSUPPORTED_OPTIONS' });
  const equalityRules = predicates.conditionalFormattings[1].rules;
  assert.deepEqual(equalityRules.map(rule => rule.type), ['duplicateValues', 'uniqueValues']);
  const equalityBook = new Workbook();
  equalityBook.model = { worksheets: [{ name: 'Copy', rows: [],
    conditionalFormattings: [{ ref: 'C1:C3', rules: equalityRules }] }] };
  const equalityReloaded = (await new Workbook().xlsx.load(await equalityBook.xlsx.writeBuffer()))
    .getWorksheet('Copy').conditionalFormattings[0];
  assert.deepEqual(equalityReloaded.rules.map(({ dxfId, ...rule }) => rule),
    equalityRules.map(({ dxfId, ...rule }) => rule));

  const averageBook = new Workbook();
  const averages = averageBook.addWorksheet('Averages');
  averages.addConditionalFormatting({ ref: 'A1:A3', rules: [
    { type: 'aboveAverage', aboveAverage: false, equalAverage: true, stdDev: 2 },
    { type: 'aboveAverage', equalAverage: false, stdDev: 0 },
    { type: 'aboveAverage' },
  ] });
  const optionsOf = sheet => sheet.conditionalFormattings[0].rules.map(
    ({ aboveAverage, equalAverage, stdDev }) => [aboveAverage, equalAverage, stdDev]);
  const expectedAverages = [[false, true, 2], [undefined, false, 0], [undefined, undefined, undefined]];
  assert.deepEqual(optionsOf(averages), expectedAverages);
  const originalAverageBytes = await averageBook.xlsx.writeBuffer();
  const copiedAverages = new Workbook();
  copiedAverages.model = averageBook.model;
  const averageBytes = await copiedAverages.xlsx.writeBuffer();
  const averageReloaded = (await new Workbook().xlsx.load(averageBytes)).getWorksheet('Averages');
  assert.deepEqual(optionsOf(averageReloaded), expectedAverages);
  assert.throws(() => {
    averages.model = { rows: [], conditionalFormattings: [{
      ref: 'A1', rules: [{ type: 'aboveAverage', stdDev: -1 }],
    }] };
  }, { code: 'INVALID_VALUE' });
  assert.deepEqual(optionsOf(averages), expectedAverages);

  assert.deepEqual(await averageBook.xlsx.writeBuffer(), originalAverageBytes);

  const iconBook = new Workbook();
  const icons = iconBook.addWorksheet('Icons');
  const iconThresholds = [{ type: 'num', value: 0 }, { type: 'num', value: 1, gte: false },
    { type: 'formula', value: 'A1+2', gte: true }];
  icons.addConditionalFormatting({ ref: 'A1:A3', rules: [{ type: 'iconSet', cfvo: iconThresholds }] });
  const iconCopy = new Workbook();
  iconCopy.model = iconBook.model;
  const iconReloaded = (await new Workbook().xlsx.load(await iconCopy.xlsx.writeBuffer())).getWorksheet('Icons');
  assert.deepEqual(iconReloaded.conditionalFormattings[0].rules[0].cfvo, iconThresholds);
  const iconsBefore = await iconBook.xlsx.writeBuffer();
  assert.throws(() => icons.addConditionalFormatting({ ref: 'A1:A3', rules: [{
    type: 'iconSet', cfvo: [iconThresholds[0], iconThresholds[1], { ...iconThresholds[2], gte: 1 }],
  }] }), { code: 'INVALID_VALUE' });
  assert.deepEqual(await iconBook.xlsx.writeBuffer(), iconsBefore);

  const customIcons = [
    { iconSet: 'NoIcons', iconId: 0 },
    { iconSet: '5Boxes', iconId: 4 },
    { iconSet: '3Triangles', iconId: 1 },
  ];
  const extendedIconBlock = { ref: 'A1:A3', rules: [{
    type: 'iconSet', x14Id: '{00000000-0000-4000-8000-000000000001}', priority: 2,
    iconSet: '3Stars', cfvo: iconThresholds, icons: customIcons,
  }] };
  const customIconBook = new Workbook();
  const customIconSheet = customIconBook.addWorksheet('Custom');
  customIconSheet.model = { rows: [], conditionalFormattings: [extendedIconBlock] };
  const customIconBytes = await customIconBook.xlsx.writeBuffer();
  const customIconReloaded = (await new Workbook().xlsx.load(customIconBytes)).getWorksheet('Custom');
  assert.deepEqual(customIconReloaded.conditionalFormattings[0].rules[0].icons, customIcons);
  assert.deepEqual(customIconReloaded.conditionalFormattings[0].rules[0].cfvo, iconThresholds);
  assert.throws(() => {
    customIconSheet.model = { rows: [], conditionalFormattings: [{
      ...extendedIconBlock, rules: [{ ...extendedIconBlock.rules[0], icons: customIcons.slice(1) }],
    }] };
  }, { code: 'CONDITIONAL_FORMAT_EDIT_REFUSED' });
  assert.deepEqual(await customIconBook.xlsx.writeBuffer(), customIconBytes);

  const authoredIconsBook = new Workbook();
  const authoredIcons = authoredIconsBook.addWorksheet('Icons');
  authoredIcons.addConditionalFormatting({ ref: 'C1:C3', rules: [
    { type: 'iconSet', iconSet: '3Triangles', cfvo: iconThresholds },
    { type: 'expression', formulae: ['C1>0'], priority: 6 },
    { type: 'iconSet', iconSet: '3Stars', cfvo: iconThresholds, custom: true,
      icons: [{ iconSet: 'NoIcons' }, { iconSet: '5Boxes', iconId: 4 }, { iconSet: '3Triangles', iconId: 1 }] },
  ] });
  const authoredRules = authoredIcons.conditionalFormattings[0].rules;
  assert.deepEqual(authoredRules.map(rule => rule.priority), [6, 7, 8]);
  assert.deepEqual(authoredRules[2].icons, customIcons);
  const generatedIds = authoredRules.slice(1).map(rule => rule.x14Id);
  for (const id of generatedIds) assert.match(id, /^\{[0-9A-F-]{36}\}$/);
  assert.notEqual(generatedIds[0], generatedIds[1]);
  const authoredReloaded = await new Workbook().xlsx.load(await authoredIconsBook.xlsx.writeBuffer());
  assert.deepEqual(authoredReloaded.getWorksheet('Icons').conditionalFormattings[0].rules, authoredRules);

  const authoredBarsBook = new Workbook();
  const authoredBars = authoredBarsBook.addWorksheet('DataBars');
  const newBar = { type: 'dataBar', cfvo: [{ type: 'min' }, { type: 'max' }],
    extendedCfvo: [{ type: 'autoMin' }, { type: 'formula', value: 'MAX(B1:B4)' }],
    color: { theme: 6, tint: 0.2 }, border: true, gradient: false, axisPosition: 'middle',
    direction: 'rightToLeft', borderColor: { argb: 'FF667788' }, negativeFillColor: { indexed: 13 },
    negativeBorderColor: { theme: 4, tint: -0.25 }, axisColor: {} };
  authoredBars.addConditionalFormatting({ ref: 'B1:B4', rules: [
    { type: 'expression', formulae: ['B1>0'], priority: 1 }, newBar,
  ] });
  const authoredBar = authoredBars.conditionalFormattings[0].rules[1];
  assert.match(authoredBar.x14Id, /^\{[0-9A-F-]{36}\}$/);
  assert.equal(authoredBar.priority, 2);
  for (const key of ['border', 'gradient', 'axisPosition', 'direction', 'borderColor', 'color',
    'negativeFillColor', 'negativeBorderColor', 'axisColor', 'cfvo', 'extendedCfvo']) {
    assert.deepEqual(authoredBar[key], newBar[key]);
  }
  const barBytes = await authoredBarsBook.xlsx.writeBuffer();
  const barReloaded = (await new Workbook().xlsx.load(barBytes)).getWorksheet('DataBars');
  assert.deepEqual(barReloaded.conditionalFormattings[0].rules[1], authoredBar);
  const barsCopy = new Workbook();
  barsCopy.model = authoredBarsBook.model;
  assert.deepEqual(barsCopy.getWorksheet('DataBars').conditionalFormattings[0].rules[1], authoredBar);
  assert.throws(() => authoredBars.addConditionalFormatting({ ref: 'B5', rules: [
    { ...newBar, axisPosition: 'invalid' },
  ] }), { code: 'INVALID_VALUE' });
  assert.deepEqual(await authoredBarsBook.xlsx.writeBuffer(), barBytes);

  const extensionOnlySource = await new Workbook().xlsx.load(barBytes, { ignoreNodes: ['conditionalFormatting'] });
  const [extensionOnlyBlock] = extensionOnlySource.getWorksheet('DataBars').conditionalFormattings;
  const [extensionOnlyBar] = extensionOnlyBlock.rules;
  assert.equal(extensionOnlyBar.color, undefined);
  const standaloneBarBook = new Workbook();
  standaloneBarBook.model = { worksheets: [{ name: 'Standalone', rows: [],
    conditionalFormattings: [{ ...extensionOnlyBlock, rules: [{ ...extensionOnlyBar, priority: 5, showValue: false }] }] }] };
  const standaloneBarBytes = await standaloneBarBook.xlsx.writeBuffer();
  const standaloneReloaded = await new Workbook().xlsx.load(standaloneBarBytes);
  const standaloneSheet = standaloneReloaded.getWorksheet('Standalone');
  assert.deepEqual(standaloneSheet.conditionalFormattings[0].rules,
    [{ ...extensionOnlyBar, priority: 5, showValue: false }]);
  const fallbackOnly = await new Workbook().xlsx.load(standaloneBarBytes, { ignoreNodes: ['extLst'] });
  assert.deepEqual(fallbackOnly.getWorksheet('Standalone').conditionalFormattings, []);

  const standardBarBook = new Workbook();
  const standardBars = standardBarBook.addWorksheet('Standard');
  const basicColors = [{ theme: 3, tint: 0.25 }, { indexed: 13 }, {}, { argb: 'FF112233' }];
  for (const [index, color] of basicColors.entries()) {
    standardBars.addConditionalFormatting({ ref: 'A' + (index + 1), rules: [{
      type: 'dataBar', cfvo: [{ type: 'min' }, { type: 'max' }], color,
    }] });
  }
  const standardBarCopy = new Workbook();
  standardBarCopy.model = standardBarBook.model;
  const standardBarReloaded = await new Workbook().xlsx.load(await standardBarCopy.xlsx.writeBuffer());
  assert.deepEqual(standardBarReloaded.getWorksheet('Standard').conditionalFormattings.map(({ rules }) => rules[0].color),
    basicColors);

  const scaleBook = new Workbook();
  const scaleSheet = scaleBook.addWorksheet('Scale');
  const scaleColors = [{ theme: 4, tint: -0.25 }, { indexed: 23 }, {}];
  const scaleThresholds = [{ type: 'min' }, { type: 'percentile', value: 50 }, { type: 'max' }];
  scaleSheet.addConditionalFormatting({ ref: 'A1:A4', rules: [
    { type: 'colorScale', cfvo: scaleThresholds, color: scaleColors },
  ] });
  const scaleCopy = new Workbook();
  scaleCopy.model = scaleBook.model;
  const scaleReloaded = await new Workbook().xlsx.load(await scaleCopy.xlsx.writeBuffer());
  const [scaleRule] = scaleReloaded.getWorksheet('Scale').conditionalFormattings[0].rules;
  assert.deepEqual(scaleRule.color, scaleColors);
  assert.deepEqual(scaleRule.cfvo, scaleThresholds);
  const scaleBytes = await scaleBook.xlsx.writeBuffer();
  assert.throws(() => scaleSheet.addConditionalFormatting({ ref: 'A1:A4', rules: [{
    type: 'colorScale', cfvo: scaleThresholds, color: [scaleColors[0], null, scaleColors[2]],
  }] }), { code: 'INVALID_VALUE' });
  assert.deepEqual(await scaleBook.xlsx.writeBuffer(), scaleBytes);

  const noteBook = new Workbook();
  const notes = noteBook.addWorksheet('Notes');
  const noteTexts = [{ text: ' first ', font: { name: 'Calibri', size: 11, bold: true, color: { argb: 'FF123456' } } },
    { text: 'second' }];
  notes.getCell('A1').note = 'Old text';
  const noteBox = {
    margins: { insetmode: 'custom', inset: [0.2, 0.3, 0.4, 0.5] },
    protection: { locked: 'False', lockText: 'True' },
    editAs: 'oneCells',
    texts: noteTexts,
  };
  notes.getCell('A1').note = noteBox;
  assert.throws(() => {
    notes.getCell('A1').note = {
      ...noteBox, margins: { get inset() { assert.fail('getter must be rejected'); } },
    };
  }, { code: 'UNSUPPORTED_OPTIONS' });
  assert.deepEqual(notes.getCell('A1').note, noteBox);
  notes.getCell('C1').note = { texts: [{ text: 'plain' }], protection: { lockText: 'False' }, editAs: 'twoCells' };
  assert.equal(notes.getCell('C1').note, 'plain');
  notes.getCell('B1').model = { ...notes.getCell('A1').model, address: 'B1' };
  assert.deepEqual(notes.getCell('B1').model.comment.note.texts, noteTexts);
  const savedNotes = await noteBook.xlsx.writeBuffer();
  const readNotes = (await new Workbook().xlsx.load(savedNotes)).getWorksheet('Notes');
  const notesCopy = new Workbook();
  const readFullNotes = XlsxDocument.prototype.sheet_notes;
  XlsxDocument.prototype.sheet_notes = function () {
    assert.fail('Worksheet presence should not request note text');
  };
  try {
    notesCopy.model = readNotes.workbook.model;
  } finally {
    XlsxDocument.prototype.sheet_notes = readFullNotes;
  }
  assert.deepEqual(notesCopy.getWorksheet('Notes').getCell('A1').note, noteBox);
  assert.deepEqual(readNotes.getCell('B1').note, noteBox);
  assert.deepEqual(notesCopy.getWorksheet('Notes').getCell('C1').model.comment.note,
    readNotes.getCell('C1').model.comment.note);
  assert.equal(readNotes.getCell('C1').model.comment.note.protection.lockText, 'False');
  assert.equal(readNotes.getCell('C1').model.comment.note.editAs, 'twoCells');
  assert.throws(() => { notes.getCell('A1').note = { ...noteBox, margins: { inset: [0, 0, 0, -1] } }; },
    { code: 'INVALID_VALUE' });
  assert.throws(() => { notes.getCell('A1').note = { texts: [{ text: '' }, { text: 'bad' }] }; },
    { code: 'INVALID_VALUE' });
  assert.deepEqual(await noteBook.xlsx.writeBuffer(), savedNotes);
  notes.getCell('A1').note = { texts: [{ text: '' }] };
  assert.equal(notes.getCell('A1').note, '');

  const csv = join(directory, 'export.csv');
  fs.writeFileSync(csv, 'existing content');
  assert.throws(() => SheetJS.writeFileAsync(csv, sheetBook, { bookType: 'csv' }, false), {
    code: 'INVALID_VALUE', message: 'Expected an asynchronous file write callback',
  });
  assert.equal(fs.readFileSync(csv, 'utf8'), 'existing content');
  await new Promise((resolve, reject) => {
    SheetJS.writeFileAsync(csv, sheetBook, error => error ? reject(error) : resolve());
  });
  assert.equal(fs.readFileSync(csv, 'utf8').replace(/^\uFEFF/, ''), '23,Node');
  const directCsv = join(directory, 'direct.csv');
  SheetJS.write(sheetBook, { type: 'file', file: directCsv, bookType: 'csv', FS: ';' });
  assert.equal(fs.readFileSync(directCsv, 'utf8').replace(/^\uFEFF/, ''), '23;Node');
  assert.throws(() => SheetJS.write(sheetBook, {
    type: 'file',
    get file() { assert.fail('file getter must not run'); },
    bookType: 'csv',
  }), { code: 'UNSUPPORTED_OPTIONS', message: 'Expected a filename for file output' });
  assert.equal(fs.readFileSync(directCsv, 'utf8').replace(/^\uFEFF/, ''), '23;Node');
  const semicolonCsv = join(directory, 'semicolon.csv');
  await new Promise((resolve, reject) => {
    SheetJS.writeFileAsync(semicolonCsv, sheetBook, { bookType: 'csv', FS: ';' },
      error => error ? reject(error) : resolve());
  });
  assert.equal(fs.readFileSync(semicolonCsv, 'utf8').replace(/^\uFEFF/, ''), '23;Node');
  const inferredCsv = join(directory, 'inferred.CSV');
  SheetJS.writeFile(sheetBook, inferredCsv, { bookType: undefined, FS: ';' });
  assert.equal(fs.readFileSync(inferredCsv, 'utf8').replace(/^\uFEFF/, ''), '23;Node');
  await new Promise((resolve, reject) => {
    SheetJS.writeFileAsync(inferredCsv, sheetBook, { bookType: undefined },
      error => error ? reject(error) : resolve());
  });
  assert.equal(fs.readFileSync(inferredCsv, 'utf8').replace(/^\uFEFF/, ''), '23,Node');
  assert.throws(() => SheetJS.writeFileAsync(inferredCsv, sheetBook, {
    get bookType() { assert.fail('bookType getter must not run'); },
  }, () => assert.fail('callback must not run')), { code: 'UNSUPPORTED_OPTIONS' });
  assert.equal(fs.readFileSync(inferredCsv, 'utf8').replace(/^\uFEFF/, ''), '23,Node');
  const defaultXlsx = join(directory, 'default.csv');
  SheetJS.write(sheetBook, { type: 'file', file: defaultXlsx, bookType: undefined });
  assert.equal(fs.readFileSync(defaultXlsx).subarray(0, 4).toString('hex'), '504b0304');
  const inferredHtml = join(directory, 'inferred.HTM');
  SheetJS.writeFile(sheetBook, inferredHtml);
  assert.match(fs.readFileSync(inferredHtml, 'utf8'), /<table\b/);
  const inferredXls = join(directory, 'inferred.XLS');
  SheetJS.writeFile(sheetBook, inferredXls);
  const reopenedXls = SheetJS.readFile(inferredXls);
  assert.deepEqual(SheetJS.utils.sheet_to_json(reopenedXls.Sheets[reopenedXls.SheetNames[0]], { header: 1 }), [[23, 'Node']]);
  const datedCsv = join(directory, 'dated.csv');
  const datedSheetBook = () => SheetJS.utils.book_new({
    A1: { t: 'd', v: new Date(2024, 1, 3) }, '!ref': 'A1',
  });
  SheetJS.writeFile(datedSheetBook(), datedCsv, { dateNF: 'yyyy-mm-dd' });
  assert.equal(fs.readFileSync(datedCsv, 'utf8').replace(/^\uFEFF/, ''), '2024-02-03');
  await new Promise((resolve, reject) => {
    SheetJS.writeFileAsync(datedCsv, datedSheetBook(), { dateNF: 'dd/mm/yyyy' },
      error => error ? reject(error) : resolve());
  });
  assert.equal(fs.readFileSync(datedCsv, 'utf8').replace(/^\uFEFF/, ''), '03/02/2024');
  const csvWorkbook = new Workbook();
  await assert.rejects(csvWorkbook.csv.readFile(csv, { sheetName: 'Invalid', map: true }), {
    code: 'INVALID_VALUE', message: 'Expected a CSV mapping function',
  });
  await assert.rejects(csvWorkbook.csv.readFile(csv, { sheetName: 'Invalid', dateFormats: [1] }), {
    code: 'INVALID_VALUE', message: 'Expected an array of CSV date formats',
  });
  assert.equal(csvWorkbook.getWorksheet('Invalid'), undefined);
  const sheet = await csvWorkbook.csv.readFile(csv, { sheetName: 'Imported' });
  assert.deepEqual(sheet.getRow(1).values.slice(1), [23, 'Node']);
  sheet.getCell('A3').value = '#REF!';
  const formatted = join(directory, 'formatted.csv');
  fs.writeFileSync(formatted, 'existing output');
  await assert.rejects(csvWorkbook.csv.writeFile(formatted, { sheetName: 'Imported', map: true }), {
    code: 'INVALID_VALUE', message: 'Expected a CSV mapping function',
  });
  await assert.rejects(csvWorkbook.csv.writeFile(formatted, {
    sheetName: 'Imported', formatterOptions: { transform: true },
  }), { code: 'INVALID_VALUE', message: 'Expected a CSV row transform function' });
  for (const [option, message] of [
    [{ dateFormat: 7 }, 'Expected a CSV date format string'],
    [{ dateUTC: 'false' }, 'Expected a boolean CSV dateUTC option'],
  ]) {
    await assert.rejects(csvWorkbook.csv.writeFile(formatted, { sheetName: 'Imported', ...option }), {
      code: 'INVALID_VALUE', message,
    });
  }
  assert.equal(fs.readFileSync(formatted, 'utf8'), 'existing output');
  await csvWorkbook.csv.writeFile(formatted, {
    sheetName: 'Imported',
    includeEmptyRows: false,
    formatterOptions: { delimiter: ';', writeBOM: true },
  });
  assert.equal(fs.readFileSync(formatted, 'utf8'), '\uFEFF23;Node\n#REF!');
  const transformed = await csvWorkbook.csv.writeBuffer({
    sheetName: 'Imported', includeEmptyRows: false,
    formatterOptions: { transform: (row, done) => done(null, [String(row[0]) + '!']) },
  });
  assert.equal(transformed.toString(), '23!\n#REF!!');
  const dated = csvWorkbook.addWorksheet('Dated');
  dated.getCell('A1').value = new Date('2024-02-29T07:08:09Z');
  const dateOutput = await csvWorkbook.csv.writeBuffer({
    sheetName: 'Dated', dateFormat: 'YYYY/MM/DD HH:mm:ss', dateUTC: true,
  });
  assert.equal(dateOutput.toString(), '2024/02/29 07:08:09');

  {
    // SheetJS 0.20.3 reads the values stored inside merged ranges like any other cell.
    const mergedBook = new Workbook();
    const merged = mergedBook.addWorksheet('Merged');
    merged.getCell('A1').value = 'Title';
    merged.mergeCells('A1:C1');
    merged.getCell('A2').value = 42;
    merged.getCell('B2').value = { richText: [{ text: 'Bold', font: { bold: true } }, { text: ' tail' }] };
    merged.mergeCells('B2:C3');
    merged.getCell('A3').value = { formula: 'A2*2', result: 84 };
    merged.mergeCells('A3:A4');
    merged.getCell('D4').value = true;
    const mergedBytes = new Uint8Array(await mergedBook.xlsx.writeBuffer());
    const fields = (cell) => cell && Object.fromEntries(['t', 'v', 'w', 'f', 'r', 'h']
      .filter((key) => cell[key] !== undefined).map((key) => [key, cell[key]]));
    const expectedMerged = {
      A1: { t: 's', v: 'Title', w: 'Title', r: '<t>Title</t>', h: 'Title' },
      A2: { t: 'n', v: 42, w: '42' },
      A3: { t: 'n', v: 84, w: '84', f: 'A2*2' },
      B2: {
        t: 's', v: 'Bold tail', w: 'Bold tail',
        r: '<r><rPr><b/><sz val="11"/><rFont val="Calibri"/></rPr><t>Bold</t></r><r><t xml:space="preserve"> tail</t></r>',
        h: '<span style="font-size:11pt;"><b>Bold</b></span> tail',
      },
      D4: { t: 'b', v: true, w: 'TRUE' },
    };
    for (const options of [{}, { sheetStubs: true }, { dense: true }]) {
      const read = SheetJS.read(mergedBytes, { type: 'array', ...options }).Sheets.Merged;
      const cell = (key) => {
        if (!read['!data']) return read[key];
        const { r, c } = SheetJS.utils.decode_cell(key);
        return read['!data'][r]?.[c];
      };
      assert.deepEqual(Object.fromEntries(Object.keys(expectedMerged).map((key) => [key, fields(cell(key))])), expectedMerged);
      assert.deepEqual(read['!merges'].map(SheetJS.utils.encode_range), ['A1:C1', 'B2:C3', 'A3:A4']);
      assert.equal(cell('B1'), undefined);
      assert.equal(SheetJS.utils.sheet_to_csv(read), 'Title,,,\n42,Bold tail,,\n84,,,\n,,,TRUE');
      assert.equal(JSON.stringify(SheetJS.utils.sheet_to_json(read, { header: 1 })),
        '[["Title"],[42,"Bold tail"],[84],[null,null,null,true]]');
    }
    const mergedModel = SheetJS.read(mergedBytes, { type: 'array' });
    assert.deepEqual(new Uint8Array(SheetJS.write(mergedModel, { type: 'array', bookType: 'xlsx' })), mergedBytes);
    mergedModel.Sheets.Merged.D4.v = false;
    const editedMerged = SheetJS.read(SheetJS.write(mergedModel, { type: 'array', bookType: 'xlsx' }), { type: 'array' }).Sheets.Merged;
    assert.deepEqual([editedMerged.A1.v, editedMerged.B2.v, editedMerged.D4.v], ['Title', 'Bold tail', false]);
    mergedModel.Sheets.Merged.A1.v = 'Changed';
    assert.throws(() => SheetJS.write(mergedModel, { type: 'array', bookType: 'xlsx' }), { code: 'UNSUPPORTED_CELL' });

    // A new workbook keeps covered values and formulas, as SheetJS 0.20.3 writes them.
    const covered = SheetJS.utils.aoa_to_sheet([['Head', 'covered', 3], [null, 7, { t: 'n', v: 2, f: 'C1-1' }]]);
    covered['!merges'] = ['A1:B1', 'B2:C2'].map(SheetJS.utils.decode_range);
    const coveredRead = SheetJS.read(SheetJS.write(SheetJS.utils.book_new(covered), { type: 'array', bookType: 'xlsx' }),
      { type: 'array' }).Sheets.Sheet1;
    assert.deepEqual(['A1', 'B1', 'C1', 'B2', 'C2'].map((key) => fields(coveredRead[key])), [
      // Inline strings carry h but no r in 0.20.3.
      { t: 's', v: 'Head', w: 'Head', h: 'Head' }, { t: 's', v: 'covered', w: 'covered', h: 'covered' }, { t: 'n', v: 3, w: '3' },
      { t: 'n', v: 7, w: '7' }, { t: 'n', v: 2, w: '2', f: 'C1-1' },
    ]);
    assert.deepEqual(coveredRead['!merges'].map(SheetJS.utils.encode_range), ['A1:B1', 'B2:C2']);
  }

  {
    // SheetJS 0.20.3 accepts raw and WTF for every input format. With cellStyles, a truthy WTF
    // adds the untinted theme color as raw_rgb; raw, password, PRN, FS and UTC do not change a ZIP workbook.
    const themedBook = new Workbook();
    const themed = themedBook.addWorksheet('Themed');
    themed.getCell('A1').value = 'accent';
    themed.getCell('A1').fill = { type: 'pattern', pattern: 'solid', fgColor: { theme: 4, tint: 0.4 }, bgColor: { theme: 5 } };
    themed.getCell('B1').value = 2;
    themed.getCell('B1').fill = { type: 'pattern', pattern: 'darkGray', fgColor: { theme: 0 }, bgColor: { theme: 9, tint: -0.25 } };
    themed.getCell('C1').value = 3;
    themed.getCell('C1').fill = { type: 'pattern', pattern: 'solid', fgColor: { argb: 'FF112233' } };
    const themedBytes = new Uint8Array(await themedBook.xlsx.writeBuffer());
    const fillsOf = (options) => {
      const sheet = SheetJS.read(themedBytes, { type: 'array', ...options }).Sheets.Themed;
      return ['A1', 'B1', 'C1'].map((key) => sheet[key].s);
    };
    const plainFills = [
      { patternType: 'solid', fgColor: { theme: 4, tint: 0.4, rgb: '95B3D7' }, bgColor: { theme: 5, rgb: 'C0504D' } },
      { patternType: 'darkGray', fgColor: { theme: 0 }, bgColor: { theme: 9, tint: -0.25, rgb: 'E46C0A' } },
      { patternType: 'solid', fgColor: { rgb: '112233' }, bgColor: { indexed: 64 } },
    ];
    const rawFills = [
      { patternType: 'solid', fgColor: { theme: 4, tint: 0.4, rgb: '95B3D7', raw_rgb: '4F81BD' }, bgColor: { theme: 5, rgb: 'C0504D', raw_rgb: 'C0504D' } },
      { patternType: 'darkGray', fgColor: { theme: 0 }, bgColor: { theme: 9, tint: -0.25, rgb: 'E46C0A', raw_rgb: 'F79646' } },
      plainFills[2],
    ];
    for (const WTF of [true, 1, 'yes']) assert.deepEqual(fillsOf({ cellStyles: true, WTF }), rawFills);
    for (const WTF of [false, 0, '', null, undefined]) assert.deepEqual(fillsOf({ cellStyles: true, WTF }), plainFills);
    assert.deepEqual(fillsOf({ WTF: true }), [undefined, undefined, undefined]);

    const unused = { raw: true, password: 'secret', PRN: true, FS: ';', UTC: false, WTF: 1 };
    const zipModel = (bytes, options) => {
      const book = SheetJS.read(bytes, { type: 'array', ...options });
      return JSON.stringify([book.SheetNames, book.Workbook, book.Sheets[book.SheetNames[0]]]);
    };
    const xlsbBytes = SheetJS.write(SheetJS.read(themedBytes, { type: 'array' }), { type: 'array', bookType: 'xlsb' });
    for (const bytes of [themedBytes, xlsbBytes]) {
      for (const options of [{}, { cellNF: true, sheetStubs: true }, { dense: true, cellDates: true }]) {
        assert.equal(zipModel(bytes, { ...options, ...unused }), zipModel(bytes, options));
        assert.equal(zipModel(bytes, { ...options, raw: undefined, WTF: null }), zipModel(bytes, options));
      }
    }
    assert.deepEqual(SheetJS.read(themedBytes, { type: 'array', bookSheets: true, ...unused }).SheetNames, ['Themed']);
    const unusedFile = join(directory, 'unused-options.xlsx');
    fs.writeFileSync(unusedFile, themedBytes);
    assert.equal(SheetJS.readFile(unusedFile, { ...unused, cellStyles: true }).Sheets.Themed.A1.s.fgColor.raw_rgb, '4F81BD');

    // A truthy raw with bookFiles keeps the compound-file header and sectors of an XLS workbook.
    const rawBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([['raw', 1]]), 'Raw');
    const xlsBytes = SheetJS.write(rawBook, { type: 'array', bookType: 'biff8' });
    const rawContainer = SheetJS.read(xlsBytes, { type: 'array', bookFiles: true, raw: 'yes' }).cfb;
    assert.deepEqual(Object.keys(rawContainer), ['FileIndex', 'FullPaths', 'raw']);
    assert.equal(rawContainer.raw.header.length, 512);
    assert.deepEqual(Array.from(rawContainer.raw.header.subarray(0, 8)), [208, 207, 17, 224, 161, 177, 26, 225]);
    assert.deepEqual(rawContainer.raw.sectors.map((sector) => sector.length), [512, 512, 512, 512]);
    for (const options of [{ bookFiles: true, raw: 0 }, { bookFiles: true, raw: null }]) {
      assert.deepEqual(Object.keys(SheetJS.read(xlsBytes, { type: 'array', ...options }).cfb), ['FileIndex', 'FullPaths']);
    }
    assert.equal(SheetJS.read(xlsBytes, { type: 'array', raw: true }).cfb, undefined);
    for (const WTF of [1, 0, null, 'yes']) {
      assert.deepEqual(SheetJS.read(xlsBytes, { type: 'array', WTF }).Sheets.Raw.A1, { t: 's', v: 'raw', w: 'raw' });
    }
    for (const bookType of ['biff2', 'csv', 'sylk', 'dif', 'html', 'xlml', 'fods', 'ods', 'wk3']) {
      const bytes = SheetJS.write(rawBook, { type: 'array', bookType });
      const expected = JSON.stringify(SheetJS.read(bytes, { type: 'array' }).Sheets);
      assert.equal(JSON.stringify(SheetJS.read(bytes, { type: 'array', WTF: 1 }).Sheets), expected, bookType);
      if (bookType === 'biff2' || bookType === 'wk3') {
        assert.equal(JSON.stringify(SheetJS.read(bytes, { type: 'array', raw: true }).Sheets), expected, bookType);
      }
    }
  }


  {
    // CE 0.20.3 string projection: shared strings carry r (stored <si> markup) and h; inline and str cells
    // only h; legacy comments carry r, T:false and nonempty h; VML note visibility becomes c.hidden.
    const base = new Workbook();
    const baseSheet = base.addWorksheet('Strings');
    baseSheet.getCell('A1').value = 'x';
    baseSheet.getCell('A1').note = 'n';
    const archive = SheetJS.CFB.read(new Uint8Array(await base.xlsx.writeBuffer({ useSharedStrings: true })), { type: 'array' });
    const partPath = (pattern) => archive.FullPaths.find((path) => pattern.test(path)).replace(/^Root Entry/, '');
    const ns = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"';
    const items = [
      '<t>plain</t>',
      '<t>a&amp;b&lt;c&gt;"\'</t>',
      '<t xml:space="preserve">l1\nl2\ttab</t>',
      '<t>x_x000D_y</t>',
      '<t>漢字</t><rPh sb="0" eb="2"><t>カンジ</t></rPh><phoneticPr fontId="1"/>',
      '<r><rPr><b/><sz val="9"/><color rgb="FFFF0000"/><rFont val="Calibri"/></rPr><t>B</t></r><r><t xml:space="preserve"> tail</t></r>',
      '<r><rPr><i/></rPr><t>I</t></r><r><rPr><u val="double"/><vertAlign val="subscript"/><outline/><shadow/></rPr><t>U</t></r><r><t></t></r>',
      '<r><rPr><b/></rPr><t>only</t></r>',
      '\n  <t>ws</t>\n',
    ];
    const cells = items.map((_, index) => `<c r="A${index + 1}" t="s"><v>${index}</v></c>`);
    cells[0] += '<c r="B1" t="inlineStr"><is><t>in&amp;</t></is></c><c r="C1" t="str"><f>"a"&amp;"&lt;"</f><v>a&lt;b</v></c>';
    cells[1] += '<c r="B2" t="inlineStr"><is><r><rPr><b/></rPr><t>ib</t></r><r><t>ip</t></r></is></c><c r="C2" t="s"><f>"q"</f><v>0</v></c>';
    const sheetXml = `<worksheet ${ns} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dimension ref="A1:D9"/><sheetData>${cells.map((cell, index) => `<row r="${index + 1}">${cell}</row>`).join('')}</sheetData><legacyDrawing r:id="LEGACY"/></worksheet>`;
    const commentXml = `<comments ${ns}><authors><author>Au</author></authors><commentList>` +
      '<comment ref="A1" authorId="0"><text><t>n&amp;1</t></text></comment>' +
      '<comment ref="A2" authorId="0"><text><r><rPr><b/><sz val="9"/></rPr><t>Bob:</t></r><r><t xml:space="preserve"> hi</t></r></text></comment>' +
      '<comment ref="A3" authorId="0"><text><t></t></text></comment>' +
      '<comment ref="D1" authorId="0"><text><t>stub</t></text></comment></commentList></comments>';
    const shape = (row, column, visible) => `<v:shape type="#_x0000_t202"><x:ClientData ObjectType="Note">${visible ? '<x:Visible/>' : ''}<x:Row>${row}</x:Row><x:Column>${column}</x:Column></x:ClientData></v:shape>`;
    const vmlXml = '<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel">' +
      shape(0, 0, false) + shape(1, 0, true) + shape(2, 0, false) + shape(0, 3, true) + '</xml>';
    const sheetPath = partPath(/worksheets\/sheet1\.xml$/);
    const rels = new TextDecoder().decode(SheetJS.CFB.find(archive, partPath(/worksheets\/_rels\/sheet1\.xml\.rels$/)).content);
    const legacyId = rels.match(/Id="([^"]+)"[^>]*vmlDrawing|vmlDrawing[^>]*Id="([^"]+)"/).slice(1).find(Boolean);
    for (const [path, xml] of [[sheetPath, sheetXml.replace('LEGACY', legacyId)], [partPath(/sharedStrings\.xml$/), `<sst ${ns}>${items.map((item) => `<si>${item}</si>`).join('')}</sst>`],
      [partPath(/comments\d*\.xml$/), commentXml], [partPath(/vmlDrawing\d*\.vml$/), vmlXml]]) {
      assert.ok(SheetJS.CFB.utils.cfb_del(archive, path));
      SheetJS.CFB.utils.cfb_add(archive, path.slice(1), Buffer.from(xml));
    }
    const stringBytes = new Uint8Array(SheetJS.CFB.write(archive, { type: 'array', fileType: 'zip' }));
    const expectedStrings = {
      A1: {
        t: "s",
        v: "plain",
        r: "<t>plain</t>",
        h: "plain",
        w: "plain",
        c: [
          {
            a: "Au",
            t: "n&1",
            r: "<t>n&amp;1</t>",
            T: false,
            h: "n&amp;1"
          }
        ]
      },
      A2: {
        t: "s",
        v: "a&b<c>\"'",
        r: "<t>a&amp;b&lt;c&gt;\"'</t>",
        h: "a&amp;b&lt;c&gt;&quot;&apos;",
        w: "a&b<c>\"'",
        c: [
          {
            a: "Au",
            t: "Bob: hi",
            r: "<r><rPr><b/><sz val=\"9\"/></rPr><t>Bob:</t></r><r><t xml:space=\"preserve\"> hi</t></r>",
            T: false,
            h: "<span style=\"font-size:9pt;\"><b>Bob:</b></span> hi"
          }
        ]
      },
      A3: {
        t: "s",
        v: "l1\nl2\ttab",
        r: "<t xml:space=\"preserve\">l1\nl2\ttab</t>",
        h: "l1<br/>l2&#x0009;tab",
        w: "l1\nl2\ttab",
        c: [
          {
            a: "Au",
            t: "",
            r: "<t></t>",
            T: false
          }
        ]
      },
      A4: {
        t: "s",
        v: "x\ry",
        r: "<t>x_x000D_y</t>",
        h: "x&#x000d;y",
        w: "x\ry"
      },
      A5: {
        t: "s",
        v: "漢字",
        r: "<t>漢字</t><rPh sb=\"0\" eb=\"2\"><t>カンジ</t></rPh><phoneticPr fontId=\"1\"/>",
        h: "漢字",
        w: "漢字"
      },
      A6: {
        t: "s",
        v: "B tail",
        r: "<r><rPr><b/><sz val=\"9\"/><color rgb=\"FFFF0000\"/><rFont val=\"Calibri\"/></rPr><t>B</t></r><r><t xml:space=\"preserve\"> tail</t></r>",
        h: "<span style=\"font-size:9pt;\"><b>B</b></span> tail",
        w: "B tail"
      },
      A7: {
        t: "s",
        v: "IU",
        r: "<r><rPr><i/></rPr><t>I</t></r><r><rPr><u val=\"double\"/><vertAlign val=\"subscript\"/><outline/><shadow/></rPr><t>U</t></r><r><t></t></r>",
        h: "<span style=\"\"><i>I</i></span><span style=\"text-decoration: underline;text-underline-style:double;text-effect: outline;text-shadow: auto;\"><sub>U</sub></span>",
        w: "IU"
      },
      A8: {
        t: "s",
        v: "only",
        r: "<r><rPr><b/></rPr><t>only</t></r>",
        h: "<span style=\"\"><b>only</b></span>",
        w: "only"
      },
      A9: {
        t: "s",
        v: "ws",
        r: "<t>ws</t>",
        h: "ws",
        w: "ws"
      },
      B1: {
        t: "s",
        v: "in&",
        h: "in&amp;",
        w: "in&"
      },
      B2: {
        t: "s",
        v: "ibip",
        h: "<span style=\"\"><b>ib</b></span>ip",
        w: "ibip"
      },
      C1: {
        t: "s",
        v: "a<b",
        h: "a&lt;b",
        w: "a<b"
      },
      C2: {
        t: "s",
        v: "plain",
        r: "<t>plain</t>",
        h: "plain",
        w: "plain"
      },
      D1: {
        t: "z",
        c: [
          {
            a: "Au",
            t: "stub",
            r: "<t>stub</t>",
            T: false,
            h: "stub"
          }
        ]
      }
    };
    const expectedHidden = {
      A1: true,
      A2: false,
      A3: true,
      D1: false
    };
    const withoutHtml = (cells) => Object.fromEntries(Object.entries(cells).map(([key, { h, ...cell }]) =>
      [key, cell.c ? { ...cell, c: cell.c.map(({ h: html, ...comment }) => comment) } : cell]));
    const projectedStrings = (options) => {
      const read = SheetJS.read(stringBytes, { type: 'array', ...options }).Sheets.Strings;
      const keys = read['!data']
        ? read['!data'].flatMap((row, r) => (row ?? []).map((cell, c) => cell && SheetJS.utils.encode_cell({ r, c }))).filter(Boolean)
        : Object.keys(read).filter((key) => key[0] !== '!');
      const cells = {};
      const hidden = {};
      for (const key of keys.sort()) {
        const cell = SheetJS.utils.sheet_get_cell(read, key);
        cells[key] = Object.fromEntries(['t', 'v', 'r', 'h', 'w', 'c'].filter((field) => cell[field] !== undefined)
          .map((field) => [field, field === 'c' ? cell.c.map((comment) => ({ ...comment })) : cell[field]]));
        if (cell.c) hidden[key] = cell.c.hidden;
      }
      return { cells, hidden };
    };
    for (const options of [{}, { dense: true }, { sheetStubs: true }]) {
      assert.deepEqual(projectedStrings(options), { cells: expectedStrings, hidden: expectedHidden });
    }
    assert.deepEqual(projectedStrings({ cellHTML: false }), { cells: withoutHtml(expectedStrings), hidden: expectedHidden });
    const stringBook = SheetJS.read(stringBytes, { type: 'array' });
    const stringOutput = { type: 'array', bookType: 'xlsx' };
    assert.deepEqual(new Uint8Array(SheetJS.write(stringBook, stringOutput)), stringBytes);
    const strings = stringBook.Sheets.Strings;
    strings.A9.v = 'changed';
    SheetJS.utils.cell_add_comment(strings.A1, 'more', 'Ed');
    const rewritten = SheetJS.read(SheetJS.write(stringBook, stringOutput), { type: 'array' }).Sheets.Strings;
    assert.equal(rewritten.A9.v, 'changed');
    assert.equal(rewritten.A9.h, 'changed');
    assert.deepEqual(rewritten.A1.c.map(({ a, t, T }) => [a, t, T]), [['Au', 'n&1', false], ['Ed', 'more', false]]);
    strings.A2.c[0].T = true;
    assert.throws(() => SheetJS.write(stringBook, stringOutput), { code: 'UNSUPPORTED_CELL' });
  }

  // Rust caches the shared-string item positions per package generation.
  // Reads after edits that append or reuse items must see the current table,
  // before and after a save and reload.
  {
    const source = new Workbook();
    const sheet = source.addWorksheet('S');
    sheet.getCell('A1').value = 'a';
    sheet.getCell('A2').value = { richText: [{ text: 'R', font: { bold: true } }, { text: 'r' }] };
    sheet.getCell('A3').value = 'b';
    const bytes = new Uint8Array(await source.xlsx.writeBuffer({ useSharedStrings: true }));
    assert.ok(SheetJS.CFB.find(SheetJS.CFB.read(bytes, { type: 'array' }), '/xl/sharedStrings.xml'));
    const book = new Workbook();
    await book.xlsx.load(bytes);
    const loaded = book.getWorksheet('S');
    const bold = { richText: [{ font: { name: 'Calibri', size: 11, bold: true }, text: 'R' }, { text: 'r' }] };
    const italic = { richText: [{ font: { name: 'Calibri', size: 11, italic: true }, text: 'X' }, { text: 'y' }] };
    assert.deepEqual(['A1', 'A2', 'A3'].map((address) => loaded.getCell(address).value), ['a', bold, 'b']);
    loaded.getCell('B1').value = 'new';
    loaded.getCell('B2').value = { richText: [{ text: 'X', font: { italic: true } }, { text: 'y' }] };
    loaded.getCell('A3').value = 'a';
    const check = (target) => assert.deepEqual(
      ['A1', 'A2', 'A3', 'B1', 'B2'].map((address) => target.getCell(address).value), ['a', bold, 'a', 'new', italic]);
    check(loaded);
    const saved = new Uint8Array(await book.xlsx.writeBuffer());
    const reloaded = new Workbook();
    await reloaded.xlsx.load(saved);
    check(reloaded.getWorksheet('S'));
    const projected = SheetJS.read(saved, { type: 'array' }).Sheets.S;
    assert.deepEqual(['A3', 'B1', 'B2'].map((address) => [projected[address].t, projected[address].v, projected[address].r]), [
      ['s', 'a', '<t>a</t>'], ['s', 'new', '<t>new</t>'],
      ['s', 'Xy', '<r><rPr><i/><sz val="11"/><rFont val="Calibri"/></rPr><t>X</t></r><r><t>y</t></r>'],
    ]);
    // A new workbook reads its cells from Rust after a save: strings written
    // afterwards are new items of the table the save already indexed.
    const fresh = new Workbook();
    const freshSheet = fresh.addWorksheet('F');
    freshSheet.getCell('A1').value = 'first';
    assert.equal(SheetJS.read(new Uint8Array(await fresh.xlsx.writeBuffer()), { type: 'array' }).Sheets.F.A1.v, 'first');
    freshSheet.getCell('A1').value = 'second';
    assert.equal(freshSheet.getCell('A1').value, 'second');
    freshSheet.getCell('B1').value = { richText: [{ text: 'X', font: { italic: true } }, { text: 'y' }] };
    freshSheet.getCell('C1').value = 'first';
    const freshCheck = (target) => assert.deepEqual(
      ['A1', 'B1', 'C1'].map((address) => target.getCell(address).value), ['second', italic, 'first']);
    freshCheck(freshSheet);
    const freshReloaded = new Workbook();
    await freshReloaded.xlsx.load(await fresh.xlsx.writeBuffer());
    freshCheck(freshReloaded.getWorksheet('F'));
  }

  {
    // SheetJS 0.20.3 write options. Its writeSync never rejects a key, and each writer reads only
    // some. Expected values: 0.20.3 output (byte-identical with and without the ignored keys).
    const optionBook = () => {
      const book = SheetJS.utils.book_new();
      SheetJS.utils.book_append_sheet(book, SheetJS.utils.aoa_to_sheet([['name', 'num'], ['x  y', 2]]), 'S');
      SheetJS.utils.book_append_sheet(book, SheetJS.utils.aoa_to_sheet([['q']]), 'T');
      return book;
    };
    const unused = {
      compression: true, bookSST: true, cellDates: true, Props: { Author: 'me' }, cellStyles: true, sheetStubs: true,
      ignoreEC: false, themeXLSX: 'x', WTF: 1, numbers: 'x', codepage: 1252, password: 'pw', bookVBA: true, FS: ';', RS: '\r\n',
      sheet: 'T',
    };
    const every = Object.keys(unused);
    const except = (...keys) => every.filter((key) => !keys.includes(key));
    const ignoredByFormat = {
      xlsx: ['sheet', 'WTF', 'numbers', 'bookVBA', 'FS', 'RS'],
      xlsm: ['sheet', 'WTF', 'numbers', 'bookVBA', 'FS', 'RS'],
      xlsb: ['sheet', 'WTF', 'cellDates', 'cellStyles', 'sheetStubs', 'numbers', 'codepage', 'bookVBA', 'FS', 'RS'],
      ods: except('compression', 'password'),
      fods: every,
      xlml: except('Props'),
      biff8: except('bookSST', 'WTF'),
      biff5: except('bookSST', 'WTF'),
      wk3: every,
      biff2: except('WTF', 'sheet'),
      csv: except('FS', 'RS', 'sheet'),
      txt: except('sheet'),
      html: except('sheet'),
      prn: except('sheet'),
      dif: except('sheet'),
      sylk: except('sheet'),
      eth: except('sheet'),
      rtf: except('sheet'),
      dbf: except('sheet'),
      wk1: except('sheet'),
    };
    const render = (bytes) => {
      const book = SheetJS.read(bytes, { type: 'array' });
      return JSON.stringify(book.SheetNames.map((name) => SheetJS.utils.sheet_to_csv(book.Sheets[name])));
    };
    for (const [bookType, keys] of Object.entries(ignoredByFormat)) {
      const options = Object.fromEntries(keys.map((key) => [key, unused[key]]));
      const plain = render(SheetJS.write(optionBook(), { bookType, type: 'array' }));
      assert.equal(render(SheetJS.write(optionBook(), { bookType, type: 'array', ...options })), plain, bookType);
      if (['fods', 'xlml', 'csv', 'txt', 'html', 'prn', 'dif', 'eth', 'rtf'].includes(bookType)) {
        assert.equal(SheetJS.write(optionBook(), { bookType, type: 'string', ...options }),
          SheetJS.write(optionBook(), { bookType, type: 'string' }), bookType);
      }
      const file = join(directory, 'unused-write-options.' + (bookType === 'biff8' ? 'xls' : bookType));
      SheetJS.writeFile(optionBook(), file, { bookType, ...options });
      assert.equal(render(fs.readFileSync(file)), plain, bookType);
    }
    // Options with an effect that this adapter does not implement stay rejected.
    for (const [bookType, options] of [['xlsb', { ignoreEC: false }], ['xlsb', { themeXLSX: 'x' }],
      ['xlsx', { password: 'pw' }], ['xlsb', { password: 'pw' }], ['ods', { password: 'pw' }]]) {
      assert.throws(() => SheetJS.write(optionBook(), { bookType, type: 'array', ...options }),
        { code: 'UNSUPPORTED_OPTIONS' }, bookType);
    }
    // 0.20.3 write and writeFile check a truthy sheet for every format; writeXLSX does not.
    for (const bookType of ['xlsx', 'xlsb', 'ods', 'fods', 'xlml', 'biff8', 'biff5', 'wk3', 'csv', 'biff2']) {
      assert.throws(() => SheetJS.write(optionBook(), { bookType, type: 'array', sheet: 'nope' }),
        { message: 'Sheet not found: nope : string' }, bookType);
      assert.throws(() => SheetJS.write(optionBook(), { bookType, type: 'array', sheet: 2 }),
        { message: 'Sheet not found: 2 : number' }, bookType);
      // 0.20.3 BIFF2-4 also refuses a numeric index ("Sheet not found: 1"); not followed here.
      if (bookType !== 'biff2') assert.doesNotThrow(() => SheetJS.write(optionBook(), { bookType, type: 'array', sheet: 1 }), bookType);
    }
    const missingSheetFile = join(directory, 'missing-sheet.xlsx');
    assert.throws(() => SheetJS.writeFile(optionBook(), missingSheetFile, { sheet: 'nope' }),
      { message: 'Sheet not found: nope : string' });
    assert.equal(fs.existsSync(missingSheetFile), false);
    assert.throws(() => SheetJS.write(optionBook(), { type: 'file', file: missingSheetFile, sheet: 'nope' }),
      { message: 'Sheet not found: nope : string' });
    assert.ok(SheetJS.writeXLSX(optionBook(), { type: 'array', sheet: 'nope' }).byteLength > 0);

    // A loaded XLSX keeps Props and Custprops when written as XLSB or XLS (0.20.3: Alice/T/Co, bar/3/true).
    const propertyBook = optionBook();
    propertyBook.Props = { Author: 'Alice', Title: 'T', Company: 'Co', CreatedDate: new Date(Date.UTC(2024, 0, 2)) };
    propertyBook.Custprops = { Foo: 'bar', Num: 3, Flag: true };
    const propertySource = SheetJS.write(propertyBook, { bookType: 'xlsx', type: 'array' });
    for (const bookType of ['xlsb', 'biff8']) {
      const written = SheetJS.read(SheetJS.write(SheetJS.read(propertySource, { type: 'array' }), { bookType, type: 'array' }),
        { type: 'array' });
      assert.deepEqual([written.Props.Author, written.Props.Title, written.Props.Company], ['Alice', 'T', 'Co'], bookType);
      assert.deepEqual([written.Custprops.Foo, written.Custprops.Num, written.Custprops.Flag], ['bar', 3, true], bookType);
    }
    // 0.20.3 writes no property streams for BIFF5.
    const biff5 = SheetJS.read(SheetJS.write(SheetJS.read(propertySource, { type: 'array' }), { bookType: 'biff5', type: 'array' }),
      { type: 'array' });
    assert.equal(biff5.Props?.Author, undefined);
    const changedDate = SheetJS.read(propertySource, { type: 'array' });
    changedDate.Props.CreatedDate.setTime(0);
    for (const bookType of ['xlsx', 'xlsb', 'biff8']) {
      assert.throws(() => SheetJS.write(changedDate, { bookType, type: 'array' }), { code: 'UNSUPPORTED_STRUCTURE' }, bookType);
    }

    // ODS/FODS encode runs of spaces, edge spaces and tabs (ODF 6.1.2) so that ODF readers keep them.
    const spaced = [' ', 'a  b', ' lead', 'trail ', 'tab\tx', 'nl\ny', '  two', 'x\n y ', 'a \tb', '<&> \'"  q'];
    const spacedBook = SheetJS.utils.book_new();
    SheetJS.utils.book_append_sheet(spacedBook, SheetJS.utils.aoa_to_sheet([spaced]), 'S');
    spacedBook.Sheets.S.A1.c = [{ a: 'me', t: ' c  d ' }];
    const columns = spaced.map((_, index) => SheetJS.utils.encode_cell({ r: 0, c: index }));
    for (const bookType of ['ods', 'fods']) {
      const sheet = SheetJS.read(SheetJS.write(spacedBook, { bookType, type: 'array' }), { type: 'array' }).Sheets.S;
      assert.deepEqual(columns.map((address) => sheet[address].v), spaced, bookType);
      assert.equal(sheet.A1.c[0].t, ' c  d ', bookType);
    }
    // The same paragraphs as 0.20.3, except that a space after a line break is kept as text:s.
    const paragraphs = [...SheetJS.write(spacedBook, { bookType: 'fods', type: 'string' })
      .matchAll(/<table:table-cell[^>]*office:value-type="string"[^>]*>(?:<office:annotation[\s\S]*?<\/office:annotation>)?((?:<text:p>.*?<\/text:p>)+)/g)]
      .map((match) => match[1]);
    assert.deepEqual(paragraphs, [
      '<text:p><text:s/></text:p>', '<text:p>a<text:s text:c="2"/>b</text:p>', '<text:p><text:s/>lead</text:p>',
      '<text:p>trail<text:s/></text:p>', '<text:p>tab<text:tab/>x</text:p>', '<text:p>nl</text:p><text:p>y</text:p>',
      '<text:p><text:s text:c="2"/>two</text:p>', '<text:p>x</text:p><text:p><text:s/>y<text:s/></text:p>',
      '<text:p>a <text:tab/>b</text:p>', '<text:p>&lt;&amp;&gt; &apos;&quot;<text:s text:c="2"/>q</text:p>',
    ]);
  }

  {
    // XLSX Workbook metadata and !cols raw attributes. Expected values are CE 0.20.3's
    // for the same patched package (keys that need Rust are absent, see the plan ledger).
    const output = { bookType: 'xlsx', type: 'array' };
    const patch = (bytes, edits) => {
      const archive = SheetJS.CFB.read(new Uint8Array(bytes), { type: 'array' });
      for (const [path, edit] of Object.entries(edits)) {
        const entry = SheetJS.CFB.find(archive, '/' + path);
        const content = edit(entry ? Buffer.from(entry.content).toString() : '');
        if (entry) assert.ok(SheetJS.CFB.utils.cfb_del(archive, '/' + path));
        SheetJS.CFB.utils.cfb_add(archive, path, typeof content === 'string' ? Buffer.from(content) : content);
      }
      return Uint8Array.from(SheetJS.CFB.write(archive, { fileType: 'zip', type: 'array' }));
    };
    const entryText = (bytes, path) =>
      Buffer.from(SheetJS.CFB.find(SheetJS.CFB.read(new Uint8Array(bytes), { type: 'array' }), '/' + path).content).toString();
    const metaBook = SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([['a', 'b', 'c', 'd', 'e', 'f', 'g']]), 'One');
    SheetJS.utils.book_append_sheet(metaBook, SheetJS.utils.aoa_to_sheet([[1]]), 'Two');
    const metaBase = SheetJS.write(metaBook, output);
    const metaCols = '<cols><col min="1" max="1" width="20.5" customWidth="1"/><col min="2" max="2" width="9" hidden="1" customWidth="1"/>' +
      '<col min="3" max="3" width="15" style="1"/><col min="4" max="4" hidden="1"/>' +
      '<col min="5" max="6" width="12" bestFit="1" outlineLevel="1" customWidth="true"/><col min="7" max="7" width="8" hidden="0" outlineLevel="0"/></cols>';
    const metaBytes = patch(metaBase, {
      'xl/workbook.xml': (xml) => xml.replace(/<workbookPr[^>]*\/>/, '<fileVersion appName="xl" lastEdited="7"/><workbookPr filterPrivacy="1" defaultThemeVersion="164011"/>')
        .replace(/<bookViews>.*<\/bookViews>/, '<bookViews><workbookView xWindow="360" yWindow="15" windowWidth="20955" windowHeight="9720" tabRatio="842" activeTab="1"/>' +
          '<workbookView firstSheet="1" visibility="hidden"/></bookViews>')
        .replace('</workbook>', '<calcPr calcId="191029" calcCompleted="0"/></workbook>'),
      'xl/worksheets/sheet1.xml': (xml) => xml.replace('<sheetData', metaCols + '<sheetData'),
    });
    const metaRead = SheetJS.read(metaBytes, { cellStyles: true });
    assert.deepEqual(metaRead.Workbook, {
      WBProps: { date1904: false },
      WBView: [
        { xWindow: '360', xwindow: '360', yWindow: '15', ywindow: '15', windowWidth: '20955', windowwidth: '20955',
          windowHeight: '9720', windowheight: '9720', activeTab: 1, activetab: '1', firstSheet: 0, visibility: 'visible' },
        { firstSheet: 1, firstsheet: '1', visibility: 'hidden', activeTab: 0 },
      ],
      Sheets: [{ name: 'One', sheetId: '1', sheetid: '1', Hidden: 0 }, { name: 'Two', sheetId: '2', sheetid: '2', Hidden: 0 }],
      xmlns: 'http://schemas.openxmlformats.org/spreadsheetml/2006/main',
    });
    assert.deepEqual(Object.keys(metaRead.Workbook), ['WBProps', 'WBView', 'Sheets', 'xmlns']);
    const noViews = patch(metaBase, { 'xl/workbook.xml': (xml) => xml.replace(/<bookViews>.*<\/bookViews>/, '') });
    assert.deepEqual(SheetJS.read(noViews).Workbook.WBView, []);
    // 0.20.3 width: null (its JSON copy of NaN) for a <col> without width; hidden parsed when present.
    const metaColumns = [
      { width: 20.5, wpx: 123, wch: 19.67, MDW: 6 },
      { width: 9, hidden: true, wpx: 54, wch: 8.17, MDW: 6 },
      { width: 15, wpx: 90, wch: 14.17, MDW: 6 },
      { width: null, hidden: true },
      { width: 12, outlineLevel: '1', outlinelevel: '1', level: 1, wpx: 72, wch: 11.17, MDW: 6 },
      { width: 12, outlineLevel: '1', outlinelevel: '1', level: 1, wpx: 72, wch: 11.17, MDW: 6 },
      { width: 8, hidden: false, outlineLevel: '0', outlinelevel: '0', level: 0, wpx: 48, wch: 7.17, MDW: 6 },
    ];
    for (const dense of [false, true]) {
      assert.deepEqual(SheetJS.read(metaBytes, { cellStyles: true, dense }).Sheets.One['!cols'], metaColumns);
    }
    // The loaded workbook writes back unchanged with the new keys and a null width.
    void metaRead.Sheets.One['!cols'];
    assert.deepEqual(new Uint8Array(SheetJS.write(metaRead, output)), metaBytes);
    metaRead.Workbook.WBView[0].activeTab = 0;
    metaRead.Workbook.Sheets[0].sheetId = '9';
    assert.deepEqual(new Uint8Array(SheetJS.write(metaRead, output)), metaBytes);
    metaRead.Sheets.One['!cols'][3].width = 30;
    assert.deepEqual(SheetJS.read(SheetJS.write(metaRead, output), { cellStyles: true }).Sheets.One['!cols'][3],
      { width: 30, hidden: true, wpx: 180, wch: 29.17, MDW: 6 });
    const loadedViews = SheetJS.read(metaBytes);
    loadedViews.Workbook.Views = [{ RTL: true }];
    assert.throws(() => SheetJS.write(loadedViews, output), { code: 'UNSUPPORTED_STRUCTURE', message: /Workbook\.Views/ });

    // A workbook shaped like 0.20.3's read result (keys 0.20.3's writer ignores,
    // WBProps defaults, raw !cols keys) is written; Views[0].RTL sets rightToLeft.
    const referenceShaped = () => ({
      SheetNames: ['One', 'Two'],
      Sheets: {
        One: { '!ref': 'A1', A1: { t: 's', v: 'a' }, '!cols': [
          { width: 20.5, customwidth: '1', wpx: 123, wch: 19.67, MDW: 6 }, { width: 15, style: '1', wpx: 90, wch: 14.17, MDW: 6 },
          { hidden: true, width: null },
          { width: 12, bestFit: '1', bestfit: '1', outlineLevel: '1', outlinelevel: '1', customwidth: 'true', level: 1, wpx: 72, wch: 11.17, MDW: 6 },
        ] },
        Two: { '!ref': 'A1', A1: { t: 'n', v: 1 } },
      },
      Workbook: {
        AppVersion: { appName: 'xl', appname: 'xl', lastEdited: '7', lastedited: '7' },
        WBProps: { allowRefreshQuery: false, autoCompressPictures: true, backupFile: false, checkCompatibility: false, CodeName: '',
          date1904: false, defaultThemeVersion: 0, filterPrivacy: false, hidePivotFieldList: false, promptedSolutions: false,
          publishItems: false, refreshAllConnections: false, saveExternalLinkValues: true, showBorderUnselectedTables: true,
          showInkAnnotation: true, showObjects: 'all', showPivotChartFilter: false, updateLinks: 'userSet' },
        WBView: [{ xWindow: '360', xwindow: '360', tabRatio: 842, tabratio: '842', activeTab: 1, activetab: '1', autoFilterDateGrouping: true }],
        Sheets: [{ name: 'One', sheetId: '1', sheetid: '1', id: 'rId1', Hidden: 0 }, { name: 'Two', sheetId: '2', sheetid: '2', id: 'rId5', Hidden: 0, CodeName: 'Sheet2' }],
        CalcPr: { calcId: '191029', calcid: '191029', calcCompleted: '0', calcMode: 'auto' },
        Names: [],
        xmlns: 'http://schemas.openxmlformats.org/spreadsheetml/2006/main',
        Views: [{ zoom: 80, RTL: true }],
      },
    });
    const shapedBytes = SheetJS.write(referenceShaped(), output);
    for (const path of ['xl/worksheets/sheet1.xml', 'xl/worksheets/sheet2.xml']) {
      assert.match(entryText(shapedBytes, path), /<sheetView [^>]*rightToLeft="1"/, path);
    }
    const shapedColumns = SheetJS.read(shapedBytes, { cellStyles: true }).Sheets.One['!cols'];
    // 0.20.3 writes the hidden column without width; Rust adds its default width (not compared).
    assert.deepEqual(shapedColumns.map((column) => [column.hidden, column.level]),
      [[undefined, undefined], [undefined, undefined], [true, undefined], [undefined, 1]]);
    assert.deepEqual([0, 1, 3].map((index) => shapedColumns[index].width), [20.5, 15, 12]);
    for (const views of [[{ RTL: false }], [], [, { RTL: true }]]) {
      const book = referenceShaped();
      book.Workbook.Views = views;
      assert.doesNotMatch(entryText(SheetJS.write(book, output), 'xl/worksheets/sheet1.xml'), /rightToLeft/);
    }
    for (const [edit, code] of [
      [(book) => { book.Workbook.WBProps.filterPrivacy = true; }, 'UNSUPPORTED_STRUCTURE'],
      [(book) => { book.Workbook.WBProps.CodeName = 'ThisBook'; }, 'UNSUPPORTED_STRUCTURE'],
      [(book) => { book.Workbook.WBProps.defaultThemeVersion = 164011; }, 'UNSUPPORTED_STRUCTURE'],
      [(book) => { book.Workbook.WBProps.unknown = 1; }, 'UNSUPPORTED_STRUCTURE'],
      [(book) => { book.Workbook.Views = [{ RTL: 1 }]; }, 'INVALID_VALUE'],
      [(book) => { book.Workbook.Views = [{ RTL: true, extra: 1 }]; }, 'UNSUPPORTED_STRUCTURE'],
      [(book) => { book.Workbook.Views = { RTL: true }; }, 'UNSUPPORTED_STRUCTURE'],
      [(book) => { book.Sheets.One['!cols'][0].unknown = 1; }, 'UNSUPPORTED_STRUCTURE'],
    ]) {
      const book = referenceShaped();
      edit(book);
      assert.throws(() => SheetJS.write(book, output), { code });
    }

    // A loaded VBA workbook keeps its sheetPr; a CodeName would need Rust to write.
    const vbaBase = SheetJS.write(SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[1]]), 'M'), { type: 'array', bookType: 'xlsm' });
    const vbaBytes = patch(vbaBase, {
      'xl/vbaProject.bin': () => Uint8Array.of(1, 2, 3),
      'xl/_rels/workbook.xml.rels': (xml) => xml.replace('</Relationships>',
        '<Relationship Id="rId77" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="vbaProject.bin"/></Relationships>'),
      '[Content_Types].xml': (xml) => xml.replace('</Types>', '<Override PartName="/xl/vbaProject.bin" ContentType="application/vnd.ms-office.vbaProject"/></Types>'),
    });
    const vbaBook = SheetJS.read(vbaBytes, { bookVBA: true });
    assert.deepEqual(new Uint8Array(SheetJS.write(vbaBook, { type: 'array', bookType: 'xlsm' })), vbaBytes);
    vbaBook.Workbook.Sheets[0].CodeName = 'Sheet9';
    assert.throws(() => SheetJS.write(vbaBook, { type: 'array', bookType: 'xlsm' }), { code: 'UNSUPPORTED_STRUCTURE', message: /CodeName/ });
    const plainBook = SheetJS.read(vbaBase);
    plainBook.Workbook.Sheets[0].CodeName = 'Sheet9';
    assert.deepEqual(new Uint8Array(SheetJS.write(plainBook, { type: 'array', bookType: 'xlsm' })), new Uint8Array(vbaBase));
  }

  {
    // Missing, single-cell, oversized, smaller and unusable <dimension>: CE 0.20.3 ignores a
    // ref without "first:last" (or reversed/absolute) and derives !ref from every <row>
    // element plus the inserted cells' columns; with nodim, from the cells only.
    // Expected values come from 0.20.3 reading the same bytes.
    const dimensionBase = SheetJS.write(SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([[1]]), 'S'), { type: 'array', bookType: 'xlsx' });
    const dimensionBook = (head) => {
      const archive = SheetJS.CFB.read(new Uint8Array(dimensionBase), { type: 'array' });
      const xml = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        + head + '<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c><c r="B1" t="inlineStr"><is><t>b</t></is></c></row>'
        + '<row r="2"><c r="A2"><v>1</v></c><c r="B2"><v>2</v></c><c r="D2" s="0"/></row>'
        + '<row r="3"><c r="C3"><v>3</v></c></row><row r="5" ht="20" customHeight="1"/></sheetData></worksheet>';
      assert.ok(SheetJS.CFB.utils.cfb_del(archive, '/xl/worksheets/sheet1.xml'));
      SheetJS.CFB.utils.cfb_add(archive, 'xl/worksheets/sheet1.xml', Buffer.from(xml));
      return Uint8Array.from(SheetJS.CFB.write(archive, { type: 'array', fileType: 'zip' }));
    };
    const dimensionXml = (bytes) => Buffer.from(SheetJS.CFB.find(SheetJS.CFB.read(new Uint8Array(bytes), { type: 'array' }),
      '/xl/worksheets/sheet1.xml').content).toString().match(/<dimension[^>]*>/)?.[0];
    const cellKeys = (sheet) => sheet['!data']
      ? sheet['!data'].flatMap((row, r) => (row ?? []).map((cell, c) => cell ? SheetJS.utils.encode_cell({ r, c }) : undefined).filter(Boolean))
      : Object.keys(sheet).filter((key) => key[0] !== '!');
    const derived = { '': 'A1:C5', dense: 'A1:C5', sheetRows: 'A1:B2', nodim: 'A1:C3', sheetStubs: 'A1:D5' };
    const expectations = {
      '': derived, '<dimension ref="A1"/>': derived, '<dimension ref="C3:A1"/>': derived, '<dimension ref="$A$1:$B$2"/>': derived,
      '<dimension ref="A1:Z99"/>': { '': 'A1:Z99', dense: 'A1:Z99', sheetRows: 'A1:B2', nodim: 'A1:C3', sheetStubs: 'A1:Z99' },
      '<dimension ref="A1:A2"/>': { '': 'A1:A2', dense: 'A1:A2', sheetRows: 'A1:A2', nodim: 'A1:C3', sheetStubs: 'A1:A2' },
    };
    for (const [head, refs] of Object.entries(expectations)) {
      const bytes = dimensionBook(head);
      for (const [option, ref] of Object.entries(refs)) {
        const options = option === '' ? {} : option === 'sheetRows' ? { sheetRows: 2 } : { [option]: true };
        const sheet = SheetJS.read(bytes, options).Sheets.S;
        assert.equal(sheet['!ref'], ref, `${head} ${option}`);
        assert.equal(sheet['!fullref'], option === 'sheetRows' && head.includes('Z99') ? 'A1:Z99' : undefined, `${head} ${option}`);
        // Cells outside a range dimension stay as keys, as in 0.20.3.
        assert.deepEqual(cellKeys(sheet), option === 'sheetRows' ? ['A1', 'B1', 'A2', 'B2']
          : option === 'sheetStubs' ? ['A1', 'B1', 'A2', 'B2', 'D2', 'C3'] : ['A1', 'B1', 'A2', 'B2', 'C3'], `${head} ${option}`);
      }
      // A no-edit save is byte-identical: the original <dimension> (or its absence) is kept.
      assert.deepEqual(new Uint8Array(SheetJS.write(SheetJS.read(bytes), { type: 'array' })), bytes);
    }
    const noDimension = SheetJS.read(dimensionBook('')).Sheets.S;
    assert.equal(SheetJS.utils.sheet_to_csv(noDimension), 'a,b,\n1,2,\n,,3\n,,\n,,');
    assert.equal(JSON.stringify(SheetJS.utils.sheet_to_json(noDimension, { header: 1 })), '[["a","b"],[1,2],[null,null,3],[],[]]');
    // Edits inside the derived !ref are written; no <dimension> is invented, and a single-cell
    // one that must grow covers the stored rows and cells (A1:C5 here, not just the edits).
    for (const [head, dimension, ref] of [['', undefined, 'A1:C5'], ['<dimension ref="A1"/>', '<dimension ref="A1:D5"/>', 'A1:D5']]) {
      const book = SheetJS.read(dimensionBook(head));
      book.Sheets.S.B2 = { t: 'n', v: 20 };
      book.Sheets.S.C1 = { t: 's', v: 'new' };
      const output = SheetJS.write(book, { type: 'array' });
      assert.equal(dimensionXml(output), dimension);
      const reread = SheetJS.read(output).Sheets.S;
      assert.deepEqual([reread['!ref'], reread.B2.v, reread.C1.v, reread.C3.v], [ref, 20, 'new', 3]);
      // Outside the derived !ref (or outside a smaller range dimension) edits are refused.
      const outside = SheetJS.read(dimensionBook(head));
      outside.Sheets.S.D9 = { t: 'n', v: 9 };
      assert.throws(() => SheetJS.write(outside, { type: 'array' }), { code: 'UNSUPPORTED_RANGE' });
    }
    const smaller = SheetJS.read(dimensionBook('<dimension ref="A1:A2"/>'));
    smaller.Sheets.S.B2 = { t: 'n', v: 20 };
    assert.throws(() => SheetJS.write(smaller, { type: 'array' }), { code: 'UNSUPPORTED_RANGE' });
  }

  {
    // Japanese Excel strings carry phonetic guide text (<rPh>, <phoneticPr>) in their <si>/<is>.
    // ExcelJS 4.4.0 load reads the base text (or the rich runs) and ignores it; expected values
    // below come from 4.4.0 reading the same items. The phonetic XML is kept on save.
    const ns = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
    const phoneticItem = '<si><t>漢字</t><rPh sb="0" eb="2"><t>カンジ</t></rPh><phoneticPr fontId="1"/></si>';
    const richItem = '<si><r><rPr><b/><sz val="11"/><color theme="1"/><rFont val="游ゴシック"/><family val="3"/><charset val="128"/><scheme val="minor"/></rPr><t>太</t></r>'
      + '<r><rPr><sz val="11"/><color theme="1"/><rFont val="游ゴシック"/><family val="3"/><charset val="128"/><scheme val="minor"/></rPr><t>字</t></r>'
      + '<rPh sb="0" eb="1"><t>フト</t></rPh><rPh sb="1" eb="2"><t>ジ</t></rPh><phoneticPr fontId="1" type="noConversion"/></si>';
    const items = [phoneticItem, richItem, '<si><t>abc</t><phoneticPr fontId="1"/></si>', '<si><t>plain</t></si>',
      '<si><t xml:space="preserve"> 東京 </t><rPh sb="1" eb="3"><t>トウキョウ</t></rPh><phoneticPr fontId="1"/></si>'];
    const inlineCells = '<c r="C1" t="inlineStr"><is><t>漢字</t><rPh sb="0" eb="2"><t>カンジ</t></rPh><phoneticPr fontId="1"/></is></c>'
      + '<c r="C2" t="inlineStr"><is><r><rPr><b/></rPr><t>太</t></r><r><t>字</t></r><rPh sb="0" eb="1"><t>フト</t></rPh></is></c>';
    const phoneticBase = SheetJS.write(SheetJS.utils.book_new(SheetJS.utils.aoa_to_sheet([['x']]), 'S'), { type: 'array', bookType: 'xlsx', bookSST: true });
    const phoneticBook = (inline) => {
      const archive = SheetJS.CFB.read(new Uint8Array(phoneticBase), { type: 'array' });
      const sheet = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="${ns}"><dimension ref="A1:C5"/><sheetData>`
        + `<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><v>1</v></c>${inline ? inlineCells.slice(0, inlineCells.indexOf('<c r="C2"')) : ''}</row>`
        + `<row r="2"><c r="A2" t="s"><v>1</v></c><c r="B2"><v>2</v></c>${inline ? inlineCells.slice(inlineCells.indexOf('<c r="C2"')) : ''}</row>`
        + '<row r="3"><c r="A3" t="s"><v>2</v></c></row><row r="4"><c r="A4" t="s"><v>3</v></c></row><row r="5"><c r="A5" t="s"><v>4</v></c></row>'
        + '</sheetData><phoneticPr fontId="1"/></worksheet>';
      for (const [path, xml] of [['xl/worksheets/sheet1.xml', sheet],
        ['xl/sharedStrings.xml', `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><sst xmlns="${ns}" count="5" uniqueCount="5">${items.join('')}</sst>`]]) {
        assert.ok(SheetJS.CFB.utils.cfb_del(archive, `/${path}`));
        SheetJS.CFB.utils.cfb_add(archive, path, Buffer.from(xml));
      }
      return Uint8Array.from(SheetJS.CFB.write(archive, { type: 'array', fileType: 'zip' }));
    };
    const zipText = (bytes, path) => Buffer.from(SheetJS.CFB.find(SheetJS.CFB.read(new Uint8Array(bytes), { type: 'array' }), `/${path}`).content).toString();
    const font = { bold: true, size: 11, color: { theme: 1 }, name: '游ゴシック', family: 3, charset: 128, scheme: 'minor' };
    const richValue = { richText: [{ font, text: '太' }, { font: { ...font, bold: undefined }, text: '字' }] };
    delete richValue.richText[1].font.bold;
    const expected = { A1: ['漢字', '漢字', ValueType.String], A2: [richValue, '太字', ValueType.RichText],
      A3: ['abc', 'abc', ValueType.String], A4: ['plain', 'plain', ValueType.String], A5: [' 東京 ', ' 東京 ', ValueType.String] };
    const loaded = await new Workbook().xlsx.load(phoneticBook(false));
    const sheet = loaded.getWorksheet('S');
    for (const [address, [value, text, type]] of Object.entries(expected)) {
      const cell = sheet.getCell(address);
      assert.deepEqual([cell.value, cell.text, cell.type, cell.model.value, cell.toString()], [value, text, type, value, text], address);
    }
    assert.equal((await loaded.csv.writeBuffer()).toString().split('\n').filter((line) => !line.startsWith('"')).join('|'), '漢字,1|abc|plain| 東京 ');
    // An unrelated edit, a copy and an overwrite: values as in 4.4.0, and the phonetic <si>
    // items stay byte-identical (4.4.0 drops them; copies carry the base text only).
    sheet.getCell('B1').value = 42;
    sheet.getCell('D1').value = sheet.getCell('A1').value;
    sheet.getCell('A5').value = 'x';
    const saved = await loaded.xlsx.writeBuffer();
    const savedItems = zipText(saved, 'xl/sharedStrings.xml');
    for (const item of [phoneticItem, richItem, items[2], items[4]]) assert.ok(savedItems.includes(item), item);
    const reread = (await new Workbook().xlsx.load(saved)).getWorksheet('S');
    assert.deepEqual(['A1', 'B1', 'D1', 'A2', 'A5'].map((address) => reread.getCell(address).value), ['漢字', 42, '漢字', richValue, 'x']);
    // Row moves keep the original items; a duplicated row reads as in 4.4.0.
    const moved = await new Workbook().xlsx.load(phoneticBook(false));
    moved.getWorksheet('S').spliceRows(2, 1);
    moved.getWorksheet('S').duplicateRow(1, 1, true);
    assert.deepEqual(['A1', 'A2', 'A3', 'A5'].map((address) => moved.getWorksheet('S').getCell(address).value), ['漢字', '漢字', 'abc', ' 東京 ']);
    assert.ok(zipText(await moved.xlsx.writeBuffer(), 'xl/sharedStrings.xml').includes(phoneticItem));
    // Inline strings with phonetic data read like the shared ones. 4.4.0 itself loses the rest of
    // such a sheet (its cell parser never closes the <c>), so these are deliberate differences.
    const inline = await new Workbook().xlsx.load(phoneticBook(true));
    const inlineSheet = inline.getWorksheet('S');
    assert.deepEqual([inlineSheet.getCell('C1').value, inlineSheet.getCell('C2').value, inlineSheet.getCell('B2').value],
      ['漢字', { richText: [{ font: { bold: true }, text: '太' }, { text: '字' }] }, 2]);
    inlineSheet.getCell('B1').value = 7;
    assert.ok(zipText(await inline.xlsx.writeBuffer(), 'xl/worksheets/sheet1.xml').includes(inlineCells.slice(0, inlineCells.indexOf('<c r="C2"'))));
    // The streaming reader caches the base text too (4.4.0 streams the last <t>, "カンジ";
    // a deliberate difference that keeps it consistent with load).
    const streamed = {};
    for await (const worksheet of new stream.xlsx.WorkbookReader(Readable.from([phoneticBook(true)]), { sharedStrings: 'cache' })) {
      for await (const row of worksheet) row.eachCell((cell) => { streamed[cell.address] = cell.text; });
    }
    assert.deepEqual(streamed, { A1: '漢字', B1: '1', C1: '漢字', A2: '太字', B2: '2', C2: '太字', A3: 'abc', A4: 'plain', A5: ' 東京 ' });
  }

  {
    // x14 data bars write what ExcelJS 4.4.0 writes: both lengths (0 and 100 when omitted) and the
    // other attributes only when they differ from its defaults; a gradient bar gets no extension.
    // The tags and reloaded rules below are 4.4.0's own output for the same rules.
    const zipText = (bytes, path) => Buffer.from(SheetJS.CFB.find(SheetJS.CFB.read(new Uint8Array(bytes), { type: 'array' }), `/${path}`).content).toString();
    const base = { type: 'dataBar', cfvo: [{ type: 'min' }, { type: 'max' }], color: { argb: 'FF638EC6' } };
    const x14Defaults = { minLength: 0, maxLength: 100, border: false, gradient: true, negativeBarColorSameAsPositive: true,
      negativeBarBorderColorSameAsPositive: true, axisPosition: 'auto', direction: 'leftToRight' };
    const cases = [
      [{ gradient: false }, '<x14:dataBar minLength="0" maxLength="100" gradient="0">', { gradient: false }],
      [{ border: true }, '<x14:dataBar minLength="0" maxLength="100" border="1">', { border: true }],
      [{ gradient: false, minLength: 5, maxLength: 70 }, '<x14:dataBar minLength="5" maxLength="70" gradient="0">',
        { gradient: false, minLength: 5, maxLength: 70 }],
      [{ gradient: false, border: false, negativeBarColorSameAsPositive: true, negativeBarBorderColorSameAsPositive: true,
        axisPosition: 'auto', direction: 'leftToRight' }, '<x14:dataBar minLength="0" maxLength="100" gradient="0">', { gradient: false }],
      [{ gradient: false, border: true, negativeBarColorSameAsPositive: false, negativeBarBorderColorSameAsPositive: false,
        axisPosition: 'middle', direction: 'rightToLeft' },
      '<x14:dataBar minLength="0" maxLength="100" border="1" gradient="0" negativeBarColorSameAsPositive="0" '
        + 'negativeBarBorderColorSameAsPositive="0" axisPosition="middle" direction="rightToLeft">',
      { gradient: false, border: true, negativeBarColorSameAsPositive: false, negativeBarBorderColorSameAsPositive: false,
        axisPosition: 'middle', direction: 'rightToLeft' }],
      [{ gradient: true }, undefined, undefined],
    ];
    for (const [extra, tag, reloaded] of cases) {
      const book = new Workbook();
      const sheet = book.addWorksheet('S');
      sheet.addConditionalFormatting({ ref: 'A1:A3', rules: [{ ...base, priority: 1, ...extra }] });
      const bytes = await book.xlsx.writeBuffer();
      const xml = zipText(bytes, 'xl/worksheets/sheet1.xml');
      assert.deepEqual(xml.match(/<x14:dataBar[^>]*>/g)?.[0], tag, JSON.stringify(extra));
      const expected = { ...base, priority: 1, ...(reloaded && { ...x14Defaults, ...reloaded }) };
      const strip = ({ x14Id, ...rule }) => rule;
      const reloadedRule = (await new Workbook().xlsx.load(bytes)).getWorksheet('S').conditionalFormattings[0].rules[0];
      assert.deepEqual(strip(reloadedRule), expected, JSON.stringify(extra));
      // A fresh rule reads like the reload (4.4.0 echoes its input; see the ledger).
      assert.deepEqual(strip(sheet.conditionalFormattings[0].rules[0]), expected);
      const copy = new Workbook();
      copy.model = book.model;
      assert.deepEqual(zipText(await copy.xlsx.writeBuffer(), 'xl/worksheets/sheet1.xml').match(/<x14:dataBar[^>]*>/g)?.[0], tag);
    }
    // 4.4.0 writes no extension for gradient:true and so drops border:true; this adapter keeps it.
    const kept = new Workbook();
    kept.addWorksheet('S').addConditionalFormatting({ ref: 'A1', rules: [{ ...base, gradient: true, border: true }] });
    assert.ok(zipText(await kept.xlsx.writeBuffer(), 'xl/worksheets/sheet1.xml').includes('<x14:dataBar minLength="0" maxLength="100" border="1">'));

    // Duplicate priorities: 4.4.0 writes both rules with priority="1"; MS-XLSX 2.6.27 forbids
    // duplicates within a worksheet part, so this adapter keeps refusing them without changes.
    const dupBook = new Workbook();
    const dupSheet = dupBook.addWorksheet('S');
    dupSheet.addConditionalFormatting({ ref: 'A1', rules: [{ type: 'expression', formulae: ['TRUE'], priority: 1 }] });
    const dupBefore = await dupBook.xlsx.writeBuffer();
    for (const rules of [
      [{ type: 'expression', formulae: ['FALSE'], priority: 1 }],
      [{ type: 'expression', formulae: ['FALSE'], priority: 2 }, { type: 'expression', formulae: ['NOT(TRUE)'], priority: 2 }],
    ]) {
      assert.throws(() => dupSheet.addConditionalFormatting({ ref: 'B1', rules }), { code: 'CONDITIONAL_FORMAT_EDIT_REFUSED' });
      assert.deepEqual(await dupBook.xlsx.writeBuffer(), dupBefore);
    }
    // A file that already has duplicates (as 4.4.0 writes them) loads as written and stays editable;
    // the new rule gets the next free priority (4.4.0 numbers it 1 again).
    const dupArchive = SheetJS.CFB.read(new Uint8Array(dupBefore), { type: 'array' });
    const dupXml = zipText(dupBefore, 'xl/worksheets/sheet1.xml').replace('</conditionalFormatting>',
      '<cfRule type="expression" priority="1"><formula>FALSE</formula></cfRule></conditionalFormatting>');
    assert.ok(SheetJS.CFB.utils.cfb_del(dupArchive, '/xl/worksheets/sheet1.xml'));
    SheetJS.CFB.utils.cfb_add(dupArchive, 'xl/worksheets/sheet1.xml', Buffer.from(dupXml));
    const dupLoaded = await new Workbook().xlsx.load(Uint8Array.from(SheetJS.CFB.write(dupArchive, { type: 'array', fileType: 'zip' })));
    const dupLoadedSheet = dupLoaded.getWorksheet('S');
    assert.deepEqual(dupLoadedSheet.conditionalFormattings.map(({ rules }) => rules.map(rule => rule.priority)), [[1, 1]]);
    dupLoadedSheet.addConditionalFormatting({ ref: 'C1', rules: [{ type: 'expression', formulae: ['C1>0'] }] });
    const dupSaved = (await new Workbook().xlsx.load(await dupLoaded.xlsx.writeBuffer())).getWorksheet('S');
    assert.deepEqual(dupSaved.conditionalFormattings.map(({ rules }) => rules.map(rule => rule.priority)), [[1, 1], [2]]);
  }

} finally {
  fs.rmSync(directory, { recursive: true });
}
console.log('Node ESM filesystem and cross-adapter checks PASS');
