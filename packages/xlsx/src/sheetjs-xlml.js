import { failure } from './contract.js';
import { decode_col, decode_range, encode_col, encode_row } from './sheetjs-address.js';
import { errorName } from './sheetjs-csv-input.js';
import { numberFormatCode } from './sheetjs-format.js';
import { printMarginDefaults, xlmlStreamSettings } from './sheetjs-options.generated.js';

export const officeNamespace = 'urn:schemas-microsoft-com:office:office';
const excelNamespace = 'urn:schemas-microsoft-com:office:excel';
const spreadsheetNamespace = 'urn:schemas-microsoft-com:office:spreadsheet';
const workbookOpening = `<Workbook xmlns="${spreadsheetNamespace}" xmlns:o="${officeNamespace}" xmlns:x="${excelNamespace}" xmlns:ss="${spreadsheetNamespace}" xmlns:dt="uuid:C2F41010-65B3-11d1-A29F-00AA00C14882" xmlns:html="http://www.w3.org/TR/REC-html40">`;
const escapedCharacters = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;',
  '\n': '&#xA;', '\r': '&#xD;', '\t': '&#x9;' };

function xml(value) {
  return String(value).replace(/[&<>"'\n\r\t]/g, (character) => escapedCharacters[character]);
}

function columnLabels({ s, e }) {
  const labels = [];
  for (let col = s.c; col <= e.c; col++) labels.push(col === -1 ? '' : encode_col(col));
  return labels;
}

function formatFor(cell) {
  return numberFormatCode(cell.z) ?? (cell.t === 'd' ? 'm/d/yy' : 'General');
}

function relativeIndex(index, base) {
  return index === base ? '' : '[' + (index - base) + ']';
}

export function formulaR1C1(formula, row, column) {
  const tokens = /"(?:[^"]|"")*"|'(?:[^']|'')*'|\[[^\]]*\]|(?<![\p{L}\p{N}_.])(\$?)([A-Z]{1,3})(\$?)([1-9]\d{0,6})(?![\p{L}\p{N}_.]|\s*[(!\[])/gu;
  return formula.replace(tokens, (token, absoluteColumn, letters, absoluteRow, digits) => {
    if (letters === undefined) return token;
    const targetColumn = decode_col(letters);
    const targetRow = Number(digits) - 1;
    if (targetColumn >= 16384 || targetRow >= 1048576) return token;
    return 'R' + (absoluteRow ? targetRow + 1 : relativeIndex(targetRow, row)) +
      'C' + (absoluteColumn ? targetColumn + 1 : relativeIndex(targetColumn, column));
  });
}

function commentsXml(comments) {
  return (comments ?? []).map(({ a, t }) => {
    const author = a ? ` ss:Author="${xml(a)}"` : '';
    const visible = comments.hidden ? '' : ' ss:ShowAlways="1"';
    return `<Comment${author}${visible}><ss:Data xmlns="http://www.w3.org/TR/REC-html40">${xml(t ?? '')}</ss:Data></Comment>`;
  }).join('');
}

function cellXml(cell, row, column, merge, styles) {
  if (!cell || ((cell.t === 'z' || (cell.v == null && !cell.f)) && !cell.c?.length)) return '';
  let type;
  let value = cell.v;
  switch (cell.t) {
    case 'n':
      type = Number.isFinite(value) ? 'Number' : 'Error';
      if (value != null && type === 'Error') value = Number.isNaN(value) ? '#NUM!' : '#DIV/0!';
      break;
    case 's': case 'str': type = 'String'; break;
    case 'b': type = 'Boolean'; if (value != null) value = value ? 1 : 0; break;
    case 'd': type = 'DateTime'; if (value != null) value = new Date(value).toISOString(); break;
    case 'e': type = 'Error'; value = errorName(value) ?? value; break;
    case 'z': break;
    default: throw failure('UNSUPPORTED_CELL', 'Unsupported SpreadsheetML cell type');
  }
  const attributes = [];
  if (cell.f) attributes.push(` ss:Formula="=${xml(formulaR1C1(cell.f, row, column))}"`);
  if (cell.F) {
    const { s, e } = decode_range(cell.F);
    if (row === s.r && column === s.c) {
      attributes.push(` ss:ArrayRange="RC:R${relativeIndex(e.r, row)}C${relativeIndex(e.c, column)}"`);
    }
  }
  if (cell.l?.Target) {
    attributes.push(` ss:HRef="${xml(cell.l.Target)}"`);
    if (cell.l.Tooltip) attributes.push(` x:HRefScreenTip="${xml(cell.l.Tooltip)}"`);
  }
  if (merge) {
    const columns = merge.e.c - merge.s.c;
    const rows = merge.e.r - merge.s.r;
    if (columns) attributes.push(` ss:MergeAcross="${columns}"`);
    if (rows) attributes.push(` ss:MergeDown="${rows}"`);
  }
  const data = type ? `<Data ss:Type="${type}">${xml(value ?? '')}</Data>` : '';
  return `<Cell${attributes.join('')} ss:StyleID="s${styles.get(formatFor(cell))}" ss:Index="${column + 1}">${data}${commentsXml(cell.c)}</Cell>`;
}

const columnDigitWidth = 6;

function columnPixels(width) {
  return Math.floor((width + Math.round(128 / columnDigitWidth) / 256) * columnDigitWidth);
}

export function normalizeXlmlColumn(column) {
  const digits = columnDigitWidth;
  const toCharacters = pixels => Math.round((pixels - 5) / digits * 100) / 100;
  const toWidth = characters => Math.round((characters * digits + 5) / digits * 256) / 256;
  if (column.width) {
    column.wpx = columnPixels(column.width);
    column.wch = toCharacters(column.wpx);
    column.MDW = digits;
  } else if (column.wpx) {
    column.wch = toCharacters(column.wpx);
    column.width = toWidth(column.wch);
    column.MDW = digits;
  } else if (typeof column.wch === 'number') {
    column.width = toWidth(column.wch);
    column.wpx = columnPixels(column.width);
    column.MDW = digits;
  }
}

function columnXml(column, index) {
  if (column == null) return '';
  normalizeXlmlColumn(column);
  const pixels = column.width ? columnPixels(column.width) : undefined;
  return `<Column ss:Index="${index + 1}"` +
    (pixels === undefined ? '' : ` ss:Width="${pixels}"`) +
    (column.hidden ? ' ss:Hidden="1"' : '') + '/>';
}

function rowOpening(row, index) {
  if (row?.hpt && !row.hpx) row.hpx = row.hpt;
  const height = row?.hpx;
  return `<Row ss:Index="${index + 1}"` +
    (height ? ` ss:AutoFitHeight="0" ss:Height="${height}"` : '') +
    (row?.hidden ? ' ss:Hidden="1"' : '') + '>';
}

function autoFilterXml(ref) {
  const { s, e } = decode_range(ref);
  const range = `R${s.r + 1}C${s.c + 1}:R${e.r + 1}C${e.c + 1}`;
  return `<AutoFilter x:Range="${range}" xmlns="${excelNamespace}"></AutoFilter>`;
}

export const documentFields = {
  Title: 'Title', Subject: 'Subject', Author: 'Author', Keywords: 'Keywords',
  Comments: 'Description', LastAuthor: 'LastAuthor', RevNumber: 'Revision',
  Application: 'AppName', LastPrinted: 'LastPrinted', CreatedDate: 'Created',
  ModifiedDate: 'LastSaved', Category: 'Category', Manager: 'Manager',
  Company: 'Company', AppVersion: 'Version', ContentStatus: 'ContentStatus',
  Identifier: 'Identifier', Language: 'Language',
};
const dateFields = new Set(['LastPrinted', 'CreatedDate', 'ModifiedDate']);

function documentProperties(book, overrides) {
  if (!book.Props && !overrides) return '';
  const properties = { ...book.Props };
  const replacement = { ...overrides };
  let output = '';
  for (const [key, tag] of Object.entries(documentFields)) {
    let value = replacement[key] ?? properties[key];
    if (value == null) continue;
    if (dateFields.has(key)) value = new Date(value).toISOString().replace(/\.\d+Z$/, 'Z');
    if (typeof value === 'boolean') value = Number(value);
    output += `<${tag}>${xml(value)}</${tag}>`;
  }
  return `<DocumentProperties xmlns="${officeNamespace}">${output}</DocumentProperties>`;
}

function customProperties(properties) {
  if (!properties) return '';
  let output = '';
  for (const [name, property] of Object.entries(properties)) {
    const tag = name.replace(/_(?=[xX][0-9a-fA-F]{4}_)|^[^A-Za-z_]|[^A-Za-z0-9_.-]/gu, (char) =>
      Array.from({ length: char.length }, (_, index) =>
        '_x' + char.charCodeAt(index).toString(16).padStart(4, '0') + '_').join(''));
    if (!tag) throw failure('INVALID_VALUE', 'Expected a custom document property name');
    let type = 'string';
    let value = property;
    if (property instanceof Date) {
      type = 'dateTime.tz';
      value = property.toISOString();
    } else if (typeof property === 'boolean') {
      type = 'boolean';
      value = Number(property);
    } else if (typeof property === 'number') {
      type = 'float';
    }
    output += `<${tag} dt:dt="${type}">${xml(value)}</${tag}>`;
  }
  return `<CustomDocumentProperties xmlns="${officeNamespace}">${output}</CustomDocumentProperties>`;
}

function namesXml(names, sheetIndex) {
  if (!names) return '';
  let output = '';
  for (const entry of names) {
    if ((entry.Sheet ?? undefined) !== sheetIndex || entry.Name.startsWith('_xlfn.')) continue;
    const name = entry.Name.replace(/^_xlnm\./, '');
    const formula = formulaR1C1(entry.Ref.replace(/^=/, ''), 0, 0);
    output += `<NamedRange ss:Name="${xml(name)}" ss:RefersTo="=${xml(formula)}"/>`;
  }
  return output || sheetIndex === undefined ? `<Names>${output}</Names>` : '';
}

function worksheetOptionsXml(margins, hidden, rightToLeft) {
  let content = '';
  if (margins) {
    const { header, footer, bottom = printMarginDefaults.bottom, left = printMarginDefaults.left,
      right = printMarginDefaults.right, top = printMarginDefaults.top } = margins;
    content = '<PageSetup>';
    if (header != null) content += `<Header x:Margin="${xml(header)}"/>`;
    if (footer != null) content += `<Footer x:Margin="${xml(footer)}"/>`;
    content += `<PageMargins x:Bottom="${xml(bottom)}" x:Left="${xml(left)}" x:Right="${xml(right)}" x:Top="${xml(top)}"/></PageSetup>`;
  }
  if (hidden) content += `<Visible>Sheet${hidden === 2 ? 'VeryHidden' : 'Hidden'}</Visible>`;
  if (rightToLeft) content += '<DisplayRightToLeft/>';
  return content ? `<WorksheetOptions xmlns="${excelNamespace}">${content}</WorksheetOptions>` : '';
}

export function* xlmlChunks(book, options, separateRows = false) {
  const { stride } = xlmlStreamSettings(options);
  const sheets = book.SheetNames.map((name, index) => {
    const sheet = book.Sheets[name];
    const reference = sheet?.['!ref'];
    return { name, sheet, index, range: reference ? decode_range(reference) : undefined,
      hidden: book.Workbook?.Sheets?.[index]?.Hidden };
  });
  const styles = new Map([['General', 21]]);
  for (const { sheet, range } of sheets) {
    if (!range) continue;
    const dense = sheet['!data'];
    const labels = dense ? undefined : columnLabels(range);
    for (let row = range.s.r; row <= range.e.r; row++) {
      const denseRow = dense?.[row];
      if (dense && !denseRow) continue;
      const rowLabel = dense ? undefined : encode_row(row);
      for (let col = range.s.c; col <= range.e.c; col++) {
        const cell = dense ? denseRow[col] : sheet[labels[col - range.s.c] + rowLabel];
        if (!cell) continue;
        const format = formatFor(cell);
        if (!styles.has(format)) styles.set(format, styles.size + 21);
      }
    }
  }
  yield '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n';
  yield workbookOpening;
  yield documentProperties(book, options?.Props);
  yield customProperties(book.Custprops);
  if (book.Workbook?.WBProps?.date1904) yield `<ExcelWorkbook xmlns="${excelNamespace}"><Date1904/></ExcelWorkbook>`;
  yield '<Styles><Style ss:ID="Default" ss:Name="Normal"><NumberFormat/></Style>' +
    Array.from(styles, ([format, index]) =>
      `<Style ss:ID="s${index}"><NumberFormat ss:Format="${xml(format)}"/></Style>`).join('') + '</Styles>';
  const names = book.Workbook?.Names;
  yield namesXml(names);
  for (const { name, sheet, index, range, hidden } of sheets) {
    yield `<Worksheet ss:Name="${xml(name)}">`;
    yield namesXml(names, index);
    if (range) {
      yield '<Table>';
      for (const column of (sheet['!cols'] ?? []).map(columnXml)) if (column) yield column;
      let batch = [];
      const merges = sheet['!merges'] ?? [];
      const dense = sheet['!data'];
      const labels = dense ? undefined : columnLabels(range);
      for (let row = range.s.r; row <= range.e.r; row++) {
        const cells = [];
        const denseRow = dense?.[row];
        if (!dense || denseRow) {
          const rowLabel = dense ? undefined : encode_row(row);
          for (let col = range.s.c; col <= range.e.c; col++) {
            const cell = dense ? denseRow[col] : sheet[labels[col - range.s.c] + rowLabel];
            if (!cell) continue;
            const merge = merges.find(({ s, e }) => row >= s.r && row <= e.r && col >= s.c && col <= e.c);
            if (merge && (merge.s.r !== row || col !== merge.s.c)) continue;
            cells.push(cellXml(cell, row, col, merge, styles));
          }
        }
        batch.push(rowOpening(sheet['!rows']?.[row], row) + cells.join('') + '</Row>');
        if (batch.length === (stride || 10)) {
          yield separateRows ? batch : batch.join('');
          batch = [];
        }
      }
      if (batch.length) yield separateRows ? batch : batch.join('');
      yield '</Table>';
    }
    yield worksheetOptionsXml(sheet?.['!margins'], hidden, book.Workbook?.Views?.[0]?.RTL);
    if (sheet?.['!autofilter']?.ref) yield autoFilterXml(sheet['!autofilter'].ref);
    yield '</Worksheet>';
  }
  yield '</Workbook>';
}
