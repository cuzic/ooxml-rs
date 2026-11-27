import { biffBuiltInNames, decorateBinaryCell } from './sheetjs-binary-records.js';
import { biffFormula } from './sheetjs-biff-formula.js';
import { xlsStream } from './sheetjs-xls-write.js';
import { failure, rustCall } from './contract.js';
import { encode_cell, encode_range } from './sheetjs-address.js';
import { get_table } from './sheetjs-ssf.js';
import { compoundWorkbookProjectionSettings } from './sheetjs-options.generated.js';
import { read_biff_shared_strings, write_biff_property_streams } from './runtime.js';

let compoundFormat;
export function setCompoundFormat(api) {
  compoundFormat = api;
}

export function writeXls(workbook, version, options, properties) {
  if (!compoundFormat) throw failure('UNSUPPORTED_ENVIRONMENT', 'XLS export requires the CFB adapter');
  const container = compoundFormat.utils.cfb_new({ root: 'R' });
  compoundFormat.utils.cfb_add(container, version === 8 ? 'Workbook' : 'Book', xlsStream(workbook, version, options));
  if (properties?.length) {
    const streams = rustCall('WORKBOOK_SAVE_REFUSED', () => write_biff_property_streams(properties));
    const summarySize = new DataView(streams.buffer, streams.byteOffset, 4).getUint32(0, true);
    const end = 4 + summarySize;
    if (summarySize) compoundFormat.utils.cfb_add(container, '\u0005SummaryInformation', streams.slice(4, end));
    if (streams.length > end) compoundFormat.utils.cfb_add(container, '\u0005DocumentSummaryInformation', streams.slice(end));
  }
  return Uint8Array.from(compoundFormat.write(container, { type: 'array' }));
}

export function compoundWorkbook(bytes, raw = false) {
  if (!compoundFormat) return undefined;
  const container = compoundFormat.read(bytes.slice(), { type: 'array', raw });
  return compoundStream(container) ? container : undefined;
}

export function compoundStream(container) {
  return compoundPart(container, ['workbook', 'book']);
}

export function compoundPart(container, names) {
  if (!Array.isArray(container?.FullPaths) || !Array.isArray(container.FileIndex)) return undefined;
  const root = container.FullPaths[0];
  if (typeof root !== 'string' || !root.endsWith('/')) return undefined;
  const index = container.FullPaths.findIndex(name => {
    if (typeof name !== 'string' || !name.startsWith(root)) return false;
    return names.includes(name.slice(root.length).toLowerCase());
  });
  return index < 0 ? undefined : container.FileIndex[index]?.content;
}

function getRecord(bytes, offset) {
  if (offset + 4 > bytes.length) throw failure('INVALID_INPUT', 'Truncated XLS record header');
  const view = new DataView(bytes.buffer, bytes.byteOffset + offset);
  const type = view.getUint16(0, true);
  const length = view.getUint16(2, true);
  const end = offset + 4 + length;
  if (end > bytes.length) throw failure('INVALID_INPUT', 'Truncated XLS record');
  const data = bytes.subarray(offset + 4, end);
  return { type, data, view: new DataView(data.buffer, data.byteOffset, data.byteLength), end };
}

function unicode(data, offset, count, wide) {
  const width = wide ? 2 : 1;
  if (offset + count * width > data.length) throw failure('INVALID_INPUT', 'Truncated XLS string');
  const chars = [];
  for (let index = 0; index < count; index += 1) {
    chars.push(String.fromCharCode(data[offset] | (wide ? data[offset + 1] << 8 : 0)));
    offset += width;
  }
  return chars.join('');
}

function readSharedStrings(records) {
  const units = rustCall('INVALID_INPUT', () => read_biff_shared_strings(records));
  const strings = [];
  for (let offset = 0; offset < units.length;) {
    const length = units[offset++];
    const end = offset + length;
    if (length <= 4096) {
      strings.push(String.fromCharCode(...units.subarray(offset, end)));
      offset = end;
      continue;
    }
    const chunks = [];
    while (offset < end) {
      const next = Math.min(offset + 4096, end);
      chunks.push(String.fromCharCode(...units.subarray(offset, next)));
      offset = next;
    }
    strings.push(chunks.join(''));
  }
  return strings;
}

function rk(raw) {
  let value;
  if (raw & 2) value = raw >> 2;
  else {
    const bits = new DataView(new ArrayBuffer(8));
    bits.setInt32(4, raw & -4, true);
    value = bits.getFloat64(0, true);
  }
  return raw & 1 ? value / 100 : value;
}

function decorate(cell, code, date1904, options) {
  decorateBinaryCell(cell, code, date1904, options);
  if (options.cellNF) cell.z = code;
  return cell;
}

function worksheet(bytes, offset, state, options, sheetIndex) {
  const resolveName = index => state.resolveName(index, sheetIndex);
  if (getRecord(bytes, offset).type !== 0x0809) throw failure('INVALID_INPUT', 'Invalid XLS worksheet offset');
  const grid = options.dense ? [] : undefined;
  const sheet = grid ? { '!data': grid } : {};
  let range;
  let pending;
  const formulaGroups = new Map();
  const formulaFollowers = [];
  let ended = false;
  function add(row, column, xf, cell) {
    if (options.sheetRows && row >= options.sheetRows) return;
    const id = state.xfs[xf] ?? 0;
    decorate(cell, state.formats.get(id) ?? get_table()[id] ?? 'General', state.date1904, options);
    if (grid) (grid[row] ??= [])[column] = cell;
    else sheet[encode_cell({ r: row, c: column })] = cell;
    range ??= { s: { r: row, c: column }, e: { r: row, c: column } };
    range.s.r = Math.min(range.s.r, row);
    range.s.c = Math.min(range.s.c, column);
    range.e.r = Math.max(range.e.r, row);
    range.e.c = Math.max(range.e.c, column);
  }
  for (; offset < bytes.length;) {
    const { type, data, view, end } = getRecord(bytes, offset);
    offset = end;
    if (type === 0x0a) {
      ended = true;
      break;
    }
    if (type === 0x00e5) {
      const count = view.getUint16(0, true);
      if (data.length < 2 + count * 8) throw failure('INVALID_INPUT', 'Truncated XLS merge ranges');
      const merges = sheet['!merges'] ??= [];
      for (let index = 0; index < count; index += 1) {
        const start = 2 + index * 8;
        const [top, bottom, left, right] = [0, 2, 4, 6].map(offset => view.getUint16(start + offset, true));
        merges.push({ s: { r: top, c: left }, e: { r: bottom, c: right } });
      }
      continue;
    }
    if (type === 0x007d && options.cellStyles) {
      const first = view.getUint16(0, true);
      const last = Math.min(255, view.getUint16(2, true));
      const width = view.getUint16(4, true) / 256;
      const flags = view.getUint16(8, true);
      const columns = sheet['!cols'] ??= [];
      for (let index = first; index <= last; index += 1) {
        columns[index] = { width, hidden: Boolean(flags & 1), level: (flags >> 8) & 7 };
      }
      continue;
    }
    if (type === 0x0208) {
      const index = view.getUint16(0, true);
      const flags = view.getUint8(12);
      const row = {};
      if (flags & 7) row.level = flags & 7;
      if (flags & 0x20) row.hidden = true;
      if (flags & 0x40) {
        row.hpt = (view.getUint16(6, true) & 0x7fff) / 20;
        row.hpx = row.hpt * 4 / 3;
      }
      if (Object.keys(row).length && (!options.sheetRows || index < options.sheetRows)) {
        (sheet['!rows'] ??= [])[index] = row;
      }
      continue;
    }
    if (type === 0x04bc || type === 0x0221 || type === 0x0021) {
      if (options.cellFormula === false) continue;
      const array = type !== 0x04bc;
      const lengthOffset = array ? 12 : 8;
      if (data.length >= lengthOffset + 2) {
        const length = view.getUint16(lengthOffset, true);
        const start = lengthOffset + 2;
        if (data.length >= start + length) {
          const s = { r: view.getUint16(0, true), c: data[4] };
          const e = { r: view.getUint16(2, true), c: data[5] };
          formulaGroups.set(encode_cell(s), { s, e, array, tokens: data.subarray(start, start + length) });
        }
      }
      continue;
    }
    if (type === 0x0207 && pending) {
      const count = view.getUint16(0, true);
      const text = state.version === 8 ? unicode(data, 3, count, data[2] & 1)
        : state.decode(data.subarray(2, 2 + count));
      const { address, formula } = pending;
      add(...address, { t: 's', v: text, ...(formula ? { f: formula } : {}) });
      pending = undefined;
      continue;
    }
    if (![0x0203, 0x0205, 0x0204, 0x0201, 0x00fd, 0x027e, 0x00bd, 6].includes(type)) continue;
    if (data.length < 6) throw failure('INVALID_INPUT', 'Invalid XLS cell record');
    const row = view.getUint16(0, true);
    const col = view.getUint16(2, true);
    const xf = view.getUint16(4, true);
    if (type === 0x00bd) {
      for (let at = 4, column = col; at + 6 <= data.length - 2; at += 6, column += 1) {
        add(row, column, view.getUint16(at, true), { t: 'n', v: rk(view.getInt32(at + 2, true)) });
      }
      continue;
    }
    let cell;
    if (type === 0x0203) cell = { t: 'n', v: view.getFloat64(6, true) };
    else if (type === 0x027e) cell = { t: 'n', v: rk(view.getInt32(6, true)) };
    else if (type === 0x0205) cell = view.getUint8(7) ? { t: 'e', v: data[6] } : { t: 'b', v: Boolean(data[6]) };
    else if (type === 0xfd) {
      const text = state.strings[view.getUint32(6, true)];
      if (text === undefined) throw failure('INVALID_INPUT', 'Invalid XLS shared string index');
      cell = { t: 's', v: text };
    } else if (type === 0x0204) {
      const count = view.getUint16(6, true);
      const text = state.version === 8 ? unicode(data, 9, count, data[8] & 1)
        : state.decode(data.subarray(8, 8 + count));
      cell = { t: 's', v: text };
    } else if (type === 6) {
      let formula;
      if (options.cellFormula !== false && data.length >= 22) {
        const count = view.getUint16(20, true);
        if (22 + count <= data.length) {
          if (count === 5 && data[22] === 1) {
            const anchor = encode_cell({ r: view.getUint16(23, true), c: view.getUint16(25, true) });
            formulaFollowers.push({ row, col, anchor });
          } else {
            formula = biffFormula(data.subarray(22, 22 + count), state.version, state.decode, undefined, state.resolveSheet, resolveName);
          }
        }
      }
      if (view.getUint16(12, true) !== 0xffff) cell = { t: 'n', v: view.getFloat64(6, true) };
      else if (data[6] === 0) pending = { address: [row, col, xf], formula };
      else if (data[6] === 1) cell = { t: 'b', v: Boolean(data[8]) };
      else if (data[6] === 2) cell = { t: 'e', v: data[8] };
      else cell = { t: 's', v: '' };
      if (cell && formula) cell.f = formula;
    } else if (options.sheetStubs) cell = { t: 'z' };
    if (cell) add(row, col, xf, cell);
  }
  if (!ended) throw failure('INVALID_INPUT', 'Missing XLS worksheet end record');
  for (const { row, col, anchor } of formulaFollowers) {
    const group = formulaGroups.get(anchor);
    const cell = grid ? grid[row]?.[col] : sheet[encode_cell({ r: row, c: col })];
    if (!group || !cell || row < group.s.r || row > group.e.r || col < group.s.c || col > group.e.c) continue;
    if (group.array) {
      cell.F = encode_range(group);
      if (row !== group.s.r || col !== group.s.c) continue;
    }
    const formula = biffFormula(group.tokens, state.version, state.decode, { row, column: col }, state.resolveSheet, resolveName);
    if (formula) cell.f = formula;
  }
  if (range) sheet['!ref'] = encode_range(range);
  return sheet;
}

function definedName(data, view, version, decode) {
  const flags = view.getUint16(0, true);
  const count = view.getUint8(3);
  const length = view.getUint16(4, true);
  const sheet = view.getUint16(version === 8 ? 8 : 6, true);
  const wide = version === 8 && Boolean(view.getUint8(14) & 1);
  const start = version === 8 ? 15 : 14;
  const end = start + count * (wide ? 2 : 1);
  if (end + length > data.length) throw failure('INVALID_INPUT', 'Truncated XLS defined name');
  const text = version === 8 ? unicode(data, start, count, wide) : decode(data.subarray(start, end));
  const builtIn = flags & 0x20 ? biffBuiltInNames[text.charCodeAt(0)] : undefined;
  if ((flags & 0x20) && builtIn === undefined) return undefined;
  return { name: builtIn === undefined ? text : '_xlnm.' + builtIn, sheet, tokens: data.subarray(end, end + length) };
}

function definedNameComment(data, view, label) {
  if (view.getUint16(0, true) !== 0x0894) return undefined;
  const nameLength = view.getUint16(12, true);
  const commentLength = view.getUint16(14, true);
  if (nameLength > 255 || commentLength > 255) return undefined;
  const wide = Boolean(view.getUint8(16) & 1);
  const name = unicode(data, 17, nameLength, wide);
  if (name.toLowerCase() !== label.toLowerCase()) return undefined;
  const end = 17 + nameLength * (wide ? 2 : 1);
  return unicode(data, end + 1, commentLength, Boolean(view.getUint8(end) & 1));
}

function filterDatabase(tokens) {
  if (tokens.length !== 11 || ![0x3b, 0x5b, 0x7b].includes(tokens[0])) return undefined;
  const view = new DataView(tokens.buffer, tokens.byteOffset, tokens.byteLength);
  const start = { r: view.getUint16(3, true), c: view.getUint16(7, true) & 0x3fff };
  const end = { r: view.getUint16(5, true), c: view.getUint16(9, true) & 0x3fff };
  if (start.r > end.r || start.c > end.c || end.c > 255) return undefined;
  return encode_range(start, end);
}

export function readXls(bytes, options, decodeCodepage) {
  const bof = getRecord(bytes, 0);
  const marker = bof.view.getUint16(0, true);
  const version = marker === 0x0600 ? 8 : marker === 0x0500 ? 5 : 0;
  if (bof.type !== 0x0809 || ![5, 8].includes(version)) {
    throw failure('UNSUPPORTED_FORMAT', 'Expected an Excel BIFF5 or BIFF8 workbook stream');
  }
  let codepage = options.codepage || 1252;
  const decode = data => decodeCodepage(data, codepage);
  const sheets = [];
  const state = { version, decode, xfs: [], formats: new Map(), strings: [], date1904: false };
  const labels = [];
  let lastLabel;
  const supbooks = [];
  const references = [];
  const quote = name => /^[A-Za-z_][A-Za-z0-9_.]*$/.test(name) ? name : "'" + name.replaceAll("'", "''") + "'";
  state.resolveName = (index, currentSheet) => {
    const { name, sheet } = labels[index - 1] ?? {};
    if (!name) return 'SH33TJSNAME' + index;
    const text = name.startsWith('_xlfn.') ? name.slice(6) : name;
    const scopedName = sheets[sheet - 1]?.name;
    return scopedName !== undefined && sheet - 1 !== currentSheet ? quote(scopedName) + '!' + text : text;
  };
  state.resolveSheet = index => {
    const item = references[index];
    if (!item || supbooks[item[0]] !== 0x0401) return undefined;
    const first = sheets[item[1]]?.name;
    const last = sheets[item[2]]?.name;
    if (first === undefined || last === undefined) return undefined;
    return item[1] === item[2] ? quote(first) : quote(first + ':' + last);
  };

  try {
    for (let offset = bof.end; offset < bytes.length;) {
      const start = offset;
      const { type, data, view, end } = getRecord(bytes, offset);
      offset = end;
      if (type === 0x0a) break;
      if (type === 0x002f) throw failure('WORKBOOK_LOAD_REFUSED', 'Encrypted BIFF workbooks require decryption');
      if (type === 0x0042) {
        const requested = view.getUint16(0, true);
        if (requested !== 1200) codepage = requested === 32769 ? 1252 : requested === 32768 ? 10000 : requested;
      }
      if (type === 0x0022) state.date1904 = view.getUint16(0, true) !== 0;
      if (type === 0x0085) {
        const name = version === 8 ? unicode(data, 8, data[6], data[7] & 1) : decode(data.subarray(7, 7 + data[6]));
        sheets.push({ name, offset: view.getUint32(0, true), Hidden: data[4] & 3 });
      }
      if (options.bookSheets && !options.WTF) continue;
      if (type === 0x00e0) state.xfs.push(view.getUint16(2, true));
      if (type === 0x041e) {
        const id = view.getUint16(0, true);
        const text = version === 8 ? unicode(data, 5, view.getUint16(2, true), data[4] & 1) : decode(data.subarray(3, 3 + data[2]));
        state.formats.set(id, text);
      }
      if (!options.bookSheets && (type === 0x0018 || type === 0x0218)) {
        lastLabel = definedName(data, view, version, decode);
        labels.push(lastLabel);
      }
      if (!options.bookSheets && version === 8 && type === 0x0894 && lastLabel) {
        const comment = definedNameComment(data, view, lastLabel.name);
        if (comment !== undefined) lastLabel.comment = comment;
      }
      if (version === 8 && type === 0x01ae) supbooks.push(view.getUint16(2, true));
      if (version === 8 && type === 0x0017) {
        const count = view.getUint16(0, true);
        if (data.length === 2 + count * 6) {
          for (let index = 0; index < count; index += 1) {
            const at = 2 + index * 6;
            references.push([view.getUint16(at, true), view.getUint16(at + 2, true), view.getUint16(at + 4, true)]);
          }
        }
      }
      if (type === 0x00fc) {
        while (offset + 4 <= bytes.length && bytes[offset] === 0x3c && bytes[offset + 1] === 0) {
          const continuation = getRecord(bytes, offset);
          offset = continuation.end;
        }
        state.strings = readSharedStrings(bytes.subarray(start, offset));
      }
    }
    const names = sheets.map(sheet => sheet.name);
    const workbook = { SheetNames: names, Workbook: { Sheets: sheets.map(({ name, Hidden }) => ({ name, Hidden })),
      WBProps: { date1904: state.date1904 } } };
    if (options.bookSheets) return workbook;
    const definedNames = [];
    const filters = new Map();
    for (const label of labels) {
      if (!label) continue;
      const { name, sheet, tokens, comment } = label;
      if (version === 8 && sheet > 0 && name === '_xlnm._FilterDatabase') {
        const filter = filterDatabase(tokens);
        if (filter !== undefined) filters.set(sheet - 1, filter);
      }
      const ref = tokens.length ? biffFormula(tokens, version, decode, undefined, state.resolveSheet, index => state.resolveName(index, sheet - 1)) : '';
      if (ref === undefined) continue;
      const numeric = tokens[0] === 0x1e && tokens.length === 3;
      const entry = { Name: name, Ref: numeric ? Number(ref) : ref };
      if (sheet > 0) entry.Sheet = sheet - 1;
      if (comment !== undefined) entry.Comment = comment;
      definedNames.push(entry);
    }
    if (definedNames.length) workbook.Workbook.Names = definedNames;
    workbook.Sheets = Object.create(null);
    const { selectSheet } = compoundWorkbookProjectionSettings(options);
    sheets.forEach(({ name, offset }, index) => {
      if (selectSheet(name, index)) {
        const projected = worksheet(bytes, offset, state, options, index);
        if (filters.has(index)) projected['!autofilter'] = { ref: filters.get(index) };
        workbook.Sheets[name] = projected;
      }
    });
    return workbook;
  } catch (error) {
    if (error instanceof RangeError) throw failure('INVALID_INPUT', 'Truncated XLS record data', error);
    throw error;
  }
}
