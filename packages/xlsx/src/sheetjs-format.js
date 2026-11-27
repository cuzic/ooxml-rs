import { dateText, displaySerialFromDate, serialFromDate } from './sheetjs-date.js';
import { errorName } from './sheetjs-csv-input.js';
import { failure } from './contract.js';

export const formatTable = {
  0: 'General', 1: '0', 2: '0.00', 3: '#,##0', 4: '#,##0.00',
  9: '0%', 10: '0.00%', 11: '0.00E+00', 12: '# ?/?', 13: '# ??/??',
  14: 'm/d/yy', 15: 'd-mmm-yy', 16: 'd-mmm', 17: 'mmm-yy',
  18: 'h:mm AM/PM', 19: 'h:mm:ss AM/PM', 20: 'h:mm', 21: 'h:mm:ss', 22: 'm/d/yy h:mm',
  37: '#,##0 ;(#,##0)', 38: '#,##0 ;[Red](#,##0)', 39: '#,##0.00;(#,##0.00)', 40: '#,##0.00;[Red](#,##0.00)',
  45: 'mm:ss', 46: '[h]:mm:ss', 47: 'mmss.0', 48: '##0.0E+0', 49: '@',
  56: '"上午/下午 "hh"時"mm"分"ss"秒 "',
};

function accountingPattern(currency, decimals) {
  const symbol = currency ? '"$"' : '';
  const number = decimals ? '#,##0.00' : '#,##0';
  const zeros = decimals ? '??' : '';
  return `_(${symbol}* ${number}_);_(${symbol}* \\(${number}\\);_(${symbol}* "-"${zeros}_);_(@_)`;
}

const accountingFormats = [
  accountingPattern(false, false), accountingPattern(true, false),
  accountingPattern(false, true), accountingPattern(true, true),
];
const implicitFormats = {
  41: accountingFormats[0], 42: accountingFormats[1], 43: accountingFormats[2], 44: accountingFormats[3],
  63: '"$"#,##0_);\\("$"#,##0\\)',
  64: '"$"#,##0_);[Red]\\("$"#,##0\\)',
  65: '"$"#,##0.00_);\\("$"#,##0.00\\)',
  66: '"$"#,##0.00_);[Red]\\("$"#,##0.00\\)',
};
const builtinAliases = {
  5: 37, 6: 38, 7: 39, 8: 40,
  23: 0, 24: 0, 25: 0, 26: 0,
  27: 14, 28: 14, 29: 14, 30: 14, 31: 14,
  50: 14, 51: 14, 52: 14, 53: 14, 54: 14, 55: 14, 56: 14, 57: 14, 58: 14,
  59: 1, 60: 2, 61: 3, 62: 4,
  67: 9, 68: 10, 69: 12, 70: 13, 71: 14, 72: 14, 73: 15, 74: 16, 75: 17,
  76: 20, 77: 21, 78: 22, 79: 45, 80: 46, 81: 47, 82: 0,
};

export function numberFormatCode(code, table = formatTable) {
  if (typeof code !== 'number') return code;
  const alias = builtinAliases[code];
  return table[code] ?? table[alias] ?? formatTable[alias] ?? implicitFormats[code];
}

function numberText(value, format) {
  const match = /^(0+|#+0*|#,##0|#,###)(?:\.([0#]+))?(,{1,6})?(%)?$/.exec(format);
  if (!match) return undefined;
  const [, integer, fraction, commas, percent] = match;
  const optional = fraction?.includes('#') ?? false;
  if (optional && integer.includes(',')) return undefined;
  const digits = fraction?.length ?? 0;
  const scaled = value * (percent ? 100 : 1) / 1000 ** (commas?.length ?? 0);
  if (digits > 10 || Math.abs(scaled) >= 1e21) return undefined;
  const factor = 10 ** digits;
  const grouped = integer.includes(',');
  const minimumWhole = integer.replace(/[^0]/g, '').length;
  const signedRounding = integer === '0' || optional || (!grouped && integer.startsWith('#') &&
    (digits > 0 || minimumWhole === 0));
  let rounded;
  if (signedRounding) rounded = (Math.abs(Math.round(scaled * factor)) / factor).toFixed(digits);
  else if (grouped && digits) rounded = Math.abs(scaled).toFixed(digits);
  else rounded = (Math.round(Math.abs(scaled) * factor) / factor).toFixed(digits);
  let [whole, decimals] = rounded.split('.');
  if (grouped) whole = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  else if (!optional) whole = whole.padStart(minimumWhole, '0');
  if (minimumWhole === 0 && whole === '0' && !(scaled < 0 && Number(rounded) !== 0)) whole = '';
  if (optional) {
    const minimum = fraction.replaceAll('#', '').length;
    decimals = decimals.replace(/0+$/, '').padEnd(minimum, '0');
  }
  const negative = scaled < 0 && ((!signedRounding && minimumWhole > 0) || Number(rounded) !== 0);
  return (negative ? '-' : '') + whole + (decimals === undefined ? '' : '.' + decimals) + (percent || '');
}

function fixedDigitGroups(value, format) {
  if (!/^0+(?:(?:-|\\-| )0+)+$/.test(format) || !Number.isFinite(value) || Math.abs(value) >= 1e21) {
    return undefined;
  }
  const groups = format.split(/(\\-|-| )/);
  const width = groups.reduce((total, group, index) => total + (index % 2 ? 0 : group.length), 0);
  const fixedWidth = groups.includes('-');
  const digits = String(Math.round(Math.abs(value))).padStart(width, '0');
  let end = digits.length;
  for (let index = groups.length - 1; index > 0; index -= 2) {
    const start = end - groups[index].length;
    groups[index] = digits.slice(start, end);
    groups[index - 1] = groups[index - 1].replace('\\', '');
    end = start;
  }
  groups[0] = digits.slice(fixedWidth ? end - groups[0].length : 0, end);
  return (value < 0 && !fixedWidth ? '-' : '') + groups.join('');
}

function phoneNumberText(value, format) {
  const match = /^\((0+|#+)\) (0+|#+)-(0+|#+)$/.exec(format);
  if (!match || !Number.isFinite(value) || Math.abs(value) >= 1e21) return undefined;
  const [, area, exchange, line] = match;
  const required = area.startsWith('0');
  if ([exchange, line].some(group => group.startsWith('0') !== required)) return undefined;
  const negative = value < 0;
  const width = area.length + exchange.length + line.length - Number(negative);
  const rounded = required ? Math.round(Math.abs(value)) : Math.abs(Math.round(value));
  const digits = (required ? String(rounded).padStart(width, '0') : rounded ? String(rounded) : '').slice(-width);
  const areaEnd = Math.max(0, digits.length - exchange.length - line.length);
  const exchangeEnd = Math.max(0, digits.length - line.length);
  const separator = !required && negative && rounded === 0 ? '' : '-';
  return (negative ? '-' : '') + '(' + digits.slice(0, areaEnd) + ') ' +
    digits.slice(areaEnd, exchangeEnd) + separator + digits.slice(exchangeEnd);
}

function scientificText(value, format) {
  const match = /^(0|##0)\.(0{1,10})E([+-])(0{1,4})$/i.exec(format);
  if (!match || !Number.isFinite(value)) return undefined;
  const [, integer, fraction, positiveSign, exponentDigits] = match;
  const magnitude = Math.abs(value);
  const [normalized, exponentText] = magnitude.toExponential(fraction.length).split('e');
  let exponent = Number(exponentText);
  let mantissa = normalized;
  if (integer === '##0' && magnitude !== 0) {
    const [significand, power] = magnitude.toExponential().split('e');
    const decimalExponent = Number(power);
    exponent = 3 * Math.floor(decimalExponent / 3);
    const scaled = Number(significand) * 10 ** (decimalExponent - exponent);
    const factor = 10 ** fraction.length;
    mantissa = (Math.round(scaled * factor) / factor).toFixed(fraction.length);
    if (Number(mantissa) >= 1000) mantissa = mantissa.slice(0, mantissa.indexOf('.') + 1);
  }
  const sign = exponent < 0 ? '-' : positiveSign === '+' ? '+' : '';
  return (value < 0 ? '-' : '') + mantissa + 'E' + sign + String(Math.abs(exponent)).padStart(exponentDigits.length, '0');
}

function sectionNumberText(value, format) {
  const match = /^(#,##0(?:\.0+)?)( *);(?:\[[a-z]+\])?\(\1\)$/i.exec(format);
  if (!match) return undefined;
  const text = numberText(Math.abs(value), match[1]);
  if (text === undefined) return undefined;
  return value < 0 ? '(' + text + ')' : text + match[2];
}

function fractionConvergent(value, maxDenominator) {
  let numerator = 1;
  let denominator = 0;
  let previousNumerator = 0;
  let previousDenominator = 1;
  while (denominator < maxDenominator) {
    const integer = Math.floor(value);
    const nextDenominator = integer * denominator + previousDenominator;
    if (nextDenominator > maxDenominator) break;
    [previousNumerator, numerator] = [numerator, integer * numerator + previousNumerator];
    [previousDenominator, denominator] = [denominator, nextDenominator];
    const rest = value - integer;
    if (rest < 0.00000005) break;
    value = 1 / rest;
  }
  return [numerator, denominator];
}

function mixedFractionText(value, format) {
  const match = /^# (\?{1,7})( ?)\/( ?)(\?{1,7}|[1-9]\d{0,6})$/.exec(format);
  if (!match || !Number.isFinite(value) || Math.abs(value) >= 1e9) return undefined;
  const [, numeratorPattern, beforeSlash, afterSlash, denominatorPattern] = match;
  const fixed = denominatorPattern[0] !== '?';
  const width = fixed ? numeratorPattern.length : Math.max(numeratorPattern.length, denominatorPattern.length);
  const absolute = Math.abs(value);
  const [numerator, denominator] = fixed
    ? [Math.round(absolute * Number(denominatorPattern)), Number(denominatorPattern)]
    : fractionConvergent(absolute, 10 ** width - 1);
  const whole = Math.floor(numerator / denominator);
  const remainder = numerator % denominator;
  const sign = value < 0 ? '-' : '';
  if (!remainder) {
    const divisorWidth = fixed ? denominatorPattern.length : width;
    const blankWidth = width + divisorWidth + 2 + beforeSlash.length + afterSlash.length;
    return sign + (whole || (fixed ? '' : '0')) + ' '.repeat(blankWidth);
  }
  const divisor = fixed ? String(denominator) : String(denominator).padEnd(width);
  return sign + (whole || '') + ' ' + String(remainder).padStart(width) + beforeSlash + '/' + afterSlash + divisor;
}

function currencyText(value, format) {
  const currency = /^"([^"]+)"(#,##0(?:\.0+)?)_\);(?:\[[a-z]+\])?\\\("\1"\2\\\)$/i.exec(format);
  if (!currency) return undefined;
  const digits = numberText(Math.abs(value), currency[2]);
  if (digits === undefined) return undefined;
  const text = currency[1] + digits;
  return value < 0 ? '(' + text + ')' : text + ' ';
}

function accountingText(value, format) {
  const index = accountingFormats.indexOf(format);
  if (index < 0) return undefined;
  if (typeof value === 'string') return ' ' + value + ' ';
  if (typeof value !== 'number' || !Number.isFinite(value)) return undefined;
  const decimal = index >= 2;
  const prefix = index % 2 ? ' $' : ' ';
  if (value === 0) return prefix + (decimal ? '-   ' : '- ');
  const digits = numberText(Math.abs(value), decimal ? '#,##0.00' : '#,##0');
  if (digits === undefined) return undefined;
  return prefix + (value < 0 ? '(' + digits + ')' : digits + ' ');
}

function formatSections(format) {
  // The pinned SheetJS release also splits at unescaped semicolons inside quotes.
  const sections = [''];
  let quoted = false;
  let skippedContinuation = false;
  let continuationHasText = false;
  let discardSectionTail = false;
  for (const token of format.match(/\\.|_.|\*.|[^]/g) ?? []) {
    if (token === ';') {
      sections.push('');
      skippedContinuation = quoted;
      continuationHasText = false;
      discardSectionTail = false;
    } else if (token === '"') {
      quoted = !quoted;
      if (skippedContinuation) {
        skippedContinuation = false;
        discardSectionTail = continuationHasText;
      } else if (!discardSectionTail) sections[sections.length - 1] += token;
    } else if (skippedContinuation) {
      continuationHasText = true;
    } else if (!discardSectionTail) {
      sections[sections.length - 1] += token;
    }
  }
  if (quoted) return undefined;
  return sections;
}

function numberCondition(section, value) {
  const match = /\[(<=|>=|<>|=|<|>)([+-]?(?:\d+(?:\.\d*)?|\.\d+))\]/.exec(section);
  if (!match) return undefined;
  const [, operator, bound] = match;
  const reference = Number(bound);
  switch (operator) {
    case '<': return value < reference;
    case '>': return value > reference;
    case '<=': return value <= reference;
    case '>=': return value >= reference;
    case '<>': return value !== reference;
    default: return value === reference;
  }
}

function customSectionText(value, format) {
  if (typeof format !== 'string' || !/[;"\\_*\[]/.test(format)) return undefined;
  const sections = formatSections(format);
  if (!sections || sections.length > 4) return undefined;
  const textTokens = /"([^"]*)(?:"|$)|\\(.)|_(.)|\[\$([^\]\-]*)(?:-[0-9a-f]+)?\]|\[([a-z]+|(?:<=|>=|<>|=|<|>)[+-]?[\d.]+)\]|\*[ *]?|@|(?:#,##0|#,###|0+|#+0*)(?:\.[0#]+)?,{0,6}%?|[ ()$€¥+\-]/gi;
  const last = sections.length - 1;
  const hasText = sections.length === 4 || sections[last].includes('@');
  const numeric = hasText ? sections.slice(0, -1) : sections;
  let index = 0;
  let magnitude = value;
  if (typeof value === 'string') {
    if (!hasText) return value;
    index = last;
  } else if (typeof value === 'number' && Number.isFinite(value)) {
    if (!numeric.length) return String(value);
    const first = numberCondition(numeric[0], value);
    const second = numberCondition(numeric[1] ?? '', value);
    if (first !== undefined || second !== undefined) {
      index = first ? 0 : second ? 1 : first !== undefined && second !== undefined ? 2 : 1;
      if (numeric[index] === undefined) index = 0;
      if (value < 0 && numeric.length > 1) magnitude = -value;
    } else if (value < 0 && numeric.length > 1) {
      index = 1;
      magnitude = -value;
    } else if (value === 0 && numeric.length > 2) {
      index = 2;
    }
  } else {
    return undefined;
  }
  const section = sections[index];
  if (/^general/i.test(section)) return generalText(value);
  const parts = Array.from(section.matchAll(textTokens));
  if (parts.map(([token]) => token).join('') !== section) return undefined;
  const currencyNegative = typeof magnitude === 'number' && magnitude < 0 &&
    parts.some(([, , , , currency]) => currency !== undefined);
  if (currencyNegative) magnitude = -magnitude;
  let rendered = currencyNegative ? '-' : '';
  let numberSeen = false;
  for (const [token, literal, escaped, spacing, currency, directive] of parts) {
    if (literal !== undefined) rendered += literal;
    else if (escaped !== undefined) rendered += escaped;
    else if (spacing !== undefined) rendered += ' ';
    else if (currency !== undefined) rendered += currency || '$';
    else if (directive !== undefined || token.startsWith('*')) continue;
    else if (token === '@') rendered += String(value);
    else if (/[0#]/.test(token)) {
      if (numberSeen || typeof magnitude !== 'number') return undefined;
      const number = numberText(magnitude, token);
      if (number === undefined) return undefined;
      rendered += number;
      numberSeen = true;
    } else rendered += token;
  }
  return rendered;
}

function trimFraction(number) {
  return number.replace(/(\.\d*?[1-9])0+$|\.0+$/, '$1');
}

function generalNumber(value) {
  if (!Number.isFinite(value)) return Number.isNaN(value) ? '#NUM!' : '#DIV/0!';
  if (Number.isInteger(value) && value >= -2147483648 && value <= 2147483647) return String(value);
  const exponent = Math.floor(Math.log(Math.abs(value)) * Math.LOG10E);
  const width = value < 0 ? 12 : 11;
  let text;
  if (exponent >= -4 && exponent < 0) {
    text = value.toPrecision(10 + exponent);
  } else if (Math.abs(exponent) <= 9) {
    text = trimFraction(value.toFixed(12));
    if (text.length > width) text = value.toPrecision(10);
    if (text.length > width) text = value.toExponential(5);
  } else if (exponent === 10) {
    text = value.toFixed(10).slice(0, 12);
  } else {
    text = trimFraction(value.toFixed(11));
    if (text.length > width || Number(text) === 0) text = value.toPrecision(6);
  }
  const [mantissa, power] = text.toUpperCase().split('E');
  return trimFraction(mantissa) + (power === undefined ? '' : 'E' + power[0] + power.slice(1).padStart(2, '0'));
}

function generalText(value, options) {
  if (typeof value === 'number') return generalNumber(value);
  if (value instanceof Date) {
    return dateText(value, options?.dateNF ?? 'm/d/yy', options?.date1904) ??
      dateText(value, 'm/d/yy', options?.date1904);
  }
  return String(value);
}

export function formatted(cell, value, options, fallbackCalendar = true) {
  if (cell?.t == null || cell.t === 'z') return '';
  if (cell.w !== undefined) return cell.w;
  if (cell.t === 'e') return errorName(cell.v) ?? cell.v;
  if (value === undefined) value = cell.v;
  if (value == null || value === '') return '';
  if (cell.t === 'b') value = value ? 'TRUE' : 'FALSE';
  const code = cell.z ?? (cell.t === 'd' ? options?.dateNF ?? 14 : cell.XF?.numFmtId ?? 0);
  const format = numberFormatCode(code) ?? 'General';
  const accounting = accountingText(value, format);
  if (accounting !== undefined) return accounting;
  if (format === '@' && value instanceof Date) return generalNumber(serialFromDate(value.getTime(), options?.date1904));
  if (typeof format === 'string' && /^(?:general|@)$/i.test(format)) return generalText(value, options);
  if (typeof value === 'string') return customSectionText(value, format) ?? value;
  if (typeof value === 'number' && !Number.isFinite(value) && /0/.test(format)) {
    return Number.isNaN(value) ? '#NUM!' : '#DIV/0!';
  }
  const date = dateText(value, format, options?.date1904);
  if (date === null && fallbackCalendar) {
    const fallback = cell.XF?.numFmtId || (cell.t === 'd' || value instanceof Date ? 14 : 0);
    return formatted({ t: cell.t, z: fallback }, value, options, false);
  }
  if (date != null) return date;
  const number = typeof value === 'number'
    ? numberText(value, format) ?? scientificText(value, format) ?? sectionNumberText(value, format) ?? mixedFractionText(value, format) ??
      currencyText(value, format) ?? fixedDigitGroups(value, format) ?? phoneNumberText(value, format)
    : undefined;
  if (number !== undefined) return number;
  const custom = customSectionText(value, format);
  if (custom !== undefined) return custom;
  throw failure('UNSUPPORTED_FORMAT', 'This cell requires an unsupported spreadsheet number format');
}

export function format_cell(cell, value, options) {
  if (cell?.t == null || cell.t === 'z') return '';
  if (cell.w !== undefined) return cell.w;
  if (cell.t === 'd' && !cell.z && options?.dateNF) cell.z = options.dateNF;
  const displayValue = value ?? cell.v;
  const isDate = cell.t === 'd' && displayValue instanceof Date;
  const serial = isDate ? displaySerialFromDate(displayValue.getTime(), options?.date1904) : undefined;
  const formattedValue = Number.isFinite(serial) ? serial : displayValue;
  const dateStyle = isDate && cell.z == null ? cell.XF?.numFmtId : undefined;
  let text;
  try {
    text = formatted(dateStyle ? { t: cell.t, z: dateStyle } : cell, formattedValue, options);
  } catch (error) {
    if (error?.code !== 'UNSUPPORTED_FORMAT') throw error;
    try {
      const fallback = cell.XF?.numFmtId || (isDate ? 14 : 0);
      text = formatted({ t: cell.t, z: fallback }, formattedValue, options);
    } catch (fallbackError) {
      if (fallbackError?.code !== 'UNSUPPORTED_FORMAT') throw fallbackError;
      return String(displayValue);
    }
  }
  if (cell.t !== 'e') cell.w = text;
  return text;
}
