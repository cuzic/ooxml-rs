import { CFB } from './sheetjs-cfb.js';
export { CFB };
import type { Readable as NodeReadable } from 'node:stream';
import browser, {
  type DraftDocumentProperties, type Workbook, type UtilityWorkbook,
  type SelectedWorkbook, type DenseWorkbook, type JsonSource,
  type SheetCsvOptions, type SheetHtmlOptions, type SheetJsonOptions,
} from './sheetjs.js';

export * from './sheetjs.js';

export namespace stream {
  type Readable = NodeReadable;
  type ReadableOptions = { objectMode?: boolean };
  function set_readable(factory: ((options?: ReadableOptions) => Readable) |
    (new (options?: ReadableOptions) => Readable)): void;
  function to_xlml(book: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
    options?: { stride?: number; Props?: DraftDocumentProperties }): Readable;
  function to_csv(sheet: JsonSource | null | undefined, options?: SheetCsvOptions): Readable;
  function to_html(sheet: JsonSource, options?: SheetHtmlOptions): Readable;
  function to_json(sheet: JsonSource | null | undefined, options?: SheetJsonOptions): Readable;
}

declare const sheetjs: Omit<typeof browser, 'stream'> & { stream: typeof stream; CFB: typeof CFB };
export default sheetjs;
