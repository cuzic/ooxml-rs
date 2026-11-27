import { failure } from './contract.js';
import { decode_cell, decode_range, encode_cell, encode_range } from './sheetjs-address.js';
import { book_new } from './sheetjs-book.js';
import { cellFromText, errorName } from './sheetjs-csv-input.js';
import { sheetDomSettings } from './sheetjs-options.generated.js';

function isHidden(element) {
  const display = element.ownerDocument?.defaultView?.getComputedStyle(element).display;
  if (display || element.style?.display) return (display || element.style.display) === 'none';
  return element.hidden === true;
}

export function externalHtmlLink(target) {
  return typeof target === 'string' && target !== '' && !target.startsWith('#') &&
    !/^(?:javascript|vbscript|data):/i.test(target.replace(/[\u0000-\u0020]/g, ''));
}

function cellHyperlink(cell) {
  if (!cell.getElementsByTagName) {
    const target = cell.hyperlinkTarget;
    return externalHtmlLink(target) ? { Target: target } : undefined;
  }
  for (const anchor of cell.getElementsByTagName('a')) {
    const target = anchor.getAttribute('href');
    if (target == null || target.startsWith('#')) continue;
    return externalHtmlLink(target) ? { Target: target } : undefined;
  }
}

export function joinHtmlCellText(fragments) {
  const lines = [''];
  for (const fragment of fragments) {
    if (fragment === null) lines.push('');
    else lines[lines.length - 1] += fragment;
  }
  const normalized = lines.map((line, index) => {
    const compact = line.replace(/[\t\n\r ]+/g, ' ').replace(/^ /, '');
    return index === lines.length - 1 ? compact.replace(/ $/, '') : compact.replace(/\b $/, '');
  });
  return normalized.join('\n').replaceAll('\u00a0', ' ');
}

function textContent(cell) {
  if (!cell.childNodes) return cell.textContent ?? '';
  const fragments = [];
  const pending = [cell];
  while (pending.length) {
    const node = pending.pop();
    if (node.nodeType === 3) fragments.push(node.nodeValue ?? '');
    else if (node.nodeName === 'BR') fragments.push(null);
    else for (let index = node.childNodes.length - 1; index >= 0; index -= 1) pending.push(node.childNodes[index]);
  }
  return joinHtmlCellText(fragments);
}

export function sheet_add_dom(sheet, table, options = {}) {
  options ??= {};
  if (table?.tagName !== 'TABLE') throw new Error('Expected an HTML TABLE');
  const { origin, sheetRows, display, raw, cellDates, UTC, dateNF, cellText } = sheetDomSettings(options);
  const typedValues = { raw, cellDates, UTC, dateNF: dateNF || undefined, cellText, cellNF: true };
  const textValues = { ...typedValues, raw: true };
  const start = typeof origin === 'string' ? decode_cell(origin) :
    typeof origin === 'number' ? { r: origin, c: 0 } : origin ?? { r: 0, c: 0 };
  const previous = sheet['!ref'] ? decode_range(sheet['!ref']) : undefined;
  const top = start.r === -1 && previous ? previous.e.r + 1 : start.r;
  const end = { r: Math.max(top, previous?.e.r ?? top), c: Math.max(start.c, previous?.e.c ?? start.c) };
  const dense = sheet['!data'];
  const rows = sheet['!rows'] ??= [];
  sheet['!cols'] ??= [];
  const merges = [];
  const limit = Math.min(sheetRows, table.rows.length);
  let visibleRow = 0;
  let sourceRow = 0;
  for (; sourceRow < table.rows.length && visibleRow < limit; sourceRow += 1) {
    const row = table.rows[sourceRow];
    const hidden = isHidden(row);
    if (hidden && display) continue;
    if (hidden) rows[visibleRow] = { hidden: true };
    const r = top + visibleRow;
    let c = start.c;
    for (const cell of row.cells) {
      if (display && isHidden(cell)) continue;
      while (true) {
        const above = merges.find(({ s, e }) => s.r < r && e.r >= r && s.c <= c && e.c >= c);
        if (!above) break;
        c = above.e.c + 1;
      }
      const rowSpan = cell.rowSpan || 1;
      const colSpan = cell.colSpan || 1;
      if (rowSpan > 1 || colSpan > 1) {
        merges.push({ s: { r, c }, e: { r: r + rowSpan - 1, c: c + colSpan - 1 } });
      }
      const text = cell.getAttribute('data-v') ?? cell.getAttribute('v') ?? textContent(cell);
      const type = cell.getAttribute('data-t') || cell.getAttribute('t');
      const error = type === 'e' && text.trim() && !raw ? errorName(Number(text)) : undefined;
      const value = text === '' ? { t: type || 'z', v: text } : error
        ? { t: 'e', v: Number(text), w: error }
        : cellFromText(text, type === 's' ? textValues : typedValues);
      const format = cell.getAttribute('data-z') || cell.getAttribute('z');
      if (value.z === undefined && format != null) value.z = format;
      const link = cellHyperlink(cell);
      if (link) value.l = link;
      if (dense != null) {
        dense[r] ??= [];
        dense[r][c] = value;
      } else {
        sheet[encode_cell({ r, c })] = value;
      }
      end.c = Math.max(end.c, c);
      c += colSpan;
    }
    visibleRow += 1;
  }
  if (merges.length) sheet['!merges'] = [...(sheet['!merges'] ?? []), ...merges];
  end.r = Math.max(end.r, top + visibleRow - 1);
  const first = { r: Math.min(0, previous?.s.r ?? 0), c: Math.min(0, previous?.s.c ?? 0) };
  sheet['!ref'] = encode_range({ s: first, e: end });
  if (visibleRow >= limit) {
    sheet['!fullref'] = encode_range({ s: first, e: { r: top + visibleRow + table.rows.length - sourceRow - 1, c: end.c } });
  }
  return sheet;
}

export function table_to_sheet(table, options) {
  return sheet_add_dom(options?.dense ? { '!data': [] } : {}, table, options);
}

export function table_to_book(table, options) {
  return book_new(table_to_sheet(table, options), options?.sheet);
}

let parseNodeHtmlTables;

export function setHtmlParser(parse) {
  parseNodeHtmlTables = parse;
}

export function htmlTables(text) {
  if (parseNodeHtmlTables) return parseNodeHtmlTables(text);
  if (typeof document === 'undefined') {
    throw failure('UNSUPPORTED_ENVIRONMENT', 'HTML import requires an HTML parser');
  }
  const template = document.createElement('template');
  template.innerHTML = text;
  return Array.from(template.content.querySelectorAll('table'))
    .filter((table) => !table.parentElement?.closest('table'));
}
