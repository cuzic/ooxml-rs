export interface CellFillColor {
  readonly rgb?: string;
  /** Untinted theme color, present when the workbook was read with a truthy WTF. */
  readonly raw_rgb?: string;
  readonly theme?: number;
  readonly tint?: number;
  readonly indexed?: number;
}

export interface CellFill {
  readonly patternType?: string;
  readonly fgColor?: CellFillColor;
  readonly bgColor?: CellFillColor;
}

export interface XlsbStringRun { ich: number; ifnt: number }

export interface CellHyperlink {
  Target: string;
  Tooltip?: string;
  /** Set to undefined explicitly to remove an existing XLSX display label. */
  display?: string | undefined;
  /** Read projections: the element's range and, for location-only links, its location and relationship. Ignored when writing. */
  ref?: string;
  location?: string;
  Rel?: { Target?: string; TargetMode?: string; Type?: string; Id?: string };
  /** Accepted and ignored when writing, as CE 0.20.3 does. */
  id?: string;
  uid?: string;
}

export type Cell = { readonly s?: CellFill; z?: string | number; w?: string; f?: string; readonly F?: string; readonly D?: true; readonly h?: string; readonly r?: string | XlsbStringRun[]; l?: CellHyperlink;
  c?: { a: string; t: string }[] } & (
  | { t: 'n'; v: number | undefined }
  | { t: 'e'; v: number | undefined }
  | { t: 's'; v: string }
  | { t: 'b'; v: boolean }
  | { t: 'd'; v: Date }
  | { t: 'z'; v?: null });

export interface RowLayout { hpt?: number; hpx?: number; hidden?: boolean; level?: number }
export interface ColumnLayout { DBF?: { name: string; type: string; len: number; dec?: number; offset?: number }; width?: number | null; wpx?: number; wch?: number; MDW?: number; hidden?: boolean; level?: number;
  /** XLSX cellStyles reads: raw outlineLevel and its lowercase alias. CE 0.20.3's other raw <col> keys are accepted and ignored when writing. */
  outlineLevel?: string; outlinelevel?: string; customwidth?: string; style?: string; bestFit?: string; bestfit?: string; collapsed?: string; phonetic?: string }

export interface SheetMargins {
  left?: number; right?: number; top?: number; bottom?: number; header?: number; footer?: number;
}

/** Worksheet protection settings for XLSB output. */
export interface SheetProtection {
  password?: string;
  objects?: boolean;
  scenarios?: boolean;
  formatCells?: boolean;
  formatColumns?: boolean;
  formatRows?: boolean;
  insertColumns?: boolean;
  insertRows?: boolean;
  insertHyperlinks?: boolean;
  deleteColumns?: boolean;
  deleteRows?: boolean;
  selectLockedCells?: boolean;
  sort?: boolean;
  autoFilter?: boolean;
  pivotTables?: boolean;
  selectUnlockedCells?: boolean;
}

export interface Worksheet {
  readonly '!ref'?: string;
  readonly '!fullref'?: string;
  '!autofilter'?: { ref: string };
  '!merges'?: Range[];
  '!margins'?: SheetMargins;
  '!protect'?: SheetProtection;
  '!rows'?: (RowLayout | undefined)[];
  '!cols'?: (ColumnLayout | undefined)[];
  [address: `${string}${number}`]: Cell | undefined;
}

export interface DefinedName {
  readonly Name: string;
  readonly Ref: string | number;
  readonly Sheet?: number;
  readonly Comment?: string;
}

export interface LoadedWorkbookMetadata {
  readonly Names?: readonly DefinedName[];
  /** Writing accepts the other CE 0.20.3 WBProps keys only with their default values. */
  WBProps?: { date1904?: boolean; CodeName?: ''; filterPrivacy?: false; defaultThemeVersion?: 0; [key: string]: unknown };
  /** Writing a new XLSX workbook applies Views[0].RTL to every worksheet; zoom is ignored. */
  Views?: ({ RTL?: boolean; zoom?: number } | undefined)[];
  /** XLSX reads: the workbookView attributes that are exposed. Ignored when writing. */
  readonly WBView?: readonly { readonly [key: string]: string | number }[];
  Sheets?: { readonly name?: string; Hidden?: 0 | 1 | 2; readonly sheetId?: string; readonly sheetid?: string; readonly id?: string; CodeName?: string }[];
  /** Read-only workbook.xml values of CE 0.20.3; ignored when writing. */
  readonly xmlns?: string;
  readonly AppVersion?: { readonly [key: string]: string };
  readonly CalcPr?: { readonly [key: string]: string };
}

export interface CalculationChainEntry {
  readonly i: number | string;
  readonly r?: string;
  readonly a?: string;
  readonly l?: string;
  readonly s?: string;
  readonly t?: string;
}

export interface ZipFileEntry {
  readonly name: string;
  readonly type: number;
  readonly content?: Uint8Array;
  readonly size: number;
  readonly mt?: Date;
}

interface LoadedDocumentProperties {
  readonly bookType: 'xlsx' | 'xlsb';
  readonly keys?: readonly string[];
  readonly files?: Readonly<Record<string, ZipFileEntry>>;
  readonly cfb?: CompoundContainer;
  readonly Deps: readonly CalculationChainEntry[] | Readonly<Record<string, never>>;
  readonly vbaraw?: Uint8Array;
  readonly Props: Readonly<Record<string, DocumentProperty | readonly string[]>>;
  readonly Custprops: Readonly<Record<string, DocumentProperty>>;
}

export interface Workbook extends LoadedDocumentProperties {
  Workbook?: LoadedWorkbookMetadata;
  readonly SheetNames: readonly string[];
  readonly Sheets: Readonly<Record<string, Worksheet>>;
}

export type ByteInput = ArrayBuffer | Uint8Array | Int8Array | readonly number[];

export interface DelimitedReadOptions {
  cellFormula?: boolean;
  sheetStubs?: boolean;
  type: 'string';
  codepage?: number;
  PRN?: boolean;
  cellDates?: boolean;
  cellNF?: boolean;
  dateNF?: string;
  UTC?: boolean;
  raw?: boolean;
  dense?: boolean;
  FS?: string;
  sheets?: SheetSelection;
  cellText?: boolean;
  sheetRows?: number;
}

export function read(text: string, options: { type: 'string'; bookProps: true; bookSheets?: boolean } |
  { type: 'string'; bookSheets: true; bookProps?: boolean }):
  Omit<UtilityWorkbook, 'Sheets'>;
export function read(text: string, options: DelimitedReadOptions & { cellStyles: boolean }): UtilityWorkbook;
export function read(text: string, options: DelimitedReadOptions): UtilityWorkbook;
export function read(input: ByteInput | string, options: Omit<DelimitedReadOptions, 'type'> &
  { type?: 'array' | 'buffer' | 'base64' | 'binary' } & ({ raw: boolean } | { FS: string } | { PRN: true } | { codepage: number })): UtilityWorkbook;
export type SheetSelection = string | number | readonly (string | number)[];

export interface SelectedWorkbook extends LoadedDocumentProperties {
  Workbook?: LoadedWorkbookMetadata;
  readonly SheetNames: readonly string[];
  readonly Sheets: Readonly<Partial<Record<string, Worksheet>>>;
}

export interface DenseWorksheet {
  readonly '!ref'?: string;
  readonly '!fullref'?: string;
  '!autofilter'?: { ref: string };
  '!merges'?: Range[];
  '!margins'?: SheetMargins;
  '!protect'?: SheetProtection;
  '!rows'?: (RowLayout | undefined)[];
  '!cols'?: (ColumnLayout | undefined)[];
  readonly '!data': Array<Array<Cell | undefined> | undefined>;
}

export interface DenseWorkbook extends LoadedDocumentProperties {
  Workbook?: LoadedWorkbookMetadata;
  readonly SheetNames: readonly string[];
  readonly Sheets: Readonly<Partial<Record<string, DenseWorksheet>>>;
}

export interface SheetNamesWorkbook {
  readonly cfb?: CompoundContainer;
  Workbook?: LoadedWorkbookMetadata;
  readonly SheetNames: readonly string[];
}

type XlsxInput = ByteInput | string;
type XlsxReadType = 'array' | 'buffer' | 'base64' | 'binary';

export type DocumentProperty = string | number | boolean | Date;
export interface PropertiesWorkbook {
  readonly cfb?: CompoundContainer;
  Props: Record<string, DocumentProperty | string[]>;
  Custprops: Record<string, DocumentProperty>;
}
export function read(input: XlsxInput, options: { type?: XlsxReadType; bookVBA?: boolean; bookDeps?: boolean; bookFiles?: boolean; WTF?: boolean; bookProps: true; bookSheets: true; codepage?: number }): PropertiesWorkbook & { SheetNames: string[] };
export function read(input: XlsxInput, options: { type?: XlsxReadType; bookVBA?: boolean; bookDeps?: boolean; bookFiles?: boolean; WTF?: boolean; bookProps: true; bookSheets?: false; codepage?: number }): PropertiesWorkbook;
export function read(input: XlsxInput, options: { type?: XlsxReadType; sheets?: SheetSelection; bookVBA?: boolean; bookDeps?: boolean; bookFiles?: boolean; WTF?: boolean; bookProps?: false; bookSheets: true; sheetRows?: number; cellNF?: boolean; cellText?: boolean; cellDates?: boolean; sheetStubs?: boolean; nodim?: boolean; cellStyles?: boolean; cellFormula?: boolean; cellHTML?: boolean; xlfn?: boolean; dateNF?: string; codepage?: number }): SheetNamesWorkbook;
export function read(input: XlsxInput, options: { type?: XlsxReadType; sheets: SheetSelection; dense?: false; bookVBA?: boolean; bookDeps?: boolean; bookFiles?: boolean; WTF?: boolean; bookProps?: false; bookSheets?: false; sheetRows?: number; cellNF?: boolean; cellText?: boolean; cellDates?: boolean; sheetStubs?: boolean; nodim?: boolean; cellStyles?: boolean; cellFormula?: boolean; cellHTML?: boolean; xlfn?: boolean; dateNF?: string; codepage?: number }): SelectedWorkbook;
export function read(input: XlsxInput, options: { type?: XlsxReadType; dense: true; sheets?: SheetSelection; bookVBA?: boolean; bookDeps?: boolean; bookFiles?: boolean; WTF?: boolean; bookProps?: false; bookSheets?: false; sheetRows?: number; cellNF?: boolean; cellText?: boolean; cellDates?: boolean; sheetStubs?: boolean; nodim?: boolean; cellStyles?: boolean; cellFormula?: boolean; cellHTML?: boolean; xlfn?: boolean; dateNF?: string; codepage?: number }): DenseWorkbook;
export function read(bytes: ByteInput, options?: { type?: 'array'; bookVBA?: boolean; bookDeps?: boolean; bookFiles?: boolean; WTF?: boolean; bookProps?: false; bookSheets?: false; sheetRows?: number; cellNF?: boolean; cellText?: boolean; cellDates?: boolean; sheetStubs?: boolean; nodim?: boolean; cellStyles?: boolean; cellFormula?: boolean; cellHTML?: boolean; xlfn?: boolean; dateNF?: string; codepage?: number }): Workbook;
export function read(bytes: ArrayBuffer | Uint8Array, options: { type: 'buffer'; bookVBA?: boolean; bookDeps?: boolean; bookFiles?: boolean; WTF?: boolean; sheetRows?: number; cellNF?: boolean; cellText?: boolean; cellDates?: boolean; sheetStubs?: boolean; nodim?: boolean; cellStyles?: boolean; cellFormula?: boolean; cellHTML?: boolean; xlfn?: boolean; dateNF?: string; codepage?: number }): Workbook;
export function read(text: string, options?: { type?: 'base64' | 'binary'; bookVBA?: boolean; bookDeps?: boolean; bookFiles?: boolean; WTF?: boolean; sheetRows?: number; cellNF?: boolean; cellText?: boolean; cellDates?: boolean; sheetStubs?: boolean; nodim?: boolean; cellStyles?: boolean; cellFormula?: boolean; cellHTML?: boolean; xlfn?: boolean; dateNF?: string; codepage?: number }): Workbook;

export interface CsvWriteOptions extends Omit<SheetCsvOptions, 'type'> {
  codepage?: number;
  sheet?: string | number;
}

export interface HtmlWriteOptions extends SheetHtmlOptions {
  codepage?: number;
  sheet?: string | number;
}

type TextWriteOptions = (CsvWriteOptions & { bookType: 'csv' | 'txt' }) |
  (HtmlWriteOptions & { bookType: 'html' | 'htm' }) | { bookType: 'prn' | 'dif' | 'sylk' | 'slk' | 'eth' | 'rtf'; sheet?: string | number; codepage?: number };

export type SpreadsheetMlWriteOptions = { bookType: 'xml' | 'xlml'; Props?: DraftDocumentProperties } |
  { bookType: 'fods' };
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
  options: SpreadsheetMlWriteOptions & { type: 'string' | 'base64' | 'binary' }): string;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
  options: SpreadsheetMlWriteOptions & { type: 'array' }): ArrayBuffer;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
  options: SpreadsheetMlWriteOptions & { type: 'buffer' }): Uint8Array;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
  options: SpreadsheetMlWriteOptions & { type: 'file'; file: string }): void;

export interface BinarySheetWriteOptions {
  bookType: 'dbf' | 'wk1' | 'wk3' | 'biff2' | 'biff3' | 'biff4' | 'biff5' | 'biff8' | 'xls' | 'xla';
  bookSST?: boolean;
  WTF?: boolean;
  sheet?: string | number;
  codepage?: number;
}
type BinarySheetWorkbook = Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook;
export function write(workbook: BinarySheetWorkbook, options: BinarySheetWriteOptions & { type: 'array' }): ArrayBuffer;
export function write(workbook: BinarySheetWorkbook, options: BinarySheetWriteOptions & { type: 'buffer' }): Uint8Array;
export function write(workbook: BinarySheetWorkbook, options: BinarySheetWriteOptions & { type: 'base64' | 'binary' }): string;
export function write(workbook: BinarySheetWorkbook, options: BinarySheetWriteOptions & { type: 'file'; file: string }): void;

export interface OdsWriteOptions {
  bookType: 'ods';
  compression?: boolean;
}
export interface XlsbWriteOptions {
  bookType: 'xlsb';
  compression?: boolean;
  bookSST?: boolean;
  Props?: DraftDocumentProperties;
}
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
  options: (OdsWriteOptions | XlsbWriteOptions) & { type: 'base64' | 'binary' }): string;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
  options: (OdsWriteOptions | XlsbWriteOptions) & { type: 'array' }): ArrayBuffer;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
  options: (OdsWriteOptions | XlsbWriteOptions) & { type: 'buffer' }): Uint8Array;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
  options: (OdsWriteOptions | XlsbWriteOptions) & { type: 'file'; file: string }): void;

export type XmlWorkbookFormat = 'xlsx' | 'xlsm' | 'xlam';
export interface XlsxWriteOptions {
  codepage?: number;
  cellDates?: boolean;
  themeXLSX?: string;
  bookSST?: boolean;
  ignoreEC?: boolean;
  compression?: boolean;
  Props?: DraftDocumentProperties;
  cellStyles?: boolean;
  sheetStubs?: boolean;
}

export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: (TextWriteOptions | (XlsxWriteOptions & { bookType?: XmlWorkbookFormat })) & { type: 'file'; file: string }): void;
export function writeXLSX(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: XlsxWriteOptions & { type: 'file'; file: string; bookType?: string }): void;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: TextWriteOptions & { type: 'array' }): ArrayBuffer;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: TextWriteOptions & { type: 'buffer' }): Uint8Array;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: TextWriteOptions & { type: 'base64' | 'binary' | 'string' }): string;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: XlsxWriteOptions & { bookType?: XmlWorkbookFormat; type: 'array' }): ArrayBuffer;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: XlsxWriteOptions & { bookType?: XmlWorkbookFormat; type: 'buffer' }): Uint8Array;
export function write(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: XlsxWriteOptions & { bookType?: XmlWorkbookFormat; type: 'base64' | 'binary' }): string;
export function writeXLSX(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: XlsxWriteOptions & { bookType?: string; type: 'array' }): ArrayBuffer;
export function writeXLSX(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: XlsxWriteOptions & { bookType?: string; type: 'buffer' }): Uint8Array;
export function writeXLSX(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, options: XlsxWriteOptions & { bookType?: string; type: 'base64' | 'binary' }): string;

export function writeFile(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
  filename: string, options: SpreadsheetMlWriteOptions | OdsWriteOptions | XlsbWriteOptions | BinarySheetWriteOptions | { Props?: DraftDocumentProperties }): void;
export function writeFile(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, filename: string, options?: (XlsxWriteOptions & { bookType?: XmlWorkbookFormat }) | (CsvWriteOptions & { bookType?: 'csv' | 'txt' }) | (HtmlWriteOptions & { bookType?: 'html' | 'htm' }) | { bookType: 'prn' | 'dif' | 'sylk' | 'slk' | 'eth' | 'rtf'; sheet?: string | number; codepage?: number }): void;
export const writeFileSync: typeof writeFile;
export interface SheetJsFileSystem {
  readFileSync?(filename: string): ByteInput;
  writeFileSync?(filename: string, bytes: Uint8Array): void;
  writeFile?(filename: string, bytes: Uint8Array, callback: (error?: Error | null) => void): void;
}
export function set_fs(fs?: SheetJsFileSystem): void;
type FileReadOptions = Omit<DelimitedReadOptions, 'type'> & {
  WTF?: boolean;
  type?: string;
  bookVBA?: boolean;
  bookFiles?: boolean;
  bookDeps?: boolean;
  bookProps?: boolean;
  bookSheets?: boolean;
  cellStyles?: boolean;
  cellHTML?: boolean;
  xlfn?: boolean;
  nodim?: boolean;
};
export function readFile(filename: string, options: FileReadOptions & { bookProps: true; bookSheets: true }): PropertiesWorkbook & { SheetNames: string[] };
export function readFile(filename: string, options: FileReadOptions & { bookProps: true }): PropertiesWorkbook;
export function readFile(filename: string, options: FileReadOptions & { bookSheets: true }): SheetNamesWorkbook;
export function readFile(filename: string, options: FileReadOptions & { dense: true }): DenseWorkbook;
export function readFile(filename: string, options: FileReadOptions & { sheets: SheetSelection }): SelectedWorkbook;
export function readFile(filename: string, options?: FileReadOptions): Workbook;
export const readFileSync: typeof readFile;
export function read(filename: string, options: FileReadOptions & { type: 'file'; bookProps: true; bookSheets: true }): PropertiesWorkbook & { SheetNames: string[] };
export function read(filename: string, options: FileReadOptions & { type: 'file'; bookProps: true }): PropertiesWorkbook;
export function read(filename: string, options: FileReadOptions & { type: 'file'; bookSheets: true }): SheetNamesWorkbook;
export function read(filename: string, options: FileReadOptions & { type: 'file'; dense: true }): DenseWorkbook;
export function read(filename: string, options: FileReadOptions & { type: 'file'; sheets: SheetSelection }): SelectedWorkbook;
export function read(filename: string, options: FileReadOptions & { type: 'file' }): Workbook;
type AsyncFileWorkbook = Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook;
type AsyncFileOptions = Parameters<typeof writeFile>[2] | SpreadsheetMlWriteOptions | OdsWriteOptions |
  XlsbWriteOptions | BinarySheetWriteOptions | { bookType: 'socialcalc'; sheet?: string | number; codepage?: number };
type FileWriteCallback = (error?: Error | null) => void;
export function writeFileAsync(filename: string, workbook: AsyncFileWorkbook, callback: FileWriteCallback): void;
export function writeFileAsync(filename: string, workbook: AsyncFileWorkbook, options: AsyncFileOptions, callback: FileWriteCallback): void;

export function writeFileXLSX(workbook: Workbook | UtilityWorkbook | SelectedWorkbook | SheetNamesWorkbook | DenseWorkbook, filename: string, options?: XlsxWriteOptions & { bookType?: string }): void;

export interface CellAddress {
  r: number;
  c: number;
}

export interface Range {
  s: CellAddress;
  e: CellAddress;
}

export interface SheetJsonOptions {
  dateNF?: string;
  header?: 1 | 'A' | string[];
  range?: string | number | Range;
  raw?: boolean;
  rawNumbers?: boolean;
  defval?: unknown;
  blankrows?: boolean;
  skipHidden?: boolean;
  UTC?: boolean;
}

export interface UtilityCell {
  l?: CellHyperlink;
  s?: CellFill;
  t: string;
  v?: unknown;
  w?: string;
  z?: string | number;
  f?: string;
  F?: string;
}

export interface ArraySheetOptions {
  origin?: string | number | CellAddress;
  dense?: boolean;
  dateNF?: string;
  cellDates?: boolean;
  sheetStubs?: boolean;
  nullError?: boolean;
  UTC?: boolean;
  date1904?: boolean;
}

export interface JsonSheetOptions extends Omit<ArraySheetOptions, 'sheetStubs' | 'date1904'> {
  header?: string[];
  skipHeader?: boolean;
}

export type ArrayCell = number | boolean | string | Date | null | undefined | UtilityCell
  | readonly [number | boolean | string | Date | null, string];

export interface UtilityWorksheet extends JsonSource {
  [address: `${string}${number}`]: UtilityCell | undefined;
}

export interface JsonSource {
  '!ref'?: string;
  '!autofilter'?: { ref: string };
  '!merges'?: Range[];
  '!margins'?: SheetMargins;
  '!protect'?: SheetProtection;
  '!data'?: ReadonlyArray<ReadonlyArray<UtilityCell | undefined> | undefined>;
  '!rows'?: (RowLayout | undefined)[];
  '!cols'?: (ColumnLayout | undefined)[];
}

export interface DraftDocumentProperties {
  Author?: string;
  Title?: string;
  Subject?: string;
  Comments?: string;
  Identifier?: string;
  Language?: string;
  Keywords?: string;
  Category?: string;
  LastAuthor?: string;
  RevNumber?: string | number;
  Version?: string;
  ContentStatus?: string;
  CreatedDate?: string | Date;
  ModifiedDate?: string | Date;
  LastPrinted?: string | Date;
  Company?: string;
  Manager?: string;
}

export interface UtilityWorkbook {
  bookType?: 'html' | 'xlml' | 'dif' | 'sylk' | 'rtf' | 'fods' | 'ods';
  Props?: DraftDocumentProperties;
  Custprops?: Record<string, DocumentProperty>;
  Workbook?: LoadedWorkbookMetadata;
  SheetNames: string[];
  Sheets: Record<string, UtilityWorksheet>;
}

export interface SheetCsvOptions {
  dateNF?: string;
  FS?: string;
  RS?: string;
  skipHidden?: boolean;
  blankrows?: boolean;
  strip?: boolean;
  forceQuotes?: boolean;
  rawNumbers?: boolean;
  type?: 'string';
}

export interface SheetHtmlOptions {
  id?: string;
  editable?: boolean;
  header?: string;
  footer?: string;
}

export interface TableReadOptions {
  cellDates?: boolean;
  cellNF?: boolean;
  dateNF?: string;
  UTC?: boolean;
  raw?: boolean;
  dense?: boolean;
  display?: boolean;
  sheetRows?: number;
  origin?: string | number | CellAddress;
  sheet?: string;
}

export namespace utils {
  const consts: { SHEET_VISIBLE: 0; SHEET_HIDDEN: 1; SHEET_VERY_HIDDEN: 2 };
  function table_to_sheet(table: HTMLTableElement, options?: TableReadOptions): UtilityWorksheet;
  function table_to_book(table: HTMLTableElement, options?: TableReadOptions): UtilityWorkbook;
  function sheet_add_dom<T extends JsonSource>(sheet: T, table: HTMLTableElement, options?: TableReadOptions): T;
  function sheet_to_html(sheet: JsonSource, options?: SheetHtmlOptions): string;
  function book_set_sheet_visibility(book: SheetNamesWorkbook | UtilityWorkbook, sheet: number | string, visibility: 0 | 1 | 2): void;
  function sheet_new(options?: { dense?: boolean }): UtilityWorksheet;
  function sheet_get_cell(sheet: UtilityWorksheet | Worksheet | DenseWorksheet, address: string | CellAddress): UtilityCell;
  function sheet_get_cell(sheet: UtilityWorksheet | Worksheet | DenseWorksheet, row: number, column?: number): UtilityCell;
  function sheet_set_array_formula<T extends JsonSource>(sheet: T, range: string | Range, formula: string, dynamic?: boolean): T;
  function cell_add_comment(cell: object, text: string, author?: string): void;
  function cell_set_hyperlink<T extends object>(cell: T, target?: string | null, tooltip?: string): T;
  function cell_set_internal_link<T extends object>(cell: T, target: string, tooltip?: string): T;
  function cell_set_number_format<T extends UtilityCell>(cell: T, format: string | number): T;
  function format_cell(cell: UtilityCell | null | undefined, value?: unknown, options?: { dateNF?: string }): string | number;
  function sheet_to_csv(sheet: JsonSource | null | undefined, options?: SheetCsvOptions): string;
  function sheet_to_txt(sheet: JsonSource | null | undefined, options?: SheetCsvOptions): string;
  function sheet_to_formulae(sheet: JsonSource | null | undefined, options?: { values?: boolean }): string[];
  function book_new(sheet?: UtilityWorksheet, name?: string): UtilityWorkbook;
  function book_append_sheet(book: UtilityWorkbook, sheet: UtilityWorksheet, name?: string, roll?: boolean): string;
  function json_to_sheet(rows: readonly object[], options?: JsonSheetOptions): UtilityWorksheet;
  function sheet_add_json<T extends JsonSource>(sheet: T, rows: readonly object[], options?: JsonSheetOptions): T;
  function sheet_add_json(sheet: null | undefined, rows: readonly object[], options?: JsonSheetOptions): UtilityWorksheet;
  function aoa_to_sheet(rows: ReadonlyArray<ReadonlyArray<ArrayCell> | undefined>, options?: ArraySheetOptions): UtilityWorksheet;
  function sheet_add_aoa<T extends JsonSource>(sheet: T, rows: ReadonlyArray<ReadonlyArray<ArrayCell> | undefined>, options?: ArraySheetOptions): T;
  function sheet_add_aoa(sheet: null | undefined, rows: ReadonlyArray<ReadonlyArray<ArrayCell> | undefined>, options?: ArraySheetOptions): UtilityWorksheet;
  function sheet_to_json<T = unknown[]>(sheet: JsonSource | null | undefined, options: SheetJsonOptions & { header: 1 }): T[];
  function sheet_to_json<T = Record<string, unknown>>(sheet: JsonSource | null | undefined, options?: SheetJsonOptions): T[];
  const sheet_to_row_object_array: typeof sheet_to_json;
  function encode_row(row: number): string;
  function decode_row(text: string): number;
  function encode_col(column: number): string;
  function decode_col(text: string): number;
  function encode_cell(cell: CellAddress): string;
  function decode_cell(text: string): CellAddress;
  function encode_range(start: CellAddress | string, end: CellAddress | string): string;
  function encode_range(range: Range, index?: number): string;
  function decode_range(text: string): Range;
  function split_cell(text: string): string[];
}

/** Number format registry and date serial utilities. */
export namespace SSF {
  const _table: Record<number, string>;
  function get_table(): Record<number, string>;
  function load(format: string, index?: number): number;
  function load_table(formats: Readonly<Record<number, string>>): void;
  function is_date(format: string): boolean;
  function parse_date_code(serial: number, options?: { date1904?: boolean }, alternateCalendar?: boolean):
    { D: number; T: number; u: number; y: number; m: number; d: number; H: number; M: number; S: number; q: number } | null;
  function format(code: number | string, value: unknown,
    options?: { date1904?: boolean; dateNF?: string; table?: Readonly<Record<number, string>> }): string | number;
}

export namespace stream {
  interface Readable {
    push(chunk: unknown): unknown;
    destroy?(error: Error): unknown;
  }
  type ReadableOptions = { objectMode?: boolean };
  function set_readable(factory: ((options?: ReadableOptions) => Readable) |
    (new (options?: ReadableOptions) => Readable)): void;
  function to_xlml(book: Workbook | UtilityWorkbook | SelectedWorkbook | DenseWorkbook,
    options?: { stride?: number; Props?: DraftDocumentProperties }): Readable;
  function to_csv(sheet: JsonSource | null | undefined, options?: SheetCsvOptions): Readable;
  function to_html(sheet: JsonSource, options?: SheetHtmlOptions): Readable;
  function to_json(sheet: JsonSource | null | undefined, options?: SheetJsonOptions): Readable;
}

export interface ZipContainerEntry {
  content?: ByteInput | string;
  type?: number;
  dir?: boolean;
  asUint8Array?(): Uint8Array;
  asNodeBuffer?(): Uint8Array;
  asBinary?(): string;
}
export type ZipContainer =
  | { FullPaths: readonly string[]; FileIndex: readonly ZipContainerEntry[] }
  | { files: Readonly<Record<string, ZipContainerEntry>> };

type CompoundContainer = Extract<ZipContainer, { FullPaths: readonly string[] }>;
type BiffReadOptions = Pick<FileReadOptions, 'dense' | 'sheetRows' | 'cellDates' | 'cellText' | 'cellFormula' | 'codepage' | 'sheetStubs' | 'dateNF' | 'WTF' | 'xlfn'> &
  { cellNF?: boolean; cellStyles?: boolean; cellHTML?: boolean; sheets?: SheetSelection; bookSheets?: boolean; bookFiles?: boolean; bookProps?: boolean };
export function parse_xlscfb(container: CompoundContainer, options: BiffReadOptions & { bookProps: true; bookSheets: true }): PropertiesWorkbook & SheetNamesWorkbook;
export function parse_xlscfb(container: CompoundContainer, options: BiffReadOptions & { bookProps: true; bookSheets?: false }): PropertiesWorkbook;
export function parse_xlscfb(container: CompoundContainer, options: BiffReadOptions & { bookSheets: true }): SheetNamesWorkbook;
export function parse_xlscfb(container: CompoundContainer, options: BiffReadOptions & { dense: true }): DenseWorkbook;
export function parse_xlscfb(container: CompoundContainer, options: BiffReadOptions & { sheets: SheetSelection }): SelectedWorkbook;
export function parse_xlscfb(container: CompoundContainer, options?: BiffReadOptions): Workbook;

type ZipReadOptions = Omit<FileReadOptions, 'type'>;
export function parse_zip(container: ZipContainer, options: ZipReadOptions & { bookProps: true; bookSheets: true }): PropertiesWorkbook & { SheetNames: string[] };
export function parse_zip(container: ZipContainer, options: ZipReadOptions & { bookProps: true }): PropertiesWorkbook;
export function parse_zip(container: ZipContainer, options: ZipReadOptions & { bookSheets: true }): SheetNamesWorkbook;
export function parse_zip(container: ZipContainer, options: ZipReadOptions & { dense: true }): DenseWorkbook;
export function parse_zip(container: ZipContainer, options: ZipReadOptions & { sheets: SheetSelection }): SelectedWorkbook;
export function parse_zip(container: ZipContainer, options?: ZipReadOptions): Workbook;

/** Version of the SheetJS CE API targeted by this adapter. */
export const version: '0.20.3';
export interface CodepageTable {
  utils: {
    decode(codepage: number, bytes: Uint8Array): string;
    encode?(codepage: number, text: string, format?: 'str'): string | ArrayLike<number>;
  };
}
export function set_cptable(table?: CodepageTable): void;

declare const sheetjs: {
  parse_zip: typeof parse_zip;
  parse_xlscfb: typeof parse_xlscfb;
  read: typeof read;
  readFile: typeof readFile;
  readFileSync: typeof readFileSync;
  write: typeof write;
  writeFile: typeof writeFile;
  writeFileSync: typeof writeFileSync;
  writeFileAsync: typeof writeFileAsync;
  writeXLSX: typeof writeXLSX;
  writeFileXLSX: typeof writeFileXLSX;
  set_fs: typeof set_fs;
  set_cptable: typeof set_cptable;
  utils: typeof utils;
  stream: typeof stream;
  SSF: typeof SSF;
};
export default sheetjs;
