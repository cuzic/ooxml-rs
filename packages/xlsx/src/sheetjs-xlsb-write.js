import { columnCharacters, columnWidth, defaultDigitWidth } from './sheetjs-column-width.js';
import { compileBiffFormula, compileBinaryNames } from './sheetjs-biff-formula.js';
import { autofilterRange, marginEdits } from './sheetjs-print-settings.js';
import { document_property_parts, write_xlsb_cells, write_xlsb_shared_strings, write_xlsb_sheet_header, write_xlsb_sheet_end, write_xlsb_styles, write_xlsb_workbook } from './runtime.js';
import { failure, rustCall } from './contract.js';
import { decode_range, encode_cell } from './sheetjs-address.js';
import { biffBuiltInNames, joinRecords, packText } from './sheetjs-binary-records.js';
import { get_table } from './sheetjs-ssf.js';
import { serialFromDate } from './sheetjs-date.js';

const relationships = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/';
const contentNamespace = 'http://schemas.openxmlformats.org/package/2006/content-types';
const metadataParts = [
  ['docProps/core.xml', 'application/vnd.openxmlformats-package.core-properties+xml',
    'http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties'],
  ['docProps/app.xml', 'application/vnd.openxmlformats-officedocument.extended-properties+xml',
    relationships + 'extended-properties'],
  ['docProps/custom.xml', 'application/vnd.openxmlformats-officedocument.custom-properties+xml',
    relationships + 'custom-properties'],
];
const workbookMime = 'application/vnd.ms-excel.sheet.binary.macroEnabled.main';

function styleRecords(formats) {
  const builtins = get_table();
  const keys = Object.keys(builtins).filter(id => Number(id) < 164);
  const custom = [];
  const ids = Int32Array.from(formats.keys(), code => {
    const id = keys.find(key => builtins[key] === code);
    if (id !== undefined) return Number(id);
    custom.push(code);
    return -1;
  });
  return write_xlsb_styles(ids, ...packText(custom));
}

function formulaValue(cell, date1904) {
  const type = cell.t === 'd' ? 'n' : cell.t;
  const numeric = cell.t === 'd' ? serialFromDate(new Date(cell.v).getTime(), date1904) : Number(cell.v ?? 0);
  if (type === 'b') return [10, Number(cell.v)];
  if (type === 'e') return [11, Number(cell.v)];
  if (type === 'n' && !Number.isFinite(numeric)) return [11, Number.isNaN(numeric) ? 36 : 7];
  if (type === 's' || type === 'str') return [8, String(cell.v ?? '').slice(0, 32767)];
  return [9, Number.isFinite(numeric) ? numeric : 0];
}

function definedNames(names, sheetIndices, sheetIndex) {
  if (names != null && !Array.isArray(names)) throw failure('INVALID_VALUE', 'Expected a list of XLSB defined names');
  const candidates = [];
  const scopes = new Map();
  for (const { Name: name, Ref: ref, Sheet: sheet, Comment: comment, Flags: flags } of names ?? []) {
    if (flags & 0x0e) continue; // Macro names require additional fields.
    if (typeof name !== 'string' || name.length === 0 || name.length > 255 ||
        (sheet !== undefined && (!Number.isInteger(sheet) || sheet < 0 || sheet >= sheetIndices.size)) ||
        (comment !== undefined && (typeof comment !== 'string' || comment.length > 255))) {
      throw failure('INVALID_VALUE', 'Invalid XLSB defined name, worksheet scope or comment');
    }
    const scope = sheet ?? -1;
    if (!scopes.has(scope)) scopes.set(scope, new Map());
    const lookup = scopes.get(scope);
    const key = name.toLowerCase();
    if (lookup.has(key)) throw failure('INVALID_VALUE', 'Duplicate XLSB defined name in the same scope');
    const candidate = { name, ref, sheet, comment, dependencies: [], dependents: [] };
    lookup.set(key, candidate);
    candidates.push(candidate);
  }
  function resolve(name, sheet, qualifiedSheet) {
    const key = name.toLowerCase();
    if (qualifiedSheet !== undefined) return scopes.get(sheetIndices.get(qualifiedSheet.toLowerCase()))?.get(key);
    return scopes.get(sheet)?.get(key) ?? scopes.get(-1)?.get(key);
  }
  const fields = [];
  const text = [];
  const formulas = [];
  for (const { name, sheet, comment = '', tokens } of compileBinaryNames(candidates, 12, sheetIndex, resolve, () => 16384)) {
    const builtIn = name.startsWith('_xlnm.') && biffBuiltInNames.includes(name.slice(6));
    const label = builtIn ? name.slice(6) : name;
    fields.push(builtIn ? 0x20 : 0, sheet ?? 0xffffffff, label.length, tokens.length, comment.length);
    text.push(label, comment);
    formulas.push(tokens);
  }
  return {
    fields: Uint32Array.from(fields), text: packText(text)[1], formulas: joinRecords(formulas),
    resolve: (name, sheet, qualifiedSheet) => resolve(name, sheet, qualifiedSheet)?.index,
  };
}

const protectionPermissions = [
  'objects', 'scenarios', 'formatCells', 'formatColumns', 'formatRows', 'insertColumns', 'insertRows',
  'insertHyperlinks', 'deleteColumns', 'deleteRows', 'selectLockedCells', 'sort', 'autoFilter',
  'pivotTables', 'selectUnlockedCells',
];

function sheetProtection(value) {
  if (!value) return [undefined, 0];
  const password = value.password ?? '';
  if (typeof password !== 'string') throw failure('INVALID_VALUE', 'Expected an XLSB sheet protection password string');
  let flags = 0;
  protectionPermissions.forEach((key, index) => {
    const permission = value[key];
    if (permission != null) flags |= 1 << index;
    if (permission) flags |= 1 << (index + 16);
  });
  return [password, flags];
}

function worksheet(sheet, state, options, resolveName, linkRelationships) {
  const extent = decode_range(sheet['!ref'] || 'A1');
  const columns = [];
  let digits = defaultDigitWidth;
  sheet['!cols']?.forEach((column, index) => {
    if (!column || index >= 16384) return;
    digits = column.MDW || digits;
    const characters = column.wpx !== undefined ? columnCharacters(column.wpx, digits) : column.wch;
    const width = column.width ?? (characters !== undefined ? columnWidth(characters, digits) : undefined);
    const units = Math.round(Math.max(0, Math.min(255, width ?? 10)) * 256);
    const flags = Number(Boolean(column.hidden)) | (width === undefined ? 0 : 2) | ((column.level & 7) << 8);
    columns.push(index, units, flags);
  });
  const dimensions = Uint32Array.of(extent.s.r, extent.e.r, extent.s.c, extent.e.c);
  const records = [write_xlsb_sheet_header(dimensions, Uint32Array.from(columns))];
  const grid = sheet['!data'];
  const getCell = (r, c) => grid ? grid[r]?.[c] : sheet[encode_cell({ r, c })];
  const arrays = new Map();
  function arrayAt(cell, row, col) {
    if (typeof cell.F !== 'string') return undefined;
    if (!arrays.has(cell.F)) {
      const range = decode_range(cell.F);
      const { s, e } = range;
      const anchor = getCell(s.r, s.c);
      const fits = s.r >= extent.s.r && s.c >= extent.s.c && s.r <= e.r && s.c <= e.c &&
        e.r <= extent.e.r && e.c <= extent.e.c;
      const tokens = fits && anchor?.F === cell.F && !anchor.D
        ? compileBiffFormula(anchor.f, 12, state.sheetIndex, resolveName) : undefined;
      arrays.set(cell.F, tokens ? { range, tokens } : undefined);
    }
    const group = arrays.get(cell.F);
    if (!group) return undefined;
    const { s, e } = group.range;
    return row >= s.r && row <= e.r && col >= s.c && col <= e.c ? group : undefined;
  }
  const rows = sheet['!rows'];
  const rowFields = [];
  const rowCounts = [];
  const cellFields = [];
  const numbers = [];
  const strings = [];
  const formulaParts = [];
  const linkedCells = new Map();
  function writeRow(row, hasCells) {
    const metadata = rows?.[row];
    let count = 0;
    const spans = new Map();
    for (let col = extent.s.c; hasCells && col <= extent.e.c; col += 1) {
      const cell = getCell(row, col);
      if (!cell) continue;
      const link = cell.l;
      if (typeof link?.Target === 'string' && link.Target) {
        if (!linkedCells.has(link)) linkedCells.set(link, []);
        linkedCells.get(link).push(row, col);
      }
      const spanIndex = col >> 10;
      const span = spans.get(spanIndex);
      if (span) span[1] = col;
      else spans.set(spanIndex, [col, col]);
      const code = cell.z ?? (cell.t === 'd' ? 'm/d/yy' : 'General');
      if (!state.formats.has(code)) state.formats.set(code, state.formats.size);
      const style = state.formats.get(code);
      const add = (kind, value = 0, tokens, anchor, array) => {
        if (kind === 5 || kind === 9) {
          numbers.push(value);
          value = numbers.length - 1;
        } else if (kind === 6 || kind === 8) {
          strings.push(value);
          value = value.length;
        }
        if (tokens) formulaParts.push(tokens);
        if (array) formulaParts.push(array.tokens);
        cellFields.push(col, style, kind, value, tokens?.length ?? 0, anchor?.c ?? 0xffffffff,
          array?.tokens.length ?? 0, array?.range.e.r ?? 0, array?.range.e.c ?? 0);
        count += 1;
      };
      const formula = cell.F ? undefined : compileBiffFormula(cell.f, 12, state.sheetIndex, resolveName);
      const group = arrayAt(cell, row, col);
      if (group) {
        const anchor = group.range.s;
        const pointer = new Uint8Array(5);
        pointer[0] = 1;
        new DataView(pointer.buffer).setUint32(1, anchor.r, true);
        add(...formulaValue(cell, state.date1904), pointer, anchor, anchor.r === row && anchor.c === col ? group : undefined);
      } else if (formula) add(...formulaValue(cell, state.date1904), formula);
      else if (cell.v == null || cell.t === 'z') add(1);
      else if (cell.t === 'n' || cell.t === 'd') {
        const value = cell.t === 'd' ? serialFromDate(new Date(cell.v).getTime(), state.date1904) : Number(cell.v);
        if (Number.isFinite(value)) add(5, value);
        else add(3, Number.isNaN(value) ? 36 : 7);
      } else if (cell.t === 'b' || cell.t === 'e') {
        add(cell.t === 'b' ? 4 : 3, Number(cell.v) & 255);
      } else {
        const text = String(cell.v).slice(0, 32767);
        if (options.bookSST) {
          if (!state.strings.has(text)) state.strings.set(text, state.strings.size);
          state.stringCount += 1;
          add(7, state.strings.get(text));
        } else add(6, text);
      }
    }
    if (count || metadata) {
      const height = metadata?.hpt ?? (metadata?.hpx === undefined ? undefined : metadata.hpx * 0.75);
      const units = height === undefined ? 320 : Math.round(Math.max(0, Math.min(409, height)) * 20);
      const flags = (metadata?.level & 7) | (metadata?.hidden ? 16 : 0) | (height === undefined ? 0 : 32);
      rowFields.push(row, units, flags, spans.size);
      for (const [first, last] of spans.values()) rowFields.push(first, last);
      rowCounts.push(count);
    }
  }
  rows?.forEach((row, index) => { if (row && index < extent.s.r) writeRow(index, false); });
  for (let row = extent.s.r; row <= extent.e.r; row += 1) writeRow(row, true);
  rows?.forEach((row, index) => { if (row && index > extent.e.r && index < 1048576) writeRow(index, false); });
  const [, units] = packText(strings);
  records.push(write_xlsb_cells(Uint32Array.from(rowFields), Uint32Array.from(rowCounts),
    Uint32Array.from(cellFields), Float64Array.from(numbers), units, joinRecords(formulaParts)));
  const coordinates = ({ s, e }) => [s.r, e.r, s.c, e.c];
  const filter = autofilterRange(sheet, extent.e.r);
  const filterCoordinates = filter && Uint32Array.from(coordinates(filter));
  const merges = Uint32Array.from((sheet['!merges'] ?? []).flatMap(coordinates));
  const linkRanges = [];
  const linkText = [];
  for (const [link, cells] of linkedCells) {
    const hash = link.Target.indexOf('#');
    const target = hash < 0 ? link.Target : link.Target.slice(0, hash);
    const location = hash < 0 ? '' : link.Target.slice(hash + 1);
    let relationship = '';
    if (target) {
      relationship = 'rId' + (linkRelationships.length + 1);
      linkRelationships.push([relationship, relationships + 'hyperlink', target, 'External']);
    }
    const tooltip = String(link.Tooltip ?? '');
    const display = String(link.display ?? '');
    const addLink = (top, bottom, left, right) => {
      linkRanges.push(top, bottom, left, right);
      linkText.push(relationship, location, tooltip, display);
    };
    const { s, e } = link.rfx ?? {};
    if (s && e && [s.r, e.r, s.c, e.c].every(Number.isInteger) &&
        s.r <= e.r && s.c <= e.c &&
        cells.length / 2 === (e.r - s.r + 1) * (e.c - s.c + 1) &&
        cells.every((value, index) => index % 2 ? s.c <= value && value <= e.c : s.r <= value && value <= e.r)) {
      addLink(s.r, e.r, s.c, e.c);
    } else {
      for (let offset = 0; offset < cells.length; offset += 2) {
        const row = cells[offset], col = cells[offset + 1];
        addLink(row, row, col, col);
      }
    }
  }
  records.push(write_xlsb_sheet_end(filterCoordinates, merges, marginEdits(sheet), Uint32Array.from(linkRanges), linkText,
    ...sheetProtection(sheet['!protect'])));
  return joinRecords(records);
}

function escapeAttribute(value) {
  return value.replace(/[&<>"\t\r\n]/g, character => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', '\t': '&#9;', '\r': '&#13;', '\n': '&#10;',
  })[character]);
}

function rels(entries) {
  return '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
    entries.map(([id, type, target, mode]) =>
      `<Relationship Id="${id}" Type="${type}" Target="${escapeAttribute(target)}"${mode ? ` TargetMode="${mode}"` : ''}/>`).join('') +
    '</Relationships>';
}

export function xlsbParts(workbook, options, properties, customProperties) {
  const parts = [];
  const xml = new TextEncoder();
  const state = { date1904: Boolean(workbook.Workbook?.WBProps?.date1904), formats: new Map([['General', 0]]),
    strings: new Map(), stringCount: 0 };
  const sheetIndices = new Map(workbook.SheetNames.map((name, index) => [name.toLowerCase(), index]));
  const references = new Map();
  state.sheetIndex = name => {
    const index = sheetIndices.get(name.toLowerCase());
    if (index === undefined) return undefined;
    if (!references.has(index)) {
      if (references.size >= 65536) return undefined;
      references.set(index, references.size);
    }
    return references.get(index);
  };
  const namesToWrite = definedNames(workbook.Workbook?.Names, sheetIndices, state.sheetIndex);
  const links = [];
  const types = [['/xl/workbook.bin', workbookMime]];
  workbook.SheetNames.forEach((name, index) => {
    const sheet = workbook.Sheets?.[name];
    if (!sheet) throw failure('SHEET_READ_REFUSED', `Worksheet is not loaded: ${name}`);
    const id = 'rId' + (index + 1);
    const target = 'worksheets/sheet' + (index + 1) + '.bin';
    links.push([id, 'worksheet', target]);
    types.push(['/xl/' + target, 'application/vnd.ms-excel.worksheet']);
    const linkRelationships = [];
    const resolveName = (label, scope) => namesToWrite.resolve(label, index, scope);
    parts.push(['xl/' + target, worksheet(sheet, state, options, resolveName, linkRelationships)]);
    if (linkRelationships.length) {
      parts.push(['xl/worksheets/_rels/sheet' + (index + 1) + '.bin.rels', xml.encode(rels(linkRelationships))]);
    }
  });
  const visibility = Uint32Array.from(workbook.SheetNames,
    (_, index) => workbook.Workbook?.Sheets?.[index]?.Hidden ?? 0);
  const [lengths, names] = packText(workbook.SheetNames);
  parts.push(['xl/workbook.bin', write_xlsb_workbook(state.date1904, visibility, lengths, names,
    Uint32Array.from(references.keys()), namesToWrite.fields, namesToWrite.text, namesToWrite.formulas)]);
  links.push(['rId' + (links.length + 1), 'styles', 'styles.bin']);
  types.push(['/xl/styles.bin', 'application/vnd.ms-excel.styles']);
  parts.push(['xl/styles.bin', styleRecords(state.formats)]);
  if (state.strings.size) {
    const text = packText([...state.strings.keys()]);
    links.push(['rId' + (links.length + 1), 'sharedStrings', 'sharedStrings.bin']);
    types.push(['/xl/sharedStrings.bin', 'application/vnd.ms-excel.sharedStrings']);
    parts.push(['xl/sharedStrings.bin', write_xlsb_shared_strings(state.stringCount, ...text)]);
  }
  parts.push(['xl/_rels/workbook.bin.rels', xml.encode(rels(links.map(([id, type, target]) =>
    [id, relationships + type, target])))]);
  const packageLinks = [['rId1', relationships + 'officeDocument', 'xl/workbook.bin']];
  const sheetNames = workbook.SheetNames.filter((_, index) => visibility[index] !== 2);
  const metadata = rustCall('WORKBOOK_EDIT_REFUSED', () => document_property_parts(properties, customProperties, sheetNames));
  metadataParts.forEach(([name, mime, type], index) => {
    if (!metadata[index]) return;
    parts.push([name, xml.encode(metadata[index])]);
    types.push(['/' + name, mime]);
    packageLinks.push(['rId' + (packageLinks.length + 1), type, name]);
  });
  parts.push(['_rels/.rels', xml.encode(rels(packageLinks))]);
  parts.push(['[Content_Types].xml', xml.encode(`<Types xmlns="${contentNamespace}">` +
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>' +
    types.map(([part, mime]) => `<Override PartName="${part}" ContentType="${mime}"/>`).join('') + '</Types>')]);
  return parts;
}
