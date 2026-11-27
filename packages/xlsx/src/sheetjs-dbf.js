import { failure } from './contract.js';
import { decode_range } from './sheetjs-address.js';
import { dateFromSerial, serialFromDate } from './sheetjs-date.js';
import { book_new } from './sheetjs-book.js';
import { aoa_to_sheet } from './sheetjs-input.js';
import { format } from './sheetjs-ssf.js';
import { jsonRows } from './sheetjs-utils.js';

const sourceCodepages = new WeakMap();
const widths = { B: 8, D: 8, L: 1 };
const codepages = new Map([[437, 0x01], [850, 0x02], [1252, 0x03], [852, 0x64], [866, 0x65],
  [865, 0x66], [861, 0x67], [895, 0x68], [620, 0x69], [737, 0x6a], [857, 0x6b],
  [950, 0x78], [949, 0x79], [936, 0x7a], [932, 0x7b], [874, 0x7c],
  [1255, 0x7d], [1256, 0x7e], [1250, 0xc8], [1251, 0xc9], [1254, 0xca], [1253, 0xcb]]);
const ansi = new Map(Array.from(new TextDecoder('windows-1252').decode(
  Uint8Array.from({ length: 256 }, (_, byte) => byte)), (character, byte) => [character, byte]));
const ascii = text => Uint8Array.from(text, character => character.charCodeAt(0) & 0xff);

function infer(value) {
  if (typeof value === 'number') return 'B';
  if (typeof value === 'boolean') return 'L';
  return value instanceof Date ? 'D' : 'C';
}

function formatDbfDate(value, date1904) {
  if (value == null) return '00000000';
  const serial = typeof value === 'number';
  const date = serial ? dateFromSerial(value, date1904) : value;
  const year = serial ? date?.getUTCFullYear() : date.getFullYear();
  if (!Number.isInteger(year) || year < 0 || year > 9999) {
    throw failure('INVALID_VALUE', 'Expected a DBF date with a four-digit year');
  }
  const month = serial ? date.getUTCMonth() : date.getMonth();
  const day = serial ? date.getUTCDate() : date.getDate();
  return String(year).padStart(4, '0') + String(month + 1).padStart(2, '0') + String(day).padStart(2, '0');
}

function fixedDbfNumber(value, decimals) {
  if (!Number.isFinite(value)) return undefined;
  if (Math.abs(value) < 1e21) return value.toFixed(decimals);

  // At this magnitude every finite JavaScript number is an integer.
  const digits = BigInt(value).toString();
  return decimals ? digits + '.' + '0'.repeat(decimals) : digits;
}

function encodeFieldText(text, width, encode, multibyte) {
  const bytes = encode(text);
  if (bytes.length <= width || !multibyte) return bytes.subarray(0, width);

  // DBF code pages are stateless. Find the longest prefix whose encoded bytes fit.
  let low = 1;
  let high = text.length;
  let result = bytes.subarray(0, 0);
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    const next = text.charCodeAt(middle);
    const previous = text.charCodeAt(middle - 1);
    const splitsSurrogate = next >= 0xdc00 && next <= 0xdfff && previous >= 0xd800 && previous <= 0xdbff;
    const prefix = encode(text.slice(0, splitsSurrogate ? middle - 1 : middle));
    if (prefix.length <= width) {
      result = prefix;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return result;
}

function encodeFieldName(name, encode, width = 10) {
  // The descriptor has ten name bytes followed by a NUL terminator.
  const characters = Array.from(name).slice(-width);
  for (let start = 0; start < characters.length; start += 1) {
    const bytes = encode(characters.slice(start).join(''));
    if (bytes.length <= width) return bytes;
  }
  return new Uint8Array();
}

export function writeDbf(sheet, options, codec, date1904) {
  if (!sheet['!ref']) throw failure('INVALID_INPUT', 'Cannot export an empty sheet to DBF');
  const firstColumn = decode_range(sheet['!ref']).s.c;
  const columns = sheet['!cols'];
  const descriptorAt = column => columns?.[column]?.DBF;
  const [head = [], ...rows] = jsonRows(sheet, { header: 1, raw: true },
    { preserveDateSerialAt: column => descriptorAt(column)?.type === 'D', initializeDenseRow: false });
  const rawPage = options.codepage || sourceCodepages.get(sheet) || 1252;
  const page = codepages.has(rawPage) ? rawPage : 1252;
  if (page !== 1252 && !codec?.encode) {
    throw failure('UNSUPPORTED_OPTIONS', 'DBF codepage requires an encoder supplied through set_cptable');
  }
  const encode = codec?.encode
    ? text => {
      const encoded = codec.encode(page, text);
      return typeof encoded === 'string' ? ascii(encoded) : Uint8Array.from(encoded);
    }
    : text => Uint8Array.from(text, character => ansi.get(character) ?? 63);
  const fields = [];
  const names = head.map(value => typeof value === 'number' ? String(value) : value);
  for (let index = 0; index < Math.max(head.length, (columns?.length ?? 0) - firstColumn); index += 1) {
    const descriptor = descriptorAt(firstColumn + index);
    let name = descriptor?.name ?? names[index];
    if (name == null) continue;
    if (typeof name !== 'string') throw failure('INVALID_VALUE', 'Expected a DBF field name');
    if (!descriptor?.name && names.indexOf(name) !== index) {
      let suffix = 0;
      while (names.includes(name + '_' + suffix)) suffix += 1;
      name += '_' + suffix;
      names[index] = name;
    }
    const values = rows.map(row => row[index]).filter(value => value != null);
    const nullableType = descriptor?.type === 'D' || descriptor?.type === 'L' ? descriptor.type : 'C';
    let type = values.length ? infer(values[0]) : nullableType;
    if (descriptor?.type === 'D' && values.every(value => value instanceof Date || typeof value === 'number')) {
      type = 'D';
    } else if (values.some(value => infer(value) !== type)) {
      type = 'C';
    }
    let width = widths[type] ?? 0;
    let decimals = 0;
    if (type === 'C') {
      for (const value of values) width = Math.max(width, Math.min(250, encode(String(value)).length));
      if (descriptor?.type === 'C') width = Math.max(width, descriptor.len ?? 0);
    } else if (type === 'B' && descriptor?.type === 'N') {
      type = 'N';
      width = descriptor.len;
      decimals = descriptor.dec ?? 0;
    }
    if (!Number.isInteger(width) || width < 0 || width > 255 || !Number.isInteger(decimals) || decimals < 0 || decimals > 100) {
      throw failure('INVALID_VALUE', 'Invalid DBF field width or precision');
    }
    fields.push({ name, explicitName: descriptor?.name != null, type, width, decimals, index });
  }

  const headerLength = 296 + fields.length * 32;
  const recordLength = 1 + fields.reduce((sum, field) => sum + field.width, 0);
  if (headerLength > 0xffff || recordLength > 0xffff) throw failure('INVALID_INPUT', 'DBF record layout is too large');
  const bytes = new Uint8Array(headerLength + rows.length * recordLength + 1);
  const view = new DataView(bytes.buffer);
  bytes.set([0x30, 17, 2, 19]);
  view.setUint32(4, rows.length, true);
  view.setUint16(8, headerLength, true);
  view.setUint16(10, recordLength, true);
  bytes[29] = codepages.get(page);
  const encodedNames = fields.map(({ name }) => encodeFieldName(name, encode));
  const nameKey = name => name.join(',');
  const usedNames = new Set(fields.flatMap((field, index) =>
    field.explicitName ? [nameKey(encodedNames[index])] : []));
  let displacement = 0;
  fields.forEach(({ name, explicitName, type, width, decimals }, index) => {
    const offset = 32 + index * 32;
    let encodedName = encodedNames[index];
    if (!explicitName) {
      for (let count = 0; usedNames.has(nameKey(encodedName)); count += 1) {
        const suffix = ascii('_' + count);
        const prefix = encodeFieldName(name, encode, 10 - suffix.length);
        encodedName = Uint8Array.of(...prefix, ...suffix);
      }
      usedNames.add(nameKey(encodedName));
    }
    bytes.set(encodedName, offset);
    bytes[offset + 11] = type.charCodeAt(0);
    view.setUint32(offset + 12, displacement, true);
    bytes.set([width, decimals, 2], offset + 16);
    displacement += width;
  });
  bytes[headerLength - 264] = 0x0d;
  rows.forEach((row, index) => {
    let offset = headerLength + index * recordLength + 1;
    for (const field of fields) {
      const value = row[field.index];
      const { width, type, decimals } = field;
      if (type === 'L') bytes[offset] = value == null ? 63 : value ? 84 : 70;
      else if (type === 'B') view.setFloat64(offset, value ?? 0, true);
      else if (type === 'N') {
        const decimal = fixedDbfNumber(value ?? 0, decimals);
        if (decimal === undefined || decimal.length > width) {
          throw failure('INVALID_VALUE', `DBF numeric value does not fit field ${field.name}`);
        }
        bytes.set(ascii(decimal.padStart(width)), offset);
      } else if (type === 'D') {
        bytes.set(ascii(formatDbfDate(value, date1904)), offset);
      } else {
        bytes.fill(32, offset, offset + width);
        bytes.set(encodeFieldText(String(value ?? ''), width, encode, codec?.encode !== undefined), offset);
      }
      offset += width;
    }
  });
  bytes[bytes.length - 1] = 0x1a;
  return bytes;
}

const versions = new Set([0x02, 0x03, 0x30, 0x31, 0x83, 0x8b, 0x8c, 0xf5]);
const driverPages = new Map([...codepages].map(([page, driver]) => [driver, page]));
for (const [driver, page] of [
  [0x04, 10000], [0x57, 1252], [0x58, 1252], [0x59, 1252], [0x96, 10007],
  [0x97, 10029], [0x98, 10006], [0xcc, 1257],
]) driverPages.set(driver, page);

function recordLayout(bytes) {
  if (!versions.has(bytes[0]) || bytes.length < 12) return undefined;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const old = bytes[0] === 0x02;
  const level7 = bytes[0] === 0x8c;
  const start = old ? 8 : level7 ? 68 : 32;
  const header = old ? 0x209 : view.getUint16(8, true);
  const record = view.getUint16(old ? 6 : 10, true);
  if (header <= start || header > bytes.length || !record) return undefined;
  const end = start + Math.floor((header - start - 1) / (old ? 16 : level7 ? 48 : 32)) *
    (old ? 16 : level7 ? 48 : 32);
  const stride = old ? 16 : level7 ? 48 : 32;
  for (let marker = start; marker <= end; marker += stride) {
    if (bytes[marker] === 0x0d) return { view, start, header, record, marker, stride, old, level7 };
  }
  return undefined;
}

export function isDbf(bytes) {
  return recordLayout(bytes) !== undefined;
}

const asciiText = bytes => String.fromCharCode(...bytes);

function readField(data, field, version, decode) {
  const text = () => decode(data);
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const sized = size => {
    if (data.length !== size) throw failure('INVALID_INPUT', 'Invalid DBF binary field width');
  };
  switch (field.type) {
    case 'C': return text().trimEnd() || undefined;
    case 'D': {
      const value = text();
      if (!/^\d{8}$/.test(value) || value === '00000000') return undefined;
      const year = Number(value.slice(0, 4));
      const month = Number(value.slice(4, 6)) - 1;
      const day = Number(value.slice(6));
      const date = new Date(0);
      date.setUTCFullYear(year, month, day);
      return date.getUTCFullYear() === year && date.getUTCMonth() === month && date.getUTCDate() === day
        ? date : undefined;
    }
    case 'F': return parseFloat(text());
    case 'N': {
      const value = text().replaceAll('\0', '').trim();
      return !value || value === '.' ? undefined : Number(value) || 0;
    }
    case 'L': {
      const flag = text().trim().toUpperCase();
      if (flag === 'T' || flag === 'Y') return true;
      if (flag === 'F' || flag === 'N') return false;
      if (flag === '' || flag === '?' || flag === '\0') return undefined;
      throw failure('INVALID_INPUT', 'Invalid DBF logical value');
    }
    case '+':
    case 'I':
      sized(4);
      return version === 0x8c ? view.getInt32(0, false) ^ 0x80000000 : view.getInt32(0, true);
    case 'B':
      return (version === 0x30 || version === 0x31) && data.length === 8 ? view.getFloat64(0, true) : undefined;
    case 'Y':
      sized(8);
      return Number(view.getBigInt64(0, true)) / 10_000;
    case 'O':
      sized(8);
      return -view.getFloat64(0, false);
    case '@':
      sized(8);
      return new Date(view.getFloat64(0, false) - 0x388317533400);
    case 'T': {
      sized(8);
      const day = view.getUint32(0, true);
      const time = view.getUint32(4, true);
      return day || time ? new Date((day - 0x253d8c) * 86_400_000 + time) : undefined;
    }
    case 'M':
      if (version === 0x8c) return '##MEMO##' + parseInt(text(), 10);
      if (data.length === 4) return '##MEMO##' + view.getUint32(0, true);
      return '##MEMO##' + parseInt(text(), 10);
    case 'G':
    case 'P':
    case '0': return undefined;
    default: throw failure('UNSUPPORTED_FORMAT', `Unsupported DBF field type ${field.type}`);
  }
}

export function readDbf(bytes, options, decodeCodepage) {
  const { view, start, header, record, marker, stride, old, level7 } = recordLayout(bytes);
  const count = old ? view.getUint16(1, true) : view.getUint32(4, true);
  if (count > Math.floor((bytes.length - header) / record)) {
    throw failure('INVALID_INPUT', 'Truncated DBF records');
  }
  const page = (!old && driverPages.get(bytes[29])) || options.codepage || 1252;
  const decode = content => content.every(byte => byte < 128) ? asciiText(content) : decodeCodepage(content, page);
  const fields = [];
  const nameLength = level7 ? 32 : 11;
  let expectedWidth = 1;
  for (let offset = start; offset < marker; offset += stride) {
    const name = decode(bytes.subarray(offset, offset + nameLength)).split(/[\0\r\n]/, 1)[0];
    const base = offset + nameLength;
    const type = String.fromCharCode(bytes[base]);
    const len = bytes[base + (old || level7 ? 1 : 5)];
    const dec = bytes[base + (old ? 4 : level7 ? 2 : 6)];
    const field = { name, type, len, dec };
    if (!level7) field.offset = old ? view.getUint16(base + 2, true) : view.getUint32(base + 1, true);
    fields.push(field);
    expectedWidth += len;
  }
  if (expectedWidth > record) throw failure('INVALID_INPUT', 'DBF fields exceed the record width');

  const rows = [fields.map(field => field.name)];
  const dateFormat = options.dateNF || 'yyyymmdd';
  const maxRows = Math.min(options.sheetRows || 1_048_576, 1_048_576);
  for (let index = 0; index < count && rows.length < maxRows; index += 1) {
    let offset = header + index * record;
    if (bytes[offset++] === 0x2a) continue;
    rows.push(fields.map(field => {
      const data = bytes.subarray(offset, offset + field.len);
      offset += field.len;
      const value = readField(data, field, bytes[0], decode);
      if (!(value instanceof Date)) return value;
      const number = serialFromDate(value.getTime(), false);
      const cell = options.cellDates ? { t: 'd', v: value, z: dateFormat } : { t: 'n', v: number, z: dateFormat };
      if (options.cellText !== false) cell.w = format(dateFormat, number);
      return cell;
    }));
  }
  const sheet = aoa_to_sheet(rows, options);
  sourceCodepages.set(sheet, page);
  sheet['!cols'] = fields.map(field => ({ wch: field.len, DBF: field }));
  const workbook = book_new(sheet);
  workbook.bookType = 'dbf';
  return workbook;
}
