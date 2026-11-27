import { address, failure } from './contract.js';
import { cellModelSettings, isWorksheetGridColumn, isWorksheetGridRow } from './exceljs-options.generated.js';
import { ValueType } from './exceljs-cell-value.js';
import { encode_col } from './sheetjs-address.js';

export function gridAddress(row, column) {
  if (!isWorksheetGridRow(row) || !isWorksheetGridColumn(column)) {
    throw failure('INVALID_ADDRESS', 'Expected one-based XLSX row and column numbers');
  }
  return encode_col(column - 1) + row;
}

export function* rowModelCells(row, cells, decodeAddress = address, readModel = cellModelSettings) {
  let previousColumn;
  for (const input of cells) {
    const model = readModel(input);
    if (model.type === ValueType.Merge) continue;
    let column;
    if (model.address !== undefined) {
      const [, targetRow, targetColumn] = decodeAddress(model.address);
      if (targetRow !== row - 1) throw failure('INVALID_ADDRESS', 'Cell model address must match the target row');
      column = targetColumn + 1;
    } else {
      if (previousColumn === undefined) throw failure('INVALID_ADDRESS', 'Expected the first cell model address');
      column = previousColumn + 1;
    }
    gridAddress(row, column);
    previousColumn = column;
    yield { model, column };
  }
}

export function rowCellColumn(row, location) {
  if (!location || location.row !== row || address(location.address)[0] !== gridAddress(row, location.col)) {
    throw failure('INVALID_ADDRESS', 'Expected an address within this row');
  }
  return location.col;
}

// `empty` marks a sheet without cells: the A1 range with an all-zero model, as 4.4.0's Range reports it.
export function worksheetDimensions(top, left, bottom, right, empty = false) {
  const tl = gridAddress(top, left);
  const br = gridAddress(bottom, right);
  const $tl = '$' + encode_col(left - 1) + '$' + top;
  const $br = '$' + encode_col(right - 1) + '$' + bottom;
  const range = `${tl}:${br}`;
  const absolute = `${$tl}:${$br}`;
  const count = (bottom - top + 1) * (right - left + 1);
  return {
    top, left, bottom, right, tl, br, $t$l: $tl, $b$r: $br, range, $range: absolute, count,
    shortRange: count === 1 ? tl : range,
    $shortRange: count === 1 ? $tl : absolute,
    model: empty ? { top: 0, left: 0, bottom: 0, right: 0 } : { top, left, bottom, right },
  };
}

export function worksheetCellAddress(reference, column) {
  if (column !== undefined) {
    const coordinate = (value) => typeof value === 'string' && /^\d+$/.test(value) ? Number(value) : value;
    return address(gridAddress(coordinate(reference), coordinate(column)));
  }
  if (typeof reference === 'string' && /^\$?[a-z]+\$?\d+$/i.test(reference)) {
    return address(reference.replaceAll('$', ''));
  }
  return address(reference);
}

export function mergeReference(cells) {
  let seen;
  while (cells.length === 1 && Array.isArray(cells[0])) {
    seen ??= new Set();
    if (seen.has(cells)) throw failure('INVALID_ADDRESS', 'Circular merge range arguments');
    seen.add(cells);
    cells = cells[0];
  }
  let first;
  let last;
  if (cells.length === 4 || (cells.length === 5 && (cells[4] === undefined || typeof cells[4] === 'string'))) {
    first = gridAddress(cells[0], cells[1]);
    last = gridAddress(cells[2], cells[3]);
  } else if (cells.length === 2 || (cells.length === 3 && (cells[2] === undefined || typeof cells[2] === 'string'))) {
    [first, last] = cells;
  } else if (cells.length === 1 && typeof cells[0] === 'string') {
    const reference = cells[0].replace(/^(?:'(?:[^']|'')+'|[^'!:\s]+)!/, '');
    [first, last] = reference.split(':');
    last ??= first;
  } else if (cells.length === 1 && cells[0] && typeof cells[0] === 'object' && !Array.isArray(cells[0])) {
    const { top, left, bottom, right } = cells[0];
    first = gridAddress(top, left);
    last = gridAddress(bottom, right);
  }
  const [, firstRow, firstCol] = worksheetCellAddress(first);
  const [, lastRow, lastCol] = worksheetCellAddress(last);
  return `${gridAddress(Math.min(firstRow, lastRow) + 1, Math.min(firstCol, lastCol) + 1)}:` +
    gridAddress(Math.max(firstRow, lastRow) + 1, Math.max(firstCol, lastCol) + 1);
}
