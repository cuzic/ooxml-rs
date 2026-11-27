import { errorName } from './sheetjs-csv-input.js';
import { format, is_date, parse_date_code } from './sheetjs-ssf.js';

export function record(type, payloadSize, fill) {
  const bytes = new Uint8Array(4 + payloadSize);
  const view = new DataView(bytes.buffer);
  view.setUint16(0, type, true);
  view.setUint16(2, payloadSize, true);
  fill?.(view, bytes);
  return bytes;
}


export function joinRecords(records) {
  const output = new Uint8Array(records.reduce((size, bytes) => size + bytes.length, 0));
  let offset = 0;
  for (const bytes of records) {
    output.set(bytes, offset);
    offset += bytes.length;
  }
  return output;
}


export function decorateBinaryCell(cell, code, date1904, { cellText, cellDates, dateNF }) {
  if (cell.t !== 'n') {
    if (cellText === false) return;
    if (cell.t === 's') cell.w = cell.v;
    else if (cell.t === 'b') cell.w = cell.v ? 'TRUE' : 'FALSE';
    else if (cell.t === 'e') cell.w = errorName(cell.v);
    return;
  }
  const outOfRange = cell.v < 0 || cell.v > 2958465;
  const dateFormat = (outOfRange || cellDates) && is_date(code);
  if (cellText !== false) {
    cell.w = outOfRange && dateFormat ? '' : format(code, cell.v, { date1904, dateNF });
  }
  if (cellDates && dateFormat) {
    const date = parse_date_code(cell.v + (date1904 ? 1462 : 0));
    if (date) {
      cell.t = 'd';
      cell.v = new Date(Date.UTC(date.y, date.m - 1, date.d, date.H, date.M, date.S, date.u));
    }
  }
}

export function packText(values) {
  const lengths = Uint32Array.from(values, value => value.length);
  const units = new Uint16Array(lengths.reduce((total, length) => total + length, 0));
  let offset = 0;
  for (const value of values) {
    for (let index = 0; index < value.length; index += 1) units[offset++] = value.charCodeAt(index);
  }
  return [lengths, units];
}

export const biffBuiltInNames = [
  'Consolidate_Area', 'Auto_Open', 'Auto_Close', 'Extract', 'Database', 'Criteria',
  'Print_Area', 'Print_Titles', 'Recorder', 'Data_Form', 'Auto_Activate', 'Auto_Deactivate',
  'Sheet_Title', '_FilterDatabase',
];
