import { biffBuiltInNames } from './sheetjs-binary-records.js';
import { checkSpreadsheetMlProjection } from './sheetjs-options.generated.js';
import { xmlRoot } from './sheetjs-xml.js';
import { failure } from './contract.js';
import { decode_range, encode_cell, encode_range } from './sheetjs-address.js';
import { errorCode } from './sheetjs-csv-input.js';
import { serialFromDate } from './sheetjs-date.js';
import { documentFields, normalizeXlmlColumn, officeNamespace } from './sheetjs-xlml.js';
import { is_date, parse_date_code } from './sheetjs-ssf.js';
import { formatTable, formatted } from './sheetjs-format.js';

const excelNamespace = 'urn:schemas-microsoft-com:office:excel';
const namespace = 'urn:schemas-microsoft-com:office:spreadsheet';
const children = (node, name) => Array.from(node?.children ?? [])
  .filter((child) => child.namespaceURI === namespace && child.localName === name);
const first = (node, name) => children(node, name)[0];
const attr = (node, name) => node?.getAttributeNS(namespace, name) ?? node?.getAttribute(name);

export function isXlmlText(text) {
  return /<(?:[\w.-]+:)?Workbook\b[^>]*\burn:schemas-microsoft-com:office:spreadsheet\b/.test(text);
}

function coordinate(node, fallback, maximum) {
  const index = attr(node, 'Index');
  const position = index == null ? fallback : Number(index) - 1;
  if (!Number.isInteger(position) || position < fallback || position >= maximum) {
    throw failure('INVALID_INPUT', 'Invalid SpreadsheetML row or column index');
  }
  return position;
}

export function formulaA1(formula, row, column) {
  const tokens = /"(?:[^"]|"")*"|'(?:[^']|'')*'|\[[^\]]*\]|(?<![\p{L}\p{N}_.])R(\[-?\d+\]|[1-9]\d*)?C(\[-?\d+\]|[1-9]\d*)?(?![\p{L}\p{N}_.!])/gu;
  return formula.replace(tokens, (token, rowRef, colRef) => {
    if (token[0] !== 'R') return token;
    const position = (value, base) => value === undefined ? base :
      value.startsWith('[') ? base + Number(value.slice(1, -1)) : Number(value) - 1;
    const r = position(rowRef, row);
    const c = position(colRef, column);
    if (r < 0 || r >= 1048576 || c < 0 || c >= 16384) return '#REF!';
    const cell = encode_cell({ r, c });
    const absCol = colRef && !colRef.startsWith('[');
    const absRow = rowRef && !rowRef.startsWith('[');
    return (absCol ? '$' : '') + (absRow ? cell.replace(/\d+$/, (digits) => '$' + digits) : cell);
  });
}

function cellValue(data, type, date1904) {
  if (!data) return { t: 'z' };
  const text = data.textContent;
  switch (type) {
    case 'Number': return text === '' ? { t: 'n' } : { t: 'n', v: Number(text) };
    case 'Boolean': return { t: 'b', v: text === '1' || text === 'true' };
    case 'String': return { t: 's', v: text, r: text };
    case 'Error': return text ? { t: 'e', v: errorCode(text) } : { t: 'n' };
    case 'DateTime': {
      const timestamp = /T\d/i.test(text) && !/(?:Z|[+-]\d{2}:?\d{2})$/i.test(text) ? text + 'Z' : text;
      const date = new Date(timestamp);
      if (!Number.isFinite(date.getTime())) throw failure('INVALID_INPUT', 'Invalid SpreadsheetML date');
      return { t: 'n', v: serialFromDate(date.getTime(), date1904) };
    }
    default: return { t: 's', v: text };
  }
}

function boundedSpan(value, start, maximum) {
  const size = value == null ? 0 : Number(value);
  if (!Number.isInteger(size) || size < 0 || start + size >= maximum) {
    throw failure('INVALID_INPUT', 'Invalid SpreadsheetML row or column span');
  }
  return size;
}

function readColumns(table, styleIds) {
  const columns = [];
  let normalizeWidths = false;
  let index = 0;
  for (const element of children(table, 'Column')) {
    const c = coordinate(element, index, 16384);
    const width = attr(element, 'Width');
    const column = {};
    const style = attr(element, 'StyleID');
    if (width != null) {
      column.wpx = Number.parseInt(width, 10);
      normalizeWidths ||= column.wpx > 10;
    }
    const hidden = attr(element, 'Hidden');
    if (hidden === '1' || hidden === 'true') column.hidden = true;
    const span = boundedSpan(attr(element, 'Span'), c, 16384);
    for (index = c; index <= c + span; index++) {
      columns[index] = { ...column };
      styleIds[index] = style;
    }
  }
  if (normalizeWidths) columns.forEach(normalizeXlmlColumn);
  return columns;
}

function readComments(entry) {
  const comments = children(entry, 'Comment');
  if (!comments.length) return undefined;
  const values = comments.map((comment) => {
    const author = attr(comment, 'Author');
    const t = first(comment, 'Data')?.textContent ?? '';
    return author == null ? { t } : { a: author, t };
  });
  if (comments.some((comment) => !['1', 'true'].includes(attr(comment, 'ShowAlways')))) {
    values.hidden = true;
  }
  return values;
}

function worksheetOptions(element) {
  return Array.from(element.children).find((child) =>
    child.namespaceURI === excelNamespace && child.localName === 'WorksheetOptions');
}

function readMargins(element) {
  const setup = worksheetOptions(element)?.getElementsByTagNameNS(excelNamespace, 'PageSetup')[0];
  if (!setup) return undefined;
  const margins = setup.getElementsByTagNameNS(excelNamespace, 'PageMargins')[0];
  const header = setup.getElementsByTagNameNS(excelNamespace, 'Header')[0];
  const footer = setup.getElementsByTagNameNS(excelNamespace, 'Footer')[0];
  if (!margins && !header && !footer) return undefined;
  const values = { left: 1, right: 1, top: 1, bottom: 1, header: 0.5, footer: 0.5 };
  const read = (key, node, name) => {
    const attribute = node?.getAttributeNS(excelNamespace, name) ?? node?.getAttribute(name);
    if (attribute != null && Number.isFinite(Number(attribute))) values[key] = Number(attribute);
  };
  for (const key of ['Bottom', 'Left', 'Right', 'Top']) read(key.toLowerCase(), margins, key);
  read('header', header, 'Margin');
  read('footer', footer, 'Margin');
  return values;
}

const namedFormats = new Map([
  ['General Number', 'General'], ['General Date', 'm/d/yy h:mm'],
  ['Long Date', 'dddd, mmmm dd, yyyy'], ['Medium Date', 'd-mmm-yy'], ['Short Date', 'm/d/yy'],
  ['Long Time', 'h:mm:ss AM/PM'], ['Medium Time', 'h:mm AM/PM'], ['Short Time', 'h:mm'],
  ['Fixed', '0.00'], ['Standard', '#,##0.00'], ['Percent', '0.00%'], ['Scientific', '0.00E+00'],
  ['Yes/No', '"Yes";"Yes";"No";@'], ['True/False', '"True";"True";"False";@'],
  ['On/Off', '"Yes";"Yes";"No";@'],
  ['Currency', '"$"#,##0.00_);[Red]\\("$"#,##0.00\\)'],
]);

function readStyleFormats(styles) {
  const definitions = new Map(children(styles, 'Style').map((style) => [
    attr(style, 'ID'), { format: attr(first(style, 'NumberFormat'), 'Format'), parent: attr(style, 'Parent') },
  ]));
  const resolved = new Map();
  for (const style of definitions.keys()) {
    const visited = new Set();
    let current = style;
    let format;
    while (current != null && !visited.has(current)) {
      if (resolved.has(current)) {
        format = resolved.get(current) ?? format;
        break;
      }
      visited.add(current);
      const definition = definitions.get(current);
      if (definition?.format) format = definition.format;
      current = definition?.parent;
    }
    resolved.set(style, namedFormats.get(format) ?? format);
  }
  return resolved;
}

const fillPatterns = new Map([
  ['None', 'none'], ['Solid', 'solid'], ['Gray50', 'mediumGray'], ['Gray75', 'darkGray'], ['Gray25', 'lightGray'],
  ['HorzStripe', 'darkHorizontal'], ['VertStripe', 'darkVertical'], ['ReverseDiagStripe', 'darkDown'],
  ['DiagStripe', 'darkUp'], ['DiagCross', 'darkGrid'], ['ThickDiagCross', 'darkTrellis'],
  ['ThinHorzStripe', 'lightHorizontal'], ['ThinVertStripe', 'lightVertical'],
  ['ThinReverseDiagStripe', 'lightDown'], ['ThinHorzCross', 'lightGrid'],
]);

function readFillPatterns(styles) {
  const definitions = new Map(children(styles, 'Style').map(style => [attr(style, 'ID'), style]));
  const resolved = new Map();
  for (const id of definitions.keys()) {
    const visited = new Set();
    for (let style = id; definitions.has(style) && !visited.has(style);) {
      visited.add(style);
      const definition = definitions.get(style);
      const pattern = attr(first(definition, 'Interior'), 'Pattern');
      if (pattern) {
        resolved.set(id, fillPatterns.get(pattern) ?? pattern);
        break;
      }
      style = attr(definition, 'Parent');
    }
  }
  return resolved;
}

function readSheet(element, styles, fills, options, date1904) {
  const sheet = options.dense ? { '!data': [] } : {};
  let range;
  const table = first(element, 'Table');
  const columnStyles = [];
  const columns = readColumns(table, columnStyles);
  const rows = [];
  const merges = [];
  const arrayRanges = [];
  let rowIndex = 0;
  let firstRow;
  let mergedStubCount = 0;
  for (const row of children(table, 'Row')) {
    const r = coordinate(row, rowIndex, 1048576);
    firstRow ??= r;
    const include = !options.sheetRows || r < options.sheetRows;
    rowIndex = r + 1;
    const height = attr(row, 'Height');
    const pixels = height == null ? undefined : Number.parseInt(height, 10);
    const hidden = attr(row, 'Hidden') === '1';
    const rowStyle = attr(row, 'StyleID');
    if (height != null || hidden) rows[r] = {
      ...(pixels !== undefined ? { hpx: pixels, hpt: pixels } : {}),
      ...(hidden ? { hidden } : {}),
    };
    let columnIndex = 0;
    for (const entry of children(row, 'Cell')) {
      if (attr(entry, 'Index') == null) {
        let covered;
        while ((covered = merges.find(({ s, e }) =>
          r > s.r && r <= e.r && columnIndex >= s.c && columnIndex <= e.c))) {
          columnIndex = covered.e.c + 1;
        }
      }
      const c = coordinate(entry, columnIndex, 16384);
      const across = boundedSpan(attr(entry, 'MergeAcross'), c, 16384);
      const down = boundedSpan(attr(entry, 'MergeDown'), r, 1048576);
      columnIndex = c + across + 1;
      if (across || down) merges.push({ s: { r, c }, e: { r: r + down, c: c + across } });
      const formula = attr(entry, 'Formula');
      const data = first(entry, 'Data');
      if (!range) range = { s: { r: firstRow, c }, e: { r, c } };
      range.s.c = Math.min(range.s.c, c);
      range.e.c = Math.max(range.e.c, c);
      if (!include) continue;
      if (options.sheetStubs && (across || down)) {
        const lastRow = options.sheetRows ? Math.min(r + down, options.sheetRows - 1) : r + down;
        mergedStubCount += (lastRow - r + 1) * (across + 1) - 1;
        if (mergedStubCount > 1_000_000) {
          throw failure('INVALID_INPUT', 'Too many SpreadsheetML merged cell stubs');
        }
        for (let stubRow = r; stubRow <= lastRow; stubRow++) {
          const denseRow = options.dense ? (sheet['!data'][stubRow] ??= []) : undefined;
          for (let stubCol = c + Number(stubRow === r); stubCol <= c + across; stubCol++) {
            if (denseRow) denseRow[stubCol] ??= { t: 'z' };
            else sheet[encode_cell({ r: stubRow, c: stubCol })] ??= { t: 'z' };
          }
        }
      }
      const dataType = attr(data, 'Type');
      const cell = cellValue(data, dataType, date1904);
      if (options.cellFormula !== false) {
        if (formula) cell.f = formulaA1(formula.replace(/^=/, ''), r, c);
        const array = attr(entry, 'ArrayRange');
        if (array) {
          const ref = formulaA1(array === 'RC' ? 'RC:RC' : array, r, c).replaceAll('$', '');
          if (!/^[A-Z]+[1-9]\d*(?::[A-Z]+[1-9]\d*)?$/.test(ref)) {
            throw failure('INVALID_INPUT', 'Invalid SpreadsheetML array range');
          }
          arrayRanges.push({ ref, ...decode_range(ref) });
        }
        const activeArray = arrayRanges.find(({ s, e }) =>
          r >= s.r && r <= e.r && c >= s.c && c <= e.c);
        if (activeArray) cell.F = activeArray.ref;
      }
      const target = attr(entry, 'HRef');
      if (target) {
        const tooltip = entry.getAttributeNS(excelNamespace, 'HRefScreenTip');
        cell.l = { Target: target, ...(tooltip == null ? {} : { Tooltip: tooltip }) };
      }
      const comments = readComments(entry);
      if (comments) cell.c = comments;
      if (cell.t === 'z' && cell.f === undefined && !comments) continue;
      const style = attr(entry, 'StyleID');
      const effectiveStyle = style ?? rowStyle ?? columnStyles[c];
      if (fills && data) {
        const patternType = fills.get(effectiveStyle);
        cell.s = patternType === undefined ? {} : { patternType };
      }
      const styleFormat = styles.get(effectiveStyle) ?? styles.get('Default') ?? 'General';
      const format = dataType === 'DateTime' && styleFormat === 'General' ? 'yyyy-mm-dd' : styleFormat;
      if (style != null) cell.StyleID = cell.ixfe = style;
      const index = attr(entry, 'Index');
      if (index != null) cell.Index = index;
      if (options.cellText !== false && cell.v !== undefined) {
        try {
          cell.w = formatted({ t: cell.t, v: cell.v, z: format }, cell.v, { ...options, date1904 });
        } catch {
          cell.w = String(cell.v);
        }
      }
      if (options.cellDates && cell.t === 'n' && typeof cell.v === 'number' && is_date(format)) {
        const decoded = parse_date_code(cell.v, { date1904 });
        if (decoded) {
          const { y, m, d, H, M, S, u } = decoded;
          cell.t = 'd';
          cell.v = new Date(Date.UTC(y, m - 1, d, H, M, S, Math.round(u * 1000)));
        }
      }
      if (options.cellNF) cell.z = format;
      if (options.dense) (sheet['!data'][r] ??= [])[c] = cell;
      else sheet[encode_cell({ r, c })] = cell;
    }
  }
  if (range) {
    range.e.r = rowIndex - 1;
    if (options.sheetRows && range.e.r >= options.sheetRows) {
      sheet['!fullref'] = encode_range(range);
      range.e.r = options.sheetRows - 1;
    }
    sheet['!ref'] = encode_range(range);
  }
  if (rows.length) sheet['!rows'] = rows;
  if (merges.length) sheet['!merges'] = merges;
  if (columns.length) sheet['!cols'] = columns;
  const filter = Array.from(element.children).find((child) =>
    child.namespaceURI === excelNamespace && child.localName === 'AutoFilter');
  const filterRef = filter?.getAttributeNS(excelNamespace, 'Range') ?? filter?.getAttribute('Range');
  if (filterRef) sheet['!autofilter'] = { ref: formulaA1(filterRef, 0, 0).replaceAll('$', '') };
  const margins = readMargins(element);
  if (margins) sheet['!margins'] = margins;
  return sheet;
}

const documentTags = new Map(Object.entries(documentFields).map(([field, tag]) => [tag, field]));
const dataTypeNamespace = 'uuid:C2F41010-65B3-11d1-A29F-00AA00C14882';

function documentMetadata(root) {
  const officeProperties = (tag) => Array.from(root.children).find((element) =>
    element.namespaceURI === officeNamespace && element.localName === tag);
  const Props = {};
  const Custprops = {};
  for (const item of officeProperties('DocumentProperties')?.children ?? []) {
    const key = documentTags.get(item.localName);
    if (key === undefined) continue;
    Props[key] = item.textContent;
  }
  for (const item of officeProperties('CustomDocumentProperties')?.children ?? []) {
    const key = item.localName.replace(/_x([0-9a-f]{4})_/gi, (_, code) => String.fromCharCode(Number.parseInt(code, 16)));
    const type = item.getAttributeNS(dataTypeNamespace, 'dt');
    const text = item.textContent;
    let value = text;
    if (type === 'boolean') value = text === '1' || text === 'true';
    else if (type === 'i2' || type === 'int') value = Number.parseInt(text, 10);
    else if (type === 'r4' || type === 'float') value = Number.parseFloat(text);
    else if (type === 'i4' || type === 'number') value = Number(text);
    else if (type === 'date' || type === 'dateTime.tz' || type === 'dateTime') value = new Date(text);
    Object.defineProperty(Custprops, key, { value, enumerable: true, writable: true, configurable: true });
  }
  return { Props, Custprops };
}

function readNames(element, sheetIndex) {
  const names = [];
  for (const item of children(first(element, 'Names'), 'NamedRange')) {
    const name = attr(item, 'Name');
    const formula = attr(item, 'RefersTo');
    if (name != null && formula != null) {
      const Name = biffBuiltInNames.includes(name) ? '_xlnm.' + name : name;
      const Ref = formulaA1(formula.replace(/^=/, ''), 0, 0);
      names.push(sheetIndex === undefined ? { Name, Ref } : { Name, Ref, Sheet: sheetIndex });
    }
  }
  return names;
}

function visibility(element) {
  const visible = worksheetOptions(element)?.getElementsByTagNameNS(excelNamespace, 'Visible')[0]?.textContent;
  return visible === 'SheetVeryHidden' ? 2 : visible === 'SheetHidden' ? 1 : 0;
}

export function readXlml(text, options) {
  checkSpreadsheetMlProjection(options);
  const root = xmlRoot(text, 'SpreadsheetML');
  if (root.localName !== 'Workbook' || root.namespaceURI !== namespace) {
    throw failure('INVALID_INPUT', 'Expected a SpreadsheetML workbook');
  }
  const date1904 = root.getElementsByTagNameNS(excelNamespace, 'Date1904').length !== 0;
  const styleTable = first(root, 'Styles');
  const styles = readStyleFormats(styleTable);
  const fills = options.cellStyles ? readFillPatterns(styleTable) : undefined;
  const metadata = documentMetadata(root);
  const worksheets = children(root, 'Worksheet');
  const sheetNames = new Set();
  const Names = readNames(root);
  const Sheets = [];
  const book = { ...metadata, SheetNames: [], Sheets: {}, SSF: { ...formatTable },
    Workbook: { WBProps: { date1904 }, Names, Sheets }, bookType: 'xlml' };
  for (const element of worksheets) {
    const name = attr(element, 'Name');
    if (!name || sheetNames.has(name)) throw failure('INVALID_INPUT', 'Invalid SpreadsheetML sheet name');
    const index = book.SheetNames.length;
    sheetNames.add(name);
    book.SheetNames.push(name);
    Sheets.push({ name, Hidden: visibility(element) });
    if (worksheetOptions(element)?.getElementsByTagNameNS(excelNamespace, 'DisplayRightToLeft').length) {
      book.Workbook.Views = [{ RTL: true }];
    }
    Names.push(...readNames(element, index));
    if (!options.bookProps && !options.bookSheets) {
      Object.defineProperty(book.Sheets, name, { value: readSheet(element, styles, fills, options, date1904),
        enumerable: true, writable: true, configurable: true });
    }
  }
  if (options.bookProps || options.bookSheets) delete book.Sheets;
  return book;
}
