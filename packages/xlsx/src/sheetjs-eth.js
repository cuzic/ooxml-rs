import { checkEthRowLimit } from './sheetjs-options.generated.js';
import { failure } from './contract.js';
import { decode_cell, decode_range, encode_cell } from './sheetjs-address.js';
import { book_new } from './sheetjs-book.js';
import { aoa_to_sheet } from './sheetjs-input.js';
import { errorCode } from './sheetjs-csv-input.js';
import { serialFromDate } from './sheetjs-date.js';
import { formatted } from './sheetjs-format.js';

const boundary = '--SocialCalcSpreadsheetControlSave';
const part = boundary + '\nContent-type: text/plain; charset=UTF-8\n';
const header = [
  'socialcalc:version:1.5',
  'MIME-Version: 1.0',
  'Content-Type: multipart/mixed; boundary=SocialCalcSpreadsheetControlSave',
  part,
  '# SocialCalc Spreadsheet Control Save\npart:sheet',
  part,
].join('\n');

const escapes = { '\\': '\\b', ':': '\\c', '\n': '\\n' };
const unescape = { b: '\\', c: ':', n: '\n' };
const encode = (text) => String(text).replace(/[\\:\n]/g, (character) => escapes[character]);
const decode = (text) => text.replace(/\\([bcn])/g, (_, key) => unescape[key]);

function cellRecord(cell) {
  if (cell?.v == null) return undefined;
  const formula = cell.f ? encode(cell.f) : undefined;
  switch (cell.t) {
    case 's': case 'str': return ['t', encode(cell.v)];
    case 'b': return [formula ? 'vtf' : 'vtc', 'nl', Number(cell.v), formula ?? (cell.v ? 'TRUE' : 'FALSE')];
    case 'd': return ['vtc', 'nd', serialFromDate(new Date(cell.v).getTime(), false),
      encode(formatted({ ...cell, z: cell.z ?? 'm/d/yy' }, cell.v))];
    case 'n':
      if (Number.isFinite(cell.v)) return formula ? ['vtf', 'n', cell.v, formula] : ['v', cell.v];
      {
        const error = Number.isNaN(cell.v) ? '#NUM!' : '#DIV/0!';
        return [formula ? 'vtf' : 'vtc', 'e' + error, 0, formula ?? error, 'e', error];
      }
    default: return undefined;
  }
}

export function writeEth(sheet) {
  const records = [header];
  const ref = sheet['!ref'];
  if (ref) {
    const { s, e } = decode_range(ref);
    const dense = sheet['!data'];
    for (let r = s.r; r <= e.r; r++) {
      for (let c = s.c; c <= e.c; c++) {
        const address = encode_cell({ r, c });
        const record = cellRecord(dense ? dense[r]?.[c] : sheet[address]);
        if (record) records.push(['cell', address, ...record].join(':'));
      }
    }
    records.push(`sheet:c:${e.c - s.c + 1}:r:${e.r - s.r + 1}:tvf:1`, 'valueformat:1:text-wiki');
  } else {
    records.push('');
  }
  records.push(boundary + '--');
  return records.join('\n');
}

export function isEthText(text) {
  return /^\uFEFF?socialcalc:version:/.test(text);
}

export function readEth(text, options) {
  checkEthRowLimit(options);
  const rows = [];
  for (const line of text.split(/\r?\n/)) {
    if (!line.startsWith('cell:')) continue;
    const [, address, kind, subtype, cache, ...rest] = line.split(':');
    if (!/^[A-Z]{1,3}[1-9]\d{0,6}$/.test(address)) throw failure('INVALID_INPUT', 'Invalid EtherCalc cell address');
    const { r, c } = decode_cell(address);
    if (r >= 1048576 || c >= 16384) throw failure('INVALID_INPUT', 'EtherCalc cell is outside worksheet limits');
    if (options.sheetRows && r >= options.sheetRows) continue;
    let value;
    if (kind === 't') value = decode(subtype ?? '');
    else if (kind === 'v') value = Number(subtype);
    else if (kind === 'vtc' || kind === 'vtf') {
      const error = subtype?.startsWith('e') ? errorCode(subtype.slice(1)) : errorCode(rest.at(-1));
      if (error !== undefined) value = { t: 'e', v: error };
      else if (subtype === 'nl') value = Number(cache) !== 0;
      else value = Number(cache);
      if (kind === 'vtf' && options.cellFormula !== false && rest[0]) {
        const formula = decode(rest[0]);
        if (typeof value === 'object') value.f = formula;
        else value = [value, formula];
      }
    }
    if (value !== undefined) (rows[r] ??= [])[c] = value;
  }
  return book_new(aoa_to_sheet(rows, options));
}
