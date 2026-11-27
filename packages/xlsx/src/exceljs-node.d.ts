import type { EventEmitter } from 'node:events';
import type { Buffer } from 'node:buffer';
import type { Readable, Writable } from 'node:stream';
import type { ParserOptionsArgs } from 'fast-csv';
import {
  Workbook as BrowserWorkbook, type Cell as BrowserCell, type Column as BrowserColumn,
  type CsvReadOptions as BrowserCsvReadOptions, type CsvWriteOptions as BrowserCsvWriteOptions,
  type AutoFilter, type CellArrayFormulaValue, type CellErrorValue, type CellSharedFormulaMasterValue, type CellSharedFormulaValue, type CellFont, type CellNoteInput, type CellModel, type CellModelInput, type CellStyle, type CellFormulaValue, type CellHyperlinkValue, type CellRichTextValue, type ColumnDefinition, type DataValidation,
  type HeaderFooter, type LoadOptions, type PageSetupOptions, type TabColor,
  type WorksheetProperties, type WorksheetState, type WorksheetView, type Row as BrowserRow, type WorkbookImage, type Worksheet as BrowserWorksheet,
  type RowModel, type RowModelInput, type WorkbookModel, type WorkbookView, type XlsxWriteOptions,
} from './exceljs.js';

export * from './exceljs.js';

export type Cell = BrowserCell<WorkbookImage>;
export type Column = BrowserColumn<WorkbookImage>;
export type Row = BrowserRow<WorkbookImage>;
export type Worksheet = BrowserWorksheet<WorkbookImage>;

export interface CsvReadOptions extends BrowserCsvReadOptions {
  parserOptions?: ParserOptionsArgs;
}

export interface CsvWriteOptions extends BrowserCsvWriteOptions {
  encoding?: BufferEncoding;
}

export interface Csv {
  read(stream: Readable | ReadableStream<Uint8Array>, options?: CsvReadOptions): Promise<Worksheet>;
  readFile(filename: string, options?: CsvReadOptions): Promise<Worksheet>;
  createInputStream(): never;
  write(stream: Writable | WritableStream<Uint8Array>, options?: CsvWriteOptions): Promise<void>;
  writeFile(filename: string, options?: CsvWriteOptions): Promise<void>;
  writeBuffer(options?: CsvWriteOptions): Promise<Buffer>;
}

interface NodeXlsx<Owner> {
  load(bytes: ArrayBuffer | Uint8Array | Blob, options?: LoadOptions & { base64?: false }): Promise<Owner>;
  load(base64: string, options: LoadOptions & { base64: true }): Promise<Owner>;
  read(stream: Readable | ReadableStream<Uint8Array>, options?: LoadOptions & { base64?: false }): Promise<Owner>;
  readFile(filename: string, options?: LoadOptions & { base64?: false }): Promise<Owner>;
  write(stream: Writable | WritableStream<Uint8Array>, options?: XlsxWriteOptions): Promise<void>;
  writeFile(filename: string, options?: XlsxWriteOptions): Promise<void>;
  writeBuffer(options?: XlsxWriteOptions): Promise<Buffer>;
}

export class Workbook extends BrowserWorkbook<WorkbookImage> {
  readonly csv: Csv;
  readonly xlsx: NodeXlsx<Workbook>;
}

export class ModelContainer {
  constructor(model: WorkbookModel<WorkbookImage>);
  model: WorkbookModel<WorkbookImage>;
  readonly xlsx: NodeXlsx<ModelContainer> & { createInputStream(): never };
}

export interface StreamingWorkbookWriterOptions {
  stream?: Writable;
  filename?: string;
  creator?: string;
  lastModifiedBy?: string;
  created?: Date;
  modified?: Date;
  lastPrinted?: Date;
  useStyles?: boolean;
  useSharedStrings?: boolean;
  zip?: { store?: boolean; comment?: string; forceZip64?: boolean; forceLocalTime?: boolean; level?: number; zlib?: { level?: number } };
}

export type StreamingCellValue = string | number | boolean | Date | CellErrorValue | CellFormulaValue | CellArrayFormulaValue | CellSharedFormulaMasterValue | CellSharedFormulaValue | CellHyperlinkValue | CellRichTextValue | null | undefined;

export interface StreamingCell extends CellStyle {
  destroy(): void;
  get model(): CellModel;
  set model(value: CellModelInput);
  readonly worksheet: StreamingWorksheet;
  readonly workbook: StreamingWorkbookWriter;
  readonly row: number;
  readonly col: number;
  readonly fullAddress: Cell['fullAddress'];
  readonly $col$row: string;
  isMergedTo(master: StreamingCell): boolean;
  get name(): Cell['name'];
  set name(value: string);
  names: string[];
  addName(name: string): void;
  removeName(name: string): void;
  removeAllNames(): void;
  style: CellStyle;
  readonly address: string;
  readonly master: StreamingCell;
  merge(master: StreamingCell, ignoreStyle?: boolean): void;
  unmerge(): void;
  addMergeRef(): void;
  releaseMergeRef(): void;
  readonly isMerged: boolean;
  readonly hyperlink?: string;
  readonly text: string;
  readonly html: string;
  readonly isHyperlink: boolean;
  readonly type: Cell['type'];
  readonly effectiveType: Cell['effectiveType'];
  readonly formulaType: Cell['formulaType'];
  readonly formula: Cell['formula'];
  readonly result: Cell['result'];
  toString(): string;
  toCsvString(): string | number;
  get dataValidation(): Cell['dataValidation'];
  set dataValidation(value: DataValidation | null | undefined);
  get note(): Cell['note'];
  set note(value: string | CellNoteInput | null | undefined);
  value: StreamingCellValue;
}

export type StreamingRowInput = StreamingCellValue[] | Record<string, StreamingCellValue>;

export interface StreamingRow extends CellStyle {
  destroy(): void;
  get model(): RowModel | null;
  set model(value: RowModelInput);
  readonly worksheet: StreamingWorksheet;
  style: CellStyle;
  readonly number: number;
  readonly cellCount: number;
  readonly actualCellCount: number;
  readonly dimensions: Row['dimensions'];
  readonly hasValues: boolean;
  eachCell(iteratee: (cell: StreamingCell, column: number) => void): void;
  eachCell(options: { includeEmpty?: boolean }, iteratee: (cell: StreamingCell, column: number) => void): void;
  addPageBreak(...args: Parameters<Row['addPageBreak']>): void;
  height?: number;
  hidden: boolean;
  outlineLevel: number;
  readonly collapsed: boolean;
  get values(): StreamingCellValue[];
  set values(values: StreamingRowInput);
  getCell(column: string | number): StreamingCell;
  getCellEx(location: { address: string; row: number; col: number }): StreamingCell;
  splice(start: number, count: number, ...inserts: StreamingCellValue[]): void;
  findCell(column: number): StreamingCell | undefined;
  commit(): void;
}

export interface StreamingWriteColumn extends CellStyle {
  readonly worksheet: StreamingWorksheet;
  get defn(): ColumnDefinition;
  set defn(value: ColumnDefinition | null | undefined);
  readonly collapsed: boolean;
  get values(): StreamingCellValue[];
  set values(value: readonly StreamingCellValue[] | null | undefined);
  style: CellStyle;
  readonly number: number;
  readonly letter: string;
  eachCell(iteratee: (cell: StreamingCell, rowNumber: number) => void): void;
  eachCell(options: { includeEmpty?: boolean }, iteratee: (cell: StreamingCell, rowNumber: number) => void): void;
  header?: string | string[];
  readonly headers: Array<string | undefined>;
  readonly headerCount: number;
  readonly isCustomWidth: boolean;
  readonly isDefault: boolean;
  equivalentTo(other: StreamingWriteColumn): boolean;
  toString(): string;
  key?: string;
  width?: number;
  hidden: boolean;
  outlineLevel: number;
}

export interface StreamingWorksheet {
  readonly workbook: StreamingWorkbookWriter;
  readonly dimensions: Worksheet['dimensions'];
  readonly dataValidations: Worksheet['dataValidations'];
  useSharedStrings: boolean;
  readonly id: number;
  readonly name: string;
  readonly committed: boolean;
  readonly startedData: boolean;
  readonly hasComments: boolean;
  destroy(): never;
  readonly lastRow?: StreamingRow;
  getRow(number: number): StreamingRow;
  findRow(number: number): StreamingRow | undefined;
  findCell(address: string): StreamingCell | undefined;
  findCell(row: number, column: number): StreamingCell | undefined;
  eachRow(iteratee: (row: StreamingRow, rowNumber: number) => void): void;
  eachRow(options: { includeEmpty?: boolean }, iteratee: (row: StreamingRow, rowNumber: number) => void): void;
  getCell(address: string): StreamingCell;
  getCell(row: number, column: number): StreamingCell;
  mergeCells(...cells: Parameters<Worksheet['mergeCells']>): void;
  addBackgroundImage(imageId: number): void;
  getBackgroundImageId(): number | undefined;
  readonly rowBreaks: Worksheet['rowBreaks'];
  readonly properties: WorksheetProperties;
  state: WorksheetState;
  get views(): Worksheet['views'];
  set views(value: readonly WorksheetView[]);
  get pageSetup(): Worksheet['pageSetup'];
  set pageSetup(value: PageSetupOptions);
  get headerFooter(): HeaderFooter;
  set headerFooter(value: Partial<HeaderFooter>);
  get autoFilter(): Worksheet['autoFilter'];
  set autoFilter(value: AutoFilter | null | undefined);
  readonly conditionalFormatting: Worksheet['conditionalFormattings'];
  addConditionalFormatting(...args: Parameters<Worksheet['addConditionalFormatting']>): void;
  removeConditionalFormatting(...args: Parameters<Worksheet['removeConditionalFormatting']>): void;
  readonly sheetProtection: Worksheet['sheetProtection'];
  protect(...args: Parameters<Worksheet['protect']>): Promise<void>;
  unprotect(): void;
  get columns(): StreamingWriteColumn[] | null;
  set columns(definitions: readonly ColumnDefinition[]);
  getColumn(column: string | number): StreamingWriteColumn;
  getColumnKey(key: string): StreamingWriteColumn | undefined;
  setColumnKey(key: string, column: StreamingWriteColumn): void;
  deleteColumnKey(key: string): void;
  eachColumnKey(callback: (column: StreamingWriteColumn, key: string) => void): void;
  addRow(values: StreamingRowInput): StreamingRow;
  commit(): void;
}

export interface StreamingWorksheetWriterOptions {
  /** @deprecated Use properties.tabColor. */
  tabColor?: TabColor;
  state?: WorksheetState;
  autoFilter?: AutoFilter;
  views?: readonly WorksheetView[];
  pageSetup?: PageSetupOptions;
  headerFooter?: Partial<HeaderFooter>;
  properties?: Partial<WorksheetProperties>;
  useSharedStrings?: boolean;
}

export interface StreamingWorkbookWriter {
  readonly stream: Writable | undefined;
  readonly definedNames: Workbook['definedNames'];
  readonly nextId: number;
  get properties(): { date1904: boolean };
  set properties(value: { date1904?: boolean });
  get calcProperties(): Workbook['calcProperties'];
  set calcProperties(value: { fullCalcOnLoad?: boolean });
  views: WorkbookView[];
  get creator(): string | undefined;
  set creator(value: string);
  get lastModifiedBy(): string | undefined;
  set lastModifiedBy(value: string);
  get created(): Date | undefined;
  set created(value: Date);
  get modified(): Date | undefined;
  set modified(value: Date);
  get lastPrinted(): Date | undefined;
  set lastPrinted(value: Date);
  addImage(image: Parameters<Workbook['addImage']>[0]): number;
  getImage(id: number): (WorkbookImage & { name: string }) | undefined;
  readonly media: readonly (WorkbookImage | undefined)[];
  useSharedStrings: boolean;
  addWorksheet(name?: string, options?: StreamingWorksheetWriterOptions | TabColor | string): StreamingWorksheet;
  getWorksheet(id?: number | string): StreamingWorksheet | undefined;
  commit(): Promise<void>;
}

export interface StreamingWorkbookReaderOptions {
  worksheets?: 'emit' | 'ignore';
  sharedStrings?: 'cache' | 'emit' | 'ignore';
  hyperlinks?: 'cache' | 'emit' | 'ignore';
  styles?: 'cache' | 'ignore';
  entries?: 'emit' | 'ignore';
}

export interface StreamingReadCell {
  destroy(): void;
  readonly row: number;
  readonly col: number;
  readonly worksheet: StreamingWorksheetReader;
  readonly workbook: StreamingWorkbookReader;
  readonly fullAddress: Cell['fullAddress'];
  readonly $col$row: string;
  readonly master: StreamingReadCell;
  get model(): CellModel & { rawValue?: { sharedString: number } };
  set model(value: CellModelInput & { rawValue?: { sharedString: number } });
  readonly text: string;
  readonly html: string;
  readonly isHyperlink: boolean;
  toString(): string;
  toCsvString(): string | number;
  readonly type: Cell['type'];
  readonly effectiveType: Cell['effectiveType'];
  readonly formulaType: Cell['formulaType'];
  readonly isMerged: boolean;
  isMergedTo(master: StreamingReadCell): boolean;
  addMergeRef(): void;
  releaseMergeRef(): void;
  merge(master: StreamingReadCell, ignoreStyle?: boolean): void;
  unmerge(): void;
  readonly address: string;
  value: StreamingCellValue | { sharedString: number } | { sharedFormula: string };
  readonly hyperlink?: string;
  readonly numFmt?: string;
  readonly font?: CellFont;
  readonly fill?: CellStyle['fill'];
  readonly border?: CellStyle['border'];
  readonly alignment?: CellStyle['alignment'];
  readonly protection?: CellStyle['protection'];
  readonly style: CellStyle;
  readonly formula?: string;
  readonly result?: CellFormulaValue['result'];
}

export interface StreamingReadRow {
  destroy(): void;
  get model(): (Omit<RowModel, 'cells'> & { cells: StreamingReadCell['model'][] }) | null;
  set model(value: Omit<RowModelInput, 'cells'> & { cells: (CellModelInput & { rawValue?: { sharedString: number } })[] });
  readonly number: number;
  get values(): StreamingReadCell['value'][];
  set values(value: StreamingReadCell['value'][] | Record<string, StreamingReadCell['value']> | null | undefined);
  readonly height?: number;
  readonly hidden: boolean;
  readonly outlineLevel: number;
  readonly collapsed: boolean;
  readonly numFmt?: string;
  readonly font?: CellFont;
  readonly fill?: CellStyle['fill'];
  readonly border?: CellStyle['border'];
  readonly alignment?: CellStyle['alignment'];
  readonly protection?: CellStyle['protection'];
  readonly style: CellStyle;
  readonly dimensions?: { min: number; max: number };
  readonly cellCount: number;
  readonly actualCellCount: number;
  splice(start: number, count: number, ...values: StreamingReadCell['value'][]): void;
  getCell(column: string | number): StreamingReadCell;
  getCellEx(location: Parameters<Row['getCellEx']>[0]): StreamingReadCell;
  findCell(column: number): StreamingReadCell | undefined;
  eachCell(callback: (cell: StreamingReadCell, column: number) => void): void;
  eachCell(options: { includeEmpty?: boolean }, callback: (cell: StreamingReadCell, column: number) => void): void;
}

export interface StreamingReadColumn {
  readonly number: number;
  readonly letter: string;
  readonly numFmt?: string;
  readonly font?: CellFont;
  readonly fill?: CellStyle['fill'];
  readonly border?: CellStyle['border'];
  readonly alignment?: CellStyle['alignment'];
  readonly protection?: CellStyle['protection'];
  key?: string;
  width?: number;
  hidden: boolean;
  outlineLevel: number;
  readonly collapsed: boolean;
  style: CellStyle;
}

export interface StreamingReadDimensions {
  readonly model: Partial<{ top: number; left: number; bottom: number; right: number }>;
  readonly top: number;
  readonly left: number;
  readonly bottom: number;
  readonly right: number;
  readonly tl: string;
  readonly br: string;
  readonly range: string;
}

interface StreamingEventEmitter<Events extends Record<keyof Events, unknown[]>> extends EventEmitter {
  on<Event extends string | symbol>(event: Event, listener: (
    ...args: Event extends keyof Events ? Events[Event] : any[]
  ) => void): this;
  once<Event extends string | symbol>(event: Event, listener: (
    ...args: Event extends keyof Events ? Events[Event] : any[]
  ) => void): this;
}

type StreamingReaderEvents<Event extends { eventType: string; value: unknown }> = {
  [Entry in Event as Entry['eventType']]: [value: Entry['value']];
} & {
  finished: [];
  error: [error: unknown];
};

export type StreamingWorksheetReadEvent =
  | { eventType: 'row'; value: StreamingReadRow }
  | { eventType: 'hyperlink'; value: { ref: string; rId?: string } };

export interface StreamingWorksheetReader extends StreamingEventEmitter<StreamingReaderEvents<StreamingWorksheetReadEvent>>, AsyncIterable<StreamingReadRow> {
  readonly workbook: StreamingWorkbookReader;
  readonly id: number;
  readonly name: string;
  readonly state: string;
  readonly columns: StreamingReadColumn[] | null;
  readonly dimensions: StreamingReadDimensions;
  getColumn(column: string | number): StreamingReadColumn;
  getColumnKey(key: string): StreamingReadColumn | undefined;
  setColumnKey(key: string, column: StreamingReadColumn): void;
  deleteColumnKey(key: string): void;
  eachColumnKey(callback: (column: StreamingReadColumn, key: string) => void): void;
  destroy(): never;
  parse(): AsyncIterable<StreamingWorksheetReadEvent[]>;
  read(): Promise<void>;
}

export interface StreamingHyperlinkRelationship {
  type: typeof import('./exceljs.js').RelationshipType.Styles;
  rId: string;
  target: string;
  targetMode: 'External' | undefined;
}

type StreamingHyperlinkReaderEvents = StreamingReaderEvents<{
  eventType: 'hyperlink';
  value: StreamingHyperlinkRelationship;
}>;

export interface StreamingHyperlinkReader extends StreamingEventEmitter<StreamingHyperlinkReaderEvents> {
  readonly workbook: StreamingWorkbookReader;
  readonly options: Required<StreamingWorkbookReaderOptions>;
  readonly id: string;
  readonly count: number;
  read(): Promise<void>;
}

export type StreamingWorkbookReadEvent =
  | { eventType: 'worksheet'; value: StreamingWorksheetReader }
  | { eventType: 'hyperlinks'; value: StreamingHyperlinkReader }
  | { eventType: 'shared-strings'; value: { index: number; text: string | { richText: { text: string; font: CellFont | null }[] } } };

export type StreamingWorkbookEntry =
  | { type: 'workbook' | 'shared-strings' | 'styles' }
  | { type: 'worksheet' | 'hyperlinks'; id: string };

type StreamingWorkbookReaderEvents = StreamingReaderEvents<StreamingWorkbookReadEvent> & {
  entry: [value: StreamingWorkbookEntry];
  end: [];
};

export interface StreamingWorkbookReader extends StreamingEventEmitter<StreamingWorkbookReaderEvents>, AsyncIterable<StreamingWorksheetReader> {
  input: string | Readable;
  options: Required<StreamingWorkbookReaderOptions>;
  readonly sharedStrings?: (string | CellRichTextValue)[];
  readonly properties?: { model: { date1904: boolean } };
  readonly model?: {
    sheets: { id: number; name: string; state: string; path: string }[];
  };
  parse(input?: string | Readable, options?: StreamingWorkbookReaderOptions): AsyncIterable<StreamingWorkbookReadEvent>;
  read(input?: string | Readable, options?: StreamingWorkbookReaderOptions): Promise<void>;
}

export const stream: {
  xlsx: {
    WorkbookReader: {
      new (input: string | Readable, options?: StreamingWorkbookReaderOptions): StreamingWorkbookReader;
      readonly Options: { [Key in keyof Required<StreamingWorkbookReaderOptions>]: NonNullable<StreamingWorkbookReaderOptions[Key]>[] };
    };
    WorkbookWriter: new (options?: StreamingWorkbookWriterOptions) => StreamingWorkbookWriter;
  };
};
