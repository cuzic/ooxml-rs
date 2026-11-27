export function encode_row(row) {
  return String(row + 1);
}

export function decode_row(text) {
  return Number.parseInt(text.replace(/\$(?=\d+$)/, ''), 10) - 1;
}

export function encode_col(column) {
  if (column < 0) throw new Error(`invalid column ${column}`);
  if (!Number.isFinite(column)) return '';
  let label = '';
  let remaining = column + 1;
  while (remaining > 0) {
    remaining -= 1;
    label = String.fromCharCode(65 + remaining % 26) + label;
    remaining = Math.floor(remaining / 26);
  }
  return label;
}

export function decode_col(text) {
  let value = 0;
  const label = text.replace(/^\$(?=[A-Z])/, '');
  for (let index = 0; index < label.length; index += 1) {
    value = value * 26 + label.charCodeAt(index) - 64;
  }
  return value - 1;
}

export function encode_cell({ c, r }) {
  return (c === -1 ? '' : encode_col(c)) + encode_row(r);
}

export function decode_cell(text) {
  let row = 0;
  let column = 0;
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index);
    if (code >= 48 && code <= 57) {
      row = row * 10 + (code - 48);
    } else if (code >= 65 && code <= 90) {
      column = column * 26 + (code - 64);
    }
  }
  return { c: column - 1, r: row - 1 };
}

export function split_cell(text) {
  const [prefix, column, row] = /^(\$?[A-Z]*)(\$?\d*)/.exec(text);
  return [column, ...(row + text.slice(prefix.length)).split(',')];
}

export function encode_range(start, end) {
  if (end === undefined || typeof end === 'number') ({ s: start, e: end } = start);
  const first = typeof start === 'string' ? start : encode_cell(start);
  const last = typeof end === 'string' ? end : encode_cell(end);
  return first === last ? first : `${first}:${last}`;
}

export function decode_range(text) {
  const separator = text.indexOf(':');
  if (separator < 0) {
    const point = decode_cell(text);
    return { s: point, e: { ...point } };
  }
  return { s: decode_cell(text.slice(0, separator)), e: decode_cell(text.slice(separator + 1)) };
}
