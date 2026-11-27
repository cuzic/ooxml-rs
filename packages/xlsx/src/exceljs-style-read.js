import { borderSides } from './exceljs-options.generated.js';

export function decodeStyleColor(color) {
  const [kind, value, tint] = color.split(':');
  if (kind === 'rgb') return { argb: value };
  if (kind === 'indexed') return { indexed: Number(value) };
  if (kind === 'theme') {
    const result = { theme: Number(value) };
    if (Number(tint)) result.tint = Number(tint);
    return result;
  }
  return undefined;
}

export function decodeBorder(values) {
  if (!values.length) return undefined;
  const border = {};
  for (const [index, key] of borderSides.entries()) {
    const [style, encodedColor] = values.slice(2 + index * 2, 4 + index * 2);
    const diagonal = key === 'diagonal';
    const up = diagonal && values[0] === 'true';
    const down = diagonal && values[1] === 'true';
    if (!style && !up && !down) continue;
    const side = {};
    if (style) side.style = style;
    const color = decodeStyleColor(encodedColor);
    if (color) side.color = color;
    if (diagonal) Object.assign(side, { up, down });
    border[key] = side;
  }
  return border;
}

export function decodeFill(values) {
  if (!values.length) return undefined;
  if (values[0] === 'angle' || values[0] === 'path') {
    const [gradient, first, second, third, fourth] = values;
    const fill = { type: 'gradient', gradient, stops: [] };
    if (gradient === 'angle') fill.degree = Number(first);
    else {
      fill.center = { left: Number(first), top: Number(second) };
      if (third !== first) fill.center.right = Number(third);
      if (fourth !== second) fill.center.bottom = Number(fourth);
    }
    for (let index = gradient === 'angle' ? 2 : 5; index < values.length; index += 2) {
      fill.stops.push({ position: Number(values[index]), color: decodeStyleColor(values[index + 1]) ?? {} });
    }
    return fill;
  }
  const [pattern, foreground, background] = values;
  const fill = { type: 'pattern', pattern };
  for (const [key, color] of [['fgColor', foreground], ['bgColor', background]]) {
    const decoded = decodeStyleColor(color);
    if (decoded) fill[key] = decoded;
  }
  return fill;
}

export function decodeProtection(flags) {
  return flags.length ? { locked: Boolean(flags[0]), hidden: Boolean(flags[1]) } : undefined;
}

export function decodeAlignment(attributes) {
  if (!attributes.length) return undefined;
  const alignment = {};
  for (let index = 0; index < attributes.length; index += 2) {
    const [key, value] = attributes.slice(index, index + 2);
    switch (key) {
      case 'horizontal': alignment.horizontal = value; break;
      case 'vertical': alignment.vertical = value === 'center' ? 'middle' : value; break;
      case 'textRotation': {
        const rotation = Number(value);
        alignment.textRotation = rotation === 255 ? 'vertical' : rotation > 90 ? 90 - rotation : rotation;
        break;
      }
      case 'wrapText':
      case 'shrinkToFit': alignment[key] = value === '1' || value === 'true'; break;
      case 'indent': alignment.indent = Number(value); break;
      case 'readingOrder':
        if (value === '1' || value === '2') alignment.readingOrder = value === '1' ? 'ltr' : 'rtl';
        break;
    }
  }
  return alignment;
}

