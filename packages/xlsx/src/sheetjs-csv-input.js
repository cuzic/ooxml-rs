import { aoa_to_sheet } from './sheetjs-input.js';
import { dateText, serialFromDate } from './sheetjs-date.js';
import { encode_cell, encode_range } from './sheetjs-address.js';

function guessSeparator(text) {
  const counts = new Map([[',', 0], ['\t', 0], [';', 0], ['|', 0]]);
  let quoted = false;
  for (const character of text.slice(0, 1024)) {
    if (character === '"') quoted = !quoted;
    else if (!quoted && counts.has(character)) counts.set(character, counts.get(character) + 1);
  }
  return [...counts].sort((a, b) => b[1] - a[1])[0][0];
}

const errors = new Map([
  ['#NULL!', 0], ['#DIV/0!', 7], ['#VALUE!', 15], ['#REF!', 23],
  ['#NAME?', 29], ['#NUM!', 36], ['#N/A', 42], ['#GETTING_DATA', 43],
]);

const errorNames = new Map([...errors].map(([name, code]) => [code, name]));

export function errorCode(name) {
  return errors.get(name);
}

export function errorName(code) {
  return errorNames.get(code);
}

function timeFromText(text) {
  const match = /^(\d{1,2})(?::([0-5]?\d)(?::([0-5]?\d)(\.\d+)?)?)?(?:\s+([AP])M?)?$/i.exec(text);
  if (!match) return undefined;
  const [, h, m, s, fraction, period] = match;
  if (m === undefined && !period) return undefined;
  let hour = Number(h);
  if (period) {
    if (hour < 1 || hour > 12) return undefined;
    hour = hour % 12 + (period.toUpperCase() === 'P' ? 12 : 0);
  } else if (hour > 23) return undefined;
  return new Date(Date.UTC(1899, 11, 31, hour, Number(m ?? 0), Number(s ?? 0),
    Number((fraction ?? '').slice(1).padEnd(3, '0').slice(0, 3))));
}

export function dateFromText(text, options) {
  const trimmed = text.trim();
  const instant = /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?$/i.exec(trimmed);
  const wallTime = timeFromText(trimmed);
  let date;
  if (wallTime) {
    date = wallTime;
  } else if (instant) {
    const timestamp = trimmed.replace(' ', 'T');
    const hasOffset = /(?:Z|[+-]\d{2}:?\d{2})$/i.test(timestamp);
    date = new Date(hasOffset ? timestamp : timestamp + 'Z');
  } else {
    const yearFirst = /^(\d{4})[-/](\d{1,2})[-/](\d{1,2})$/.exec(trimmed);
    const yearLast = /^(\d{1,2})([-/])(\d{1,2})\2(\d{2}|\d{4})$/.exec(trimmed);
    if (!yearFirst && !yearLast) return undefined;
    const dayFirst = !yearFirst && /^d{1,2}[-/]m{1,2}[-/]y{2,4}$/i.test(options.dateNF ?? '');
    const [yearText, monthText, dayText] = yearFirst ? yearFirst.slice(1) :
      [yearLast[4], yearLast[dayFirst ? 3 : 1], yearLast[dayFirst ? 1 : 3]];
    let year = Number(yearText);
    if (yearText.length === 2) year += year < 50 ? 2000 : 1900;
    const month = Number(monthText);
    const day = Number(dayText);
    if (month < 1 || month > 12 || day < 1 || day > 31) return undefined;
    date = new Date(0);
    date.setUTCFullYear(year, month - 1, day);
  }
  if (!Number.isFinite(date.getTime())) return undefined;
  if (options.UTC === false) date = new Date(date.getTime() + date.getTimezoneOffset() * 60000);
  return date;
}

function matchesDateFormat(text, format) {
  const tokens = /^([dmy]+)([-/])([dmy]+)\2([dmy]+)$/i.exec(format);
  const parts = /^(\d+)([-/])(\d+)\2(\d+)$/.exec(text.trim());
  if (!tokens || !parts || tokens[2] !== parts[2]) return false;
  return [1, 3, 4].every((index) => {
    const length = parts[index].length;
    return tokens[index][0].toLowerCase() === 'y' ? length === 2 || length === 4 : length <= 2;
  });
}

export function numberFromText(text) {
  if (!text.trim()) return undefined;
  const direct = Number(text);
  if (Number.isFinite(direct)) return direct;
  if (!/\d/.test(text)) return undefined;
  let divisor = 1;
  let normalized = text.replace(/(\d),(?=\d)/g, '$1').replaceAll('$', '').replace(/%/g, () => {
    divisor *= 100;
    return '';
  });
  if (Number.isFinite(Number(normalized))) return Number(normalized) / divisor;
  normalized = normalized.replace(/\(([^()]*)\)/, (_, value) => {
    divisor = -divisor;
    return value;
  });
  const value = Number(normalized) / divisor;
  return Number.isFinite(value) ? value : undefined;
}

export function cellFromText(text, options, delimited = false) {
  const cell = { t: 's', v: text };
  if (options.cellText !== false) cell.w = text;
  if (options.raw) return cell;
  const number = numberFromText(text);
  if (text.startsWith('="') && text.endsWith('"') && text.length > 2) {
    cell.v = text.slice(2, -1).replaceAll('""', '"');
  } else if (delimited && text.startsWith('=') && text.length > 1) {
    cell.f = text.slice(1);
  } else if (text === 'TRUE' || text === 'FALSE') {
    cell.t = 'b';
    cell.v = text === 'TRUE';
  } else if (number !== undefined) {
    cell.t = 'n';
    cell.v = number;
  } else if (errors.has(text)) {
    cell.t = 'e';
    cell.v = errors.get(text);
    cell.w = text;
  } else {
    const date = dateFromText(text, options);
    if (date) {
      cell.t = options.cellDates ? 'd' : 'n';
      cell.v = options.cellDates ? date : serialFromDate(date.getTime(), false);
      if (options.cellNF) cell.z = options.dateNF ?? 'm/d/yy';
      if (options.cellText !== false && options.dateNF && !matchesDateFormat(text, options.dateNF)) {
        cell.w = dateText(date, options.dateNF, false) ?? text;
      }
    }
  }
  return cell;
}

function readFixedWidth(text, options) {
  const lines = text.split(/[\r\n]/);
  while (lines.length && lines.at(-1) === '') lines.pop();
  let firstWidth = 10;
  for (const line of lines) {
    const space = line.indexOf(' ');
    firstWidth = Math.max(firstWidth, space < 0 ? line.length : space + 1);
  }
  const rows = [];
  const limit = options.sheetRows ? Math.min(options.sheetRows, lines.length) : lines.length;
  const scalarOptions = { cellDates: true, cellText: false };
  for (let row = 0; row < limit; row += 1) {
    const line = lines[row];
    const cells = [];
    rows.push(cells);
    const fields = [line.slice(0, firstWidth)];
    for (let offset = firstWidth; offset <= line.length; offset += 10) fields.push(line.slice(offset, offset + 10));
    fields.forEach((field, column) => {
      const text = field.trim();
      if (options.raw) cells[column] = text;
      else if (text) {
        const cell = cellFromText(text, scalarOptions);
        cells[column] = cell.t === 'e' ? cell : cell.t === 's' ? text : cell.v;
      }
    });
  }
  return { SheetNames: ['Sheet1'], Sheets: { Sheet1: aoa_to_sheet(rows, options) } };
}

export function readDelimited(text, options) {
  text = text.replace(/^\ufeff/, '');
  if (options.PRN && !options.FS && !text.startsWith('sep=') && !/[\t,;]/.test(text)) {
    return readFixedWidth(text, options);
  }
  const directive = /^sep=(.)(?:\r\n|\r|\n)/.exec(text);
  const separator = directive?.[1] ?? (options.FS || guessSeparator(text))[0];
  if (directive) text = text.slice(directive[0].length);
  const worksheet = options.dense ? { '!data': [] } : {};
  const rows = worksheet['!data'];
  const end = { r: 0, c: 0 };
  let row = 0;
  let col = 0;
  let start = 0;
  let quoted = false;

  function finish(index) {
    let field = text.slice(start, index);
    if (field.startsWith('"') && field.endsWith('"')) {
      field = field.slice(1, -1).replaceAll('""', '"');
    }
    if (field) {
      const cell = cellFromText(field, options, true);
      if (rows) {
        rows[row] ??= [];
        rows[row][col] = cell;
      } else {
        worksheet[encode_cell({ r: row, c: col })] = cell;
      }
    }
    end.r = Math.max(end.r, row);
    end.c = Math.max(end.c, col);
  }

  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (character === '"' && text[start] === '"') quoted = !quoted;
    if (quoted || (character !== separator && character !== '\r' && character !== '\n')) continue;
    finish(index);
    if (character === separator) {
      col += 1;
    } else {
      row += 1;
      col = 0;
      if (character === '\r' && text[index + 1] === '\n') index += 1;
    }
    start = index + 1;
    if (options.sheetRows && row >= options.sheetRows) {
      start = text.length;
      break;
    }
  }
  if (start < text.length) finish(text.length);
  worksheet['!ref'] = encode_range({ s: { r: 0, c: 0 }, e: end });
  return { SheetNames: ['Sheet1'], Sheets: { Sheet1: worksheet } };
}
