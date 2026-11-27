import { parseFragment } from 'parse5';
import { joinHtmlCellText } from './sheetjs-dom.js';

const children = node => node.childNodes ?? [];
const attribute = (node, key) => node.attrs.find(attr => attr.name === key)?.value ?? null;

function cellContent(node) {
  const parts = [];
  const pending = [node];
  let hyperlinkTarget;
  while (pending.length) {
    const child = pending.pop();
    if (child.nodeName === '#text') parts.push(child.value);
    else if (child.tagName === 'br') parts.push(null);
    else {
      if (hyperlinkTarget === undefined && child.tagName === 'a') {
        const href = attribute(child, 'href');
        if (href !== null && !href.startsWith('#')) hyperlinkTarget = href;
      }
      const nodes = children(child);
      for (let index = nodes.length - 1; index >= 0; index -= 1) pending.push(nodes[index]);
    }
  }
  return { textContent: joinHtmlCellText(parts), hyperlinkTarget };
}

function span(node, key, max) {
  const match = /^\s*\+?(\d+)/.exec(attribute(node, key) ?? '');
  return match ? Math.min(Number(match[1]), max) : 1;
}

function style(node) {
  const inline = attribute(node, 'style') ?? '';
  const display = [...inline.matchAll(/(?:^|;)\s*display\s*:\s*([^;]+)/gi)].at(-1)?.[1];
  return { display: display?.replace(/\s*!important\s*$/i, '').trim().toLowerCase() };
}

function rows(table) {
  const bySection = section => children(table).filter(node => node.tagName === section)
    .flatMap(node => children(node).filter(row => row.tagName === 'tr'));
  const body = children(table).flatMap(node => node.tagName === 'tr' ? [node]
    : node.tagName === 'tbody' ? children(node).filter(row => row.tagName === 'tr') : []);
  return [...bySection('thead'), ...body, ...bySection('tfoot')].map(row => ({
    style: style(row),
    hidden: attribute(row, 'hidden') !== null,
    cells: children(row).filter(cell => cell.tagName === 'td' || cell.tagName === 'th').map(cell => ({
      ...cellContent(cell),
      style: style(cell),
      hidden: attribute(cell, 'hidden') !== null,
      rowSpan: span(cell, 'rowspan', 65534),
      colSpan: span(cell, 'colspan', 1000),
      getAttribute: key => attribute(cell, key),
    })),
  }));
}

export function parseHtmlTables(text) {
  const tables = [];
  const pending = [parseFragment(text)];
  while (pending.length) {
    const node = pending.pop();
    if (node.tagName === 'table') {
      tables.push({ tagName: 'TABLE', rows: rows(node) });
      continue;
    }
    const nodes = children(node);
    for (let index = nodes.length - 1; index >= 0; index -= 1) pending.push(nodes[index]);
  }
  return tables;
}
