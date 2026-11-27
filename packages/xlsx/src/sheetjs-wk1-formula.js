import { encode_col, encode_row } from './sheetjs-address.js';

const operators = ['+', '-', '*', '/', '^', '=', '<>', '<=', '>=', '<', '>'];
const functions = {
  0x1f: ['NA', 0], 0x21: ['ABS', 1], 0x22: ['TRUNC', 1], 0x23: ['SQRT', 1],
  0x24: ['LOG', 1], 0x25: ['LN', 1], 0x26: ['PI', 0], 0x27: ['SIN', 1],
  0x28: ['COS', 1], 0x29: ['TAN', 1], 0x2a: ['ATAN2', 2], 0x2b: ['ATAN', 1],
  0x2c: ['ASIN', 1], 0x2d: ['ACOS', 1], 0x2e: ['EXP', 1], 0x2f: ['MOD', 2],
  0x31: ['ISNA', 1], 0x32: ['ISERR', 1], 0x33: ['FALSE', 0], 0x34: ['TRUE', 0],
  0x35: ['RAND', 0], 0x36: ['DATE', 3], 0x3f: ['ROUND', 2], 0x40: ['TIME', 3],
  0x44: ['ISNUMBER', 1], 0x45: ['ISTEXT', 1], 0x46: ['LEN', 1], 0x47: ['VALUE', 1],
  0x49: ['MID', 3], 0x4a: ['CHAR', 1], 0x50: ['SUM'], 0x51: ['AVERAGEA'],
  0x52: ['COUNTA'], 0x53: ['MINA'], 0x54: ['MAXA'], 0x66: ['UPPER', 1],
  0x67: ['LOWER', 1], 0x6b: ['PROPER', 1], 0x6d: ['TRIM', 1], 0x6f: ['T', 1],
};

function coordinate(word, base, encode, limit) {
  const relative = Boolean(word & 0x8000);
  const value = word & 0x7fff;
  const index = (relative ? base : 0) + (value >= 0x2000 ? value - 0x4000 : value);
  if (index < 0 || index >= limit) return undefined;
  return (relative ? '' : '$') + encode(index);
}

export function wk1Formula(bytes, row, column, decodeText) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const stack = [];
  let offset = 0;
  function address() {
    const col = coordinate(view.getUint16(offset, true), column, encode_col, 16384);
    const line = coordinate(view.getUint16(offset + 2, true), row, encode_row, 1048576);
    offset += 4;
    return col && line ? col + line : '#REF!';
  }

  while (offset < bytes.length) {
    const op = bytes[offset++];
    if (op === 3) return offset === bytes.length && stack.length === 1 ? stack[0] : undefined;
    if (op === 0) {
      if (offset + 8 > bytes.length) return undefined;
      const value = view.getFloat64(offset, true);
      if (!Number.isFinite(value)) return undefined;
      stack.push(String(value));
      offset += 8;
    } else if (op === 1 || op === 2) {
      if (offset + op * 4 > bytes.length) return undefined;
      const start = address();
      stack.push(op === 1 ? start : start + ':' + address());
    } else if (op === 5) {
      if (offset + 2 > bytes.length) return undefined;
      stack.push(String(view.getUint16(offset, true)));
      offset += 2;
    } else if (op === 6) {
      const end = bytes.indexOf(0, offset);
      if (end < 0) return undefined;
      stack.push('"' + decodeText(bytes.subarray(offset, end)).replaceAll('"', '""') + '"');
      offset = end + 1;
    } else if ([4, 8, 0x16, 0x17].includes(op)) {
      if (!stack.length) return undefined;
      const argument = stack.pop();
      stack.push(op === 4 ? '(' + argument + ')' : op === 8 ? '-' + argument
        : op === 0x16 ? 'NOT(' + argument + ')' : '+' + argument);
    } else if ((op >= 9 && op <= 0x15) || op === 0x18) {
      if (stack.length < 2) return undefined;
      const right = stack.pop();
      const left = stack.pop();
      if (op === 0x14 || op === 0x15) stack.push((op === 0x14 ? 'AND(' : 'OR(') + left + ',' + right + ')');
      else stack.push(left + (op === 0x18 ? '&' : operators[op - 9]) + right);
    } else if (Object.hasOwn(functions, op)) {
      const [name, fixedCount] = functions[op];
      const count = fixedCount ?? bytes[offset++];
      if (count === undefined || count > stack.length) return undefined;
      const arguments_ = stack.splice(stack.length - count, count);
      stack.push(name + '(' + arguments_.join(',') + ')');
    } else return undefined;
  }
  return stack.length === 1 ? stack[0] : undefined;
}
