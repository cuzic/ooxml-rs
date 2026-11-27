import { Buffer } from 'node:buffer';
import { createReadStream, createWriteStream } from 'node:fs';
import { Readable, Writable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import dayjs from 'dayjs';
import customParseFormat from 'dayjs/plugin/customParseFormat.js';
import utc from 'dayjs/plugin/utc.js';
import { format, parse } from 'fast-csv';
import {
  checkedCsvTransform, csvReadDestinationSettings, csvReadStreamSettings, csvWriteFileSettings, csvWriteStreamSettings,
} from './exceljs-options.generated.js';
import { failure } from './contract.js';
import { csvReadMap } from './exceljs-csv-input.js';
import { worksheetRows } from './exceljs-csv-rows.js';

dayjs.extend(customParseFormat);
dayjs.extend(utc);

function parseDate(field, pattern) {
  const date = dayjs(field, pattern, true);
  return date.isValid() ? date.toDate() : undefined;
}

function formatDate(value, { dateFormat, dateUTC }) {
  return (dateUTC ? dayjs.utc(value) : dayjs(value)).format(dateFormat);
}

export function createCsv(workbook) {
  async function read(stream, options, filename) {
    options ??= {};
    const map = csvReadMap(options, parseDate);
    const worksheet = workbook.addWorksheet(csvReadDestinationSettings(options).sheetName);
    const rows = [];
    const parser = parse(csvReadStreamSettings(options).parserOptions);
    const input = stream ?? createReadStream(filename);
    const source = typeof ReadableStream !== 'undefined' && input instanceof ReadableStream
      ? Readable.fromWeb(input) : input;
    await pipeline(source, parser, async function collect(records) {
      for await (const record of records) {
        if (!Array.isArray(record)) {
          throw failure('UNSUPPORTED_OPTIONS', 'ExcelJS CSV input requires array records');
        }
        rows.push(record.map(map));
      }
    });
    worksheet.addRows(rows);
    return worksheet;
  }

  async function write(stream, options, filename) {
    options ??= {};
    const rows = worksheetRows(workbook, options, formatDate);
    const { formatterOptions } = csvWriteStreamSettings(options);
    checkedCsvTransform(formatterOptions?.transform);
    const formatter = format(formatterOptions);
    const output = stream ?? createWriteStream(filename, csvWriteFileSettings(options));
    const destination = typeof WritableStream !== 'undefined' && output instanceof WritableStream
      ? Writable.fromWeb(output) : output;
    return pipeline(Readable.from(rows), formatter, destination);
  }

  return {
    read,
    readFile(filename, options) {
      return read(undefined, options, filename);
    },
    createInputStream() {
      throw new Error('CSV#createInputStream is deprecated. Use CSV#read instead.');
    },
    write,
    writeFile(filename, options) {
      return write(undefined, options, filename);
    },
    async writeBuffer(options) {
      const chunks = [];
      await write(new Writable({
        write(chunk, _encoding, callback) {
          chunks.push(chunk);
          callback();
        },
      }), options);
      return Buffer.concat(chunks);
    },
  };
}
