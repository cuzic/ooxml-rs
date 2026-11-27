import { write_biff_shared_strings } from './runtime.js';
import { compileBiffFormula, compileBinaryNames } from './sheetjs-biff-formula.js';
import { biffBuiltInNames, record, joinRecords, packText } from './sheetjs-binary-records.js';
import { failure, rustCall } from './contract.js';
import { decode_range, encode_cell } from './sheetjs-address.js';
import { autofilterRange } from './sheetjs-print-settings.js';
import { serialFromDate } from './sheetjs-date.js';

function putText(view, offset, text, wide) {
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index);
    if (wide) view.setUint16(offset + index * 2, code, true);
    else view.setUint8(offset + index, code < 128 ? code : 95);
  }
}

function bof(version, type) {
  return record(0x0809, version === 8 ? 16 : 8, view => {
    view.setUint16(4, version === 8 ? 0x0600 : 0x0500, true);
    view.setUint16(6, type, true);
    view.setUint16(8, 0x7262, true);
    view.setUint16(10, 0x07cd, true);
  });
}

function layoutRecords(sheet) {
  const columns = [];
  const rows = [];
  sheet['!cols']?.slice(0, 256).forEach((column, index) => {
    if (!column) return;
    const digits = column.MDW || 7;
    const pixels = column.wpx ?? (column.wch == null ? undefined : column.wch * digits + 5);
    const width = column.width ?? (pixels === undefined ? 0 : pixels / digits);
    columns.push(record(0x007d, 12, view => {
      view.setUint16(4, index, true);
      view.setUint16(6, index, true);
      view.setUint16(8, Math.max(0, Math.min(65535, Math.round(width * 256))), true);
      view.setUint8(12, Number(Boolean(column.hidden)));
      view.setUint8(13, (column.level ?? 0) & 7);
    }));
  });
  sheet['!rows']?.slice(0, 65536).forEach((row, index) => {
    if (!row) return;
    const height = row.hpt ?? (row.hpx === undefined ? undefined : row.hpx * 0.75);
    rows.push(record(0x0208, 16, view => {
      view.setUint16(4, index, true);
      view.setUint16(10, height === undefined ? 255 : Math.max(0, Math.min(0x7fff, Math.round(height * 20))), true);
      view.setUint8(16, ((row.level ?? 0) & 7) | (row.hidden ? 0x20 : 0) | (height !== undefined ? 0x40 : 0));
    }));
  });
  return [columns, rows];
}

function mergeRecords(ranges) {
  const records = [];
  for (let index = 0; index < ranges.length; index += 1027) {
    const chunk = ranges.slice(index, index + 1027);
    records.push(record(0x00e5, 2 + chunk.length * 8, view => {
      view.setUint16(4, chunk.length, true);
      chunk.forEach(({ s, e }, item) => {
        const offset = 6 + item * 8;
        if (s.r < 0 || s.c < 0 || e.r < s.r || e.c < s.c || e.r > 65535 || e.c > 255) {
          throw failure('INVALID_INPUT', 'Merged cells exceed XLS limits');
        }
        [s.r, e.r, s.c, e.c].forEach((value, key) => view.setUint16(offset + key * 2, value, true));
      });
    }));
  }
  return records;
}

function arrayFormulaRecord({ range: { s, e }, tokens }) {
  return record(0x0221, 14 + tokens.length, (view, bytes) => {
    view.setUint16(4, s.r, true);
    view.setUint16(6, e.r, true);
    view.setUint8(8, s.c);
    view.setUint8(9, e.c);
    view.setUint16(16, tokens.length, true);
    bytes.set(tokens, 18);
  });
}

function labelRecord(name, sheet, tokens, builtIn = -1, hidden = false) {
  const length = builtIn < 0 ? name.length : 1;
  const wide = builtIn < 0;
  const nameBytes = length * (wide ? 2 : 1);
  return record(0x0018, 15 + nameBytes + tokens.length, (view, bytes) => {
    view.setUint16(4, (wide ? 0 : 0x20) | Number(hidden), true);
    bytes[7] = length;
    view.setUint16(8, tokens.length, true);
    view.setUint16(12, sheet === undefined ? 0 : sheet + 1, true);
    bytes[18] = Number(wide);
    if (wide) putText(view, 19, name, true);
    else bytes[19] = builtIn;
    bytes.set(tokens, 19 + nameBytes);
  });
}

function filterLabel(sheet, reference, { s, e }) {
  const tokens = new Uint8Array(11);
  tokens[0] = 0x3b; // PtgArea3d
  const view = new DataView(tokens.buffer);
  view.setUint16(1, reference, true);
  view.setUint16(3, s.r, true);
  view.setUint16(5, e.r, true);
  view.setUint16(7, s.c, true);
  view.setUint16(9, e.c, true);
  return labelRecord('', sheet, tokens, 0x0d, true);
}

function nameCommentRecord(name, comment) {
  return record(0x0894, 18 + 2 * (name.length + comment.length), (view, bytes) => {
    view.setUint16(4, 0x0894, true);
    view.setUint16(16, name.length, true);
    view.setUint16(18, comment.length, true);
    bytes[20] = 1;
    putText(view, 21, name, true);
    const end = 21 + name.length * 2;
    bytes[end] = 1;
    putText(view, end + 1, comment, true);
  });
}

function definedNameRecords(names, sheetCount, sheetIndex, filterSheets, registerName, resolveName, strict) {
  if (names == null) return [];
  if (!Array.isArray(names)) throw failure('INVALID_VALUE', 'Expected a list of defined names');
  const candidates = [];
  for (const { Name: name, Ref: ref, Sheet: sheet, Comment: comment } of names) {
    if (typeof name !== 'string' || !name.length || name.length > 255 ||
        (sheet !== undefined && (!Number.isInteger(sheet) || sheet < 0 || sheet >= sheetCount || sheet >= 65535))) {
      throw failure('INVALID_VALUE', 'Invalid BIFF8 defined name or worksheet scope');
    }
    if (comment !== undefined && (typeof comment !== 'string' || comment.length > 255)) {
      throw failure('INVALID_VALUE', 'Expected a BIFF8 defined name comment of at most 255 characters');
    }
    if (name.toLowerCase() === '_xlnm._filterdatabase' && filterSheets.has(sheet)) {
      if (comment !== undefined) {
        filterSheets.set(sheet, joinRecords([filterSheets.get(sheet), nameCommentRecord(name, comment)]));
      }
      continue;
    }
    const candidate = { name, sheet, ref, comment, dependencies: [], dependents: [] };
    registerName(name, sheet, candidate);
    candidates.push(candidate);
  }
  const included = compileBinaryNames(candidates, 8, sheetIndex, resolveName,
    candidate => 8224 - 15 - candidate.name.length * 2, strict ? 'Unsupported BIFF8 defined name formula' : undefined);
  const labels = [];
  for (const { name, sheet, comment, tokens } of included) {
    const builtIn = name.startsWith('_xlnm.') ? biffBuiltInNames.indexOf(name.slice(6)) : -1;
    labels.push(labelRecord(name, sheet, tokens, builtIn));
    if (comment !== undefined) labels.push(nameCommentRecord(name, comment));
  }
  return labels;
}

export function xlsStream(workbook, version, options) {
  const wide = version === 8;
  const date1904 = Boolean(workbook.Workbook?.WBProps?.date1904);
  const formats = new Map([['General', 0]]);
  const strings = new Map();
  let stringCount = 0;
  const sheets = [];
  const filters = new Map();
  const namesByIndex = new Map(workbook.SheetNames.map((name, index) => [name.toLowerCase(), index]));
  const referenceIndex = new Map();
  const sheetIndex = name => {
    const index = namesByIndex.get(name.toLowerCase());
    if (index === undefined) return undefined;
    if (!referenceIndex.has(index)) referenceIndex.set(index, referenceIndex.size);
    return referenceIndex.get(index);
  };

  const inputs = workbook.SheetNames.map((name, index) => {
    const sheet = workbook.Sheets?.[name];
    if (!sheet) throw failure('SHEET_READ_REFUSED', `Worksheet is not loaded: ${name}`);
    const range = decode_range(sheet['!ref'] || 'A1');
    const lastRow = Math.min(range.e.r, wide ? 65535 : 16383);
    const lastColumn = Math.min(range.e.c, 255);
    if (options.WTF && (lastRow !== range.e.r || lastColumn !== range.e.c)) {
      throw failure('INVALID_INPUT', 'Worksheet exceeds XLS limits');
    }
    return { name, index, sheet, range, lastRow, lastColumn };
  });
  const filterRanges = new Map();
  const nameIndices = new Map();
  const registerName = (name, sheet, entry) => {
    const scope = sheet ?? -1;
    if (!nameIndices.has(scope)) nameIndices.set(scope, new Map());
    const lookup = nameIndices.get(scope);
    const key = name.toLowerCase();
    if (lookup.has(key)) throw failure('INVALID_VALUE', 'Duplicate BIFF8 defined name in the same scope');
    lookup.set(key, entry);
  };
  const resolveName = (name, sheet, qualifiedSheet) => {
    const key = name.toLowerCase();
    if (qualifiedSheet !== undefined) {
      return nameIndices.get(namesByIndex.get(qualifiedSheet.toLowerCase()))?.get(key);
    }
    return nameIndices.get(sheet)?.get(key) ?? nameIndices.get(-1)?.get(key);
  };
  if (wide) {
    for (const { name, index, sheet, lastRow } of inputs) {
      const filter = autofilterRange(sheet, lastRow);
      if (!filter) continue;
      if (filter.e.r > 65535 || filter.e.c > 255 || index >= 65535) {
        throw failure('INVALID_VALUE', 'Autofilter exceeds BIFF8 limits');
      }
      filterRanges.set(index, filter);
      filters.set(index, filterLabel(index, sheetIndex(name), filter));
    }
  }
  const nameLabels = wide
    ? definedNameRecords(workbook.Workbook?.Names, inputs.length, sheetIndex, filters, registerName, resolveName, options.WTF)
    : [];

  for (const { name, index, sheet, range, lastRow, lastColumn } of inputs) {
    const resolveCellName = wide ? (value, qualifiedSheet) => resolveName(value, index, qualifiedSheet)?.index : undefined;
    const [columns, rows] = wide ? layoutRecords(sheet) : [[], []];
    const records = [bof(version, 0x10), ...columns];
    const filter = filterRanges.get(index);
    if (filter) records.push(record(0x009d, 2, view => view.setUint16(4, filter.e.c - filter.s.c + 1, true)));
    records.push(record(0x0200, wide ? 14 : 10, view => {
      if (wide) {
        view.setUint32(4, range.s.r, true);
        view.setUint32(8, lastRow + 1, true);
      } else {
        view.setUint16(4, range.s.r, true);
        view.setUint16(6, lastRow + 1, true);
      }
      view.setUint16(wide ? 12 : 8, range.s.c, true);
      view.setUint16(wide ? 14 : 10, lastColumn + 1, true);
    }), ...rows);
    const arrays = new Map();
    const getCell = (row, column) => sheet['!data'] ? sheet['!data'][row]?.[column] : sheet[encode_cell({ r: row, c: column })];
    function arrayAt(cell, row, column) {
      const reference = cell.F;
      if (typeof reference !== 'string') return undefined;
      if (!arrays.has(reference)) {
        const bounds = decode_range(reference);
        const { s, e } = bounds;
        const anchor = getCell(s.r, s.c);
        const fits = s.r >= range.s.r && s.c >= range.s.c && s.r <= e.r && s.c <= e.c && e.r <= lastRow && e.c <= lastColumn;
        const tokens = fits && anchor?.F === reference && !anchor.D
          ? compileBiffFormula(anchor.f, version, sheetIndex, resolveCellName) : undefined;
        arrays.set(reference, tokens ? { range: bounds, tokens } : undefined);
      }
      const group = arrays.get(reference);
      if (!group) return undefined;
      const { s, e } = group.range;
      return row >= s.r && row <= e.r && column >= s.c && column <= e.c ? group : undefined;
    }

    for (let row = range.s.r; row <= lastRow; row += 1) {
      for (let column = range.s.c; column <= lastColumn; column += 1) {
        const cell = getCell(row, column);
        if (!cell) continue;
        const code = cell.z ?? (cell.t === 'd' ? 'm/d/yy' : 'General');
        if (!formats.has(code)) formats.set(code, formats.size);
        const xf = formats.get(code);
        const add = (type, length, write) => records.push(record(type, 6 + length, view => {
          view.setUint16(4, row, true);
          view.setUint16(6, column, true);
          view.setUint16(8, xf, true);
          write?.(view);
        }));
        const group = arrayAt(cell, row, column);
        const anchor = group?.range.s;
        const formula = anchor ? Uint8Array.of(1, anchor.r & 255, anchor.r >> 8, anchor.c, 0)
          : compileBiffFormula(cell.f, version, sheetIndex, resolveCellName);
        if (formula) {
          const numeric = cell.t === 'n' || cell.t === 'd';
          const string = (cell.t === 's' || cell.t === 'str') && cell.v != null;
          const value = cell.t === 'd' ? serialFromDate(new Date(cell.v).getTime(), date1904) : cell.v;
          add(6, 16 + formula.length, view => {
            if (numeric && Number.isFinite(value)) view.setFloat64(10, value, true);
            else {
              const marker = string ? 0 : cell.t === 'b' ? 1 : cell.t === 'e' ? 2 : 3;
              view.setUint8(10, marker);
              if (marker === 1 || marker === 2) view.setUint8(12, Number(cell.v));
              view.setUint16(16, 0xffff, true);
            }
            view.setUint16(18, 0x21, true);
            view.setUint16(24, formula.length, true);
            new Uint8Array(view.buffer).set(formula, 26);
          });
          if (string) {
            const text = String(cell.v).slice(0, wide ? 4100 : 255);
            records.push(record(0x0207, (wide ? 3 : 2) + text.length * (wide ? 2 : 1), view => {
              view.setUint16(4, text.length, true);
              if (wide) view.setUint8(6, 1);
              putText(view, wide ? 7 : 6, text, wide);
            }));
          }
          if (group && anchor.r === row && anchor.c === column) records.push(arrayFormulaRecord(group));
          continue;
        }
        if (cell.v == null || cell.t === 'z') {
          add(0x0201, 0);
        } else if (cell.t === 'n' || cell.t === 'd') {
          const value = cell.t === 'd' ? serialFromDate(new Date(cell.v).getTime(), date1904) : Number(cell.v);
          if (Number.isFinite(value)) add(0x0203, 8, view => view.setFloat64(10, value, true));
          else add(0x0205, 2, view => { view.setUint8(10, Number.isNaN(value) ? 36 : 7); view.setUint8(11, 1); });
        } else if (cell.t === 'b' || cell.t === 'e') {
          add(0x0205, 2, view => { view.setUint8(10, Number(cell.v)); view.setUint8(11, Number(cell.t === 'e')); });
        } else {
          const text = String(cell.v).slice(0, wide ? 32767 : 255);
          if (wide && (options.bookSST || text.length > 4100)) {
            if (!strings.has(text)) strings.set(text, strings.size);
            stringCount += 1;
            add(0x00fd, 4, view => view.setUint32(10, strings.get(text), true));
          } else {
            add(0x0204, (wide ? 3 : 2) + text.length * (wide ? 2 : 1), view => {
              view.setUint16(10, text.length, true);
              if (wide) view.setUint8(12, 1);
              putText(view, wide ? 13 : 12, text, wide);
            });
          }
        }
      }
    }
    if (wide) records.push(...mergeRecords(sheet['!merges'] ?? []));
    records.push(record(0x0a, 0));
    sheets.push({ name, Hidden: workbook.Workbook?.Sheets?.[index]?.Hidden ?? 0, data: joinRecords(records) });
  }
  const globals = [bof(version, 5)];
  if (options.bookType === 'xla') globals.push(record(0x0087, 0)); // Addin
  globals.push(record(0x42, 2, view => view.setUint16(4, wide ? 1200 : 1252, true)));
  if (date1904) globals.push(record(0x22, 2, view => view.setUint16(4, 1, true)));
  for (const [code, index] of formats) {
    if (index === 0) continue;
    const text = code.slice(0, 255);
    globals.push(record(0x041e, (wide ? 5 : 3) + text.length * (wide ? 2 : 1), view => {
      view.setUint16(4, 163 + index, true);
      if (wide) {
        view.setUint16(6, text.length, true);
        view.setUint8(8, 1);
      } else view.setUint8(6, text.length);
      putText(view, wide ? 9 : 7, text, wide);
    }));
  }
  for (const index of formats.values()) {
    globals.push(record(0xe0, wide ? 20 : 16, view => view.setUint16(6, index === 0 ? 0 : 163 + index, true)));
  }
  const bounds = sheets.map(({ name, Hidden }) => record(0x85, (wide ? 8 : 7) + name.length * (wide ? 2 : 1), view => {
    view.setUint8(8, Hidden);
    view.setUint8(10, name.length);
    if (wide) view.setUint8(11, 1);
    putText(view, wide ? 12 : 11, name, wide);
  }));
  globals.push(...bounds);
  if (wide && referenceIndex.size) {
    globals.push(record(0x01ae, 4, view => {
      view.setUint16(4, sheets.length, true);
      view.setUint16(6, 0x0401, true);
    }));
    if (referenceIndex.size > 1370) throw failure('INVALID_INPUT', 'Too many sheets for XLS internal references');
    globals.push(record(0x0017, 2 + 6 * referenceIndex.size, view => {
      view.setUint16(4, referenceIndex.size, true);
      for (const [sheet, index] of referenceIndex) {
        view.setUint16(8 + index * 6, sheet, true);
        view.setUint16(10 + index * 6, sheet, true);
      }
    }));
  }

  if (wide) {
    globals.push(...nameLabels, ...filters.values());
    const [lengths, units] = packText([...strings.keys()]);
    globals.push(rustCall('WORKBOOK_SAVE_REFUSED', () => write_biff_shared_strings(stringCount, lengths, units)));
  }
  globals.push(record(0x0a, 0));
  let offset = globals.reduce((sum, bytes) => sum + bytes.length, 0);
  bounds.forEach((bytes, index) => {
    new DataView(bytes.buffer).setUint32(4, offset, true);
    offset += sheets[index].data.length;
  });
  return joinRecords([...globals, ...sheets.map(sheet => sheet.data)]);
}
