import { decode_cell, decode_range, encode_cell, encode_range } from './sheetjs-address.js';
import { formatted, numberFormatCode } from './sheetjs-format.js';
import { is_date } from './sheetjs-ssf.js';
import { sheetArrayInputSettings, sheetJsonInputSettings } from './sheetjs-options.generated.js';

const millisecondsPerDay = 86400000;
const excelEpoch = Date.UTC(1899, 11, 30);

function worksheetDate(date, utc) {
  return utc ? date : new Date(date.getTime() - date.getTimezoneOffset() * 60000);
}

function dateSerial(date) {
  const serial = (date.getTime() - excelEpoch) / millisecondsPerDay;
  return serial < 60 ? serial - 1 : serial;
}

function dateCell(date, options) {
  const value = worksheetDate(date, options.UTC);
  let serial = dateSerial(value);
  if (options.date1904) serial -= 1462;
  const z = options.dateNF;
  const w = Number.isFinite(serial) && serial >= 0 ? formatted({ t: 'n', z }, serial) : '';
  return options.cellDates
    ? { t: 'd', v: value, z, w }
    : { t: 'n', v: serial, z, w };
}

function arrayCell(input, options) {
  if (input instanceof Date) return dateCell(input, options);
  if (input && typeof input === 'object' && !Array.isArray(input)) return input;
  const [value, formula] = Array.isArray(input) ? input : [input];
  let cell;
  if (value === null) {
    if (formula) cell = { t: 'n', v: null };
    else if (options.nullError) cell = { t: 'e', v: 0 };
    else if (options.sheetStubs) cell = { t: 'z', v: null };
  } else if (typeof value === 'number') {
    if (Number.isFinite(value)) cell = { t: 'n', v: value };
    else cell = { t: 'e', v: Number.isNaN(value) ? 15 : 7 };
  } else if (typeof value === 'boolean') {
    cell = { t: 'b', v: value };
  } else if (value instanceof Date) {
    cell = dateCell(value, options);
  } else {
    cell = { t: 's', v: value };
  }
  if (cell && Array.isArray(input)) cell.f = formula;
  return cell;
}

function decodeOrigin(origin = 0) {
  if (typeof origin === 'string') return decode_cell(origin);
  return typeof origin === 'number' ? { r: origin, c: 0 } : origin;
}

export function sheet_add_aoa(sheet, data, inputOptions) {
  const options = sheetArrayInputSettings(inputOptions);
  sheet ??= options.dense ? { '!data': [] } : {};
  const dense = sheet['!data'];
  const reference = sheet['!ref'];
  const range = decode_range(reference || 'A1');
  const offset = decodeOrigin(options.origin);
  let startRow = offset.r;
  if (startRow === -1 && reference) {
    startRow = range.e.r + 1;
    range.e.r = startRow;
  }
  const edits = [];
  const denseRows = [];
  let seen = false;
  for (let row = 0; row < data.length; row += 1) {
    const source = data[row];
    if (!source) continue;
    if (!Array.isArray(source)) throw new Error('aoa_to_sheet expects an array of arrays');
    const r = startRow + row;
    if (dense) denseRows.push(r);
    for (let column = 0; column < source.length; column += 1) {
      if (source[column] === undefined) continue;
      const c = offset.c + column;
      range.s.r = Math.min(range.s.r, r);
      range.s.c = Math.min(range.s.c, c);
      range.e.r = Math.max(range.e.r, r);
      range.e.c = Math.max(range.e.c, c);
      seen = true;
      const cell = arrayCell(source[column], options);
      if (!cell) continue;
      const key = dense ? undefined : encode_cell({ r, c });
      const previous = dense ? dense[r]?.[c] : sheet[key];
      edits.push({ r, c, key, cell, format: previous?.z });
    }
  }
  for (const row of denseRows) dense[row] ??= [];
  for (const { r, c, key, cell, format } of edits) {
    if (format) cell.z = format;
    if (dense) dense[r][c] = cell;
    else sheet[key] = cell;
  }
  if (seen) sheet['!ref'] = encode_range(range);
  return sheet;
}

export function aoa_to_sheet(data, options) {
  return sheet_add_aoa(undefined, data, options);
}

export function sheet_add_json(sheet, records, inputOptions) {
  const options = sheetJsonInputSettings(inputOptions);
  sheet ??= options.dense ? { '!data': [] } : {};
  const dense = sheet['!data'];
  const existing = sheet['!ref'] ? decode_range(sheet['!ref']) : undefined;
  const { r, c } = decodeOrigin(options.origin);
  const start = r === -1 ? (existing?.e.r ?? -1) + 1 : r;
  const withHeader = !options.skipHeader;
  const header = options.header ?? [];
  const columns = new Map();
  header.forEach((key, index) => {
    if (!columns.has(key)) columns.set(key, index);
  });

  function cellAt(row, column) {
    if (dense != null) {
      dense[row] ??= [];
      return [dense[row], column];
    }
    return [sheet, encode_cell({ r: row, c: column })];
  }

  records.forEach((record, index) => {
    const row = start + index + Number(withHeader);
    if (dense != null) dense[row] ??= [];
    for (const [field, input] of Object.entries(record)) {
      if (!columns.has(field)) {
        columns.set(field, header.length);
        header.push(field);
      }
      const [cells, key] = cellAt(row, c + columns.get(field));
      if (input && typeof input === 'object' && !(input instanceof Date)) {
        cells[key] = input;
        continue;
      }
      const cell = cells[key] ?? {};
      let value = input;
      let type = { string: 's', number: 'n', boolean: 'b' }[typeof value] ?? 'z';
      if (value instanceof Date) {
        const date = worksheetDate(value, options.UTC);
        type = options.cellDates ? 'd' : 'n';
        value = options.cellDates ? date : dateSerial(date);
        if (!cell.z || !is_date(numberFormatCode(cell.z) ?? '')) {
          cell.z = options.dateNF;
        }
      } else if (value === null && options.nullError) {
        type = 'e';
        value = 0;
      }
      cell.t = type;
      cell.v = value;
      delete cell.w;
      delete cell.R;
      cells[key] = cell;
    }
  });
  if (dense != null) dense[start] ??= [];
  if (withHeader) {
    for (let index = 0; index < header.length; index += 1) {
      const [cells, key] = cellAt(start, c + index);
      cells[key] = { t: 's', v: header[index] };
    }
  }
  const endRow = start + records.length - 1 + Number(withHeader);
  sheet['!ref'] = encode_range({
    s: { r: 0, c: 0 },
    e: { r: Math.max(existing?.e.r ?? -1, endRow), c: Math.max(existing?.e.c ?? 0, c, c + header.length - 1) },
  });
  return sheet;
}

export function json_to_sheet(records, options) {
  return sheet_add_json(undefined, records, options);
}
