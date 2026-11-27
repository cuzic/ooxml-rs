import {
  checkedCsvTransform, csvFormatterSettings, csvReadDestinationSettings, csvReadStreamSettings, csvWriteStreamSettings,
} from './exceljs-options.generated.js';
import { csvReadMap, readCsvRows } from './exceljs-csv-input.js';
import { worksheetRows } from './exceljs-csv-rows.js';

const months = new Intl.DateTimeFormat('en', { month: 'long', timeZone: 'UTC' });
const weekdays = new Intl.DateTimeFormat('en', { weekday: 'long', timeZone: 'UTC' });
const monthNames = Array.from({ length: 12 }, (_, month) => months.format(Date.UTC(2000, month, 1)));
const weekdayNames = Array.from({ length: 7 }, (_, day) => weekdays.format(Date.UTC(2000, 0, day + 2)));
const pad = (value, width = 2) => String(value).padStart(width, '0');

function formatDate(date, { dateFormat, dateUTC }) {
  const local = dateUTC ? date : new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
  const [year, month, day, hour, minute, second, millisecond] = [
    local.getUTCFullYear(), local.getUTCMonth() + 1, local.getUTCDate(), local.getUTCHours(),
    local.getUTCMinutes(), local.getUTCSeconds(), local.getUTCMilliseconds(),
  ];
  const offset = dateUTC ? 0 : -date.getTimezoneOffset();
  const timezone = (offset < 0 ? '-' : '+') + pad(Math.floor(Math.abs(offset) / 60)) + ':' + pad(Math.abs(offset) % 60);
  const dayOfWeek = local.getUTCDay();
  const monthName = monthNames[month - 1];
  const weekName = weekdayNames[dayOfWeek];
  const tokens = {
    YY: pad(year % 100), YYYY: pad(year, 4), M: month, MM: pad(month), MMM: monthName.slice(0, 3), MMMM: monthName,
    D: day, DD: pad(day), d: dayOfWeek, dd: weekName.slice(0, 2), ddd: weekName.slice(0, 3), dddd: weekName,
    H: hour, HH: pad(hour), h: hour % 12 || 12, hh: pad(hour % 12 || 12), m: minute, mm: pad(minute),
    s: second, ss: pad(second), SSS: pad(millisecond, 3), A: hour < 12 ? 'AM' : 'PM', a: hour < 12 ? 'am' : 'pm',
    Z: timezone, ZZ: timezone.replace(':', ''),
  };
  const pattern = dateFormat || (dateUTC ? 'YYYY-MM-DD[T]HH:mm:ss[Z]' : 'YYYY-MM-DD[T]HH:mm:ssZ');
  return pattern.replace(/\[([^\]]*)\]|YYYY|YY|M{1,4}|D{1,2}|d{1,4}|H{1,2}|h{1,2}|m{1,2}|s{1,2}|SSS|A|a|Z{1,2}/g,
    (token, literal) => literal ?? tokens[token]);
}

const dateFields = {
  YYYY: '\\d{4}', YY: '\\d{2}', M: '\\d{1,2}', MM: '\\d{2}',
  MMM: '[A-Za-z]{3}', MMMM: '[A-Za-z]+', D: '\\d{1,2}', DD: '\\d{2}',
  d: '[0-6]', dd: '[A-Za-z]{2}', ddd: '[A-Za-z]{3}', dddd: '[A-Za-z]+',
  H: '\\d{1,2}', HH: '\\d{2}', h: '\\d{1,2}', hh: '\\d{2}',
  m: '\\d{1,2}', mm: '\\d{2}', s: '\\d{1,2}', ss: '\\d{2}',
  SSS: '\\d{3}', A: 'AM|PM', a: 'am|pm', Z: 'Z|[+-]\\d{2}:\\d{2}', ZZ: 'Z|[+-]\\d{4}',
};
const regexLiteral = value => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

function dateParser(pattern) {
  const tokens = [];
  const expression = Array.from(pattern.matchAll(/\[([^\]]*)\]|YYYY|YY|M{1,4}|D{1,2}|d{1,4}|H{1,2}|h{1,2}|m{1,2}|s{1,2}|SSS|A|a|Z{1,2}|./gs),
    ([part, literal]) => {
      if (!dateFields[part]) return regexLiteral(literal ?? part);
      tokens.push(part);
      return '(' + dateFields[part] + ')';
    }).join('');
  const matcher = new RegExp('^' + expression + '$');
  return text => {
    const match = matcher.exec(text);
    if (!match) return undefined;
    const fields = Object.fromEntries(tokens.map((token, index) => [token, match[index + 1]]));
    const now = new Date();
    const year = fields.YYYY ? Number(fields.YYYY)
      : fields.YY ? Number(fields.YY) + (Number(fields.YY) > 68 ? 1900 : 2000) : now.getFullYear();
    // The reference parser's four-digit years below 100 cannot round-trip.
    if (fields.YYYY && year < 100) return undefined;
    const month = fields.MMMM ? monthNames.indexOf(fields.MMMM)
      : fields.MMM ? monthNames.findIndex(name => name.slice(0, 3) === fields.MMM)
      : Number(fields.MM ?? fields.M ?? (fields.YYYY || fields.YY ? 1 : now.getMonth() + 1)) - 1;
    const day = Number(fields.DD ?? fields.D ?? (fields.MM || fields.M || fields.MMM || fields.MMMM || fields.YYYY || fields.YY ? 1 : now.getDate()));
    const period = (fields.A ?? fields.a)?.toLowerCase();
    let hour = Number(fields.HH ?? fields.H ?? fields.hh ?? fields.h ?? 0);
    if (period) hour = hour % 12 + (period === 'pm' ? 12 : 0);
    const minute = Number(fields.mm ?? fields.m ?? 0);
    const second = Number(fields.ss ?? fields.s ?? 0);
    const millisecond = Number(fields.SSS ?? 0);
    const zone = fields.Z ?? fields.ZZ;
    const date = new Date(0);
    if (zone) {
      date.setUTCFullYear(year, month, day);
      date.setUTCHours(hour, minute, second, millisecond);
      if (zone !== 'Z') {
        const units = zone.replace(':', '');
        const minutes = Number(units.slice(1, 3)) * 60 + Number(units.slice(3));
        date.setTime(date.getTime() - (zone[0] === '-' ? -minutes : minutes) * 60_000);
      }
    } else {
      date.setFullYear(year, month, day);
      date.setHours(hour, minute, second, millisecond);
    }
    return formatDate(date, { dateFormat: pattern }) === text ? date : undefined;
  };
}

function transformRow(row, transform) {
  if (!transform) return row;
  if (transform.length < 2) return transform(row);
  return new Promise((resolve, reject) => {
    transform(row, (error, result) => error ? reject(error) : resolve(result));
  });
}

async function* csvChunks(workbook, options) {
  const {
    delimiter, rowDelimiter, quote: quoteOption, escape,
    quoteColumns, quoteHeaders, headers: requestedHeaders,
    writeHeaders, alwaysWriteHeaders, writeBOM, includeEndRowDelimiter, transform: transformOption,
  } = csvFormatterSettings(csvWriteStreamSettings(options).formatterOptions);
  const transform = checkedCsvTransform(transformOption);
  const quote = typeof quoteOption === 'boolean' ? (quoteOption ? '"' : '') : quoteOption;
  const escapedQuote = (typeof escape === 'string' ? escape : quote) + quote;
  const shouldWriteHeaders = Boolean(requestedHeaders) && writeHeaders;
  const special = new Set(delimiter + rowDelimiter + '|\r\n');
  let wroteLine = false;
  let headers = Array.isArray(requestedHeaders) ? requestedHeaders : undefined;
  let hadInput = false;
  let wroteHeaders = false;

  function line(fields, quoteConfig) {
    const text = fields.map((value, index) => {
      const field = String(value ?? '').replaceAll('\0', '');
      if (quote && field.includes(quote)) return quote + field.replaceAll(quote, () => escapedQuote) + quote;
      const column = Array.isArray(quoteConfig) ? index : headers?.[index];
      const force = typeof quoteConfig === 'boolean' ? quoteConfig : quoteConfig?.[column];
      return force || [...field].some(character => special.has(character)) ? quote + field + quote : field;
    }).join(delimiter);
    const prefix = wroteLine ? rowDelimiter : '';
    wroteLine = true;
    return prefix + text;
  }

  for (const input of worksheetRows(workbook, options, formatDate)) {
    if (!hadInput && writeBOM) yield '\ufeff';
    hadInput = true;
    const row = await transformRow(input, transform);
    if (!row) continue;
    const arrayRow = Array.isArray(row);
    const pairs = arrayRow && Array.isArray(row[0]) && row[0].length === 2;
    const headerFromRow = !headers && shouldWriteHeaders && arrayRow && !pairs;
    headers ??= pairs ? row.map(pair => pair[0]) : arrayRow ? row : Object.keys(row);
    if (shouldWriteHeaders && !wroteHeaders) {
      yield line(headers, quoteHeaders);
      wroteHeaders = true;
    }
    if (headerFromRow) continue;
    if (pairs) yield line(headers.map((_, index) => row[index]?.[1]), quoteColumns);
    else if (!arrayRow) yield line(headers.map(header => row[header]), quoteColumns);
    else yield line(shouldWriteHeaders ? Array.from(headers, (_, index) => row[index]) : row, quoteColumns);
  }

  if (alwaysWriteHeaders && !wroteLine) {
    if (!headers) throw new Error('alwaysWriteHeaders requires headers');
    yield line(headers, quoteHeaders);
  }
  if (includeEndRowDelimiter) yield rowDelimiter;
}

export function createCsv(workbook) {
  async function writeBuffer(options) {
    const chunks = [];
    for await (const chunk of csvChunks(workbook, options ?? {})) chunks.push(chunk);
    return new TextEncoder().encode(chunks.join(''));
  }
  return {
    async read(stream, options) {
      options ??= {};
      const parsers = new Map();
      const map = csvReadMap(options, (text, pattern) => {
        if (!parsers.has(pattern)) parsers.set(pattern, dateParser(pattern));
        return parsers.get(pattern)(text);
      });
      const worksheet = workbook.addWorksheet(csvReadDestinationSettings(options).sheetName);
      const { parserOptions } = csvReadStreamSettings(options);
      worksheet.addRows(await readCsvRows(stream, parserOptions, map));
      return worksheet;
    },
    writeBuffer,
    async write(stream, options) {
      const encoder = new TextEncoder();
      const writer = stream.getWriter();
      let started = false;
      try {
        let pending = '';
        for await (const chunk of csvChunks(workbook, options ?? {})) {
          const text = pending + chunk;
          // A formatter callback can split a UTF-16 pair across chunks.
          const last = text.charCodeAt(text.length - 1);
          const splitPair = last >= 0xd800 && last <= 0xdbff;
          pending = splitPair ? text.slice(-1) : '';
          const ready = splitPair ? text.slice(0, -1) : text;
          if (ready) {
            started = true;
            await writer.write(encoder.encode(ready));
          }
        }
        if (pending) {
          started = true;
          await writer.write(encoder.encode(pending));
        }
        await writer.close();
      } catch (error) {
        if (started) await writer.abort(error).catch(() => {});
        throw error;
      } finally {
        writer.releaseLock();
      }
    },
    createInputStream() {
      throw new Error('CSV#createInputStream is deprecated. Use CSV#read instead.');
    },
  };
}
