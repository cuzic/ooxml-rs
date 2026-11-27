import { failure } from './contract.js';

export let codepageTable;

export function set_cptable(table) {
  if (table !== undefined && typeof table?.utils?.decode !== 'function') {
    throw failure('INVALID_VALUE', 'Expected codepage utilities with a decode method');
  }
  codepageTable = table;
}
