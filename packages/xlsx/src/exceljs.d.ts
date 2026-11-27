export interface CellBorderSide {
  style?: 'thin' | 'dotted' | 'hair' | 'medium' | 'double' | 'thick' | 'dashed' | 'dashDot'
    | 'dashDotDot' | 'slantDashDot' | 'mediumDashed' | 'mediumDashDotDot' | 'mediumDashDot';
  color?: TabColor;
}

export interface CellBorder {
  top?: CellBorderSide;
  left?: CellBorderSide;
  bottom?: CellBorderSide;
  right?: CellBorderSide;
  diagonal?: CellBorderSide & { up?: boolean; down?: boolean };
}

export type CellGradientFill = {
  type: 'gradient';
  stops: { position: number; color: TabColor }[];
} & (
  { gradient: 'angle'; degree: number } |
  { gradient: 'path'; center: { left: number; top: number; right?: number; bottom?: number } }
);

export interface CellPatternFill {
  type: 'pattern';
  pattern: 'none' | 'solid' | 'darkVertical' | 'darkHorizontal' | 'darkGrid' | 'darkTrellis' | 'darkDown' | 'darkUp'
    | 'lightVertical' | 'lightHorizontal' | 'lightGrid' | 'lightTrellis' | 'lightDown' | 'lightUp'
    | 'darkGray' | 'mediumGray' | 'lightGray' | 'gray125' | 'gray0625';
  fgColor?: TabColor;
  bgColor?: TabColor;
}

export interface CellFont {
  name?: string;
  size?: number;
  bold?: boolean;
  italic?: boolean;
  strike?: boolean;
  underline?: boolean | 'none' | 'single' | 'double' | 'singleAccounting' | 'doubleAccounting';
  vertAlign?: 'baseline' | 'superscript' | 'subscript';
  scheme?: 'none' | 'minor' | 'major';
  family?: number;
  charset?: number;
  color?: TabColor;
}

export interface CellAlignment {
  horizontal?: 'left' | 'center' | 'right' | 'fill' | 'centerContinuous' | 'distributed' | 'justify';
  vertical?: 'top' | 'middle' | 'bottom' | 'distributed' | 'justify';
  textRotation?: number | 'vertical';
  wrapText?: boolean;
  indent?: number;
  shrinkToFit?: boolean;
  readingOrder?: 'ltr' | 'rtl';
}

export const ValueType: {
  readonly Null: 0; readonly Merge: 1; readonly Number: 2; readonly String: 3;
  readonly Date: 4; readonly Hyperlink: 5; readonly Formula: 6; readonly SharedString: 7;
  readonly RichText: 8; readonly Boolean: 9; readonly Error: 10;
};
export const FormulaType: { readonly None: 0; readonly Master: 1; readonly Shared: 2 };
export const RelationshipType: {
  readonly None: 0; readonly OfficeDocument: 1; readonly Worksheet: 2;
  readonly CalcChain: 3; readonly SharedStrings: 4; readonly Styles: 5;
  readonly Theme: 6; readonly Hyperlink: 7;
};
export const DocumentType: { readonly Xlsx: 1 };
export const ReadingOrder: { readonly LeftToRight: 1; readonly RightToLeft: 2 };

export const ErrorValue: {
  readonly NotApplicable: '#N/A';
  readonly Ref: '#REF!';
  readonly Name: '#NAME?';
  readonly DivZero: '#DIV/0!';
  readonly Null: '#NULL!';
  readonly Value: '#VALUE!';
  readonly Num: '#NUM!';
};

export interface CellErrorValue {
  error: typeof ErrorValue[keyof typeof ErrorValue] | '#GETTING_DATA';
}

export interface CellRichTextValue {
  richText: { text: string; font?: CellFont }[];
}

export interface CellHyperlinkValue {
  text: string;
  hyperlink: string;
  tooltip?: string;
}

export interface CellFormulaValue {
  formula: string;
  result?: number | string | boolean | Date | CellErrorValue;
}

export interface CellArrayFormulaValue extends CellFormulaValue {
  shareType: 'array';
  ref: string;
}

export interface CellSharedFormulaMasterValue extends CellFormulaValue {
  shareType: 'shared';
  ref: string;
}

export interface CellSharedFormulaValue {
  sharedFormula: string;
  result?: CellFormulaValue['result'];
}

export type DataValidation = (
  { type: 'any'; formulae?: [] } |
  { type: 'list' | 'custom' | 'whole' | 'decimal' | 'date' | 'time' | 'textLength'; formulae: (string | number | Date)[] }
) & {
  operator?: 'between' | 'notBetween' | 'equal' | 'notEqual' | 'lessThan' | 'lessThanOrEqual'
    | 'greaterThan' | 'greaterThanOrEqual';
  allowBlank?: boolean;
  showInputMessage?: boolean;
  showErrorMessage?: boolean;
  promptTitle?: string;
  prompt?: string;
  errorTitle?: string;
  error?: string;
  errorStyle?: 'stop' | 'warning' | 'information';
};

export interface CellStyle {
  numFmt?: string;
  font?: CellFont;
  fill?: CellPatternFill | CellGradientFill;
  border?: CellBorder;
  alignment?: CellAlignment;
  protection?: { locked?: boolean; hidden?: boolean };
}

export interface BufferedWorkbookImage {
  type: 'image';
  extension: string;
  buffer: Uint8Array;
}

export interface FileWorkbookImage {
  type: 'image';
  extension: string;
  filename: string;
}

export type WorkbookImage = BufferedWorkbookImage | FileWorkbookImage;

export interface CellNote {
  margins: { insetmode: 'auto' | 'custom'; inset: number[] };
  protection: { locked: 'True' | 'False'; lockText: 'True' | 'False' };
  editAs: 'absolute' | 'oneCells' | 'twoCells';
  texts: { text: string; font?: CellStyle['font'] }[];
}

export interface CellNoteInput {
  texts: CellNote['texts'];
  margins?: Partial<CellNote['margins']>;
  protection?: Partial<CellNote['protection']>;
  editAs?: CellNote['editAs'];
}

export interface CellModel {
  address: string;
  type: typeof ValueType[keyof typeof ValueType];
  style: CellStyle;
  value?: Cell['value'];
  master?: string;
  text?: string;
  hyperlink?: string;
  tooltip?: string;
  shareType?: 'array' | 'shared';
  ref?: string;
  formula?: string;
  sharedFormula?: string;
  result?: CellFormulaValue['result'];
  comment?: {
    type: 'note';
    note: CellNote;
  };
}

export type CellModelInput = Omit<CellModel, 'address' | 'style'> & { address?: string; style?: CellStyle };

export interface Cell<Media extends WorkbookImage = BufferedWorkbookImage> {
  destroy(): void;
  get model(): CellModel;
  set model(value: CellModelInput);
  style: CellStyle;
  readonly workbook: Workbook<Media>;
  get name(): string | undefined;
  set name(value: string);
  names: string[];
  addName(name: string): void;
  removeName(name: string): void;
  removeAllNames(): void;
  readonly fullAddress: { sheetName: string; address: string; row: number; col: number };
  readonly $col$row: string;
  readonly isHyperlink: boolean;
  readonly address: string;
  readonly row: number;
  readonly col: number;
  readonly worksheet: Worksheet<Media>;
  get border(): CellBorder | undefined;
  set border(value: CellBorder);
  get fill(): CellPatternFill | CellGradientFill | undefined;
  set fill(value: CellPatternFill | CellGradientFill);
  get font(): CellFont | undefined;
  set font(value: CellFont);
  get alignment(): CellAlignment | undefined;
  set alignment(value: CellAlignment);
  get protection(): { locked: boolean; hidden: boolean } | undefined;
  set protection(value: { locked?: boolean; hidden?: boolean });
  get numFmt(): string | undefined;
  set numFmt(code: string);
  readonly master: Cell<Media>;
  merge(master: Cell<Media>, ignoreStyle?: boolean): void;
  unmerge(): void;
  addMergeRef(): void;
  releaseMergeRef(): void;
  readonly isMerged: boolean;
  isMergedTo(master: Cell<Media>): boolean;
  readonly type: typeof ValueType[keyof typeof ValueType];
  readonly effectiveType: typeof ValueType[keyof typeof ValueType];
  readonly formulaType: typeof FormulaType[keyof typeof FormulaType] | undefined;
  get dataValidation(): DataValidation | undefined;
  set dataValidation(value: DataValidation | null | undefined);
  get note(): string | CellNote | undefined;
  set note(value: string | CellNoteInput | null | undefined);
  readonly hyperlink: string | undefined;
  readonly text: string;
  readonly html: string;
  toString(): string;
  toCsvString(): string | number;
  readonly formula: string | undefined;
  readonly result: CellFormulaValue['result'];
  get value(): number | string | boolean | Date | CellErrorValue | CellFormulaValue | CellArrayFormulaValue | CellSharedFormulaMasterValue | CellSharedFormulaValue | CellHyperlinkValue | CellRichTextValue | null;
  set value(value: number | string | boolean | Date | CellErrorValue | CellHyperlinkValue | CellRichTextValue | CellFormulaValue | CellArrayFormulaValue | CellSharedFormulaMasterValue | CellSharedFormulaValue | null);
}

export interface RowModel {
  cells: CellModel[];
  number: number;
  min: number;
  max: number;
  height: number | undefined;
  style: CellStyle;
  hidden: boolean;
  outlineLevel: number;
  collapsed: boolean;
}

export type RowModelInput = Partial<Omit<RowModel, 'cells' | 'number'>> & { number: number; cells: CellModelInput[] };

export interface Row<Media extends WorkbookImage = BufferedWorkbookImage> {
  destroy(): void;
  get model(): RowModel | null;
  set model(value: RowModelInput);
  commit(): void;
  get border(): CellBorder | undefined;
  set border(value: CellBorder);
  get fill(): CellStyle['fill'];
  set fill(value: NonNullable<CellStyle['fill']>);
  get protection(): { locked: boolean; hidden: boolean } | undefined;
  set protection(value: { locked?: boolean; hidden?: boolean });
  get alignment(): CellAlignment | undefined;
  set alignment(value: CellAlignment);
  get font(): CellFont | undefined;
  set font(value: CellFont);
  style: ColumnStyle;
  get numFmt(): string | undefined;
  set numFmt(value: string);
  splice(start: number, count: number, ...values: Cell['value'][]): void;
  readonly dimensions: { min: number; max: number } | null;
  addPageBreak(left?: number, right?: number): void;
  readonly number: number;
  get height(): number | undefined;
  set height(value: number);
  hidden: boolean;
  outlineLevel: number;
  readonly collapsed: boolean;
  get values(): Array<Cell['value']>;
  set values(value: RowInput | null | undefined);
  readonly cellCount: number;
  readonly actualCellCount: number;
  readonly hasValues: boolean;
  eachCell(iteratee: (cell: Cell<Media>, column: number) => void): void;
  eachCell(options: { includeEmpty?: boolean }, iteratee: (cell: Cell<Media>, column: number) => void): void;
  readonly worksheet: Worksheet<Media>;
  getCell(column: number | string): Cell<Media>;
  getCellEx(location: { address: string; row: number; col: number }): Cell<Media>;
  findCell(column: number): Cell<Media> | undefined;
}

export type RowInput = readonly (Cell['value'] | undefined)[] | Record<string, Cell['value'] | undefined>;

export interface Column<Media extends WorkbookImage = BufferedWorkbookImage> {
  get border(): CellBorder | undefined;
  set border(value: CellBorder);
  get fill(): CellStyle['fill'];
  set fill(value: NonNullable<CellStyle['fill']>);
  get protection(): { locked: boolean; hidden: boolean } | undefined;
  set protection(value: { locked?: boolean; hidden?: boolean });
  get alignment(): CellAlignment | undefined;
  set alignment(value: CellAlignment);
  get font(): CellFont | undefined;
  set font(value: CellFont);
  style: ColumnStyle;
  get numFmt(): string | undefined;
  set numFmt(value: string);
  readonly worksheet: Worksheet<Media>;
  get defn(): ColumnDefinition;
  set defn(value: ColumnDefinition | null | undefined);
  readonly headers: Array<string | undefined>;
  readonly headerCount: number;
  readonly isCustomWidth: boolean;
  readonly isDefault: boolean;
  equivalentTo(other: Column<Media>): boolean;
  toString(): string;
  eachCell(iteratee: (cell: Cell<Media>, row: number) => void): void;
  eachCell(options: { includeEmpty?: boolean }, iteratee: (cell: Cell<Media>, row: number) => void): void;
  get values(): Array<Cell['value'] | undefined>;
  set values(value: readonly (Cell['value'] | undefined)[] | null | undefined);
  readonly number: number;
  readonly letter: string;
  key: string | undefined;
  header: string | string[] | undefined;
  get width(): number | undefined;
  set width(value: number);
  hidden: boolean;
  outlineLevel: number;
  readonly collapsed: boolean;
}

export type ColumnStyle = CellStyle;

export interface ColumnDefinition {
  style?: ColumnStyle;
  key?: string;
  header?: string | string[];
  width?: number;
  hidden?: boolean;
  outlineLevel?: number;
}

export type FilterPoint = string | { row: number; column: number };
export type AutoFilter = string | { from: FilterPoint; to: FilterPoint };

export interface PageMargins {
  left: number;
  right: number;
  top: number;
  bottom: number;
  header: number;
  footer: number;
}

export interface PrintOptions {
  showGridLines: boolean;
  showRowColHeaders: boolean;
  horizontalCentered: boolean;
  verticalCentered: boolean;
}

export interface PageLayoutSettings {
  orientation: 'portrait' | 'landscape';
  paperSize?: number;
  scale: number;
  fitToWidth: number;
  fitToHeight: number;
  fitToPage: boolean;
  pageOrder: 'downThenOver' | 'overThenDown';
  cellComments: 'None' | 'none' | 'asDisplayed' | 'asDisplyed' | 'atEnd';
  errors: 'displayed' | 'blank' | 'dash' | 'NA';
  blackAndWhite: boolean;
  draft: boolean;
  useFirstPageNumber: boolean;
  usePrinterDefaults: boolean;
  horizontalDpi: number;
  verticalDpi: number;
  firstPageNumber?: number;
  copies: number;
  printArea?: string;
  printTitlesRow?: string;
  printTitlesColumn?: string;
}

export interface PageSetupOptions extends Partial<PrintOptions & PageLayoutSettings> {
  margins?: Partial<PageMargins>;
}

export interface HeaderFooter {
  oddHeader: string | null;
  oddFooter: string | null;
  evenHeader: string | null;
  evenFooter: string | null;
  firstHeader: string | null;
  firstFooter: string | null;
  differentOddEven: boolean;
  differentFirst: boolean;
}

export interface WorksheetView {
  state?: 'normal' | 'frozen' | 'split';
  xSplit?: number;
  ySplit?: number;
  zoomScale?: number;
  zoomScaleNormal?: number;
  showRuler?: boolean;
  showRowColHeaders?: boolean;
  tabSelected?: boolean;
  style?: 'normal' | 'pageBreakPreview' | 'pageLayout';
  showGridLines?: boolean;
  rightToLeft?: boolean;
  workbookViewId?: number;
  topLeftCell?: string;
  activePane?: 'topLeft' | 'topRight' | 'bottomLeft' | 'bottomRight';
  activeCell?: string;
}

export type ConditionalFormattingThreshold =
  | { type: 'min' | 'max' }
  | { type: 'num' | 'percent' | 'percentile'; value: number }
  | { type: 'formula'; value: string };

export type ExtendedDataBarThreshold = ConditionalFormattingThreshold | { type: 'autoMin' | 'autoMax' };

export type ConditionalFormattingRule = { priority?: number; stopIfTrue?: boolean; style?: CellStyle } & (
  | { type: 'expression'; formulae: [string | number] }
  | { type: 'cellIs'; operator: 'between' | 'notBetween' | 'equal' | 'notEqual' | 'greaterThan' | 'lessThan' |
      'greaterThanOrEqual' | 'lessThanOrEqual'; formulae: [string | number, (string | number)?] }
  | { type: 'containsText'; operator?: 'containsText' | 'notContains' | 'beginsWith' | 'endsWith'; text: string }
  | { type: 'containsText'; operator: 'containsBlanks' | 'notContainsBlanks' | 'containsErrors' | 'notContainsErrors' }
  | { type: 'top10'; rank: number; percent?: boolean; bottom?: boolean }
  | { type: 'iconSet'; cfvo: (ConditionalFormattingThreshold & { gte?: boolean })[]; iconSet?: string;
      reverse?: boolean; showValue?: boolean; custom?: boolean; icons?: { iconSet: string; iconId?: number }[] }
  | { type: 'timePeriod'; timePeriod: 'lastWeek' | 'thisWeek' | 'nextWeek' | 'yesterday' | 'today' |
      'tomorrow' | 'last7Days' | 'lastMonth' | 'thisMonth' | 'nextMonth'; formulae?: [string] }
  | { type: 'dataBar'; cfvo: [ConditionalFormattingThreshold, ConditionalFormattingThreshold]; color: TabColor;
      minLength?: number; maxLength?: number; showValue?: boolean;
      border?: boolean; gradient?: boolean; negativeBarColorSameAsPositive?: boolean;
      negativeBarBorderColorSameAsPositive?: boolean; axisPosition?: 'auto' | 'middle' | 'none';
      direction?: 'leftToRight' | 'rightToLeft' | 'context';
      fillColor?: TabColor; borderColor?: TabColor; negativeFillColor?: TabColor;
      negativeBorderColor?: TabColor; axisColor?: TabColor;
      extendedCfvo?: [ExtendedDataBarThreshold, ExtendedDataBarThreshold] }
  | { type: 'colorScale'; cfvo: ConditionalFormattingThreshold[]; color: TabColor[] }
  | { type: 'aboveAverage'; aboveAverage?: boolean; equalAverage?: boolean; stdDev?: number }
  | { type: 'duplicateValues' | 'uniqueValues' });

export interface ConditionalFormattingBlock {
  ref: string;
  rules: ({ type: string; priority?: number; formulae?: string[]; dxfId?: number; style?: CellStyle } & Record<string, unknown>)[];
}

export interface TableColumn {
  name: string;
  filterButton: boolean;
  get totalsRowFunction(): string | undefined;
  set totalsRowFunction(value: TableColumnOptions['totalsRowFunction']);
  totalsRowLabel?: string;
  totalsRowFormula?: string;
  totalsRowResult?: CellFormulaValue['result'];
  style?: Partial<CellStyle>;
  readonly calculatedColumnFormula?: string;
}

export interface TableColumnOptions {
  name: string;
  filterButton?: boolean;
  totalsRowLabel?: string;
  totalsRowFunction?: 'none' | 'sum' | 'average' | 'count' | 'countNums' | 'max' | 'min' | 'stdDev' | 'var' | 'custom';
  totalsRowFormula?: string;
  totalsRowResult?: CellFormulaValue['result'];
  style?: Partial<CellStyle>;
}

export type TableCellValue = string | number | boolean | Date | CellErrorValue | null | undefined |
  { formula: string; result?: CellFormulaValue['result'] | null; shareType?: never; ref?: never };

export interface Table {
  readonly name: string;
  readonly displayName: string;
  readonly ref: string;
  readonly tableRef: string;
  readonly headerRow: boolean;
  readonly totalsRow: boolean;
  get theme(): string | null | undefined;
  set theme(value: string | null);
  showFirstColumn: boolean;
  showLastColumn: boolean;
  showRowStripes: boolean;
  showColumnStripes: boolean;
  readonly columns: TableColumn[];
  addRow(values: TableCellValue[], rowNumber?: number): void;
  addColumn(column: TableColumnOptions, values: TableCellValue[], columnNumber?: number): void;
  removeColumns(columnNumber: number, count?: number): void;
  removeRows(rowNumber: number, count?: number): void;
  commit(): void;
  getColumn(index: number): TableColumn | undefined;
}

export interface ImageAnchor {
  readonly nativeCol: number;
  readonly nativeColOff: number;
  readonly nativeRow: number;
  readonly nativeRowOff: number;
  readonly col: number;
  readonly row: number;
}

export type ImageMarker = { col: number; row: number } | Pick<ImageAnchor, 'nativeCol' | 'nativeRow'> &
  Partial<Pick<ImageAnchor, 'nativeColOff' | 'nativeRowOff'>>;

export type ImageRange = string | ({
  tl: ImageMarker;
  editAs?: 'twoCell' | 'oneCell' | 'absolute';
  hyperlinks?: { hyperlink: string; tooltip?: string };
} & ({ br: ImageMarker; ext?: never } | { ext: { width: number; height: number }; br?: never }));

export interface WorksheetImage {
  type: 'image';
  imageId: number;
  name: string;
  altText: string;
  range: { tl: ImageAnchor; br?: ImageAnchor; ext?: { width: number; height: number };
    editAs: 'twoCell' | 'oneCell' | 'absolute'; hyperlinks?: { hyperlink: string; tooltip?: string } };
}

export type WorksheetState = 'visible' | 'hidden' | 'veryHidden';

export interface TabColor {
  argb?: string;
  theme?: number;
  indexed?: number;
  tint?: number;
}

export interface OutlineProperties {
  summaryBelow?: boolean;
  summaryRight?: boolean;
}

export interface WorksheetProperties {
  outlineProperties: OutlineProperties;
  tabColor?: TabColor | null;
  dyDescent: number;
  defaultRowHeight: number;
  defaultColWidth?: number;
  outlineLevelRow: number;
  outlineLevelCol: number;
}

export interface WorksheetProtection {
  objects: boolean;
  scenarios: boolean;
  selectLockedCells: boolean;
  selectUnlockedCells: boolean;
  formatCells: boolean;
  formatColumns: boolean;
  formatRows: boolean;
  insertColumns: boolean;
  insertRows: boolean;
  insertHyperlinks: boolean;
  deleteColumns: boolean;
  deleteRows: boolean;
  sort: boolean;
  autoFilter: boolean;
  pivotTables: boolean;
  spinCount: number;
}

export interface WorksheetDimensions {
  readonly top: number;
  readonly left: number;
  readonly bottom: number;
  readonly right: number;
  readonly tl: string;
  readonly br: string;
  readonly $t$l: string;
  readonly $b$r: string;
  readonly range: string;
  readonly $range: string;
  readonly count: number;
  readonly shortRange: string;
  readonly $shortRange: string;
}

export interface WorksheetMergeRange {
  top: number;
  left: number;
  bottom: number;
  right: number;
  sheetName?: string;
}
type WorksheetMergeEndpoints =
  | [first: string, last: string, sheetName?: string]
  | [top: number, left: number, bottom: number, right: number, sheetName?: string];
type WorksheetMergeArgumentList =
  | Readonly<WorksheetMergeEndpoints>
  | readonly [range: string | WorksheetMergeRange | WorksheetMergeArgumentList];
type WorksheetMergeArguments =
  | WorksheetMergeEndpoints
  | [range: string | WorksheetMergeRange | WorksheetMergeArgumentList];

type WorksheetImageModelAnchor = Omit<ImageAnchor, 'col' | 'row'>;

export interface WorksheetModel extends Pick<Worksheet, 'id' | 'name' | 'properties' | 'state' | 'pageSetup' |
  'headerFooter' | 'rowBreaks' | 'views' | 'autoFilter' | 'sheetProtection' | 'conditionalFormattings'> {
  dataValidations: Record<string, DataValidation>;
  rows: RowModel[];
  dimensions: WorksheetDimensions & { model: Omit<WorksheetMergeRange, 'sheetName'> };
  merges: string[];
  cols: {
    min: number; max: number; width: number; style: CellStyle; isCustomWidth: boolean;
    hidden: boolean; outlineLevel: number; collapsed: boolean;
  }[] | undefined;
  tables: WorksheetTableModel[];
  media: ({ type: 'background'; imageId: number } | {
    type: 'image'; imageId: number; hyperlinks?: WorksheetImage['range']['hyperlinks'];
    range: { tl: WorksheetImageModelAnchor; br?: WorksheetImageModelAnchor;
      ext?: WorksheetImage['range']['ext']; editAs: WorksheetImage['range']['editAs'] };
  })[];
}

type TableColumnDxfKey = 'headerRowDxfId' | 'dataDxfId' | 'totalsRowDxfId';
type TableBorderDxfKey = 'headerRowBorderDxfId' | 'tableBorderDxfId' | 'totalsRowBorderDxfId';
type TableDifferentialFormats<Key extends string> = Partial<Record<Key, number>> & {
  differentialStyles?: Partial<Record<Key, CellStyle>>;
};

export interface WorksheetTableModel extends TableDifferentialFormats<TableColumnDxfKey | TableBorderDxfKey> {
  name: string;
  displayName?: string;
  ref?: string;
  tableRef: string;
  headerRow?: boolean;
  totalsRow?: boolean;
  theme?: string | null;
  showFirstColumn?: boolean;
  showLastColumn?: boolean;
  showRowStripes?: boolean;
  showColumnStripes?: boolean;
  style?: Pick<WorksheetTableModel, 'theme' | 'showFirstColumn' | 'showLastColumn' | 'showRowStripes' | 'showColumnStripes'>;
  columns: (Omit<TableColumnOptions, 'style'> & TableDifferentialFormats<TableColumnDxfKey> & { calculatedColumnFormula?: string })[];
}

export interface WorksheetProtectionModel extends Partial<WorksheetProtection> {
  sheet?: boolean;
  algorithmName?: string;
  hashValue?: string;
  saltValue?: string;
}

export interface WorksheetModelInput {
  id?: number;
  name?: string;
  rows: RowModelInput[];
  /** Taken from the target sheet's existing page breaks. */
  rowBreaks?: WorksheetModel['rowBreaks'];
  /** Recomputed from the assigned rows. */
  dimensions?: WorksheetModel['dimensions'];
  media?: WorksheetModel['media'];
  tables?: WorksheetTableModel[];
  merges?: string[];
  mergeCells?: string[];
  cols?: (Pick<NonNullable<WorksheetModel['cols']>[number], 'min' | 'max'> &
    Partial<Omit<NonNullable<WorksheetModel['cols']>[number], 'min' | 'max'>>)[];
  dataValidations?: Record<string, DataValidation>;
  conditionalFormattings?: ({ ref: string; rules: ConditionalFormattingRule[] } | ConditionalFormattingBlock)[];
  sheetProtection?: WorksheetProtectionModel | null;
  properties?: Partial<WorksheetProperties>;
  pageSetup?: PageSetupOptions & { rowBreaks?: null; colBreaks?: null };
  headerFooter?: Partial<HeaderFooter>;
  views?: WorksheetView[];
  autoFilter?: AutoFilter | null;
  state?: WorksheetState;
}

export interface Worksheet<Media extends WorkbookImage = BufferedWorkbookImage> {
  get model(): WorksheetModel;
  set model(value: WorksheetModelInput);
  destroy(): void;
  readonly workbook: Workbook<Media>;
  readonly dimensions: WorksheetDimensions;
  readonly dataValidations: {
    model: Record<string, DataValidation>;
    add(reference: string, rule: DataValidation): DataValidation;
    find(reference: string): DataValidation | undefined;
    remove(reference: string): void;
  };
  readonly rowBreaks: ReadonlyArray<{ id: number; max: number; man: number; min?: number }>;
  readonly sheetProtection: WorksheetProtectionModel | null | undefined;
  protect(password?: string | null, options?: Partial<WorksheetProtection>): Promise<void>;
  unprotect(): void;
  state: WorksheetState;
  get headerFooter(): HeaderFooter;
  set headerFooter(value: Partial<HeaderFooter>);
  get pageSetup(): PrintOptions & PageLayoutSettings & { get margins(): PageMargins; set margins(value: Partial<PageMargins>); };
  set pageSetup(value: PageSetupOptions);
  get views(): Array<WorksheetView & { activePane?: string }>;
  set views(value: readonly WorksheetView[]);
  addTable(options: { name: string; displayName?: string; ref: string; headerRow?: boolean; totalsRow?: boolean;
    columns: TableColumnOptions[];
    rows: (Cell['value'] | undefined)[][]; style?: { theme?: string | null; showFirstColumn?: boolean; showLastColumn?: boolean;
      showRowStripes?: boolean; showColumnStripes?: boolean } }): Table;
  addImage(imageId: number, range: ImageRange): void;
  getImages(): WorksheetImage[];
  addBackgroundImage(imageId: number): void;
  getBackgroundImageId(): number | undefined;
  getTables(): Table[];
  getTable(name: string): Table | undefined;
  removeTable(name: string): void;
  readonly conditionalFormattings: ConditionalFormattingBlock[];
  removeConditionalFormatting(filter?: number | ((block: ConditionalFormattingBlock, index: number,
    values: ConditionalFormattingBlock[]) => unknown)): void;
  addConditionalFormatting(options: { ref: string; rules: ConditionalFormattingRule[] }): void;
  readonly properties: WorksheetProperties;
  /** @deprecated Use `properties.tabColor`. */
  tabColor?: TabColor | null;
  readonly hasMerges: boolean;
  mergeCells(...cells: WorksheetMergeArguments): void;
  mergeCellsWithoutStyle(...cells: WorksheetMergeArguments): void;
  unMergeCells(...cells: WorksheetMergeArguments): void;
  get autoFilter(): string | undefined;
  set autoFilter(value: AutoFilter | null | undefined);
  name: string;
  readonly id: number | undefined;
  getCell(reference: string): Cell<Media>;
  getCell(row: number | string, column: number | string): Cell<Media>;
  findCell(row: number | string, column: number | string): Cell<Media> | undefined;
  findCell(reference: string): Cell<Media> | undefined;
  readonly columnCount: number;
  readonly actualColumnCount: number;
  readonly lastColumn: Column<Media> | undefined;
  readonly rowCount: number;
  readonly actualRowCount: number;
  readonly lastRow: Row<Media> | undefined;
  eachRow(iteratee: (row: Row<Media>, rowNumber: number) => void): void;
  eachRow(options: { includeEmpty?: boolean }, iteratee: (row: Row<Media>, rowNumber: number) => void): void;
  getSheetValues(): Array<Row['values']>;
  addRow(values: RowInput, style?: 'n' | 'i' | 'i+'): Row<Media>;
  addRows(values: readonly RowInput[], style?: 'n' | 'i' | 'i+'): Row<Media>[];
  get columns(): Column<Media>[] | null;
  set columns(definitions: readonly ColumnDefinition[]);
  getColumn(reference: number | string): Column<Media>;
  getRow(row: number): Row<Media>;
  findRow(row: number): Row<Media> | undefined;
  findRows(start: number, count: number): Array<Row<Media> | undefined>;
  getColumnKey(key: string): Column<Media> | undefined;
  setColumnKey(key: string, column: Column<Media>): void;
  deleteColumnKey(key: string): void;
  eachColumnKey(iteratee: (column: Column<Media>, key: string) => void): void;
  insertRow(position: number, values: RowInput, style?: 'n' | 'i' | 'i+' | 'o' | 'o+'): Row<Media>;
  spliceColumns(position: number, count: number, ...values: (readonly (Cell['value'] | undefined)[])[]): void;
  duplicateRow(row: number, count: number, insert?: boolean): void;
  fillFormula(range: string | { top: number; left: number; bottom: number; right: number }, formula: string,
    results?: CellFormulaValue['result'][] | CellFormulaValue['result'][][] |
      ((row: number, column: number) => CellFormulaValue['result']) | undefined,
    shareType?: 'array' | 'shared'): void;
  spliceRows(position: number, count: number, ...values: RowInput[]): void;
  insertRows(position: number, values: readonly RowInput[], style?: 'n' | 'i' | 'i+' | 'o' | 'o+'): Row<Media>[] | undefined;
  getRows(start: number, count: number): Row<Media>[] | undefined;
}

export interface LoadOptions {
  ignoreNodes?: readonly ('autoFilter' | 'cols' | 'conditionalFormatting' | 'dataValidations' | 'headerFooter' | 'hyperlinks' | 'mergeCells' | 'pageMargins' | 'pageSetup' | 'printOptions' | 'rowBreaks' | 'sheetFormatPr' | 'sheetPr' | 'sheetViews' | 'tableParts')[];
  maxRows?: number;
  maxCols?: number;
}

export interface WorkbookView {
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  firstSheet?: number;
  activeTab?: number;
  visibility?: 'visible' | 'hidden' | 'veryHidden';
}

export interface DefinedNamesRanges {
  name: string;
  ranges: string[];
}

export type DefinedNameLocation = string | (
  { sheetName?: string } & (
    { address: string } |
    { row: number; col: number } |
    { top: number; left: number; bottom: number; right: number }
  )
);

export interface DefinedNameMatrixCell {
  sheetName?: string;
  address?: string;
  row: number;
  col: number;
  mark?: boolean;
}

export interface DefinedNameCellMatrix {
  findCellAt(sheetName: string | undefined, rowNumber: number, colNumber: number): DefinedNameMatrixCell | undefined;
  forEach(callback: (cell: DefinedNameMatrixCell) => void): void;
  map<T>(callback: (cell: DefinedNameMatrixCell) => T): T[];
}

export interface DefinedNameMatrix extends DefinedNameCellMatrix {
  addCell(address: string): void;
  addCellEx(location: DefinedNameLocation): void;
  findCell(address: string): DefinedNameMatrixCell | undefined;
  findCellEx(location: DefinedNameLocation, create?: boolean): DefinedNameMatrixCell | undefined;
  getCell(address: string): DefinedNameMatrixCell;
  getCellEx(location: DefinedNameLocation): DefinedNameMatrixCell;
  getCellAt(sheetName: string | undefined, row: number, col: number): DefinedNameMatrixCell;
  removeCellEx(location: DefinedNameLocation): void;
  spliceRows(sheetName: string, start: number, numDelete: number, numInsert?: number): void;
  spliceColumns(sheetName: string, start: number, numDelete: number, numInsert?: number): void;
  forEachInSheet(sheetName: string, callback: (cell: DefinedNameMatrixCell, row: number, col: number) => void): void;
}

export interface DefinedNames {
  add(location: string, name: string): void;
  addEx(location: DefinedNameLocation, name: string): void;
  remove(location: DefinedNameLocation, name: string): void;
  removeEx(location: DefinedNameLocation, name: string): void;
  removeAllNames(location: DefinedNameLocation): void;
  spliceRows(sheetName: string, start: number, numDelete: number, numInsert?: number): void;
  spliceColumns(sheetName: string, start: number, numDelete: number, numInsert?: number): void;
  model: DefinedNamesRanges[];
  getRanges(name: string, matrix?: DefinedNameCellMatrix): DefinedNamesRanges;
  normaliseMatrix(matrix: {
    forEachInSheet(sheetName: string, callback: (cell: DefinedNameMatrixCell, row: number, col: number) => void): void;
  }, sheetName: string): void;
  getMatrix(name: string): DefinedNameMatrix;
  getNames(address: string): string[];
  getNamesEx(location: DefinedNameLocation): string[];
  forEach(callback: (name: string, cell: { sheetName?: string; address: string; row: number; col: number }) => void): void;
}

type CsvQuoteSelection = boolean | boolean[] | Record<string, boolean>;
type CsvFormatterRow = unknown[] | Record<string, unknown>;
type CsvRowCallback = (error?: Error | null, row?: CsvFormatterRow | null) => void;
export interface CsvReadOptions {
  sheetName?: string;
  dateFormats?: string[];
  map?: (value: string, index: number) => unknown;
  parserOptions?: {
    delimiter?: string;
    quote?: string | null;
    escape?: string;
    encoding?: string;
    trim?: boolean;
    ltrim?: boolean;
    rtrim?: boolean;
    comment?: string;
    ignoreEmpty?: boolean;
    maxRows?: number;
    skipRows?: number;
    skipLines?: number;
  };
}

export interface CsvWriteOptions {
  sheetName?: string;
  sheetId?: number;
  dateFormat?: string;
  dateUTC?: boolean;
  map?: (value: unknown, index: number) => unknown;
  includeEmptyRows?: boolean;
  formatterOptions?: {
    delimiter?: string;
    rowDelimiter?: string;
    quote?: string | boolean;
    escape?: string;
    headers?: boolean | string[] | null;
    writeHeaders?: boolean;
    quoteColumns?: CsvQuoteSelection;
    quoteHeaders?: CsvQuoteSelection;
    includeEndRowDelimiter?: boolean;
    writeBOM?: boolean;
    alwaysWriteHeaders?: boolean;
    transform?: ((row: CsvFormatterRow) => CsvFormatterRow | null) |
      ((row: CsvFormatterRow, callback: CsvRowCallback) => void);
  };
}

export interface XlsxWriteOptions {
  useSharedStrings?: boolean;
  useStyles?: boolean;
  zip?: {
    compression?: 'STORE' | 'DEFLATE';
    compressionOptions?: { level?: number } | null;
  };
}

export interface WorkbookModel<Media extends WorkbookImage = WorkbookImage> extends Pick<Workbook, 'lastPrinted' | 'created' | 'modified' | 'properties' |
  'views' | 'company' | 'manager' | 'title' | 'subject' | 'keywords' | 'category' | 'description' |
  'language' | 'revision' | 'contentStatus' | 'calcProperties'> {
  creator: string;
  lastModifiedBy: string;
  worksheets: WorksheetModel[];
  sheets: WorksheetModel[];
  definedNames: DefinedNamesRanges[];
  themes: Record<string, string> | undefined;
  media: (Media | undefined)[];
}

export interface WorkbookModelInput<Media extends WorkbookImage = BufferedWorkbookImage>
  extends Partial<Omit<WorkbookModel<Media>, 'worksheets' | 'sheets' | 'media'>> {
  worksheets: WorksheetModelInput[];
  sheets?: Pick<WorksheetModelInput, 'id'>[];
  media?: (Media | { type?: 'image'; extension: 'png' | 'jpeg' | 'gif'; base64: string } | undefined)[];
}

export class Workbook<Media extends WorkbookImage = BufferedWorkbookImage> {
  get model(): WorkbookModel<Media>;
  set model(value: WorkbookModelInput<Media>);
  get company(): string | undefined;
  set company(value: string);
  get manager(): string | undefined;
  set manager(value: string);
  get created(): Date | undefined;
  set created(value: Date);
  get modified(): Date | undefined;
  set modified(value: Date);
  get lastPrinted(): Date | undefined;
  set lastPrinted(value: Date);
  get revision(): number | undefined;
  set revision(value: number);
  get creator(): string | undefined;
  set creator(value: string);
  get lastModifiedBy(): string | undefined;
  set lastModifiedBy(value: string);
  get title(): string | undefined;
  set title(value: string);
  get subject(): string | undefined;
  set subject(value: string);
  get keywords(): string | undefined;
  set keywords(value: string);
  get category(): string | undefined;
  set category(value: string);
  get description(): string | undefined;
  set description(value: string);
  get language(): string | undefined;
  set language(value: string);
  get identifier(): string | undefined;
  set identifier(value: string);
  get version(): string | undefined;
  set version(value: string);
  get contentStatus(): string | undefined;
  set contentStatus(value: string);
  get contentType(): string | undefined;
  set contentType(value: string);
  get properties(): { date1904: boolean };
  set properties(value: { date1904?: boolean });
  get calcProperties(): { fullCalcOnLoad: boolean };
  set calcProperties(value: { fullCalcOnLoad?: boolean });
  readonly csv: {
    read(stream: ReadableStream<Uint8Array>, options?: CsvReadOptions): Promise<Worksheet<Media>>;
    createInputStream(): never;
    write(stream: WritableStream<Uint8Array>, options?: CsvWriteOptions): Promise<void>;
    writeBuffer(options?: CsvWriteOptions): Promise<Uint8Array>;
  };
  readonly xlsx: {
    load(bytes: ArrayBuffer | Uint8Array | Blob, options?: LoadOptions & { base64?: false }): Promise<Workbook<Media>>;
    load(base64: string, options: LoadOptions & { base64: true }): Promise<Workbook<Media>>;
    read(stream: ReadableStream<Uint8Array>, options?: LoadOptions & { base64?: false }): Promise<Workbook<Media>>;
    write(stream: WritableStream<Uint8Array>, options?: XlsxWriteOptions): Promise<void>;
    writeBuffer(options?: XlsxWriteOptions): Promise<Uint8Array>;
  };
  get views(): WorkbookView[] | undefined;
  set views(value: WorkbookView[]);
  readonly worksheets: Worksheet<Media>[];
  readonly nextId: number;
  readonly definedNames: DefinedNames;
  clearThemes(): void;
  addImage(image: { extension: 'png' | 'jpeg' | 'gif' } & (
    { buffer: ArrayBuffer | Uint8Array; base64?: never; filename?: never } |
    { base64: string; buffer?: never; filename?: never } |
    (FileWorkbookImage extends Media ? { filename: string; buffer?: never; base64?: never } : never)
  )): number;
  getImage(imageId: number): Media | undefined;
  readonly media: readonly (Media | undefined)[];
  addWorksheet(name?: string, options?: { state?: WorksheetState; autoFilter?: AutoFilter; views?: readonly WorksheetView[]; pageSetup?: PageSetupOptions; headerFooter?: Partial<HeaderFooter>; properties?: Partial<WorksheetProperties> }): Worksheet<Media>;
  /** @deprecated Pass the tab color as `options.properties.tabColor`. */
  addWorksheet(name: string | undefined, tabColor: string | TabColor): Worksheet<Media>;
  removeWorksheet(id: string | number): void;
  removeWorksheetEx(worksheet: Worksheet<Media>): void;
  getWorksheet(id?: string | number): Worksheet<Media> | undefined;
  eachSheet(iteratee: (worksheet: Worksheet<Media>, id: number | undefined) => void): void;
}
