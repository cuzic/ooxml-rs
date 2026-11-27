import CFB from '../wasm/cfb-browser.js';
import { setCompoundFormat } from './sheetjs-xls.js';
import sheetjs from './sheetjs.js';

export * from './sheetjs.js';
export { CFB };

export function read(input, options) {
  setCompoundFormat(CFB);
  return sheetjs.read(input, options);
}

export function readFile(filename, options) {
  setCompoundFormat(CFB);
  return sheetjs.readFile(filename, options);
}

export { readFile as readFileSync };

export function parse_zip(container, options) {
  setCompoundFormat(CFB);
  return sheetjs.parse_zip(container, options);
}

export function write(workbook, options) {
  setCompoundFormat(CFB);
  return sheetjs.write(workbook, options);
}

export function writeFile(workbook, filename, options) {
  setCompoundFormat(CFB);
  return sheetjs.writeFile(workbook, filename, options);
}

export { writeFile as writeFileSync };

export function writeFileAsync(filename, workbook, options, callback) {
  setCompoundFormat(CFB);
  return sheetjs.writeFileAsync(filename, workbook, options, callback);
}

export default {
  ...sheetjs, read, readFile, readFileSync: readFile, parse_zip, write, writeFile, writeFileSync: writeFile, writeFileAsync, CFB,
};
