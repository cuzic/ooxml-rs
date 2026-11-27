import { corePropertyKinds } from './core-property-kinds.mjs';
import { printMarginDefaults } from './print-margin-defaults.mjs';
import { worksheetVisibilityCodes } from './worksheet-visibility.mjs';
const sheetField = (check, message) => ({ check, message, code: 'INVALID_VALUE' });

const dateFormatCheck = { check: 'optionalString', message: 'Expected dateNF to be a number format string' };
const sheetSelectorCheck = { check: 'optionalSheetSelectors', message: 'Expected sheet names or zero-based indexes' };

const sheetSelectionReference = 'Selects loaded sheets by case-insensitive name or zero-based index, individually or in an array. Omission loads all sheets. SheetNames keeps the full list.';

const binaryWorkbook = {
  type: {},
  sheets: {},
  bookSheets: {},
  dense: {},
  sheetRows: {},
  sheetStubs: {},
  cellDates: {},
  cellNF: {},
  cellText: {},
  cellFormula: {},
  cellStyles: {},
  codepage: {},
};

const checkedBooleanOption = key => ({
  check: 'optionalBoolean', message: `Expected a boolean ${key} option`,
});

// Both formats enable these projections when cellStyles is truthy.
const projectWithCellStyles = {
  trueWhen: 'truthy',
  alsoEnabledBy: 'cellStyles',
};

// 0.20.3 reads these options only for other input formats or for encrypted
// compound files. A ZIP workbook (XLSX or XLSB) gives the same result for any value.
const zipWorkbookUnusedOptions = {
  raw: { referenceNote: 'Accepted, any value. No effect: 0.20.3 uses it for text input and compound-file headers.' },
  password: { referenceNote: 'Accepted, any value. No effect: 0.20.3 uses it only to decrypt compound files.' },
  PRN: { referenceNote: 'Accepted, any value. No effect: 0.20.3 uses it only for plain text input.' },
  FS: { referenceNote: 'Accepted, any value. No effect: 0.20.3 uses it only for delimited text input.' },
  UTC: { referenceNote: 'Accepted, any value. No effect: 0.20.3 uses it only for text, DBF and Lotus dates.' },
};

// 0.20.3 throws on some malformed parts with a truthy WTF instead of skipping them.
// This adapter reports malformed parts and ignores unknown elements regardless of WTF.
const parseErrorNote = 'With a truthy value 0.20.3 throws on some malformed or unrecognized content that it otherwise skips; this adapter does not change its error handling.';
const unusedWtfOption = { referenceNote: 'Accepted, any value. ' + parseErrorNote };

const workbookResources = {
  bookFiles: { workbookResource: { output: 'files', disabledBy: 'bookSheets' } },
  bookVBA: { workbookResource: { output: 'vba', disabledBy: 'bookSheets' } },
  bookDeps: { workbookResource: { output: 'dependencies', disabledBy: 'bookSheets' } },
};

function workbookResourceProjection(name, options) {
  return {
    name,
    fields: Object.fromEntries(Object.entries(options)
      .filter(([, option]) => option.workbookResource)
      .map(([key, { workbookResource: { output, disabledBy } }]) => [
        output, { from: [key, disabledBy], transform: 'enabledUnless' },
      ])),
  };
}

const xlsbReadOptions = {
  ...binaryWorkbook,
  sheets: { referenceNote: sheetSelectionReference },
  bookSheets: {
    referenceNote: 'A truthy value returns the sheet names and workbook metadata without loading the sheets. With bookProps, returns property records and sheet names.',
  },
  cellText: { projection: { trueWhen: 'notExplicitlyFalse' } },
  cellDates: { projection: { trueWhen: 'truthy' } },
  dense: { projection: { trueWhen: 'truthy' } },
  sheetRows: { projection: { rustField: 'rowLimit', as: 'xlsbRowLimit' } },
  sheetStubs: { projection: { ...projectWithCellStyles, rustField: 'stubs' } },
  cellNF: { projection: { ...projectWithCellStyles, rustField: 'numberFormats' } },
  cellFormula: { projection: { rustField: 'formulas', trueWhen: 'notExplicitlyFalse' } },
  cellStyles: { projection: { rustField: 'columnStyles', trueWhen: 'truthy' } },
  nodim: { referenceNote: 'Accepted. The declared worksheet dimensions are retained.' },
  cellHTML: { referenceNote: 'Accepted. XLSB rich-text runs are exposed without generating HTML.' },
  bookFiles: workbookResources.bookFiles,
  bookVBA: workbookResources.bookVBA,
  bookProps: {
    referenceNote: 'A truthy value returns Props and Custprops. SheetNames is included when bookSheets is also truthy.',
  },
  bookDeps: workbookResources.bookDeps,
  dateNF: { ...dateFormatCheck, projection: {} },
  xlfn: {
    ...checkedBooleanOption('xlfn'),
    default: false,
    referenceNote: 'Preserves the _xlfn. prefix when resolving identifiers in formulas.',
  },
  ...zipWorkbookUnusedOptions,
  WTF: unusedWtfOption,
};

// Preserve the order in which the adapter reads user options.
const xlsbProjectionOrder = ['sheetRows', 'cellStyles', 'sheetStubs', 'cellNF', 'cellFormula', 'dense', 'cellText', 'cellDates', 'dateNF'];
function worksheetProjectionFields(options, order) {
  const declared = Object.keys(options).filter(key => options[key].projection);
  if (order.length !== declared.length || declared.some(key => !order.includes(key))) {
    throw new Error('Worksheet projection order must list every projected option exactly once');
  }
  return Object.fromEntries(order.map(key => {
    const { rustField, as, trueWhen, alsoEnabledBy } = options[key].projection;
    const booleanTransforms = {
      truthy: alsoEnabledBy ? 'anyTruthy' : 'truthy',
      notExplicitlyFalse: 'unlessFalse',
    };
    if ((alsoEnabledBy && trueWhen !== 'truthy') ||
        (as && trueWhen) || (trueWhen && !Object.hasOwn(booleanTransforms, trueWhen))) {
      throw new Error(`Invalid worksheet projection for ${key}`);
    }
    return [rustField ?? key, {
      from: alsoEnabledBy ? [alsoEnabledBy, key] : key,
      transform: as ?? (trueWhen ? booleanTransforms[trueWhen] : 'identity'),
    }];
  }));
}

const compoundReadOptions = {
  ...binaryWorkbook,
  dateNF: { ...dateFormatCheck },
  bookFiles: checkedBooleanOption('bookFiles'),
  bookProps: checkedBooleanOption('bookProps'),
  cellHTML: checkedBooleanOption('cellHTML'),
  WTF: {
    referenceNote: 'Accepted, any value. A truthy value reports a malformed property stream and, with bookSheets, reads the workbook records after the sheet list.',
  },
  xlfn: checkedBooleanOption('xlfn'),
  raw: {
    referenceNote: 'Accepted, any value. With a truthy bookFiles, a truthy value adds the compound-file header and sectors as cfb.raw.',
  },
};

const earlyBiffReadOptions = {
  type: {}, dense: {}, sheetRows: {}, sheetStubs: {}, cellDates: {}, cellNF: {}, cellText: {}, codepage: {},
  dateNF: { ...dateFormatCheck },
  cellHTML: checkedBooleanOption('cellHTML'),
  raw: { referenceNote: 'Accepted, any value. No effect.' },
  WTF: unusedWtfOption,
};

const lotusReadOptions = {
  type: {}, dense: {}, cellDates: {}, dateNF: {}, codepage: {}, sheetRows: {}, cellFormula: {},
  raw: { referenceNote: 'Accepted, any value. No effect.' },
  WTF: unusedWtfOption,
};

const dbfReadOptions = {
  type: {},
  sheetRows: { check: 'optionalNonnegativeInteger', message: 'Expected a nonnegative DBF row limit' },
  dense: {}, cellDates: {}, cellText: {}, dateNF: {}, codepage: {}, UTC: {}, sheetStubs: {},
  raw: { referenceNote: 'Accepted, any value. No effect.' },
  WTF: { referenceNote: 'Accepted, any value. With a truthy value 0.20.3 throws on an unreadable file instead of returning an empty workbook; this adapter reports it regardless of WTF.' },
};

function readFields(options) {
  return Object.fromEntries(Object.entries(options).map(
    ([key, { projection, workbookResource, referenceNote, ...field }]) => [key, field],
  ));
}

const sheetSelectionProjection = name => ({
  name,
  fields: { selectSheet: { from: 'sheets', transform: 'sheetSelection' } },
});

const textRowLimitCheck = format => ({
  check: 'optionalNonnegativeInteger', message: `Expected a nonnegative ${format} row limit`,
});

const textReadBooleans = ['cellDates', 'cellNF', 'UTC', 'PRN', 'cellFormula', 'sheetStubs', 'bookProps', 'bookSheets'];
const delimitedWriteFields = ['FS', 'RS', 'skipHidden', 'blankrows', 'strip', 'forceQuotes', 'rawNumbers', 'dateNF'];
const htmlWriteFields = ['header', 'footer', 'id', 'editable'];
const earlyBiffVersions = { biff2: 2, biff3: 3, biff4: 4 };
const compoundBiffVersions = { biff5: 5, biff8: 8, xls: 8, xla: 8 };
const otherBinaryFormats = { dbf: 'application/x-dbf', wk1: 'application/octet-stream', wk3: 'application/octet-stream' };

// 0.20.3 `write` only fills defaults (fix_write_opts) and never rejects a key. Each writer reads
// some options. A format accepts the others with any value and ignores them, as listed here.
const noEffect = reason => ({ referenceNote: 'Accepted, any value. No effect: ' + reason });
const unusedWriteOptions = {
  compression: noEffect('0.20.3 compresses only ZIP output (XLSX, XLSB, ODS).'),
  bookSST: noEffect('0.20.3 writes a shared string table only for XLSX, XLSB and BIFF5/8.'),
  cellDates: noEffect('only the 0.20.3 XLSX writer reads it.'),
  Props: noEffect('0.20.3 merges options.Props only into XLSX, XLSB and SpreadsheetML properties. Workbook Props are written as without it.'),
  cellStyles: noEffect('0.20.3 turns it into cellNF and sheetStubs, which this writer does not read.'),
  sheetStubs: noEffect('0.20.3 reads it only when writing XLSX and SpreadsheetML.'),
  ignoreEC: noEffect('only the 0.20.3 XLSX and XLSB writers read it.'),
  themeXLSX: noEffect('only the 0.20.3 XLSX and XLSB writers read it.'),
  WTF: noEffect('0.20.3 reads it on write only for XLSX, XLSB and BIFF range limits and XLSX/XLSB property dates.'),
  numbers: noEffect('0.20.3 reads it only for Numbers output.'),
  codepage: noEffect('0.20.3 selects a codepage only for DBF, WK1 and WK3 output.'),
  password: noEffect('0.20.3 does not encrypt output.'),
  bookVBA: noEffect('0.20.3 writes a workbook vbaraw regardless of it.'),
  FS: noEffect('only 0.20.3 CSV output reads the field separator.'),
  RS: noEffect('only 0.20.3 CSV output reads the record separator.'),
};
const unusedWrite = (...keys) => Object.fromEntries(keys.map(key => [key, unusedWriteOptions[key]]));
const allSheetsWritten = {
  referenceNote: 'Every sheet is written. A truthy value that names no sheet or indexes past SheetNames throws "Sheet not found", as in 0.20.3.',
};
const zipWriteErrorNote = 'Accepted, any value. With a truthy value 0.20.3 throws on a worksheet range beyond A1:XFD1048576 or an invalid property date instead of clamping or omitting it; this adapter handles both the same way regardless of WTF.';
const sheetWriteOptions = ['compression', 'bookSST', 'cellDates', 'Props', 'cellStyles', 'sheetStubs',
  'ignoreEC', 'themeXLSX', 'WTF', 'numbers', 'password', 'bookVBA', 'FS', 'RS'];
const sheetWriteUnused = (...used) => unusedWrite(...sheetWriteOptions.filter(key => !used.includes(key)));

const xlmlStubNote = 'stub cells (t "z") without a value or formula are never written. 0.20.3 also drops a stub cell that has a value or formula unless the option is truthy.';

// Options accepted without effect, by write format. Keys that a format reads keep their own declarations.
export const writeOptionsByFormat = {
  'xlsx / xlsm / xlam': {
    sheet: { referenceNote: allSheetsWritten.referenceNote + ' writeXLSX and writeFileXLSX do not check it.' },
    WTF: { referenceNote: zipWriteErrorNote },
    ...unusedWrite('numbers', 'bookVBA', 'FS', 'RS'),
  },
  xlsb: {
    sheet: allSheetsWritten,
    WTF: { referenceNote: zipWriteErrorNote },
    ...unusedWrite('cellDates', 'cellStyles', 'sheetStubs', 'numbers', 'codepage', 'bookVBA', 'FS', 'RS'),
  },
  ods: {
    sheet: allSheetsWritten,
    ...unusedWrite('bookSST', 'cellDates', 'Props', 'cellStyles', 'sheetStubs', 'ignoreEC', 'themeXLSX', 'WTF',
      'numbers', 'codepage', 'bookVBA', 'FS', 'RS'),
  },
  fods: { sheet: allSheetsWritten, ...sheetWriteUnused(), codepage: unusedWriteOptions.codepage },
  'xlml / xml': {
    sheet: allSheetsWritten,
    ...sheetWriteUnused('Props'),
    cellStyles: noEffect(xlmlStubNote),
    sheetStubs: noEffect(xlmlStubNote),
    codepage: unusedWriteOptions.codepage,
  },
  'csv / txt': sheetWriteUnused('FS', 'RS'),
  'html, prn, dif, sylk / slk, eth, rtf, dbf, wk1, wk3': sheetWriteUnused(),
  'biff5 / biff8 / xls / xla': { sheet: allSheetsWritten, ...sheetWriteUnused('bookSST', 'WTF') },
  'biff2 / biff3 / biff4': sheetWriteUnused('WTF'),
};
const writeKeys = (label, ...omit) => Object.keys(writeOptionsByFormat[label]).filter(key => !omit.includes(key));
const writeFieldsFor = label => Object.fromEntries(writeKeys(label).map(key => [key, {}]));
const textFormats = [
  [['csv', 'txt'], [...delimitedWriteFields, ...writeKeys('csv / txt')]],
  [['html'], [...htmlWriteFields, ...writeKeys('html, prn, dif, sylk / slk, eth, rtf, dbf, wk1, wk3')]],
  [Object.keys(compoundBiffVersions), ['bookSST', 'WTF', ...writeKeys('biff5 / biff8 / xls / xla', 'sheet')]],
  [Object.keys(earlyBiffVersions), ['WTF', ...writeKeys('biff2 / biff3 / biff4')]],
  [['prn', 'dif', 'sylk', 'slk', 'eth', 'rtf', ...Object.keys(otherBinaryFormats)],
    writeKeys('html, prn, dif, sylk / slk, eth, rtf, dbf, wk1, wk3')],
];

const byteTypes = ['array', 'buffer', 'base64', 'binary'];
const textOutputTypes = [...byteTypes, 'string'];

export const fileOutputFormats = {
  defaultFormat: 'xlsx',
  writeAliases: { htm: 'html' },
  asyncWriteAliases: { socialcalc: 'eth' },
  extensions: { htm: 'html', slk: 'sylk', socialcalc: 'eth', xls: 'biff8' },
  earlyBiffVersions,
  compoundBiffVersions,
  binaryMimeTypes: {
    ...Object.fromEntries([...Object.keys(earlyBiffVersions), ...Object.keys(compoundBiffVersions)]
      .map(format => [format, 'application/octet-stream'])),
    ...otherBinaryFormats,
  },
  documentMimeTypes: {
    xlsb: 'application/vnd.ms-excel.sheet.binary.macroEnabled.12',
    ods: 'application/vnd.oasis.opendocument.spreadsheet',
    fods: 'application/vnd.oasis.opendocument.spreadsheet-flat-xml',
    xlml: 'application/xml;charset=utf-8',
  },
  xmlWorkbookMimeTypes: {
    xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
    xlsm: 'application/vnd.ms-excel.sheet.macroEnabled.12',
    xlam: 'application/vnd.ms-excel.addin.macroEnabled.12',
  },
  textMimeTypes: {
    csv: 'text/csv;charset=utf-8',
    txt: 'text/plain;charset=utf-16le',
    html: 'text/html;charset=utf-8',
    prn: 'text/plain;charset=utf-8',
    dif: 'text/plain;charset=utf-8',
    sylk: 'text/plain;charset=utf-8',
    slk: 'text/plain;charset=utf-8',
    eth: 'text/plain;charset=utf-8',
    rtf: 'application/rtf',
  },
};

export const writeOutputTypes = {
  xlsx: { types: byteTypes, label: 'XLSX' },
  xlsb: { types: byteTypes, label: 'XLSB' },
  ods: { types: byteTypes, label: 'ODS' },
  fods: { types: textOutputTypes, label: 'FODS' },
  xlml: { types: textOutputTypes, label: 'SpreadsheetML' },
  text: { types: textOutputTypes, label: 'delimited text' },
  binarySheet: { types: byteTypes },
};

const sheetInputFields = {
  dense: {},
  origin: { defaultIfNullish: 0 },
  dateNF: { defaultIfFalsy: 'm/d/yy' },
  cellDates: {},
  nullError: {},
  UTC: {},
};

const marginFields = Object.keys(printMarginDefaults);

const layoutFields = ['hidden', 'level'];

// CE 0.20.3 keeps every <col> attribute as a raw string (with a lowercase alias)
// when cellStyles is set. Its writers read only width/wpx/wch/MDW/hidden/level,
// so these are accepted and ignored when writing.
const rawColumnAttributes = ['outlineLevel', 'outlinelevel', 'customwidth', 'style', 'bestFit', 'bestfit', 'collapsed', 'phonetic'];

// CE 0.20.3 WBPropsDef (ECMA-376 18.2.28). Its reader fills these defaults and its
// XLSX writer writes only values that differ (CodeName '' writes "ThisWorkbook").
// Writing other values needs Rust support, so only the defaults are accepted.
const workbookPropertyDefaults = {
  allowRefreshQuery: false, autoCompressPictures: true, backupFile: false, checkCompatibility: false,
  CodeName: '', defaultThemeVersion: 0, filterPrivacy: false, hidePivotFieldList: false,
  promptedSolutions: false, publishItems: false, refreshAllConnections: false, saveExternalLinkValues: true,
  showBorderUnselectedTables: true, showInkAnnotation: true, showObjects: 'all', showPivotChartFilter: false,
  updateLinks: 'userSet',
};

// Workbook keys CE 0.20.3 reads from workbook.xml but its writers ignore
// (write_wb_xml writes a fixed root, no fileVersion or calcPr, and bookViews
// only from Sheets[].Hidden). Views is checked separately.
const ignoredWorkbookMetadata = ['AppVersion', 'WBView', 'CalcPr', 'xmlns'];

// r (stored string markup) and h (HTML) are read projections; CE 0.20.3 ignores them when writing XLSX.
const xlsxCellFields = ['t', 'v', 'w', 'z', 'l', 'c', 'f', 'F', 'D', 's', 'r', 'h'];

const xlsxReadBoolean = (defaultValue, projection = {}) => ({
  check: 'optionalBoolean', message: 'Expected boolean worksheet projection options',
  default: defaultValue,
  ...projection,
});

const xlsxReadOptions = {
  type: {},
  sheets: { ...sheetSelectorCheck, referenceNote: sheetSelectionReference },
  bookSheets: { check: 'optionalBoolean', message: 'Expected a boolean bookSheets option' },
  bookProps: xlsxReadBoolean(false),
  bookVBA: xlsxReadBoolean(false, workbookResources.bookVBA),
  bookDeps: xlsxReadBoolean(false, workbookResources.bookDeps),
  bookFiles: xlsxReadBoolean(false, workbookResources.bookFiles),
  sheetRows: {
    check: 'optionalNonnegativeInteger', message: 'Expected a nonnegative sheetRows limit',
    default: 0,
    projection: {},
  },
  dense: xlsxReadBoolean(false, { projection: {} }),
  cellNF: xlsxReadBoolean(false, { projection: projectWithCellStyles }),
  cellText: xlsxReadBoolean(true, { projection: {} }),
  cellDates: xlsxReadBoolean(false, { projection: {} }),
  sheetStubs: xlsxReadBoolean(false, { projection: projectWithCellStyles }),
  nodim: xlsxReadBoolean(false, { projection: {} }),
  cellStyles: xlsxReadBoolean(false, { projection: {} }),
  cellFormula: xlsxReadBoolean(true, { projection: {} }),
  cellHTML: xlsxReadBoolean(true, { projection: {} }),
  xlfn: xlsxReadBoolean(false, { projection: {} }),
  dateNF: { ...dateFormatCheck, projection: {} },
  codepage: {},
  ...zipWorkbookUnusedOptions,
  WTF: {
    projection: { trueWhen: 'truthy' },
    referenceNote: 'With cellStyles, a theme fill color also reports its untinted theme color as raw_rgb. ' + parseErrorNote,
  },
};

// Preserve the order in which the adapter reads user options.
const xlsxProjectionOrder = [
  'sheetRows', 'dense', 'cellText', 'cellDates', 'nodim', 'cellStyles',
  'cellFormula', 'cellHTML', 'xlfn', 'dateNF', 'cellNF', 'sheetStubs', 'WTF',
];

// Preserve check order when several invalid values are supplied together.
const xlsxReadValidationOrder = [
  'dense', 'cellNF', 'cellText', 'cellDates', 'sheetStubs', 'nodim', 'cellStyles', 'cellFormula',
  'cellHTML', 'xlfn', 'bookProps', 'bookVBA', 'bookDeps', 'bookFiles',
  'dateNF', 'sheetRows', 'bookSheets', 'sheets',
];

const writeBooleanField = (key, presence = 'optional') => ({
  type: 'boolean', presence, message: `Expected a boolean ${key} option`,
});

export const readOptionsByFormat = {
  XLSX: xlsxReadOptions,
  XLSB: xlsbReadOptions,
  'compound XLS (BIFF5/8)': compoundReadOptions,
  'early BIFF (BIFF2/3/4)': earlyBiffReadOptions,
  'Lotus WK1/WK3': lotusReadOptions,
  DBF: dbfReadOptions,
};

export const contracts = {
  checkedZipEntryName: {
    scalarArgument: { check: 'unicodeString', message: 'Invalid ZIP entry name', code: 'INVALID_INPUT' },
  },
  byteReadOptionFields: { descriptorFields: { omit: ['type'] } },
  writeFileOptionFields: {
    descriptorFields: {
      required: ['type', 'file'],
      message: 'Expected a filename for file output',
    },
  },
  asyncFileOptionFields: {
    descriptorFields: {
      optional: ['bookType'],
      omit: ['type', 'file'],
      message: 'Unsupported XLSX options',
    },
  },
  isWritableNumberFormat: { testArgument: { check: 'unicodeString' } },
  isWellFormedWorksheetName: { testArgument: { check: 'wellFormedUnicode' } },
  hasAllowedWorksheetNamePunctuation: { testArgument: { check: 'noWorksheetNamePunctuation' } },
  hasAllowedWorksheetNameEdges: { testArgument: { check: 'noEdgeApostrophe' } },
  checkedMergeList: {
    scalarArgument: { check: 'array', message: 'Expected a list of merged ranges' },
  },
  isWorksheetRow: { testArgument: { check: 'integerRange', choices: [0, 1048575] } },
  isWorksheetColumn: { testArgument: { check: 'integerRange', choices: [0, 16383] } },
  validateWorksheetRowLayout: {
    properties: layoutFields,
    propertyGroups: { worksheetRowSizeKeys: ['hpt', 'hpx'] },
    ownKeysError: 'Unsupported worksheet layout entry',
    ownKeysCode: 'UNSUPPORTED_STRUCTURE',
    shapeOnly: true,
  },
  validateWorksheetColumnLayout: {
    properties: [...layoutFields, 'DBF', ...rawColumnAttributes],
    propertyGroups: { worksheetColumnSizeKeys: ['width', 'wpx', 'wch', 'MDW'] },
    ownKeysError: 'Unsupported worksheet layout entry',
    ownKeysCode: 'UNSUPPORTED_STRUCTURE',
    shapeOnly: true,
  },
  isWorkbookVisibilityMetadata: {
    testArgument: { check: 'objectWithOwnKeys', choices: ['Sheets', 'Names', 'WBProps', 'Views', ...ignoredWorkbookMetadata] },
  },
  // sheetId/sheetid/id are <sheet> attributes CE 0.20.3 reads and its writer
  // renumbers. CodeName is written only for workbooks with vbaraw in a non-xlsx
  // bookType, which a new workbook cannot have; loaded VBA workbooks check it in JS.
  isWorksheetVisibilityEntry: {
    testArgument: { check: 'objectWithOwnKeys', choices: ['Hidden', 'name', 'sheetId', 'sheetid', 'id', 'CodeName'] },
  },
  checkedWorkbookViews: {
    scalarArgument: { check: 'array', message: 'Expected a list of workbook views', code: 'UNSUPPORTED_STRUCTURE' },
  },
  // CE 0.20.3 writes Views[0].RTL as rightToLeft on every worksheet view and ignores zoom.
  xlsxWorkbookViewSettings: {
    ownDataProperties: true,
    ownKeysError: 'Expected a workbook view with RTL and zoom',
    ownKeysCode: 'UNSUPPORTED_STRUCTURE',
    fields: {
      RTL: sheetField('optionalBoolean', 'Expected a boolean RTL'),
      zoom: {},
    },
  },
  // Read defaults for the CE 0.20.3 WBViewDef keys among the workbookView
  // attributes that Wasm exposes, in its field order. integer: parsed like 0.20.3.
  workbookViewAttributes: {
    constant: {
      xWindow: {}, yWindow: {}, windowWidth: {}, windowHeight: {},
      firstSheet: { default: 0, integer: true }, activeTab: { default: 0, integer: true },
      visibility: { default: 'visible' },
    },
  },
  worksheetVisibilityCodes: { constant: worksheetVisibilityCodes },
  worksheetVisibilityStates: {
    constant: Object.fromEntries(Object.entries(worksheetVisibilityCodes).map(([state, code]) => [code, state])),
  },
  isWorksheetVisibility: { testArgument: { check: 'oneOf', choices: Object.values(worksheetVisibilityCodes) } },
  workbookDateSystemSettings: {
    ownDataProperties: true,
    ownKeysError: 'Expected workbook date system metadata',
    ownKeysCode: 'UNSUPPORTED_STRUCTURE',
    fields: {
      date1904: sheetField('optionalBoolean', 'Expected a boolean date1904'),
      ...Object.fromEntries(Object.entries(workbookPropertyDefaults).map(([key, value]) => [key, {
        check: 'optionalOneOf', choices: [value], code: 'UNSUPPORTED_STRUCTURE',
        message: `Writing a non-default WBProps.${key} is unsupported`,
      }])),
    },
  },
  isXlsxCellShape: { testArgument: { check: 'objectWithOwnKeys', choices: xlsxCellFields } },
  isXlmlImportedCellShape: {
    testArgument: { check: 'objectWithOwnKeys', choices: [...xlsxCellFields, 'StyleID', 'ixfe', 'Index'] },
  },
  worksheetPrintMarginPatch: {
    exportProperties: 'marginKeys',
    exportPropertyDefaults: { printMarginDefaults },
    indexedDefaults: 'printMarginDefaults',
    ownDataProperties: true,
    ownKeysError: 'Expected worksheet print margins',
    ownKeysCode: 'INVALID_VALUE',
    fields: Object.fromEntries(marginFields.map(key =>
      [key, sheetField('nullableNonnegativeNumber', 'Invalid worksheet print margin')])),
  },
  worksheetAutofilterSettings: {
    ownDataProperties: true,
    ownKeysError: 'Expected an autofilter with an A1 ref',
    ownKeysCode: 'INVALID_VALUE',
    fields: { ref: sheetField('nonemptyString', 'Expected an autofilter with an A1 ref') },
  },
  checkedAsyncFileCallback: {
    scalarArgument: { check: 'function', message: 'Expected an asynchronous file write callback' },
  },
  checkedDownloadFilename: {
    scalarArgument: { check: 'nonemptyString', message: 'Expected a download filename', code: 'INVALID_INPUT' },
  },
  checkedLegacyComments: {
    scalarArgument: { check: 'array', message: 'Expected an array of legacy comments', code: 'UNSUPPORTED_CELL' },
  },
  xlsxCellCommentSettings: {
    ownKeysError: 'Invalid SheetJS comment',
    ownKeysCode: 'INVALID_VALUE',
    fields: {
      a: sheetField('unicodeString', 'Expected a comment author and text'),
      t: sheetField('unicodeString', 'Expected a comment author and text'),
      // Read projections carry the stored markup and HTML; CE 0.20.3 ignores both when writing.
      r: {},
      h: {},
      // CE 0.20.3 writes a truthy T as a threaded comment, which this adapter does not create.
      T: { check: 'truthy', message: 'Threaded SheetJS comments are unsupported', code: 'UNSUPPORTED_CELL' },
    },
  },
  xlsxCellHyperlinkSettings: {
    ownDataProperties: true,
    rejectAccessors: ['display'],
    ownKeysError: 'Invalid SheetJS hyperlink',
    ownKeysCode: 'INVALID_VALUE',
    fields: {
      Target: sheetField('nonemptyUnicodeString', 'Expected hyperlink target and optional tooltip'),
      Tooltip: sheetField('optionalUnicodeString', 'Expected hyperlink target and optional tooltip'),
      display: sheetField('optionalUnicodeString', 'Expected an optional hyperlink display label'),
      // Read projections carry the element's ref/location and its relationship (CE 0.20.3 also
      // keeps r:id as id and xr:uid as uid); the 0.20.3 writer ignores all of them.
      ref: {},
      id: {},
      location: {},
      Rel: {},
      uid: {},
    },
  },
  xlsxDefinedNameSettings: {
    ownDataProperties: true,
    fields: {
      Name: sheetField('nonemptyString', 'Expected a defined name, reference and optional worksheet scope'),
      Ref: {},
      Sheet: sheetField('optionalU32', 'Expected a defined name, reference and optional worksheet scope'),
      Comment: sheetField('optionalString', 'Expected a defined name comment'),
    },
  },
  sheetArrayInputSettings: {
    fields: { ...sheetInputFields, sheetStubs: {}, date1904: {} },
  },
  sheetJsonInputSettings: {
    fields: { ...sheetInputFields, header: {}, skipHeader: {} },
  },
  sheetDomSettings: {
    fields: {
      origin: {}, sheetRows: { defaultIfFalsy: 10000000 }, display: {}, raw: {},
      cellDates: {}, UTC: {}, dateNF: {}, cellText: { defaultIfNullish: false },
    },
  },
  sheetHtmlSettings: {
    fields: {
      header: { defaultIfNullish: '<html><head><meta charset="utf-8"/><title>SheetJS Table Export</title></head><body>' },
      footer: { defaultIfNullish: '</body></html>' },
      id: {},
      editable: {},
    },
  },
  sheetNewSettings: { fields: { dense: {} } },
  sheetFormulaSettings: {
    applyFieldDefaults: true,
    fields: { values: { default: true } },
  },
  sheetArrayFormulaSettings: {
    applyFieldDefaults: true,
    fields: {
      formula: {
        type: 'nonblankUnicodeString', presence: 'required',
        message: 'Expected a nonblank array formula', code: 'INVALID_VALUE',
      },
      dynamic: { default: false },
    },
  },
  isJsonCellType: { testArgument: { check: 'oneOf', choices: ['s', 'b', 'n', 'd', 'e', 'z'] } },
  sheetJsonSettings: {
    ownBooleanDefaults: { raw: true },
    fields: { range: {}, header: {}, skipHidden: {}, defval: {}, blankrows: {}, rawNumbers: {}, UTC: {} },
  },
  sheetCsvSettings: {
    applyFieldDefaults: true,
    fields: {
      ...Object.fromEntries(delimitedWriteFields.map(key => [key, {}])),
      FS: { default: ',' },
      RS: { default: '\n' },
      date1904: {},
    },
  },
  readInputSettings: {
    matchOptions: true,
    validateBeforeMatch: true,
    partial: true,
    fields: {
      codepage: { check: 'optionalNonnegativeInteger', message: 'Expected a nonnegative integer codepage' },
      type: {
        type: 'enum',
        presence: 'optional',
        choices: [...byteTypes, 'string', 'file'],
        message: 'Unsupported XLSX input type',
      },
    },
  },
  textReadSettings: {
    matchOptions: true,
    fields: {
      type: {}, raw: {}, dense: {}, FS: {}, cellText: {}, sheetRows: {}, WTF: {},
      cellDates: checkedBooleanOption('cellDates'),
      cellNF: checkedBooleanOption('cellNF'),
      dateNF: { check: 'optionalString', message: 'Expected a dateNF format string' },
      UTC: checkedBooleanOption('UTC'),
      PRN: checkedBooleanOption('PRN'),
      codepage: {},
      cellFormula: checkedBooleanOption('cellFormula'),
      sheetStubs: checkedBooleanOption('sheetStubs'),
    },
    variantFields: {
      bookProps: checkedBooleanOption('bookProps'),
      bookSheets: checkedBooleanOption('bookSheets'),
    },
    validationOrder: [...textReadBooleans, 'dateNF'],
    variants: {
      plain: {},
      spreadsheetMl: { prefix: ['bookProps', 'bookSheets', 'cellStyles', 'sheets'] },
      openDocument: { prefix: ['sheets', 'bookProps', 'bookSheets'] },
    },
    checks: {
      checkOpenDocumentRowLimit: { sheetRows: textRowLimitCheck('FODS') },
      checkSpreadsheetMlProjection: {
        sheets: sheetSelectorCheck,
        cellStyles: { check: 'optionalBoolean', message: 'Invalid SpreadsheetML projection options' },
        dense: { check: 'optionalBoolean', message: 'Invalid SpreadsheetML projection options' },
        cellText: { check: 'optionalBoolean', message: 'Invalid SpreadsheetML projection options' },
        sheetRows: { check: 'optionalNonnegativeInteger', message: 'Invalid SpreadsheetML projection options' },
      },
      checkDifRowLimit: { sheetRows: textRowLimitCheck('DIF') },
      checkSylkRowLimit: { sheetRows: textRowLimitCheck('SYLK') },
      checkEthRowLimit: { sheetRows: textRowLimitCheck('EtherCalc') },
      checkRtfRowLimit: { sheetRows: textRowLimitCheck('RTF') },
      checkOpenDocumentSelectors: { sheets: sheetSelectorCheck },
    },
    projections: [sheetSelectionProjection('openDocumentWorkbookProjectionSettings')],
  },
  textWriteSettings: {
    matchOptions: true,
    fields: {
      bookType: {},
      sheet: {},
      codepage: { check: 'optionalNonnegativeInteger', message: 'Expected a nonnegative integer codepage' },
    },
    variantFields: { dateNF: dateFormatCheck },
    variants: Object.fromEntries(textFormats.flatMap(([formats, suffix]) => formats.flatMap(format => [
      [format, { suffix: [...suffix, 'type'] }],
      [format + ':file', { suffix }],
    ]))),
  },
  xlsbReadSettings: {
    matchOptions: true,
    fields: readFields(xlsbReadOptions),
    projections: [{
      name: 'xlsbSheetProjectionSettings',
      fields: worksheetProjectionFields(xlsbReadOptions, xlsbProjectionOrder),
    }, sheetSelectionProjection('xlsbWorkbookProjectionSettings'), {
      name: 'xlsbWorkbookCatalogueSettings',
      fields: {
        catalogueOnly: { from: ['bookSheets', 'bookProps'], transform: 'anyTruthy' },
        bookSheets: { from: 'bookSheets', transform: 'truthy' },
        bookProps: { from: 'bookProps', transform: 'truthy' },
      },
    },
    workbookResourceProjection('xlsbWorkbookResourceSettings', xlsbReadOptions)],
  },
  compoundReadSettings: {
    matchOptions: true,
    fields: readFields(compoundReadOptions),
    projections: [sheetSelectionProjection('compoundWorkbookProjectionSettings'), {
      name: 'compoundContainerSettings',
      fields: { raw: { from: ['bookFiles', 'raw'], transform: 'allTruthy' } },
    }],
  },
  earlyBiffReadSettings: {
    matchOptions: true,
    fields: readFields(earlyBiffReadOptions),
  },
  lotusReadSettings: {
    matchOptions: true,
    fields: readFields(lotusReadOptions),
  },
  dbfReadSettings: {
    matchOptions: true,
    fields: readFields(dbfReadOptions),
  },
  xlsxReadSettings: {
    matchOptions: true,
    fields: readFields(xlsxReadOptions),
    projections: [sheetSelectionProjection('xlsxWorkbookProjectionSettings'),
      workbookResourceProjection('xlsxWorkbookResourceSettings', xlsxReadOptions), {
      name: 'xlsxSheetProjectionSettings',
      fields: worksheetProjectionFields(xlsxReadOptions, xlsxProjectionOrder),
    }],
    validationOrder: xlsxReadValidationOrder,
  },
  writableProperties: {
    constant: {
      Author: 'creator', Title: 'title', Subject: 'subject',
      Comments: 'description', Identifier: 'identifier', Language: 'language',
      Keywords: 'keywords', Category: 'category', LastAuthor: 'lastModifiedBy',
      RevNumber: 'revision', Version: 'version', ContentStatus: 'contentStatus',
      CreatedDate: 'created', ModifiedDate: 'modified', LastPrinted: 'lastPrinted',
      Company: 'company', Manager: 'manager',
    },
  },
  documentPropertyOverrideModes: {
    constant: { company: 'omit', manager: 'omit', created: 'ifMissing', modified: 'ifMissing' },
  },
  isDocumentDateProperty: {
    testArgument: { check: 'oneOf', choices: Object.keys(corePropertyKinds).filter(key => corePropertyKinds[key] === 'date') },
  },
  isValidDocumentPropertyDate: { testArgument: { check: 'validDateTime' } },
  isBiffDateProperty: { testArgument: { check: 'oneOf', choices: ['EditTime', 'CreatedDate', 'ModifiedDate', 'LastPrinted'] } },
  isBiffListProperty: { testArgument: { check: 'oneOf', choices: ['SheetNames', 'DefinedNames', 'ChartNames'] } },
  isBiffDerivedProperty: { testArgument: {
    check: 'oneOf',
    choices: ['SystemIdentifier', 'CodePage', 'FMTID', 'Thumbnail', 'DigSig',
  'Worksheets', 'SheetNames', 'NamedRanges', 'DefinedNames', 'Chartsheets', 'ChartNames',
  'HeadingPairs', 'TitlesOfParts', '_PID_LINKBASE', '_PID_HLINKS'],
  } },
  isXlsbDerivedProperty: {
    testArgument: {
      check: 'oneOf',
      choices: ['Application', 'AppVersion', 'DocSecurity', 'HyperlinksChanged', 'SharedDoc',
      'LinksUpToDate', 'ScaleCrop', 'Worksheets', 'SheetNames', 'NamedRanges', 'DefinedNames', 'Chartsheets', 'ChartNames'],
    },
  },
  xlsxWriteSettings: {
    matchOptions: true,
    validateBeforeMatch: true,
    seed: true,
    configurableOmission: true,
    fields: {
      cellStyles: writeBooleanField('cellStyles'),
      sheetStubs: writeBooleanField('sheetStubs'),
      compression: writeBooleanField('compression'),
      ignoreEC: writeBooleanField('ignoreEC'),
      bookSST: writeBooleanField('bookSST'),
      cellDates: writeBooleanField('cellDates'),
      themeXLSX: { check: 'optionalString', message: 'Expected a themeXLSX XML string' },
      codepage: { check: 'optionalNonnegativeInteger', message: 'Expected a nonnegative integer codepage' },
      Props: {},
      ...writeFieldsFor('xlsx / xlsm / xlam'),
    },
  },
  odsWriteSettings: {
    matchOptions: true,
    seed: true,
    fields: {
      compression: { type: 'boolean', presence: 'optional', message: 'Expected a boolean compression option' },
      ...writeFieldsFor('ods'),
    },
  },
  fodsWriteSettings: { matchOptions: true, seed: true, fields: writeFieldsFor('fods') },
  xlmlWriteSettings: { matchOptions: true, seed: true, fields: { Props: {}, ...writeFieldsFor('xlml / xml') } },
  xlmlStreamSettings: {
    nullishDefaults: { stride: 10 },
    fields: { stride: sheetField('nullishNonnegativeSafeInteger', 'Expected a nonnegative SpreadsheetML row stride') },
  },
  xlsbWriteSettings: {
    matchOptions: true,
    validateBeforeMatch: true,
    seed: true,
    fields: {
      // Early checks run only on supplied keys, but reject explicit undefined here.
      compression: writeBooleanField('compression', 'required'),
      bookSST: writeBooleanField('bookSST', 'required'),
      Props: {},
      ...writeFieldsFor('xlsb'),
    },
  },
};
