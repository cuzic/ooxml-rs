import { decorateBinaryCell } from './sheetjs-binary-records.js';
import { decodeBookProperties } from './sheetjs-book-properties.js';
import { projectedZipFiles } from './sheetjs-zip-files.js';
import { marginKeys } from './sheetjs-print-settings.js';
import { columnCharacters, columnPixels, inferDigitWidth } from './sheetjs-column-width.js';
import { biffFormula } from './sheetjs-biff-formula.js';
import { failure } from './contract.js';
import { encode_cell, encode_range } from './sheetjs-address.js';
import { get_table } from './sheetjs-ssf.js';
import { xlsbSheetProjectionSettings, xlsbWorkbookCatalogueSettings, xlsbWorkbookProjectionSettings, xlsbWorkbookResourceSettings } from './sheetjs-options.generated.js';

function decodeUnits(units, start, end) {
  const parts = [];
  for (let offset = start; offset < end; offset += 4096) {
    parts.push(String.fromCharCode(...units.subarray(offset, Math.min(offset + 4096, end))));
  }
  return parts.join('');
}

function readWorkbookInfo(archive, catalogueOnly) {
  const info = archive.workbook_info(Boolean(catalogueOnly));
  try {
    const sheets = info.take_sheets();
    const text = info.take_text();
    const tabs = [];
    let offset = 0;
    for (let index = 0; index < sheets.length; index += 3) {
      const relationshipEnd = offset + sheets[index + 1];
      const nameEnd = relationshipEnd + sheets[index + 2];
      tabs.push({
        Hidden: sheets[index],
        relationship: decodeUnits(text, offset, relationshipEnd),
        name: decodeUnits(text, relationshipEnd, nameEnd),
      });
      offset = nameEnd;
    }
    const result = { tabs, date1904: info.date1904() };
    if (!catalogueOnly) {
      result.references = info.take_references();
      result.names = readNames(info);
    }
    return result;
  } finally {
    info.free();
  }
}

function readNames(info) {
  const fields = info.take_names();
  if (!fields.length) return [];
  const text = info.take_name_text();
  const formulas = info.take_name_formulas();
  const names = [];
  let textOffset = 0;
  let formulaOffset = 0;
  for (let index = 0; index < fields.length; index += 5) {
    const [flags, sheet, nameLength, formulaLength, commentLength] = fields.subarray(index, index + 5);
    const nameEnd = textOffset + nameLength;
    const commentEnd = nameEnd + commentLength;
    const name = { Name: (flags & 0x20 ? '_xlnm.' : '') + decodeUnits(text, textOffset, nameEnd), Flags: flags };
    if (sheet < 0x0FFFFFFF) name.Sheet = sheet;
    if (commentLength) name.Comment = decodeUnits(text, nameEnd, commentEnd);
    const formulaEnd = formulaOffset + formulaLength;
    names.push({ name, tokens: formulas.subarray(formulaOffset, formulaEnd) });
    textOffset = commentEnd;
    formulaOffset = formulaEnd;
  }
  return names;
}

function readStyles(archive, state) {
  const styles = archive.styles();
  try {
    const formats = styles.take_formats();
    const text = styles.take_text();
    let offset = 0;
    for (let index = 0; index < formats.length; index += 2) {
      const end = offset + formats[index + 1];
      state.formats.set(formats[index], decodeUnits(text, offset, end));
      offset = end;
    }
    state.styles = styles.take_cell_formats();
  } finally {
    styles.free();
  }
}

function projectSharedStrings(strings, selective) {
  const lengths = strings.take_lengths();
  const units = strings.take_units();
  const count = lengths.length / 2;
  function decode(index, start) {
    const end = start + lengths[index * 2];
    const value = decodeUnits(units, start, end);
    let runOffset = end;
    const runs = Array.from({ length: lengths[index * 2 + 1] }, () => {
      const run = { ich: units[runOffset], ifnt: units[runOffset + 1] };
      runOffset += 2;
      return run;
    });
    return { value, runs };
  }

  if (!selective) {
    const entries = [];
    let offset = 0;
    for (let index = 0; index < count; index += 1) {
      entries.push(decode(index, offset));
      offset += lengths[index * 2] + lengths[index * 2 + 1] * 2;
    }
    return index => index < count ? entries[index] : undefined;
  }

  const offsets = new Uint32Array(count);
  let offset = 0;
  for (let index = 0; index < count; index += 1) {
    offsets[index] = offset;
    offset += lengths[index * 2] + lengths[index * 2 + 1] * 2;
  }
  const decoded = [];
  return index => {
    if (index >= count) return undefined;
    if (Object.hasOwn(decoded, index)) return decoded[index];
    return decoded[index] = decode(index, offsets[index]);
  };
}

function range(view) {
  return {
    s: { r: view.getUint32(0, true), c: view.getUint32(8, true) },
    e: { r: view.getUint32(4, true), c: view.getUint32(12, true) },
  };
}

function formulaData(view, offset) {
  const length = view.getUint32(offset, true);
  const start = offset + 4;
  const extra = start + length;
  const extraLength = view.getUint32(extra, true);
  if (extra + 4 + extraLength > view.byteLength) throw failure('INVALID_INPUT', 'Truncated XLSB formula');
  return {
    tokens: new Uint8Array(view.buffer, view.byteOffset + start, length),
    column: extraLength === 4 ? view.getUint32(extra + 4, true) : undefined,
  };
}

function readSheet(projection, state, settings) {
  const { rowLimit, columnStyles, formulas, numberFormats, dense } = settings;
  const grid = dense ? [] : undefined;
  const sheet = grid ? { '!data': grid } : {};
  let populated;
  let digitWidth;
  const groups = new Map();
  const pointers = [];
  const decode = (tokens, row, column) => biffFormula(tokens, 12, undefined, { row, column }, state.resolveSheet, state.resolveName);
  const locations = ([firstRow, lastRow, firstColumn, lastColumn]) =>
    ({ s: { r: firstRow, c: firstColumn }, e: { r: lastRow, c: lastColumn } });
  const dimensionFields = projection.dimensions();
  const dimensions = dimensionFields && locations(dimensionFields);
  const getCell = (r, c) => grid ? grid[r]?.[c] : sheet[encode_cell({ r, c })];
  const view = new DataView(projection.take_cells().buffer);
  const text = new DataView(projection.take_text().buffer);
  const builtins = get_table();
  for (let offset = 0; offset < view.byteLength; offset += 16) {
    const first = view.getUint32(offset, true);
    const second = view.getUint32(offset + 4, true);
    const row = first & 0xfffff;
    const column = second & 0x3fff;
    const style = (first >>> 20) | (((second >>> 14) & 0xfff) << 12);
    const data = offset + 8;
    let cell;
    switch (second >>> 26) {
      case 0: cell = { t: 'z', v: undefined }; break;
      case 1: cell = { t: 'n', v: view.getFloat64(data, true) }; break;
      case 2: cell = { t: 'e', v: view.getUint8(data) }; break;
      case 3: cell = { t: 'b', v: Boolean(view.getUint8(data)) }; break;
      case 4: {
        const start = view.getUint32(data, true);
        const end = start + view.getUint32(data + 4, true);
        const chars = [];
        for (let index = start; index < end; index += 2) chars.push(String.fromCharCode(text.getUint16(index, true)));
        cell = { t: 's', v: chars.join('') };
        break;
      }
      case 5: {
        const entry = state.sharedString(view.getUint32(data, true));
        if (entry === undefined) throw failure('INVALID_INPUT', 'Invalid XLSB shared string index');
        cell = { t: 's', v: entry.value, r: entry.runs };
        break;
      }
    }
    if (cell.t !== 'z') {
      const id = state.styles[style] ?? 0;
      const code = state.formats.get(id) ?? builtins[id] ?? 'General';
      if (numberFormats) cell.z = code;
      decorateBinaryCell(cell, code, state.date1904, settings);
    }
    if (grid) (grid[row] ??= [])[column] = cell;
    else sheet[encode_cell({ r: row, c: column })] = cell;
    populated ??= { s: { r: row, c: column }, e: { r: row, c: column } };
    populated.s.r = Math.min(populated.s.r, row);
    populated.s.c = Math.min(populated.s.c, column);
    populated.e.r = Math.max(populated.e.r, row);
    populated.e.c = Math.max(populated.e.c, column);
  }
  if (formulas) {
    const bytes = projection.take_formulas();
    const headers = new DataView(bytes.buffer);
    for (let offset = 0; offset < bytes.length;) {
      const kind = headers.getUint32(offset, true);
      const row = headers.getUint32(offset + 4, true);
      const column = headers.getUint32(offset + 8, true);
      const length = headers.getUint32(offset + 12, true);
      offset += 16;
      const payload = new DataView(bytes.buffer, offset, length);
      offset += length;
      const cell = getCell(row, column);
      const { tokens, column: anchorColumn } = formulaData(payload, kind === 1 ? 17 : kind === 2 ? 16 : 0);
      if (kind) {
        const bounds = range(payload);
        const array = kind === 1;
        const key = encode_cell(bounds.s);
        groups.set(key, { bounds, array, tokens });
        if (cell && array) cell.F = encode_range(bounds);
      } else if (cell && tokens.length === 5 && tokens[0] === 1 && anchorColumn < 16384) {
        const anchorRow = new DataView(tokens.buffer, tokens.byteOffset, tokens.length).getUint32(1, true);
        if (anchorRow < 1048576) pointers.push({ cell, row, column, key: encode_cell({ r: anchorRow, c: anchorColumn }) });
        continue;
      }
      if (cell) {
        const formula = decode(tokens, row, column);
        if (formula !== undefined) cell.f = formula;
      }
    }
  }
  for (const { cell, row, column, key } of pointers) {
    const group = groups.get(key);
    if (!group) continue;
    const { s, e } = group.bounds;
    if (row < s.r || row > e.r || column < s.c || column > e.c) continue;
    if (group.array) cell.F = encode_range(group.bounds);
    else {
      const formula = decode(group.tokens, row, column);
      if (formula !== undefined) cell.f = formula;
    }
  }
  const rows = projection.rows();
  for (let offset = 0; offset < rows.length; offset += 3) {
    const [index, height, flags] = rows.subarray(offset, offset + 3);
    const metadata = { r: index };
    if (flags & 7) metadata.level = flags & 7;
    if (flags & 16) metadata.hidden = true;
    if (flags & 32) {
      metadata.hpt = height / 20;
      metadata.hpx = metadata.hpt * 4 / 3;
    }
    (sheet['!rows'] ??= [])[index] = metadata;
  }
  if (columnStyles) {
    const runs = projection.columns();
    for (let offset = 0; offset < runs.length; offset += 4) {
      const [first, last, units, flags] = runs.subarray(offset, offset + 4);
      const width = units / 256;
      const columns = sheet['!cols'] ??= [];
      digitWidth ??= inferDigitWidth(width);
      const metadata = { width, hidden: Boolean(flags & 1), level: (flags >> 8) & 7 };
      if (width) {
        metadata.MDW = digitWidth;
        metadata.wpx = columnPixels(width, digitWidth);
        metadata.wch = columnCharacters(metadata.wpx, digitWidth);
      }
      for (let index = first; index <= last; index += 1) columns[index] = { ...metadata };
    }
  }

  const filter = projection.filter();
  if (filter) sheet['!autofilter'] = { ref: encode_range(locations(filter)) };
  const merges = projection.merges();
  for (let offset = 0; offset < merges.length; offset += 4) {
    (sheet['!merges'] ??= []).push(locations(merges.subarray(offset, offset + 4)));
  }
  const margins = projection.margins();
  if (margins) sheet['!margins'] = Object.fromEntries(marginKeys.map((key, index) => [key, margins[index]]));
  const hyperlinkRanges = projection.hyperlink_ranges();
  if (hyperlinkRanges.length) {
    const fields = projection.take_hyperlink_fields();
    for (let index = 0; index < hyperlinkRanges.length / 4; index += 1) {
      const rfx = locations(hyperlinkRanges.subarray(index * 4, index * 4 + 4));
      const [relId, loc, tooltip, display, target, type, targetMode] = fields.slice(index * 7, index * 7 + 7);
      const link = { rfx, relId, loc, display };
      if (tooltip) link.Tooltip = tooltip;
      if (type) {
        link.Target = target + (loc ? '#' + loc : '');
        link.Rel = { Type: type, Target: target, Id: relId };
        if (targetMode) link.Rel.TargetMode = targetMode;
      } else if (!relId) {
        link.Target = '#' + loc;
      }
      for (let row = rfx.s.r; row <= rfx.e.r; row += 1) {
        for (let column = rfx.s.c; column <= rfx.e.c; column += 1) {
          const cells = grid ? (grid[row] ??= []) : sheet;
          const key = grid ? column : encode_cell({ r: row, c: column });
          (cells[key] ??= { t: 'z', v: undefined }).l = link;
        }
      }
    }
  }
  const bounds = dimensions ?? populated;
  if (bounds) {
    sheet['!ref'] = encode_range(bounds);
    if (rowLimit !== undefined && rowLimit <= bounds.e.r) {
      const end = {
        r: Math.min(Math.max(0, rowLimit - 1), populated?.e.r ?? 0),
        c: Math.min(bounds.e.c, populated?.e.c ?? 0),
      };
      const start = { r: Math.min(bounds.s.r, end.r), c: Math.min(bounds.s.c, end.c) };
      sheet['!fullref'] = sheet['!ref'];
      sheet['!ref'] = encode_range(start, end);
    }
  }
  return sheet;
}

function readDependencies(fields) {
  if (fields === undefined) return Object.freeze({});
  const entries = [];
  for (let offset = 0; offset < fields.length; offset += 4) {
    const [i, row, column, flags] = fields.subarray(offset, offset + 4);
    const entry = { i, r: encode_cell({ r: row, c: column }) };
    if (flags & 2) entry.l = '1';
    if (flags & 8) entry.a = '1';
    entries.push(Object.freeze(entry));
  }
  return Object.freeze(entries);
}

export function readXlsb(archive, options) {
  try {
    const { catalogueOnly, bookSheets, bookProps } = xlsbWorkbookCatalogueSettings(options);
    const { tabs, date1904, references, names } = readWorkbookInfo(archive, catalogueOnly);
    if (bookProps) {
      const properties = decodeBookProperties(archive.document_properties());
      if (bookSheets) properties.SheetNames = tabs.map(tab => tab.name);
      return properties;
    }
    const state = { date1904, formats: new Map(), styles: [] };
    const workbook = {
      SheetNames: tabs.map(tab => tab.name),
      Workbook: { Sheets: tabs.map(({ name, Hidden }) => ({ name, Hidden })), WBProps: { date1904: state.date1904 } },
    };
    if (bookSheets) return workbook;
    const { Props, Custprops } = decodeBookProperties(archive.document_properties());
    Props.Worksheets = tabs.length;
    Props.SheetNames = workbook.SheetNames.slice();
    Object.defineProperties(workbook, {
      Props: { enumerable: true, value: Object.freeze(Props) },
      Custprops: { enumerable: true, value: Object.freeze(Custprops) },
    });
    state.resolveSheet = index => {
      const first = references[index * 2] ?? index;
      const last = references[index * 2 + 1] ?? index;
      const start = tabs[first]?.name;
      const end = tabs[last]?.name;
      if (start === undefined || end === undefined) return undefined;
      const name = first === last ? start : start + ':' + end;
      return /^[A-Za-z_][A-Za-z0-9_.]*$/.test(name) ? name : "'" + name.replaceAll("'", "''") + "'";
    };
    state.resolveName = index => {
      const name = names[index - 1]?.name.Name ?? 'SH33TJSNAME' + index;
      return !options.xlfn && name.startsWith('_xlfn.') ? name.slice(6) : name;
    };
    workbook.Workbook.Names = names.map(({ name, tokens }) => ({
      ...name,
      Ref: biffFormula(tokens, 12, undefined, undefined, state.resolveSheet, state.resolveName) ?? '#REF!',
    }));
    readStyles(archive, state);
    const sharedStrings = archive.shared_strings();
    let selectedTabs;
    try {
      const { selectSheet } = xlsbWorkbookProjectionSettings(options);
      selectedTabs = options.sheets === undefined ? tabs : tabs.filter(({ name }, index) => selectSheet(name, index));
      if (selectedTabs.length) {
        state.sharedString = projectSharedStrings(sharedStrings, selectedTabs.length < tabs.length);
      }
    } finally {
      sharedStrings.free();
    }
    workbook.Sheets = Object.create(null);
    for (const { name, relationship } of selectedTabs) {
      const settings = xlsbSheetProjectionSettings(options);
      const { rowLimit, columnStyles, stubs, formulas } = settings;
      let projection;
      try {
        projection = archive.project_sheet(relationship, rowLimit, columnStyles, stubs, formulas);
        workbook.Sheets[name] = readSheet(projection, state, settings);
      } finally {
        projection?.free();
      }
    }
    const resources = xlsbWorkbookResourceSettings(options);
    if (resources.files) {
      const { keys, files } = projectedZipFiles(archive);
      Object.defineProperties(workbook, {
        keys: { enumerable: true, value: Object.freeze(keys) },
        files: { enumerable: true, value: Object.freeze(files) },
      });
    }
    if (resources.vba) {
      const project = archive.vba_project();
      if (project !== undefined) {
        Object.defineProperty(workbook, 'vbaraw', { enumerable: true, get: () => project.slice() });
      }
    }
    workbook.bookType = 'xlsb';
    Object.defineProperty(workbook, 'Deps', { enumerable: true,
      value: readDependencies(resources.dependencies ? archive.calculation_chain() : undefined) });
    return workbook;
  } catch (error) {
    if (error?.message?.startsWith('INVALID_INPUT: ')) throw failure('INVALID_INPUT', error.message.slice(15));
    if (error instanceof RangeError) throw failure('INVALID_INPUT', 'Truncated XLSB record data', error);
    throw error;
  }
}
