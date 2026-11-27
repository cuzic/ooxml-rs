import { failure } from './contract.js';
import { encode_col, encode_row } from './sheetjs-address.js';
import { errorName } from './sheetjs-csv-input.js';

const binary = ['+', '-', '*', '/', '^', '&', '<', '<=', '=', '>=', '>', '<>', ' ', ',', ':'];
const functions = new Map([
  [0, ['COUNT']], [1, ['IF']], [4, ['SUM']], [5, ['AVERAGE']], [6, ['MIN']], [7, ['MAX']],
  [8, ['ROW']], [9, ['COLUMN']], [10, ['NA', 0]], [15, ['SIN', 1]], [16, ['COS', 1]],
  [17, ['TAN', 1]], [19, ['PI', 0]], [20, ['SQRT', 1]], [24, ['ABS', 1]], [25, ['INT', 1]],
  [27, ['ROUND', 2]], [34, ['TRUE', 0]], [35, ['FALSE', 0]], [36, ['AND']], [37, ['OR']],
  [38, ['NOT', 1]], [39, ['MOD', 2]], [65, ['DATE', 3]], [66, ['TIME', 3]],
  [67, ['DAY', 1]], [68, ['MONTH', 1]], [69, ['YEAR', 1]], [74, ['NOW', 0]],
  [76, ['ROWS', 1]], [77, ['COLUMNS', 1]], [100, ['CHOOSE']], [101, ['HLOOKUP']],
  [102, ['VLOOKUP']], [111, ['CHAR', 1]], [112, ['LOWER', 1]], [113, ['UPPER', 1]],
  [115, ['LEFT']], [116, ['RIGHT']], [118, ['TRIM', 1]], [169, ['COUNTA']],
  [221, ['TODAY', 0]], [337, ['POWER', 2]], [345, ['SUMIF']], [346, ['COUNTIF', 2]],
]);

export function biffFormula(bytes, version, decode, base, resolveSheet, resolveName) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const stack = [];
  let offset = 0;
  const word = () => { const value = view.getUint16(offset, true); offset += 2; return value; };
  const byte = () => view.getUint8(offset++);
  const xlsb = version === 12;
  const wide = version >= 8;
  const rowNumber = () => {
    if (!xlsb) return word();
    const value = view.getUint32(offset, true);
    offset += 4;
    return value;
  };
  function address(row, column, relative) {
    const flags = wide ? column : row;
    const rowRelative = Boolean(flags & 0x8000);
    const columnRelative = Boolean(flags & 0x4000);
    const rowLimit = xlsb ? 1048576 : wide ? 65536 : 16384;
    const columnLimit = wide ? 16384 : 256;
    let r = row & (rowLimit - 1);
    let c = column & (columnLimit - 1);
    if (relative) {
      if (!base) return '#REF!';
      if (rowRelative) r = (r + base.row) % rowLimit;
      if (columnRelative) c = (c + base.column) % columnLimit;
    }
    if (!xlsb && c > 255) return '#REF!';
    return (columnRelative ? '' : '$') + encode_col(c) + (rowRelative ? '' : '$') + encode_row(r);
  }
  function call(name, count) {
    if (count === undefined || count > stack.length) return false;
    stack.push(name + '(' + stack.splice(stack.length - count, count).join(',') + ')');
    return true;
  }
  try {
    while (offset < bytes.length) {
      const raw = byte();
      const op = raw < 0x20 ? raw : 0x20 | (raw & 0x1f);
      if (op >= 3 && op <= 0x11) {
        if (stack.length < 2) return undefined;
        let right = stack.pop();
        const left = stack.pop();
        if (xlsb && op === 0x11) {
          const firstSheet = left.lastIndexOf('!');
          const lastSheet = right.lastIndexOf('!');
          if (firstSheet > 0 && lastSheet > 0 &&
              left.slice(0, firstSheet).toLowerCase() === right.slice(0, lastSheet).toLowerCase()) {
            right = right.slice(lastSheet + 1);
          }
        }
        stack.push(left + binary[op - 3] + right);
      } else if (op >= 0x12 && op <= 0x15) {
        if (!stack.length) return undefined;
        const value = stack.pop();
        stack.push(op === 0x12 ? '+' + value : op === 0x13 ? '-' + value : op === 0x14 ? value + '%' : '(' + value + ')');
      } else if (op === 0x16) stack.push('');
      else if (op === 0x1c) stack.push(errorName(byte()) ?? '#VALUE!');
      else if (op === 0x1d) stack.push(byte() ? 'TRUE' : 'FALSE');
      else if (op === 0x1e) stack.push(String(word()));
      else if (op === 0x1f) {
        const number = view.getFloat64(offset, true);
        if (!Number.isFinite(number)) return undefined;
        stack.push(String(number));
        offset += 8;
      } else if (op === 0x17) {
        const count = xlsb ? word() : byte();
        const width = xlsb || (wide && (byte() & 1)) ? 2 : 1;
        if (offset + count * width > bytes.length) return undefined;
        let text;
        if (!wide) text = decode(bytes.subarray(offset, offset + count));
        else text = Array.from({ length: count }, (_, i) => String.fromCharCode(width === 2
          ? view.getUint16(offset + i * 2, true) : bytes[offset + i])).join('');
        offset += count * width;
        stack.push('"' + text.replaceAll('"', '""') + '"');
      } else if (op === 0x23 && resolveName) {
        const index = wide ? view.getUint32(offset, true) : view.getUint16(offset, true);
        offset += wide ? 4 : 14;
        if (offset > bytes.length) return undefined;
        stack.push(resolveName(index));
      } else if ([0x24, 0x25, 0x2c, 0x2d, 0x3a, 0x3b].includes(op)) {
        const external = op === 0x3a || op === 0x3b;
        const sheet = external ? resolveSheet?.(word()) : undefined;
        if (external && (!wide || sheet === undefined)) return undefined;
        const prefix = external ? sheet + '!' : '';
        const top = rowNumber();
        const relative = op === 0x2c || op === 0x2d;
        if ([0x24, 0x2c, 0x3a].includes(op)) stack.push(prefix + address(top, wide ? word() : byte(), relative));
        else {
          const bottom = rowNumber();
          const left = wide ? word() : byte();
          const right = wide ? word() : byte();
          stack.push(prefix + address(top, left, relative) + ':' + address(bottom, right, relative));
        }
      } else if (op === 0x21 || op === 0x22) {
        const argc = op === 0x22 ? byte() : undefined;
        const info = functions.get(word());
        if (!info || !call(info[0], argc ?? info[1])) return undefined;
      } else if (op === 0x19) {
        const attr = byte();
        const count = word();
        if (attr & 0x04) offset += (count + 1) * 2;
        if ((attr & 0x10) && !call('SUM', 1)) return undefined;
        if (offset > bytes.length) return undefined;
      } else return undefined;
    }
  } catch (error) {
    if (error instanceof RangeError) return undefined;
    throw error;
  }
  return stack.length === 1 && stack[0] ? stack[0] : undefined;
}

const functionIds = new Map([...functions].map(([id, [name, count]]) => [name, [id, count]]));
const precedence = new Map([
  ['=', 1], ['<>', 1], ['<', 1], ['<=', 1], ['>', 1], ['>=', 1], ['&', 2],
  ['+', 3], ['-', 3], ['*', 4], ['/', 4], ['^', 5], [':', 8],
]);

function reference(token, version) {
  const match = /^(\$?)([A-Z]{1,3})(\$?)([1-9]\d*)$/i.exec(token);
  if (!match) return undefined;
  const [, fixedColumn, letters, fixedRow, line] = match;
  let column = 0;
  for (const letter of letters.toUpperCase()) column = column * 26 + letter.charCodeAt(0) - 64;
  column -= 1;
  const row = Number(line) - 1;
  if (column > (version === 12 ? 16383 : 255) || row > (version === 12 ? 1048575 : version === 8 ? 65535 : 16383)) return undefined;
  const flags = (fixedColumn ? 0 : 0x4000) | (fixedRow ? 0 : 0x8000);
  return version >= 8 ? [row, column | flags] : [row | flags, column];
}

const tokenPattern = /\s*(?:('(?:[^']|'')+'!|[\p{L}_][\p{L}\p{N}_.]*!)|(\d+(?:\.\d*)?(?:E[+-]?\d+)?|\.\d+(?:E[+-]?\d+)?)|("(?:[^"]|"")*")|(\$?[\p{L}_][\p{L}\p{N}_.$]*)|(<=|>=|<>|[()+\-*/^&%=<>,:]))/iuy;

export function compileBiffFormula(source, version, sheetIndex, resolveName) {
  if (typeof source !== 'string' || source.length > 8192) return undefined;
  const tokens = [];
  const text = source.replace(/^=/, '').trim();
  const lexer = new RegExp(tokenPattern);
  while (lexer.lastIndex < text.length) {
    const match = lexer.exec(text);
    if (!match) return undefined;
    tokens.push(match[1] ?? match[2] ?? match[3] ?? match[4] ?? match[5]);
  }
  const output = [];
  let position = 0;
  const word = value => output.push(value & 255, value >> 8);
  const column = value => version >= 8 ? word(value) : output.push(value);
  const rowNumber = value => {
    word(value & 65535);
    if (version === 12) word(value >>> 16);
  };
  function expression(minimum, depth) {
    if (depth > 128) return false;
    const token = tokens[position++];
    if (!token) return false;
    if (token === '+' || token === '-') {
      if (!expression(7, depth + 1)) return false;
      output.push(token === '+' ? 0x12 : 0x13);
    } else if (token === '(') {
      if (!expression(1, depth + 1) || tokens[position++] !== ')') return false;
      output.push(0x15);
    } else if (/^[\d.]/.test(token)) {
      const value = Number(token);
      if (!Number.isFinite(value)) return false;
      if (Number.isInteger(value) && value <= 65535) {
        output.push(0x1e);
        word(value);
      } else {
        const view = new DataView(new ArrayBuffer(8));
        view.setFloat64(0, value, true);
        output.push(0x1f, ...new Uint8Array(view.buffer));
      }
    } else if (token.startsWith('"')) {
      const value = token.slice(1, -1).replaceAll('""', '"');
      if (value.length > 255 || (version < 8 && /[^\x00-\x7f]/.test(value))) return false;
      output.push(0x17);
      if (version === 12) word(value.length);
      else output.push(value.length);
      if (version === 8) output.push(1);
      for (let index = 0; index < value.length; index += 1) column(value.charCodeAt(index));
    } else if (tokens[position] === '(') {
      const info = functionIds.get(token.toUpperCase());
      if (!info) return false;
      position += 1;
      let count = 0;
      if (tokens[position] !== ')') {
        do {
          if (++count > 255 || !expression(1, depth + 1)) return false;
          if (tokens[position] !== ',') break;
          position += 1;
        } while (true);
      }
      if (tokens[position++] !== ')' || (info[1] !== undefined && info[1] !== count)) return false;
      output.push(0x42, count);
      word(info[0]);
    } else if (/^(?:TRUE|FALSE)$/i.test(token)) {
      output.push(0x1d, Number(token.toUpperCase() === 'TRUE'));
    } else {
      const qualified = token.endsWith('!');
      const label = qualified ? token.slice(0, -1) : undefined;
      const sheet = label?.startsWith("'") ? label.slice(1, -1).replaceAll("''", "'") : label;
      const operand = qualified ? tokens[position++] ?? '' : token;
      const from = reference(operand, version);
      const index = from && qualified && version >= 8 ? sheetIndex?.(sheet) : undefined;
      if (from && qualified && index === undefined) return false;
      if (!from) {
        const name = resolveName?.(operand, sheet, output.length + 1);
        if (name === undefined) return false;
        output.push(0x43); // PtgName
        word(name & 65535);
        word(name >>> 16);
      } else if (tokens[position] === ':') {
        position += 1;
        const to = reference(tokens[position++] ?? '', version);
        if (!to) return false;
        output.push(qualified ? 0x5b : 0x45);
        if (qualified) word(index);
        rowNumber(from[0]);
        rowNumber(to[0]);
        column(from[1]);
        column(to[1]);
      } else {
        output.push(qualified ? 0x5a : 0x44);
        if (qualified) word(index);
        rowNumber(from[0]);
        column(from[1]);
      }
    }
    while (position < tokens.length) {
      const next = tokens[position];
      if (next === '%' && minimum <= 6) {
        position += 1;
        output.push(0x14);
        continue;
      }
      const rank = precedence.get(next);
      if (!rank || rank < minimum || next === ':') break;
      position += 1;
      if (!expression(rank + 1, depth + 1)) return false;
      output.push(3 + binary.indexOf(next));
    }
    return true;
  }
  return expression(1, 0) && position === tokens.length && output.length <= 8192
    ? Uint8Array.from(output) : undefined;
}

export function compileBinaryNames(candidates, version, sheetIndex, resolveName, maxTokens, error) {
  const omitted = [];
  for (const candidate of candidates) {
    const { sheet, ref } = candidate;
    let tokens;
    if (Number.isInteger(ref) && ref >= 0 && ref <= 65535) {
      tokens = Uint8Array.of(0x1e, ref & 255, ref >>> 8); // PtgInt
    } else if (typeof ref === 'number' && Number.isFinite(ref)) {
      tokens = new Uint8Array(9);
      tokens[0] = 0x1f; // PtgNum
      new DataView(tokens.buffer).setFloat64(1, ref, true);
    } else {
      tokens = ref === '' && version === 8 ? new Uint8Array() : ref === '#REF!' && version === 12 ? Uint8Array.of(0x1c, 0x17) : compileBiffFormula(ref, version, sheetIndex, (name, qualifiedSheet, offset) => {
        const dependency = resolveName(name, sheet, qualifiedSheet);
        if (!dependency) return undefined;
        candidate.dependencies.push([offset, dependency]);
        dependency.dependents.push(candidate);
        return 0; // Filled once the surviving labels have their final indices.
      });
    }
    if (!tokens || tokens.length > maxTokens(candidate)) {
      if (error) throw failure('INVALID_VALUE', error);
      omitted.push(candidate);
    } else {
      candidate.tokens = tokens;
    }
  }
  for (const candidate of omitted) {
    for (const dependent of candidate.dependents) {
      if (dependent.tokens) {
        dependent.tokens = undefined;
        omitted.push(dependent);
      }
    }
  }
  const included = candidates.filter(candidate => candidate.tokens);
  included.forEach((candidate, index) => { candidate.index = index + 1; });
  for (const { tokens, dependencies } of included) {
    const view = new DataView(tokens.buffer, tokens.byteOffset, tokens.byteLength);
    for (const [offset, dependency] of dependencies) view.setUint32(offset, dependency.index, true);
  }
  return included;
}
