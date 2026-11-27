import { checkSylkRowLimit } from './sheetjs-options.generated.js';
import { failure } from './contract.js';
import { book_new } from './sheetjs-book.js';
import { decode_range, encode_cell, encode_range } from './sheetjs-address.js';
import { errorCode, errorName, numberFromText } from './sheetjs-csv-input.js';
import { serialFromDate } from './sheetjs-date.js';
import { formatted, numberFormatCode } from './sheetjs-format.js';
import { is_date, parse_date_code } from './sheetjs-ssf.js';
import { formulaA1 } from './sheetjs-xlml-read.js';
import { formulaR1C1 } from './sheetjs-xlml.js';

const separator = '\r\n';

function cellPayload(cell, row, col, date1904) {
  switch (cell.t) {
    case 'n': {
      const number = Number.isFinite(cell.v) ? cell.v : cell.v === undefined || Number.isNaN(cell.v) ? '#NUM!' : '#DIV/0!';
      return cell.f && !cell.F ? number + ';E' + formulaR1C1(cell.f, row, col) : number;
    }
    case 'b': return cell.v ? 'TRUE' : 'FALSE';
    case 'e': return cell.w || errorName(cell.v) || cell.v;
    case 'd': return serialFromDate(new Date(cell.v).getTime(), date1904);
    case 's': case 'str': return '"' + String(cell.v ?? '').replaceAll('"', '').replaceAll(';', ';;') + '"';
    default: return '';
  }
}

function appendLayout(records, sheet) {
  sheet['!cols']?.forEach((column, index) => {
    if (!column) return;
    const pixels = column.wpx ?? (typeof column.width === 'number' ? Math.floor(column.width * 6 + 0.5) : undefined);
    const width = column.hidden ? 0 : column.wch ?? (pixels === undefined ? undefined : (pixels - 5) / 6);
    if (width !== undefined) records.push(`F;W${index + 1} ${index + 1} ${Math.round(width)}`);
  });
  sheet['!rows']?.forEach((row, index) => {
    if (!row) return;
    const height = row.hidden ? 0 : row.hpt ?? row.hpx;
    if (height !== undefined) records.push(`F;M${height * 20};R${index + 1}`);
  });
}

export function writeSylk(sheet, date1904) {
  const reference = sheet['!ref'];
  const preamble = ['ID;PSheetJS;N;E', 'P;PGeneral'];
  const formats = new Map([['General', 0]]);
  const comments = [];
  const body = [];
  if (reference) {
    const { s, e } = decode_range(reference);
    const dense = sheet['!data'];
    for (let row = s.r; row <= e.r; row++) {
      if (dense && !dense[row]) continue;
      const records = [];
      for (let col = s.c; col <= e.c; col++) {
        const cell = dense ? dense[row][col] : sheet[encode_cell({ r: row, c: col })];
        if (!cell) continue;
        const location = `Y${row + 1};X${col + 1}`;
        if (cell.c?.length) {
          const comment = cell.c.map(({ t }) => t).join('').replaceAll(';', ';;')
            .replaceAll('\n', '\x1b :').replaceAll('\r', '\x1b =');
          comments.push(`C;${location};A${comment}`);
        }
        if (cell.v == null && (!cell.f || cell.F)) continue;
        const format = numberFormatCode(cell.z) ?? (cell.t === 'd' ? 'm/d/yy' : 'General');
        if (format !== 'General') {
          if (!formats.has(format)) {
            formats.set(format, formats.size);
            preamble.push('P;P' + format.replaceAll(';', ';;'));
          }
          records.push(`F;P${formats.get(format)};${location}`);
        }
        records.push(`C;${location};K${cellPayload(cell, row, col, date1904)}`);
      }
      body.push(records.join(separator));
    }
  }
  preamble.push('F;P0;DG0G8;M255');
  appendLayout(preamble, sheet);
  if (reference) {
    const { s, e } = decode_range(reference);
    preamble.push(`B;Y${e.r - s.r + 1};X${e.c - s.c + 1};D${s.c} ${s.r} ${e.c} ${e.r}`);
  }
  preamble.push('O;L;D;B' + (date1904 ? ';V4' : '') + ';K47;G100 0.001');
  return [...preamble, ...comments, ...(body.length ? body : ['']), 'E', ''].join(separator);
}

export function isSylkText(text) {
  return /^\uFEFF?ID;P[^\r\n]*(?:\r\n|\n|\r)/.test(text);
}

function recordFields(line) {
  const fields = [''];
  for (let index = 0; index < line.length; index++) {
    const character = line[index];
    if (character === ';' && line[index + 1] === ';') index++;
    else if (character === ';') {
      fields.push('');
      continue;
    }
    fields[fields.length - 1] += character;
  }
  return fields;
}

function position(value, current, maximum) {
  const index = value === undefined ? current : Number(value) - 1;
  if (!Number.isInteger(index) || index < 0 || index >= maximum) {
    throw failure('INVALID_INPUT', 'SYLK cell coordinates are outside worksheet limits');
  }
  return index;
}

function sylkValue(raw) {
  if (raw === undefined) return { t: 'n' };
  if (raw.startsWith('"') && raw.endsWith('"')) return { t: 's', v: raw.slice(1, -1) };
  if (raw === 'TRUE' || raw === 'FALSE') return { t: 'b', v: raw === 'TRUE' };
  const error = errorCode(raw);
  if (error !== undefined) return { t: 'e', v: error };
  const number = numberFromText(raw);
  return number === undefined ? { t: 's', v: raw } : { t: 'n', v: number };
}

export function readSylk(text, options) {
  checkSylkRowLimit(options);
  const lines = text.split(/\r\n|\r|\n/);
  const cells = new Map();
  const formats = [];
  const styles = new Map();
  const ordinaryFormulas = new Map();
  const sharedFormulas = [];
  const names = [];
  const rows = [];
  const columns = [];
  let date1904 = false;
  let row = 0;
  let column = 0;
  let ended = false;
  for (let index = 1; index < lines.length; index++) {
    let line = lines[index];
    if (!line) continue;
    if (/^C;.*;K"/.test(line)) {
      let quotes = (line.match(/"/g) ?? []).length;
      while (quotes % 2 && index + 1 < lines.length) {
        const next = lines[++index];
        line += '\n' + next;
        quotes += (next.match(/"/g) ?? []).length;
      }
    }
    const [record, ...fields] = recordFields(line);
    const get = (key) => fields.find((field) => field.startsWith(key))?.slice(1);
    if (record === 'E') {
      ended = true;
      break;
    }
    if (record === 'P' && get('P') !== undefined) formats.push(get('P'));
    else if (record === 'O') date1904 = /^[1-4]$/.test(get('V') ?? '');
    else if (record === 'NN' && get('N') && get('E')) {
      names.push({ Name: get('N'), Ref: 'Sheet1!' + formulaA1(get('E'), 0, 0), Sheet: 0 });
    } else if (record === 'F') {
      const formatRow = get('Y');
      const formatColumn = get('X');
      row = position(formatRow, row, 1048576);
      column = position(formatColumn, column, 16384);
      if (get('P') !== undefined && formatColumn !== undefined) {
        styles.set(encode_cell({ r: row, c: column }), formats[Number(get('P'))]);
      }
      const widthRecord = get('W');
      if (widthRecord !== undefined) {
        const [start, end, width] = widthRecord.trim().split(/\s+/).map(Number);
        if (!Number.isInteger(start) || !Number.isInteger(end) || !Number.isFinite(width) ||
            start < 1 || end < start || end > 16384 || width < 0) {
          throw failure('INVALID_INPUT', 'Invalid SYLK column width range');
        }
        for (let c = start - 1; c < end; c++) {
          columns[c] = width ? { wch: width, wpx: Math.round(width * 6 + 5), width: Math.round((width + 5 / 6) * 256) / 256, MDW: 6 } : { hidden: true };
        }
      }
      const rowRecord = get('R');
      if (rowRecord !== undefined) {
        const r = position(rowRecord, row, 1048576);
        const height = Number(get('M') ?? 0) / 20;
        if (!Number.isFinite(height) || height < 0) throw failure('INVALID_INPUT', 'Invalid SYLK row height');
        rows[r] = height ? { hpt: height, hpx: height } : { hidden: true };
      }
    } else if (record === 'C') {
      const y = get('Y');
      row = position(y, row, 1048576);
      column = position(get('X'), y === undefined ? column : 0, 16384);
      const key = encode_cell({ r: row, c: column });
      const formula = options.cellFormula === false ? undefined : get('E');
      if (formula) ordinaryFormulas.set(key, formula);
      if (options.sheetRows && row >= options.sheetRows) continue;
      const cached = get('K');
      const comment = get('A');
      const shared = options.cellFormula !== false && get('S') !== undefined;
      if (cached === undefined && formula === undefined && !comment && !shared) continue;
      const prior = cells.get(key)?.cell;
      const cell = cached === undefined && prior ? { ...prior } :
        { ...(prior ?? {}), ...(cached === undefined && !formula && !shared ? { t: 'z' } : sylkValue(cached)) };
      if (comment) cell.c = [{ a: 'SheetJSYLK', t: comment.replaceAll('\x1b :', '\n').replaceAll('\x1b =', '\r') }];
      if (formula && shared) throw failure('INVALID_INPUT', 'SYLK shared formula has its own expression');
      if (formula) cell.f = formulaA1(formula, row, column);
      if (shared) {
        const sourceRow = position(get('R'), -1, 1048576);
        const sourceColumn = position(get('C'), -1, 16384);
        sharedFormulas.push({ key, source: encode_cell({ r: sourceRow, c: sourceColumn }) });
      }
      const format = styles.get(key);
      if (format !== undefined) {
        cell.z = format;
        if (options.cellText !== false) {
          try { cell.w = formatted(cell, cell.v, { ...options, date1904 }); }
          catch { cell.w = String(cell.v ?? ''); }
        }
        if (options.cellDates && cell.t === 'n' && is_date(format)) {
          const parts = parse_date_code(cell.v, { date1904 });
          if (parts) {
            const { y, m, d, H, M, S, u } = parts;
            cell.t = 'd';
            cell.v = new Date(Date.UTC(y, m - 1, d, H, M, S, Math.round(u * 1000)));
          }
        }
      }
      cells.set(key, { row, column, cell });
    }
  }
  if (!ended) throw failure('INVALID_INPUT', 'SYLK end record is missing');
  for (const { key, source } of sharedFormulas) {
    const entry = cells.get(key);
    const formula = ordinaryFormulas.get(source);
    if (formula === undefined) throw failure('INVALID_INPUT', 'SYLK shared formula base is missing');
    entry.cell.f = formulaA1(formula, entry.row, entry.column);
  }
  const sheet = options.dense ? { '!data': [] } : {};
  let range;
  for (const [key, { row: r, column: c, cell }] of cells) {
    if (options.dense) (sheet['!data'][r] ??= [])[c] = cell;
    else sheet[key] = cell;
    if (!range) range = { s: { r, c }, e: { r, c } };
    range.s.r = Math.min(range.s.r, r);
    range.s.c = Math.min(range.s.c, c);
    range.e.r = Math.max(range.e.r, r);
    range.e.c = Math.max(range.e.c, c);
  }
  if (range) sheet['!ref'] = encode_range(range);
  if (rows.length) sheet['!rows'] = rows;
  if (columns.length) sheet['!cols'] = columns;
  return { ...book_new(sheet), Workbook: { WBProps: { date1904 }, Names: names }, bookType: 'sylk' };
}
