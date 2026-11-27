import { checkRtfRowLimit } from './sheetjs-options.generated.js';
import { failure } from './contract.js';
import { book_new } from './sheetjs-book.js';
import { errorCode, numberFromText } from './sheetjs-csv-input.js';
import { decode_range, encode_cell, encode_range } from './sheetjs-address.js';
import { formatted } from './sheetjs-format.js';

function escapeRtf(value) {
  return String(value).replace(/[{}\\\r\n\t\u0080-\uffff]/g, (character) => {
    if (character === '\r' || character === '\n') return '\\par ';
    if (character === '\t') return '\\tab ';
    const code = character.charCodeAt(0);
    if (code < 128) return '\\' + character;
    return '\\u' + (code >= 32768 ? code - 65536 : code) + '?';
  });
}

export function writeRtf(sheet) {
  const output = ['{\\rtf1\\ansi'];
  if (sheet['!ref']) {
    const { s, e } = decode_range(sheet['!ref']);
    const dense = sheet['!data'];
    let tableRow = '\\trowd\\trautofit1';
    for (let c = s.c; c <= e.c; c++) tableRow += '\\cellx' + (c + 1);
    tableRow += '\\pard\\intbl';
    for (let r = s.r; r <= e.r; r++) {
      output.push(tableRow);
      for (let c = s.c; c <= e.c; c++) {
        const cell = dense ? dense[r]?.[c] : sheet[encode_cell({ r, c })];
        const text = cell?.v == null ? '' : escapeRtf(formatted(cell, cell.v));
        output.push(' ', text, '\\cell');
      }
      output.push('\\pard\\intbl\\row');
    }
  }
  output.push('}');
  return output.join('');
}

export function isRtfText(text) {
  return /^\uFEFF?\{\\rtf\d+/.test(text);
}

function parseCell(text, options) {
  if (text === 'TRUE' || text === 'FALSE') return { t: 'b', v: text === 'TRUE' };
  const number = numberFromText(text);
  if (number !== undefined) return { t: 'n', v: number, ...(options.cellText === false ? {} : { w: text }) };
  const error = errorCode(text);
  return error === undefined ? { t: 's', v: text } : { t: 'e', v: error, w: text };
}

export function readRtf(text, options) {
  checkRtfRowLimit(options);
  const sheet = options.dense ? { '!data': [] } : {};
  const tokens = /\\([A-Za-z]+)(-?\d+)? ?|\\'([a-f\d]{2})|\\([^\r\n])|([{}])|([^\\{}])/gi;
  const stack = [];
  let state = { hidden: false, fallback: 1 };
  let skip = 0;
  let payload = '';
  let row = -1;
  let col = 0;
  let width = 0;
  let active = false;
  let inCell = false;
  const codepage = new TextDecoder('windows-1252');
  const append = (chunk) => {
    if (skip) {
      skip--;
    } else if (inCell) {
      payload += chunk;
    }
  };
  for (const [, word, value, hex, symbol, brace, plain] of text.matchAll(tokens)) {
    if (brace === '{') {
      stack.push(state);
      state = { ...state };
      continue;
    }
    if (brace === '}') {
      if (!stack.length) throw failure('INVALID_INPUT', 'Unbalanced RTF group');
      state = stack.pop();
      continue;
    }
    if (symbol === '*') state.hidden = true;
    if (['fonttbl', 'colortbl', 'stylesheet', 'info', 'pict', 'object', 'header', 'footer'].includes(word)) {
      state.hidden = true;
    }
    if (state.hidden) continue;
    if (plain !== undefined) {
      if (plain !== '\r' && plain !== '\n') append(plain);
    } else if (hex !== undefined) {
      append(codepage.decode(Uint8Array.of(Number.parseInt(hex, 16))));
    } else if (symbol !== undefined) {
      if ('{}\\'.includes(symbol)) append(symbol);
      else if (symbol === '~') append('\u00a0');
    } else if (word === 'uc') {
      state.fallback = Math.max(0, Number(value) || 0);
    } else if (word === 'u') {
      append(String.fromCharCode(Number(value) & 65535));
      skip = state.fallback;
    } else if (word === 'trowd') {
      active = true;
      inCell = false;
      payload = '';
      col = 0;
      row++;
      if (row >= 1048576) throw failure('INVALID_INPUT', 'RTF table row limit exceeded');
      if (options.sheetRows && row >= options.sheetRows) {
        row--;
        break;
      }
    } else if (word === 'intbl' && active) {
      inCell = true;
    } else if (word === 'par' || word === 'line' || word === 'tab') {
      append(word === 'tab' ? '\t' : '\n');
    } else if (word === 'cell' && active) {
      if (col >= 16384) throw failure('INVALID_INPUT', 'RTF table column limit exceeded');
      if (payload) {
        const cell = parseCell(payload, options);
        if (options.dense) (sheet['!data'][row] ??= [])[col] = cell;
        else sheet[encode_cell({ r: row, c: col })] = cell;
      }
      payload = '';
      col++;
      inCell = true;
    } else if (word === 'row' && active) {
      width = Math.max(width, col);
      active = inCell = false;
    }
  }
  if (row < 0) throw failure('INVALID_INPUT', 'RTF table is missing');
  sheet['!ref'] = encode_range({ r: 0, c: 0 }, { r: row, c: Math.max(0, width - 1) });
  return { ...book_new(sheet), bookType: 'rtf' };
}
