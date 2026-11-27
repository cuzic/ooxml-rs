import { checkDifRowLimit } from './sheetjs-options.generated.js';
import { failure } from './contract.js';
import { decode_range, encode_cell } from './sheetjs-address.js';
import { dateFromText, numberFromText } from './sheetjs-csv-input.js';
import { book_new } from './sheetjs-book.js';
import { aoa_to_sheet } from './sheetjs-input.js';
import { formatted } from './sheetjs-format.js';

const separator = '\r\n';

function textRecord(text) {
  return '1,0' + separator + '"' + String(text).replaceAll('"', '""') + '"';
}

function numberRecord(number, marker = 'V') {
  return '0,' + number + separator + marker;
}

function cellRecord(cell, options) {
  switch (cell?.t) {
    case 'n':
      if (cell.w != null || cell.v != null) return numberRecord(cell.w ?? cell.v);
      return textRecord(cell.f != null && !cell.F ? '=' + cell.f : '');
    case 'b': return numberRecord(cell.v ? 1 : 0, cell.v ? 'TRUE' : 'FALSE');
    case 's': case 'str':
      return textRecord(Number.isNaN(Number(cell.v)) ? cell.v : '="' + cell.v + '"');
    case 'd':
      return numberRecord(formatted({ ...cell, z: cell.z ?? 'm/d/yy' }, cell.v, options));
    default: return textRecord('');
  }
}

export function writeDif(sheet, options) {
  if (!sheet['!ref']) throw failure('UNSUPPORTED_RANGE', 'Cannot export an empty sheet to DIF');
  const { s, e } = decode_range(sheet['!ref']);
  const output = [
    'TABLE', '0,1', '"sheetjs"',
    'VECTORS', '0,' + (e.r - s.r + 1), '""',
    'TUPLES', '0,' + (e.c - s.c + 1), '""',
    'DATA', '0,0', '""',
  ];
  const dense = sheet['!data'];
  for (let r = s.r; r <= e.r; r++) {
    output.push('-1,0', 'BOT');
    for (let c = s.c; c <= e.c; c++) {
      output.push(cellRecord(dense ? dense[r]?.[c] : sheet[encode_cell({ r, c })], options));
    }
  }
  output.push('-1,0', 'EOD');
  return output.join(separator);
}

export function isDifText(text) {
  return /^\uFEFF?TABLE\r?\n0,\d+\r?\n/.test(text);
}

function stringValue(record) {
  if (record[0] !== '"' || record.at(-1) !== '"') {
    throw failure('INVALID_INPUT', 'Invalid DIF string record');
  }
  const value = record.slice(1, -1).replaceAll('""', '"');
  const literal = /^="(.*)"$/s.exec(value);
  return (literal?.[1] ?? value) || null;
}

export function readDif(text, options) {
  checkDifRowLimit(options);
  const lines = text.split(/\r?\n/);
  const start = lines.indexOf('DATA');
  if (start < 0) throw failure('INVALID_INPUT', 'DIF data section is missing');
  const rows = [];
  let row;
  let complete = false;
  for (let index = start + 3; index < lines.length; index++) {
    const metadata = /^(-1|0|1),(.*)$/.exec(lines[index].trim());
    if (!metadata || index + 1 >= lines.length) throw failure('INVALID_INPUT', 'Invalid DIF record');
    const [, type, value] = metadata;
    let payload = lines[++index];
    if (type === '1') {
      let quotes = (payload.match(/"/g) ?? []).length;
      while (quotes % 2 && index + 1 < lines.length) {
        const next = lines[++index];
        payload += '\n' + next;
        quotes += (next.match(/"/g) ?? []).length;
      }
    }
    payload = payload.trim();
    if (type === '-1') {
      if (payload === 'EOD') {
        complete = true;
        break;
      }
      if (payload !== 'BOT') throw failure('INVALID_INPUT', 'Invalid DIF row marker');
      if (rows.length === 1048576) throw failure('INVALID_INPUT', 'DIF row limit exceeded');
      rows.push(row = []);
    } else {
      if (!row || row.length === 16384) throw failure('INVALID_INPUT', 'Invalid DIF column record');
      if (type === '1') row.push(stringValue(payload));
      else if (payload === 'TRUE' || payload === 'FALSE') row.push(payload === 'TRUE');
      else row.push(numberFromText(value) ?? dateFromText(value, options) ?? value);
    }
  }
  if (!complete) throw failure('INVALID_INPUT', 'DIF end marker is missing');
  const projected = options.sheetRows ? rows.slice(0, options.sheetRows) : rows;
  const sheet = aoa_to_sheet(projected, options);
  return { ...book_new(sheet), bookType: 'dif' };
}
