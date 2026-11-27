import { decorateBinaryCell, record, joinRecords } from './sheetjs-binary-records.js';
import { failure } from './contract.js';
import { decode_range, encode_cell, encode_range } from './sheetjs-address.js';
import { book_new } from './sheetjs-book.js';
import { serialFromDate } from './sheetjs-date.js';
import { get_table } from './sheetjs-ssf.js';

const boftypes = new Map([[0x0009, 2], [0x0209, 3], [0x0409, 4]]);
const singleByte = text => Uint8Array.from({ length: text.length }, (_, i) => {
  const code = text.charCodeAt(i);
  return code < 128 ? code : 95;
});

function biffCell(cell, row, column, version, date1904, formatIndex) {
  function make(type, size, emit) {
    return record(type, 7 + size, (view, bytes) => {
      view.setUint16(4, row, true);
      view.setUint16(6, column, true);
      bytes[8] = bytes[9] = formatIndex;
      emit?.(view, bytes);
    });
  }
  const errorOrBool = (value, error) => make(5, 2, (_, bytes) => bytes.set([Number(value), Number(error)], 11));
  if (cell.v == null) return make(1, 0);
  if (cell.t === 'n' || cell.t === 'd') {
    const value = cell.t === 'd' ? serialFromDate(new Date(cell.v).getTime(), date1904) : cell.v;
    if (version === 2 && Number.isInteger(value) && value >= 0 && value < 65536) {
      return make(2, 2, view => view.setUint16(11, value, true));
    }
    if (!Number.isFinite(value)) return errorOrBool(Number.isNaN(value) ? 0x24 : 7, true);
    return make(3, 8, view => view.setFloat64(11, value, true));
  }
  if (cell.t === 'e' || cell.t === 'b') return errorOrBool(cell.v, cell.t === 'e');
  if (cell.t === 's' || cell.t === 'str') {
    const value = singleByte(String(cell.v).slice(0, 255));
    return make(4, 1 + value.length, (_, bytes) => {
      bytes[11] = value.length;
      bytes.set(value, 12);
    });
  }
  return make(1, 0);
}

export function writeEarlyBiff(sheet, version, date1904, strict) {
  const range = decode_range(sheet['!ref'] || 'A1');
  if (strict && (range.e.c > 255 || range.e.r > 16383)) {
    throw failure('INVALID_INPUT', 'Worksheet exceeds the early BIFF worksheet limits');
  }
  const formats = new Map([['General', 0]]);
  const cells = [];
  const notes = [];
  for (let row = range.s.r; row <= Math.min(range.e.r, 16383); row += 1) {
    for (let column = range.s.c; column <= Math.min(range.e.c, 255); column += 1) {
      const cell = sheet['!data'] ? sheet['!data'][row]?.[column] : sheet[encode_cell({ r: row, c: column })];
      if (!cell) continue;
      const code = cell.z ?? 'General';
      if (!formats.has(code)) {
        if (formats.size >= 64) throw failure('INVALID_VALUE', 'Too many early BIFF formats');
        formats.set(code, formats.size);
      }
      cells.push(biffCell(cell, row, column, version, date1904, formats.get(code)));
      if (cell.c?.length) {
        const text = cell.c.map(note => String(note.t ?? '')).join('');
        for (let index = 0; index < Math.max(1, text.length); index += 2048) {
          const part = singleByte(text.slice(index, index + 2048));
          const size = index === 0 ? text.length : part.length;
          if (size > 65529) throw failure('INVALID_VALUE', 'Early BIFF note is too long');
          notes.push(record(0x1c, 6 + size, (view, bytes) => {
            view.setUint16(4, index === 0 ? row : 0xffff, true);
            view.setUint16(6, index === 0 ? column : 0xffff, true);
            view.setUint16(8, size, true);
            bytes.set(part, 10);
          }));
        }
      }
    }
  }
  const records = [record((version - 2) * 0x0200 + 9, version === 2 ? 4 : 6, view => {
    view.setUint16(4, version, true);
    view.setUint16(6, 0x10, true);
    if (version !== 2) view.setUint16(8, 0x7262, true);
  })];
  if (date1904) records.push(record(0x22, 2, view => view.setUint16(4, 1, true)));
  for (const code of formats.keys()) {
    const data = singleByte(code.slice(0, 255));
    const offset = version === 4 ? 2 : 0;
    records.push(record(version === 4 ? 0x041e : 0x001e, offset + data.length + 1, (_, bytes) => {
      bytes[4 + offset] = data.length;
      bytes.set(data, 5 + offset);
    }));
  }
  for (const index of formats.values()) {
    records.push(record(0x0043 + (version - 2) * 0x0200, version === 2 ? 4 : 12, (_, bytes) => {
      bytes[version === 2 ? 6 : 5] = index;
    }));
  }
  return joinRecords([...records, ...cells, ...notes, record(0x0a, 0)]);
}

export function isEarlyBiff(bytes) {
  return bytes.length >= 8 && boftypes.has(bytes[0] | (bytes[1] << 8)) &&
    bytes[3] === 0 && (bytes[2] === 4 || bytes[2] === 6);
}

export function readEarlyBiff(bytes, options, decodeText) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const version = boftypes.get(view.getUint16(0, true));
  const formats = [];
  const xfs = [];
  const grid = options.dense ? [] : undefined;
  const sheet = grid ? { '!data': grid } : {};
  let range;
  let date1904 = false;
  let end = false;
  const notes = [];
  let pendingNote;
  for (let offset = 0; offset + 4 <= bytes.length;) {
    const type = view.getUint16(offset, true);
    const size = view.getUint16(offset + 2, true);
    const start = offset + 4;
    offset = start + size;
    if (offset > bytes.length) throw failure('INVALID_INPUT', 'Truncated BIFF record');
    if (type === 0x0a) {
      end = true;
      break;
    }
    if (type === 0x22 && size >= 2) date1904 = view.getUint16(start, true) !== 0;
    if ((type === 0x1e || type === 0x041e) && size >= (type === 0x041e ? 3 : 1)) {
      const at = start + (type === 0x041e ? 2 : 0);
      formats.push(decodeText(bytes.subarray(at + 1, Math.min(offset, at + 1 + bytes[at]))));
      continue;
    }
    if ((type === 0x43 && size >= 3) || ([0x0243, 0x0443].includes(type) && size >= 2)) {
      xfs.push(bytes[start + (type === 0x43 ? 2 : 1)]);
      continue;
    }
    if (type === 0x1c && size >= 6) {
      const row = view.getUint16(start, true);
      const column = view.getUint16(start + 2, true);
      const length = view.getUint16(start + 4, true);
      const part = bytes.subarray(start + 6, start + 6 + Math.min(length, size - 6, 2048));
      if (row === 0xffff && column === 0xffff) {
        if (pendingNote) pendingNote.text += decodeText(part);
      } else {
        pendingNote = { row, column, text: decodeText(part) };
        notes.push(pendingNote);
      }
      continue;
    }
    const low = type & 255;
    if (type !== low && ![0x0201, 0x0203, 0x0204, 0x0205].includes(type)) continue;
    if (low < 1 || low > 5) continue;
    const body = start + (type === low ? 7 : 6);
    if (body > offset) throw failure('INVALID_INPUT', 'Invalid BIFF cell record');
    const row = view.getUint16(start, true);
    const column = view.getUint16(start + 2, true);
    if (options.sheetRows && row >= options.sheetRows) continue;
    let cell;
    if (low === 1) {
      if (!options.sheetStubs) continue;
      cell = { t: 'z', v: undefined };
    } else if (low === 2 && body + 2 <= offset) cell = { t: 'n', v: view.getUint16(body, true) };
    else if (low === 3 && body + 8 <= offset) cell = { t: 'n', v: view.getFloat64(body, true) };
    else if (low === 4 && body + (type === low ? 1 : 2) <= offset) {
      const at = body + (type === low ? 1 : 2);
      const length = type === low ? bytes[body] : view.getUint16(body, true);
      if (at + length > offset) throw failure('INVALID_INPUT', 'Truncated BIFF label');
      cell = { t: 's', v: decodeText(bytes.subarray(at, at + length)) };
    } else if (low === 5 && body + 2 <= offset) {
      cell = bytes[body + 1] ? { t: 'e', v: bytes[body] } : { t: 'b', v: Boolean(bytes[body]) };
    } else throw failure('INVALID_INPUT', 'Truncated BIFF cell value');
    if (cell.t !== 'z') {
      const index = type === low ? bytes[start + 4] & 63 : view.getUint16(start + 4, true);
      const formatIndex = xfs[index] ?? (version === 2 ? bytes[start + 5] & 63 : 0);
      const code = formats[formatIndex] ?? get_table()[formatIndex] ?? 'General';
      decorateBinaryCell(cell, code, date1904, options);
      cell.z = code;
    }
    if (grid) (grid[row] ??= [])[column] = cell;
    else sheet[encode_cell({ r: row, c: column })] = cell;
    range ??= { s: { r: row, c: column }, e: { r: row, c: column } };
    range.s.r = Math.min(range.s.r, row);
    range.s.c = Math.min(range.s.c, column);
    range.e.r = Math.max(range.e.r, row);
    range.e.c = Math.max(range.e.c, column);
  }
  if (!end) throw failure('INVALID_INPUT', 'Missing BIFF end record');
  for (const { row, column, text } of notes) {
    const cell = grid ? grid[row]?.[column] : sheet[encode_cell({ r: row, c: column })];
    if (cell) cell.c = [{ a: '', t: text }];
  }
  if (range) sheet['!ref'] = encode_range(range);
  const workbook = book_new(sheet);
  if (date1904) workbook.Workbook = { WBProps: { date1904 } };
  return workbook;
}
