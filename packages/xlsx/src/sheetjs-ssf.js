import { displaySerialFromDate } from './sheetjs-date.js';
import { formatTable as _table, formatted, numberFormatCode } from './sheetjs-format.js';

export { formatTable as _table } from './sheetjs-format.js';

export function get_table() {
  return _table;
}

export function load(format, index) {
  if (typeof index !== 'number') {
    let available = Number(index) || -1;
    for (let id = 0; id < 392; id++) {
      if (_table[id] == null) {
        if (available < 0) available = id;
      } else if (_table[id] === format) {
        available = id;
        break;
      }
    }
    index = available < 0 ? 391 : available;
  }
  _table[index] = format;
  return index;
}

export function load_table(formats) {
  for (let id = 0; id < 392; id++) {
    if (formats[id] !== undefined) load(formats[id], id);
  }
}

export function is_date(format) {
  const tokens = /"[^"]*(?:"|$)|[\\_].|\[([^\]]*)\]|general|[0#?.][0#?.,E+%-]*|AM\/PM|A\/P|上午\/下午|[bmdyhseg]/gi;
  for (const [token, bracket] of format.matchAll(tokens)) {
    if (bracket !== undefined) {
      if (/^[hms]+$/i.test(bracket)) return true;
      continue;
    }
    if (token.startsWith('g') ||
        (/^[0#?.]/.test(token) && token.includes('e')) ||
        (/^[bmdyhsea上]/i.test(token))) return true;
  }
  return false;
}

export function parse_date_code(serial, options, alternateCalendar = false) {
  if (!Number.isFinite(serial) || serial < 0 || serial > 2958465) return null;
  const [mantissa, exponent = '0'] = serial.toPrecision(16).split('e');
  const length = mantissa.startsWith('0.') ? 17 : mantissa.includes('.') ? 16 : 15;
  const normalized = Number(mantissa.slice(0, length) + 'e' + exponent);
  let D = Math.floor(normalized);
  const seconds = (normalized - D) * 86400;
  let time = Math.floor(seconds);
  let u = seconds - time;
  let T = time;
  if (u < 1e-6) u = 0;
  if (u > 0.9999) {
    u = 0;
    if (++time === 86400) {
      time = T = 0;
      D++;
    }
  }
  const day = D + (options?.date1904 ? 1462 : 0);
  let y, m, d, q;
  if (day === 0 || day === 60) {
    [y, m, d, q] = day === 0
      ? alternateCalendar ? [1317, 8, 29, 6] : [1900, 1, 0, 6]
      : alternateCalendar ? [1317, 10, 29, 3] : [1900, 2, 29, 3];
  } else {
    const date = new Date(Date.UTC(1899, 11, 31) + (day > 60 ? day - 1 : day) * 86400000);
    y = date.getUTCFullYear() - (alternateCalendar ? 581 : 0);
    m = date.getUTCMonth() + 1;
    d = date.getUTCDate();
    q = (date.getUTCDay() + ((alternateCalendar ? date.getTime() < 60 : day < 60) ? 6 : 0)) % 7;
  }
  return { D, T, u, y, m, d, H: Math.floor(time / 3600), M: Math.floor(time / 60) % 60, S: time % 60, q };
}

export function format(code, value, options) {
  let formatCode = (code === 14 || code === 'm/d/yy') && options?.dateNF
    ? options.dateNF : typeof code === 'number' ? numberFormatCode(code, options?.table ?? undefined) ?? 'General' : code;
  if (typeof formatCode === 'string' && /^general/i.test(formatCode)) formatCode = 'General';
  if (formatCode == null || formatCode === '') {
    if (typeof value === 'number' || value instanceof Date) return '';
    formatCode = 'General';
  }
  const type = value instanceof Date ? 'd' : typeof value === 'number' ? 'n' : typeof value === 'boolean' ? 'b' : 's';
  if (type === 'd') {
    const serial = displaySerialFromDate(value.getTime(), options?.date1904);
    if (Number.isFinite(serial)) {
      if (formatCode === 'General') return format(14, serial, options);
      return formatted({ t: type, z: formatCode }, serial, options, false);
    }
  }
  return formatted({ t: type, z: formatCode }, value, options, false);
}
