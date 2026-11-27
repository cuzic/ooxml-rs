import { address, data, failure } from './contract.js';
import { decode_range } from './sheetjs-address.js';

import { marginKeys, worksheetAutofilterSettings, worksheetPrintMarginPatch } from './sheetjs-options.generated.js';
export { marginKeys };

export function marginEdits(sheet, original) {
  const values = data(sheet, '!margins');
  if (values == null) return undefined;
  return worksheetPrintMarginPatch(values, original);
}

export function readAutofilter(sheet) {
  if (!Object.hasOwn(sheet, '!autofilter')) return undefined;
  const descriptor = Object.getOwnPropertyDescriptor(sheet, '!autofilter');
  if (!Object.hasOwn(descriptor, 'value')) {
    throw failure('INVALID_VALUE', 'Expected worksheet autofilter data');
  }
  return descriptor.value == null ? undefined : worksheetAutofilterSettings(descriptor.value).ref;
}


export function autofilterRange(sheet, lastRow) {
  const filter = readAutofilter(sheet);
  if (filter === undefined) return undefined;
  const endpoints = filter.split(':');
  if (endpoints.length > 2) throw failure('INVALID_VALUE', 'Invalid autofilter range');
  const area = decode_range(endpoints.map(endpoint => address(endpoint.replaceAll('$', ''))[0]).join(':'));
  if (area.s.r > area.e.r || area.s.c > area.e.c) throw failure('INVALID_VALUE', 'Invalid autofilter range');
  if (area.s.r === area.e.r) area.e.r = Math.max(area.s.r, lastRow);
  return area;
}
