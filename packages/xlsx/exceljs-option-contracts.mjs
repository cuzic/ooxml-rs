import { corePropertyKinds } from './core-property-kinds.mjs';
import { printMarginDefaults } from './print-margin-defaults.mjs';
import { worksheetVisibilityCodes } from './worksheet-visibility.mjs';

const zipLevelRange = [-1, 9];

const csvMapField = {
  check: 'truthyFunction', message: 'Expected a CSV mapping function', code: 'INVALID_VALUE',
};
const csvCharacterError = 'CSV delimiters, quotes and escapes must be single characters';
const csvRowError = 'ExcelJS CSV input requires array records';
const gridLimitError = 'Expected boolean base64 and nonnegative integer grid limits';
const worksheetRowRange = [1, 1048576];
const worksheetColumnRange = [1, 16384];
const formulaCoordinate = choices => ({
  check: 'integerRange', choices, message: 'Expected one-based XLSX row and column numbers', code: 'INVALID_ADDRESS',
});
const outlineLevelRange = [0, 7];
const defaultRowHeightRange = [0, 409];
const defaultColWidthRange = [0, 255];
const outlineLevelMessage = 'Expected an outline level from 0 to 7';
const rowDescentMessage = 'Expected a nonnegative worksheet row descent';
const defaultRowHeightMessage = 'Expected a worksheet default dimension from 0 to 409';
const defaultColWidthMessage = 'Expected a worksheet default dimension from 0 to 255';
const worksheetPropertyField = (check, message, choices) => ({ check, choices, message, code: 'INVALID_VALUE' });
const valueField = (check, message, choices) => ({ check, message, choices, code: 'INVALID_VALUE' });
const tableField = (message, check, settings = {}) => ({
  check, ...settings, message, code: 'INVALID_VALUE',
});
const tableModelField = tableField.bind(undefined, 'Invalid worksheet table model');
const tableCreationField = tableField.bind(undefined, 'Expected a table name, columns and rows');
const tableColumnField = tableField.bind(undefined, 'Invalid table column');
const tableColumnDxfKeys = ['headerRowDxfId', 'dataDxfId', 'totalsRowDxfId'];
const tableBorderDxfKeys = ['headerRowBorderDxfId', 'tableBorderDxfId', 'totalsRowBorderDxfId'];
const tableDxfId = {
  check: 'optionalU32', message: 'Invalid worksheet table differential format ID', code: 'INVALID_VALUE',
};
const unsupportedTableFormat = {
  check: 'undefined', message: 'Unsupported worksheet table differential style', code: 'UNSUPPORTED_OPTIONS',
};
const tableStyleThemeCheck = 'nullableString';
const tableStyleFlagCheck = 'boolean';
const tableStyleFlagDefaults = {
  showFirstColumn: false,
  showLastColumn: false,
  showRowStripes: false,
  showColumnStripes: false,
};
const tableStyleFlags = Object.keys(tableStyleFlagDefaults);
const tableStyleField = tableField.bind(undefined, 'Invalid table style');
const tableTotalFunctions = ['none', 'sum', 'average', 'count', 'countNums', 'max', 'min', 'stdDev', 'var', 'custom'];

const ignorableWorksheetNodes = [
  'autoFilter', 'cols', 'conditionalFormatting', 'dataValidations', 'dimension', 'drawing',
  'extLst', 'headerFooter', 'hyperlinks', 'mergeCells', 'pageMargins', 'pageSetup', 'picture',
  'printOptions', 'rowBreaks', 'sheetFormatPr', 'sheetPr', 'sheetProtection', 'sheetViews', 'tableParts',
];

const protectionPermissionDefaults = {
  objects: true, scenarios: true, selectLockedCells: true, selectUnlockedCells: true,
  formatCells: false, formatColumns: false, formatRows: false, insertColumns: false,
  insertRows: false, insertHyperlinks: false, deleteColumns: false, deleteRows: false,
  sort: false, autoFilter: false, pivotTables: false,
};
const protectionPermissions = Object.keys(protectionPermissionDefaults);

const headerFooterText = ['oddHeader', 'oddFooter', 'evenHeader', 'evenFooter', 'firstHeader', 'firstFooter'];
const headerFooterFlags = ['differentOddEven', 'differentFirst'];
const headerFooterField = (key, check) => ({
  check, message: `Invalid worksheet header and footer value for ${key}`, code: 'INVALID_VALUE',
});

const printOptionNames = ['showGridLines', 'showRowColHeaders', 'horizontalCentered', 'verticalCentered'];
const pageSettingNames = ['orientation', 'paperSize', 'scale', 'fitToWidth', 'fitToHeight', 'fitToPage'];
const printReferenceNames = ['printArea', 'printTitlesRow', 'printTitlesColumn'];
const printerSettingDefaults = {
  pageOrder: 'downThenOver',
  cellComments: 'None',
  errors: 'displayed',
  blackAndWhite: false,
  draft: false,
  useFirstPageNumber: false,
  usePrinterDefaults: false,
  horizontalDpi: 0xffffffff,
  verticalDpi: 0xffffffff,
  firstPageNumber: undefined,
  copies: 1,
};
const printerSettingNames = Object.keys(printerSettingDefaults);
const printerCommentValues = {
  None: 'none', none: 'none', asDisplayed: 'asDisplayed', asDisplyed: 'asDisplayed', atEnd: 'atEnd',
};

const pageSettingField = (key, check, choices) => ({
  check, choices, message: `Invalid worksheet page setup value for ${key}`, code: 'INVALID_VALUE',
});
const printerSettingField = (key, check, choices) => ({
  check, choices, message: `Invalid worksheet printer setting for ${key}`, code: 'INVALID_VALUE',
});
const printOptionField = {
  check: 'boolean', message: 'Expected a boolean worksheet print option', code: 'INVALID_VALUE',
};

const worksheetViewField = (check, settings = {}) => ({
  check, ...settings, message: 'Invalid worksheet view', code: 'INVALID_VALUE',
});

const workbookViewField = (check, settings = {}) => ({
  check, ...settings, message: 'Invalid workbook view settings', code: 'INVALID_VALUE',
});

const cellStyleProperties = ['numFmt', 'font', 'alignment', 'protection', 'fill', 'border'];
const alignmentField = (check, choices) => ({
  check, choices, message: 'Invalid cell alignment setting', code: 'INVALID_VALUE',
});

const fontField = (check, settings = {}) => ({
  check, ...settings, message: 'Invalid cell font setting', code: 'INVALID_VALUE',
});

const fillPatterns = [
  'none', 'solid', 'darkVertical', 'darkHorizontal', 'darkGrid', 'darkTrellis', 'darkDown', 'darkUp',
  'lightVertical', 'lightHorizontal', 'lightGrid', 'lightTrellis', 'lightDown', 'lightUp',
  'darkGray', 'mediumGray', 'lightGray', 'gray125', 'gray0625',
];

const fillPatternError = 'Expected a supported cell fill pattern';

const borderStyles = [
  'thin', 'dotted', 'hair', 'medium', 'double', 'thick', 'dashed', 'dashDot',
  'dashDotDot', 'slantDashDot', 'mediumDashed', 'mediumDashDotDot', 'mediumDashDot',
];

const validationTypes = ['any', 'list', 'custom', 'whole', 'decimal', 'date', 'time', 'textLength'];
const twoBoundComparisons = ['between', 'notBetween'];
const comparisonOperators = [
  ...twoBoundComparisons, 'equal', 'notEqual', 'lessThan', 'lessThanOrEqual', 'greaterThan', 'greaterThanOrEqual',
];
const gradientFraction = {
  check: 'numberRange', choices: [0, 1], message: 'Invalid gradient geometry or stop positions', code: 'INVALID_VALUE',
};
const borderStyleField = {
  check: 'optionalOneOf', choices: borderStyles, message: 'Invalid cell border style', code: 'INVALID_VALUE',
};
const validationField = (check, settings = {}) => ({
  check, ...settings, message: 'Invalid cell data validation', code: 'INVALID_VALUE',
});
const validationFlags = ['allowBlank', 'showInputMessage', 'showErrorMessage'];
const validationMessages = ['promptTitle', 'prompt', 'errorTitle', 'error'];

const conditionalField = (message, check, settings = {}) => ({
  check, ...settings, message, code: 'INVALID_VALUE',
});
const conditionalRangeField = conditionalField.bind(undefined, 'Expected a conditional formatting range and at least one rule');
const conditionalRuleField = conditionalField.bind(undefined, 'Invalid conditional formatting priority or stopIfTrue');
const conditionalRankField = conditionalField.bind(undefined, 'Invalid conditional formatting rank');
const extendedDataBarField = conditionalField.bind(undefined, 'Invalid extended data bar setting');
const dataBarField = conditionalField.bind(undefined, 'Invalid conditional formatting data bar');
const iconSetField = conditionalField.bind(undefined, 'Invalid conditional formatting icon set');
const extendedDataBarFields = ['border', 'gradient', 'negativeBarColorSameAsPositive',
  'negativeBarBorderColorSameAsPositive', 'axisPosition', 'direction', 'fillColor', 'borderColor',
  'negativeFillColor', 'negativeBorderColor', 'axisColor', 'extendedCfvo'];
const dataBarProperties = ['cfvo', 'color', 'minLength', 'maxLength', 'showValue'];
const iconSetProperties = ['cfvo', 'iconSet', 'reverse', 'showValue', 'custom'];

const imageExtensions = ['png', 'jpeg', 'gif'];
const imageFilenameError = 'Expected a PNG, JPEG or GIF filename as the only image source';
const workbookImageError = 'Expected PNG, JPEG or GIF image bytes or base64';
const cellRichTextError = 'Expected a nonempty rich text array';
const cellRichTextRunError = 'Expected nonempty text in every rich text run';
const cellHyperlinkError = 'Expected hyperlink text, target and optional tooltip';
const imageAnchorError = 'Expected an image anchor and exactly one endpoint or extent';

const ErrorValue = {
  NotApplicable: '#N/A', Ref: '#REF!', Name: '#NAME?', DivZero: '#DIV/0!',
  Null: '#NULL!', Value: '#VALUE!', Num: '#NUM!',
};

export const streamingReaderModes = {
  worksheets: {
    emit: 'Expose worksheets to async iteration, parse(), and read() events.',
    ignore: 'Skip worksheet output.',
  },
  sharedStrings: {
    cache: 'Resolve shared strings in cells and expose the table on the reader.',
    emit: 'Expose indexed text or rich-text entries through parse() and shared-strings events. Cells retain shared-string indexes.',
    ignore: 'Skip the shared string table. Cells retain shared-string indexes.',
  },
  hyperlinks: {
    cache: 'Return hyperlink values in worksheet cells.',
    emit: 'Expose hyperlink relationships and emit worksheet hyperlink events.',
    ignore: 'Skip hyperlink values and events.',
  },
  styles: {
    cache: 'Apply cell, row, and column styles, including date number formats.',
    ignore: 'Skip the workbook style table.',
  },
  entries: {
    emit: 'Notify listeners about recognized ZIP parts through the entry event. The workbook notification precedes publication of model and properties.',
    ignore: 'Skip entry events.',
  },
};

const streamingReaderOptions = Object.fromEntries(Object.entries(streamingReaderModes)
  .map(([key, modes]) => [key, Object.keys(modes)]));

const worksheetStates = Object.keys(worksheetVisibilityCodes);

const cellModelFields = ['address', 'type', 'value', 'master', 'text', 'hyperlink', 'tooltip', 'shareType',
  'ref', 'formula', 'sharedFormula', 'result', 'style', 'comment'];

export const contracts = {
  streamingReaderOptions: { constant: streamingReaderOptions },
  streamingWorkbookReaderSettings: {
    applyFieldDefaults: true,
    ownDataProperties: true,
    ownKeysError: 'Unsupported streaming workbook reader option',
    fields: {
      worksheets: { type: 'enum', presence: 'optional', choices: streamingReaderOptions.worksheets, default: 'emit', message: 'Invalid worksheets mode', code: 'INVALID_VALUE' },
      sharedStrings: { type: 'enum', presence: 'optional', choices: streamingReaderOptions.sharedStrings, default: 'cache', message: 'Invalid shared strings mode', code: 'INVALID_VALUE' },
      hyperlinks: { type: 'enum', presence: 'optional', choices: streamingReaderOptions.hyperlinks, default: 'ignore', message: 'Invalid hyperlinks mode', code: 'INVALID_VALUE' },
      styles: { type: 'enum', presence: 'optional', choices: streamingReaderOptions.styles, default: 'ignore', message: 'Invalid styles mode', code: 'INVALID_VALUE' },
      entries: { type: 'enum', presence: 'optional', choices: streamingReaderOptions.entries, default: 'ignore', message: 'Invalid entries mode', code: 'INVALID_VALUE' },
    },
  },
  streamingWorksheetOptions: {
    descriptorFields: {
      optional: ['useSharedStrings', 'tabColor'],
      message: 'Expected worksheet useSharedStrings and tabColor data properties',
    },
  },
  checkedStreamingSharedStrings: {
    scalarArgument: { check: 'optionalBoolean', message: 'Expected a boolean useSharedStrings option' },
  },
  checkedStreamingRowHeight: {
    scalarArgument: { check: 'optionalNumberRange', message: 'Expected a row height from 0 to 409', choices: defaultRowHeightRange },
  },
  streamingWorkbookWriterSettings: {
    ownDataProperties: true,
    applyFieldDefaults: true,
    ownKeysError: 'Unsupported streaming workbook writer option',
    fields: {
      filename: valueField('optionalString', 'Expected an XLSX filename'),
      stream: {},
      creator: { ...valueField('optionalString', 'Expected a streaming workbook creator'), defaultIfFalsy: 'ExcelJS' },
      lastModifiedBy: { ...valueField('optionalString', 'Expected a streaming workbook last editor'), defaultIfFalsy: 'ExcelJS' },
      created: {}, modified: {}, lastPrinted: {},
      useStyles: { ...valueField('optionalBoolean', 'Expected a boolean useStyles option'), default: false },
      useSharedStrings: { ...valueField('optionalBoolean', 'Expected a boolean useSharedStrings option'), default: false },
      zip: {},
    },
  },
  streamingZipSettings: {
    applyFieldDefaults: true,
    ownDataProperties: true,
    ownKeysError: 'Unsupported streaming workbook ZIP option',
    fields: {
      store: { type: 'boolean', presence: 'optional', default: false, message: 'Expected a boolean ZIP store option', code: 'INVALID_VALUE' },
      forceLocalTime: { type: 'boolean', presence: 'optional', message: 'Expected a boolean forceLocalTime option', code: 'INVALID_VALUE' },
      forceZip64: { type: 'boolean', presence: 'optional', message: 'Expected a boolean forceZip64 option', code: 'INVALID_VALUE' },
      comment: { type: 'unicodeString', presence: 'optional', message: 'Expected a ZIP comment string', code: 'INVALID_VALUE' },
      level: { type: 'integer', presence: 'optional', range: zipLevelRange, message: 'Expected a ZIP level from -1 to 9', code: 'INVALID_VALUE' },
      zlib: {},
    },
  },
  checkedStreamingZipCommentLength: {
    scalarArgument: {
      check: 'optionalUtf8ByteLimit',
      choices: 65535,
      message: 'ZIP comments cannot exceed 65535 UTF-8 bytes',
    },
  },
  streamingZlibSettings: {
    ownDataProperties: true,
    ownKeysError: 'Unsupported streaming workbook zlib option',
    fields: {
      level: { type: 'integer', presence: 'optional', range: zipLevelRange, message: 'Expected a ZIP level from -1 to 9', code: 'INVALID_VALUE' },
    },
  },
  corePropertyKinds: { constant: corePropertyKinds },
  isCorePropertyText: { testArgument: { check: 'unicodeString' } },
  isCorePropertyDate: { testArgument: { check: 'validDate' } },
  checkedWorkbookRevision: {
    scalarArgument: {
      check: 'integerRange',
      message: 'Expected a nonnegative revision integer',
      choices: [0, Number.MAX_SAFE_INTEGER],
    },
  },
  workbookModelSettings: {
    ownDataProperties: true,
    ownKeysError: 'Unsupported workbook model field',
    fields: {
      ...Object.fromEntries(Object.keys(corePropertyKinds).map(key => [key, {}])),
      properties: {}, calcProperties: {},
      worksheets: valueField('array', 'Expected workbook model worksheets'),
      sheets: valueField('optionalArray', 'Expected workbook model sheets'),
      definedNames: valueField('optionalArray', 'Expected workbook model definedNames'),
      views: valueField('optionalArray', 'Expected workbook model views'),
      themes: {},
      media: valueField('optionalArray', 'Expected workbook model media'),
    },
  },
  checkedWorksheetModelId: {
    scalarArgument: {
      check: 'optionalIntegerRange',
      message: 'Invalid workbook model worksheet ID',
      choices: [1, 0xffffffff],
      code: 'INVALID_VALUE',
    },
  },
  worksheetModelSettings: {
    ownDataProperties: true,
    ownKeysError: 'Unsupported worksheet model field',
    fields: {
      id: {}, name: {},
      rows: valueField('array', 'Expected worksheet model rows'),
      pageSetup: {}, headerFooter: {}, views: {}, autoFilter: {}, state: {}, dataValidations: {},
      merges: {}, mergeCells: {}, cols: {}, properties: {},
      conditionalFormattings: valueField('optionalArray', 'Expected worksheet model conditional formatting blocks'),
      sheetProtection: {}, rowBreaks: {}, dimensions: {},
      media: valueField('optionalArray', 'Expected worksheet model media'),
      tables: valueField('optionalArray', 'Expected worksheet model tables'),
    },
  },
  columnModelSettings: {
    ownDataProperties: true,
    ownKeysError: 'Unsupported column model field',
    fields: {
      min: {}, max: {},
      width: valueField('optionalNumberRange', 'Expected column width between 0 and 255', [0, 255]),
      style: {},
      isCustomWidth: valueField('optionalBoolean', 'Expected a boolean custom-width flag'),
      hidden: valueField('optionalBoolean', 'Expected a boolean column visibility'),
      outlineLevel: {}, collapsed: {},
    },
  },
  rowModelSettings: {
    ownDataProperties: true,
    ownKeysError: 'Unsupported row model field',
    fields: {
      cells: valueField('array', 'Expected row model cells'),
      number: {}, min: {}, max: {}, height: {}, style: {},
      hidden: valueField('optionalBoolean', 'Expected a boolean row visibility'),
      outlineLevel: {}, collapsed: {},
    },
  },
  cellModelSettings: {
    properties: cellModelFields,
    ownDataProperties: true,
    ownKeysError: 'Unsupported cell model field',
  },
  streamingReaderCellModelSettings: {
    properties: [...cellModelFields, 'rawValue'],
    ownDataProperties: true,
    ownKeysError: 'Unsupported streaming reader cell model field',
  },
  cellNoteSettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Unsupported worksheet note field',
    exportFieldDefaults: 'cellNoteDefaults',
    fields: {
      texts: { check: 'nonemptyArray', message: 'Expected worksheet note text runs', code: 'INVALID_VALUE' },
      margins: {},
      protection: {},
      editAs: {
        check: 'optionalOneOf', choices: ['absolute', 'oneCells', 'twoCells'], default: 'absolute',
        message: 'Unsupported worksheet note positioning',
      },
    },
  },
  cellNoteMarginsSettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Unsupported worksheet note margin field',
    exportFieldDefaults: 'cellNoteMarginDefaults',
    fields: {
      insetmode: {
        check: 'optionalOneOf', choices: ['auto', 'custom'], default: 'auto',
        message: 'Unsupported worksheet note inset mode',
      },
      inset: {
        check: 'optionalNonnegativeTuple', length: 4, default: [0.13, 0.13, 0.25, 0.25],
        message: 'Expected four finite nonnegative note margins', code: 'INVALID_VALUE',
      },
    },
  },
  cellNoteProtectionSettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Unsupported worksheet note protection field',
    exportFieldDefaults: 'cellNoteProtectionDefaults',
    fields: {
      locked: {
        check: 'optionalOneOf', choices: ['True', 'False'], default: 'True',
        message: 'Unsupported worksheet note lock setting',
      },
      lockText: {
        check: 'optionalOneOf', choices: ['True', 'False'], default: 'True',
        message: 'Unsupported worksheet note text lock setting',
      },
    },
  },
  checkedCellNote: {
    scalarArgument: {
      check: 'optionalNullableUnicodeString',
      message: 'Expected a plain cell note string',
      code: 'UNSUPPORTED_OPTIONS',
    },
  },
  autofilterEndpointSettings: { properties: ['row', 'column'] },
  isWorksheetGridRow: { testArgument: { check: 'integerRange', choices: worksheetRowRange } },
  isSpliceCount: { testArgument: { check: 'nonnegativeInteger' } },
  isWorksheetGridColumn: { testArgument: { check: 'integerRange', choices: worksheetColumnRange } },
  checkedPrintReference: {
    scalarArgument: { check: 'string', message: 'Expected a print reference string' },
  },
  isWellFormedWorksheetName: { testArgument: { check: 'wellFormedUnicode' } },
  hasAllowedWorksheetNamePunctuation: { testArgument: { check: 'noWorksheetNamePunctuation' } },
  hasAllowedWorksheetNameEdges: { testArgument: { check: 'noEdgeApostrophe' } },
  isExcelErrorCode: {
    testArgument: { check: 'oneOf', choices: [...Object.values(ErrorValue), '#GETTING_DATA'] },
    exportConstants: { ErrorValue },
  },
  readCellFormulaResult: {
    variantOwnKeys: {
      common: ['formula', 'result'],
      variants: { ordinary: [], range: ['shareType', 'ref'] },
      readDataProperty: 'result',
      message: 'Expected a formula and an optional cached result',
    },
  },
  readSharedFormulaResult: {
    variantOwnKeys: {
      common: ['sharedFormula', 'result'],
      variants: { member: [] },
      readDataProperty: 'result',
      message: 'Expected a shared formula master address and an optional result',
    },
  },
  cellHyperlinkSettings: {
    ownKeysError: cellHyperlinkError,
    ownKeysCode: 'INVALID_VALUE',
    fields: {
      hyperlink: { type: 'nonemptyUnicodeString', presence: 'required', message: cellHyperlinkError, code: 'INVALID_VALUE' },
      text: { type: 'unicodeString', presence: 'required', message: cellHyperlinkError, code: 'INVALID_VALUE' },
      tooltip: { type: 'unicodeString', presence: 'optional', message: cellHyperlinkError, code: 'INVALID_VALUE' },
    },
  },
  cellRichTextSettings: {
    ownKeysError: cellRichTextError,
    ownKeysCode: 'INVALID_VALUE',
    fields: {
      richText: valueField('nonemptyArray', cellRichTextError),
    },
  },
  cellRichTextRunSettings: {
    ownKeysError: cellRichTextRunError,
    ownKeysCode: 'INVALID_VALUE',
    fields: {
      text: valueField('nonemptyUnicodeString', cellRichTextRunError),
      font: {},
    },
  },
  workbookImageSettings: {
    exactlyOne: { keys: ['buffer', 'base64'], presence: 'defined', message: workbookImageError },
    fields: {
      extension: valueField('oneOf', workbookImageError, imageExtensions),
      buffer: {}, base64: {},
    },
  },
  workbookImageFileSettings: {
    fields: {
      extension: valueField('oneOf', imageFilenameError, imageExtensions),
      filename: valueField('nonemptyString', imageFilenameError),
      buffer: valueField('undefined', imageFilenameError),
      base64: valueField('undefined', imageFilenameError),
    },
  },
  worksheetMediaSettings: {
    ownDataProperties: true,
    ownKeysError: 'Unsupported worksheet media model field',
    rejectArrays: true,
    fields: {
      type: valueField('oneOf', 'Invalid worksheet media type', ['background', 'image']),
      imageId: valueField('integerRange', 'Invalid workbook image ID', [0, Number.MAX_SAFE_INTEGER]),
      range: {}, hyperlinks: {},
    },
  },
  imageAnchorSettings: {
    applyFieldDefaults: true,
    exactlyOne: { keys: ['br', 'ext'], presence: 'truthy', message: imageAnchorError },
    fields: {
      tl: {}, br: {}, ext: {}, hyperlinks: {},
      editAs: { ...valueField('oneOf', imageAnchorError, ['twoCell', 'oneCell', 'absolute']), default: 'oneCell' },
    },
  },
  imageHyperlinkSettings: {
    fields: {
      hyperlink: valueField('unicodeString', imageAnchorError),
      tooltip: valueField('optionalUnicodeString', imageAnchorError),
    },
  },
  imageGridCoordinates: {
    fields: {
      col: { check: 'numberBelow', choices: [0, 16384], message: 'Invalid image anchor cell', code: 'INVALID_ADDRESS' },
      row: { check: 'numberBelow', choices: [0, 1048576], message: 'Invalid image anchor cell', code: 'INVALID_ADDRESS' },
    },
  },
  checkedImageAnchorOffset: {
    scalarArgument: { check: 'integerRange', message: 'Invalid image anchor offset', choices: [0, Number.MAX_SAFE_INTEGER] },
  },
  definedNameLocationSettings: {
    fields: {
      sheetName: { check: 'optionalString', message: 'Expected a named cell location', code: 'INVALID_ADDRESS' },
      top: {}, left: {}, bottom: {}, right: {}, row: {}, col: {},
    },
  },
  definedNameSpliceSettings: {
    fields: Object.fromEntries([['start', 1], ['count', 0], ['insert', 0]].map(([key, minimum]) => [
      key, { type: 'integer', presence: 'required', range: [minimum, Number.MAX_SAFE_INTEGER],
        message: 'Expected valid defined-name splice coordinates', code: 'INVALID_ADDRESS' },
    ])),
  },
  checkedDefinedNamesModel: {
    scalarArgument: { check: 'array', message: 'Expected an array of defined names' },
  },
  definedNameModelEntry: {
    fields: {
      name: {},
      ranges: valueField('array', 'Expected a defined name with an array of cell ranges'),
    },
  },
  checkedDefinedName: {
    scalarArgument: { check: 'definedName', message: 'Expected a valid defined name' },
  },
  checkedDefinedNameAddress: {
    scalarArgument: { check: 'optionalNullableString', message: 'Expected a named cell location', code: 'INVALID_ADDRESS' },
  },
  workbookPropertySettings: {
    ownKeysError: 'Unsupported workbook property',
    ownDataProperties: true,
    fields: {
      date1904: valueField('optionalBoolean', 'Expected a boolean date1904'),
    },
  },
  workbookCalculationSettings: {
    ownKeysError: 'Unsupported workbook calculation option',
    ownDataProperties: true,
    fields: {
      fullCalcOnLoad: valueField('optionalBoolean', 'Expected a boolean fullCalcOnLoad'),
    },
  },
  workbookViewSettings: {
    applyFieldDefaults: true,
    exportProperties: 'workbookViewFields',
    truthyDefaults: { width: 12000, height: 24000 },
    ownKeysError: 'Expected an ExcelJS workbook view',
    ownKeysCode: 'INVALID_VALUE',
    rejectArrays: true,
    fields: {
      x: workbookViewField('integerRange', { choices: [-0x80000000, 0x7fffffff], default: 0 }),
      y: workbookViewField('integerRange', { choices: [-0x80000000, 0x7fffffff], default: 0 }),
      width: workbookViewField('optionalU32'),
      height: workbookViewField('optionalU32'),
      firstSheet: workbookViewField('nullableU32'),
      activeTab: workbookViewField('nullableU32'),
      visibility: workbookViewField('optionalOneOf', { choices: ['visible', 'hidden', 'veryHidden'], default: 'visible' }),
    },
  },
  checkedCellNumberFormat: { scalarArgument: { check: 'unicodeString', message: 'Expected a number format string' } },
  checkedRowNumberFormat: { scalarArgument: { check: 'nonemptyUnicodeString', message: 'Expected a row number format' } },
  checkedColumnNumberFormat: { scalarArgument: { check: 'nonemptyUnicodeString', message: 'Expected a column number format' } },
  defaultCellStyleSettings: {
    ownKeysError: 'Unsupported style property',
    rejectArrays: true,
    fields: {
      ...Object.fromEntries(cellStyleProperties.map(key => [key, {}])),
      numFmt: valueField('optionalNonemptyUnicodeString', 'Expected a number format'),
    },
  },
  validateCellStyle: {
    properties: cellStyleProperties,
    ownKeysError: 'Unsupported cell style property',
    shapeOnly: true,
  },
  patternFillSettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: fillPatternError,
    fields: {
      type: {},
      pattern: { check: 'oneOf', choices: fillPatterns, message: fillPatternError },
      fgColor: {},
      bgColor: {},
    },
  },
  // Differential (dxf) pattern fills may omit the pattern, as in 4.4.0, which writes `<patternFill>` without
  // patternType. An omitted dxf patternType means solid, and the Rust writer omits it for 'solid', so the
  // default 'solid' writes the same `<patternFill>` as 4.4.0.
  differentialPatternFillSettings: {
    applyFieldDefaults: true,
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: fillPatternError,
    fields: {
      type: {},
      pattern: { check: 'optionalOneOf', choices: fillPatterns, default: 'solid', message: fillPatternError },
      fgColor: {},
      bgColor: {},
    },
  },
  gradientFillSettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Invalid gradient geometry or stop positions',
    ownKeysCode: 'INVALID_VALUE',
    fields: {
      type: {},
      gradient: {
        check: 'oneOf', choices: ['angle', 'path'],
        message: 'Invalid gradient geometry or stop positions', code: 'INVALID_VALUE',
      },
      degree: {},
      center: {},
      stops: { check: 'array', message: 'Invalid gradient geometry or stop positions', code: 'INVALID_VALUE' },
    },
  },
  isGradientAngle: { testArgument: { check: 'integerRange', choices: [0, 359] } },
  gradientCenterSettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Invalid gradient geometry or stop positions',
    ownKeysCode: 'INVALID_VALUE',
    defaultFrom: { right: 'left', bottom: 'top' },
    fields: { left: gradientFraction, top: gradientFraction, right: gradientFraction, bottom: gradientFraction },
  },
  gradientStopSettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Invalid gradient stop position',
    ownKeysCode: 'INVALID_VALUE',
    fields: {
      position: { ...gradientFraction, message: 'Invalid gradient stop position' },
      color: {},
    },
  },
  cellProtectionSettings: {
    applyFieldDefaults: true,
    ownKeysError: 'Expected cell protection settings',
    ownKeysCode: 'INVALID_VALUE',
    rejectArrays: true,
    fields: {
      locked: {
        check: 'boolean', default: true,
        message: 'Expected boolean cell protection settings', code: 'INVALID_VALUE',
      },
      hidden: {
        check: 'boolean', default: false,
        message: 'Expected boolean cell protection settings', code: 'INVALID_VALUE',
      },
    },
  },
  excelColorSettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Expected an ExcelJS tab color',
    ownKeysCode: 'INVALID_VALUE',
    fields: {
      argb: { check: 'optionalArgbHex', message: 'Expected eight hexadecimal ARGB digits', code: 'INVALID_VALUE' },
      theme: {
        check: 'optionalIntegerRange', choices: [0, 0xffffffff],
        when: { key: 'argb', defined: false }, message: 'Invalid tab color theme', code: 'INVALID_VALUE',
      },
      indexed: {
        check: 'optionalIntegerRange', choices: [0, 0xffffffff],
        when: [{ key: 'argb', defined: false }, { key: 'theme', defined: false }],
        message: 'Invalid tab color indexed', code: 'INVALID_VALUE',
      },
      tint: { check: 'optionalNumberRange', choices: [-1, 1], when: [{ key: 'argb', defined: false }, { key: 'theme', defined: true }],
        message: 'Expected a tint from -1 to 1', code: 'INVALID_VALUE' },
    },
  },
  cellBorderSettings: {
    exportProperties: 'borderSides',
    ownKeysError: 'Expected cell border settings',
    ownKeysCode: 'INVALID_VALUE',
    ownDataProperties: true,
    rejectAccessors: true,
    fields: { top: {}, left: {}, bottom: {}, right: {}, diagonal: {} },
  },
  cellBorderSideSettings: {
    ownKeysError: 'Invalid cell border side',
    ownKeysCode: 'INVALID_VALUE',
    ownDataProperties: true,
    rejectAccessors: true,
    fields: { style: borderStyleField, color: {} },
  },
  cellBorderDiagonalSettings: {
    applyFieldDefaults: true,
    ownKeysError: 'Invalid cell diagonal border',
    ownKeysCode: 'INVALID_VALUE',
    ownDataProperties: true,
    rejectAccessors: true,
    fields: {
      style: borderStyleField,
      color: {},
      up: { check: 'boolean', default: false, message: 'Invalid upward diagonal flag', code: 'INVALID_VALUE' },
      down: { check: 'boolean', default: false, message: 'Invalid downward diagonal flag', code: 'INVALID_VALUE' },
    },
  },
  cellFontSettings: {
    exportFieldDefaults: 'cellFontDefaults',
    ownKeysError: 'Expected cell font settings',
    ownKeysCode: 'INVALID_VALUE',
    rejectArrays: true,
    fields: {
      name: fontField('optionalNonemptyUnicodeString', { default: 'Calibri' }),
      size: fontField('optionalNumberRange', { choices: [1, 409], default: 11 }),
      bold: fontField('optionalBoolean'),
      italic: fontField('optionalBoolean'),
      strike: fontField('optionalBoolean'),
      underline: fontField('optionalOneOf', {
        choices: [true, false, 'none', 'single', 'double', 'singleAccounting', 'doubleAccounting'],
      }),
      vertAlign: fontField('optionalOneOf', { choices: ['baseline', 'superscript', 'subscript'] }),
      scheme: fontField('optionalOneOf', { choices: ['none', 'minor', 'major'] }),
      family: fontField('optionalIntegerRange', { choices: [0, 255] }),
      charset: fontField('optionalIntegerRange', { choices: [0, 255] }),
      color: {},
    },
  },
  cellAlignmentSettings: {
    ownKeysError: 'Expected cell alignment settings',
    ownKeysCode: 'INVALID_VALUE',
    rejectArrays: true,
    fields: {
      horizontal: alignmentField('optionalOneOf', [
        'left', 'center', 'right', 'fill', 'centerContinuous', 'distributed', 'justify',
      ]),
      vertical: alignmentField('optionalOneOf', ['top', 'middle', 'bottom', 'distributed', 'justify']),
      textRotation: alignmentField('optionalExcelRotation'),
      wrapText: alignmentField('optionalBoolean'),
      indent: alignmentField('optionalIntegerRange', [0, 255]),
      shrinkToFit: alignmentField('optionalBoolean'),
      readingOrder: alignmentField('optionalOneOf', ['ltr', 'rtl']),
    },
  },
  checkedValidationModel: {
    scalarArgument: { check: 'record', message: 'Expected a data validation model' },
  },
  dataValidationSettings: {
    applyFieldDefaults: true,
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Invalid cell data validation',
    ownKeysCode: 'INVALID_VALUE',
    fields: {
      type: validationField('oneOf', { choices: validationTypes }),
      operator: validationField('oneOf', { choices: comparisonOperators, default: 'between' }),
      formulae: {},
      errorStyle: validationField('oneOf', { choices: ['stop', 'warning', 'information'], default: 'stop' }),
      ...Object.fromEntries(validationFlags.map(key => [key, validationField('boolean', { default: false })])),
      ...Object.fromEntries(validationMessages.map(key => [key, validationField('unicodeString', { default: '' })])),
    },
  },
  isValidationFlagName: { testArgument: { check: 'oneOf', choices: validationFlags } },
  isComparisonOperator: { testArgument: { check: 'oneOf', choices: comparisonOperators } },
  isTwoBoundComparison: { testArgument: { check: 'oneOf', choices: twoBoundComparisons } },
  isConditionalEndpoint: { testArgument: { check: 'oneOf', choices: ['min', 'max'] } },
  isConditionalNumericThreshold: { testArgument: { check: 'oneOf', choices: ['num', 'percent', 'percentile'] } },
  isConditionalFormula: { testArgument: { check: 'nonblankUnicodeString' } },
  conditionalFormattingSettings: {
    ownKeysError: 'Expected a conditional formatting range and at least one rule',
    ownKeysCode: 'INVALID_VALUE',
    fields: {
      ref: conditionalRangeField('string'),
      rules: conditionalRangeField('nonemptyArray'),
    },
  },
  checkedConditionalModelDxf: {
    scalarArgument: { check: 'optionalU32', message: 'Invalid conditional formatting model style ID', code: 'INVALID_VALUE' },
  },
  conditionalRuleSettings: {
    applyFieldDefaults: true,
    fields: {
      priority: conditionalRuleField('optionalIntegerRange', { choices: [1, 0x7fffffff] }),
      stopIfTrue: conditionalRuleField('boolean', { default: false }),
    },
  },
  conditionalRankSettings: {
    applyFieldDefaults: true,
    fields: {
      rank: conditionalRankField('integerRange', { choices: [1, 0xffffffff] }),
      percent: conditionalRankField('boolean', { default: false }),
      bottom: conditionalRankField('boolean', { default: false }),
    },
  },
  conditionalAverageSettings: {
    applyFieldDefaults: true,
    fields: {
      aboveAverage: conditionalField('Expected a boolean aboveAverage', 'boolean', { default: true }),
      equalAverage: conditionalField('Expected a boolean equalAverage', 'optionalBoolean'),
      stdDev: conditionalField('Expected a nonnegative standard deviation count', 'optionalIntegerRange', {
        choices: [0, 0x7fffffff],
      }),
    },
  },
  validateConditionalRule: {
    variantOwnKeys: {
      common: ['type', 'priority', 'stopIfTrue', 'style'],
      variants: {
        expression: ['formulae'],
        cellIs: ['operator', 'formulae'],
        containsText: ['operator', 'text'],
        colorScale: ['cfvo', 'color'],
        dataBar: dataBarProperties,
        iconSet: iconSetProperties,
        timePeriod: ['timePeriod', 'formulae'],
        top10: ['rank', 'percent', 'bottom'],
        aboveAverage: ['aboveAverage', 'equalAverage', 'stdDev'],
        duplicateValues: [],
        uniqueValues: [],
      },
      message: 'Unsupported conditional formatting rule',
    },
  },
  extendedDataBarFields: { constant: extendedDataBarFields },
  extendedDataBarSettings: {
    applyFieldDefaults: true,
    fields: {
      border: extendedDataBarField('boolean', { default: false }),
      gradient: extendedDataBarField('boolean', { default: true }),
      negativeBarColorSameAsPositive: extendedDataBarField('boolean', { default: true }),
      negativeBarBorderColorSameAsPositive: extendedDataBarField('boolean', { default: true }),
      axisPosition: conditionalField('Invalid extended data bar axis', 'oneOf', {
        choices: ['auto', 'middle', 'none'], default: 'auto',
      }),
      direction: conditionalField('Invalid extended data bar direction', 'oneOf', {
        choices: ['leftToRight', 'rightToLeft', 'context'], default: 'leftToRight',
      }),
      // 4.4.0's x14 writer always writes both lengths, 0 and 100 when omitted.
      minLength: dataBarField('integerRange', { choices: [0, 100], default: 0 }),
      maxLength: dataBarField('integerRange', { choices: [0, 100], default: 100 }),
    },
  },
  conditionalDataBarSettings: {
    applyFieldDefaults: true,
    fields: {
      cfvo: dataBarField('arrayLength', { length: 2 }),
      color: {},
      minLength: dataBarField('integerRange', { choices: [0, 100], default: 10 }),
      maxLength: dataBarField('integerRange', { choices: [0, 100], default: 90 }),
      showValue: dataBarField('boolean', { default: true }),
    },
  },
  conditionalCustomIconSettings: {
    ownDataProperties: true,
    ownKeysError: 'Unsupported conditional formatting icon selection field',
    rejectArrays: true,
    fields: {
      iconSet: valueField('nonemptyUnicodeString', 'Expected a conditional formatting icon set name'),
      iconId: valueField('optionalIntegerRange', 'Expected a conditional formatting icon index', [0, 0xffffffff]),
    },
  },
  checkedConditionalThresholdGte: {
    scalarArgument: { check: 'optionalBoolean', message: 'Expected a boolean icon threshold gte', code: 'INVALID_VALUE' },
  },
  conditionalIconSetSettings: {
    applyFieldDefaults: true,
    fields: {
      cfvo: iconSetField('array'),
      iconSet: iconSetField('string', { default: '3TrafficLights1' }),
      reverse: iconSetField('boolean', { default: false }),
      showValue: iconSetField('boolean', { default: true }),
      custom: iconSetField('oneOf', { choices: [false], default: false }),
    },
  },
  isFormulaOperand: { testArgument: { check: 'formulaOperand' } },
  formulaRectangleSettings: {
    fields: {
      top: formulaCoordinate(worksheetRowRange),
      left: formulaCoordinate(worksheetColumnRange),
      bottom: formulaCoordinate(worksheetRowRange),
      right: formulaCoordinate(worksheetColumnRange),
    },
  },
  checkedFillFormulaMode: {
    scalarArgument: {
      check: 'oneOf',
      message: 'Expected shared or array formula mode',
      choices: ['shared', 'array'],
      default: 'shared',
      code: 'UNSUPPORTED_OPTIONS',
    },
  },
  checkedFillFormulaText: {
    scalarArgument: { check: 'nonblankUnicodeString', message: 'Expected a formula' },
  },
  checkedFillFormulaResults: {
    scalarArgument: { check: 'optionalArrayOrFunction', message: 'Expected formula results as an array or callback' },
  },
  checkedOutline: {
    scalarArgument: { check: 'integerRange', message: outlineLevelMessage, choices: outlineLevelRange },
  },
  checkedRowDescent: {
    scalarArgument: { check: 'nonnegativeNumber', message: rowDescentMessage },
  },
  checkedDefaultRowHeight: {
    scalarArgument: { check: 'optionalNumberRange', message: defaultRowHeightMessage, choices: defaultRowHeightRange },
  },
  checkedDefaultColWidth: {
    scalarArgument: { check: 'optionalNumberRange', message: defaultColWidthMessage, choices: defaultColWidthRange },
  },
  worksheetPropertySettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Unsupported worksheet creation property',
    fields: {
      tabColor: {},
      dyDescent: worksheetPropertyField('optionalNonnegativeNumber', rowDescentMessage),
      defaultRowHeight: worksheetPropertyField('optionalNumberRange', defaultRowHeightMessage, defaultRowHeightRange),
      defaultColWidth: worksheetPropertyField('optionalNumberRange', defaultColWidthMessage, defaultColWidthRange),
      outlineLevelRow: worksheetPropertyField('optionalIntegerRange', outlineLevelMessage, outlineLevelRange),
      outlineLevelCol: worksheetPropertyField('optionalIntegerRange', outlineLevelMessage, outlineLevelRange),
      outlineProperties: {},
    },
  },
  worksheetOutlineSettings: {
    exportProperties: 'outlineSummaryKeys',
    ownKeysError: 'Expected worksheet outline properties',
    ownKeysCode: 'INVALID_VALUE',
    ownDataProperties: true,
    rejectAccessors: true,
    fields: {
      summaryBelow: worksheetPropertyField('optionalBoolean', 'Expected a boolean value for summaryBelow'),
      summaryRight: worksheetPropertyField('optionalBoolean', 'Expected a boolean value for summaryRight'),
    },
  },
  // Values worksheet.views reports for sheetView attributes the file omits. tabSelected has no
  // default: 4.4.0 never reports a loaded tabSelected, so only a declared "1" is reported (as true).
  worksheetViewReadDefaults: {
    constant: {
      workbookViewId: 0, zoomScale: 100, showGridLines: true, rightToLeft: false,
      zoomScaleNormal: 100, showRuler: true, showRowColHeaders: true,
    },
  },
  worksheetViewSettings: {
    applyFieldDefaults: true,
    ownKeysError: 'Unsupported worksheet view option',
    rejectArrays: true,
    fields: {
      state: worksheetViewField('optionalOneOf', { choices: ['normal', 'frozen', 'split'], default: 'normal' }),
      xSplit: worksheetViewField('nonnegativeNumber', { default: 0 }),
      ySplit: worksheetViewField('nonnegativeNumber', { default: 0 }),
      zoomScale: worksheetViewField('optionalU32'),
      showGridLines: worksheetViewField('optionalBoolean'),
      rightToLeft: worksheetViewField('optionalBoolean'),
      workbookViewId: worksheetViewField('optionalU32', { default: 0 }),
      topLeftCell: {},
      activeCell: {},
      zoomScaleNormal: worksheetViewField('optionalU32'),
      showRuler: worksheetViewField('optionalBoolean'),
      showRowColHeaders: worksheetViewField('optionalBoolean'),
      tabSelected: worksheetViewField('optionalBoolean'),
      style: worksheetViewField('optionalOneOf', { choices: ['normal', 'pageBreakPreview', 'pageLayout'] }),
      activePane: worksheetViewField('optionalOneOf', {
        choices: ['topLeft', 'topRight', 'bottomLeft', 'bottomRight'],
      }),
    },
  },
  rowStyleSettings: {
    enumArgument: {
      values: {
        n: { source: 'none', includeEmpty: false },
        i: { source: 'previous', includeEmpty: false },
        'i+': { source: 'previous', includeEmpty: true },
        o: { source: 'original', includeEmpty: false },
        'o+': { source: 'original', includeEmpty: true },
      },
      append: ['n', 'i', 'i+'],
      message: 'Unsupported row style option',
    },
  },
  validateColumnDefinition: {
    properties: ['key', 'header', 'width', 'hidden', 'outlineLevel', 'style'],
    ownKeysError: 'Unsupported column definition',
    shapeOnly: true,
  },
  checkedPageMargins: {
    scalarArgument: { check: 'record', message: 'Expected worksheet margins' },
  },
  pageMarginPatch: {
    indexedPatch: {
      keys: Object.keys(printMarginDefaults),
      exportPropertyDefaults: { printMarginDefaults },
      exportKeys: 'pageMarginNames',
      predicate: 'nonnegativeNumber',
      message: 'Expected nonnegative worksheet margins',
      code: 'INVALID_VALUE',
    },
  },
  pageSetupEntries: {
    numericEntries: {
      array: 'Float64Array', keys: pageSettingNames, absent: NaN, booleanFrom: { orientation: 'landscape' },
    },
  },
  printOptionEntries: {
    numericEntries: { array: 'Uint8Array', keys: printOptionNames, absent: 2 },
  },
  printerSettingEntries: {
    stringEntries: {
      keys: printerSettingNames,
      aliases: { cellComments: printerCommentValues },
      inferredFlag: { key: 'useFirstPageNumber', from: 'firstPageNumber' },
    },
  },
  printOptionNames: { constant: printOptionNames },
  pageSettingNames: { constant: pageSettingNames },
  printReferenceNames: { constant: printReferenceNames },
  printerSettingNames: { constant: printerSettingNames },
  pageSetupSettings: {
    exportPropertyDefaults: {
      printerSettingDefaults,
      pageSettingDefaults: {
        orientation: 'portrait', paperSize: undefined, scale: 100, fitToWidth: 1, fitToHeight: 1, fitToPage: false,
      },
    },
    creationFlag: { key: 'fitToPage', anyTruthy: ['fitToWidth', 'fitToHeight'], unless: 'scale' },
    ownKeysError: 'Unsupported worksheet pageSetup option',
    orderedOwnDataProperties: true,
    fields: {
      margins: {},
      rowBreaks: pageSettingField('rowBreaks', 'nullish'),
      colBreaks: pageSettingField('colBreaks', 'nullish'),
      ...Object.fromEntries(printOptionNames.map(key => [key, printOptionField])),
      orientation: pageSettingField('orientation', 'optionalOneOf', ['portrait', 'landscape']),
      paperSize: pageSettingField('paperSize', 'optionalU32'),
      scale: pageSettingField('scale', 'integerRange', [10, 400]),
      fitToWidth: pageSettingField('fitToWidth', 'optionalU32'),
      fitToHeight: pageSettingField('fitToHeight', 'optionalU32'),
      fitToPage: pageSettingField('fitToPage', 'boolean'),
      ...Object.fromEntries(printReferenceNames.map(key => [key, {}])),
      pageOrder: printerSettingField('pageOrder', 'optionalOneOf', ['downThenOver', 'overThenDown']),
      cellComments: printerSettingField('cellComments', 'optionalOneOf', Object.keys(printerCommentValues)),
      errors: printerSettingField('errors', 'optionalOneOf', ['displayed', 'blank', 'dash', 'NA']),
      ...Object.fromEntries(['blackAndWhite', 'draft', 'useFirstPageNumber', 'usePrinterDefaults'].map(key =>
        [key, printerSettingField(key, 'boolean')])),
      ...Object.fromEntries(['horizontalDpi', 'verticalDpi', 'firstPageNumber', 'copies'].map(key =>
        [key, printerSettingField(key, 'optionalU32')])),
    },
  },
  headerFooterSettings: {
    exportProperties: 'headerFooterNames',
    maskedStrings: true,
    ownKeysError: 'Unsupported worksheet headerFooter option',
    ownDataProperties: true,
    fields: {
      ...Object.fromEntries(headerFooterText.map(key => [key, headerFooterField(key, 'optionalNullableUnicodeString')])),
      ...Object.fromEntries(headerFooterFlags.map(key => [key, headerFooterField(key, 'optionalBoolean')])),
    },
  },
  sheetProtectionModelSettings: {
    ownKeysError: 'Unsupported worksheet protection model field',
    ownDataProperties: true,
    fields: {
      ...Object.fromEntries(['sheet', ...protectionPermissions].map(key => [key, {
        check: 'optionalBoolean', message: `Expected a boolean for ${key}`, code: 'INVALID_VALUE',
      }])),
      ...Object.fromEntries(['algorithmName', 'hashValue', 'saltValue'].map(key => [key, {
        check: 'optionalUnicodeString', message: `Expected a string for ${key}`, code: 'INVALID_VALUE',
      }])),
      spinCount: {
        check: 'optionalIntegerRange', choices: [0, 0xffffffff],
        message: 'Expected a stored spin count between 0 and 4294967295', code: 'INVALID_VALUE',
      },
    },
  },
  sheetProtectionSettings: {
    ownKeysError: 'Unsupported worksheet protection option',
    ownDataProperties: true,
    applyFieldDefaults: true,
    exportFieldDefaults: 'sheetProtectionDefaults',
    fields: {
      ...Object.fromEntries(Object.entries(protectionPermissionDefaults).map(([key, defaultValue]) => [key, {
        check: 'boolean', default: defaultValue, message: `Expected a boolean for ${key}`, code: 'INVALID_VALUE',
      }])),
      spinCount: {},
    },
  },
  tableColumnDxfKeys: { constant: tableColumnDxfKeys },
  tableDifferentialStyles: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Unsupported worksheet table differential style field',
    exportProperties: 'tableDxfKeys',
    fields: Object.fromEntries([...tableColumnDxfKeys, ...tableBorderDxfKeys].map(key => [key, {}])),
  },
  tableModelSettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Unsupported worksheet table model field',
    rejectArrays: true,
    fields: {
      id: {},
      ...Object.fromEntries([...tableColumnDxfKeys, ...tableBorderDxfKeys].map(key => [key, tableDxfId])),
      differentialStyles: {},
      name: tableModelField('nonemptyUnicodeString'),
      displayName: tableModelField('optionalUnicodeString'),
      ref: {},
      tableRef: tableModelField('nonemptyUnicodeString'),
      autoFilterRef: {},
      headerRow: tableModelField('optionalBoolean'),
      totalsRow: tableModelField('optionalBoolean'),
      style: {},
      theme: {},
      columns: tableModelField('nonemptyArray'),
      rows: {},
      tl: {},
      ...Object.fromEntries(tableStyleFlags.map(key => [key, {}])),
    },
  },
  tableModelColumnSettings: {
    ownDataProperties: true,
    rejectAccessors: true,
    ownKeysError: 'Unsupported worksheet table model column field',
    rejectArrays: true,
    fields: {
      name: {},
      filterButton: {},
      totalsRowFunction: {},
      totalsRowLabel: {},
      totalsRowFormula: {},
      totalsRowResult: {},
      calculatedColumnFormula: tableField('Invalid calculated column formula', 'optionalNonemptyUnicodeString'),
      colId: {},
      dxfId: unsupportedTableFormat,
      ...Object.fromEntries(tableColumnDxfKeys.map(key => [key, tableDxfId])),
      differentialStyles: {},
    },
  },
  tableSettings: {
    applyFieldDefaults: true,
    ownKeysError: 'Unsupported worksheet table option',
    fields: {
      name: tableCreationField('nonemptyUnicodeString'),
      displayName: tableCreationField('optionalUnicodeString'),
      ref: {},
      headerRow: tableCreationField('boolean', { default: true }),
      totalsRow: tableCreationField('boolean', { default: false }),
      style: { default: {} },
      columns: tableCreationField('nonemptyArray'),
      rows: tableCreationField('array'),
    },
  },
  tableStyleFlags: { constant: tableStyleFlags },
  tableStyleSettings: {
    applyFieldDefaults: true,
    ownKeysError: 'Unsupported table style option',
    fields: {
      theme: tableStyleField(tableStyleThemeCheck, { default: 'TableStyleMedium2' }),
      ...Object.fromEntries(Object.entries(tableStyleFlagDefaults).map(([key, defaultValue]) =>
        [key, tableStyleField(tableStyleFlagCheck, { default: defaultValue })])),
    },
  },
  tableColumnSettings: {
    applyFieldDefaults: true,
    ownKeysError: 'Unsupported table column option',
    fields: {
      name: tableColumnField('nonemptyUnicodeString'),
      filterButton: tableColumnField('boolean', { default: false }),
      totalsRowFunction: tableColumnField('oneOf', { choices: tableTotalFunctions, default: 'none' }),
      totalsRowLabel: tableColumnField('optionalUnicodeString'),
      totalsRowFormula: tableColumnField('nonblankUnicodeString', {
        when: { key: 'totalsRowFunction', equals: 'custom' },
      }),
      totalsRowResult: {},
      style: {},
    },
  },
  checkedTableTheme: { scalarArgument: { check: tableStyleThemeCheck, message: 'Invalid table style value' } },
  checkedTableStyleFlag: { scalarArgument: { check: tableStyleFlagCheck, message: 'Invalid table style value' } },
  checkedTableColumnName: { scalarArgument: { check: 'nonemptyUnicodeString', message: 'Invalid table column property' } },
  checkedTableColumnFilterButton: { scalarArgument: { check: 'boolean', message: 'Invalid table column property' } },
  checkedTableTotalFunction: { scalarArgument: {
    check: 'optionalOneOf',
    message: 'Invalid table totals function',
    choices: tableTotalFunctions,
  } },
  checkedTableTotalText: { scalarArgument: { check: 'optionalUnicodeString', message: 'Invalid table totals text' } },
  checkedTableColumnValues: { scalarArgument: { check: 'array', message: 'Expected an array of table column values' } },
  checkedTableColumnIndex: { scalarArgument: {
    check: 'optionalNonnegativeInteger',
    message: 'Expected a nonnegative integer table column position',
  } },
  checkedTableRow: { scalarArgument: { check: 'array', message: 'Invalid table row' } },
  checkedTableRowIndex: { scalarArgument: {
    check: 'optionalNonnegativeInteger',
    message: 'Expected a nonnegative integer table row position',
  } },
  worksheetIterationSettings: {
    properties: ['includeEmpty'],
    defaults: { includeEmpty: false },
  },
  csvWorksheetSettings: {
    applyFieldDefaults: true,
    fields: {
      sheetName: {},
      sheetId: {},
      includeEmptyRows: { default: true },
      map: csvMapField,
      dateFormat: { check: 'optionalString', message: 'Expected a CSV date format string', code: 'INVALID_VALUE' },
      dateUTC: { check: 'optionalBoolean', message: 'Expected a boolean CSV dateUTC option', code: 'INVALID_VALUE' },
    },
  },
  csvReadSettings: {
    truthyDefaults: {
      dateFormats: ['YYYY-MM-DD[T]HH:mm:ssZ', 'YYYY-MM-DD[T]HH:mm:ss', 'MM-DD-YYYY', 'YYYY-MM-DD'],
    },
    fields: {
      map: csvMapField,
      dateFormats: { check: 'truthyStringArray', message: 'Expected an array of CSV date formats', code: 'INVALID_VALUE' },
    },
  },
  csvReadDestinationSettings: { fields: { sheetName: {} } },
  csvReadStreamSettings: { fields: { parserOptions: {} } },
  csvWriteStreamSettings: { fields: { formatterOptions: {} } },
  csvWriteFileSettings: {
    fields: { encoding: {} },
    nullishDefaults: { encoding: 'utf8' },
  },
  csvParserSettings: {
    applyFieldDefaults: true,
    defaultFrom: { escape: 'quote' },
    fields: {
      delimiter: { check: 'csvDelimiter', default: ',', message: csvCharacterError },
      quote: { check: 'csvOptionalQuote', default: '"', message: csvCharacterError },
      escape: { check: 'csvOptionalQuote', message: csvCharacterError },
      encoding: { default: 'utf-8' },
      trim: {},
      ltrim: {},
      rtrim: {},
      comment: {},
      headers: { check: 'truthy', message: csvRowError },
      objectMode: { check: 'falsy', default: true, message: csvRowError },
      renameHeaders: { check: 'truthy', message: csvRowError },
      ignoreEmpty: {},
      maxRows: { default: 0 },
      skipRows: { default: 0 },
      skipLines: { default: 0 },
    },
  },
  checkedCsvTransform: {
    scalarArgument: { check: 'truthyFunction', message: 'Expected a CSV row transform function' },
  },
  csvFormatterSettings: {
    applyFieldDefaults: true,
    nullishDefaults: { writeHeaders: true },
    defaultFrom: { quoteHeaders: 'quoteColumns' },
    fields: {
      delimiter: { default: ',' },
      rowDelimiter: { default: '\n' },
      quote: { default: '"' },
      escape: {},
      quoteColumns: { default: false },
      quoteHeaders: {},
      headers: {},
      writeHeaders: {},
      alwaysWriteHeaders: {},
      writeBOM: {},
      includeEndRowDelimiter: {},
      transform: {},
    },
  },
  workbookLoadSettings: {
    matchOptions: true,
    fields: {
      base64: { type: 'boolean', presence: 'optional', message: gridLimitError },
      maxRows: { check: 'optionalU32', message: gridLimitError, default: 0 },
      maxCols: { check: 'optionalU32', message: gridLimitError, default: 0 },
      ignoreNodes: {
        check: 'optionalListOf',
        choices: ignorableWorksheetNodes,
        message: 'Unsupported worksheet node in ignoreNodes',
        default: [],
      },
    },
  },
  workbookOutputSettings: {
    matchOptions: true,
    deferredCheck: 'validateWorkbookOutputSettings',
    fields: {
      zip: {},
      useSharedStrings: { type: 'boolean', presence: 'optional', message: 'Expected a boolean useSharedStrings option' },
      useStyles: { type: 'boolean', presence: 'optional', message: 'Expected a boolean useStyles option' },
    },
  },
  zipOutputSettings: {
    matchOptions: true,
    projections: [{
      name: 'zipCompressionSettings',
      fields: { compressed: { from: 'compression', transform: 'zipCompression' } },
    }],
    fields: {
      compression: {
        check: 'optionalZipCompression',
        message: 'Expected ZIP compression DEFLATE or STORE',
        default: 'DEFLATE',
      },
      compressionOptions: {},
    },
  },
  zipLevelSettings: {
    matchOptions: true,
    projections: [{
      name: 'zipDeflateSettings',
      fields: { deflateLevel: { from: 'level', transform: 'deflateLevel' } },
    }],
    fields: {
      level: {
        type: 'integer',
        presence: 'optional',
        range: zipLevelRange,
        message: 'Expected a DEFLATE level between -1 and 9',
      },
    },
  },
  isWorksheetState: { testArgument: { check: 'oneOf', choices: worksheetStates } },
  checkedWorksheetCreationState: {
    scalarArgument: {
      check: 'optionalOneOf', choices: worksheetStates, message: 'Invalid worksheet state', code: 'UNSUPPORTED_OPTIONS',
    },
  },
  worksheetSettings: {
    matchOptions: true,
    fields: { state: {}, autoFilter: {}, views: {}, pageSetup: {}, headerFooter: {}, properties: {} },
  },
};
