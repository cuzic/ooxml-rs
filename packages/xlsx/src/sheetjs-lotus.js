import { record, joinRecords } from './sheetjs-binary-records.js';
import { failure } from './contract.js';
import { decode_range, encode_cell, encode_range } from './sheetjs-address.js';
import { book_new } from './sheetjs-book.js';
import { serialFromDate } from './sheetjs-date.js';
import { format_cell } from './sheetjs-format.js';
import { wk1Formula } from './sheetjs-wk1-formula.js';
import { is_date } from './sheetjs-ssf.js';

const dateFormats = ['mmmm', 'dd-mmm-yyyy', 'dd-mmm', 'mmm-yyyy', '@', 'mm/dd',
  'hh:mm:ss AM/PM', 'hh:mm AM/PM', 'mm/dd/yyyy', 'mm/dd', 'hh:mm:ss', 'hh:mm'];

function cellRecord(row, column, cell) {
  const writeAddress = (view, format) => {
    view.setUint8(4, format);
    view.setUint16(5, column, true);
    view.setUint16(7, row, true);
  };
  if (cell.t === 'n' || cell.t === 'd') {
    const number = cell.t === 'd' ? serialFromDate(new Date(cell.v).getTime(), false) : Number(cell.v);
    const format = cell.z && is_date(cell.z) || cell.t === 'd'
      ? 0xf0 | (dateFormats.indexOf(cell.z) + 1 || 2) : 0xff;
    const integer = Number.isInteger(number) && number >= -32768 && number <= 32767;
    return record(integer ? 0x0d : 0x0e, integer ? 7 : 13, view => {
      writeAddress(view, format);
      if (integer) view.setInt16(9, number, true);
      else view.setFloat64(9, number, true);
    });
  }
  const label = format_cell(cell).slice(0, 239);
  return record(0x0f, label.length + 7, (view, bytes) => {
    writeAddress(view, 0xff);
    bytes[9] = 0x27;
    asciiLabel(bytes, 10, label);
  });
}

export function writeWk1(sheet) {
  if (!sheet['!ref']) throw failure('INVALID_INPUT', 'Cannot export an empty sheet to WK1');
  const range = decode_range(sheet['!ref']);
  const records = [record(0, 2, view => view.setUint16(4, 0x0406, true)), record(6, 8, view => {
    [range.s.c, range.s.r, range.e.c, range.e.r].forEach((value, index) => view.setUint16(4 + index * 2, value, true));
  })];
  const dense = sheet['!data'];
  for (let row = range.s.r; row <= Math.min(range.e.r, 8191); row += 1) {
    for (let column = range.s.c; column <= range.e.c; column += 1) {
      const cell = dense ? dense[row]?.[column] : sheet[encode_cell({ r: row, c: column })];
      if (cell && cell.t !== 'z') records.push(cellRecord(row, column, cell));
    }
  }
  records.push(record(1, 0));
  return joinRecords(records);
}

export function isWk1(bytes) {
  return bytes[0] === 0 && bytes[1] === 0 && bytes[2] === 2 && bytes[3] === 0 &&
    bytes[5] === 4 && [4, 6].includes(bytes[4]);
}

export function readWk1(bytes, options, decodeText) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const grid = options.dense ? [] : undefined;
  const sheet = grid ? { '!data': grid } : {};
  let range = { s: { r: 0, c: 0 }, e: { r: 0, c: 0 } };
  let done = false;
  for (let offset = 0; offset + 4 <= bytes.length;) {
    const type = view.getUint16(offset, true);
    const length = view.getUint16(offset + 2, true);
    const start = offset + 4;
    offset = start + length;
    if (offset > bytes.length) throw failure('INVALID_INPUT', 'Truncated WK1 record');
    if (type === 1) {
      done = true;
      break;
    }
    if (type === 6 && length === 8) {
      const [left, top, right, bottom] = [0, 2, 4, 6].map(i => view.getUint16(start + i, true));
      if (left !== 0xffff) range = { s: { r: top, c: left }, e: { r: bottom, c: right } };
      continue;
    }
    const labels = type === 0x0f || type === 0x33;
    if (!labels && type !== 0x0d && type !== 0x0e && type !== 0x10) continue;
    const minimum = labels ? 5 : type === 0x0d ? 7 : 13;
    if (length < minimum) throw failure('INVALID_INPUT', 'Invalid WK1 cell record length');
    const code = bytes[start];
    const column = view.getUint16(start + 1, true);
    const row = view.getUint16(start + 3, true);
    if (column >= 16384) throw failure('INVALID_INPUT', 'WK1 cell is outside the worksheet');
    let cell;
    if (labels) {
      const chars = bytes.subarray(start + 5, offset);
      const end = chars.indexOf(0);
      let value = decodeText(end < 0 ? chars : chars.subarray(0, end));
      if (value.charCodeAt(0) < 0x30) value = value.slice(1);
      cell = { t: 's', v: value };
    } else {
      const value = type === 0x0d ? view.getInt16(start + 5, true) : view.getFloat64(start + 5, true);
      cell = { t: 'n', v: value };
      const index = code & 15;
      if ((code & 0x70) === 0x70 && index > 1 && index < 15) {
        cell.z = options.dateNF || dateFormats[index - 1] || 'm/d/yy';
        if (options.cellDates && value >= 0 && Math.floor(value) !== 60) {
          cell = { ...cell, t: 'd', v: new Date(Date.UTC(1899, 11, value < 60 ? 31 : 30) + value * 86400000) };
        }
      }
    }
    if (type === 0x10 && options.cellFormula !== false && length >= 15) {
      const formulaLength = view.getUint16(start + 13, true);
      if (formulaLength <= length - 15) {
        const formula = wk1Formula(bytes.subarray(start + 15, start + 15 + formulaLength), row, column, decodeText);
        if (formula !== undefined) cell.f = formula;
      }
    }
    if (grid) (grid[row] ??= [])[column] = cell;
    else sheet[encode_cell({ r: row, c: column })] = cell;
    range.e.r = Math.max(range.e.r, row);
    range.e.c = Math.max(range.e.c, column);
  }
  if (!done) throw failure('INVALID_INPUT', 'Missing WK1 end record');
  sheet['!ref'] = encode_range(range);
  return book_new(sheet);
}

export function isWk3(bytes) {
  return bytes[0] === 0 && bytes[1] === 0 && bytes[2] === 26 && bytes[3] === 0;
}

function putExtended(view, offset, value) {
  if (!Number.isFinite(value)) throw failure('INVALID_VALUE', 'Expected a finite WK3 number');
  if (value === 0) {
    view.setUint16(offset + 8, 0xffff, true);
    return;
  }
  const absolute = Math.abs(value);
  let exponent = Math.min(1023, Math.floor(Math.log2(absolute)));
  let fraction = absolute / (2 ** exponent);
  if (fraction < 1) {
    exponent -= 1;
    fraction *= 2;
  }
  view.setBigUint64(offset, BigInt(fraction * (2 ** 63)), true);
  view.setUint16(offset + 8, exponent + 16383 + (value < 0 ? 0x8000 : 0), true);
}

function extendedCell(view, offset) {
  const mantissa = view.getBigUint64(offset, true);
  const exponent = view.getUint16(offset + 8, true);
  if (exponent === 0xffff) {
    if (mantissa === 0xc000000000000000n) return { t: 'e', v: 15 };
    if (mantissa === 0xd000000000000000n) return { t: 'e', v: 42 };
    return { t: 'n', v: 0 };
  }
  const sign = (exponent & 0x8000) ? -1 : 1;
  const power = (exponent & 0x7fff) - 16383;
  return { t: 'n', v: sign * (Number(mantissa) / (2 ** 63)) * (2 ** power) };
}

function asciiLabel(bytes, start, text) {
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index);
    bytes[start + index] = code >= 128 ? 95 : code;
  }
}

export function writeWk3(workbook) {
  const sheets = workbook.SheetNames.flatMap(name => {
    const sheet = workbook.Sheets?.[name];
    return sheet?.['!ref'] ? [{ name, sheet, range: decode_range(sheet['!ref']) }] : [];
  });
  const bottom = sheets.reduce((row, entry) => Math.max(row, Math.min(8191, entry.range.e.r)), 0);
  const right = sheets.reduce((col, entry) => Math.max(col, entry.range.e.c), 0);
  if (sheets.length > 255 || right > 255) throw failure('INVALID_INPUT', 'Worksheet dimensions exceed WK3 limits');
  const records = [record(0, 26, view => {
    view.setUint16(4, 0x1000, true);
    view.setUint16(6, 4, true);
    view.setUint16(12, bottom, true);
    view.setUint8(14, sheets.length);
    view.setUint8(15, right);
    view.setUint16(20, 0x0201, true);
  })];
  sheets.forEach(({ name }, index) => {
    records.push(record(0x1b, name.length + 5, (view, bytes) => {
      view.setUint16(4, 0x36b0, true);
      view.setUint16(6, index, true);
      asciiLabel(bytes, 8, name);
    }));
  });
  sheets.forEach(({ sheet, range }, index) => {
    const address = (view, row, column) => {
      view.setUint16(4, row, true);
      view.setUint8(6, index);
      view.setUint8(7, column);
    };
    const dense = sheet['!data'];
    for (let row = range.s.r; row <= Math.min(range.e.r, 8191); row += 1) {
      for (let column = range.s.c; column <= range.e.c; column += 1) {
        const cell = dense ? dense[row]?.[column] : sheet[encode_cell({ r: row, c: column })];
        if (!cell || cell.t === 'z') continue;
        if (cell.t === 'n') {
          records.push(record(0x17, 14, view => {
            address(view, row, column);
            putExtended(view, 8, Number(cell.v));
          }));
        } else {
          const label = format_cell(cell).slice(0, 239);
          records.push(record(0x16, label.length + 6, (view, bytes) => {
            address(view, row, column);
            bytes[8] = 0x27;
            asciiLabel(bytes, 9, label);
          }));
        }
      }
    }
  });
  records.push(record(1, 0));
  return joinRecords(records);
}

function wk3Number(view, type, offset) {
  if (type === 0x17 || type === 0x19) return extendedCell(view, offset);
  if (type === 0x27 || type === 0x28) return { t: 'n', v: view.getFloat64(offset, true) };
  if (type === 0x25) return { t: 'n', v: view.getInt32(offset, true) >> 6 };
  const value = view.getUint16(offset, true);
  const scale = [5000, 500, 1 / 20, 1 / 200, 1 / 2000, 1 / 20000, 1 / 16, 1 / 64];
  return { t: 'n', v: value & 1 ? (value >> 4) * scale[(value >> 1) & 7] : value >> 1 };
}

export function readWk3(bytes, options, decodeText) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const names = [];
  const sheets = [];
  const minimum = { 0x16: 4, 0x17: 14, 0x18: 6, 0x19: 14, 0x25: 8, 0x27: 12, 0x28: 12 };
  const readText = (start, end) => {
    const part = bytes.subarray(start, end);
    const nul = part.indexOf(0);
    return decodeText(nul < 0 ? part : part.subarray(0, nul));
  };
  let done = false;
  for (let offset = 0; offset + 4 <= bytes.length;) {
    const type = view.getUint16(offset, true);
    const length = view.getUint16(offset + 2, true);
    const start = offset + 4;
    offset = start + length;
    if (offset > bytes.length) throw failure('INVALID_INPUT', 'Truncated WK3 record');
    if (type === 1) {
      done = true;
      break;
    }
    if (type === 0x1b && length >= 5 && view.getUint16(start, true) === 0x36b0) {
      const index = view.getUint16(start + 2, true);
      if (index > 255) throw failure('INVALID_INPUT', 'Invalid WK3 worksheet index');
      names[index] = readText(start + 4, offset);
      continue;
    }
    if (!Object.hasOwn(minimum, type)) continue;
    if (length < minimum[type]) throw failure('INVALID_INPUT', 'Invalid WK3 cell record length');
    const row = view.getUint16(start, true);
    const index = bytes[start + 2];
    const column = bytes[start + 3];
    const entry = sheets[index] ??= { sheet: options.dense ? { '!data': [] } : {}, row: 0, column: 0 };
    if (options.sheetRows && row >= options.sheetRows) continue;
    let cell;
    if (type === 0x16) {
      let value = readText(start + 4, offset);
      if (value.charCodeAt(0) < 0x30) value = value.slice(1);
      value = value.replace(/\x0f./g, pair => String.fromCharCode(pair.charCodeAt(1) - 0x20)).replaceAll('\r\n', '\n');
      cell = { t: 's', v: value };
    } else {
      cell = wk3Number(view, type, start + 4);
    }
    if (options.dense) (entry.sheet['!data'][row] ??= [])[column] = cell;
    else entry.sheet[encode_cell({ r: row, c: column })] = cell;
    entry.row = Math.max(entry.row, row);
    entry.column = Math.max(entry.column, column);
  }
  if (!done) throw failure('INVALID_INPUT', 'Missing WK3 end record');
  const workbook = { SheetNames: [], Sheets: Object.create(null) };
  for (let index = 0; index < Math.max(1, names.length, sheets.length); index += 1) {
    if (index && !names[index] && !sheets[index]) continue;
    const { sheet, row, column } = sheets[index] ?? { sheet: {}, row: 0, column: 0 };
    sheet['!ref'] = encode_range({ s: { r: 0, c: 0 }, e: { r: row, c: column } });
    const name = names[index] || 'Sheet' + (index + 1);
    workbook.SheetNames.push(name);
    workbook.Sheets[name] = sheet;
  }
  return workbook;
}
