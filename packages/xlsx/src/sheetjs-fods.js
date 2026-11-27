import { xmlRoot } from './sheetjs-xml.js';
import { failure } from './contract.js';
import { serialFromDate } from './sheetjs-date.js';
import { decode_range, encode_cell, encode_range } from './sheetjs-address.js';
import { formatted } from './sheetjs-format.js';
import { checkOpenDocumentRowLimit, checkOpenDocumentSelectors, openDocumentWorkbookProjectionSettings } from './sheetjs-options.generated.js';

const office = 'urn:oasis:names:tc:opendocument:xmlns:office:1.0';
const table = 'urn:oasis:names:tc:opendocument:xmlns:table:1.0';
const text = 'urn:oasis:names:tc:opendocument:xmlns:text:1.0';
const style = 'urn:oasis:names:tc:opendocument:xmlns:style:1.0';
const formulaNamespace = 'urn:oasis:names:tc:opendocument:xmlns:of:1.2';
const xlink = 'http://www.w3.org/1999/xlink';
const dc = 'http://purl.org/dc/elements/1.1/';
const meta = 'urn:oasis:names:tc:opendocument:xmlns:meta:1.0';
const mime = 'application/vnd.oasis.opendocument.spreadsheet';
const xmlEntities = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;' };
const xml = (value) => String(value).replace(/[&<>"']/g, (character) => xmlEntities[character]);

function openFormula(formula) {
  const tokens = /"(?:[^"]|"")*"|(?<![\p{L}\p{N}_.])(?:(?<sheet>'(?:[^']|'')*'|[\p{L}_][\p{L}\p{N}_.]*)!)?(?<reference>\$?[A-Z]{1,3}\$?[1-9]\d{0,6})(?::(?<end>\$?[A-Z]{1,3}\$?[1-9]\d{0,6}))?(?![\p{L}\p{N}_.]|\s*\()|,/gu;
  return 'of:=' + formula.replace(/^=/, '').replace(tokens, (token, sheet, reference, end) => {
    if (token === ',') return ';';
    if (reference === undefined) return token;
    return '[' + (sheet ?? '') + '.' + reference + (end === undefined ? '' : ':.' + end) + ']';
  });
}

// ODF 6.1.2 collapses runs of white space and drops it at paragraph edges. Encode runs of
// spaces as text:s with a count, a single edge space as text:s, and tabs as text:tab, as 0.20.3 does.
function paragraphContent(line) {
  return xml(line)
    .replace(/ {2,}/g, (run) => `<text:s text:c="${run.length}"/>`)
    .replace(/\t/g, '<text:tab/>')
    .replace(/^ /, '<text:s/>')
    .replace(/ $/, '<text:s/>');
}

function textParagraphs(value, link) {
  const target = link?.Target;
  const title = link?.Tooltip == null ? '' : ` xlink:title="${xml(link.Tooltip)}"`;
  return String(value).split(/\r\n|\n|\r/).map((line) => {
    const content = paragraphContent(line);
    return '<text:p>' + (target ? `<text:a xlink:href="${xml(target)}"${title}>${content}</text:a>` : content) + '</text:p>';
  }).join('');
}

function annotations(comments) {
  return (comments ?? []).map((comment) => {
    const author = comment.a ? `<dc:creator>${xml(comment.a)}</dc:creator>` : '';
    return `<office:annotation office:display="${!comments.hidden}">${author}${textParagraphs(comment.t ?? '')}</office:annotation>`;
  }).join('');
}

function cellXml(cell, merge, r, c) {
  let attribute = '';
  if (merge) {
    const columns = merge.e.c - merge.s.c + 1;
    const rows = merge.e.r - merge.s.r + 1;
    if (columns > 1) attribute += ` table:number-columns-spanned="${columns}"`;
    if (rows > 1) attribute += ` table:number-rows-spanned="${rows}"`;
  }
  if (cell == null) return `<table:table-cell${attribute}/>`;
  const notes = annotations(cell.c);
  if (cell.f) {
    attribute += ` table:formula="${xml(openFormula(cell.f))}"`;
    if (cell.F) {
      const { s, e } = decode_range(cell.F);
      if (s.r !== r || s.c !== c || e.r < r || e.c < c || e.r >= 1048576 || e.c >= 16384) {
        throw failure('INVALID_ADDRESS', 'Expected the FODS array formula at its range origin');
      }
      attribute += ` table:number-matrix-columns-spanned="${e.c - c + 1}"` +
        ` table:number-matrix-rows-spanned="${e.r - r + 1}"`;
    }
  }
  const value = cell.v;
  if (value == null || cell.t === 'z') {
    return notes ? `<table:table-cell${attribute}>${notes}</table:table-cell>` : `<table:table-cell${attribute}/>`;
  }
  switch (cell.t) {
    case 'b':
      attribute += ` office:value-type="boolean" office:boolean-value="${Boolean(value)}"`;
      break;
    case 'n':
      attribute += ` office:value-type="float" office:value="${xml(value)}"`;
      break;
    case 'd':
      attribute += ` office:value-type="date" office:date-value="${xml(new Date(value).toISOString())}"`;
      break;
    default:
      attribute += ' office:value-type="string"';
  }
  const display = cell.t === 'd'
    ? formatted({ ...cell, z: cell.z ?? 'm/d/yy' }, value)
    : cell.t === 'n' || cell.t === 'b' || cell.t === 'e' ? formatted(cell, value) : value;
  return `<table:table-cell${attribute}>${notes}${textParagraphs(display, cell.l)}</table:table-cell>`;
}

function namedRangesXml(book, sheetIndex) {
  const names = book.Workbook?.Names?.filter((name) => name.Sheet === sheetIndex) ?? [];
  if (!names.length) return '';
  const ranges = names.map(({ Name, Ref }) => {
    const defaultSheet = book.SheetNames[sheetIndex ?? 0];
    const quotedSheet = /^[\p{L}_][\p{L}\p{N}_]*$/u.test(defaultSheet)
      ? defaultSheet : "'" + defaultSheet.replaceAll("'", "''") + "'";
    const reference = /^((?:'(?:[^']|'')*'|[^!]+)!)?(\$?[A-Z]{1,3}\$?[1-9]\d*)(?::(\$?[A-Z]{1,3}\$?[1-9]\d*))?$/.exec(Ref);
    const baseSheet = reference?.[1]?.slice(0, -1) ?? quotedSheet;
    const base = '$' + baseSheet + '.$A$1';
    if (reference) {
      const address = (sheetIndex === undefined ? '$' : '') + baseSheet + '.' + reference[2] +
        (reference[3] === undefined ? '' : ':.' + reference[3]);
      return `<table:named-range table:name="${xml(Name)}" table:cell-range-address="${xml(address)}" table:base-cell-address="${xml(base)}"/>`;
    }
    return `<table:named-expression table:name="${xml(Name)}" table:expression="${xml(openFormula(Ref))}" table:base-cell-address="${xml(base)}"/>`;
  });
  return '<table:named-expressions>' + ranges.join('') + '</table:named-expressions>';
}

const metadataFields = [
  ['Title', 'dc', 'title'], ['Subject', 'dc', 'subject'], ['Comments', 'dc', 'description'],
  ['Author', 'meta', 'initial-creator'], ['LastAuthor', 'dc', 'creator'],
  ['Keywords', 'meta', 'keyword'], ['CreatedDate', 'meta', 'creation-date'],
  ['ModifiedDate', 'dc', 'date'], ['LastPrinted', 'meta', 'print-date'],
  ['Language', 'dc', 'language'], ['Identifier', 'dc', 'identifier'],
];

function documentMetadataXml(book) {
  const properties = [];
  for (const [field, prefix, tag] of metadataFields) {
    const property = book.Props?.[field];
    if (property == null) continue;
    const value = property instanceof Date ? property.toISOString() : property;
    properties.push(`<${prefix}:${tag}>${xml(value)}</${prefix}:${tag}>`);
  }
  for (const [name, value] of Object.entries(book.Custprops ?? {})) {
    const kind = value instanceof Date ? 'date' : typeof value === 'number' ? 'float' :
      typeof value === 'boolean' ? 'boolean' : 'string';
    const content = value instanceof Date ? value.toISOString() : value;
    properties.push(`<meta:user-defined meta:name="${xml(name)}" meta:value-type="${kind}">${xml(content)}</meta:user-defined>`);
  }
  return '<office:meta>' + properties.join('') + '</office:meta>';
}

export function writeOdfMetadata(book) {
  return '<?xml version="1.0" encoding="UTF-8"?>' +
    `<office:document-meta xmlns:office="${office}" xmlns:dc="${dc}" xmlns:meta="${meta}" office:version="1.2">` +
    documentMetadataXml(book) + '</office:document-meta>';
}

export function writeFods(book, packaged = false) {
  const root = packaged ? 'office:document-content' : 'office:document';
  const mimeAttribute = packaged ? '' : ` office:mimetype="${mime}"`;
  const output = [
    '<?xml version="1.0" encoding="UTF-8"?>',
    `<${root} xmlns:office="${office}" xmlns:table="${table}" xmlns:text="${text}" xmlns:style="${style}" xmlns:of="${formulaNamespace}" xmlns:xlink="${xlink}" xmlns:dc="${dc}" xmlns:meta="${meta}" office:version="1.2"${mimeAttribute}>`,
  ];
  if (!packaged && (book.Props || book.Custprops)) output.push(documentMetadataXml(book));
  const sheetMetadata = book.Workbook?.Sheets;
  if (sheetMetadata?.some((sheet) => sheet?.Hidden)) {
    output.push('<office:automatic-styles><style:style style:name="HiddenTable" style:family="table">' +
      '<style:table-properties table:display="false"/></style:style></office:automatic-styles>');
  }
  output.push('<office:body><office:spreadsheet>');
  for (const [index, name] of book.SheetNames.entries()) {
    const sheet = book.Sheets[name];
    const styleAttribute = sheetMetadata?.[index]?.Hidden ? ' table:style-name="HiddenTable"' : '';
    output.push(`<table:table table:name="${xml(name)}"${styleAttribute}>`);
    const merges = sheet?.['!merges'] ?? [];
    let range = sheet?.['!ref'] ? decode_range(sheet['!ref']) : undefined;
    for (const merge of merges) {
      if (!range) range = { s: { ...merge.s }, e: { ...merge.e } };
      range.s.r = Math.min(range.s.r, merge.s.r);
      range.s.c = Math.min(range.s.c, merge.s.c);
      range.e.r = Math.max(range.e.r, merge.e.r);
      range.e.c = Math.max(range.e.c, merge.e.c);
    }
    if (range) {
      const { s, e } = range;
      if (s.r) output.push(`<table:table-row table:number-rows-repeated="${s.r}"><table:table-cell/></table:table-row>`);
      const dense = sheet['!data'];
      for (let r = s.r; r <= e.r; r++) {
        output.push('<table:table-row>');
        if (s.c) output.push(`<table:table-cell table:number-columns-repeated="${s.c}"/>`);
        for (let c = s.c; c <= e.c; c++) {
          const address = encode_cell({ r, c });
          const merge = merges.find(({ s, e }) => r >= s.r && r <= e.r && c >= s.c && c <= e.c);
          if (merge && (merge.s.r !== r || merge.s.c !== c)) {
            output.push('<table:covered-table-cell/>');
          } else {
            output.push(cellXml(dense ? dense[r]?.[c] : sheet[address], merge, r, c));
          }
        }
        output.push('</table:table-row>');
      }
    }
    output.push(namedRangesXml(book, index), '</table:table>');
  }
  output.push(namedRangesXml(book), `</office:spreadsheet></office:body></${root}>`);
  return output.join('\n');
}

export function isFodsText(input) {
  return /<(?:[\w.-]+:)?document\b[^>]*\burn:oasis:names:tc:opendocument:xmlns:office:1\.0\b/.test(input);
}

const childElements = (node, namespace, tag) => Array.from(node?.children ?? [])
  .filter((element) => element.namespaceURI === namespace && element.localName === tag);
const attribute = (node, namespace, tag) => node.getAttributeNS(namespace, tag);

function repeats(node, kind, remaining) {
  const setting = attribute(node, table, 'number-' + kind + '-repeated');
  const count = setting === null ? 1 : Number(setting);
  if (!Number.isInteger(count) || count < 1 || count > remaining) {
    throw failure('INVALID_INPUT', 'FODS repeated cell or row exceeds worksheet bounds');
  }
  return count;
}

function span(node, axis, remaining) {
  const value = attribute(node, table, 'number-' + axis + '-spanned');
  const size = value === null ? 1 : Number(value);
  if (!Number.isInteger(size) || size < 1 || size > remaining) {
    throw failure('INVALID_INPUT', 'FODS merged span exceeds worksheet bounds');
  }
  return size;
}

function formulaFromOdf(value) {
  const formula = value.replace(/^(?:[\w.-]+:)?=/, '');
  const parts = /"(?:[^"]|"")*"|\[([^\]]+)\]|;/g;
  return formula.replace(parts, (token, reference) => {
    if (token === ';') return ',';
    if (reference === undefined) return token;
    return reference.replace(/(^|:)(?:('(?:[^']|'')*'|[^.]+)?\.)/g, (_, start, sheet) =>
      start + (sheet ? sheet.replace(/^\$/, '') + '!' : ''));
  });
}

function paragraphText(node) {
  let value = '';
  for (const child of node.childNodes) {
    if (child.nodeType === 3) value += child.nodeValue;
    else if (child.nodeType === 1) {
      if (child.namespaceURI === text && child.localName === 's') {
        const count = Number(attribute(child, text, 'c') ?? 1);
        if (!Number.isInteger(count) || count < 1 || count > 32767) throw failure('INVALID_INPUT', 'Invalid FODS space count');
        value += ' '.repeat(count);
      } else if (child.namespaceURI === text && child.localName === 'tab') value += '\t';
      else if (child.namespaceURI === text && child.localName === 'line-break') value += '\n';
      else value += paragraphText(child);
    }
  }
  return value;
}

function readCell(element, options) {
  const type = attribute(element, office, 'value-type');
  const paragraphs = childElements(element, text, 'p');
  const display = paragraphs.map(paragraphText).join('\n');
  const comments = childElements(element, office, 'annotation');
  const anchor = paragraphs.map((paragraph) => paragraph.getElementsByTagNameNS(text, 'a')[0]).find(Boolean);
  const formula = attribute(element, table, 'formula');
  let cell;
  if (type === 'float' || type === 'percentage' || type === 'currency') {
    const value = attribute(element, office, 'value');
    cell = value === null ? { t: 'n' } : { t: 'n', v: Number(value) };
  } else if (type === 'boolean') {
    cell = { t: 'b', v: attribute(element, office, 'boolean-value') === 'true' };
  } else if (type === 'date') {
    const raw = attribute(element, office, 'date-value');
    const date = new Date(raw ?? NaN);
    if (!Number.isFinite(date.getTime())) throw failure('INVALID_INPUT', 'Invalid FODS date');
    cell = options.cellDates ? { t: 'd', v: date } : { t: 'n', v: serialFromDate(date.getTime(), false) };
    if (options.cellNF) cell.z = options.dateNF ?? 'm/d/yy';
  } else if (type === 'string' || paragraphs.length) {
    cell = { t: 's', v: attribute(element, office, 'string-value') ?? display };
  } else if (formula && options.cellFormula !== false) {
    cell = { t: 'n' };
  }
  if (!cell && comments.length) cell = { t: 'z' };
  if (!cell) return undefined;
  if (comments.length) {
    cell.c = comments.map((comment) => ({
      a: childElements(comment, dc, 'creator')[0]?.textContent ?? '',
      t: childElements(comment, text, 'p').map(paragraphText).join('\n'),
    }));
    if (comments.every((comment) => attribute(comment, office, 'display') === 'false')) cell.c.hidden = true;
  }
  if (anchor) {
    const target = attribute(anchor, xlink, 'href');
    const tooltip = attribute(anchor, xlink, 'title');
    if (target) cell.l = { Target: target, ...(tooltip == null ? {} : { Tooltip: tooltip }) };
  }
  if (formula && options.cellFormula !== false) cell.f = formulaFromOdf(formula);
  if (paragraphs.length && options.cellText !== false) cell.w = display;
  return cell;
}

function readFodsSheet(element, options) {
  const sheet = options.dense ? { '!data': [] } : {};
  const merges = [];
  const arrays = [];
  let range;
  let row = 0;
  for (const layout of childElements(element, table, 'table-row')) {
    const height = repeats(layout, 'rows', 1048576 - row);
    const limit = options.sheetRows ? Math.max(0, Math.min(height, options.sheetRows - row)) : height;
    let column = 0;
    for (const node of layout.children) {
      if (node.namespaceURI !== table || !['table-cell', 'covered-table-cell'].includes(node.localName)) continue;
      const width = repeats(node, 'columns', 16384 - column);
      const cell = node.localName === 'table-cell' ? readCell(node, options) : undefined;
      if (node.localName === 'table-cell') {
        const mergeRows = span(node, 'rows', 1048576 - row);
        const mergeColumns = span(node, 'columns', 16384 - column);
        if (mergeRows > 1 || mergeColumns > 1) {
          if (height !== 1 || width !== 1) throw failure('UNSUPPORTED_STRUCTURE', 'FODS repeated merge anchors are unsupported');
          merges.push({ s: { r: row, c: column },
            e: { r: row + mergeRows - 1, c: column + mergeColumns - 1 } });
        }
      }
      if (cell?.f && (node.hasAttributeNS(table, 'number-matrix-rows-spanned') ||
          node.hasAttributeNS(table, 'number-matrix-columns-spanned'))) {
        const arrayRows = span(node, 'matrix-rows', 1048576 - row);
        const arrayColumns = span(node, 'matrix-columns', 16384 - column);
        if (height !== 1 || width !== 1) throw failure('UNSUPPORTED_STRUCTURE', 'FODS repeated array anchors are unsupported');
        const s = { r: row, c: column };
        const e = { r: row + arrayRows - 1, c: column + arrayColumns - 1 };
        arrays.push({ s, e, ref: encode_range(s, e) });
      }
      if (cell) {
        const endRow = row + height - 1;
        const endColumn = column + width - 1;
        if (!range) range = { s: { r: row, c: column }, e: { r: endRow, c: endColumn } };
        range.s.c = Math.min(column, range.s.c);
        range.e.c = Math.max(endColumn, range.e.c);
        range.e.r = Math.max(endRow, range.e.r);
        for (let r = row; r < row + limit; r++) {
          for (let c = column; c <= endColumn; c++) {
            const value = { ...cell };
            if (r !== row || c !== column) {
              if (cell.v instanceof Date) value.v = new Date(cell.v);
              if (cell.l) value.l = { ...cell.l };
              if (cell.c) {
                value.c = cell.c.map((comment) => ({ ...comment }));
                if (cell.c.hidden) value.c.hidden = true;
              }
            }
            const array = arrays.find(({ s, e }) => r >= s.r && r <= e.r && c >= s.c && c <= e.c);
            if (array) value.F = array.ref;
            if (options.dense) (sheet['!data'][r] ??= [])[c] = value;
            else sheet[encode_cell({ r, c })] = value;
          }
        }
      }
      column += width;
    }
    row += height;
  }
  if (range) {
    if (options.sheetRows && range.e.r >= options.sheetRows) {
      sheet['!fullref'] = encode_range(range);
      range.e.r = options.sheetRows - 1;
    }
    sheet['!ref'] = encode_range(range);
  }
  if (merges.length) sheet['!merges'] = merges;
  return sheet;
}

function readNamedRanges(parent, sheetIndex) {
  const names = [];
  for (const group of childElements(parent, table, 'named-expressions')) {
    for (const node of group.children) {
      if (node.namespaceURI !== table) continue;
      const Name = attribute(node, table, 'name');
      const range = attribute(node, table, 'cell-range-address');
      const expression = attribute(node, table, 'expression');
      const Ref = range ? formulaFromOdf('=[' + range + ']') :
        expression ? formulaFromOdf(expression) : undefined;
      if (Name && Ref) names.push(sheetIndex === undefined ? { Name, Ref } : { Name, Ref, Sheet: sheetIndex });
    }
  }
  return names;
}

function tableVisibility(...documents) {
  const definitions = new Map();
  for (const document of documents) for (const container of ['styles', 'automatic-styles']) {
    for (const group of childElements(document, office, container)) {
      for (const entry of childElements(group, style, 'style')) {
        if (attribute(entry, style, 'family') !== 'table') continue;
        const properties = childElements(entry, style, 'table-properties')[0];
        definitions.set(attribute(entry, style, 'name'), {
          display: properties ? attribute(properties, table, 'display') : null,
          parent: attribute(entry, style, 'parent-style-name'),
        });
      }
    }
  }
  return (element) => {
    const direct = attribute(element, table, 'display');
    if (direct != null) return direct === 'false' ? 1 : 0;
    const visited = new Set();
    let name = attribute(element, table, 'style-name');
    while (name && !visited.has(name)) {
      visited.add(name);
      const definition = definitions.get(name);
      if (definition?.display != null) return definition.display === 'false' ? 1 : 0;
      name = definition?.parent;
    }
    return 0;
  };
}

function readOdfMetadata(root) {
  const group = childElements(root, office, 'meta')[0];
  if (!group) return {};
  const Props = {};
  const Custprops = {};
  for (const [field, prefix, tag] of metadataFields) {
    const node = childElements(group, prefix === 'dc' ? dc : meta, tag)[0];
    if (!node) continue;
    Props[field] = field.endsWith('Date') || field === 'LastPrinted' ? new Date(node.textContent) : node.textContent;
  }
  for (const node of childElements(group, meta, 'user-defined')) {
    const name = attribute(node, meta, 'name');
    if (name == null) continue;
    const type = attribute(node, meta, 'value-type');
    const content = node.textContent;
    const value = type === 'date' ? new Date(content) : type === 'float' ? Number(content) :
      type === 'boolean' ? content === 'true' : content;
    Object.defineProperty(Custprops, name, { value, enumerable: true, writable: true, configurable: true });
  }
  return { Props, Custprops };
}

function parseOdfXml(input) {
  const root = xmlRoot(input, 'OpenDocument');
  if (root.namespaceURI !== office) throw failure('INVALID_INPUT', 'Invalid OpenDocument XML');
  return root;
}

export function readFods(input, options, parts) {
  checkOpenDocumentRowLimit(options);
  const root = parseOdfXml(input);
  const body = childElements(root, office, 'body')[0];
  const spreadsheet = childElements(body, office, 'spreadsheet')[0];
  if (!spreadsheet) throw failure('INVALID_INPUT', 'Expected an OpenDocument spreadsheet');
  checkOpenDocumentSelectors(options);
  const selection = options.bookSheets ? undefined : openDocumentWorkbookProjectionSettings(options);
  const definedNames = [];
  const metadata = [];
  const hidden = tableVisibility(parts?.styles ? parseOdfXml(parts.styles) : null, root);
  const properties = readOdfMetadata(parts?.metadata ? parseOdfXml(parts.metadata) : root);
  const book = { ...properties, SheetNames: [], Sheets: {}, Workbook: { Names: definedNames, Sheets: metadata }, bookType: 'fods' };
  const names = new Set();
  for (const element of childElements(spreadsheet, table, 'table')) {
    const name = attribute(element, table, 'name');
    if (!name || names.has(name)) throw failure('INVALID_INPUT', 'Invalid FODS worksheet name');
    names.add(name);
    const index = book.SheetNames.length;
    book.SheetNames.push(name);
    metadata.push({ name, Hidden: hidden(element) });
    definedNames.push(...readNamedRanges(element, index));
    if (selection?.selectSheet(name, index)) {
      Object.defineProperty(book.Sheets, name, { value: readFodsSheet(element, options),
        enumerable: true, configurable: true, writable: true });
    }
  }
  definedNames.push(...readNamedRanges(spreadsheet));
  if (options.bookSheets) delete book.Sheets;
  return book;
}
