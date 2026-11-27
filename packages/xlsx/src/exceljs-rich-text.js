import { failure } from './contract.js';
import { cellFontDefaults, cellFontSettings, cellRichTextRunSettings, excelColorSettings } from './exceljs-options.generated.js';

export function tabColorAttributes(color) {
  if (color == null) return [];
  const { argb, theme, indexed, tint } = excelColorSettings(color);
  if (argb !== undefined) return ['rgb', argb.toUpperCase()];
  if (theme !== undefined) {
    return tint === undefined ? ['theme', String(theme)] : ['theme', String(theme), 'tint', String(tint)];
  }
  if (indexed !== undefined) return ['indexed', String(indexed)];
  return ['auto', '1'];
}

export function encodeFont(font, defaults = cellFontDefaults) {
  const { name = defaults.name, size = defaults.size, bold, italic, strike, underline, vertAlign, scheme, family, charset, color } = cellFontSettings(font);
  const [kind, colorValue, , tint] = tabColorAttributes(color);
  if (['theme', 'indexed'].includes(kind) && Number(colorValue) > 255) {
    throw failure('INVALID_VALUE', 'Font color index exceeds 255');
  }
  return [name, size, bold, italic, strike, underline === true ? 'single' : underline, vertAlign, scheme,
    family, charset, kind ? kind + ':' + colorValue : '', tint].map((v) => String(v ?? ''));
}

export function encodeRichTextRuns(runs) {
  return Array.from(runs, run => {
    const { text, font } = cellRichTextRunSettings(run);
    return [text, ...(font === undefined ? Array(12).fill('') : encodeFont(font))];
  }).flat();
}

export function decodeFont(values) {
  if (!values.length) return undefined;
  const [name, size, bold, italic, strike, underline, vertAlign, scheme, family, charset, color, tint] = values;
  const font = { name, size: Number(size) };
  for (const [key, flag] of Object.entries({ bold, italic, strike })) {
    if (flag === 'true') font[key] = true;
  }
  if (underline) font.underline = underline === 'single' ? true : underline;
  if (vertAlign) font.vertAlign = vertAlign;
  if (scheme) font.scheme = scheme;
  if (family) font.family = Number(family);
  if (charset) font.charset = Number(charset);
  const [kind, value] = color.split(':');
  if (kind === 'rgb') font.color = { argb: value };
  if (kind === 'indexed') font.color = { indexed: Number(value) };
  if (kind === 'theme') {
    font.color = { theme: Number(value) };
    if (Number(tint)) font.color.tint = Number(tint);
  }
  return font;
}

// Bits the Wasm reader sets for font fields the XML element wrote.
const declaredFontName = 1;
const declaredFontSize = 2;
// ExcelJS reports these elements as true whenever they are present.
const declaredFontFlags = [[4, 'outline'], [8, 'shadow'], [16, 'condense'], [32, 'extend']];

// Applies the declared-field bits: drops the name and size that Rust filled
// with defaults when the element left them out, and adds the flags ExcelJS
// reports from element presence, so fonts read back as ExcelJS parses them.
export function applyDeclaredFontFields(font, fields) {
  if (!(fields & declaredFontName)) delete font.name;
  if (!(fields & declaredFontSize)) delete font.size;
  for (const [bit, key] of declaredFontFlags) if (fields & bit) font[key] = true;
  return font;
}

// Decodes a font followed by its declared-field bits.
export function decodeDeclaredFont(values) {
  const font = decodeFont(values.slice(0, 12));
  return font && applyDeclaredFontFields(font, Number(values[12]));
}

// Decodes runs of 14 fields: text, font and declared-field bits.
export function decodeDeclaredRichTextRuns(fields) {
  const runs = [];
  for (let index = 0; index < fields.length; index += 14) {
    const [text, ...font] = fields.slice(index, index + 14);
    runs.push(font[1] ? { font: decodeDeclaredFont(font), text } : { text });
  }
  return runs;
}

export function decodeRichTextRuns(fields) {
  const runs = [];
  for (let index = 0; index < fields.length; index += 13) {
    const [text, ...font] = fields.slice(index, index + 13);
    runs.push(font[1] ? { text, font: decodeFont(font) } : { text });
  }
  return runs;
}
