import { csvParserSettings, csvReadSettings } from './exceljs-options.generated.js';
import { failure } from './contract.js';

const errors = new Set(['#N/A', '#REF!', '#NAME?', '#DIV/0!', '#NULL!', '#VALUE!', '#NUM!']);

export function csvReadMap(options, parseDate) {
  const { dateFormats, map } = csvReadSettings(options);
  if (map) return map;
  return field => {
    if (field === '') return null;
    const number = Number(field);
    if (Number.isFinite(number)) return number;
    for (const pattern of dateFormats) {
      const date = parseDate(field, pattern);
      if (date) return date;
    }
    if (field === 'true' || field === 'false') return field === 'true';
    if (errors.has(field)) return { error: field };
    return field;
  };
}

export async function readCsvRows(stream, options, map) {
  const {
    delimiter, quote, escape, encoding,
    trim, ltrim, rtrim, comment,
    ignoreEmpty, maxRows, skipRows, skipLines,
  } = csvParserSettings(options);
  const rowLimit = maxRows > 0 ? maxRows : Infinity;
  const adjust = trim || (ltrim && rtrim) ? field => field.trim()
    : ltrim ? field => field.trimStart() : rtrim ? field => field.trimEnd() : field => field;
  const whitespace = character => character !== delimiter && character !== '\r' && character !== '\n' && /\s/.test(character);
  const rows = [];
  const decoder = new TextDecoder(encoding);
  const reader = stream.getReader();
  let text = '';
  let done = false;
  let row = [];
  let start = 0;
  let position = 0;
  let counted = 0;
  let lines = 0;

  function finishRow() {
    if ((!ignoreEmpty || row.some(field => field.trim() !== '')) && lines++ >= skipLines) {
      if (counted++ >= skipRows) rows.push(row.map(map));
    }
    row = [];
  }

  async function extendField() {
    text = text.slice(start);
    position -= start;
    start = 0;
    const result = await reader.read();
    done = result.done;
    text += done ? decoder.decode() : decoder.decode(result.value, { stream: true });
  }

  try {
    while (rows.length < rowLimit) {
      while (!done && (position >= text.length ||
          (comment && !row.length && text.length - position < comment.length))) await extendField();
      if (position >= text.length) break;
      if (comment && position === start && !row.length && text.startsWith(comment, position)) {
        for (;;) {
          while (position < text.length && text[position] !== '\r' && text[position] !== '\n') position += 1;
          if (position < text.length || done) break;
          start = position;
          await extendField();
        }
        while (!done && text[position] === '\r' && position + 1 === text.length) await extendField();
        if (text[position++] === '\r' && text[position] === '\n') position += 1;
        start = position;
        continue;
      }
      let parts;
      while (true) {
        while (whitespace(text[position])) position += 1;
        if (position < text.length || done) break;
        if (position > start) (parts ??= []).push(text.slice(start, position));
        start = position;
        await extendField();
      }
      // A row can begin with whitespace before its first delimiter. The
      // reference treats that prefix as empty, including on blank lines.
      if (!row.length && (position === text.length || [delimiter, '\r', '\n'].includes(text[position]))) {
        if (position === text.length) break;
        parts = undefined;
        start = position;
      }
      if (quote && text[position] === quote) {
        const fragments = [];
        let closed = false;
        position += 1;
        while (!done || position < text.length) {
          const segment = position;
          while (position < text.length && text[position] !== quote && text[position] !== escape) position += 1;
          if (position > segment) fragments.push(text.slice(segment, position));
          if (!done && position + 1 >= text.length) {
            start = position;
            await extendField();
            continue;
          }
          if (position >= text.length) break;
          const character = text[position++];
          if (character === escape && (text[position] === quote || text[position] === escape)) {
            fragments.push(text[position++]);
          } else if (character === quote) {
            closed = true;
            break;
          } else fragments.push(character);
        }
        if (!closed) throw failure('INVALID_INPUT', 'CSV quoted field is missing a closing quote');
        row.push(adjust(fragments.join('')));
        start = position;
        while (true) {
          while (whitespace(text[position])) position += 1;
          if (position < text.length || done) break;
          start = position;
          await extendField();
        }
        if (position < text.length && ![delimiter, '\r', '\n'].includes(text[position])) {
          throw failure('INVALID_INPUT', 'Unexpected text after a CSV closing quote');
        }
      } else {
        for (;;) {
          while (position < text.length && ![delimiter, '\r', '\n'].includes(text[position])) position += 1;
          if (position < text.length || done) break;
          if (position > start) (parts ??= []).push(text.slice(start, position));
          start = position;
          await extendField();
        }
        if (parts || position !== start || row.length || text[position] === delimiter) {
          const tail = text.slice(start, position);
          if (parts) parts.push(tail);
          row.push(adjust(parts ? parts.join('') : tail));
        }
      }
      start = position;
      while (!done && position + 1 >= text.length && text[position] === delimiter) await extendField();
      const next = text[position++];
      if (next === delimiter && position === text.length) {
        row.push('');
        finishRow();
      } else if (next !== delimiter) {
        finishRow();
        if (next === '\r' && rows.length < rowLimit) {
          start = position;
          while (!done && position >= text.length) await extendField();
          if (text[position] === '\n') position += 1;
        }
      }
      start = position;
    }
    if (!done) await reader.cancel();
    return rows;
  } catch (error) {
    await reader.cancel(error).catch(() => {});
    throw error;
  } finally {
    reader.releaseLock();
  }
}
