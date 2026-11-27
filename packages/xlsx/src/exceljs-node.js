import './runtime-node.js';

import { Buffer } from 'node:buffer';
import { randomUUID } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { createCsv } from './exceljs-csv-node.js';
import { workbookImageFileSettings } from './exceljs-options.generated.js';
import { imageFileReader, prepareImageFile } from './contract.js';
import { Workbook as BrowserWorkbook } from './exceljs.js';
import { WorkbookWriter as StreamingWorkbookWriter } from './exceljs-stream-writer-node.js';
import { WorkbookReader } from './exceljs-stream-reader-node.js';

export * from './exceljs.js';

class WorkbookWriter extends StreamingWorkbookWriter {
  constructor(options) {
    super(new Workbook(), options);
  }
}

export const stream = { xlsx: { WorkbookReader, WorkbookWriter } };

// Minimal 1×1 headers. A nonce keeps deferred sources distinct in Rust's
// media registry. Only resolved file bytes are used in saved workbooks.
const placeholderHeaders = {
  png: '89504e470d0a1a0a0000000d4948445200000001000000010802000000000000000000000049454e44',
  jpeg: 'ffd8ffc000070800010001ffd9',
  gif: '47494638396101000100',
};

export class Workbook extends BrowserWorkbook {
  [imageFileReader](filename) {
    return readFile(filename);
  }

  [prepareImageFile](image) {
    const { extension, filename } = workbookImageFileSettings(image);
    const buffer = Buffer.concat([Buffer.from(placeholderHeaders[extension], 'hex'), Buffer.from(randomUUID())]);
    return { extension, filename, buffer };
  }

  constructor() {
    super();
    this.csv = createCsv(this);
    const { read, write, writeBuffer } = this.xlsx;
    const readStream = (stream, options) => read(
      stream instanceof Readable ? Readable.toWeb(stream) : stream, options,
    );
    Object.assign(this.xlsx, {
      read: readStream,
      async readFile(filename, options) {
        const stream = createReadStream(filename);
        try {
          return await readStream(stream, options);
        } finally {
          stream.destroy();
        }
      },
      async write(stream, options) {
        if (typeof WritableStream !== 'undefined' && stream instanceof WritableStream) {
          return write(stream, options);
        }
        const bytes = await writeBuffer(options);
        await pipeline(Readable.from([bytes]), stream);
      },
      async writeFile(filename, options) {
        const bytes = await writeBuffer(options);
        await writeFile(filename, bytes);
      },
      async writeBuffer(options) {
        return Buffer.from(await writeBuffer(options));
      },
    });
  }
}

export class ModelContainer {
  #xlsx;

  constructor(model) {
    this.model = model;
  }

  get xlsx() {
    return this.#xlsx ??= {
      createInputStream() {
        throw new Error(
          '`XLSX#createInputStream` is deprecated. You should use `XLSX#read` instead. ' +
          'This method will be removed in version 5.0. Please follow upgrade instruction: ' +
          'https://github.com/exceljs/exceljs/blob/master/UPGRADE-4.0.md',
        );
      },
      load: (data, options) => this.#readModel('load', data, options),
      read: (stream, options) => this.#readModel('read', stream, options),
      readFile: (filename, options) => this.#readModel('readFile', filename, options),
      write: (stream, options) => this.#writeModel('write', stream, options),
      writeFile: (filename, options) => this.#writeModel('writeFile', filename, options),
      writeBuffer: options => this.#writeModel('writeBuffer', options),
    };
  }

  async #readModel(method, input, options) {
    const workbook = new Workbook();
    await workbook.xlsx[method](input, options);
    this.model = workbook.model;
    return this;
  }

  async #writeModel(method, ...args) {
    const workbook = new Workbook();
    workbook.model = this.model;
    return workbook.xlsx[method](...args);
  }
}
