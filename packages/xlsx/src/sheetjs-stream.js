import { failure } from './contract.js';
import { sheetCsvSettings } from './sheetjs-options.generated.js';
import { csvRows, htmlChunks, jsonRows } from './sheetjs-utils.js';
import { xlmlChunks } from './sheetjs-xlml.js';

let readableFactory;

export function set_readable(factory) {
  if (typeof factory !== 'function') throw failure('INVALID_VALUE', 'Expected a Readable constructor or factory');
  readableFactory = factory;
}

function fromChunks(createIterator, options, pushChunk = (stream, chunk) => stream.push(chunk)) {
  if (!readableFactory) throw failure('UNSUPPORTED_OPTIONS', 'Configure a Readable with stream.set_readable');
  const stream = typeof readableFactory.prototype?.push === 'function'
    ? new readableFactory(options) : readableFactory(options);
  let iterator = createIterator(stream);
  const close = error => {
    const active = iterator;
    iterator = undefined;
    try {
      active?.return();
    } catch (closeError) {
      return error ?? closeError;
    }
    return error;
  };
  const destroy = stream._destroy;
  if (typeof destroy === 'function') {
    stream._destroy = function (error, callback) {
      destroy.call(this, close(error), callback);
    };
  }
  stream._read = () => {
    if (!iterator) return;
    try {
      const { value, done } = iterator.next();
      if (done) iterator = undefined;
      pushChunk(stream, done ? null : value);
    } catch (error) {
      close(error);
      if (typeof stream.destroy !== 'function') throw error;
      stream.destroy(error);
    }
  };
  return stream;
}

function fromSheetChunks(sheet, createIterator, options) {
  return fromChunks(stream => {
    if (sheet?.['!ref'] != null) return createIterator();
    stream.push(null);
  }, options);
}

function csvChunks(sheet, options) {
  const settings = sheetCsvSettings(options);
  const separator = settings.RS ?? '\n';
  const rows = csvRows(sheet, settings);
  return readChunks();

  function* readChunks() {
    yield '\uFEFF';
    let first = true;
    for (const row of rows) {
      yield first ? row : separator + row;
      first = false;
    }
  }
}

export function to_csv(sheet, options) {
  return fromSheetChunks(sheet, () => csvChunks(sheet, options));
}

export function to_html(sheet, options) {
  return fromChunks(stream => {
    const chunks = htmlChunks(sheet, options);
    stream.push(chunks.next().value);
    stream.push(chunks.next().value);
    return chunks;
  });
}

export function to_json(sheet, options) {
  return fromSheetChunks(sheet, () => jsonRows(sheet, options), { objectMode: true });
}

export function to_xlml(book, options) {
  return fromChunks(() => xlmlChunks(book, options, true), undefined, (stream, chunk) => {
    if (Array.isArray(chunk)) {
      for (const row of chunk) stream.push(row);
    } else {
      stream.push(chunk);
    }
  });
}
