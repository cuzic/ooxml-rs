import { codepageTable } from './sheetjs-codepage.js';
import { isJsonCellType, isWorksheetColumn, isWorksheetRow, sheetArrayFormulaSettings, sheetCsvSettings, sheetFormulaSettings, sheetHtmlSettings, sheetJsonSettings, sheetNewSettings } from './sheetjs-options.generated.js';
import { decode_cell, decode_range, encode_cell, encode_col, encode_range, encode_row } from './sheetjs-address.js';
import { format_cell } from './sheetjs-format.js';
import { dateFromSerial, displaySerialFromDate } from './sheetjs-date.js';
import { is_date } from './sheetjs-ssf.js';
import { externalHtmlLink } from './sheetjs-dom.js';
export { format_cell } from './sheetjs-format.js';

export * from './sheetjs-address.js';
export { sheet_add_dom, table_to_sheet, table_to_book } from './sheetjs-dom.js';
export { book_new, book_append_sheet, book_set_sheet_visibility } from './sheetjs-book.js';
export { aoa_to_sheet, sheet_add_aoa, json_to_sheet, sheet_add_json } from './sheetjs-input.js';

function uniqueHeader(value, used) {
  const label = String(value);
  let suffix = used.get(label);
  if (suffix === undefined) {
    used.set(label, 1);
    return label;
  }
  let name;
  do {
    name = label + '_' + suffix;
    suffix += 1;
  } while (used.has(name));
  used.set(label, suffix);
  used.set(name, 1);
  return name;
}

export function jsonRows(sheet, options = {}, { preserveDateSerialAt, initializeDenseRow = true } = {}) {
  if (sheet?.['!ref'] == null) return [][Symbol.iterator]();
  options ??= {};
  const { range: requestedRange, header, skipHidden, defval, blankrows, rawNumbers, raw, UTC } = sheetJsonSettings(options);
  const requested = requestedRange ?? sheet['!ref'];
  const range = typeof requested === 'object'
    ? requested
    : decode_range(typeof requested === 'number' ? sheet['!ref'] : requested);
  const start = typeof requested === 'number' ? requested : range.s.r;
  const array = header === 1;
  const automatic = !array && header !== 'A' && !Array.isArray(header);
  const dense = sheet['!data'];
  // SheetJS treats a missing starting dense row as an empty row when setting up columns.
  if (dense != null && initializeDenseRow) dense[start] ||= [];
  const startingRow = dense == null ? undefined : dense[start] ?? [];
  const headerRowLabel = automatic && dense == null ? encode_row(start) : undefined;

  const columns = [];
  const used = new Map();
  for (let column = range.s.c; column <= range.e.c; column += 1) {
    if (skipHidden && sheet['!cols']?.[column]?.hidden) continue;
    const letters = dense == null || header === 'A' ? encode_col(column) : undefined;
    let key = column - range.s.c;
    if (header === 'A') key = letters;
    if (Array.isArray(header)) key = header[key];
    if (automatic) {
      const cell = dense == null ? sheet[letters + headerRowLabel] : startingRow?.[column];
      key = uniqueHeader(cell == null ? '__EMPTY' : format_cell(cell, cell.v, options), used);
    }
    columns.push({ column, letters, key });
  }

  const includeBlankRows = array ? blankrows !== false : Boolean(blankrows);
  return readRows();

  function* readRows() {
    for (let index = start + Number(automatic); index <= range.e.r; index += 1) {
      if (skipHidden && sheet['!rows']?.[index]?.hidden) continue;
      const denseRow = dense?.[index] ?? (index === start && !initializeDenseRow ? startingRow : undefined);
      if (dense != null && denseRow == null && !includeBlankRows) continue;
      const row = array ? [] : {};
      if (!array) Object.defineProperty(row, '__rowNum__', { value: index });
      let hasValue = false;
      const rowLabel = dense == null ? encode_row(index) : undefined;
      for (const { column, letters, key } of columns) {
        const cell = dense == null ? sheet[letters + rowLabel] : denseRow?.[column];
        const type = cell?.t;
        if (type !== undefined && !isJsonCellType(type)) throw new Error('unrecognized type ' + type);
        if (key == null) continue;
        let value = cell?.v;
        if (cell?.t === 'z' && value != null) continue;
        if (cell?.t === 'e') value = value === 0 ? null : undefined;
        if (cell?.t === undefined) {
          value = dense != null && !denseRow ? undefined : defval;
        } else if (value == null) {
          if (!(cell.t === 'e' && value === null)) {
            value = defval === undefined && raw && value === null ? null : defval;
          }
        } else {
          hasValue = true;
          const useRaw = cell.t === 'n' && typeof rawNumbers === 'boolean' ? rawNumbers : raw;
          const numericDate = cell.t === 'n' && typeof value === 'number' && typeof cell.z === 'string' &&
            !preserveDateSerialAt?.(column) && is_date(cell.z);
          if (numericDate) value = dateFromSerial(value, false) ?? value;
          if (((numericDate && value instanceof Date) || (useRaw && cell.t === 'd')) &&
              !UTC && options.raw !== false) {
            const date = new Date(value);
            value = new Date(date.getTime() + date.getTimezoneOffset() * 60000);
          }
          if (!useRaw) {
            if (numericDate && value instanceof Date) {
              value = displaySerialFromDate(value.getTime(), false);
            }
            value = format_cell(cell, value, options);
          }
        }
        if (value !== undefined) {
          Object.defineProperty(row, key, { value, writable: true, enumerable: true, configurable: true });
        }
      }
      if (hasValue || includeBlankRows) yield row;
    }
  }
}

export function sheet_to_json(sheet, options) {
  return Array.from(jsonRows(sheet, options));
}

export { sheet_to_json as sheet_to_row_object_array };

function quoteCsv(text) {
  return '"' + text.replaceAll('"', '""') + '"';
}

export function csvRows(sheet, settings) {
  const { FS, RS, skipHidden, blankrows, strip, forceQuotes, rawNumbers } = settings;
  const range = decode_range(sheet['!ref']);
  const dense = sheet['!data'];
  const columns = [];
  for (let col = range.s.c; col <= range.e.c; col += 1) {
    if (!skipHidden || !sheet['!cols']?.[col]?.hidden) columns.push([col, dense == null ? encode_col(col) : undefined]);
  }
  const rowInfo = skipHidden ? sheet['!rows'] : undefined;
  return readRows();

  function* readRows() {
    let first = true;
    for (let row = range.s.r; row <= range.e.r; row += 1) {
      if (rowInfo?.[row]?.hidden) continue;
      const denseRow = dense?.[row];
      if (dense != null && denseRow == null && blankrows === false) continue;
      const rowLabel = dense == null ? encode_row(row) : undefined;
      const fields = [];
      let populated = false;
      for (const [col, letters] of columns) {
        const cell = dense == null ? sheet[letters + rowLabel] : denseRow?.[col];
        let text = '';
        if (cell?.v != null) {
          populated = true;
          text = String(rawNumbers && cell.t === 'n' ? cell.v : format_cell(cell, cell.v, settings));
          if (text && (forceQuotes || text.includes('"') || (FS && text.includes(FS[0])) ||
              (RS && text.includes(RS[0])) || (text === 'ID' && first && fields.length === 0))) {
            text = quoteCsv(text);
          }
        } else if (cell?.f != null && !cell.F) {
          populated = true;
          text = '=' + cell.f;
          if (text.includes(',')) text = quoteCsv(text);
        }
        fields.push(text);
      }
      if (blankrows === false && !populated) continue;
      if (strip) {
        while (fields.at(-1) === '') fields.pop();
      }
      const text = fields.join(FS);
      if (text || blankrows !== false) {
        yield text;
        first = false;
      }
    }
  }
}

export function sheet_to_csv(sheet, options) {
  if (sheet?.['!ref'] == null) return '';
  const settings = sheetCsvSettings(options);
  return Array.from(csvRows(sheet, settings)).join(settings.RS);
}

export function tabSeparatedText(sheet, options) {
  options ||= {};
  options.FS = '\t';
  options.RS = '\n';
  return sheet_to_csv(sheet, options);
}

export function sheet_to_txt(sheet, options) {
  const text = tabSeparatedText(sheet, options);
  const codec = codepageTable?.utils;
  if (options?.type === 'string' || typeof codec?.encode !== 'function') return text;
  const encoded = codec.encode(1200, text, 'str');
  if (typeof encoded === 'string') return '\xff\xfe' + encoded;
  const parts = ['\xff\xfe'];
  for (let start = 0; start < encoded.length; start += 4096) {
    parts.push(String.fromCharCode(...Array.prototype.slice.call(encoded, start, start + 4096)));
  }
  return parts.join('');
}

function hasStableFormulaFields(cell) {
  if (cell == null) return true;
  const prototype = Object.getPrototypeOf(cell);
  if (prototype !== null && prototype !== Object.prototype) return false;
  for (const key of ['F', 'f', 't', 'v', 'w']) {
    const field = Object.getOwnPropertyDescriptor(cell, key) ??
      (prototype && Object.getOwnPropertyDescriptor(prototype, key));
    if (field && (!Object.hasOwn(field, 'value') ||
        (field.value != null && ['object', 'function'].includes(typeof field.value)))) return false;
  }
  return true;
}

function stableDenseFormulaRow(denseRow, range, maxKeys = Infinity) {
  const keys = Object.getOwnPropertyNames(denseRow);
  if (keys.length > maxKeys) return undefined;
  const cells = [];
  for (const key of keys) {
    const column = Number(key);
    if (!isWorksheetColumn(column) || String(column) !== key || column < range.s.c || column > range.e.c) continue;
    const cell = Object.getOwnPropertyDescriptor(denseRow, key);
    if (!cell || !Object.hasOwn(cell, 'value') || !hasStableFormulaFields(cell.value)) return undefined;
    if (cell.value != null) cells.push({ column, value: cell.value });
  }
  return cells;
}

function sparseDenseFormulaCells(dense, range, canBound) {
  const height = range.e.r - range.s.r + 1;
  if (height < 4096 || !canBound(dense)) return undefined;
  if ([range.s.r, range.s.r + Math.floor(height / 2), range.e.r].every(row => Object.hasOwn(dense, row))) {
    return undefined;
  }
  const rowKeys = Object.getOwnPropertyNames(dense);
  if (rowKeys.length * 4 >= height) return undefined;
  const cells = [];
  for (const rowKey of rowKeys) {
    const row = Number(rowKey);
    if (!isWorksheetRow(row) || String(row) !== rowKey || row < range.s.r || row > range.e.r) continue;
    const entry = Object.getOwnPropertyDescriptor(dense, rowKey);
    if (!entry || !Object.hasOwn(entry, 'value')) return undefined;
    const denseRow = entry.value;
    if (denseRow == null) continue;
    if (!canBound(denseRow)) return undefined;
    const values = stableDenseFormulaRow(denseRow, range);
    if (!values) return undefined;
    for (const { column, value } of values) cells.push({ row, column, value });
  }
  return cells;
}

export function sheet_to_formulae(sheet, options) {
  if (sheet?.['!ref'] == null) return [];
  const range = decode_range(sheet['!ref']);
  const { values } = sheetFormulaSettings(options);
  const formulas = [];
  function append(cell, key) {
    if (cell == null) return;
    if (cell.F != null) {
      if (!cell.f) return;
      key = cell.F.includes(':') ? cell.F : cell.F + ':' + cell.F;
    }
    let value = cell.f;
    if (value == null) {
      if (values === false || cell.t === 'z') return;
      if (cell.t === 'n' && cell.v != null) value = cell.v;
      else if (cell.t === 'b') value = cell.v ? 'TRUE' : 'FALSE';
      else if (cell.w !== undefined) value = "'" + cell.w;
      else if (cell.v === undefined) return;
      else value = cell.t === 's' ? "'" + cell.v : cell.v;
    }
    formulas.push(`${key}=${value}`);
  }

  const dense = sheet['!data'];
  const width = range.e.c - range.s.c + 1;
  const area = width * (range.e.r - range.s.r + 1);
  if (dense == null && area >= 4096) {
    const prototype = Object.getPrototypeOf(sheet);
    if (prototype === null || prototype === Object.prototype) {
      const keys = Object.getOwnPropertyNames(sheet);
      if (prototype) keys.push(...Object.getOwnPropertyNames(prototype).filter(key => !Object.hasOwn(sheet, key)));
      if (keys.length * 4 < area) {
        const positions = [];
        for (const key of keys) {
          if (!/^[A-Z]+[1-9][0-9]*$/.test(key)) continue;
          const { r, c } = decode_cell(key);
          if (r >= range.s.r && r <= range.e.r && c >= range.s.c && c <= range.e.c) positions.push({ r, c, key });
        }
        if (positions.every(({ key }) => {
          const descriptor = Object.getOwnPropertyDescriptor(sheet, key) ??
            (prototype && Object.getOwnPropertyDescriptor(prototype, key));
          return descriptor && Object.hasOwn(descriptor, 'value') && hasStableFormulaFields(descriptor.value);
        })) {
          positions.sort((a, b) => a.r - b.r || a.c - b.c);
          for (const { key } of positions) append(sheet[key], key);
          return formulas;
        }
      }
    }
  }
  const columns = Object.create(null);
  // Standard arrays cannot have own indexed properties beyond their length.
  const noInheritedIndexes = () => ![Array.prototype, Object.prototype].some(prototype =>
    Object.getOwnPropertyNames(prototype).some(key => /^(?:0|[1-9][0-9]*)$/.test(key)));
  const standardArray = array => Array.isArray(array) && Object.getPrototypeOf(array) === Array.prototype;
  const canInitiallyBound = dense != null && noInheritedIndexes();
  const canBound = array => canInitiallyBound && standardArray(array);
  const sparseDenseCells = sparseDenseFormulaCells(dense, range, canBound);
  if (sparseDenseCells) {
    for (const { row, column, value } of sparseDenseCells) append(value, encode_col(column) + encode_row(row));
    return formulas;
  }
  let boundRows = canBound(dense);
  for (let row = range.s.r; row <= range.e.r; row += 1) {
    if (boundRows && row >= dense.length) {
      if (standardArray(dense) && noInheritedIndexes()) break;
      boundRows = false;
    }
    const denseRow = dense?.[row];
    if (dense != null && denseRow == null) continue;
    const rowLabel = encode_row(row);
    let boundColumns = canBound(denseRow);
    if (boundColumns && width >= 4096 && denseRow.length - range.s.c >= 4096 &&
        ![range.s.c, range.s.c + Math.floor(width / 2), range.e.c].every(column => Object.hasOwn(denseRow, column)) &&
        noInheritedIndexes()) {
      const cells = stableDenseFormulaRow(denseRow, range, width / 4);
      if (cells) {
        for (const { column, value } of cells) append(value, (columns[column] ??= encode_col(column)) + rowLabel);
        continue;
      }
    }
    for (let column = range.s.c; column <= range.e.c; column += 1) {
      if (boundColumns && column >= denseRow.length) {
        if (standardArray(denseRow) && noInheritedIndexes()) break;
        boundColumns = false;
      }
      if (denseRow != null) {
        const cell = denseRow[column];
        if (cell != null) append(cell, (columns[column] ??= encode_col(column)) + rowLabel);
      } else {
        const key = (columns[column] ??= encode_col(column)) + rowLabel;
        append(sheet[key], key);
      }
    }
  }
  return formulas;
}

export function sheet_new(options) {
  return sheetNewSettings(options).dense ? { '!data': [] } : {};
}

export function sheet_get_cell(sheet, reference, column = 0) {
  const dense = sheet['!data'];
  if (dense != null && isWorksheetRow(reference) && isWorksheetColumn(column)) {
    dense[reference] ||= [];
    return dense[reference][column] ||= { t: 'z' };
  }
  const key = typeof reference === 'string' ? reference :
    typeof reference === 'number' ? encode_col(column || 0) + encode_row(reference) : encode_cell(reference);
  if (dense != null) {
    const { r, c } = decode_cell(key);
    dense[r] ||= [];
    return dense[r][c] ||= { t: 'z' };
  }
  return sheet[key] ||= { t: 'z' };
}

export function sheet_set_array_formula(sheet, range, formula, dynamic) {
  const settings = sheetArrayFormulaSettings({ formula, dynamic });
  const bounds = typeof range === 'string' ? decode_range(range) : range;
  const { s, e } = bounds ?? {};
  if (![s, e].every(point => isWorksheetRow(point?.r) && isWorksheetColumn(point?.c)) ||
      s.r > e.r || s.c > e.c) {
    throw new Error('Expected a valid array formula and range');
  }
  const existing = sheet['!ref'];
  if (existing != null && typeof existing !== 'string') throw new Error('Expected a valid existing worksheet range');
  const current = existing ? decode_range(existing) : bounds;
  if (![current.s, current.e].every(point => isWorksheetRow(point.r) && isWorksheetColumn(point.c)) ||
      current.s.r > current.e.r || current.s.c > current.e.c) {
    throw new Error('Expected a valid existing worksheet range');
  }
  const reference = typeof range === 'string' ? range : encode_range(bounds);
  for (let row = s.r; row <= e.r; row += 1) {
    for (let column = s.c; column <= e.c; column += 1) {
      const cell = sheet_get_cell(sheet, row, column);
      cell.t = 'n';
      cell.F = reference;
      delete cell.v;
      delete cell.f;
      delete cell.D;
      if (row === s.r && column === s.c) {
        cell.f = settings.formula;
        if (settings.dynamic) cell.D = true;
      }
    }
  }
  sheet['!ref'] = encode_range({
    s: { r: Math.min(current.s.r, s.r), c: Math.min(current.s.c, s.c) },
    e: { r: Math.max(current.e.r, e.r), c: Math.max(current.e.c, e.c) },
  });
  return sheet;
}

export function cell_add_comment(cell, text, author) {
  (cell.c ||= []).push({ t: text, a: author || 'SheetJS' });
}

export function cell_set_hyperlink(cell, target, tooltip) {
  if (!target) delete cell.l;
  else cell.l = tooltip ? { Target: target, Tooltip: tooltip } : { Target: target };
  return cell;
}

export function cell_set_internal_link(cell, target, tooltip) {
  return cell_set_hyperlink(cell, '#' + target, tooltip);
}

export function cell_set_number_format(cell, format) {
  cell.z = format;
  return cell;
}

const htmlEntities = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;' };

function escapeHtml(value, breaks = true) {
  const escaped = String(value).replace(/[&<>"']/g, (character) => htmlEntities[character]);
  return breaks ? escaped.replaceAll('\n', '<br/>') : escaped;
}

export function htmlChunks(sheet, options) {
  const settings = sheetHtmlSettings(options ?? {});
  const reference = sheet?.['!ref'];
  const range = reference ? decode_range(reference) : undefined;
  return readHtmlChunks(sheet, settings, range);
}

function* readHtmlChunks(sheet, { header, footer, id, editable }, range) {
  yield header;
  yield id ? `<table id="${escapeHtml(id, false)}">` : '<table>';
  if (range) {
    const { s, e } = range;
    const dense = sheet['!data'];
    const columns = [];
    for (let column = s.c; column <= e.c; column += 1) columns.push(column === -1 ? '' : encode_col(column));
    const merges = (sheet['!merges'] ?? [])
      .map((range, index) => ({ range, index }))
      .filter(({ range }) => {
        if (!range?.s || !range.e) return false;
        const { s, e } = range;
        return [s.r, s.c, e.r, e.c].every(value => Number.isSafeInteger(value) && value >= 0) &&
          s.r <= e.r && s.c <= e.c;
      })
      .sort((a, b) => a.range.s.r - b.range.s.r);
    let nextMerge = 0;
    let activeMerges = [];
    let orderedMerges = [];
    for (let row = s.r; row <= e.r; row += 1) {
      const previousCount = activeMerges.length;
      const previousNext = nextMerge;
      if (previousCount) activeMerges = activeMerges.filter(({ range }) => range.e.r >= row);
      while (nextMerge < merges.length && merges[nextMerge].range.s.r <= row) {
        const merge = merges[nextMerge++];
        if (merge.range.e.r >= row) activeMerges.push(merge);
      }
      if (activeMerges.length !== previousCount || nextMerge !== previousNext) {
        orderedMerges = activeMerges.map(({ range }) => range).sort((a, b) => a.s.c - b.s.c);
        if (orderedMerges.some((range, index) => index > 0 && orderedMerges[index - 1].e.c >= range.s.c)) {
          // Overlapping ranges keep the priority of their original worksheet order.
          activeMerges.sort((a, b) => a.index - b.index);
          orderedMerges = undefined;
        }
      }
      let mergeIndex = 0;
      const output = ['<tr>'];
      const rowLabel = encode_row(row);
      const denseRow = dense?.[row];
      for (let column = s.c; column <= e.c; column += 1) {
        let merge;
        if (orderedMerges) {
          while (orderedMerges[mergeIndex]?.e.c < column) mergeIndex += 1;
          const current = orderedMerges[mergeIndex];
          if (current && current.s.c <= column) merge = current;
        } else {
          merge = activeMerges.find(({ range }) => column >= range.s.c && column <= range.e.c)?.range;
        }
        if (merge && (row !== merge.s.r || column !== merge.s.c)) {
          if (orderedMerges) column = merge.e.c;
          continue;
        }
        const key = columns[column - s.c] + rowLabel;
        let cell = dense == null ? sheet[key] : denseRow?.[column];
        if (cell?.t === 'n' && cell.v != null && !isFinite(cell.v)) {
          cell = isNaN(cell.v) ? { t: 'e', v: 36, w: '#NUM!' } : { t: 'e', v: 7, w: '#DIV/0!' };
        }
        const display = cell?.v == null ? '' : cell.t === 'e' ? cell.w ?? '' : format_cell(cell);
        let text = escapeHtml(display);
        const attributes = [];
        const attr = (name, value, breaks = false) =>
          attributes.push(` ${name}="${escapeHtml(value, breaks)}"`);
        if (merge) {
          if (merge.s.r !== merge.e.r) attr('rowspan', merge.e.r - merge.s.r + 1);
          if (merge.s.c !== merge.e.c) attr('colspan', merge.e.c - merge.s.c + 1);
        }
        if (editable) {
          text = `<span contenteditable="true">${text}</span>`;
        } else if (cell) {
          attr('data-t', cell.t || 'z');
          if (cell.v != null) attr('data-v', cell.v instanceof Date ? cell.v.toISOString() : cell.v, true);
          if (cell.z != null) attr('data-z', cell.z);
          const target = cell.l?.Target;
          if (externalHtmlLink(target)) {
            text = `<a href="${escapeHtml(target, false)}">${text}</a>`;
          }
        }
        attr('id', `${id || 'sjs'}-${key}`);
        output.push(`<td${attributes.join('')}>${text}</td>`);
        if (orderedMerges && merge) column = merge.e.c;
      }
      output.push('</tr>');
      yield output.join('');
    }
  }
  yield '</table>' + footer;
}

export function sheet_to_html(sheet, options) {
  return Array.from(htmlChunks(sheet, options)).join('');
}
