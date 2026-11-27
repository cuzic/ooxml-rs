import {
  autofilterEndpointSettings, borderSides, cellAlignmentSettings, cellBorderDiagonalSettings, cellBorderSettings, cellBorderSideSettings, cellFontDefaults, cellHyperlinkSettings, cellProtectionSettings,
  cellModelSettings, cellNoteDefaults, cellNoteMarginDefaults, cellNoteMarginsSettings, cellNoteProtectionDefaults, cellNoteProtectionSettings, cellNoteSettings, columnModelSettings, cellRichTextSettings, checkedCellNote, checkedConditionalModelDxf, checkedConditionalThresholdGte, checkedWorkbookRevision, checkedWorksheetCreationState, checkedWorksheetModelId, corePropertyKinds, isCorePropertyDate, isCorePropertyText, checkedCellNumberFormat, checkedRowNumberFormat, checkedColumnNumberFormat, checkedDefaultColWidth,
  checkedDefaultRowHeight, checkedFillFormulaMode, checkedFillFormulaResults, checkedFillFormulaText, formulaRectangleSettings,
  checkedOutline, checkedPageMargins, checkedPrintReference, checkedRowDescent, checkedValidationModel, conditionalDataBarSettings, conditionalIconSetSettings, defaultCellStyleSettings, extendedDataBarFields, extendedDataBarSettings, gradientCenterSettings, gradientFillSettings, gradientStopSettings,
  conditionalAverageSettings, conditionalCustomIconSettings, conditionalFormattingSettings, conditionalRankSettings, conditionalRuleSettings,
  checkedTableColumnFilterButton, checkedTableColumnIndex, checkedTableColumnName, checkedTableColumnValues, checkedTableRow, checkedTableRowIndex, checkedTableStyleFlag, checkedTableTheme,
  checkedTableTotalFunction, checkedTableTotalText,
  checkedDefinedName, checkedDefinedNameAddress, checkedDefinedNamesModel, checkedImageAnchorOffset,
  definedNameLocationSettings, definedNameModelEntry, definedNameSpliceSettings, imageGridCoordinates,
  hasAllowedWorksheetNameEdges, hasAllowedWorksheetNamePunctuation, isWellFormedWorksheetName, headerFooterNames, headerFooterSettings, imageAnchorSettings, imageHyperlinkSettings,
  isExcelErrorCode, isGradientAngle,
  isComparisonOperator, isConditionalEndpoint, isConditionalFormula, isConditionalNumericThreshold, isFormulaOperand, isTwoBoundComparison, isValidationFlagName, isWorksheetGridRow, isSpliceCount, isWorksheetGridColumn, isWorksheetState, outlineSummaryKeys, pageMarginPatch,
  pageMarginNames, pageSettingDefaults, pageSettingNames, pageSetupEntries, pageSetupSettings, printMarginDefaults, printOptionEntries, printOptionNames, printReferenceNames,
  printerSettingDefaults, printerSettingEntries, rowModelSettings, rowStyleSettings, sheetProtectionDefaults, sheetProtectionModelSettings, sheetProtectionSettings,
  tableColumnDxfKeys, tableColumnSettings, tableDifferentialStyles, tableDxfKeys, tableModelColumnSettings, tableModelSettings, tableSettings, tableStyleFlags, tableStyleSettings, validateCellStyle,
  readCellFormulaResult, readSharedFormulaResult, validateColumnDefinition, validateConditionalRule, dataValidationSettings, patternFillSettings, differentialPatternFillSettings, validateWorkbookOutputSettings,
  worksheetPropertySettings, workbookCalculationSettings, workbookLoadSettings,
  workbookImageSettings, workbookModelSettings, workbookOutputSettings, workbookPropertySettings, workbookViewFields, workbookViewSettings,
  worksheetIterationSettings, worksheetMediaSettings, worksheetModelSettings, worksheetOutlineSettings, worksheetSettings, worksheetViewReadDefaults, worksheetViewSettings,
  zipCompressionSettings, zipDeflateSettings, zipLevelSettings, zipOutputSettings,
} from './exceljs-options.generated.js';
export { ErrorValue } from './exceljs-options.generated.js';
import { dateFromIso, isoCellText } from './sheetjs-date.js';
import { address, applyStreamingHyperlinks, applyStreamingNotes, copyInput, data, decodeBase64, failure, imageFileReader, prepareImageFile, prepareStreamingCellModel, registerStreamingStyle, replaceCellNames, rustCall, scalar, translateStreamingFormula, validateStreamingRowStyle } from './contract.js';
import { csvTextOf, effectiveTypeOf, excelDateMillis, formulaErrorCode, FormulaType, projectCellModel, textOf, typeOf, ValueType } from './exceljs-cell-value.js';
import { decode_range, encode_col } from './sheetjs-address.js';
import { gridAddress, mergeReference, rowCellColumn, rowModelCells, worksheetCellAddress, worksheetDimensions } from './exceljs-address.js';
import { decodeDeclaredFont, decodeDeclaredRichTextRuns, decodeRichTextRuns, encodeFont, encodeRichTextRuns, tabColorAttributes } from './exceljs-rich-text.js';
import { decodeAlignment, decodeBorder, decodeFill, decodeProtection, decodeStyleColor } from './exceljs-style-read.js';
import { XlsxDocument } from './runtime.js';
import { createCsv } from './exceljs-csv.js';
import { csvScalarCells } from './exceljs-csv-rows.js';

function formulaRange(range, allowSingleCell = false) {
  let firstRow, firstColumn, lastRow, lastColumn;
  if (range && typeof range === 'object' && !Array.isArray(range)) {
    const { top, left, bottom, right } = formulaRectangleSettings(range);
    firstRow = top - 1;
    firstColumn = left - 1;
    lastRow = bottom - 1;
    lastColumn = right - 1;
  } else {
    if (typeof range !== 'string') throw failure('INVALID_ADDRESS', 'Expected an A1 range or worksheet coordinates');
    const endpoints = range.split(':');
    if (endpoints.length !== 2 && !(allowSingleCell && endpoints.length === 1)) {
      throw failure('INVALID_ADDRESS', 'Expected two endpoints for the formula range');
    }
    [, firstRow, firstColumn] = worksheetCellAddress(endpoints[0]);
    [, lastRow, lastColumn] = worksheetCellAddress(endpoints[1] ?? endpoints[0]);
  }
  const top = Math.min(firstRow, lastRow);
  const left = Math.min(firstColumn, lastColumn);
  const bottom = Math.max(firstRow, lastRow);
  const right = Math.max(firstColumn, lastColumn);
  const anchor = gridAddress(top + 1, left + 1);
  return { top, left, bottom, right, anchor, reference: anchor + ':' + gridAddress(bottom + 1, right + 1) };
}

function parseOutlineSummary(value) {
  const settings = worksheetOutlineSettings(value);
  return outlineSummaryKeys.map(key => settings[key]);
}

function checkedDefaultDimension(rows, value) {
  return (rows ? checkedDefaultRowHeight : checkedDefaultColWidth)(value);
}

function encodeDefaultStyle(style, fontDefaults = cellFontDefaults, differential = false) {
  if (style === undefined) return [];
  const { numFmt, font, alignment, protection, fill, border } = defaultCellStyleSettings(style);
  const edits = [];
  if (numFmt !== undefined) edits.push(['number_format', numFmt]);
  if (font !== undefined) edits.push(['font', encodeFont(font, fontDefaults)]);
  if (alignment !== undefined) edits.push(['alignment', encodeAlignment(alignment)]);
  if (protection !== undefined) edits.push(['protection', ...encodeProtection(protection)]);
  if (fill !== undefined) edits.push(['fill', encodeFill(fill, differential)]);
  if (border !== undefined) edits.push(['border', encodeBorder(border)]);
  return edits;
}

function encodeTableColumnStyle(style) {
  return encodeDefaultStyle(style).flatMap(([kind, ...args]) => {
    const values = Array.isArray(args[0]) ? args[0] : args.map(String);
    return [kind, String(values.length), ...values];
  });
}

function captureTableColumnStyle(style) {
  return { fields: encodeTableColumnStyle(style), model: structuredClone(style) };
}

function writeTableColumnStyles(document, sheet, name, edits) {
  if (!edits.length) return;
  document.patch_table_column_styles(sheet, name, Uint32Array.from(
    edits.flatMap(([position, fields]) => [position, fields.length])),
  edits.flatMap(([, fields]) => fields));
}

function readTabColor(document, name, includeAutomatic = false) {
  const attributes = rustCall('WORKSHEET_READ_REFUSED', () => document.tab_color(name));
  const values = new Map();
  for (let index = 0; index < attributes.length; index += 2) {
    values.set(attributes[index], attributes[index + 1]);
  }
  if (values.has('rgb')) return { argb: values.get('rgb') };
  if (values.has('theme')) {
    const color = { theme: Number(values.get('theme')) };
    if (values.has('tint')) color.tint = Number(values.get('tint'));
    return color;
  }
  if (values.has('indexed')) return { indexed: Number(values.get('indexed')) };
  if (includeAutomatic && ['1', 'true'].includes(values.get('auto'))) return {};
  return undefined;
}

function filterRange(filter) {
  if (filter == null || typeof filter === 'string') return filter ?? undefined;
  const toAddress = (point) => {
    if (typeof point === 'string') return address(point)[0];
    const { row, column } = autofilterEndpointSettings(point);
    return gridAddress(row, column);
  };
  return `${toAddress(filter.from)}:${toAddress(filter.to)}`;
}

function readMargins(document, name) {
  return rustCall('WORKSHEET_READ_REFUSED', () => document.margins(name));
}

function writeMargins(document, name, margins) {
  checkedPageMargins(margins);
  const next = pageMarginPatch(margins, readMargins(document, name));
  if (next) {
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_margins(name, next));
  }
}

function normalizePrintReference(key, value) {
  if (value === '') return '';
  const input = checkedPrintReference(value).replaceAll('$', '');
  if (key === 'printArea') {
    return input.split('&&').map((range) => {
      if (!/^[a-z]+[0-9]+(?::[a-z]+[0-9]+)?$/i.test(range)) {
        throw failure('INVALID_ADDRESS', 'Expected a print area cell range');
      }
      return mergeReference([range]);
    }).join('&&');
  }
  const rows = key === 'printTitlesRow';
  const pattern = rows ? /^([1-9][0-9]*):([1-9][0-9]*)$/ : /^([A-Z]+):([A-Z]+)$/i;
  const match = pattern.exec(input);
  if (!match) throw failure('INVALID_ADDRESS', 'Expected a print title range');
  const points = match.slice(1).map((part) => rows ? address('A' + part)[1] : address(part + '1')[2]);
  points.sort((a, b) => a - b);
  return points.map((point) => rows ? String(point + 1) : encode_col(point)).join(':');
}

function readPrintReferences(document, name, titles) {
  const formulas = rustCall('WORKSHEET_READ_REFUSED', () => document.print_references(name, titles));
  return formulas.flatMap((formula) =>
    (formula.match(/(?:'(?:[^']|'')*'|[^,'])+/g) ?? []).map((part) =>
      part.slice(part.lastIndexOf('!') + 1).trim().replaceAll('$', '')));
}

function writePrintReferences(document, name, settings) {
  const prefix = "'" + name.replaceAll("'", "''") + "'!";
  if (Object.hasOwn(settings, 'printArea')) {
    const areas = settings.printArea ? settings.printArea.split('&&').map((range) =>
      prefix + range.replace(/([A-Z]+)([0-9]+)/g, (_, col, row) => '$' + col + '$' + row)) : [];
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_print_references(name, false, areas));
  }
  if (Object.hasOwn(settings, 'printTitlesRow') || Object.hasOwn(settings, 'printTitlesColumn')) {
    const current = readPrintReferences(document, name, true);
    const rows = settings.printTitlesRow ?? current.find((range) => /^\d+:\d+$/.test(range));
    const columns = settings.printTitlesColumn ?? current.find((range) => /^[A-Z]+:[A-Z]+$/i.test(range));
    const titles = [columns, rows].filter(Boolean).map((range) => prefix + '$' + range.replace(':', ':$'));
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_print_references(name, true, titles.length ? [titles.join(',')] : []));
  }
}

function parsePageSetup(options, creating = false) {
  return pageSetupSettings(options, (key, value) =>
    printReferenceNames.includes(key) ? normalizePrintReference(key, value) : value, creating);
}

function writePageSetup(document, name, settings) {
  if (settings.margins !== undefined) writeMargins(document, name, settings.margins);
  const values = pageSetupEntries(settings);
  if (values) rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_page_setup(name, values));
  const printerSettings = printerSettingEntries(settings);
  if (printerSettings.length) {
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_printer_settings(name, printerSettings));
  }
  writePrintReferences(document, name, settings);
  const updates = printOptionEntries(settings);
  if (updates) {
    const readError = rustCall('WORKSHEET_EDIT_REFUSED', () => document.update_print_options(name, updates));
    if (readError !== undefined) throw failure('WORKSHEET_READ_REFUSED', readError);
  }
}

function writeHeaderFooter(document, name, settings) {
  if (settings.changed) {
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_header_footer(name, settings.values, settings.changed));
  }
}

function encodeStyleColor(color) {
  const [kind, value, , tint] = tabColorAttributes(color);
  if (['theme', 'indexed'].includes(kind) && Number(value) > 255) {
    throw failure('INVALID_VALUE', 'Style color index exceeds 255');
  }
  if (!kind || kind === 'auto') return kind ?? '';
  return kind === 'theme' ? `theme:${value}:${tint ?? 0}` : `${kind}:${value}`;
}

function encodeBorder(input) {
  const border = cellBorderSettings(input);
  const diagonal = border.diagonal === undefined ? undefined : cellBorderDiagonalSettings(border.diagonal);
  const sides = borderSides.flatMap(key => {
    const side = key === 'diagonal' ? diagonal :
      border[key] === undefined ? undefined : cellBorderSideSettings(border[key]);
    return [side?.style ?? '', side?.style ? encodeStyleColor(side.color) : ''];
  });
  return [String(diagonal?.up ?? false), String(diagonal?.down ?? false), ...sides];
}

function validationReference(reference) {
  if (typeof reference !== 'string' || !reference.trim()) {
    throw failure('INVALID_ADDRESS', 'Expected an A1 cell or validation range');
  }
  return reference.trim().split(/\s+/).map((item) => {
    if (!/^\$?[a-z]+\$?\d+(?::\$?[a-z]+\$?\d+)?$/i.test(item)) {
      throw failure('INVALID_ADDRESS', 'Expected an A1 cell or validation range');
    }
    const range = item.replaceAll('$', '');
    return range.includes(':') ? mergeReference([range]) : address(range)[0];
  }).join(' ');
}

function encodeValidation(input) {
  if (input == null) return undefined;
  const rule = dataValidationSettings(input);
  const compare = !['any', 'list', 'custom'].includes(rule.type);
  const count = rule.type === 'any' ? 0 : compare && isTwoBoundComparison(rule.operator) ? 2 : 1;
  const entries = rule.formulae ?? (count === 0 ? [] : undefined);
  if (!Array.isArray(entries) || entries.length !== count) {
    throw failure('INVALID_VALUE', `Expected ${count} validation formula entries`);
  }
  const formulas = Array.from(entries, (entry) => {
    if (isFormulaOperand(entry)) return String(entry);
    if (rule.type === 'date' && entry instanceof Date && Number.isFinite(entry.getTime())) return new Date(entry.getTime());
    throw failure('INVALID_VALUE', 'Invalid validation formula');
  });
  return { rule, formulas };
}

function writeValidation(document, name, reference, settings, split = false) {
  if (!settings) return document.set_cell_validation(name, reference, [], split);
  const { rule, formulas } = settings;
  const [first = '', second = ''] = formulas;
  const firstDate = first instanceof Date ? first.getTime() : undefined;
  const secondDate = second instanceof Date ? second.getTime() : undefined;
  document.set_cell_validation(name, reference, [
    rule.type, rule.operator, firstDate === undefined ? first : '', secondDate === undefined ? second : '',
    String(rule.allowBlank),
    String(rule.showInputMessage), rule.promptTitle, rule.prompt,
    String(rule.showErrorMessage), rule.errorTitle, rule.error, rule.errorStyle,
  ], split, firstDate, secondDate);
}

function decodeValidation(document, attributes) {
  if (!attributes.length) return undefined;
  const rule = {};
  const formulas = [];
  for (let index = 0; index < attributes.length; index += 2) {
    const [key, value] = attributes.slice(index, index + 2);
    if (key === 'formula1' || key === 'formula2') formulas.push(value);
    else rule[key] = isValidationFlagName(key) ? value === '1' || value === 'true' : value;
  }
  if (rule.type === 'any') return rule;
  if (rule.type !== 'list' && rule.type !== 'custom') rule.operator ??= 'between';
  const type = rule.type;
  const literal = type === 'custom' || type === 'list';
  let dateOffset;
  rule.formulae = formulas.map((formula) => {
    const number = Number(formula);
    if (!Number.isFinite(number) || literal) return formula;
    if (type !== 'date') return number;
    dateOffset ??= document.date1904() ? 1462 : 0;
    return new Date((number - 25569 + dateOffset) * 86400000);
  });
  return rule;
}

function* validationModelRules(fields) {
  for (let offset = 0; offset < fields.length;) {
    const reference = fields[offset++];
    const end = offset + 1 + Number(fields[offset]) * 2;
    yield [reference, fields.slice(offset + 1, end)];
    offset = end;
  }
}

function prepareWorksheetProperties(input) {
  const properties = worksheetPropertySettings(input);
  const outlineSummary = properties.outlineProperties === undefined ? undefined :
    parseOutlineSummary(properties.outlineProperties);
  const tabColor = Object.hasOwn(input, 'tabColor') ? tabColorAttributes(properties.tabColor) : undefined;
  const dimensions = [[true, properties.defaultRowHeight], [false, properties.defaultColWidth]]
    .filter(([, value]) => value !== undefined);
  const levels = [[true, properties.outlineLevelRow], [false, properties.outlineLevelCol]]
    .filter(([, value]) => value !== undefined);
  return { outlineSummary, rowDescent: properties.dyDescent, tabColor, dimensions, levels };
}

function writeWorksheetProperties(document, name, properties, creating = false) {
  const { outlineSummary, rowDescent, tabColor, dimensions, levels } = properties;
  if (outlineSummary !== undefined) {
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_outline_summary(name, ...outlineSummary));
  }
  if (rowDescent !== undefined) {
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_default_row_descent(name, rowDescent));
  }
  for (const [rows, dimension] of dimensions) {
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_default_dimension(name, rows, dimension));
  }
  for (const [rows, level] of levels) {
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_outline_level(name, rows, level));
  }
  if (tabColor !== undefined && (!creating || tabColor.length)) {
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_tab_color(name, tabColor));
  }
}

function prepareValidationReplacement(input) {
  checkedValidationModel(input);
  const replacements = new Map();
  for (const key of Reflect.ownKeys(input)) {
    const reference = validationReference(key);
    const rule = encodeValidation(Object.getOwnPropertyDescriptor(input, key).value);
    if (!rule || replacements.has(reference)) {
      throw failure('INVALID_VALUE', 'Expected one data validation rule for each range');
    }
    replacements.set(reference, rule);
  }
  return replacements;
}

function writeValidationReplacement(document, name, fields, replacements) {
  for (const [reference] of validationModelRules(fields)) writeValidation(document, name, reference);
  for (const [reference, rule] of replacements) writeValidation(document, name, reference, rule);
}

function projectValidationModel(document, fields, explicitRanges) {
  const model = {};
  for (const [reference, attributes] of validationModelRules(fields)) {
    const rule = decodeValidation(document, attributes);
    if (explicitRanges.has(reference)) {
      model[reference] = rule;
      continue;
    }
    for (const token of reference.split(/\s+/)) {
      if (!token) continue;
      const { s, e } = decode_range(token.replaceAll('$', ''));
      for (let row = s.r; row <= e.r; row++) {
        for (let col = s.c; col <= e.c; col++) model[gridAddress(row + 1, col + 1)] = rule;
      }
    }
  }
  return model;
}

function encodeGradient(input) {
  const { gradient, degree, center, stops } = gradientFillSettings(input);
  let geometry;
  if (gradient === 'angle' && isGradientAngle(degree)) {
    geometry = ['angle', String(degree)];
  } else if (gradient === 'path') {
    const { left, top, right, bottom } = gradientCenterSettings(center);
    geometry = ['path', ...[left, top, right, bottom].map(String)];
  }
  const entries = Array.from(stops, gradientStopSettings);
  if (!geometry || entries.length < 2 || entries[0].position !== 0 || entries.at(-1).position !== 1) {
    throw failure('INVALID_VALUE', 'Invalid gradient geometry or stop positions');
  }
  return [...geometry, ...entries.flatMap(({ position, color }, index) => {
    if (index && position < entries[index - 1].position) {
      throw failure('INVALID_VALUE', 'Invalid gradient stop position');
    }
    const encoded = encodeStyleColor(color);
    if (!encoded) throw failure('INVALID_VALUE', 'Expected a gradient stop color');
    return [String(position), encoded];
  })];
}

function encodeFill(fill, differential = false) {
  const type = fill && typeof fill === 'object' ? Object.getOwnPropertyDescriptor(fill, 'type')?.value : undefined;
  if (type === 'gradient') return encodeGradient(fill);
  if (type !== 'pattern') throw failure('UNSUPPORTED_OPTIONS', 'Expected a supported cell fill pattern');
  const { pattern, fgColor, bgColor } = (differential ? differentialPatternFillSettings : patternFillSettings)(fill);
  return [pattern, encodeStyleColor(fgColor), encodeStyleColor(bgColor)];
}

function encodeProtection(value) {
  const { locked, hidden } = cellProtectionSettings(value);
  return [locked, hidden];
}

const cellStyleEncoders = {
  font: (value) => ['set_cell_font', encodeFont(value)],
  fill: (value) => ['set_cell_fill', encodeFill(value)],
  border: (value) => ['set_cell_border', encodeBorder(value)],
  alignment: (value) => ['set_cell_alignment', encodeAlignment(value)],
  numFmt(value) {
    return ['set_number_format', checkedCellNumberFormat(value)];
  },
  protection: (value) => ['set_cell_protection', ...encodeProtection(value)],
};

function encodeAlignment(value) {
  const { horizontal, vertical, textRotation, wrapText, indent, shrinkToFit, readingOrder } = cellAlignmentSettings(value);
  const rotation = textRotation === 'vertical' ? 255 : textRotation < 0 ? 90 - textRotation : textRotation;
  const order = { ltr: 1, rtl: 2 }[readingOrder];
  return [horizontal, vertical === 'middle' ? 'center' : vertical, rotation, wrapText, indent, shrinkToFit, order]
    .map((entry) => String(entry ?? ''));
}

const protectionPermissions = Object.keys(sheetProtectionDefaults);

function encodeConditionalStyle(style) {
  return encodeDefaultStyle(style, {}, true).flatMap(([kind, ...args]) => {
    const values = Array.isArray(args[0]) ? args[0] : args.map(String);
    return [kind, String(values.length), ...values];
  });
}

function readMetadataNodes(fields) {
  let index = 0;
  const readNode = () => {
    const tag = fields[index++];
    const count = Number(fields[index++]);
    const entries = Array.from({ length: count }, () => [fields[index++], fields[index++]]);
    const text = fields[index++];
    const children = Array.from({ length: Number(fields[index++]) }, readNode);
    return { tag, attributes: Object.fromEntries(entries), text, children };
  };
  const nodes = [];
  while (index < fields.length) nodes.push(readNode());
  return nodes;
}

const workbookImageRegistries = new WeakMap();
const imageForPlacement = Symbol('imageForPlacement');

function imageRegistry(workbook) {
  if (!workbookImageRegistries.has(workbook)) {
    workbookImageRegistries.set(workbook, { ids: new Map(), placements: new Map(), backgrounds: new Map(), fileParts: new Map(), media: [], sheets: new Map() });
  }
  return workbookImageRegistries.get(workbook);
}

function workbookImages(workbook, state) {
  const registry = imageRegistry(workbook);
  const version = state.stagedRevision ?? state.bytes;
  if (registry.version === version) return registry;
  registry.media.forEach((entry, index) => { if (entry && !entry.buffer) delete registry.media[index]; });
  const { ids, media } = registry;
  const sheets = new Map();
  const backgroundIds = new Map();
  for (const sheet of workbook.worksheets) {
    const data = rustCall('WORKSHEET_READ_REFUSED', () => state.document.worksheet_images(sheet.name));
    const images = [];
    for (let offset = 0; offset < data.length;) {
      const end = offset + 1 + Number(data[offset]);
      const [index, shape, part, name, altText, hyperlink, tooltip, editAs, ...position] = data.slice(offset + 1, end);
      if (!ids.has(part)) ids.set(part, media.length);
      const imageId = registry.placements.get(sheet)?.get(shape) ?? ids.get(part);
      if (!media[imageId]?.buffer) {
        media[imageId] = { sheet: sheet.name, index: Number(index), extension: part.split('.').at(-1).toLowerCase() };
      }
      if (!state.ignoredNodes?.has('drawing') || registry.placements.get(sheet)?.has(shape)) {
        images.push({ shape, imageId, name, altText, hyperlink, tooltip, editAs, position: position.map(Number) });
      }
      offset = end;
    }
    const part = rustCall('WORKSHEET_READ_REFUSED', () => state.document.worksheet_background_image(sheet.name));
    if (part !== undefined) {
      if (!ids.has(part)) ids.set(part, media.length);
      const imageId = registry.backgrounds.get(sheet) ?? ids.get(part);
      if (!media[imageId]?.buffer) {
        media[imageId] = { sheet: sheet.name, background: true, extension: part.split('.').at(-1).toLowerCase() };
      }
      if (!state.ignoredNodes?.has('picture') || registry.backgrounds.has(sheet)) {
        backgroundIds.set(sheet, imageId);
      }
    }
    sheets.set(sheet.name, images);
  }
  Object.assign(registry, { version, sheets, backgroundIds });
  return registry;
}

function encodeImageMarker(sheet, marker) {
  if (!marker || typeof marker !== 'object') throw failure('INVALID_VALUE', 'Expected an image anchor');
  let { nativeCol, nativeColOff = 0, nativeRow, nativeRowOff = 0 } = marker;
  if (nativeCol === undefined && nativeRow === undefined) {
    const { col, row } = imageGridCoordinates(marker);
    nativeCol = Math.floor(col);
    nativeRow = Math.floor(row);
    const column = sheet.getColumn(nativeCol + 1);
    nativeColOff = Math.floor((col - nativeCol) * (column.isCustomWidth ? Math.floor(column.width * 10000) : 640000));
    nativeRowOff = Math.floor((row - nativeRow) * Math.floor((sheet.findRow(nativeRow + 1)?.height ?? 18) * 10000));
  }
  gridAddress(nativeRow + 1, nativeCol + 1);
  return [nativeCol, checkedImageAnchorOffset(nativeColOff), nativeRow, checkedImageAnchorOffset(nativeRowOff)];
}

function excelImageAnchor(sheet, [nativeCol, nativeColOff, nativeRow, nativeRowOff]) {
  return {
    nativeCol, nativeColOff, nativeRow, nativeRowOff,
    get col() {
      const column = sheet.getColumn(nativeCol + 1);
      const width = column.isCustomWidth ? Math.floor(column.width * 10000) : 640000;
      return nativeCol + Math.min(width - 1, nativeColOff) / width;
    },
    get row() {
      const height = Math.floor((sheet.findRow(nativeRow + 1)?.height ?? 18) * 10000);
      return nativeRow + Math.min(height - 1, nativeRowOff) / height;
    },
  };
}


function encodeTableCells(cells) {
  const formulas = cells.filter((value) => value?.formula !== undefined);
  return [
    cells.map((value) => value instanceof Date ? value.toISOString() : value?.formula ?? value?.error ?? value),
    Uint8Array.from(cells, (value) => {
      if (value instanceof Date) return 4;
      if (value?.error !== undefined) return 5;
      if (value?.formula === undefined) return 0;
      if (value.result instanceof Date) return 3;
      return value.result?.error === undefined ? 1 : 2;
    }),
    formulas.map(({ result }) => result instanceof Date ? result.toISOString() : result?.error ?? result),
  ];
}

function writeTableInitialValues(document, sheet, row, column, width, rows) {
  if (rows.length === 0) return;
  const cells = rows.flat();
  const reference = gridAddress(row + 1, column + 1) + ':' + gridAddress(row + rows.length, column + width);
  const isScalar = (value) => value === null || typeof value !== 'object';
  const supportsBatch = (value) => isScalar(value) || value.error !== undefined ||
    (value.formula !== undefined && value.range === undefined && !(value.result instanceof Date));
  const writeBatch = (first, last) => {
    if (first === last) return;
    const values = first === 0 && last === cells.length ? cells : cells.slice(first, last);
    const encoded = values.every(isScalar) ? [values, new Uint8Array(), []] : encodeTableCells(values);
    document.write_table_initial_values(sheet, reference, first, ...encoded);
  };
  let first = 0;
  for (const [index, value] of cells.entries()) {
    if (supportsBatch(value)) continue;
    writeBatch(first, index);
    writeExcelValue(document, sheet, row + Math.floor(index / width), column + index % width, value);
    first = index + 1;
  }
  writeBatch(first, cells.length);
}

function writeTableRows(document, sheet, name, width, rows, position) {
  const placement = Uint32Array.from(position === undefined ? [width] : [width, position]);
  document.write_table_rows(sheet, name, placement, ...encodeTableCells(rows.flat()));
}

function encodeTableColumn(options, totalsRow, defaultLabel) {
  const { name, filterButton, totalsRowFunction, totalsRowLabel, totalsRowFormula, totalsRowResult, style } =
    tableColumnSettings(options);
  const label = totalsRowLabel ?? (totalsRowFunction === 'none' ? defaultLabel : undefined);
  let kind = 'none';
  let value = '';
  if (totalsRow) {
    if (totalsRowFunction === 'custom') {
      kind = 'custom';
      value = totalsRowFormula;
    } else if (label !== undefined) {
      kind = 'label';
      value = label;
    } else {
      kind = totalsRowFunction;
    }
  }
  const result = excelFormulaResult(totalsRowResult);
  if (result !== undefined && (kind === 'none' || kind === 'label')) {
    throw failure('INVALID_VALUE', 'A totals result requires a formula or function');
  }
  return { settings: [name, String(filterButton), kind, value], result,
    style: style === undefined ? undefined : captureTableColumnStyle(style) };
}

function encodeTableDxf(model, keys = tableDxfKeys) {
  const styles = model.differentialStyles === undefined ? {} : tableDifferentialStyles(model.differentialStyles);
  if (Object.entries(styles).some(([key, style]) => style !== undefined && !keys.includes(key))) {
    throw failure('UNSUPPORTED_OPTIONS', 'Unsupported table column differential style field');
  }
  return keys.flatMap(key => {
    const style = styles[key];
    if (style === undefined) {
      if (model[key] !== undefined) {
        throw failure('UNSUPPORTED_OPTIONS', 'Table differential format ID requires a resolved style');
      }
      return [''];
    }
    const fields = encodeConditionalStyle(style);
    return [String(fields.length), ...fields];
  });
}

function prepareTableModel(input) {
  const model = tableModelSettings(input);
  const { name, displayName = name, tableRef, headerRow = true, totalsRow = false, style, columns } = model;
  const settingsStyle = { theme: null, ...style };
  for (const key of ['theme', ...tableStyleFlags]) {
    if (model[key] !== undefined) settingsStyle[key] = model[key];
  }
  const checkedStyle = tableStyleSettings(settingsStyle);
  const settings = [name, displayName, String(headerRow), String(totalsRow), checkedStyle.theme ?? '',
    ...tableStyleFlags.map(key => String(checkedStyle[key]))];
  const definitions = Array.from(columns, column => {
    const checked = tableModelColumnSettings(column);
    const { calculatedColumnFormula, colId, dxfId, headerRowDxfId, dataDxfId, totalsRowDxfId, differentialStyles,
      ...options } = checked;
    const { settings } = encodeTableColumn(options, true);
    if (options.totalsRowFunction === 'none') {
      settings[2] = settings[2] === 'label' ? 'label-with-none' : 'explicit-none';
    }
    return { settings, calculatedColumnFormula: calculatedColumnFormula ?? '', differential: encodeTableDxf(checked, tableColumnDxfKeys) };
  });
  const differential = [...encodeTableDxf(model), ...definitions.flatMap(definition => definition.differential)];
  return { name, tableRef, settings, columns: definitions.flatMap(definition => definition.settings),
    calculated: definitions.map(definition => definition.calculatedColumnFormula),
    differential: differential.some(Boolean) ? differential : [] };
}

function writeTableTotals(document, sheet, name, totals) {
  if (!totals.length) return;
  const positions = [];
  const settings = [];
  const values = [];
  const kinds = [];
  for (const [position, kind, value, result] of totals) {
    positions.push(position);
    settings.push(kind, value);
    if (result instanceof Date) {
      values.push(result.toISOString());
      kinds.push('d');
    } else {
      values.push(result?.error ?? result);
      kinds.push(result?.error === undefined ? '' : 'e');
    }
  }
  document.set_table_column_totals(sheet, name, Uint32Array.from(positions), settings, values, kinds);
}

function tableEditValue(input) {
  const value = excelValue(input ?? null);
  if (value === null || typeof value !== 'object' || value instanceof Date || value.error !== undefined ||
      (value.formula !== undefined && value.range === undefined)) return value;
  throw failure('INVALID_VALUE', 'Expected a scalar, Date, Excel error, or ordinary formula');
}

const tableColumnIds = new WeakMap();
const tableColumnsById = new WeakMap();

function readTableDxfIds(attributes, keys = tableDxfKeys) {
  const ids = {};
  for (const key of keys) {
    if (attributes[key] !== undefined) ids[key] = Number(attributes[key]);
  }
  return ids;
}

function readTable({ attributes, children }) {
  const { name, displayName = name, ref: tableRef, headerRowCount, totalsRowCount } = attributes;
  const styleInfo = children.find(({ tag }) => tag === 'tableStyleInfo')?.attributes ?? {};
  const bool = (value) => value === '1' || value === 'true';
  const { name: theme } = styleInfo;
  const style = { theme };
  for (const key of tableStyleFlags) {
    style[key] = bool(styleInfo[key]);
  }
  const autoFilter = children.find(({ tag }) => tag === 'autoFilter');
  const filters = autoFilter?.children ?? [];
  const columns = (children.find(({ tag }) => tag === 'tableColumns')?.children ?? []).map((column, index) => {
    const { name, totalsRowFunction, totalsRowLabel } = column.attributes;
    const filter = filters.find(({ attributes }) => Number(attributes.colId) === index);
    const filterButton = Boolean(autoFilter) && !bool(filter?.attributes.hiddenButton) &&
      (filter?.attributes.showButton === undefined || bool(filter.attributes.showButton));
    const model = { name, totalsRowFunction, totalsRowLabel, filterButton, ...readTableDxfIds(column.attributes, tableColumnDxfKeys),
      totalsRowFormula: column.children.find(({ tag }) => tag === 'totalsRowFormula')?.text,
      calculatedColumnFormula: column.children.find(({ tag }) => tag === 'calculatedColumnFormula')?.text };
    tableColumnIds.set(model, column.attributes.id ?? index);
    return model;
  });
  const model = {
    name, displayName, ref: tableRef.split(':')[0], tableRef, headerRow: headerRowCount !== '0',
    totalsRow: Number(totalsRowCount ?? 0) > 0, ...style, ...readTableDxfIds(attributes), columns,
  };
  tableColumnsById.set(model, new Map(columns.map((column) => [tableColumnIds.get(column), column])));
  return model;
}

function readConditionalFormats(fields) {
  return readMetadataNodes(fields).map(({ attributes, children }) => ({
    ref: attributes.sqref,
    rules: children.map(readConditionalRule),
  }));
}

function differentialColor({ rgb, theme, indexed, tint, auto } = {}) {
  if (rgb !== undefined) return { argb: rgb };
  if (theme !== undefined) return tint === undefined ? { theme: Number(theme) } : { theme: Number(theme), tint: Number(tint) };
  if (indexed !== undefined) return { indexed: Number(indexed) };
  if (auto === '1' || auto === 'true') return {};
}

function readDifferentialStyles(document, ids) {
  const byId = new Map();
  if (!ids.size) return byId;
  const styles = rustCall('WORKSHEET_READ_REFUSED', () => document.differential_formats(Uint32Array.from(ids)));
  for (let index = 0; index < styles.length;) {
    const id = Number(styles[index]);
    const end = index + 2 + Number(styles[index + 1]);
    byId.set(id, readDifferentialStyle(styles.slice(index + 2, end)));
    index = end;
  }
  return byId;
}

function readDifferentialStyle(fields) {
  const [root] = readMetadataNodes(fields);
  if (!root) return undefined;
  const style = {};
  const flag = (value) => value === undefined || value === '1' || value === 'true';
  for (const { tag, attributes, children } of root.children) {
    if (tag === 'numFmt') style.numFmt = attributes.formatCode;
    if (tag === 'alignment') style.alignment = decodeAlignment(Object.entries(attributes).flat());
    if (tag === 'protection') {
      style.protection = { locked: flag(attributes.locked), hidden: attributes.hidden !== undefined && flag(attributes.hidden) };
    }
    if (tag === 'font') {
      const font = {};
      for (const { tag, attributes } of children) {
        const value = attributes.val;
        if (['b', 'i', 'strike'].includes(tag)) font[{ b: 'bold', i: 'italic', strike: 'strike' }[tag]] = flag(value);
        if (['name', 'scheme', 'vertAlign'].includes(tag)) font[tag] = value;
        if (['sz', 'family', 'charset'].includes(tag)) font[tag === 'sz' ? 'size' : tag] = Number(value);
        if (tag === 'u') font.underline = value === undefined || value === 'single' ? true : value;
        if (tag === 'color') font.color = differentialColor(attributes);
      }
      style.font = font;
    }
    if (tag === 'border') {
      const border = {};
      for (const side of children) {
        const value = {};
        const colorAttributes = side.children.find(({ tag }) => tag === 'color')?.attributes;
        if (!side.attributes.style && !['rgb', 'theme', 'indexed'].some((key) => colorAttributes?.[key])) continue;
        if (side.attributes.style !== undefined) value.style = side.attributes.style;
        if (side.children[0]) value.color = differentialColor(side.children[0].attributes);
        if (side.tag === 'diagonal') {
          value.up = attributes.diagonalUp !== undefined && flag(attributes.diagonalUp);
          value.down = attributes.diagonalDown !== undefined && flag(attributes.diagonalDown);
        }
        border[side.tag] = value;
      }
      style.border = border;
    }
    if (tag === 'fill') {
      const fill = children[0];
      if (fill?.tag === 'patternFill') {
        // An omitted dxf patternType means solid. 4.4.0 reports no pattern key, but the Rust writer also omits
        // patternType for an explicit 'solid', so the two cannot be told apart here (see the ledger).
        const pattern = { type: 'pattern', pattern: fill.attributes.patternType ?? 'solid' };
        for (const color of fill.children) pattern[color.tag] = differentialColor(color.attributes);
        style.fill = pattern;
      } else if (fill?.tag === 'gradientFill') {
        const path = fill.attributes.type === 'path';
        const stops = fill.children.map(({ attributes, children }) =>
          ({ position: Number(attributes.position), color: differentialColor(children[0]?.attributes) }));
        style.fill = path
          ? { type: 'gradient', gradient: 'path', center: Object.fromEntries(
            ['left', 'right', 'top', 'bottom'].filter((key) => fill.attributes[key] !== undefined)
              .map((key) => [key, Number(fill.attributes[key])])), stops }
          : { type: 'gradient', gradient: 'angle', degree: Number(fill.attributes.degree ?? 0), stops };
      }
    }
  }
  return style;
}

function readExtendedConditionalRule({ attributes, children }) {
  const visual = children.find(child => child.tag === attributes.type);
  if (!visual || !['dataBar', 'iconSet'].includes(visual.tag)) return undefined;
  const rule = { type: attributes.type, x14Id: attributes.id };
  if (attributes.priority !== undefined) rule.priority = Number(attributes.priority);
  const props = visual.attributes;
  const bool = (key, fallback) => props[key] === undefined ? fallback : props[key] === '1' || props[key] === 'true';
  rule.cfvo = visual.children.filter(child => child.tag === 'cfvo').map(child => {
    const result = { type: child.attributes.type };
    if (visual.tag === 'iconSet' && child.attributes.gte !== undefined) {
      result.gte = child.attributes.gte === '1' || child.attributes.gte === 'true';
    }
    const value = child.children.find(item => item.tag === 'f')?.text;
    if (value !== undefined) result.value = result.type === 'formula' ? value : Number.parseFloat(value || '0');
    return result;
  });
  if (visual.tag === 'iconSet') {
    rule.iconSet = props.iconSet ?? '3TrafficLights';
    rule.reverse = bool('reverse', false);
    rule.showValue = bool('showValue', true);
    const icons = visual.children.filter(child => child.tag === 'cfIcon');
    if (icons.length) rule.icons = icons.map(({ attributes }) => ({
      iconSet: attributes.iconSet, iconId: Number(attributes.iconId),
    }));
  } else {
    rule.minLength = Number(props.minLength ?? 0);
    rule.maxLength = Number(props.maxLength ?? 100);
    rule.border = bool('border', false);
    rule.gradient = bool('gradient', true);
    rule.negativeBarColorSameAsPositive = bool('negativeBarColorSameAsPositive', true);
    rule.negativeBarBorderColorSameAsPositive = bool('negativeBarBorderColorSameAsPositive', true);
    rule.axisPosition = props.axisPosition ?? 'auto';
    rule.direction = props.direction ?? 'leftToRight';
    if (props.showValue !== undefined) rule.showValue = bool('showValue', true);
    for (const child of visual.children) {
      if (['fillColor', 'borderColor', 'negativeFillColor', 'negativeBorderColor', 'axisColor'].includes(child.tag)) {
        rule[child.tag] = differentialColor(child.attributes);
      }
    }
  }
  return rule;
}

function mergeExtendedConditionalFormats(blocks, fields) {
  const extensionIds = new Map();
  const byRef = new Map(blocks.map(block => [block.ref, block]));
  const byId = new Map(blocks.flatMap(block => block.rules.filter(rule => rule.x14Id)
    .map(rule => [rule.x14Id.toLowerCase(), rule])));
  for (const { children } of readMetadataNodes(fields)) {
    const ref = children.find(child => child.tag === 'sqref')?.text.trim();
    if (!ref) continue;
    for (const child of children) {
      if (child.tag !== 'cfRule') continue;
      const rule = readExtendedConditionalRule(child);
      if (!rule) continue;
      const linked = byId.get(rule.x14Id?.toLowerCase());
      if (linked) {
        if (rule.type === 'dataBar' && linked.cfvo !== undefined && !sameModelData(rule.cfvo, linked.cfvo)) {
          linked.extendedCfvo = rule.cfvo;
        }
        for (const [key, value] of Object.entries(rule)) {
          if (linked[key] === undefined) linked[key] = value;
        }
      } else {
        let block = byRef.get(ref);
        if (!block) {
          block = { ref, rules: [] };
          blocks.push(block);
          byRef.set(ref, block);
        }
        block.rules.push(rule);
        if (!extensionIds.has(block)) extensionIds.set(block, []);
        extensionIds.get(block).push(rule.x14Id ?? '');
      }
    }
  }
  return extensionIds;
}

function readConditionalRule({ attributes, children }) {
  const rule = { ...attributes };
  const boolean = (value) => value === '1' || value === 'true';
  for (const key of ['priority', 'rank', 'dxfId', 'stdDev']) {
    if (rule[key] !== undefined) rule[key] = Number(rule[key]);
  }
  for (const key of ['stopIfTrue', 'percent', 'bottom', 'aboveAverage', 'equalAverage']) {
    if (rule[key] !== undefined) rule[key] = boolean(rule[key]);
  }
  if (['containsBlanks', 'notContainsBlanks', 'containsErrors', 'notContainsErrors'].includes(rule.type)) {
    rule.operator = rule.type;
    rule.type = 'containsText';
  }
  const link = children.find(child => child.tag === 'extLst')?.children
    .find(child => child.tag === 'ext' && child.attributes.uri?.toLowerCase() === '{b025f937-c7b1-47d3-b67f-a62eff666e3e}');
  const id = link?.children.find(child => child.tag === 'id')?.text.trim();
  if (id) rule.x14Id = id;
  const formulae = children.filter(({ tag }) => tag === 'formula').map(({ text }) => text);
  if (formulae.length) rule.formulae = formulae;
  const visual = children.find(({ tag }) => ['colorScale', 'iconSet', 'dataBar'].includes(tag));
  if (visual) {
    rule.cfvo = visual.children.filter(({ tag }) => tag === 'cfvo').map(({ attributes: { type, val, gte } }) => {
      const threshold = val === undefined ? { type } : { type, value: type === 'formula' ? val : Number(val) };
      if (visual.tag === 'iconSet' && gte !== undefined) threshold.gte = boolean(gte);
      return threshold;
    });
    if (visual.tag === 'colorScale') {
      rule.color = visual.children.filter(({ tag }) => tag === 'color')
        .map(({ attributes }) => differentialColor(attributes));
    }
    if (visual.tag === 'dataBar') {
      const color = visual.children.find(({ tag }) => tag === 'color');
      rule.color = differentialColor(color?.attributes);
      // Only declared attributes are reported (4.4.0 reports none from <dataBar>; see the ledger for the difference).
      if (visual.attributes.minLength !== undefined) rule.minLength = Number(visual.attributes.minLength);
      if (visual.attributes.maxLength !== undefined) rule.maxLength = Number(visual.attributes.maxLength);
    }
    if (visual.tag === 'iconSet') {
      rule.iconSet = visual.attributes.iconSet ?? '3TrafficLights1';
      rule.reverse = boolean(visual.attributes.reverse);
    }
    if (visual.tag === 'iconSet' && (!id || visual.attributes.showValue !== undefined)) rule.showValue = boolean(visual.attributes.showValue ?? '1');
    if (visual.tag === 'dataBar' && visual.attributes.showValue !== undefined) rule.showValue = boolean(visual.attributes.showValue);
  }
  return rule;
}

function conditionalThresholdFields(threshold) {
  const { type, value } = threshold ?? {};
  if (isConditionalEndpoint(type)) return [type, ''];
  if (type === 'formula' && isConditionalFormula(value)) return [type, value];
  if (isConditionalNumericThreshold(type) && Number.isFinite(value)) return [type, String(value)];
  throw failure('INVALID_VALUE', 'Invalid conditional formatting threshold');
}

function prepareConditionalFormatting(options, fromModel = false) {
  const { ref, rules: inputRules } = conditionalFormattingSettings(options);
  const rules = Array.from(inputRules, (input) => {
    const rule = fromModel ? conditionalModelRule(input) : input;
    const fields = conditionalRuleFields(rule, fromModel);
    const { priority, stopIfTrue } = conditionalRuleSettings(rule);
    return { fields, priority, stopIfTrue, style: encodeConditionalStyle(rule.style) };
  });
  const headers = new Uint32Array(rules.length * 4);
  const entries = [];
  for (const [index, { fields, priority, stopIfTrue, style }] of rules.entries()) {
    headers.set([fields.length + 1, style.length, priority ?? 0, Number(stopIfTrue)], index * 4);
    entries.push(ref, ...fields, ...style);
  }
  return { headers, entries };
}

function conditionalModelRule(input) {
  if (!input || typeof input !== 'object') throw failure('INVALID_VALUE', 'Expected a conditional formatting rule');
  const { dxfId, ...rule } = input;
  checkedConditionalModelDxf(dxfId);
  if (dxfId !== undefined && rule.style === undefined) {
    throw failure('UNSUPPORTED_OPTIONS', 'Conditional formatting model style ID requires a resolved style');
  }
  if (rule.type === 'notContainsText' || rule.type === 'beginsWith' || rule.type === 'endsWith') {
    rule.operator ??= rule.type === 'notContainsText' ? 'notContains' : rule.type;
    rule.type = 'containsText';
  }
  return rule;
}

function sameModelData(input, projected) {
  if (Object.is(input, projected)) return true;
  if (input === null || projected === null || typeof input !== 'object' || typeof projected !== 'object' ||
      Object.getPrototypeOf(input) !== Object.getPrototypeOf(projected)) return false;
  const keys = Reflect.ownKeys(projected);
  if (Reflect.ownKeys(input).length !== keys.length) return false;
  return keys.every(key => {
    const property = Object.getOwnPropertyDescriptor(input, key);
    return property && 'value' in property && sameModelData(property.value, projected[key]);
  });
}

function removeConditionalFormats(candidate, sheet, standard, extended) {
  if (extended.length) candidate.remove_standalone_conditional_formats(sheet, extended);
  if (standard.length) candidate.remove_selected_conditional_formats(sheet, Uint32Array.from(standard));
}

function conditionalIconSetFields(rule) {
  const { cfvo, iconSet, reverse, showValue } = conditionalIconSetSettings(rule);
  return [iconSet, String(reverse), String(showValue), ...Array.from(cfvo, threshold =>
    [...conditionalThresholdFields(threshold), String(checkedConditionalThresholdGte(threshold?.gte) ?? '')]).flat()];
}

const extendedDataBarDefaults = extendedDataBarSettings({});
const extendedDataBarFlags = ['border', 'gradient', 'negativeBarColorSameAsPositive',
  'negativeBarBorderColorSameAsPositive', 'axisPosition', 'direction'];

function conditionalRuleFields(rule, fromModel) {
  if (!rule || typeof rule !== 'object') throw failure('INVALID_VALUE', 'Expected a conditional formatting rule');
  const { type, formulae, operator, text } = rule;
  if (type === 'iconSet' && ((fromModel && rule.x14Id !== undefined) || rule.icons !== undefined ||
      ['3Stars', '3Triangles', '5Boxes'].includes(rule.iconSet))) {
    const { x14Id, icons, custom, ...icon } = rule;
    if (x14Id !== undefined && (!fromModel || typeof x14Id !== 'string' || !x14Id)) {
      throw failure('INVALID_VALUE', 'Expected an extended icon rule ID');
    }
    if ((icons !== undefined && (!Array.isArray(icons) || !icons.length)) ||
        (custom !== undefined && custom !== (icons !== undefined))) {
      throw failure('INVALID_VALUE', 'Invalid conditional formatting icon set');
    }
    const choices = Array.from(icons ?? [], (value, index) => {
      const { iconSet, iconId } = conditionalCustomIconSettings(value);
      return [iconSet, String(iconId ?? index)];
    });
    validateConditionalRule(icon, type);
    const [name, reverse, showValue, ...thresholds] = conditionalIconSetFields(icon);
    return ['extendedIconSet', x14Id ?? '', String(choices.length), name, reverse, showValue, ...choices.flat(), ...thresholds];
  }
  if (type === 'dataBar' && rule.gradient === true && rule.x14Id === undefined) {
    // 4.4.0 writes no x14 extension for a gradient bar (dropping any other x14 option). When
    // every other x14 option is omitted or at its default, write the same standard-only rule.
    const { border, gradient, negativeBarColorSameAsPositive, negativeBarBorderColorSameAsPositive,
      axisPosition, direction, ...standard } = rule;
    const settings = extendedDataBarSettings({ border, gradient, negativeBarColorSameAsPositive,
      negativeBarBorderColorSameAsPositive, axisPosition, direction });
    if (extendedDataBarFlags.every(key => settings[key] === extendedDataBarDefaults[key]) &&
        extendedDataBarFields.every(key => standard[key] === undefined)) {
      return conditionalRuleFields(standard, fromModel);
    }
  }
  if (type === 'dataBar' && ((fromModel && rule.x14Id !== undefined) ||
      extendedDataBarFields.some(key => rule[key] !== undefined))) {
    const { x14Id, border, gradient, negativeBarColorSameAsPositive, negativeBarBorderColorSameAsPositive,
      axisPosition, direction, fillColor, borderColor, negativeFillColor, negativeBorderColor, axisColor, extendedCfvo, ...standard } = rule;
    if (x14Id !== undefined && (!fromModel || typeof x14Id !== 'string' || !x14Id)) {
      throw failure('INVALID_VALUE', 'Expected an extended data bar rule ID');
    }
    const settings = extendedDataBarSettings({ border, gradient, negativeBarColorSameAsPositive,
      negativeBarBorderColorSameAsPositive, axisPosition, direction,
      minLength: standard.minLength, maxLength: standard.maxLength });
    // As 4.4.0: both lengths are always written (0 and 100 when omitted); the other x14
    // attributes only when they differ from its defaults (an empty field omits the attribute).
    const flags = extendedDataBarFlags.map(key =>
      settings[key] === extendedDataBarDefaults[key] ? '' : String(settings[key]));
    const colors = [fillColor, borderColor, negativeFillColor, negativeBorderColor, axisColor].map(encodeStyleColor);
    const standalone = fromModel && x14Id !== undefined && !Object.hasOwn(standard, 'color');
    const extendedThresholds = standalone ? standard.cfvo : extendedCfvo;
    if (extendedThresholds !== undefined && (!Array.isArray(extendedThresholds) || extendedThresholds.length !== 2)) {
      throw failure('INVALID_VALUE', 'Expected two extended data bar thresholds');
    }
    const thresholds = Array.from(extendedThresholds ?? [], threshold =>
      ['autoMin', 'autoMax'].includes(threshold?.type) ? [threshold.type, ''] : conditionalThresholdFields(threshold));
    let fields;
    if (standalone) {
      validateConditionalRule(standard, type);
      const { minLength, maxLength } = conditionalDataBarSettings(standard);
      fields = ['standaloneDataBar', '', String(minLength), String(maxLength), String(standard.showValue ?? ''), '', '', '', ''];
    } else {
      fields = conditionalRuleFields(standard);
    }
    return [...fields, x14Id ?? '', String(settings.minLength), String(settings.maxLength),
      ...flags, ...colors, ...thresholds.flat()];
  }
  if (fromModel && type === 'containsText') {
    const { formulae, ...predicate } = rule;
    if (formulae === undefined) return conditionalRuleFields(predicate);
    if (!Array.isArray(formulae) || formulae.length !== 1 || !isConditionalFormula(formulae[0])) {
      throw failure('INVALID_VALUE', 'Expected a conditional formatting predicate formula');
    }
    const fields = conditionalRuleFields({ ...predicate, text: text ?? '' });
    return [...fields, formulae[0], text === undefined ? '1' : '0', operator === undefined ? '1' : '0'];
  }
  validateConditionalRule(rule, type);
  if (type === 'timePeriod') {
    const { timePeriod } = rule;
    if (typeof timePeriod !== 'string' || (formulae !== undefined &&
        (!Array.isArray(formulae) || formulae.length !== 1 || typeof formulae[0] !== 'string' || !formulae[0].trim()))) {
      throw failure('INVALID_VALUE', 'Invalid conditional formatting time period');
    }
    return [type, timePeriod, formulae?.[0] ?? ''];
  }
  if (type === 'dataBar') {
    const { cfvo, color, minLength, maxLength, showValue } = conditionalDataBarSettings(rule);
    const encoded = encodeStyleColor(color);
    if (!encoded || minLength > maxLength) {
      throw failure('INVALID_VALUE', 'Invalid conditional formatting data bar');
    }
    // Only declared options reach the file (an empty field leaves the attribute out), as 4.4.0 reports only them.
    const declared = (key, value) => rule[key] === undefined ? '' : String(value);
    return [type, encoded, declared('minLength', minLength), declared('maxLength', maxLength), declared('showValue', showValue),
      ...Array.from(cfvo, conditionalThresholdFields).flat()];
  }
  if (type === 'iconSet') return [type, ...conditionalIconSetFields(rule)];
  if (type === 'colorScale') {
    const { cfvo, color } = rule;
    if (!Array.isArray(cfvo) || !Array.isArray(color) || ![2, 3].includes(cfvo.length) || color.length !== cfvo.length) {
      throw failure('INVALID_VALUE', 'Expected two or three matching thresholds and colors');
    }
    const stops = Array.from(cfvo, (threshold, index) => {
      const encoded = encodeStyleColor(color[index]);
      if (!encoded) throw failure('INVALID_VALUE', 'Expected a color for each threshold');
      return [...conditionalThresholdFields(threshold), encoded];
    });
    return [type, ...stops.flat()];
  }
  if (type === 'containsText') {
    const operation = operator ?? 'containsText';
    if (['containsBlanks', 'notContainsBlanks', 'containsErrors', 'notContainsErrors'].includes(operation)) {
      return [type, operation, ''];
    }
    if (!['containsText', 'notContains', 'beginsWith', 'endsWith'].includes(operation) || typeof text !== 'string') {
      throw failure('INVALID_VALUE', 'Invalid conditional formatting text comparison');
    }
    return [type, operation, text];
  }
  if (type === 'top10') {
    const { rank, percent, bottom } = conditionalRankSettings(rule);
    return [type, String(rank), String(percent), String(bottom)];
  }
  if (type === 'aboveAverage') {
    const { aboveAverage, equalAverage, stdDev } = conditionalAverageSettings(rule);
    return [type, String(aboveAverage), String(equalAverage ?? ''), String(stdDev ?? '')];
  }
  if (type === 'duplicateValues' || type === 'uniqueValues') return [type];
  if (type === 'cellIs' && !isComparisonOperator(operator)) {
    throw failure('INVALID_VALUE', 'Invalid conditional formatting operator');
  }
  const count = isTwoBoundComparison(operator) ? 2 : 1;
  if (!Array.isArray(formulae) || formulae.length !== count || Array.from(formulae).some(value => !isFormulaOperand(value))) {
    throw failure('INVALID_VALUE', 'Invalid conditional formatting formulas');
  }
  const [first, second = ''] = formulae.map(String);
  return type === 'expression' ? [type, first] : [type, operator, first, second];
}

function parseViews(views) {
  if (!Array.isArray(views)) throw failure('INVALID_VALUE', 'Expected an array of worksheet views');
  const parsed = Array.from(views, parseView);
  const ids = new Set(parsed.map((view) => view.workbookViewId));
  if (ids.size !== parsed.length) throw failure('INVALID_VALUE', 'Duplicate worksheet workbookViewId');
  return parsed;
}

function parseView(view) {
  const {
    state, xSplit, ySplit, zoomScale, showGridLines, rightToLeft, workbookViewId, topLeftCell, activeCell,
    zoomScaleNormal, showRuler, showRowColHeaders, tabSelected, style, activePane,
  } = worksheetViewSettings(view);
  if (state !== 'split' && (activePane !== undefined ||
      [xSplit, ySplit].some(value => !Number.isInteger(value) || value > 0xffffffff))) {
    throw failure('INVALID_VALUE', 'Invalid worksheet view');
  }
  return {
    workbookViewId, state, xSplit, ySplit, zoomScale, showGridLines, rightToLeft,
    zoomScaleNormal, showRuler, showRowColHeaders, tabSelected, style,
    activePane: state === 'split' ? activePane ?? 'topLeft' : undefined,
    topLeftCell: topLeftCell === undefined ? undefined : address(topLeftCell)[0],
    activeCell: activeCell === undefined ? undefined : address(activeCell)[0],
  };
}

function writeViews(document, name, views) {
  const ids = Uint32Array.from(views, (view) => view.workbookViewId);
  rustCall('WORKSHEET_EDIT_REFUSED', () => document.reset_sheet_views(name, ids));
  for (const view of views) {
    rustCall('WORKSHEET_EDIT_REFUSED', () => document.set_sheet_view(
      name, view.workbookViewId, view.state, view.xSplit, view.ySplit,
      view.zoomScale, view.showGridLines, view.rightToLeft,
      view.zoomScaleNormal, view.showRuler, view.showRowColHeaders, view.tabSelected, view.style,
      view.topLeftCell, view.activeCell, view.activePane,
    ));
  }
}

function readViews(document, name) {
  const entries = rustCall('WORKSHEET_READ_REFUSED', () => document.sheet_views(name));
  const views = [];
  for (let offset = 0; offset < entries.length;) {
    const end = offset + 1 + Number(entries[offset]) * 2;
    views.push(readView(entries.slice(offset + 1, end)));
    offset = end;
  }
  return views;
}

function readView(entries) {
  const view = { ...worksheetViewReadDefaults };
  for (let index = 0; index < entries.length; index += 2) {
    const [key, value] = [entries[index], entries[index + 1]];
    const flag = value === '1' || value === 'true';
    if (['workbookViewId', 'zoomScale', 'zoomScaleNormal', 'xSplit', 'ySplit'].includes(key)) view[key] = Number(value);
    else if (typeof worksheetViewReadDefaults[key] === 'boolean') view[key] = flag;
    else if (key === 'tabSelected') { if (flag) view.tabSelected = true; }
    else view[key === 'view' ? 'style' : key] = value;
  }
  if (view.state === 'frozenSplit') view.state = 'frozen';
  if (view.state === 'frozen' || view.state === 'split') {
    view.xSplit ??= 0;
    view.ySplit ??= 0;
  }
  if (view.state === 'split') view.activePane ??= 'topLeft';
  else delete view.activePane;
  return view;
}

export { ValueType, FormulaType } from './exceljs-cell-value.js';
export const RelationshipType = {
  None: 0, OfficeDocument: 1, Worksheet: 2, CalcChain: 3,
  SharedStrings: 4, Styles: 5, Theme: 6, Hyperlink: 7,
};
export const DocumentType = { Xlsx: 1 };
export const ReadingOrder = { LeftToRight: 1, RightToLeft: 2 };

function excelFormulaResult(result) {
  if (result == null) return undefined;
  if (result instanceof Date) return excelValue(result);
  const error = formulaErrorCode(result);
  if (error !== undefined) return { error };
  return scalar(result, true);
}

function excelValue(input) {
  if (input === null) return null;
  if (input instanceof Date) return new Date(excelDateMillis(input));
  if (input && typeof input === 'object') {
    const error = Object.getOwnPropertyDescriptor(input, 'error')?.value;
    if (isExcelErrorCode(error)) return { error };
    if (Object.hasOwn(input, 'richText')) {
      const { richText } = cellRichTextSettings(input);
      return { richText: encodeRichTextRuns(richText) };
    }
    if (Object.hasOwn(input, 'hyperlink')) return cellHyperlinkSettings(input);
    const shared = Object.getOwnPropertyDescriptor(input, 'sharedFormula');
    if (shared) {
      const result = readSharedFormulaResult(input, 'member');
      if (typeof shared.value !== 'string') {
        throw failure('UNSUPPORTED_OPTIONS', 'Expected a shared formula master address and an optional result');
      }
      return { sharedFormula: worksheetCellAddress(shared.value)[0], result: excelFormulaResult(result) };
    }
    const formula = Object.getOwnPropertyDescriptor(input, 'formula');
    if (formula && typeof formula.value === 'string') {
      const shareType = Object.getOwnPropertyDescriptor(input, 'shareType')?.value;
      const hasRange = shareType === 'array' || shareType === 'shared';
      const result = readCellFormulaResult(input, hasRange ? 'range' : 'ordinary');
      const value = { formula: scalar(formula.value, false), result: excelFormulaResult(result) };
      if (hasRange) {
        const ref = Object.getOwnPropertyDescriptor(input, 'ref')?.value;
        if (typeof ref !== 'string') throw failure('INVALID_ADDRESS', 'Expected an A1 range for the formula');
        value.range = formulaRange(ref, true);
        value.shareType = shareType;
      }
      return value;
    }
  }
  return scalar(input, true);
}

function writeExcelRangeFormula(document, sheet, row, col, reference, formula, result, shareType) {
  const date = result instanceof Date;
  const value = date ? document.excel_date_serial(result.getTime()) : result?.error ?? result;
  const type = result?.error === undefined ? undefined : 'e';
  if (shareType === 'shared') document.write_shared_formula_master(sheet, reference, formula, value, type);
  else document.write_array_formula(sheet, reference, formula, false, value, type);
  if (date && !document.is_date_cell(sheet, row, col)) {
    document.set_number_format(sheet, row, col, 'mm-dd-yy');
  }
}

function writeExcelValue(document, sheet, row, column, value) {
  const reference = gridAddress(row + 1, column + 1);
  if (value?.hyperlink !== undefined) {
    document.set_cell_hyperlink(sheet, reference, value.hyperlink, value.text, value.tooltip);
    return;
  }
  document.clear_cell_hyperlink(sheet, reference);
  if (value?.richText !== undefined) document.set_cell_rich_text(sheet, row, column, value.richText);
  else if (value instanceof Date) {
    if (document.read_excel_iso_date(sheet, row, column) !== undefined) {
      document.write_iso_date(sheet, row, column, isoCellText(value));
    } else {
      document.write_excel_date(sheet, row, column, value.getTime());
    }
  } else if (value?.sharedFormula !== undefined) {
    const { sharedFormula, result } = value;
    const date = result instanceof Date;
    const cached = date ? document.excel_date_serial(result.getTime()) : result?.error ?? result;
    document.write_shared_formula_member(sheet, row, column, sharedFormula, cached,
      result?.error === undefined ? undefined : 'e');
    if (date && !document.is_date_cell(sheet, row, column)) {
      document.set_number_format(sheet, row, column, 'mm-dd-yy');
    }
  } else if (value?.formula !== undefined) {
    const { formula, result, range, shareType } = value;
    if (range !== undefined) {
      if (range.top !== row || range.left !== column) {
        throw failure('INVALID_ADDRESS', 'Formula range must start at the assigned cell');
      }
      writeExcelRangeFormula(document, sheet, row, column, range.reference, formula, result, shareType);
    } else if (result instanceof Date) {
      const serial = document.excel_date_serial(result.getTime());
      document.write_formula_with_result(sheet, row, column, formula, serial, undefined);
      if (!document.is_date_cell(sheet, row, column)) document.set_number_format(sheet, row, column, 'mm-dd-yy');
    } else {
      document.write_formula_with_result(sheet, row, column, formula, result?.error ?? result,
        result?.error === undefined ? undefined : 'e');
    }
  } else if (value?.error !== undefined) document.write_excel_error(sheet, row, column, value.error);
  else document.write_scalar(sheet, row, column, value, true, true);
}

function cellModelValue(model) {
  const { type, value, text, hyperlink, tooltip, shareType, ref, formula, sharedFormula, result } = model;
  if (type === ValueType.Null) return null;
  if (type === ValueType.Hyperlink) return excelValue({ text, hyperlink, tooltip });
  if (type === ValueType.Formula) {
    if (sharedFormula !== undefined) return excelValue({ sharedFormula, result });
    const input = shareType === undefined ? { formula, result } : { formula, shareType, ref, result };
    const formulaValue = excelValue(input);
    if (formulaValue?.formula !== undefined) return formulaValue;
  } else if (typeOf(value) === type || (type === ValueType.String && typeOf(value) === ValueType.RichText)) {
    return excelValue(value);
  }
  throw failure('INVALID_VALUE', 'Cell model type does not match its value');
}

function encodeNoteBox({ margins, protection, editAs }) {
  return [margins.insetmode, protection.locked, protection.lockText, editAs, ...margins.inset.map(String)];
}

function prepareCellNote(value) {
  if (value == null || typeof value !== 'object') return checkedCellNote(value) ?? undefined;
  const { texts, margins, protection, editAs } = cellNoteSettings(value);
  const [first] = texts;
  const plain = texts.length === 1 && first && typeof first === 'object' && Reflect.ownKeys(first).length === 1
    ? Object.getOwnPropertyDescriptor(first, 'text')?.value : undefined;
  const plainText = typeof plain === 'string' ? checkedCellNote(plain) : undefined;
  if (margins === undefined && protection === undefined && editAs === undefined) {
    return plainText ?? { runs: encodeRichTextRuns(texts), box: [] };
  }
  const { insetmode = cellNoteMarginDefaults.insetmode, inset = cellNoteMarginDefaults.inset } =
    margins === undefined ? {} : cellNoteMarginsSettings(margins);
  const { locked = cellNoteProtectionDefaults.locked, lockText = cellNoteProtectionDefaults.lockText } =
    protection === undefined ? {} : cellNoteProtectionSettings(protection);
  return {
    runs: plainText === undefined ? encodeRichTextRuns(texts) : [plainText, ...Array(12).fill('')],
    box: encodeNoteBox({ margins: { insetmode, inset }, protection: { locked, lockText },
      editAs: editAs ?? cellNoteDefaults.editAs }),
  };
}

function cellModelNote(comment) {
  if (comment === undefined) return undefined;
  if (comment?.type !== 'note' || !comment.note || typeof comment.note !== 'object') {
    throw failure('UNSUPPORTED_OPTIONS', 'Expected a worksheet note model');
  }
  return prepareCellNote(comment.note);
}

function writeCellNote(candidate, sheet, row, column, note) {
  if (note && typeof note === 'object') candidate.set_cell_note_runs(sheet, row, column, note.runs, note.box);
  else candidate.set_cell_note(sheet, row, column, note);
}

function mergesFromRowModels(rows, plannedRanges) {
  const links = [];
  const followers = [];
  const rangesByAnchor = plannedRanges && new Map(plannedRanges.map(range =>
    [gridAddress(range.s.r + 1, range.s.c + 1), range]));
  for (const row of rows) {
    const { number, cells } = rowModelSettings(row);
    for (const input of cells) {
      if (input === null || typeof input !== 'object' || data(input, 'type') !== ValueType.Merge) continue;
      const model = cellModelSettings(input);
      const { address: follower, master, style, comment } = model;
      if (plannedRanges && style === undefined && comment === undefined) continue;
      const [anchor, anchorRow, anchorColumn] = address(master);
      const [, row, column] = address(follower);
      if (plannedRanges) {
        const range = rangesByAnchor.get(anchor);
        if (!range || row < range.s.r || row > range.e.r || column < range.s.c || column > range.e.c) {
          if (!plannedRanges.some(({ s, e }) => s.r <= row && row <= e.r && s.c <= column && column <= e.c)) continue;
          throw failure('INVALID_ADDRESS', 'Merged follower master must match the planned range');
        }
      }
      if (row !== number - 1 || (row === anchorRow && column === anchorColumn)) {
        throw failure('INVALID_ADDRESS', 'Invalid merged follower address in row model');
      }
      if (plannedRanges && (row < anchorRow || column < anchorColumn)) {
        throw failure('INVALID_ADDRESS', 'Merge master must be the upper-left cell of the inferred range');
      }
      if (style !== undefined || comment !== undefined) {
        followers.push({ row, column, anchor, hasStyle: style !== undefined, model: prepareCellModel(model, true) });
      }
      if (!plannedRanges) links.push(anchorRow, anchorColumn, row, column);
    }
  }
  const ranges = links.length ? rustCall('INVALID_ADDRESS', () =>
    XlsxDocument.infer_merge_ranges(Uint32Array.from(links))) : [];
  return { ranges, followers };
}

function prepareCellModel(model, merged = false) {
  const value = merged ? undefined : cellModelValue(model);
  const note = cellModelNote(model.comment);
  const style = model.style ?? {};
  validateCellStyle(style, true);
  const edits = Reflect.ownKeys(style).map(name => cellStyleEncoders[name](style[name]));
  return { value, note, edits, merged };
}

function writeCellModel(candidate, sheet, row, column, model) {
  if (candidate.cell_style_index(sheet, row, column) !== 0) {
    candidate.set_cell_style_index(sheet, row, column, 0);
  }
  if (!model.merged) writeExcelValue(candidate, sheet, row, column, model.value);
  for (const [method, ...args] of model.edits) candidate[method](sheet, row, column, ...args);
  if (model.note !== undefined) writeCellNote(candidate, sheet, row, column, model.note);
}

function noteModel(texts, fields = []) {
  const [insetmode, locked, lockText, editAs, ...inset] = fields;
  return {
    margins: { insetmode: insetmode || cellNoteMarginDefaults.insetmode, inset: inset.length === 4 ? inset.map(Number) : [...cellNoteMarginDefaults.inset] },
    protection: { locked: locked || cellNoteProtectionDefaults.locked, lockText: lockText || cellNoteProtectionDefaults.lockText },
    editAs: editAs || cellNoteDefaults.editAs, texts,
  };
}

const htmlEntities = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;' };

const destroyRowHandle = Symbol('destroyRowHandle');

class Row {
  #worksheet;
  #number;
  #columns;
  #hasValues;
  #layout;
  #replaceValues;
  #splice;
  #style;

  constructor(worksheet, number, columns, hasValues, layout, replaceValues, splice) {
    this.#worksheet = worksheet;
    this.#number = number;
    this.#columns = columns;
    this.#hasValues = hasValues;
    this.#layout = layout;
    this.#replaceValues = replaceValues;
    this.#splice = splice;
    const row = this;
    this.#style = {
      get border() { return row.border; },
      set border(value) { row.border = value; },
      get fill() { return row.fill; },
      set fill(value) { row.fill = value; },
      get protection() { return row.protection; },
      set protection(value) { row.protection = value; },
      get alignment() { return row.alignment; },
      set alignment(value) { row.alignment = value; },
      get font() { return row.font; },
      set font(value) { row.font = value; },
      get numFmt() { return row.numFmt; },
      set numFmt(code) {
        if (code === undefined) row.#layout.clearStyle();
        else row.numFmt = code;
      },
    };
  }

  splice(start, count, ...values) {
    this.#splice(start, count, values);
  }

  addPageBreak(left, right) {
    this.#layout.addPageBreak(left, right);
  }

  get worksheet() { return this.#worksheet; }
  commit() {
    if (!this.#worksheet) throw new TypeError('Row has been destroyed');
    // Document rows already apply their edits as they are made.
  }

  destroy() {
    this.#worksheet?.[destroyRowHandle](this);
    this.#worksheet = undefined;
    this.#columns = undefined;
    this.#hasValues = undefined;
    this.#layout = undefined;
    this.#replaceValues = undefined;
    this.#splice = undefined;
    this.#style = undefined;
  }

  get number() { return this.#number; }
  get model() {
    const [height, hidden, outlineLevel] = this.#layout.read();
    const columns = this.#columns();
    if (!height && !columns.length) return null;
    const cells = columns.map(([column]) => this.#worksheet.getCell(this.#number, column).model);
    const style = Object.fromEntries(Object.entries(this.#style).filter(([, value]) => value !== undefined));
    return {
      cells, number: this.#number, min: columns[0]?.[0] ?? 0, max: columns.at(-1)?.[0] ?? 0,
      height, style, hidden, outlineLevel,
      collapsed: outlineLevel > 0 && outlineLevel >= this.#worksheet.properties.outlineLevelRow,
    };
  }
  set model(input) { this.#layout.replaceModel(input); }
  get style() { return this.#style; }
  set style(value) {
    const edits = encodeDefaultStyle(value);
    if (edits.length) this.#layout.writeStyle(edits);
    else this.#layout.clearStyle();
  }

  get border() { return this.#layout.readBorder(); }
  set border(value) { this.#layout.writeStyle([['border', encodeBorder(value)]]); }
  get fill() { return this.#layout.readFill(); }
  set fill(value) { this.#layout.writeStyle([['fill', encodeFill(value)]]); }
  get protection() { return this.#layout.readProtection(); }
  set protection(value) { this.#layout.writeStyle([['protection', ...encodeProtection(value)]]); }
  get alignment() { return this.#layout.readAlignment(); }
  set alignment(value) { this.#layout.writeStyle([['alignment', encodeAlignment(value)]]); }
  get font() { return this.#layout.readFont(); }
  set font(value) { this.#layout.writeStyle([['font', encodeFont(value)]]); }
  get numFmt() { return this.#layout.readNumberFormat(); }
  set numFmt(code) {
    checkedRowNumberFormat(code);
    this.#layout.writeStyle([['number_format', code]]);
  }
  get height() { return this.#layout.read()[0]; }
  set height(value) { this.#layout.write(value, undefined); }
  get hidden() { return this.#layout.read()[1]; }
  set hidden(value) { this.#layout.write(undefined, value); }
  get collapsed() {
    const level = this.outlineLevel;
    return level > 0 && level >= this.#worksheet.properties.outlineLevelRow;
  }
  get outlineLevel() { return this.#layout.read()[2]; }
  set outlineLevel(value) { this.#layout.write(undefined, undefined, checkedOutline(value)); }

  getCell(column) {
    return this.#worksheet.getCell(this.#number, this.#worksheet.getColumn(column).number);
  }

  getCellEx(location) {
    return this.#worksheet.getCell(this.#number, rowCellColumn(this.#number, location));
  }

  findCell(column) {
    return this.#worksheet.findCell(this.#number, column);
  }

  get dimensions() {
    let min;
    let max;
    for (const [col, present] of this.#columns()) {
      if (!present) continue;
      min ??= col;
      max = col;
    }
    return min === undefined ? null : { min, max };
  }

  get cellCount() { return this.#columns().at(-1)?.[0] ?? 0; }
  get actualCellCount() { return this.#columns().filter(([, present]) => present).length; }
  get hasValues() { return this.#hasValues(); }

  eachCell(options, iteratee) {
    if (typeof options === 'function') [options, iteratee] = [undefined, options];
    if (worksheetIterationSettings(options).includeEmpty) {
      const count = this.cellCount;
      for (let col = 1; col <= count; col += 1) iteratee(this.getCell(col), col);
      return;
    }
    for (const [col, present] of this.#columns()) {
      if (present) iteratee(this.getCell(col), col);
    }
  }

  get values() {
    const values = [];
    this.eachCell((cell, col) => { values[col] = cell.value; });
    return values;
  }

  set values(entries) {
    this.#replaceValues(entries);
  }
}

function parseColumnModel(columns) {
  if (!Array.isArray(columns)) throw failure('INVALID_VALUE', 'Expected worksheet model column ranges');
  const ranges = Array.from(columns, input => {
    const { min, max, width, style, isCustomWidth, hidden, outlineLevel } = columnModelSettings(input);
    gridAddress(1, min);
    gridAddress(1, max);
    if (max < min) throw failure('INVALID_ADDRESS', 'Expected an ordered column range');
    return { min, max, width: isCustomWidth === false ? undefined : width, hidden: hidden ?? false,
      outline: checkedOutline(outlineLevel ?? 0), style: encodeDefaultStyle(style) };
  }).sort((a, b) => a.min - b.min);
  for (let index = 1; index < ranges.length; index++) {
    if (ranges[index - 1].max >= ranges[index].min) {
      throw failure('INVALID_VALUE', 'Worksheet model column ranges overlap');
    }
  }
  return ranges;
}

function writeColumnModel(document, name, ranges) {
  const fields = document.sheet_layout(name);
  for (let index = 0; index < fields.length; index += 6) {
    if (fields[index] !== 'column') continue;
    const first = Number(fields[index + 1]) - 1;
    const last = Number(fields[index + 2]) - 1;
    document.clear_column_width(name, first, last);
    document.clear_column_style(name, first, last);
    document.set_column_layout(name, first, undefined, false, 0, last);
  }
  for (const { min, max, width, hidden, outline, style } of ranges) {
    const first = min - 1;
    const last = max - 1;
    document.set_column_layout(name, first, width, hidden, outline, last);
    for (const [kind, ...args] of style) document['set_column_' + kind](name, first, ...args);
    if (style.length && first !== last) document.extend_column_style(name, first, last);
  }
}

function modelImageAnchor({ nativeCol, nativeColOff, nativeRow, nativeRowOff }) {
  return { nativeCol, nativeColOff, nativeRow, nativeRowOff };
}

const assertLiveHandles = Symbol('assertLiveHandles');
const pendingCellMerges = Symbol('pendingCellMerges');
const writePendingCellMerges = Symbol('writePendingCellMerges');

class Worksheet {
  #cells = new Map();
  #cellMerges;
  #destroyedCellAddresses;
  #mergeDocument;
  #merges;
  #conditionalDocument;
  #conditionalProjection;
  #tableDocument;
  #tables;
  #linkedMergeStyles = new Map();
  #columns;
  #columnKeys = new Map();
  #rows = new Map();
  #destroyedRowNumbers;
  #createdCells = new Map();
  #storedRows;
  #check;
  #name;
  #prepareRename;
  #properties;
  #pageSetup;
  #headerFooter;
  #dataValidations;
  #explicitValidationRanges = new Set();
  #protectionEdited;
  #setState;
  #id;

  #workbook;

  constructor(workbook, name, id, check, prepareRename, setState, fresh) {
    this.#protectionEdited = fresh;
    this.#workbook = workbook;
    this.#name = name;
    this.#id = id;
    this.#check = check;
    this.#prepareRename = prepareRename;
    this.#setState = setState;
    const worksheet = this;
    let printDocument;
    let printSettings;
    function printSetting(document, group, read) {
      if (document !== printDocument) {
        printDocument = document;
        printSettings = new Map();
      }
      if (!printSettings.has(group)) printSettings.set(group, read());
      return printSettings.get(group);
    }
    const margins = {};
    for (const [index, key] of pageMarginNames.entries()) {
      Object.defineProperty(margins, key, {
        enumerable: true,
        get() {
          const { document, ignoredNodes } = worksheet.#check();
          if (ignoredNodes?.has('pageMargins')) return printMarginDefaults[key];
          return printSetting(document, 'margins', () => readMargins(document, worksheet.#name))[index];
        },
        set(value) { worksheet.#updateMargins({ [key]: value }); },
      });
    }
    this.#pageSetup = {
      get margins() { return margins; },
      set margins(value) { worksheet.#updateMargins(value); },
    };
    for (const [index, key] of printOptionNames.entries()) {
      Object.defineProperty(this.#pageSetup, key, {
        enumerable: true,
        get() {
          const { document, ignoredNodes } = worksheet.#check();
          if (ignoredNodes?.has('printOptions')) return false;
          const options = printSetting(document, 'printOptions', () =>
            rustCall('WORKSHEET_READ_REFUSED', () => document.print_options(worksheet.#name)));
          return Boolean(options[index]);
        },
        set(value) { worksheet.pageSetup = { [key]: value }; },
      });
    }
    for (const [index, key] of pageSettingNames.entries()) {
      Object.defineProperty(this.#pageSetup, key, {
        enumerable: true,
        get() {
          const { document, ignoredNodes } = worksheet.#check();
          if (key === 'fitToPage' && ignoredNodes?.has('sheetPr')) return pageSettingDefaults.fitToPage;
          if (ignoredNodes?.has('pageSetup') && key !== 'fitToPage') {
            return pageSettingDefaults[key];
          }
          const settings = printSetting(document, 'pageSetup', () =>
            rustCall('WORKSHEET_READ_REFUSED', () => document.page_setup(worksheet.#name)));
          const value = settings[index];
          if (key === 'orientation') return value === 1 ? 'landscape' : 'portrait';
          if (key === 'fitToPage') return Boolean(value);
          return Number.isNaN(value) ? undefined : value;
        },
        set(value) { worksheet.pageSetup = { [key]: value }; },
      });
    }
    for (const [key, fallback] of Object.entries(printerSettingDefaults)) {
      Object.defineProperty(this.#pageSetup, key, {
        enumerable: true,
        get() {
          const { document, ignoredNodes } = worksheet.#check();
          if (ignoredNodes?.has('pageSetup')) return fallback;
          const settings = printSetting(document, 'printerSettings', () => {
            const fields = rustCall('WORKSHEET_READ_REFUSED', () => document.printer_settings(worksheet.#name));
            const values = new Map();
            for (let index = 0; index < fields.length; index += 2) {
              values.set(fields[index], fields[index + 1]);
            }
            return values;
          });
          const value = settings.get(key);
          if (value === undefined) return fallback;
          if (typeof fallback === 'boolean') return value === '1' || value === 'true';
          if (typeof fallback === 'number' || key === 'firstPageNumber') return Number(value);
          return value === 'none' && key === 'cellComments' ? 'None' : value;
        },
        set(value) { worksheet.pageSetup = { [key]: value }; },
      });
    }
    for (const key of printReferenceNames) {
      Object.defineProperty(this.#pageSetup, key, {
        enumerable: true,
        get() {
          const { document } = worksheet.#check();
          const areas = key === 'printArea';
          const ranges = printSetting(document, areas ? 'printArea' : 'printTitles', () =>
            readPrintReferences(document, worksheet.#name, !areas));
          if (areas) return ranges.length ? ranges.join('&&') : undefined;
          return ranges.find((range) => key === 'printTitlesRow' ? /^\d+:\d+$/.test(range) : /^[A-Z]+:[A-Z]+$/i.test(range));
        },
        set(value) { worksheet.pageSetup = { [key]: value }; },
      });
    }
    const explicitValidationRanges = this.#explicitValidationRanges;
    this.#dataValidations = {
      get model() {
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('dataValidations')) return {};
        return rustCall('WORKSHEET_READ_REFUSED', () =>
          projectValidationModel(document, document.validation_rules(worksheet.#name), explicitValidationRanges));
      },
      set model(input) {
        const replacements = prepareValidationReplacement(input);
        const state = worksheet.#check();
        const name = worksheet.#name;
        const existing = rustCall('WORKSHEET_READ_REFUSED', () => state.document.validation_rules(name));
        if (existing.length || replacements.size) {
          worksheet.#writeDocument(state, candidate => rustCall('WORKSHEET_EDIT_REFUSED', () =>
            writeValidationReplacement(candidate, name, existing, replacements)));
        }
        explicitValidationRanges.clear();
        for (const reference of replacements.keys()) explicitValidationRanges.add(reference);
      },
      add(reference, rule) {
        const range = validationReference(reference);
        const validation = encodeValidation(rule);
        if (!validation) throw failure('INVALID_VALUE', 'Expected a data validation rule');
        const state = worksheet.#check();
        worksheet.#writeDocument(state, (candidate) =>
          rustCall('WORKSHEET_EDIT_REFUSED', () => writeValidation(candidate, worksheet.#name, range, validation)));
        explicitValidationRanges.add(range);
        return rule;
      },
      find(reference) {
        const range = validationReference(reference);
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('dataValidations')) return undefined;
        return rustCall('WORKSHEET_READ_REFUSED', () =>
          decodeValidation(document, document.range_validation(worksheet.#name, range)));
      },
      remove(reference) {
        const range = validationReference(reference);
        const state = worksheet.#check();
        worksheet.#writeDocument(state, (candidate) =>
          rustCall('WORKSHEET_EDIT_REFUSED', () => writeValidation(candidate, worksheet.#name, range)));
        explicitValidationRanges.delete(range);
      },
    };
    this.#headerFooter = {};
    for (const [index, key] of headerFooterNames.entries()) {
      Object.defineProperty(this.#headerFooter, key, {
        enumerable: true,
        get() {
          const { document, ignoredNodes } = worksheet.#check();
          if (ignoredNodes?.has('headerFooter')) return index < 6 ? null : false;
          const values = rustCall('WORKSHEET_READ_REFUSED', () => document.header_footer(worksheet.#name));
          return index < 6 ? values[index] || null : values[index] === 'true';
        },
        set(value) { worksheet.headerFooter = { [key]: value }; },
      });
    }
    const outlineProperties = {};
    for (const [index, key] of outlineSummaryKeys.entries()) {
      Object.defineProperty(outlineProperties, key, {
        enumerable: true,
        get() {
          const { document, ignoredNodes } = worksheet.#check();
          if (ignoredNodes?.has('sheetPr')) return undefined;
          const flags = rustCall('WORKSHEET_READ_REFUSED', () => document.outline_summary(worksheet.#name));
          return flags[index] === 2 ? undefined : Boolean(flags[index]);
        },
        set(value) { worksheet.properties.outlineProperties = { [key]: value }; },
      });
    }
    this.#properties = {
      get outlineProperties() {
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('sheetPr')) return undefined;
        const flags = rustCall('WORKSHEET_READ_REFUSED', () => document.outline_summary(worksheet.#name));
        return flags.every(flag => flag === 2) ? undefined : outlineProperties;
      },
      set outlineProperties(value) {
        const summary = parseOutlineSummary(value);
        const state = worksheet.#check();
        worksheet.#writeDocument(state, (candidate) =>
          rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.set_outline_summary(worksheet.#name, ...summary)));
      },
      get dyDescent() {
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('sheetFormatPr')) return 55;
        return rustCall('WORKSHEET_READ_REFUSED', () => document.default_row_descent(worksheet.#name)) ?? 55;
      },
      set dyDescent(value) {
        const descent = checkedRowDescent(value);
        const state = worksheet.#check();
        worksheet.#writeDocument(state, (candidate) =>
          rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.set_default_row_descent(worksheet.#name, descent)));
      },
      get defaultRowHeight() { return worksheet.#readDefaultDimension(true); },
      set defaultRowHeight(value) { worksheet.#writeDefaultDimension(true, value); },
      get defaultColWidth() { return worksheet.#readDefaultDimension(false); },
      set defaultColWidth(value) { worksheet.#writeDefaultDimension(false, value); },
      get tabColor() {
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('sheetPr')) return undefined;
        return readTabColor(document, worksheet.#name);
      },
      set tabColor(value) {
        const attributes = tabColorAttributes(value);
        const state = worksheet.#check();
        worksheet.#writeDocument(state, (candidate) =>
          rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.set_tab_color(worksheet.#name, attributes)));
      },
      get outlineLevelRow() { return worksheet.#readOutlineLevels()[0]; },
      set outlineLevelRow(value) { worksheet.#writeOutlineLevel(true, value); },
      get outlineLevelCol() { return worksheet.#readOutlineLevels()[1]; },
      set outlineLevelCol(value) { worksheet.#writeOutlineLevel(false, value); },
    };
  }

  destroy() {
    this.#check();
    this.#workbook.removeWorksheetEx(this);
  }

  get tabColor() { return this.properties.tabColor; }

  set tabColor(value) { this.properties.tabColor = value; }

  get workbook() {
    this.#check();
    return this.#workbook;
  }

  get name() {
    return this.#name;
  }

  set name(value) {
    const state = this.#check();
    const rename = this.#prepareRename(value);
    if (!rename) return;
    this.#writeDocument(state, candidate => this.#writeRename(candidate, rename.name));
    this.#publishRename(rename);
  }

  #writeRename(candidate, name) {
    rustCall('WORKSHEET_RENAME_REFUSED', () => candidate.rename_worksheet(this.#name, name));
  }

  #publishRename(rename) {
    this.#name = rename.name;
    rename.publish();
  }

  [translateStreamingFormula](formula, from, to) {
    const { document } = this.#check();
    return rustCall('CELL_READ_REFUSED', () => document.translate_streaming_formula(this.#name, formula, from, to));
  }

  [validateStreamingRowStyle](style) {
    encodeDefaultStyle(style ?? undefined);
  }

  [prepareStreamingCellModel](model) {
    const { value } = prepareCellModel(model);
    let streamingValue = value;
    if (value?.richText) streamingValue = { richText: decodeRichTextRuns(value.richText) };
    else if (value?.shareType) streamingValue = { formula: value.formula, result: value.result,
      shareType: value.shareType, ref: model.ref };
    return { value: streamingValue, style: model.style, note: model.comment?.note };
  }

  [applyStreamingNotes](row, updates) {
    const notes = updates.map(([column, value]) => [column, prepareCellNote(value)]);
    const state = this.#check();
    this.#writeDocument(state, candidate => {
      for (const [column, note] of notes) {
        rustCall('CELL_EDIT_REFUSED', () => writeCellNote(candidate, this.#name, row - 1, column - 1, note));
      }
    });
  }

  get rowBreaks() {
    const { document, ignoredNodes } = this.#check();
    if (ignoredNodes?.has('rowBreaks')) return [];
    const values = rustCall('WORKSHEET_READ_REFUSED', () => document.row_page_breaks(this.#name));
    const breaks = [];
    for (let index = 0; index < values.length; index += 4) {
      const [id, min, max, man] = values.subarray(index, index + 4);
      breaks.push(min === 0 ? { id, max, man } : { id, max, man, min });
    }
    return breaks;
  }

  get sheetProtection() {
    const { document, ignoredNodes } = this.#check();
    if (!this.#protectionEdited && ignoredNodes?.has('sheetProtection')) return undefined;
    const attributes = rustCall('WORKSHEET_READ_REFUSED', () => document.sheet_protection_attributes(this.#name));
    if (!attributes) return this.#protectionEdited ? null : undefined;
    const values = new Map();
    for (let index = 0; index < attributes.length; index += 2) values.set(attributes[index], attributes[index + 1]);
    const model = { sheet: values.get('sheet') === '1' ? true : undefined };
    protectionPermissions.forEach(key => {
      model[key] = sheetProtectionDefaults[key] ? (values.get(key) === '1' ? false : undefined)
        : (values.get(key) === '0' ? true : undefined);
    });
    if (values.get('algorithmName')) {
      for (const key of ['algorithmName', 'hashValue', 'saltValue']) model[key] = values.get(key);
      model.spinCount = Number.parseInt(values.get('spinCount'), 10);
    }
    return model;
  }

  async protect(password, options = {}) {
    if (password != null && typeof password !== 'string') {
      throw failure('INVALID_VALUE', 'Expected a worksheet password string');
    }
    const settings = sheetProtectionSettings(options);
    const permissions = Uint8Array.from(protectionPermissions, key => Number(settings[key]));
    const requested = settings.spinCount;
    const spinCount = Number.isFinite(requested) ? Math.round(Math.max(0, requested)) : 100000;
    if (password && spinCount > 10_000_000) {
      throw failure('INVALID_VALUE', 'Worksheet protection spin count exceeds 10000000');
    }
    const state = this.#check();
    this.#writeDocument(state, (candidate) =>
      rustCall('WORKSHEET_EDIT_REFUSED', () =>
        candidate.protect_sheet(this.#name, password ?? '', password ? spinCount : 0, permissions)));
    this.#protectionEdited = true;
  }

  unprotect() {
    const state = this.#check();
    this.#writeDocument(state, (candidate) =>
      rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.unprotect_sheet(this.#name)));
    this.#protectionEdited = true;
  }

  get dataValidations() {
    this.#check();
    return this.#dataValidations;
  }

  get headerFooter() {
    this.#check();
    return this.#headerFooter;
  }

  set headerFooter(value) {
    const settings = headerFooterSettings(value);
    const state = this.#check();
    this.#writeDocument(state, (candidate) => writeHeaderFooter(candidate, this.#name, settings));
  }

  get pageSetup() {
    this.#check();
    return this.#pageSetup;
  }

  set pageSetup(value) {
    const settings = parsePageSetup(value);
    const state = this.#check();
    this.#writeDocument(state, (candidate) => writePageSetup(candidate, this.#name, settings));
  }

  #updateMargins(value) {
    const state = this.#check();
    this.#writeDocument(state, (candidate) => writeMargins(candidate, this.#name, value));
  }

  get properties() {
    this.#check();
    return this.#properties;
  }

  #readDefaultDimension(rows) {
    const { document, ignoredNodes } = this.#check();
    if (ignoredNodes?.has('sheetFormatPr')) return rows ? 15 : undefined;
    const values = rustCall('WORKSHEET_READ_REFUSED', () => document.default_dimensions(this.#name));
    const value = values[rows ? 0 : 1];
    return Number.isNaN(value) ? undefined : value;
  }

  #writeDefaultDimension(rows, value) {
    const dimension = checkedDefaultDimension(rows, value);
    const state = this.#check();
    this.#writeDocument(state, (candidate) =>
      rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.set_default_dimension(this.#name, rows, dimension)));
  }

  #readOutlineLevels() {
    const { document, ignoredNodes } = this.#check();
    if (ignoredNodes?.has('sheetFormatPr')) return [0, 0];
    return rustCall('WORKSHEET_READ_REFUSED', () => document.outline_levels(this.#name));
  }

  #writeOutlineLevel(rows, value) {
    const state = this.#check();
    const level = checkedOutline(value);
    const previous = rustCall('WORKSHEET_READ_REFUSED', () => state.document.outline_levels(this.#name));
    if (previous[rows ? 0 : 1] === level) return;
    this.#writeDocument(state, (candidate) =>
      rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.set_outline_level(this.#name, rows, level)));
  }

  #mergedRanges(includeIgnored = false) {
    const { document, ignoredNodes } = this.#check();
    if (!includeIgnored && ignoredNodes?.has('mergeCells')) return [];
    if (this.#mergeDocument !== document) {
      this.#merges = rustCall('CELL_READ_REFUSED', () => document.merged_ranges(this.#name))
        .map((ref) => ({ ref, ...decode_range(ref) }));
      this.#mergeDocument = document;
    }
    return this.#merges;
  }

  #mergeAt(row, column, includeIgnored = false) {
    return this.#mergedRanges(includeIgnored).find(({ s, e }) =>
      row >= s.r && row <= e.r && column >= s.c && column <= e.c);
  }

  get hasMerges() { return this.#mergedRanges().length > 0; }

  mergeCells(...cells) {
    this.#mergeCells(cells, true);
  }

  mergeCellsWithoutStyle(...cells) {
    this.#mergeCells(cells, false);
  }

  #mergeCells(cells, copyStyle) {
    const state = this.#check();
    const range = mergeReference(cells);
    this.#writeDocument(state, (candidate) =>
      rustCall('MERGE_EDIT_REFUSED', () => candidate.merge_clearing(this.#name, range, copyStyle)));
    if (copyStyle) this.#linkedMergeStyles.set(range, new Set());
    this.#storedRows = undefined;
    this.#createdCells.clear();
  }

  unMergeCells(...cells) {
    const state = this.#check();
    const reference = mergeReference(cells);
    const { s, e } = decode_range(reference);
    const first = rustCall('CELL_READ_REFUSED', () =>
      state.document.first_merged_overlap(this.#name, reference, s.r, s.c));
    if (first === undefined) return;
    this.#writeDocument(state, (candidate) =>
      rustCall('MERGE_EDIT_REFUSED', () => candidate.unmerge_intersecting(this.#name, reference)));
    for (const key of this.#linkedMergeStyles.keys()) {
      const range = decode_range(key);
      if (range.s.r <= e.r && range.e.r >= s.r && range.s.c <= e.c && range.e.c >= s.c) {
        this.#linkedMergeStyles.delete(key);
      }
    }
  }

  #styleInsertedMergeCells(candidate, axis, start, count, s, e, detached) {
    // The first surviving linked member supplies the format for the new band.
    for (let row = s.r; row <= e.r; row++) {
      if (axis === 'r' && row === start) row += count;
      for (let col = s.c; col <= e.c; col++) {
        if (axis === 'c' && col === start) col += count;
        if (detached.has(gridAddress(row + 1, col + 1))) continue;
        const first = { ...s, [axis]: start };
        const last = { ...e, [axis]: start + count - 1 };
        const band = gridAddress(first.r + 1, first.c + 1) + ':' +
          gridAddress(last.r + 1, last.c + 1);
        rustCall('CELL_EDIT_REFUSED', () =>
          candidate.sync_merge_style(this.#name, band, row, col, []));
        return;
      }
    }
  }

  #shiftedLinkedMergeStyles(candidate, axis, start, delta) {
    if (!delta || !this.#linkedMergeStyles.size) return this.#linkedMergeStyles;
    const end = start + Math.max(0, -delta);
    const shifted = new Set(rustCall('CELL_READ_REFUSED', () => candidate.merged_ranges(this.#name)));
    const links = new Map();
    for (const [ref, detached] of this.#linkedMergeStyles) {
      const { s, e } = decode_range(ref);
      if (e[axis] < start) {
        if (shifted.has(ref)) links.set(ref, detached);
        continue;
      }
      if (delta < 0 && s[axis] >= start && e[axis] < end) continue;
      if (s[axis] >= end) s[axis] += delta;
      else if (s[axis] >= start) s[axis] = start;
      e[axis] = e[axis] >= end ? e[axis] + delta : start - 1;
      const nextRef = gridAddress(s.r + 1, s.c + 1) + ':' + gridAddress(e.r + 1, e.c + 1);
      if (!shifted.has(nextRef)) continue;
      const nextDetached = new Set();
      for (const key of detached) {
        const [, row, col] = address(key);
        const at = { r: row, c: col };
        if (at[axis] >= start && at[axis] < end) continue;
        if (at[axis] >= end) at[axis] += delta;
        nextDetached.add(gridAddress(at.r + 1, at.c + 1));
      }
      if (delta > 0 && s[axis] < start) {
        this.#styleInsertedMergeCells(candidate, axis, start, delta, s, e, nextDetached);
      }
      links.set(nextRef, nextDetached);
    }
    return links;
  }

  get views() {
    const { document, ignoredNodes } = this.#check();
    if (ignoredNodes?.has('sheetViews')) return [];
    return readViews(document, this.#name);
  }

  set views(value) {
    const state = this.#check();
    const views = parseViews(value);
    this.#writeDocument(state, (candidate) => writeViews(candidate, this.#name, views));
  }

  addConditionalFormatting(options) {
    const state = this.#check();
    const { headers, entries } = prepareConditionalFormatting(options);
    this.#writeDocument(state, (candidate) =>
      rustCall('CONDITIONAL_FORMAT_EDIT_REFUSED', () =>
        candidate.add_conditional_rules(this.#name, headers, entries)));
  }

  addBackgroundImage(imageId) {
    this.#addImage({ type: 'background', imageId });
  }

  getBackgroundImageId() {
    const state = this.#check();
    return workbookImages(this.#workbook, state).backgroundIds.get(this);
  }

  addImage(imageId, range) {
    this.#addImage({ type: 'image', imageId, range });
  }

  #addImage(input) {
    const state = this.#check();
    const image = this.#prepareImage(input);
    let placement;
    this.#writeDocument(state, candidate => { placement = this.#writeImage(candidate, image); });
    this.#publishImage(image, placement);
  }

  #prepareImage({ type, imageId, range, hyperlinks: modelLink }) {
    const image = this.#workbook[imageForPlacement](imageId);
    if (!image) throw failure('INVALID_VALUE', 'Unknown workbook image ID');
    if (type === 'background') return { type, imageId, image };
    if (typeof range === 'string') {
      const [first, last = first, extra] = range.split(':');
      if (extra !== undefined) throw failure('INVALID_ADDRESS', 'Expected one image range');
      const [, top, left] = address(first);
      const [, bottom, right] = address(last);
      range = { tl: { col: left, row: top }, br: { col: right + 1, row: bottom + 1 } };
    }
    const { tl, br, ext, hyperlinks, editAs } = imageAnchorSettings(range);
    const linkInput = modelLink || hyperlinks;
    const link = linkInput === undefined ? undefined : imageHyperlinkSettings(linkInput);
    const position = encodeImageMarker(this, tl);
    if (br) position.push(...encodeImageMarker(this, br));
    else {
      const size = [ext.width, ext.height].map((value) => Math.round(value * 9525));
      if (!size.every((value) => Number.isSafeInteger(value) && value > 0)) {
        throw failure('INVALID_VALUE', 'Expected positive image dimensions');
      }
      position.push(...size);
    }
    return { type, imageId, image, position: Float64Array.from(position), editAs, link };
  }

  #writeImage(candidate, { type, image, position, editAs, link }) {
    return rustCall('IMAGE_EDIT_REFUSED', () => type === 'background'
      ? [candidate.set_worksheet_background_image(this.#name, image.buffer)]
      : candidate.insert_worksheet_image(this.#name, image.buffer, position, editAs, link?.hyperlink, link?.tooltip));
  }

  #publishImage({ type, imageId, image }, [part, shape]) {
    const registry = imageRegistry(this.#workbook);
    if (!registry.ids.has(part)) registry.ids.set(part, imageId);
    if (type === 'background') registry.backgrounds.set(this, imageId);
    else {
      if (!registry.placements.has(this)) registry.placements.set(this, new Map());
      registry.placements.get(this).set(shape, imageId);
    }
    if (image.filename !== undefined) registry.fileParts.set(part, image);
  }

  getImages() {
    const state = this.#check();
    return workbookImages(this.#workbook, state).sheets.get(this.#name).map(({ imageId, name, altText, hyperlink, tooltip, editAs, position }) => {
      const range = { tl: excelImageAnchor(this, position), editAs };
      if (hyperlink) range.hyperlinks = tooltip ? { hyperlink, tooltip } : { hyperlink };
      if (position.length === 8) range.br = excelImageAnchor(this, position.slice(4));
      else range.ext = { width: position[4] / 9525, height: position[5] / 9525 };
      return { type: 'image', imageId, name, altText, range };
    });
  }

  addTable(options) {
    const state = this.#check();
    const { name, displayName, ref, headerRow, totalsRow, style, columns, rows } = tableSettings(options);
    if (rows.length === 0 && !headerRow) {
      throw failure('INVALID_VALUE', 'A table without headers requires at least one data row');
    }
    const tableStyle = tableStyleSettings(style);
    const { theme } = tableStyle;
    const flags = tableStyleFlags.map((key) => tableStyle[key]);
    const definitions = Array.from(columns, (column, index) =>
      encodeTableColumn(column, totalsRow, index === 0 ? 'Total' : undefined));
    const data = Array.from(rows, (row) => {
      if (checkedTableRow(row).length > columns.length) throw failure('INVALID_VALUE', 'Invalid table row');
      return Array.from({ length: columns.length }, (_, index) => excelValue(row[index] ?? null));
    });
    const [first, row, col] = address(ref);
    const end = gridAddress(row + data.length + Number(headerRow) + Number(totalsRow), col + columns.length);
    const reference = first + ':' + end;
    const settings = [name, displayName ?? '', String(headerRow), String(totalsRow), theme ?? '', ...flags.map(String)];
    this.#writeDocument(state, (candidate) => {
      rustCall('TABLE_EDIT_REFUSED', () => {
        writeTableInitialValues(candidate, this.#name, row + Number(headerRow), col, columns.length, data);
        candidate.add_table(this.#name, reference, settings, definitions.flatMap(({ settings }) => settings));
        const styles = definitions.flatMap(({ style }, position) =>
          style?.fields.length ? [[position, style.fields]] : []);
        writeTableColumnStyles(candidate, this.#name, name, styles);
        const totals = definitions.flatMap(({ settings, result }, position) =>
          result === undefined ? [] : [[position, settings[2], settings[3], result]]);
        writeTableTotals(candidate, this.#name, name, totals);
      });
    });
    this.#storedRows = undefined;
    const definition = this.#tableDefinitions(state.document).find(({ model }) => model.name === name);
    return this.#tableHandle(state, definition, definitions.map(({ style }) => style?.model));
  }

  #tableDefinitions(document) {
    if (this.#tableDocument === document) return this.#tables;
    const fields = rustCall('WORKSHEET_READ_REFUSED', () => document.table_nodes(this.#name));
    const tables = readMetadataNodes(fields).map((definition) => ({
      id: definition.attributes.id,
      model: readTable(definition),
    }));
    const targets = tables.flatMap(({ model }) => [model, ...model.columns]);
    const ids = new Set(targets.flatMap(model =>
      tableDxfKeys.map(key => model[key]).filter(id => Number.isInteger(id) && id >= 0 && id <= 0xffffffff)));
    if (ids.size) {
      const styles = readDifferentialStyles(document, ids);
      for (const target of targets) {
        const entries = tableDxfKeys.flatMap(key => styles.has(target[key])
          ? [[key, structuredClone(styles.get(target[key]))]] : []);
        if (entries.length) target.differentialStyles = Object.fromEntries(entries);
      }
    }
    this.#tableDocument = document;
    this.#tables = tables;
    return tables;
  }

  #tableHandle(state, { id, model }, initialStyles) {
    const columns = [];
    const table = { ...model, columns, getColumn(index) { return columns[index]; } };
    const pending = new Map();
    const pendingColumns = new Map();
    const pendingColumnStyles = new Map();
    const columnStyles = new Map(initialStyles?.flatMap((style, position) =>
      style === undefined ? [] : [[tableColumnIds.get(model.columns[position]), style]]));
    const retiredColumns = new WeakSet();
    let columnOrder;
    const pendingRows = [];
    const rowEdits = [];
    let rowDelta = 0;
    let tableEditAnchor;
    const keys = ['theme', ...tableStyleFlags];
    const current = () => {
      if (this.#check() !== state) throw failure('STALE_HANDLE', 'Table belongs to a previous workbook load');
      const existing = this.#tableDefinitions(state.document)
        .find((entry) => entry.id === id && entry.model.name === table.name);
      if (!existing) throw failure('STALE_HANDLE', 'Table was removed');
      return existing.model;
    };
    const dataRowCount = ({ ref, tableRef, headerRow, totalsRow }) => {
      const [, first] = address(ref);
      const [, last] = address(tableRef.split(':').at(-1));
      return last - first + 1 - Number(headerRow) - Number(totalsRow);
    };
    Object.defineProperty(table, 'tableRef', { enumerable: true, get: () => current().tableRef });
    table.addRow = (values, rowNumber) => {
      const prior = current();
      const width = columnOrder?.length ?? prior.columns.length;
      if (checkedTableRow(values).length > width) {
        throw failure('INVALID_VALUE', 'Invalid table row');
      }
      const committed = dataRowCount(prior) + rowDelta;
      const length = committed + pendingRows.length;
      const position = checkedTableRowIndex(rowNumber) ?? length;
      if (position > length) throw failure('INVALID_VALUE', 'Table row position is outside the data rows');
      const cells = Array.from({ length: width }, (_, index) => tableEditValue(values[index]));
      let row = cells;
      if (columnOrder) {
        const positions = new Map(prior.columns.map((column, index) => [tableColumnIds.get(column), index]));
        row = Array(prior.columns.length).fill(null);
        for (const [index, column] of columnOrder.entries()) {
          if (typeof column === 'object') column.cells.splice(position, 0, cells[index]);
          else row[positions.get(column)] = cells[index];
        }
      }
      if (rowNumber !== undefined) tableEditAnchor ??= prior.tableRef;
      if (position < committed) {
        rowEdits.push({ position, row });
        rowDelta += 1;
      } else {
        pendingRows.splice(position - committed, 0, row);
      }
    };
    table.removeRows = (rowNumber, count = 1) => {
      const prior = current();
      checkedTableRowIndex(rowNumber);
      checkedTableRowIndex(count);
      if (rowNumber === undefined) throw failure('INVALID_VALUE', 'Expected a nonnegative integer table row position');
      const committed = dataRowCount(prior) + rowDelta;
      const length = committed + pendingRows.length;
      const removed = Math.min(count, length - rowNumber);
      if (removed <= 0) return;
      if (removed === length && !prior.headerRow) {
        throw failure('INVALID_VALUE', 'A table without headers requires at least one data row');
      }
      for (const column of columnOrder ?? []) {
        if (typeof column === 'object') column.cells.splice(rowNumber, removed);
      }
      const committedRemoval = Math.max(0, Math.min(removed, committed - rowNumber));
      pendingRows.splice(Math.max(0, rowNumber - committed), removed - committedRemoval);
      if (committedRemoval) {
        tableEditAnchor ??= prior.tableRef;
        rowDelta -= committedRemoval;
        const previous = rowEdits.at(-1);
        if (previous?.removals) previous.removals.push(rowNumber, committedRemoval);
        else rowEdits.push({ removals: [rowNumber, committedRemoval] });
      }
      if (!pendingRows.length && !rowEdits.length && !columnOrder) tableEditAnchor = undefined;
    };
    table.addColumn = (column, values, columnNumber) => {
      const prior = current();
      const width = columnOrder?.length ?? prior.columns.length;
      const position = checkedTableColumnIndex(columnNumber) ?? width;
      if (position > width) throw failure('INVALID_VALUE', 'Table column position is outside the columns');
      const height = dataRowCount(prior) + rowDelta + pendingRows.length;
      if (checkedTableColumnValues(values).length > height) {
        throw failure('INVALID_VALUE', 'Column values exceed table data rows');
      }
      const definition = encodeTableColumn(column, prior.totalsRow);
      const cells = Array.from({ length: height }, (_, index) => tableEditValue(values[index]));
      tableEditAnchor ??= prior.tableRef;
      columnOrder ??= prior.columns.map((column) => tableColumnIds.get(column));
      columnOrder.splice(position, 0, { ...definition, cells });
    };
    table.removeColumns = (columnNumber, count = 1) => {
      const prior = current();
      checkedTableColumnIndex(count);
      if (checkedTableColumnIndex(columnNumber) === undefined) {
        throw failure('INVALID_VALUE', 'Expected a nonnegative integer table column position');
      }
      const length = columnOrder?.length ?? prior.columns.length;
      const removed = Math.min(count, length - columnNumber);
      if (removed <= 0) return;
      if (removed === length) throw failure('INVALID_VALUE', 'A table requires at least one column');
      tableEditAnchor ??= prior.tableRef;
      columnOrder ??= prior.columns.map((column) => tableColumnIds.get(column));
      columnOrder.splice(columnNumber, removed);
      if (columnOrder.length === prior.columns.length && columnOrder.every((id) => typeof id !== 'object')) {
        columnOrder = undefined;
        if (!pendingRows.length && !rowEdits.length) tableEditAnchor = undefined;
      }
    };
    for (const key of keys) {
      const validate = key === 'theme' ? checkedTableTheme : checkedTableStyleFlag;
      Object.defineProperty(table, key, {
        enumerable: true,
        get: () => pending.has(key) ? pending.get(key) : current()[key],
        set(value) {
          pending.set(key, validate(value));
        },
      });
    }
    const bindColumn = (model) => {
      const column = { ...model };
      const id = tableColumnIds.get(model);
      tableColumnIds.set(column, id);
      const currentColumn = () => {
        const model = tableColumnsById.get(current()).get(id);
        if (!model || retiredColumns.has(column)) throw failure('STALE_HANDLE', 'Table column was removed');
        return model;
      };
      for (const [key, validate] of [
        ['name', checkedTableColumnName], ['filterButton', checkedTableColumnFilterButton],
        ['totalsRowFunction', checkedTableTotalFunction], ['totalsRowLabel', checkedTableTotalText],
        ['totalsRowFormula', checkedTableTotalText], ['totalsRowResult', excelFormulaResult],
      ]) {
        Object.defineProperty(column, key, {
          enumerable: true,
          get: () => {
            const model = currentColumn();
            const edit = pendingColumns.get(id);
            if (edit && Object.hasOwn(edit, key)) return edit[key];
            if (key === 'totalsRowResult') {
              const table = current();
              if (!table.totalsRow) return undefined;
              const [, row] = address(table.tableRef.split(':').at(-1));
              const [, , firstColumn] = address(table.ref);
              return this.getCell(row + 1, firstColumn + table.columns.indexOf(model) + 1).result;
            }
            return model[key];
          },
          set(value) {
            currentColumn();
            const checked = validate(value);
            if (!pendingColumns.has(id)) pendingColumns.set(id, {});
            pendingColumns.get(id)[key] = checked;
          },
        });
      }
      Object.defineProperty(column, 'style', {
        enumerable: true,
        get() {
          currentColumn();
          return structuredClone(pendingColumnStyles.has(id) ? pendingColumnStyles.get(id).model : columnStyles.get(id));
        },
        set(value) {
          currentColumn();
          pendingColumnStyles.set(id, captureTableColumnStyle(value));
        },
      });
      return column;
    };
    columns.push(...model.columns.map(bindColumn));
    table.commit = () => {
      const prior = current();
      if (!pending.size && !pendingColumns.size && !pendingColumnStyles.size &&
          !pendingRows.length && !rowEdits.length && !columnOrder) return;
      if (tableEditAnchor !== undefined && tableEditAnchor !== prior.tableRef) {
        throw failure('STALE_HANDLE', 'The table range changed after an edit position was selected');
      }
      const retainedIds = columnOrder && new Set(columnOrder.filter((column) => typeof column !== 'object'));
      const theme = pending.has('theme') ? pending.get('theme') ?? '' : undefined;
      const flags = Uint8Array.from(tableStyleFlags, (key) => Number(pending.get(key) ?? prior[key]));
      const [, row, startColumn] = address(prior.ref);
      this.#writeDocument(state, (candidate) => rustCall('TABLE_EDIT_REFUSED', () => {
        if (pending.size) candidate.set_table_style(this.#name, table.name, theme, flags);
        const renamedColumns = [];
        const names = [];
        const filterColumns = [];
        const filterVisibility = [];
        const totals = [];
        const finalPositions = columnOrder && new Map(columnOrder.map((id, position) => [id, position]));
        const columnStyleEdits = [];
        for (const [index, model] of prior.columns.entries()) {
          const id = tableColumnIds.get(model);
          const edit = pendingColumns.get(id);
          if (retainedIds && !retainedIds.has(id)) continue;
          const style = pendingColumnStyles.get(id);
          if (style?.fields.length) columnStyleEdits.push([finalPositions?.get(id) ?? index, style.fields]);
          if (!edit) continue;
          const { name, filterButton } = edit;
          if (name !== undefined && name !== prior.columns[index].name) {
            if (!prior.headerRow) throw failure('UNSUPPORTED_OPTIONS', 'Column renaming requires a table header');
            renamedColumns.push(startColumn + index);
            names.push(name);
          }
          if (filterButton !== undefined && filterButton !== prior.columns[index].filterButton) {
            filterColumns.push(index);
            filterVisibility.push(Number(filterButton));
          }
          if (['totalsRowFunction', 'totalsRowLabel', 'totalsRowFormula', 'totalsRowResult'].some((key) => Object.hasOwn(edit, key))) {
            const options = {
              name: model.name, filterButton: model.filterButton, totalsRowFunction: model.totalsRowFunction,
              totalsRowLabel: model.totalsRowLabel, totalsRowFormula: model.totalsRowFormula, ...edit,
            };
            const { settings: [, , functionName, value], result } = encodeTableColumn(options, true);
            totals.push([finalPositions?.get(id) ?? index, functionName, value, result]);
          }
        }
        if (filterColumns.length) candidate.set_table_filter_buttons(this.#name, table.name,
          Uint32Array.from(filterColumns), Uint8Array.from(filterVisibility));
        if (names.length) candidate.write_table_headers(this.#name, row, Uint32Array.from(renamedColumns), names);
        for (const edit of rowEdits) {
          if (edit.removals) candidate.remove_table_rows(this.#name, table.name, Uint32Array.from(edit.removals));
          else writeTableRows(candidate, this.#name, table.name, prior.columns.length, [edit.row], edit.position);
        }
        if (pendingRows.length) writeTableRows(candidate, this.#name, table.name, prior.columns.length, pendingRows);
        if (columnOrder) {
          if (!retainedIds.size) {
            candidate.replace_table_columns(this.#name, table.name, columnOrder.flatMap(({ settings }) => settings),
              ...encodeTableCells(columnOrder.flatMap(({ cells }) => cells)));
          } else {
            const removals = [];
            for (let index = prior.columns.length - 1; index >= 0;) {
              if (retainedIds.has(tableColumnIds.get(prior.columns[index]))) {
                index -= 1;
                continue;
              }
              const end = index;
              while (index >= 0 && !retainedIds.has(tableColumnIds.get(prior.columns[index]))) index -= 1;
              removals.push(index + 1, end - index);
            }
            if (removals.length) candidate.remove_table_columns(this.#name, table.name, Uint32Array.from(removals));
            for (const [position, column] of columnOrder.entries()) {
              if (typeof column === 'object') {
                const [name, button] = column.settings;
                candidate.insert_table_column(this.#name, table.name, [String(position), name, button],
                  ...encodeTableCells(column.cells));
              }
            }
          }
          for (const [position, column] of columnOrder.entries()) {
            if (typeof column !== 'object') continue;
            if (column.style?.fields.length) columnStyleEdits.push([position, column.style.fields]);
            if (column.result !== undefined || (retainedIds.size && column.settings[2] !== 'none')) {
              totals.push([position, column.settings[2], column.settings[3], column.result]);
            }
          }
        }
        writeTableColumnStyles(candidate, this.#name, table.name, columnStyleEdits);
        writeTableTotals(candidate, this.#name, table.name, totals);
      }));
      if (pendingRows.length || rowEdits.length || columnOrder || pendingColumnStyles.size) this.#storedRows = undefined;
      if (columnOrder) {
        const byId = new Map();
        for (const column of columns) {
          const id = tableColumnIds.get(column);
          if (retainedIds.has(id)) byId.set(id, column);
          else {
            retiredColumns.add(column);
            columnStyles.delete(id);
          }
        }
        columns.splice(0, columns.length, ...current().columns.map((column) =>
          byId.get(tableColumnIds.get(column)) ?? bindColumn(column)));
        for (const [position, entry] of columnOrder.entries()) {
          if (typeof entry === 'object' && entry.style) {
            columnStyles.set(tableColumnIds.get(columns[position]), entry.style.model);
          }
        }
      }
      columnOrder = undefined;
      pendingRows.length = 0;
      rowEdits.length = 0;
      rowDelta = 0;
      tableEditAnchor = undefined;
      for (const [id, { model }] of pendingColumnStyles) {
        if (!retainedIds || retainedIds.has(id)) columnStyles.set(id, model);
      }
      pendingColumnStyles.clear();
      pending.clear();
      pendingColumns.clear();
    };
    return table;
  }

  getTables() {
    const state = this.#check();
    if (state.ignoredNodes?.has('tableParts')) return [];
    return this.#tableDefinitions(state.document).map((definition) => this.#tableHandle(state, definition));
  }

  getTable(name) {
    const state = this.#check();
    if (state.ignoredNodes?.has('tableParts')) return undefined;
    const definition = this.#tableDefinitions(state.document).find(({ model }) => model.name === name);
    return definition && this.#tableHandle(state, definition);
  }

  removeTable(name) {
    const state = this.#check();
    if (!this.getTable(name)) return;
    this.#writeDocument(state, (candidate) =>
      rustCall('TABLE_EDIT_REFUSED', () => candidate.remove_table(this.#name, name)));
  }

  get conditionalFormattings() {
    return this.#projectConditionalFormats().blocks;
  }

  #projectConditionalFormats() {
    const { document, ignoredNodes } = this.#check();
    if (this.#conditionalDocument === document) return structuredClone(this.#conditionalProjection);
    const fields = ignoredNodes?.has('conditionalFormatting') ? []
      : rustCall('WORKSHEET_READ_REFUSED', () => document.conditional_format_blocks(this.#name));
    const blocks = readConditionalFormats(fields);
    const standardCount = blocks.length;
    const extensionIds = ignoredNodes?.has('extLst') ? new Map() : mergeExtendedConditionalFormats(blocks,
      rustCall('WORKSHEET_READ_REFUSED', () => document.extended_conditional_format_blocks(this.#name)));
    const styledRules = blocks.flatMap(({ rules }) =>
      rules.filter(rule => Number.isInteger(rule.dxfId) && rule.dxfId >= 0));
    if (styledRules.length) {
      const byId = readDifferentialStyles(document, new Set(styledRules.map(rule => rule.dxfId >>> 0)));
      for (const rule of styledRules) {
        const style = byId.get(rule.dxfId >>> 0);
        if (style) rule.style = structuredClone(style);
      }
    }
    for (const { rules } of blocks) for (const rule of rules) delete rule.dxfId;
    this.#conditionalDocument = document;
    this.#conditionalProjection = { blocks, standardCount, extensionIds };
    return structuredClone(this.#conditionalProjection);
  }

  removeConditionalFormatting(filter) {
    const state = this.#check();
    const { blocks, standardCount, extensionIds } = this.#projectConditionalFormats();
    const source = blocks.slice();
    let removed;
    if (typeof filter === 'function') {
      const bytes = state.bytes;
      removed = [];
      blocks.filter((block, index, values) => {
        const keep = filter(block, index, values);
        if (!keep) removed.push(index);
        return keep;
      });
      if (this.#check() !== state || state.bytes !== bytes) {
        throw failure('CONDITIONAL_FORMAT_EDIT_REFUSED', 'Worksheet changed during conditional formatting selection');
      }
    } else if (typeof filter === 'number') {
      const index = Math.trunc(filter) || 0;
      const ordinal = index < 0 ? Math.max(0, blocks.length + index) : index;
      removed = ordinal < blocks.length ? [ordinal] : [];
    } else {
      removed = blocks.map((_, index) => index);
    }
    if (!removed.length) return;
    const standard = removed.filter(index => index < standardCount);
    const extended = removed.flatMap(index => extensionIds.get(source[index]) ?? []);
    this.#writeDocument(state, (candidate) => rustCall('CONDITIONAL_FORMAT_EDIT_REFUSED', () =>
      removeConditionalFormats(candidate, this.#name, standard, extended)));
  }

  get autoFilter() {
    const { document, ignoredNodes } = this.#check();
    if (ignoredNodes?.has('autoFilter')) return undefined;
    return rustCall('WORKSHEET_READ_REFUSED', () => document.autofilter(this.#name));
  }

  set autoFilter(value) {
    const state = this.#check();
    const range = filterRange(value);
    if (range === rustCall('WORKSHEET_READ_REFUSED', () => state.document.autofilter(this.#name))) return;
    this.#writeDocument(state, (candidate) =>
      rustCall('AUTOFILTER_EDIT_REFUSED', () => candidate.set_autofilter(this.#name, range)));
  }

  get state() {
    const { document } = this.#check();
    return rustCall('WORKSHEET_READ_REFUSED', () => document.sheet_state(this.#name));
  }

  set state(value) {
    this.#setState(value);
  }

  get id() {
    return this.#id;
  }

  [csvScalarCells]() {
    const { document, ignoredNodes } = this.#check();
    try {
      return document.csv_scalar_cells(this.#name, ignoredNodes?.has('hyperlinks') ?? false,
        ignoredNodes?.has('mergeCells') ?? false);
    } catch {
      // The document cell readers report errors with the existing adapter contract.
      return [];
    }
  }

  #stored() {
    const { document } = this.#check();
    if (!this.#storedRows) {
      const stored = rustCall('CELL_READ_REFUSED', () => {
        const entries = document.worksheet_presence(this.#name);
        const firstCell = entries[0] + 1;
        const rows = new Map();
        for (let index = 1; index < firstCell; index += 1) rows.set(entries[index], new Map());
        for (let index = firstCell; index < entries.length; index += 2) {
          const flags = entries[index + 1];
          rows.get(entries[index]).set(flags >>> 1, Boolean(flags & 1));
        }
        return rows;
      });
      for (const [row, cells] of this.#createdCells) {
        for (const [col, present] of cells) {
          if (present || !stored.get(row)?.has(col)) this.#record(stored, row, col, present);
        }
      }
      for (const key of this.#cells.keys()) {
        const [, row, col] = address(key);
        if (!stored.get(row + 1)?.has(col + 1)) this.#record(stored, row + 1, col + 1, false);
      }
      this.#storedRows = stored;
      this.#createdCells.clear();
    }
    return this.#storedRows;
  }

  #record(rows, row, col, present) {
    if (!rows.has(row)) rows.set(row, new Map());
    rows.get(row).set(col, present);
  }

  #rowNumbers() {
    const rows = new Set([...this.#stored().keys(), ...this.#rows.keys()]);
    for (const { s, e } of this.#mergedRanges()) {
      for (let row = s.r + 1; row <= e.r + 1; row += 1) rows.add(row);
    }
    return [...rows].sort((a, b) => a - b);
  }

  #rowHasValues(number) {
    for (const present of this.#stored().get(number)?.values() ?? []) {
      if (present) return true;
    }
    const row = number - 1;
    return this.#mergedRanges().some(({ s, e }) =>
      row >= s.r && row <= e.r && (row > s.r || e.c > s.c));
  }

  #rowColumns(number) {
    const cells = new Map(this.#stored().get(number));
    const row = number - 1;
    for (const { s, e } of this.#mergedRanges()) {
      if (row < s.r || row > e.r) continue;
      for (let column = s.c; column <= e.c; column += 1) {
        if (row !== s.r || column !== s.c) cells.set(column + 1, true);
      }
    }
    return [...cells].sort(([a], [b]) => a - b);
  }

  getRow(number) {
    this.#check();
    gridAddress(number, 1);
    if (!this.#rows.has(number)) {
      const layout = {
        replaceModel: (input) => this.#replaceRowModel(number, input),
        read: () => {
          const { document } = this.#check();
          return rustCall('ROW_READ_REFUSED', () => document.row_layout(this.#name, number - 1));
        },
        readBorder: () => {
          const { document } = this.#check();
          return decodeBorder(rustCall('ROW_READ_REFUSED', () => document.row_border(this.#name, number - 1)));
        },
        readFill: () => {
          const { document } = this.#check();
          return decodeFill(rustCall('ROW_READ_REFUSED', () => document.row_fill(this.#name, number - 1)));
        },
        readProtection: () => {
          const { document } = this.#check();
          return decodeProtection(rustCall('ROW_READ_REFUSED', () => document.row_protection(this.#name, number - 1)));
        },
        readAlignment: () => {
          const { document } = this.#check();
          return decodeAlignment(rustCall('ROW_READ_REFUSED', () => document.row_alignment(this.#name, number - 1)));
        },
        readFont: () => {
          const { document } = this.#check();
          return decodeDeclaredFont(rustCall('ROW_READ_REFUSED', () => document.row_font(this.#name, number - 1)));
        },
        readNumberFormat: () => {
          const { document } = this.#check();
          return rustCall('ROW_READ_REFUSED', () => document.row_number_format(this.#name, number - 1));
        },
        clearStyle: () => {
          const state = this.#check();
          if (rustCall('ROW_READ_REFUSED', () => state.document.row_style_index(this.#name, number - 1)) === undefined) return;
          this.#writeDocument(state, (candidate) =>
            rustCall('ROW_EDIT_REFUSED', () => candidate.set_row_style_index(this.#name, number - 1, undefined)));
        },
        writeStyle: (edits) => {
          const state = this.#check();
          const columns = [...(this.#stored().get(number)?.keys() ?? [])];
          this.#writeDocument(state, (candidate) => {
            for (const [property, ...args] of edits) {
              rustCall('ROW_EDIT_REFUSED', () => candidate['set_row_' + property](this.#name, number - 1, ...args));
              for (const column of columns) {
                const method = property === 'number_format' ? 'set_number_format' : 'set_cell_' + property;
                rustCall('CELL_EDIT_REFUSED', () => candidate[method](this.#name, number - 1, column - 1, ...args));
              }
            }
          });
        },
        addPageBreak: (left, right) => {
          const state = this.#check();
          if ([left, right].some((column) => column !== undefined &&
              (!Number.isInteger(column) || column < 0 || column > 16384))) {
            throw failure('INVALID_VALUE', 'Expected valid page break columns');
          }
          const first = Math.max(0, (left ?? 1) - 1);
          const last = Math.max(0, (right ?? 1) - 1) || 16838;
          this.#writeDocument(state, (candidate) =>
            rustCall('ROW_EDIT_REFUSED', () => candidate.add_row_page_break(this.#name, number, first, last)));
        },
        write: (height, hidden, outline) => {
          const state = this.#check();
          if ((height !== undefined && typeof height !== 'number') ||
              (hidden !== undefined && typeof hidden !== 'boolean')) {
            throw failure('INVALID_VALUE', 'Expected a numeric height or boolean visibility');
          }
          this.#writeDocument(state, (candidate) =>
            rustCall('ROW_EDIT_REFUSED', () => candidate.set_row_layout(this.#name, number - 1, height, hidden, outline)));
        },
      };
      this.#rows.set(number, new Row(this, number, () =>
        this.#rowColumns(number), () => this.#rowHasValues(number), layout,
      (entries) => this.#replaceRowValues(number, entries),
      (start, count, values) => this.#spliceRowCells(number, start, count, values)));
    }
    return this.#rows.get(number);
  }

  findRow(number) {
    this.#check();
    return this.#rows.get(number) ?? (
      this.#stored().has(number) || this.#mergedRanges().some(({ s, e }) => number > s.r && number <= e.r + 1)
        ? this.getRow(number) : undefined
    );
  }

  get model() {
    const { document, ignoredNodes } = this.#check();
    const rows = [];
    let left = 0;
    let right = 0;
    for (const number of this.#rowNumbers()) {
      const row = this.getRow(number).model;
      if (!row) continue;
      rows.push(row);
      if (row.min && (!left || row.min < left)) left = row.min;
      right = Math.max(right, row.max);
    }
    const bounds = { top: rows[0]?.number ?? 0, left, bottom: rows.at(-1)?.number ?? 0, right };
    const dimensions = {
      ...worksheetDimensions(bounds.top || 1, left || 1, bounds.bottom || 1, right || 1),
      model: bounds,
    };
    const media = this.getImages().map(({ type, imageId, range }) => ({
      type, imageId, hyperlinks: range.hyperlinks,
      range: { tl: modelImageAnchor(range.tl), br: range.br && modelImageAnchor(range.br), ext: range.ext, editAs: range.editAs },
    }));
    const background = this.getBackgroundImageId();
    if (background !== undefined) media.unshift({ type: 'background', imageId: background });
    const { margins, ...pageSetup } = this.pageSetup;
    const { outlineProperties, dyDescent, defaultRowHeight, defaultColWidth, outlineLevelRow, outlineLevelCol } = this.properties;
    const tabColor = ignoredNodes?.has('sheetPr') ? undefined : readTabColor(document, this.#name, true);
    return {
      id: this.id, name: this.name, dataValidations: this.dataValidations.model,
      properties: {
        dyDescent, defaultRowHeight, defaultColWidth, tabColor, outlineLevelRow, outlineLevelCol,
        outlineProperties: outlineProperties && { ...outlineProperties },
      },
      state: this.state, pageSetup: { margins: { ...margins }, ...pageSetup },
      headerFooter: { ...this.headerFooter }, rowBreaks: this.rowBreaks, views: this.views,
      autoFilter: this.autoFilter, media, sheetProtection: this.sheetProtection,
      tables: ignoredNodes?.has('tableParts') ? [] :
        this.#tableDefinitions(document).map(({ model }) => structuredClone(model)),
      conditionalFormattings: this.conditionalFormattings,
      cols: this.#modelColumns(document, ignoredNodes), rows, dimensions, merges: this.#mergedRanges().map(({ ref }) => ref),
    };
  }

  #modelColumns(document, ignoredNodes) {
    if (ignoredNodes?.has('cols')) return undefined;
    const fields = rustCall('COLUMN_READ_REFUSED', () => document.sheet_layout(this.#name));
    const ranges = [];
    for (let index = 0; index < fields.length; index += 6) {
      const [kind, first, last] = fields.slice(index, index + 3);
      if (kind !== 'column') continue;
      const min = Number(first);
      const max = Number(last);
      gridAddress(1, min);
      gridAddress(1, max);
      const column = this.#columns?.[min - 1] ?? this.#columnHandle(min);
      if (column.isDefault) continue;
      const previous = ranges.at(-1);
      if (previous?.max === min - 1 && column.equivalentTo({ ...previous, ...previous.style })) {
        previous.max = max;
        continue;
      }
      const { width, hidden, outlineLevel, style } = column.defn;
      ranges.push({ min, max, width: width ?? 9, style, isCustomWidth: column.isCustomWidth,
        hidden, outlineLevel, collapsed: column.collapsed });
    }
    return ranges.length ? ranges : undefined;
  }

  set model(input) {
    const {
      id, name, rows, pageSetup, headerFooter, views, autoFilter, state, dataValidations, merges, mergeCells, cols, properties,
      conditionalFormattings, sheetProtection, media, tables,
    } = worksheetModelSettings(input);
    if (id !== undefined && id !== this.id) {
      throw failure('UNSUPPORTED_OPTIONS', 'Worksheet model ID must match the target sheet');
    }
    if (state !== undefined && !isWorksheetState(state)) throw failure('INVALID_VALUE', 'Invalid worksheet state');
    const workbook = this.#check();
    const rename = name === undefined ? undefined : this.#prepareRename(name);
    const currentMerges = this.#mergedRanges(true);
    const inferred = mergeCells === undefined && merges === undefined && !currentMerges.length
      ? mergesFromRowModels(rows) : undefined;
    const requestedMerges = mergeCells ?? merges ?? (inferred?.ranges.length ? inferred.ranges : undefined);
    let removedMerges = [];
    let addedMerges = [];
    let plannedMerges = currentMerges;
    if (requestedMerges !== undefined) {
      if (!Array.isArray(requestedMerges) || requestedMerges.some(ref => typeof ref !== 'string')) {
        throw failure('INVALID_VALUE', 'Expected worksheet model merge ranges');
      }
      const requested = new Set(Array.from(requestedMerges, ref => mergeReference([ref])));
      const current = new Set(currentMerges.map(({ ref }) => ref));
      removedMerges = [...current].filter(ref => !requested.has(ref));
      addedMerges = [...requested].filter(ref => !current.has(ref));
      plannedMerges = [...requested].map(ref => decode_range(ref));
    }
    const followers = inferred?.followers
      ?? (plannedMerges.length ? mergesFromRowModels(rows, plannedMerges).followers : []);
    const rowEdits = new Map();
    for (const row of rows) {
      if (row === null || typeof row !== 'object') throw failure('INVALID_VALUE', 'Expected a row model');
      const number = Object.getOwnPropertyDescriptor(row, 'number')?.value;
      gridAddress(number, 1);
      if (rowEdits.has(number)) throw failure('INVALID_VALUE', 'Duplicate worksheet model row');
      rowEdits.set(number, this.#prepareRowModel(number, row, plannedMerges));
    }
    for (const number of this.#rowNumbers()) {
      if (!rowEdits.has(number)) rowEdits.set(number, this.#prepareRowModel(number, { number, cells: [] }, plannedMerges));
    }
    const fresh = workbook.stagedRevision !== undefined && !this.#stored().size;
    const page = pageSetup === undefined ? undefined : parsePageSetup(pageSetup);
    const headers = headerFooter === undefined ? undefined : headerFooterSettings(headerFooter);
    const parsedViews = views === undefined ? undefined : parseViews(views);
    const filter = autoFilter === undefined ? undefined : filterRange(autoFilter);
    const validations = dataValidations === undefined ? undefined : prepareValidationReplacement(dataValidations);
    const columns = cols === undefined ? undefined : parseColumnModel(cols);
    const sheetProperties = properties === undefined ? undefined : prepareWorksheetProperties(properties);
    const previousConditions = conditionalFormattings && this.#projectConditionalFormats();
    const conditions = previousConditions && !sameModelData(conditionalFormattings, previousConditions.blocks)
      ? Array.from(conditionalFormattings, options => prepareConditionalFormatting(options, true)) : undefined;
    const protection = sheetProtection == null ? sheetProtection : sheetProtectionModelSettings(sheetProtection);
    const images = media && Array.from(media, value => this.#prepareImage(worksheetMediaSettings(value)));
    if (images && images.filter(image => image.type === 'background').length > 1) {
      throw failure('INVALID_VALUE', 'A worksheet can have only one background image');
    }
    const previousMedia = images && workbookImages(this.#workbook, workbook);
    const placements = [];
    const tableEdits = new Map();
    for (const input of tables ?? []) {
      const table = prepareTableModel(input);
      if (tableEdits.has(table.name)) throw failure('INVALID_VALUE', 'Duplicate worksheet table model');
      tableEdits.set(table.name, table);
    }
    const removedTables = [];
    if (tables !== undefined && !workbook.ignoredNodes?.has('tableParts')) {
      for (const { model } of this.#tableDefinitions(workbook.document)) {
        const requested = tableEdits.get(model.name);
        if (requested && JSON.stringify(requested) === JSON.stringify(prepareTableModel(model))) {
          tableEdits.delete(model.name);
        } else {
          removedTables.push(model.name);
        }
      }
    }
    const previousValidations = validations && rustCall('WORKSHEET_READ_REFUSED', () =>
      workbook.document.validation_rules(this.#name));
    this.#writeDocument(workbook, candidate => {
      for (const name of removedTables) {
        rustCall('TABLE_EDIT_REFUSED', () => candidate.remove_table(this.#name, name));
      }
      if (columns) rustCall('COLUMN_EDIT_REFUSED', () => writeColumnModel(candidate, this.#name, columns));
      for (const reference of removedMerges) {
        rustCall('MERGE_EDIT_REFUSED', () => candidate.unmerge_intersecting(this.#name, reference));
      }
      this.#writeModelRows(candidate, rowEdits.values(), fresh);
      for (const reference of addedMerges) {
        rustCall('MERGE_EDIT_REFUSED', () => candidate.merge_clearing(this.#name, reference, false));
      }
      this.#writeModelFollowers(candidate, followers);
      if (page) writePageSetup(candidate, this.#name, page);
      if (headers) writeHeaderFooter(candidate, this.#name, headers);
      if (sheetProperties) writeWorksheetProperties(candidate, this.#name, sheetProperties);
      if (parsedViews) writeViews(candidate, this.#name, parsedViews);
      if (autoFilter !== undefined) {
        rustCall('AUTOFILTER_EDIT_REFUSED', () => candidate.set_autofilter(this.#name, filter));
      }
      if (validations) {
        rustCall('WORKSHEET_EDIT_REFUSED', () =>
          writeValidationReplacement(candidate, this.#name, previousValidations, validations));
      }
      if (conditions) {
        rustCall('CONDITIONAL_FORMAT_EDIT_REFUSED', () => {
          const standard = Array.from({ length: previousConditions.standardCount }, (_, index) => index);
          const extended = [...previousConditions.extensionIds.values()].flat();
          removeConditionalFormats(candidate, this.#name, standard, extended);
          for (const { headers, entries } of conditions) {
            candidate.add_conditional_rules(this.#name, headers, entries);
          }
        });
      }
      if (protection !== undefined) {
        rustCall('WORKSHEET_EDIT_REFUSED', () => {
          if (protection === null) {
            candidate.unprotect_sheet(this.#name);
            return;
          }
          const permissions = Uint8Array.from(protectionPermissions, key =>
            Number(protection[key] ?? sheetProtectionDefaults[key]));
          candidate.replace_sheet_protection(this.#name, protection.sheet ?? false, permissions,
            protection.algorithmName, protection.hashValue, protection.saltValue, protection.spinCount);
        });
      }
      if (previousMedia) {
        rustCall('IMAGE_EDIT_REFUSED', () => {
          for (const { shape } of previousMedia.sheets.get(this.#name)) {
            if (!shape) throw failure('IMAGE_EDIT_REFUSED', 'Existing image has no shape ID');
            candidate.remove_worksheet_image(this.#name, Number(shape));
          }
          if (previousMedia.backgroundIds.has(this)) candidate.remove_worksheet_background_image(this.#name);
        });
        for (const image of images) placements.push(this.#writeImage(candidate, image));
      }
      if (state !== undefined && state !== this.state) {
        rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.set_sheet_state(this.#name, state));
      }
      for (const table of tableEdits.values()) {
        rustCall('TABLE_EDIT_REFUSED', () => candidate.attach_table_model(
          this.#name, table.tableRef, table.settings, table.columns, table.calculated, table.differential));
      }
      if (rename) this.#writeRename(candidate, rename.name);
    });
    if (previousMedia) {
      previousMedia.placements.delete(this);
      previousMedia.backgrounds.delete(this);
      images.forEach((image, index) => this.#publishImage(image, placements[index]));
    }
    if (protection !== undefined) this.#protectionEdited = true;
    if (rename) this.#publishRename(rename);
    for (const edit of rowEdits.values()) this.#publishRowModel(edit);
    if (columns) {
      this.#columns = undefined;
      this.#columnKeys.clear();
    }
    if (removedMerges.length || addedMerges.length) {
      for (const reference of removedMerges) this.#linkedMergeStyles.delete(reference);
      this.#storedRows = undefined;
      this.#createdCells.clear();
      for (const { row, next } of rowEdits.values()) {
        for (const [column, model] of next) {
          if (model.value === null) this.#record(this.#createdCells, row, column, false);
        }
      }
    }
    this.#publishModelFollowers(followers, currentMerges);
    if (validations) {
      this.#explicitValidationRanges.clear();
      for (const reference of validations.keys()) this.#explicitValidationRanges.add(reference);
    }
  }

  get dimensions() {
    let top = Infinity;
    let left = Infinity;
    let bottom = 0;
    let right = 0;
    const expand = (a, b, c, d) => {
      top = Math.min(top, a);
      left = Math.min(left, b);
      bottom = Math.max(bottom, c);
      right = Math.max(right, d);
    };
    for (const [row, cells] of this.#stored()) {
      for (const [column, present] of cells) {
        if (present) expand(row, column, row, column);
      }
    }
    for (const { s, e } of this.#mergedRanges()) {
      expand(s.r + 1, s.c + 1, e.r + 1, e.c + 1);
    }
    return bottom ? worksheetDimensions(top, left, bottom, right) : worksheetDimensions(1, 1, 1, 1, true);
  }

  get columnCount() {
    let highest = 0;
    for (const cells of this.#stored().values()) {
      let rowMax = 0;
      let populated = false;
      for (const [column, present] of cells) {
        rowMax = Math.max(rowMax, column);
        populated ||= present;
      }
      if (populated) highest = Math.max(highest, rowMax);
    }
    for (const { e } of this.#mergedRanges()) highest = Math.max(highest, e.c + 1);
    return highest;
  }

  get actualColumnCount() {
    const columns = new Set();
    for (const cells of this.#stored().values()) {
      for (const [column, present] of cells) {
        if (present) columns.add(column);
      }
    }
    for (const { s, e } of this.#mergedRanges()) {
      const first = s.r === e.r && !this.#stored().get(s.r + 1)?.get(s.c + 1) ? s.c + 1 : s.c;
      for (let column = first; column <= e.c; column += 1) columns.add(column + 1);
    }
    return columns.size;
  }

  get lastColumn() {
    const count = this.columnCount;
    return count ? this.getColumn(count) : undefined;
  }

  get rowCount() {
    let highest = 0;
    for (const row of this.#stored().keys()) highest = Math.max(highest, row);
    for (const row of this.#rows.keys()) highest = Math.max(highest, row);
    for (const { e } of this.#mergedRanges()) highest = Math.max(highest, e.r + 1);
    return highest;
  }
  get lastRow() { return this.findRow(this.rowCount); }
  get actualRowCount() {
    let count = 0;
    this.eachRow(() => { count += 1; });
    return count;
  }

  eachRow(options, iteratee) {
    if (typeof options === 'function') [options, iteratee] = [undefined, options];
    if (worksheetIterationSettings(options).includeEmpty) {
      const count = this.rowCount;
      for (let row = 1; row <= count; row += 1) iteratee(this.getRow(row), row);
      return;
    }
    for (const row of this.#rowNumbers()) {
      const handle = this.getRow(row);
      if (handle.hasValues) iteratee(handle, row);
    }
  }

  getSheetValues() {
    const values = [];
    for (const row of this.#rowNumbers()) values[row] = this.getRow(row).values;
    return values;
  }

  findRows(start, count) {
    this.#check();
    gridAddress(start, 1);
    if (!Number.isInteger(count)) throw failure('INVALID_ADDRESS', 'Expected an integer row count');
    const length = Math.max(0, Math.min(count, this.rowCount - start + 1));
    const rows = new Array(length);
    for (let offset = 0; offset < length; offset += 1) {
      const row = this.findRow(start + offset);
      if (row !== undefined) rows[offset] = row;
    }
    return rows;
  }

  getRows(start, count) {
    this.#check();
    if (count < 1) return undefined;
    gridAddress(start, 1);
    if (!Number.isInteger(count)) throw failure('INVALID_ADDRESS', 'Expected an integer row count');
    gridAddress(start + count - 1, 1);
    return Array.from({ length: count }, (_, offset) => this.getRow(start + offset));
  }

  getColumnKey(key) {
    this.#check();
    return this.#columnKeys.get(key);
  }

  setColumnKey(key, column) {
    this.#check();
    this.#columnKeys.set(key, column);
  }

  deleteColumnKey(key) {
    this.#check();
    this.#columnKeys.delete(key);
  }

  eachColumnKey(iteratee) {
    this.#check();
    for (const [key, column] of [...this.#columnKeys]) iteratee(column, key);
  }

  get columns() {
    this.#check();
    return this.#columns ?? null;
  }

  set columns(definitions) {
    const state = this.#check();
    if (!Array.isArray(definitions)) throw failure('INVALID_VALUE', 'Expected an array of column definitions');
    const columns = [];
    const edits = [];
    const layout = [];
    const formats = [];
    definitions.forEach((definition) => {
      validateColumnDefinition(definition);
      const number = columns.length + 1;
      gridAddress(1, number);
      const { key, header, width, hidden, outlineLevel, style } = definition;
      const format = encodeDefaultStyle(style);
      if (format.length) formats.push([number, format]);
      if (width !== undefined || hidden !== undefined || outlineLevel !== undefined) {
        layout.push([number, width, hidden, outlineLevel === undefined ? undefined : checkedOutline(outlineLevel)]);
      }
      for (const edit of this.#headerEdits(number, header)) edits.push(edit);
      columns.push(this.#columnHandle(number, key, header));
    });
    this.#writeCells(state, edits, layout, formats.length ? (candidate) => {
      for (const [number, format] of formats) this.#applyColumnStyle(candidate, number, format);
    } : undefined);
    this.#columns = columns;
    this.#columnKeys.clear();
    for (const column of columns) {
      if (column.key) this.#columnKeys.set(column.key, column);
    }
  }

  #headerEdits(column, header) {
    const labels = header === undefined ? [] : Array.isArray(header) ? header : [header];
    const edits = [];
    labels.forEach((label, row) => {
      gridAddress(row + 1, column);
      edits.push([row + 1, column, scalar(label, true)]);
    });
    return edits;
  }

  #applyColumnStyle(candidate, number, edits) {
    for (const [property, ...args] of edits) {
      rustCall('COLUMN_EDIT_REFUSED', () => candidate['set_column_' + property](this.#name, number - 1, ...args));
      const method = property === 'number_format' ? 'set_number_format' : 'set_cell_' + property;
      for (const [row, cells] of this.#stored()) {
        if (cells.has(number)) {
          rustCall('CELL_EDIT_REFUSED', () => candidate[method](this.#name, row - 1, number - 1, ...args));
        }
      }
    }
  }

  #columnHandle(number, key, header) {
    const worksheet = this;
    const style = {
      get border() { return column.border; },
      set border(value) { column.border = value; },
      get fill() { return column.fill; },
      set fill(value) { column.fill = value; },
      get protection() { return column.protection; },
      set protection(value) { column.protection = value; },
      get alignment() { return column.alignment; },
      set alignment(value) { column.alignment = value; },
      get font() { return column.font; },
      set font(value) { column.font = value; },
      get numFmt() { return column.numFmt; },
      set numFmt(code) {
        if (code !== undefined) {
          column.numFmt = code;
          return;
        }
        const state = worksheet.#check();
        worksheet.#writeDocument(state, (candidate) =>
          rustCall('COLUMN_EDIT_REFUSED', () => candidate.clear_column_style(worksheet.#name, number - 1)));
      },
    };
    const column = {
      get worksheet() { return worksheet; },
      get border() {
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('cols')) return undefined;
        return decodeBorder(rustCall('COLUMN_READ_REFUSED', () => document.column_border(worksheet.#name, number - 1)));
      },
      set border(value) {
        const border = encodeBorder(value);
        worksheet.#writeDocument(worksheet.#check(), (candidate) =>
          worksheet.#applyColumnStyle(candidate, number, [['border', border]]));
      },
      get fill() {
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('cols')) return undefined;
        return decodeFill(rustCall('COLUMN_READ_REFUSED', () => document.column_fill(worksheet.#name, number - 1)));
      },
      set fill(value) {
        const fill = encodeFill(value);
        worksheet.#writeDocument(worksheet.#check(), (candidate) =>
          worksheet.#applyColumnStyle(candidate, number, [['fill', fill]]));
      },
      get protection() {
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('cols')) return undefined;
        return decodeProtection(rustCall('COLUMN_READ_REFUSED', () => document.column_protection(worksheet.#name, number - 1)));
      },
      set protection(value) {
        const flags = encodeProtection(value);
        worksheet.#writeDocument(worksheet.#check(), (candidate) =>
          worksheet.#applyColumnStyle(candidate, number, [['protection', ...flags]]));
      },
      get alignment() {
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('cols')) return undefined;
        return decodeAlignment(rustCall('COLUMN_READ_REFUSED', () => document.column_alignment(worksheet.#name, number - 1)));
      },
      set alignment(value) {
        const alignment = encodeAlignment(value);
        worksheet.#writeDocument(worksheet.#check(), (candidate) =>
          worksheet.#applyColumnStyle(candidate, number, [['alignment', alignment]]));
      },
      get font() {
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('cols')) return undefined;
        return decodeDeclaredFont(rustCall('COLUMN_READ_REFUSED', () => document.column_font(worksheet.#name, number - 1)));
      },
      set font(value) {
        const font = encodeFont(value);
        const state = worksheet.#check();
        worksheet.#writeDocument(state, (candidate) => worksheet.#applyColumnStyle(candidate, number, [['font', font]]));
      },
      get numFmt() {
        const { document, ignoredNodes } = worksheet.#check();
        if (ignoredNodes?.has('cols')) return undefined;
        return rustCall('COLUMN_READ_REFUSED', () => document.column_number_format(worksheet.#name, number - 1));
      },
      set numFmt(code) {
        checkedColumnNumberFormat(code);
        const state = worksheet.#check();
        worksheet.#writeDocument(state, (candidate) => worksheet.#applyColumnStyle(candidate, number, [['number_format', code]]));
      },
      get style() { return style; },
      set style(value) {
        const edits = encodeDefaultStyle(value);
        if (!edits.length) {
          style.numFmt = undefined;
          return;
        }
        const state = worksheet.#check();
        worksheet.#writeDocument(state, (candidate) => worksheet.#applyColumnStyle(candidate, number, edits));
      },
      get defn() {
        const [width, hidden, outlineLevel] = worksheet.#columnLayout(number);
        const entries = Object.entries({ numFmt: column.numFmt, font: column.font, alignment: column.alignment, protection: column.protection, fill: column.fill, border: column.border });
        return { header, key, width, hidden, outlineLevel, style: Object.fromEntries(entries.filter(([, value]) => value !== undefined)) };
      },
      set defn(value) {
        if (value === null || value === undefined) {
          const state = worksheet.#check();
          worksheet.#writeDocument(state, (candidate) => {
            rustCall('COLUMN_EDIT_REFUSED', () => {
              const [, , outline] = candidate.column_layout(worksheet.#name, number - 1);
              if (outline) candidate.set_column_layout(worksheet.#name, number - 1, undefined, undefined, 0);
              candidate.clear_column_width(worksheet.#name, number - 1);
              candidate.clear_column_style(worksheet.#name, number - 1);
            });
          });
          header = undefined;
          column.key = undefined;
          return;
        }
        validateColumnDefinition(value, true);
        const { header: nextHeader, key: nextKey, width = 9, hidden = false, outlineLevel = 0, style } = value;
        const format = encodeDefaultStyle(style);
        if (nextKey !== undefined && typeof nextKey !== 'string') {
          throw failure('INVALID_VALUE', 'Expected a string column key');
        }
        const state = worksheet.#check();
        worksheet.#writeCells(state, worksheet.#headerEdits(number, nextHeader),
          [[number, width, hidden, checkedOutline(outlineLevel)]],
          format.length ? (candidate) => worksheet.#applyColumnStyle(candidate, number, format) : undefined);
        header = nextHeader;
        column.key = nextKey;
      },
      get headers() { return Array.isArray(header) ? header : [header]; },
      get headerCount() { return column.headers.length; },
      get isCustomWidth() {
        const width = column.width;
        return width !== undefined && width !== 9;
      },
      get isDefault() { return !column.isCustomWidth && !column.hidden && !column.outlineLevel && !column.numFmt && !column.font && !column.alignment && !column.protection && !column.fill && !column.border; },
      equivalentTo(other) {
        return column.width === other.width && column.hidden === other.hidden &&
          column.outlineLevel === other.outlineLevel && column.numFmt === other.numFmt &&
          JSON.stringify(column.font) === JSON.stringify(other.font) &&
          JSON.stringify(column.alignment) === JSON.stringify(other.alignment) &&
          JSON.stringify(column.protection) === JSON.stringify(other.protection) &&
          JSON.stringify(column.fill) === JSON.stringify(other.fill) &&
          JSON.stringify(column.border) === JSON.stringify(other.border);
      },
      toString() {
        const headers = column.headers;
        return JSON.stringify({ key: column.key, width: column.width, headers: headers.length ? headers : undefined });
      },
      get number() { return number; },
      get letter() { return encode_col(number - 1); },
      get key() { return key; },
      set key(value) {
        worksheet.#check();
        if (worksheet.#columnKeys.get(key) === column) worksheet.#columnKeys.delete(key);
        key = value;
        if (key) worksheet.#columnKeys.set(key, column);
      },
      eachCell(options, iteratee) {
        if (typeof options === 'function') [options, iteratee] = [undefined, options];
        worksheet.eachRow(options, (row, rowNumber) => iteratee(row.getCell(number), rowNumber));
      },
      get values() {
        const values = [];
        column.eachCell((cell, row) => {
          if (cell.type !== ValueType.Null) values[row] = cell.value;
        });
        return values;
      },
      set values(entries) {
        const state = worksheet.#check();
        if (entries == null) return;
        if (!Array.isArray(entries)) throw failure('INVALID_VALUE', 'Expected an array of column values');
        const offset = Number(Object.hasOwn(entries, 0));
        const edits = [];
        entries.forEach((value, index) => {
          const row = index + offset;
          gridAddress(row, number);
          const merge = worksheet.#mergeAt(row - 1, number - 1);
          edits.push([merge ? merge.s.r + 1 : row, merge ? merge.s.c + 1 : number, excelValue(value)]);
        });
        if (!edits.length) return;
        const stored = worksheet.#stored();
        const batchScalars = edits.every(([row, col]) =>
          !stored.get(row)?.has(col) && !worksheet.#mergeAt(row - 1, col - 1, true));
        worksheet.#writeCells(state, edits, [], undefined, batchScalars);
      },
      get width() { return worksheet.#columnLayout(number)[0]; },
      set width(value) { worksheet.#writeCells(worksheet.#check(), [], [[number, value, undefined]]); },
      get collapsed() {
        const level = column.outlineLevel;
        return level > 0 && level >= worksheet.properties.outlineLevelCol;
      },
      get outlineLevel() { return worksheet.#columnLayout(number)[2]; },
      set outlineLevel(value) {
        worksheet.#writeCells(worksheet.#check(), [], [[number, undefined, undefined, checkedOutline(value)]]);
      },
      get hidden() { return worksheet.#columnLayout(number)[1]; },
      set hidden(value) { worksheet.#writeCells(worksheet.#check(), [], [[number, undefined, value]]); },
      get header() { return header; },
      set header(value) {
        const state = worksheet.#check();
        worksheet.#writeCells(state, worksheet.#headerEdits(number, value));
        header = value;
      },
    };
    return column;
  }

  #columnLayout(number) {
    const { document, ignoredNodes } = this.#check();
    if (ignoredNodes?.has('cols')) return [undefined, false, 0];
    return rustCall('COLUMN_READ_REFUSED', () => document.column_layout(this.#name, number - 1));
  }

  getColumn(reference) {
    this.#check();
    const keyed = this.#columnKeys.get(reference);
    if (keyed) return keyed;
    const number = typeof reference === 'string' ? address(reference + '1')[2] + 1 : reference;
    gridAddress(1, number);
    this.#columns ??= [];
    while (this.#columns.length < number) {
      this.#columns.push(this.#columnHandle(this.#columns.length + 1));
    }
    return this.#columns[number - 1];
  }

  spliceColumns(position, count, ...values) {
    const state = this.#check();
    gridAddress(1, position);
    if (!isSpliceCount(count)) {
      throw failure('INVALID_ADDRESS', 'Expected a nonnegative column count');
    }
    if (!count && !values.length) return;
    gridAddress(1, position + Math.max(count, values.length) - 1);
    const replaced = Math.min(count, values.length);
    const edits = [];
    for (const [offset, entries] of values.entries()) {
      if (!Array.isArray(entries)) throw failure('INVALID_VALUE', 'Expected arrays of column values');
      const column = position + offset;
      const next = new Map();
      entries.forEach((entry, index) => {
        const row = index + 1;
        gridAddress(row, column);
        if (entry !== undefined) next.set(row, excelValue(entry));
      });
      if (offset < replaced) {
        for (const [row, cells] of this.#stored()) {
          if (cells.get(column) && !next.has(row)) edits.push([row, column, null]);
        }
      }
      for (const [row, value] of next) {
        if (offset < replaced || value !== null) edits.push([row, column, value]);
      }
    }
    let mergeStyles;
    this.#writeDocument(state, (candidate) => {
      const tail = position + replaced - 1;
      if (count > replaced) {
        rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.delete_columns(this.#name, tail, count - replaced));
      }
      if (values.length > replaced) {
        rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.insert_columns(this.#name, tail, values.length - replaced));
      }
      for (const [row, column, value] of edits) {
        rustCall('CELL_EDIT_REFUSED', () => writeExcelValue(candidate, this.#name, row - 1, column - 1, value));
      }
      mergeStyles = this.#shiftedLinkedMergeStyles(candidate, 'c', tail, values.length - count);
    });
    this.#linkedMergeStyles = mergeStyles;
    this.#storedRows = undefined;
    this.#createdCells.clear();
    const delta = values.length - count;
    if (delta) {
      for (const key of this.#cells.keys()) {
        if (address(key)[2] >= position + replaced - 1) this.#cells.delete(key);
      }
    }
    if (this.#columns) {
      const next = this.#columns.slice(0, position - 1);
      const remapped = new Map(next.map((column) => [column, column]));
      for (const column of this.#columns.slice(position + count - 1)) {
        const number = column.number + delta;
        if (number <= 16384) {
          const shifted = number === column.number ? column : this.#columnHandle(number, column.key, column.header);
          next[number - 1] = shifted;
          remapped.set(column, shifted);
        }
      }
      this.#columns = Array.from(next, (column, index) => column ?? this.#columnHandle(index + 1));
      for (const [key, column] of this.#columnKeys) {
        const shifted = remapped.get(column);
        if (shifted) this.#columnKeys.set(key, shifted);
        else this.#columnKeys.delete(key);
      }
    }
  }

  insertRow(position, values, style) {
    return this.insertRows(position, [values], style)[0];
  }

  insertRows(position, values, style = 'n') {
    const { source, includeEmpty } = rowStyleSettings(style, true);
    if (!Array.isArray(values)) throw failure('INVALID_VALUE', 'Expected an array of rows');
    gridAddress(position, 1);
    const formats = values.map((_, offset) => source === 'none' ? undefined :
      this.#rowStyle(source === 'previous' ? position - 1 : position + offset, includeEmpty));
    this.#spliceRows(position, 0, values, (candidate) => {
      formats.forEach((format, offset) => this.#applyRowStyle(candidate, position + offset, format));
    });
    return this.getRows(position, values.length);
  }

  fillFormula(range, formula, results, shareType = undefined) {
    const state = this.#check();
    shareType = checkedFillFormulaMode(shareType);
    checkedFillFormulaText(formula);
    const { top, left, bottom, right, reference } = formulaRange(range);
    const text = scalar(formula, false);
    checkedFillFormulaResults(results);
    const matrix = Array.isArray(results?.[0]);
    const width = right - left + 1;
    const values = [];
    const types = [];
    const dates = [];
    let mergeDocument;
    let firstOverlap;

    for (let row = top; row <= bottom; row++) {
      for (let col = left; col <= right; col++) {
        const { document } = this.#check();
        if (document !== mergeDocument) {
          // A result callback or accessor can edit the worksheet during the fill.
          mergeDocument = document;
          firstOverlap = rustCall('CELL_READ_REFUSED', () =>
            document.first_merged_overlap(this.#name, reference, row, col));
        }
        if (values.length === firstOverlap) {
          throw failure('CELL_EDIT_REFUSED', 'Formula range overlaps a merged cell');
        }
        const result = typeof results === 'function' ? results(row + 1, col + 1)
          : matrix ? results[row - top]?.[col - left] : results?.[(row - top) * width + col - left];
        const value = excelFormulaResult(result);
        if (value instanceof Date) {
          if (shareType === 'array' && values.length) dates.push(isoCellText(value));
          values.push(value.getTime());
          types.push('dateMillis');
        } else {
          values.push(value?.error ?? value);
          types.push(value?.error === undefined ? '' : 'e');
        }
      }
    }
    this.#writeDocument(state, (candidate) => rustCall('CELL_EDIT_REFUSED', () => {
      if (shareType === 'shared') candidate.write_shared_formula(this.#name, reference, text, values, types);
      else candidate.write_excel_array_formula(this.#name, reference, text, values, types, dates);
    }));
    this.#storedRows = undefined;
  }

  spliceRows(position, count, ...values) {
    this.#spliceRows(position, count, values);
  }

  duplicateRow(rowNumber, count, insert = false) {
    this.#check();
    gridAddress(rowNumber, 1);
    if (!isSpliceCount(count) || typeof insert !== 'boolean') {
      throw failure('INVALID_VALUE', 'Expected a nonnegative duplicate count and a boolean insert flag');
    }
    if (!count) return;
    gridAddress(rowNumber + count, 1);
    if (!this.findRow(rowNumber)) throw failure('INVALID_VALUE', 'Source row does not exist');
    if (this.#mergedRanges(true).some(({ s, e }) => rowNumber > s.r && rowNumber <= e.r + 1)) {
      throw failure('UNSUPPORTED_OPTIONS', 'Cannot duplicate a row intersecting merged cells');
    }
    const values = [];
    for (const [column, present] of this.#stored().get(rowNumber) ?? []) {
      if (present) {
        const value = this.getCell(rowNumber, column).value;
        values[column] = value?.formula === undefined ? value : { formula: value.formula };
      }
    }
    const format = this.#rowStyle(rowNumber, true);
    this.#spliceRows(rowNumber + 1, insert ? 0 : count, Array(count).fill(values), (candidate) => {
      for (let row = rowNumber + 1; row <= rowNumber + count; row += 1) {
        this.#applyRowStyle(candidate, row, format);
      }
    });
  }

  #rowStyle(row, includeEmpty) {
    if (row < 1 || row > 1048576 || !this.findRow(row)) return undefined;
    const { document } = this.#check();
    const [height] = rustCall('ROW_READ_REFUSED', () => document.row_layout(this.#name, row - 1));
    const styles = [];
    for (const [column, present] of this.#stored().get(row) ?? []) {
      if (present || includeEmpty) {
        const style = rustCall('CELL_READ_REFUSED', () => document.cell_style_index(this.#name, row - 1, column - 1));
        styles.push([column, style]);
      }
    }
    const index = rustCall('ROW_READ_REFUSED', () => document.row_style_index(this.#name, row - 1));
    return { height, styles, index };
  }

  #applyRowStyle(candidate, row, format) {
    if (!format) return;
    rustCall('ROW_EDIT_REFUSED', () => {
      if (candidate.row_style_index(this.#name, row - 1) !== format.index) {
        candidate.set_row_style_index(this.#name, row - 1, format.index);
      }
    });
    if (format.height !== undefined) {
      rustCall('ROW_EDIT_REFUSED', () => candidate.set_row_layout(this.#name, row - 1, format.height, undefined, undefined));
    }
    for (const [column, style] of format.styles) {
      rustCall('CELL_EDIT_REFUSED', () => {
        if (candidate.cell_style_index(this.#name, row - 1, column - 1) !== style) {
          candidate.set_cell_style_index(this.#name, row - 1, column - 1, style);
        }
      });
    }
  }

  #spliceRows(position, count, values, afterWrite) {
    const state = this.#check();
    gridAddress(position, 1);
    if (!isSpliceCount(count)) {
      throw failure('INVALID_ADDRESS', 'Expected a nonnegative row count');
    }
    if (!count && !values.length) return;
    gridAddress(position + Math.max(count, values.length) - 1, 1);
    const replaced = Math.min(count, values.length);
    const edits = [];
    for (const [offset, entries] of values.entries()) {
      const row = position + offset;
      const next = this.#rowValues(row, entries);
      if (offset < replaced) {
        for (const [column, present] of this.#stored().get(row) ?? []) {
          if (present && !next.has(column)) edits.push([row, column, null]);
        }
      }
      for (const [column, value] of next) {
        if (offset < replaced || value !== null) edits.push([row, column, value]);
      }
    }
    let mergeStyles;
    this.#writeDocument(state, (candidate) => {
      const tail = position + replaced - 1;
      if (count > replaced) {
        rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.delete_rows(this.#name, tail, count - replaced));
      }
      if (values.length > replaced) {
        rustCall('WORKSHEET_EDIT_REFUSED', () => candidate.insert_rows(this.#name, tail, values.length - replaced));
      }
      for (const [row, column, value] of edits) {
        rustCall('CELL_EDIT_REFUSED', () => writeExcelValue(candidate, this.#name, row - 1, column - 1, value));
      }
      afterWrite?.(candidate);
      mergeStyles = this.#shiftedLinkedMergeStyles(candidate, 'r', tail, values.length - count);
    });
    this.#linkedMergeStyles = mergeStyles;
    this.#storedRows = undefined;
    this.#createdCells.clear();
    if (count !== values.length) {
      const firstShifted = position + replaced;
      for (const row of this.#rows.keys()) {
        if (row >= firstShifted) this.#rows.delete(row);
      }
      for (const key of this.#cells.keys()) {
        if (address(key)[1] >= firstShifted - 1) this.#cells.delete(key);
      }
    }
  }

  addRow(values, style) {
    return this.addRows([values], style)[0];
  }

  addRows(values, style = 'n') {
    const state = this.#check();
    const { source, includeEmpty } = rowStyleSettings(style, false);
    if (!Array.isArray(values)) throw failure('INVALID_VALUE', 'Expected an array of rows');
    const start = this.rowCount + 1;
    const rows = [];
    const edits = [];
    values.forEach((entries) => {
      const row = start + rows.length;
      gridAddress(row, 1);
      const cells = this.#rowValues(row, entries);
      for (const [col, value] of cells) {
        if (value !== null) edits.push([row, col, value]);
      }
      rows.push([row, cells]);
    });

    const format = source === 'none' || !rows.length ? undefined : this.#rowStyle(start - 1, includeEmpty);
    const afterWrite = format ? (candidate) => {
      let inherited = format;
      for (const [row, columns] of rows) {
        this.#applyRowStyle(candidate, row, inherited);
        if (!includeEmpty) {
          inherited = { ...inherited, styles: inherited.styles.filter(([col]) => columns.get(col) != null) };
        }
      }
    } : undefined;
    this.#writeCells(state, edits, [], afterWrite, true);
    return rows.map(([row, columns]) => {
      const handle = this.getRow(row);
      for (const col of columns.keys()) handle.getCell(col);
      return handle;
    });
  }

  #rowValues(row, entries) {
    const cells = new Map();
    const append = (value, col) => {
      if (value === undefined) return;
      gridAddress(row, col);
      cells.set(col, excelValue(value));
    };
    if (Array.isArray(entries)) {
      const offset = Number(Object.hasOwn(entries, 0));
      entries.forEach((value, index) => append(value, index + offset));
    } else if (entries && typeof entries === 'object') {
      for (const [key, column] of this.#columnKeys) append(entries[key], column.number);
    } else if (entries != null) {
      throw failure('INVALID_VALUE', 'Expected row values as an array or keyed object');
    }
    return cells;
  }

  #spliceRowCells(row, start, count, values) {
    const state = this.#check();
    gridAddress(row, start);
    if (!isSpliceCount(count)) throw failure('INVALID_ADDRESS', 'Expected a nonnegative cell count');
    if (!count && !values.length) return;
    gridAddress(row, start + Math.max(count, values.length) - 1);
    if (this.#mergedRanges(true).some(({ s, e }) => row > s.r && row <= e.r + 1 && e.c + 1 >= start)) {
      throw failure('UNSUPPORTED_OPTIONS', 'Cannot splice through a merged range');
    }

    const edits = new Map();
    const delta = values.length - count;
    const end = start + count;
    const document = state.document;
    const noteColumns = rustCall('CELL_READ_REFUSED', () => document.row_note_columns(this.#name, row - 1));
    let hasAffectedNotes = false;
    for (const col of noteColumns) {
      const column = col + 1;
      if (column < start || (!delta && column >= end)) continue;
      hasAffectedNotes = true;
      if (column >= end) gridAddress(row, column + delta);
    }
    const sharedGroups = new Map();
    const arrayGroups = [];
    const sharedMembers = [];
    for (const [column, present] of this.#stored().get(row) ?? []) {
      if (column < start || (!delta && column >= end)) continue;
      const style = document.cell_style_index(this.#name, row - 1, column - 1);
      if (present || style) edits.set(column, { value: null, style: 0 });
      if (!present || column >= end) continue;
      const cell = this.getCell(row, column);
      const old = cell.value;
      if (old?.shareType === 'shared' || old?.shareType === 'array') {
        const range = decode_range(old.ref);
        if (range.s.r === row - 1 && range.e.r === row - 1 && range.s.c + 1 === column &&
            range.e.c + 1 < end) {
          if (old.shareType === 'array') arrayGroups.push({ reference: old.ref });
          else sharedGroups.set(cell.address, { column, members: [] });
        }
      } else if (old?.sharedFormula !== undefined) {
        sharedMembers.push({ column, old });
      }
    }
    for (const [column, present] of this.#stored().get(row) ?? []) {
      if (!delta || column < end) continue;
      const style = document.cell_style_index(this.#name, row - 1, column - 1);
      if (!present && !style) continue;
      const target = column + delta;
      gridAddress(row, target);
      const cell = this.getCell(row, column);
      const old = cell.value;
      if (old?.shareType === 'shared' || old?.shareType === 'array') {
        const range = decode_range(old.ref);
        if (range.s.r === row - 1 && range.e.r === row - 1 && range.s.c + 1 === column) {
          const final = gridAddress(row, range.e.c + 1 + delta);
          const reference = `${gridAddress(row, target)}:${final}`;
          const value = excelValue({ ...old, ref: reference });
          if (old.shareType === 'array') arrayGroups.push({ reference: old.ref, target, value });
          else sharedGroups.set(cell.address, { column, target, members: [], value });
          edits.set(target, { value: null, style });
          continue;
        }
      } else if (old?.sharedFormula !== undefined) {
        sharedMembers.push({ column, target, old });
        edits.set(target, { value: null, style });
        continue;
      }
      const value = old?.formula === undefined ? excelValue(old) : excelValue({ formula: old.formula, result: old.result });
      edits.set(target, { value, style });
    }
    for (const { column, target, old } of sharedMembers) {
      const group = sharedGroups.get(old.sharedFormula);
      if (!group) throw failure('CELL_EDIT_REFUSED', 'Cannot splice through a shared formula group');
      if (target === undefined) {
        group.members.push({ column });
      } else {
        group.members.push({ column, target, result: old.result });
      }
    }
    for (const [index, value] of values.entries()) {
      edits.set(start + index, { value: excelValue(value), style: 0 });
    }
    if (!edits.size && !hasAffectedNotes) return;
    this.#writeDocument(state, (candidate) => {
      if (hasAffectedNotes) rustCall('CELL_EDIT_REFUSED', () =>
        candidate.splice_row_notes(this.#name, row - 1, start - 1, count, values.length));
      for (const group of arrayGroups) {
        rustCall('CELL_EDIT_REFUSED', () => candidate.clear_static_array_row(this.#name, group.reference));
      }
      for (const group of sharedGroups.values()) {
        rustCall('CELL_EDIT_REFUSED', () => candidate.clear_shared_formula_for_move(
          this.#name, row - 1, group.column - 1, Uint32Array.from(group.members, member => member.column - 1)));
      }
      const coordinates = [];
      const scalars = [];
      const styles = [];
      const flush = () => {
        if (!scalars.length) return;
        rustCall('CELL_EDIT_REFUSED', () => candidate.write_excel_scalar_batch(
          this.#name, Uint32Array.from(coordinates), scalars, Uint32Array.from(styles)));
        coordinates.length = scalars.length = styles.length = 0;
      };
      for (const [column, { value, style }] of edits) {
        if (value === null || typeof value !== 'object') {
          coordinates.push(row - 1, column - 1);
          scalars.push(value);
          styles.push(style);
          continue;
        }
        flush();
        rustCall('CELL_EDIT_REFUSED', () => {
          writeExcelValue(candidate, this.#name, row - 1, column - 1, value);
          if (candidate.cell_style_index(this.#name, row - 1, column - 1) !== style) {
            candidate.set_cell_style_index(this.#name, row - 1, column - 1, style);
          }
        });
      }
      flush();
      for (const group of arrayGroups) {
        if (group.target !== undefined) rustCall('CELL_EDIT_REFUSED', () =>
          writeExcelValue(candidate, this.#name, row - 1, group.target - 1, group.value));
      }
      for (const group of sharedGroups.values()) {
        if (group.target === undefined) continue;
        rustCall('CELL_EDIT_REFUSED', () => {
          const { formula, range, result } = group.value;
          const results = [result, ...group.members.map(member => member.result)];
          const cached = results.map(value => value instanceof Date ? value.getTime() : value?.error ?? value);
          const types = results.map(value => value instanceof Date ? 'dateMillis' : value?.error === undefined ? '' : 'e');
          candidate.write_sparse_shared_formula_row(this.#name, range.reference, formula,
            Uint32Array.from(group.members, member => member.target - 1), cached, types);
        });
      }
    });
    this.#storedRows = undefined;
  }

  #writeModelFollowers(candidate, followers) {
    for (const { row, column, hasStyle, model } of followers) {
      rustCall('CELL_EDIT_REFUSED', () => {
        if (hasStyle) writeCellModel(candidate, this.#name, row, column, model);
        else writeCellNote(candidate, this.#name, row, column, model.note);
      });
    }
  }

  #publishModelFollowers(followers, merges) {
    if (!followers.length || !this.#linkedMergeStyles.size) return;
    const refsByAnchor = new Map(merges.map(({ s, ref }) => [gridAddress(s.r + 1, s.c + 1), ref]));
    for (const { row, column, anchor, hasStyle } of followers) {
      if (hasStyle) this.#linkedMergeStyles.get(refsByAnchor.get(anchor))?.add(gridAddress(row + 1, column + 1));
    }
  }

  #replaceRowModel(row, input) {
    const state = this.#check();
    const merges = this.#mergedRanges(true).filter(({ s, e }) => s.r < row && row <= e.r + 1);
    const edit = this.#prepareRowModel(row, input, merges);
    const followers = merges.length ? mergesFromRowModels([input], merges).followers : [];
    this.#writeDocument(state, candidate => {
      this.#writeRowModel(candidate, edit);
      this.#writeModelFollowers(candidate, followers);
    });
    this.#publishRowModel(edit);
    this.#publishModelFollowers(followers, merges);
  }

  #prepareRowModel(row, input, merges = this.#mergedRanges(true)) {
    const findMerge = column => merges.find(({ s, e }) =>
      row - 1 >= s.r && row - 1 <= e.r && column - 1 >= s.c && column - 1 <= e.c);
    const { number, cells, height, hidden, outlineLevel, style } = rowModelSettings(input);
    if (number !== row) throw failure('INVALID_VALUE', 'Invalid row number in model');
    if (height !== undefined && typeof height !== 'number') {
      throw failure('INVALID_VALUE', 'Expected a numeric row height');
    }
    const outline = checkedOutline(outlineLevel ?? 0);
    const rowStyle = encodeDefaultStyle(style ?? undefined);
    const next = new Map();
    for (const { model, column } of rowModelCells(row, cells)) {
      const merge = findMerge(column);
      if (merge && (merge.s.r !== row - 1 || merge.s.c !== column - 1)) {
        throw failure('UNSUPPORTED_OPTIONS', 'Cell model cannot change merge membership');
      }
      next.set(column, prepareCellModel(model));
    }
    const removed = [...(this.#stored().get(row)?.keys() ?? [])]
      .filter(column => !next.has(column) && !findMerge(column));
    return { row, height, hidden, outline, rowStyle, next, removed };
  }

  #writeModelRows(candidate, edits, fresh) {
    if (!fresh) {
      for (const edit of edits) this.#writeRowModel(candidate, edit);
      return;
    }
    const coordinates = [];
    const values = [];
    let permitsBatch = true;
    const flush = () => {
      if (!values.length) return;
      rustCall('ROW_EDIT_REFUSED', () => candidate.write_excel_scalar_batch(
        this.#name, Uint32Array.from(coordinates), values));
      coordinates.length = 0;
      values.length = 0;
    };
    const write = (row, column, cell) => {
      const { value, edits, note } = cell;
      if (permitsBatch && !edits.length && note === undefined && (value === null || typeof value !== 'object')) {
        if (value !== null) {
          coordinates.push(row, column);
          values.push(value);
        }
        return;
      }
      flush();
      this.#writeModelCell(candidate, row, column, cell);
      // A range formula may have materialized cells that were absent at the start.
      if (value?.range !== undefined) permitsBatch = false;
    };
    for (const edit of edits) this.#writeRowModel(candidate, edit, write, permitsBatch);
    flush();
  }

  #writeModelCell(candidate, row, column, model) {
    writeCellModel(candidate, this.#name, row, column, model);
    if (model.note === undefined && candidate.cell_note(this.#name, row, column) !== undefined) {
      candidate.set_cell_note(this.#name, row, column, undefined);
    }
  }

  #writeRowModel(candidate, { row, height, hidden, outline, rowStyle, next, removed },
    write = (row, column, model) => this.#writeModelCell(candidate, row, column, model), fresh = false) {
    rustCall('ROW_EDIT_REFUSED', () => {
      const defaultHeight = height === undefined || height === 0;
      if (!fresh && defaultHeight) candidate.clear_row_height(this.#name, row - 1);
      const willWriteRow = fresh && [...next.values()].some(({ value }) => value !== null && typeof value !== 'object');
      if (!willWriteRow || !defaultHeight || hidden || outline) {
        candidate.set_row_layout(this.#name, row - 1, defaultHeight ? undefined : height, hidden ?? false, outline);
      }
      if (!fresh && candidate.row_style_index(this.#name, row - 1) !== undefined) {
        candidate.set_row_style_index(this.#name, row - 1, undefined);
      }
      for (const [kind, ...args] of rowStyle) candidate['set_row_' + kind](this.#name, row - 1, ...args);
      for (const column of removed) {
        writeCellModel(candidate, this.#name, row - 1, column - 1, { value: null, edits: [] });
        candidate.set_cell_note(this.#name, row - 1, column - 1, undefined);
      }
      if (removed.length) {
        candidate.prune_empty_cells(this.#name, row - 1, Uint32Array.from(removed, column => column - 1));
      }
      for (const [column, model] of next) write(row - 1, column - 1, model);
    });
  }

  #publishRowModel({ row, next, removed }) {
    const stored = this.#storedRows ?? this.#createdCells;
    for (const column of removed) {
      stored.get(row)?.delete(column);
      this.#cells.delete(gridAddress(row, column));
    }
    for (const [column, model] of next) {
      this.#record(stored, row, column, model.value !== null);
    }
  }

  #replaceRowValues(row, entries) {
    const state = this.#check();
    const next = this.#rowValues(row, entries);
    const existing = this.#stored().get(row) ?? new Map();
    const edits = [];
    for (const [col, present] of existing) {
      if (present && !next.has(col)) edits.push([row, col, null]);
    }
    for (const [col, value] of next) {
      if (value !== null || existing.get(col)) edits.push([row, col, value]);
    }
    const batchScalars = edits.length > 0 && !existing.size && !this.#mergedRanges(true).some(({ s }) => s.r === row - 1);
    this.#writeCells(state, edits, [], undefined, batchScalars);
    for (const col of next.keys()) this.getCell(row, col);
  }

  #writeCells(state, edits, layout = [], afterWrite, batchScalars = false) {
    if (edits.length || layout.length || afterWrite) {
      this.#writeDocument(state, (candidate) => {
        const coordinates = [];
        const values = [];
        const flush = () => {
          if (!values.length) return;
          rustCall('CELL_EDIT_REFUSED', () =>
            candidate.write_excel_scalar_batch(this.#name, Uint32Array.from(coordinates), values));
          coordinates.length = 0;
          values.length = 0;
        };
        for (const [row, col, value] of edits) {
          if (batchScalars && typeof value !== 'object') {
            coordinates.push(row - 1, col - 1);
            values.push(value);
          } else {
            flush();
            rustCall('CELL_EDIT_REFUSED', () => writeExcelValue(candidate, this.#name, row - 1, col - 1, value));
          }
        }
        flush();
        for (const [column, width, hidden, outline] of layout) {
          if ((width !== undefined && typeof width !== 'number') ||
              (hidden !== undefined && typeof hidden !== 'boolean')) {
            throw failure('INVALID_VALUE', 'Expected a numeric width or boolean visibility');
          }
          rustCall('COLUMN_EDIT_REFUSED', () => candidate.set_column_layout(this.#name, column - 1, width, hidden, outline));
        }
        afterWrite?.(candidate);
      });
    }
    const stored = this.#storedRows ?? this.#createdCells;
    for (const [row, col, value] of edits) this.#record(stored, row, col, value !== null);
  }

  #writeDocument(state, edit) {
    editWorkbookDocument(state, edit);
  }

  findCell(reference, col) {
    this.#check();
    const [key, row, column] = worksheetCellAddress(reference, col);
    const existing = this.#cells.get(key);
    if (existing) return existing;
    if (this.#stored().get(row + 1)?.has(column + 1) || this.#mergeAt(row, column)) {
      return this.getCell(key);
    }
    return undefined;
  }

  [destroyRowHandle](row) {
    if (this.#rows.get(row.number) === row) (this.#destroyedRowNumbers ??= new Set()).add(row.number);
  }

  get [pendingCellMerges]() { return this.#cellMerges?.size > 0; }

  [writePendingCellMerges](document) {
    if (!this[pendingCellMerges]) return;
    const links = [];
    for (const [key, { master, follower }] of this.#cellMerges) {
      if (this.#cells.get(key) !== follower || this.#cells.get(master.address) !== master) {
        throw failure('MERGE_EDIT_REFUSED', 'A structural edit invalidated a cell merge');
      }
      const [, row, col] = address(key);
      links.push(master.row - 1, master.col - 1, row, col);
    }
    rustCall('MERGE_EDIT_REFUSED', () => {
      const ranges = XlsxDocument.infer_merge_ranges(Uint32Array.from(links), true);
      for (const [key, { master, linkedStyle }] of this.#cellMerges) {
        if (!linkedStyle) continue;
        const [, row, col] = address(key);
        const style = document.cell_style_index(this.#name, master.row - 1, master.col - 1);
        document.set_cell_style_index(this.#name, row, col, style);
      }
      for (const range of ranges) document.merge_preserving_covered_values(this.#name, range);
    });
  }

  [assertLiveHandles]() {
    if (this.#destroyedRowNumbers) {
      for (const number of this.#destroyedRowNumbers) {
        const row = this.#rows.get(number);
        if (row && row.worksheet === undefined) throw new TypeError('Row has been destroyed');
        this.#destroyedRowNumbers.delete(number);
      }
    }
    if (!this.#destroyedCellAddresses) return;
    for (const key of this.#destroyedCellAddresses) {
      const cell = this.#cells.get(key);
      if (cell && cell.address === undefined) throw new TypeError('Cell has been destroyed');
      this.#destroyedCellAddresses.delete(key);
    }
  }

  getCell(reference, col) {
    this.#check();
    const [key, row, column] = worksheetCellAddress(reference, col);
    if (!this.#cells.has(key)) {
      let destroyed = false;
      const assertLive = () => {
        if (destroyed) throw new TypeError('Cell has been destroyed');
      };
      const check = () => {
        assertLive();
        return this.#check();
      };
      let mergeReferences = 0;
      const directMerge = () => worksheet.#cellMerges?.get(key);
      const styleMaster = () => {
        const merge = directMerge();
        return merge?.linkedStyle ? merge.master : undefined;
      };
      const sheetName = () => this.#name;
      const ignoredCoveredCell = (ignoredNodes) => {
        if (!ignoredNodes?.has('mergeCells')) return false;
        const merge = worksheet.#mergeAt(row, column, true);
        return merge && (merge.s.r !== row || merge.s.c !== column);
      };
      const rowHandle = this.getRow(row + 1);
      const rows = this.#storedRows ?? this.#createdCells;
      if (!rows.get(row + 1)?.has(column + 1)) this.#record(rows, row + 1, column + 1, false);
      const setPresence = (present) => this.#record(this.#storedRows ?? this.#createdCells, row + 1, column + 1, present);
      const worksheet = this;
      const namedCell = () => {
        check();
        const point = { r: row, c: column };
        return { sheet: sheetName(), s: point, range: formatNamedReference(sheetName(), point, point) };
      };
      const writeStyle = (value, linked) => {
        const master = styleMaster();
        if (master && linked) {
          const [property] = Reflect.ownKeys(value);
          master[property] = value[property];
          return;
        }
        validateCellStyle(value, true);
        const edits = Reflect.ownKeys(value).map((name) => cellStyleEncoders[name](value[name]));
        const state = check();
        const merge = worksheet.#linkedMergeStyles.size ? worksheet.#mergeAt(row, column, true) : undefined;
        const detached = merge && worksheet.#linkedMergeStyles.get(merge.ref);
        if (!edits.length) edits.push(['set_cell_style_index', 0]);
        worksheet.#writeDocument(state, (candidate) => {
          for (const [method, ...args] of edits) {
            rustCall('CELL_EDIT_REFUSED', () => candidate[method](sheetName(), row, column, ...args));
          }
          if (linked && detached && !detached.has(key)) {
            rustCall('CELL_EDIT_REFUSED', () =>
              candidate.sync_merge_style(sheetName(), merge.ref, row, column, [...detached]));
          }
        });
        if (!linked) {
          detached?.add(key);
          const direct = directMerge();
          if (direct) direct.linkedStyle = false;
        }
      };
      const readNote = (fullModel = false) => {
        const { document } = check();
        const fields = rustCall('CELL_READ_REFUSED', () => document.cell_note_runs(sheetName(), row, column));
        if (fields === undefined) return undefined;
        const runs = decodeDeclaredRichTextRuns(fields);
        if (!fullModel && runs.length === 1 && runs[0].font === undefined) return runs[0].text;
        return noteModel(runs, rustCall('CELL_READ_REFUSED', () => document.cell_note_box(sheetName(), row, column)));
      };
      this.#cells.set(key, {
        destroy() {
          destroyed = true;
          if (worksheet.#cells.get(key) === this) {
            (worksheet.#destroyedCellAddresses ??= new Set()).add(key);
          }
        },
        get address() { return destroyed ? undefined : key; },
        get model() { return projectCellModel(this, readNote); },
        set model(input) {
          const model = cellModelSettings(input);
          if (model.address !== undefined && model.address !== key) {
            throw failure('INVALID_ADDRESS', 'Cell model address must match the target');
          }
          const master = this.master;
          const merged = master !== this;
          if (merged ? model.type !== ValueType.Merge || model.master !== master.address : model.type === ValueType.Merge) {
            throw failure('UNSUPPORTED_OPTIONS', 'Cell model cannot change merge membership');
          }
          const replacement = prepareCellModel(model, merged);
          const state = check();
          const sheet = sheetName();
          const merge = worksheet.#linkedMergeStyles.size ? worksheet.#mergeAt(row, column, true) : undefined;
          worksheet.#writeDocument(state, candidate => rustCall('CELL_EDIT_REFUSED', () =>
            writeCellModel(candidate, sheet, row, column, replacement)));
          if (merge) worksheet.#linkedMergeStyles.get(merge.ref)?.add(key);
          if (!merged) setPresence(replacement.value !== null);
        },
        get row() { assertLive(); return rowHandle.number; },
        get col() { assertLive(); return column + 1; },
        get workbook() { assertLive(); return worksheet.workbook; },
        get fullAddress() {
          check();
          return { sheetName: sheetName(), address: key, row: row + 1, col: column + 1 };
        },
        get name() { return this.names[0]; },
        set name(value) { this.names = [value]; },
        get names() { return this.workbook.definedNames.getNames(namedCell().range); },
        set names(value) { setNamedCellMembership(this.workbook.definedNames, namedCell(), value); },
        addName(name) { this.workbook.definedNames.add(namedCell().range, name); },
        removeName(name) { this.workbook.definedNames.remove(namedCell().range, name); },
        removeAllNames() { this.names = []; },
        get $col$row() { assertLive(); return '$' + encode_col(column) + '$' + (row + 1); },
        get isHyperlink() { return this.type === ValueType.Hyperlink; },
        get worksheet() { assertLive(); return rowHandle.worksheet; },
        addMergeRef() { check(); mergeReferences += 1; },
        releaseMergeRef() { check(); mergeReferences -= 1; },
        merge(master, ignoreStyle) {
          const state = check();
          if (master === this || master?.worksheet !== worksheet || worksheet.#cells.get(master.address) !== master ||
              worksheet.#mergeAt(row, column, true) || worksheet.#mergeAt(master.row - 1, master.col - 1, true)) {
            throw failure('MERGE_EDIT_REFUSED', 'Expected distinct cells outside worksheet merges on the same worksheet');
          }
          for (let ancestor = master; ancestor; ancestor = worksheet.#cellMerges?.get(ancestor.address)?.master) {
            if (ancestor === this) throw failure('MERGE_EDIT_REFUSED', 'Cell merge would create a cycle');
          }
          const previous = directMerge();
          worksheet.#writeDocument(state, candidate => rustCall('CELL_EDIT_REFUSED', () =>
            writeExcelValue(candidate, sheetName(), row, column, null)));
          previous?.master.releaseMergeRef();
          (worksheet.#cellMerges ??= new Map()).set(key, { master, follower: this, linkedStyle: !ignoreStyle });
          master.addMergeRef();
          setPresence(true);
        },
        unmerge() {
          const state = check();
          const merge = directMerge();
          if (!merge) return;
          const inherited = encodeDefaultStyle({
            ...worksheet.getColumn(column + 1).style,
            ...Object.fromEntries(Object.entries(rowHandle.style).filter(([, value]) => value !== undefined)),
          });
          worksheet.#writeDocument(state, candidate => rustCall('CELL_EDIT_REFUSED', () => {
            candidate.set_cell_style_index(sheetName(), row, column, 0);
            for (const [property, ...args] of inherited) {
              const method = property === 'number_format' ? 'set_number_format' : 'set_cell_' + property;
              candidate[method](sheetName(), row, column, ...args);
            }
          }));
          merge.master.releaseMergeRef();
          worksheet.#cellMerges.delete(key);
          setPresence(false);
        },
        get isMerged() {
          assertLive();
          return directMerge() !== undefined || worksheet.#mergeAt(row, column) !== undefined || mergeReferences > 0;
        },
        get master() {
          assertLive();
          const direct = directMerge();
          if (direct) return direct.master;
          const merged = worksheet.#mergeAt(row, column);
          return merged ? worksheet.getCell(merged.s.r + 1, merged.s.c + 1) : this;
        },
        isMergedTo(master) {
          const current = this.master;
          return current !== this && current === master;
        },
        get type() { return this.master === this ? typeOf(this.value) : ValueType.Merge; },
        get effectiveType() {
          return effectiveTypeOf(this.value);
        },
        get formulaType() {
          const { document, ignoredNodes } = check();
          if (ignoredCoveredCell(ignoredNodes)) return undefined;
          const [formula, , , , , , sharedFormula] = rustCall('CELL_READ_REFUSED',
            () => document.read_excel_formula(sheetName(), row, column));
          if (sharedFormula !== undefined) return FormulaType.Shared;
          return formula === undefined ? undefined : FormulaType.Master;
        },
        get text() {
          const value = this.value;
          return this.master === this ? textOf(value) : value.toString();
        },
        get html() {
          const text = this.text;
          const start = text.search(/["&<>]/);
          return start < 0 ? text : text.slice(0, start) +
            text.slice(start).replace(/[&<>"']/g, (character) => htmlEntities[character]);
        },
        toString() { return this.text; },
        toCsvString() { return this.master === this ? csvTextOf(this.value) : ''; },
        get formula() {
          const { document, ignoredNodes } = check();
          if (ignoredCoveredCell(ignoredNodes)) return undefined;
          return rustCall('CELL_READ_REFUSED', () => document.read_excel_formula(sheetName(), row, column)[0]);
        },
        get result() {
          return this.formulaType === undefined ? undefined : this.value.result;
        },
        get style() {
          if (destroyed) return undefined;
          const entries = Object.keys(cellStyleEncoders).map((name) => [name, this[name]]);
          return Object.fromEntries(entries.filter(([, value]) => value !== undefined));
        },
        set style(value) { writeStyle(value, false); },
        get border() {
          const master = styleMaster();
          if (master) return master.border;
          const { document } = check();
          return decodeBorder(rustCall('CELL_READ_REFUSED', () => document.cell_border(sheetName(), row, column)));
        },
        set border(value) { writeStyle({ border: value }, true); },
        get fill() {
          const master = styleMaster();
          if (master) return master.fill;
          const { document } = check();
          return decodeFill(rustCall('CELL_READ_REFUSED', () => document.cell_fill(sheetName(), row, column)));
        },
        set fill(value) { writeStyle({ fill: value }, true); },
        get font() {
          const master = styleMaster();
          if (master) return master.font;
          const { document } = check();
          return decodeDeclaredFont(rustCall('CELL_READ_REFUSED', () => document.cell_font(sheetName(), row, column)));
        },
        set font(value) { writeStyle({ font: value }, true); },
        get alignment() {
          const master = styleMaster();
          if (master) return master.alignment;
          const { document } = check();
          return decodeAlignment(rustCall('CELL_READ_REFUSED', () => document.cell_alignment(sheetName(), row, column)));
        },
        set alignment(value) { writeStyle({ alignment: value }, true); },
        get protection() {
          const master = styleMaster();
          if (master) return master.protection;
          const { document } = check();
          const flags = rustCall('CELL_READ_REFUSED', () => document.cell_protection(sheetName(), row, column));
          return decodeProtection(flags);
        },
        set protection(value) { writeStyle({ protection: value }, true); },
        get numFmt() {
          const master = styleMaster();
          if (master) return master.numFmt;
          const { document } = check();
          return rustCall('CELL_READ_REFUSED', () => document.number_format(sheetName(), row, column));
        },
        set numFmt(value) { writeStyle({ numFmt: value }, true); },
        get dataValidation() {
          const { document, ignoredNodes } = check();
          if (ignoredNodes?.has('dataValidations')) return undefined;
          return rustCall('CELL_READ_REFUSED', () => decodeValidation(document, document.cell_validation(sheetName(), row, column)));
        },
        set dataValidation(value) {
          const validation = encodeValidation(value);
          const state = check();
          worksheet.#writeDocument(state, (candidate) =>
            rustCall('CELL_EDIT_REFUSED', () => writeValidation(candidate, sheetName(), key, validation, true)));
        },
        get note() { return readNote(); },
        set note(value) {
          const note = prepareCellNote(value);
          const state = check();
          worksheet.#writeDocument(state, (candidate) =>
            rustCall('CELL_EDIT_REFUSED', () => writeCellNote(candidate, sheetName(), row, column, note)));
        },
        get hyperlink() {
          return this.value?.hyperlink;
        },
        get value() {
          const master = this.master;
          if (master !== this) return master.value;
          const { document, ignoredNodes } = check();
          if (ignoredCoveredCell(ignoredNodes)) return null;
          return rustCall('CELL_READ_REFUSED', () => {
            const name = sheetName();
            const value = document.probe_excel_scalar(name, row, column);
            if (Number.isNaN(value)) {
              const rejectScalar = () => document.read_scalar(name, row, column, false, true);
              const [type, ...fields] = document.read_excel_complex(name, row, column);
              if (type === 'date') {
                const date = dateFromIso(fields[0], document.date1904());
                return date === undefined ? rejectScalar() : date;
              }
              if (type === 'richText') return { richText: decodeDeclaredRichTextRuns(fields) };
              const projectResult = (scalar, errorCode, iso, millis) => {
                if (errorCode !== undefined) return { error: errorCode };
                if (iso !== undefined) return dateFromIso(iso, document.date1904());
                return millis === undefined ? scalar : new Date(millis);
              };
              if (type === undefined) return rejectScalar();
              if (type === 'arrayResult') {
                const [scalar, errorCode, iso, millis] = fields;
                const projected = projectResult(scalar, errorCode, iso, millis);
                return projected === undefined ? rejectScalar() : projected;
              }
              const [formula, result, code, cachedIso, ref, shareType, sharedFormula, millis] = fields;
              const value = sharedFormula === undefined ? { formula } : { sharedFormula };
              if (ref !== undefined) Object.assign(value, { ref, shareType });
              const projected = projectResult(result, code, cachedIso, millis);
              if (projected !== undefined) value.result = projected;
              return value;
            }
            const [hyperlink, tooltip, display] = ignoredNodes?.has('hyperlinks') ? [] : document.cell_hyperlink(name, row, column);
            if (hyperlink) {
              return { text: value == null ? display || hyperlink : String(value), hyperlink, ...(tooltip ? { tooltip } : {}) };
            }
            if (value === undefined) return { error: document.read_excel_error(name, row, column) };
            if (typeof value !== 'number') return value;
            const millis = document.excel_date_millis(name, row, column, value);
            return millis === undefined ? value : new Date(millis);
          });
        },
        set value(input) {
          const master = this.master;
          if (master !== this) {
            master.value = input;
            return;
          }
          const state = check();
          const sheet = sheetName();
          const value = excelValue(input);
          commitWorkbookEdit(state,
            candidate => writeExcelValue(candidate, sheet, row, column, value),
            error => failure('CELL_EDIT_REFUSED', `${sheet}!${key}: ${error.message}`, error));
          setPresence(value !== null);
        },
      });
    }
    return this.#cells.get(key);
  }
}

function parseWorkbookLoadOptions(options) {
  const { base64, maxRows, maxCols, ignoreNodes } = workbookLoadSettings(options);
  return { base64, maxRows, maxCols, ignoredNodes: new Set(ignoreNodes) };
}

function xlsxOutput(state, settings) {
  const { useSharedStrings, useStyles, zip } = settings;
  if (zip === undefined && useSharedStrings === undefined && useStyles !== false) {
    return state.bytes?.slice() ?? rustCall('WORKBOOK_SAVE_REFUSED', () => state.document.to_bytes());
  }
  const zipSettings = zipOutputSettings(zip);
  const { compressionOptions } = zipSettings;
  const deflateLevel = compressionOptions == null ? undefined
    : zipDeflateSettings(zipLevelSettings(compressionOptions)).deflateLevel;
  const compressed = zip === undefined ? undefined : zipCompressionSettings(zipSettings).compressed;
  return rustCall('WORKBOOK_SAVE_REFUSED', () => {
    if (useStyles === false) {
      return state.document.to_bytes_without_cell_styles(useSharedStrings, compressed, deflateLevel);
    }
    if (useSharedStrings !== undefined) {
      return state.document.to_bytes_with_string_storage(useSharedStrings, true, compressed, deflateLevel);
    }
    if (!compressed) return state.document.to_bytes_with_compression(false);
    if (deflateLevel !== undefined) return state.document.to_bytes_with_deflate_level(deflateLevel);
    return state.document.to_bytes_with_compression(true);
  });
}

function parseWorkbookViews(views) {
  if (!Array.isArray(views)) throw failure('INVALID_VALUE', 'Expected an array of workbook views');
  return Array.from(views, workbookViewSettings);
}

function writeWorkbookViews(document, views) {
  const values = views.flatMap((view) => workbookViewFields.map((key) => String(view[key] ?? '')));
  rustCall('WORKBOOK_EDIT_REFUSED', () => document.set_workbook_views(values));
}

function formatNamedReference(sheet, s, e) {
  if (sheet && !isWellFormedWorksheetName(sheet)) {
    throw failure('INVALID_ADDRESS', 'Named reference contains an invalid worksheet name');
  }
  const qualify = sheet ? (/^[A-Za-z0-9]+$/.test(sheet) ? sheet : `'${sheet.replaceAll("'", "''")}'`) + '!' : '';
  const absolute = ({ r, c }) => `$${encode_col(c)}$${r + 1}`;
  return qualify + absolute(s) + (s.r === e.r && s.c === e.c ? '' : ':' + absolute(e));
}

const matrixRangeScans = new WeakMap();

function matrixNamedRanges(matrix, escapeSheetNames = false) {
  matrix.forEach(cell => { cell.mark = true; });
  return matrix.map(cell => {
    if (!cell.mark) return undefined;
    const { sheetName, row, col } = cell;
    cell.mark = false;
    const claim = (r, c) => {
      const neighbor = matrix.findCellAt(sheetName, r, c);
      if (!neighbor?.mark) return false;
      neighbor.mark = false;
      return true;
    };
    let top = row;
    let bottom = row;
    while (claim(top - 1, col)) top--;
    while (claim(bottom + 1, col)) bottom++;
    const claimColumn = c => {
      const neighbors = [];
      for (let r = top; r <= bottom; r++) {
        const neighbor = matrix.findCellAt(sheetName, r, c);
        if (!neighbor?.mark) return false;
        neighbors.push(neighbor);
      }
      for (const neighbor of neighbors) neighbor.mark = false;
      return true;
    };
    let left = col;
    let right = col;
    while (claimColumn(left - 1)) left--;
    while (claimColumn(right + 1)) right++;
    const start = { r: top - 1, c: left - 1 };
    const end = { r: bottom - 1, c: right - 1 };
    if (escapeSheetNames || !sheetName || /^[A-Za-z0-9]+$/.test(sheetName)) {
      return formatNamedReference(sheetName, start, end);
    }
    return `'${sheetName}'!${formatNamedReference(undefined, start, end)}`;
  }).filter(range => range !== undefined);
}

function containsNamedCell({ sheet, s, e }, cell) {
  const { r, c } = cell.s;
  return sheet === cell.sheet && r >= s.r && r <= e.r && c >= s.c && c <= e.c;
}

function excludeNamedCell(reference, cell) {
  const { sheet, s, e } = reference;
  const { r, c } = cell.s;
  if (!containsNamedCell(reference, cell)) return [reference.range];

  return [
    [s, { r: r - 1, c: e.c }],
    [{ r, c: s.c }, { r, c: c - 1 }],
    [{ r, c: c + 1 }, { r, c: e.c }],
    [{ r: r + 1, c: s.c }, e],
  ].filter(([start, end]) => start.r <= end.r && start.c <= end.c)
    .map(([start, end]) => formatNamedReference(sheet, start, end));
}

function namedLocation(location) {
  if (typeof location === 'string') return location;
  if (!location || typeof location !== 'object') {
    throw failure('INVALID_ADDRESS', 'Expected a named cell location');
  }
  const { sheetName, top, left, bottom, right, row, col } = definedNameLocationSettings(location);
  const first = top === undefined ? checkedDefinedNameAddress(location.address) ?? gridAddress(row, col) : gridAddress(top, left);
  const last = top === undefined ? first : gridAddress(bottom, right);
  const [, firstRow, firstCol] = address(first.replaceAll('$', ''));
  const [, lastRow, lastCol] = address(last.replaceAll('$', ''));
  return formatNamedReference(sheetName, { r: firstRow, c: firstCol }, { r: lastRow, c: lastCol });
}

function singleNamedCell(location) {
  const [cell, extra] = parseNamedReferences(namedLocation(location));
  if (!cell || extra || cell.s.r !== cell.e.r || cell.s.c !== cell.e.c) {
    throw failure('INVALID_ADDRESS', 'Expected one cell reference');
  }
  return cell;
}

function eachNamedCell(ranges, callback) {
  const sheets = new Map();
  for (const range of ranges) {
    if (!sheets.has(range.sheet)) sheets.set(range.sheet, []);
    sheets.get(range.sheet).push(range);
  }
  for (const [sheetName, rectangles] of sheets) {
    const rows = [...new Set(rectangles.flatMap(({ s, e }) => [s.r, e.r + 1]))].sort((a, b) => a - b);
    const byColumn = rectangles.sort((a, b) => a.s.c - b.s.c);
    for (let index = 0; index + 1 < rows.length; index += 1) {
      const firstRow = rows[index];
      const columns = [];
      for (const { s, e } of byColumn) {
        if (firstRow < s.r || firstRow > e.r) continue;
        const last = columns.at(-1);
        if (last && s.c <= last[1] + 1) last[1] = Math.max(last[1], e.c);
        else columns.push([s.c, e.c]);
      }
      if (!columns.length) continue;
      for (let row = firstRow; row < rows[index + 1]; row += 1) {
        for (const [left, right] of columns) {
          for (let col = left; col <= right; col += 1) {
            callback({ sheetName, address: gridAddress(row + 1, col + 1), row: row + 1, col: col + 1 });
          }
        }
      }
    }
  }
}

function setNamedCellMembership(definedNames, cell, names) {
  if (!Array.isArray(names)) throw failure('INVALID_VALUE', 'Expected an array of names');
  const requested = new Map();
  for (const name of names) {
    checkedDefinedName(name);
    requested.set(name.toUpperCase(), name);
  }
  const model = definedNames.model;
  for (const entry of model) {
    const key = entry.name.toUpperCase();
    if (requested.delete(key)) {
      const covered = entry.ranges.some((range) => containsNamedCell(parseNamedReferences(range)[0], cell));
      if (!covered) entry.ranges.push(cell.range);
    } else {
      entry.ranges = entry.ranges.flatMap((range) => excludeNamedCell(parseNamedReferences(range)[0], cell));
    }
  }
  for (const name of requested.values()) model.push({ name, ranges: [cell.range] });
  definedNames.model = model;
}

function parseNamedReferences(formula) {
  const token = /\s*(?:('(?:[^']|'')+'|[^'!,\s]+)!)?(\$?[A-Z]{1,3}\$?[1-9][0-9]{0,6})(?::(\$?[A-Z]{1,3}\$?[1-9][0-9]{0,6}))?\s*(,|$)/giy;
  const refs = [];
  let offset = 0;
  while (offset < formula.length) {
    token.lastIndex = offset;
    const match = token.exec(formula);
    if (!match) return [];
    const [, label, first, last = first, comma] = match;
    offset = token.lastIndex;
    if (comma && offset === formula.length) return [];
    try {
      const [, firstRow, firstCol] = address(first.replaceAll('$', ''));
      const [, lastRow, lastCol] = address(last.replaceAll('$', ''));
      const s = { r: Math.min(firstRow, lastRow), c: Math.min(firstCol, lastCol) };
      const e = { r: Math.max(firstRow, lastRow), c: Math.max(firstCol, lastCol) };
      const sheet = label?.startsWith("'") ? label.slice(1, -1).replaceAll("''", "'") : label;
      if (sheet && /[\[\]:]/.test(sheet)) return [];
      const source = comma ? match[0].slice(0, -1) : match[0];
      refs.push({ sheet, s, e, source, range: formatNamedReference(sheet, s, e) });
    } catch (error) {
      if (error.code === 'INVALID_ADDRESS') return [];
      throw error;
    }
  }
  return refs;
}

function editWorkbookDocument(state, action) {
  if (state.stagedRevision !== undefined) {
    action(state.document);
    state.stagedRevision += 1;
    return;
  }
  commitWorkbookEdit(state, action);
}

function commitWorkbookEdit(state, action, mapError) {
  const candidate = rustCall('WORKBOOK_LOAD_REFUSED', () => state.document.fork());
  let bytes;
  try {
    action(candidate);
    bytes = mapError ? candidate.to_bytes() : rustCall('WORKBOOK_SAVE_REFUSED', () => candidate.to_bytes());
  } catch (error) {
    candidate.free();
    throw mapError ? mapError(error) : error;
  }
  const previous = state.document;
  state.document = candidate;
  state.bytes = bytes;
  previous.free();
}

export class Workbook {
  #state;
  #loadSequence = 0;
  #publishedLoad = 0;
  #sheets = new Map();
  #sheetsById = new Map();
  #firstSheet;
  #pendingFullCalcOnLoad = false;
  #calcProperties;
  #properties;
  #pendingDate1904 = false;
  #pendingCoreProperties = new Map();
  #definedNames;
  #pendingDefinedNames = [];
  #pendingViews;
  #pendingTheme;

  constructor() {
    this.csv = createCsv(this);
    const workbook = this;
    const matrices = new Map();
    this.#definedNames = {
      add: (location, name) => this.#addDefinedName(location, name),
      remove: (location, name) => this.#removeDefinedName(location, name),
      get model() {
        return workbook.#namedRanges().map(({ name, ranges }) => ({ name, ranges: ranges.map(({ range }) => range) }));
      },
      set model(value) {
        workbook.#replaceNamedRanges(value);
      },
      getRanges: (name, matrix) => ({
        name,
        ranges: matrix ? (matrixRangeScans.get(matrix)?.() ?? matrixNamedRanges(matrix)) :
          this.#rangesForName(name).map(({ range }) => range),
      }),
      getMatrix: (name) => {
        if (!matrices.has(name)) matrices.set(name, this.#definedNameMatrix(name));
        return matrices.get(name);
      },
      getNames: (location) => this.#getDefinedNames(location),
      getNamesEx: (location) => this.#getDefinedNames(location),
      forEach: (callback) => {
        for (const { name, ranges } of this.#namedRanges()) {
          eachNamedCell(ranges, (cell) => callback(name, cell));
        }
      },
      addEx: (location, name) => this.#addDefinedName(namedLocation(location), name),
      removeEx: (location, name) => this.#removeDefinedName(location, name),
      normaliseMatrix: (matrix, sheetName) => {
        matrix.forEachInSheet(sheetName, (cell, row, col) => {
          if (cell && (cell.row !== row || cell.col !== col)) {
            const address = gridAddress(row, col);
            Object.assign(cell, { row, col, address });
          }
        });
      },
      spliceRows: (sheet, start, count, insert) => this.#spliceNamedRanges(sheet, 'r', start, count, insert),
      spliceColumns: (sheet, start, count, insert) => this.#spliceNamedRanges(sheet, 'c', start, count, insert),
      [replaceCellNames]: (location, names) => setNamedCellMembership(this.#definedNames, singleNamedCell(location), names),
      removeAllNames: (location) => setNamedCellMembership(this.#definedNames, singleNamedCell(location), []),
    };
    for (const [key, kind] of Object.entries(corePropertyKinds)) {
      Object.defineProperty(this, key, {
        enumerable: true,
        get: () => {
          const value = this.#state
            ? rustCall('WORKBOOK_READ_REFUSED', () => this.#state.document.core_text_property(key))
            : this.#pendingCoreProperties.get(key);
          if (value === undefined) return undefined;
          if (kind === 'date') return new Date(value);
          return kind === 'revision' ? Number(value) : value;
        },
        set: (value) => {
          if (kind === 'date') {
            if (!isCorePropertyDate(value)) {
              throw failure('INVALID_VALUE', `Expected a valid Date for ${key}`);
            }
            value = Date.prototype.toISOString.call(value);
          } else if (kind === 'revision') {
            value = String(checkedWorkbookRevision(value));
          } else if (!isCorePropertyText(value)) {
            throw failure('INVALID_VALUE', `Expected a string for ${key}`);
          }
          if (!this.#state) {
            this.#pendingCoreProperties.set(key, value);
            return;
          }
          this.#editStructure(this.#state, (candidate) =>
            rustCall('WORKBOOK_EDIT_REFUSED', () => candidate.set_core_text_property(key, value)));
        },
      });
    }
    this.#properties = {
      get date1904() {
        const document = workbook.#state?.document;
        return document ? rustCall('WORKBOOK_READ_REFUSED', () => document.date1904()) : workbook.#pendingDate1904;
      },
      set date1904(value) { workbook.properties = { date1904: value }; },
    };
    this.#calcProperties = {
      get fullCalcOnLoad() {
        return (workbook.#state?.document.full_calc_on_load() ?? workbook.#pendingFullCalcOnLoad) || undefined;
      },
      set fullCalcOnLoad(value) { workbook.calcProperties = { fullCalcOnLoad: value }; },
    };
    this.xlsx = {
      load: async (input, options) => {
        const settings = parseWorkbookLoadOptions(options);
        const { base64 } = settings;
        const sequence = ++this.#loadSequence;
        let bytes;
        if (base64) {
          bytes = decodeBase64(input);
        } else if (typeof Blob !== 'undefined' && input instanceof Blob) {
          try {
            bytes = new Uint8Array(await Blob.prototype.arrayBuffer.call(input));
          } catch (error) {
            throw failure('WORKBOOK_LOAD_REFUSED', 'Unable to read the workbook Blob', error);
          }
        } else {
          bytes = copyInput(input);
        }
        return this.#loadBytes(bytes, settings, sequence);
      },
      read: async (stream, options) => {
        const settings = parseWorkbookLoadOptions(options);
        if (settings.base64) throw failure('UNSUPPORTED_OPTIONS', 'Base64 input requires xlsx.load');
        if (typeof ReadableStream === 'undefined' || !(stream instanceof ReadableStream)) {
          throw failure('INVALID_INPUT', 'Expected a ReadableStream for XLSX input');
        }
        const sequence = ++this.#loadSequence;
        let reader;
        let bytes;
        try {
          reader = stream.getReader();
          const chunks = [];
          let length = 0;
          for (;;) {
            const { value, done } = await reader.read();
            if (done) break;
            if (!(value instanceof Uint8Array)) {
              await reader.cancel().catch(() => {});
              throw failure('INVALID_INPUT', 'Expected Uint8Array chunks for XLSX input');
            }
            chunks.push(new Uint8Array(value));
            length += value.byteLength;
          }
          bytes = new Uint8Array(length);
          let offset = 0;
          for (const chunk of chunks) {
            bytes.set(chunk, offset);
            offset += chunk.length;
          }
        } catch (error) {
          if (error?.code === 'INVALID_INPUT') throw error;
          throw failure('WORKBOOK_LOAD_REFUSED', 'Unable to read the workbook stream', error);
        } finally {
          reader?.releaseLock();
        }
        return this.#loadBytes(bytes, settings, sequence);
      },
      write: async (stream, options) => {
        const bytes = await this.#outputWithImages(options);
        if (typeof WritableStream === 'undefined' || !(stream instanceof WritableStream)) {
          throw failure('INVALID_INPUT', 'Expected a WritableStream for XLSX output');
        }
        let writer;
        try {
          writer = stream.getWriter();
          await writer.write(bytes);
          await writer.close();
        } catch (error) {
          throw failure('WORKBOOK_SAVE_REFUSED', 'Unable to write the workbook stream', error);
        } finally {
          writer?.releaseLock();
        }
      },
      writeBuffer: async (options) => this.#outputWithImages(options),
    };
  }

  #applyPendingWorkbookSettings(document, errorCode, views = this.#pendingViews) {
    rustCall(errorCode, () => {
      for (const [key, value] of this.#pendingCoreProperties) document.set_core_text_property(key, value);
      if (this.#pendingDate1904) document.set_date1904(true);
      if (this.#pendingFullCalcOnLoad) document.set_full_calc_on_load(true);
      if (this.#pendingTheme !== undefined) document.set_theme_xml(this.#pendingTheme);
      if (views !== undefined) writeWorkbookViews(document, views);
      if (this.#pendingDefinedNames.length) document.set_global_defined_names(this.#pendingDefinedNames);
    });
  }

  async #outputWithImages(options) {
    const settings = workbookOutputSettings(options);
    const state = this.#state;
    validateWorkbookOutputSettings(settings);
    const sheets = this.worksheets;
    for (const sheet of sheets) sheet[assertLiveHandles]();
    if (!state) {
      const document = rustCall('WORKBOOK_CREATE_REFUSED', () => XlsxDocument.create_empty(Math.floor(Date.now() / 1000)));
      try {
        this.#applyPendingWorkbookSettings(document, 'WORKBOOK_SAVE_REFUSED');
        return xlsxOutput({ document }, settings);
      } finally {
        document.free();
      }
    }

    const registry = workbookImageRegistries.get(this);
    const filenames = new Set(registry?.media.flatMap(image => image?.filename === undefined ? [] : [image.filename]));
    if (!filenames.size && !sheets.some(sheet => sheet[pendingCellMerges])) return xlsxOutput(state, settings);

    const candidate = rustCall('WORKBOOK_LOAD_REFUSED', () => state.document.fork());
    try {
      for (const sheet of sheets) sheet[writePendingCellMerges](candidate);
      if (!filenames.size) return xlsxOutput({ document: candidate }, settings);
      const parts = Array.from(registry.fileParts);
      const contents = await Promise.all(Array.from(filenames, async filename =>
        [filename, await this[imageFileReader](filename)]));
      const byFilename = new Map(contents);
      const names = new Set(candidate.zip_entry_names());
      for (const [part, { filename, buffer }] of parts) {
        if (!names.has(part)) continue;
        const current = candidate.zip_entry_bytes(part);
        if (current.length !== buffer.length || current.some((byte, index) => byte !== buffer[index])) continue;
        rustCall('WORKBOOK_SAVE_REFUSED', () => candidate.replace_image_part(part, byFilename.get(filename)));
      }
      return xlsxOutput({ document: candidate }, settings);
    } finally {
      candidate.free();
    }
  }

  #loadBytes(bytes, { maxRows, maxCols, ignoredNodes }, sequence) {
    if (sequence < this.#publishedLoad) {
      throw failure('WORKBOOK_LOAD_SUPERSEDED', 'A newer workbook load has already completed');
    }
    const document = rustCall('WORKBOOK_LOAD_REFUSED', () => XlsxDocument.open_exceljs(bytes));
    return this.#publishDocument(document, bytes, { maxRows, maxCols, ignoredNodes }, sequence);
  }

  #publishDocument(document, bytes, { maxRows, maxCols, ignoredNodes }, sequence) {
    const state = { bytes, document, ignoredNodes, missingWorkbookViewsAreUndefined: true,
      stagedRevision: bytes === undefined ? 0 : undefined };
    let sheets;
    const sheetsById = new Map();
    let firstSheet;
    try {
      const ids = document.worksheet_ids();
      const names = document.worksheet_names();
      if (maxRows || maxCols) {
        for (const name of names) {
          rustCall('WORKBOOK_LOAD_REFUSED', () => document.check_grid_limits(name, maxRows, maxCols));
        }
      }
      sheets = new Map(names.map((name, index) => {
        const id = ids[index];
        const sheet = this.#worksheetHandle(name, id, state);
        if (id !== undefined) {
          sheetsById.set(id, sheet);
          if (!firstSheet || id <= firstSheet.id) firstSheet = sheet;
        }
        return [name, sheet];
      }));
    } catch (error) {
      document.free();
      throw error;
    }
    this.#state?.document.free();
    this.#publishedLoad = sequence;
    workbookImageRegistries.delete(this);
    this.#state = state;
    this.#sheets = sheets;
    this.#sheetsById = sheetsById;
    this.#firstSheet = firstSheet ?? sheets.values().next().value;
    return this;
  }

  get views() {
    if (!this.#state) return (this.#pendingViews ?? []).map((view) => ({ ...view }));
    const values = rustCall('WORKBOOK_READ_REFUSED', () => this.#state.document.workbook_views());
    if (!values.length && this.#state.missingWorkbookViewsAreUndefined) return undefined;
    const views = [];
    for (let offset = 0; offset < values.length; offset += workbookViewFields.length) {
      const [x, y, width, height, firstSheet, activeTab, visibility] = values.slice(offset, offset + workbookViewFields.length);
      const view = { x: Number(x || 0), y: Number(y || 0), width: Number(width || 25000),
        height: Number(height || 10000), visibility: visibility || 'visible' };
      if (firstSheet) view.firstSheet = Number(firstSheet);
      if (activeTab) view.activeTab = Number(activeTab);
      views.push(view);
    }
    return views;
  }

  set views(value) {
    const views = parseWorkbookViews(value);
    if (!this.#state) {
      this.#pendingViews = views;
      return;
    }
    this.#editStructure(this.#state, (candidate) => writeWorkbookViews(candidate, views));
    this.#state.missingWorkbookViewsAreUndefined = false;
  }

  get properties() { return this.#properties; }

  set properties(value) {
    const { date1904: enabled } = workbookPropertySettings(value);
    if (enabled === undefined) return;
    if (!this.#state) {
      this.#pendingDate1904 = enabled;
      return;
    }
    if (this.#properties.date1904 === enabled) return;
    this.#editStructure(this.#state, (candidate) =>
      rustCall('WORKBOOK_EDIT_REFUSED', () => candidate.set_date1904(enabled)));
  }

  get calcProperties() { return this.#calcProperties; }

  set calcProperties(value) {
    const { fullCalcOnLoad: enabled } = workbookCalculationSettings(value);
    if (enabled === undefined) return;
    if (!this.#state) {
      this.#pendingFullCalcOnLoad = enabled;
      return;
    }
    if (this.#state.document.full_calc_on_load() === enabled) return;
    this.#editStructure(this.#state, (candidate) =>
      rustCall('WORKBOOK_EDIT_REFUSED', () => candidate.set_full_calc_on_load(enabled)));
  }

  get media() {
    const registry = this.#state ? workbookImages(this, this.#state) : imageRegistry(this);
    return registry.media.map((_, index) => this.getImage(index));
  }

  get model() {
    const worksheets = this.worksheets.map(sheet => sheet.model);
    const media = this.media;
    let themes = !this.#state && this.#pendingTheme !== undefined ? { theme1: this.#pendingTheme } : undefined;
    if (this.#publishedLoad && this.#state && !this.#state.themesCleared) {
      const { document } = this.#state;
      themes = rustCall('WORKBOOK_READ_REFUSED', () => {
        const decoder = new TextDecoder();
        return Object.fromEntries(document.zip_entry_names().flatMap(part => {
          const match = /^xl\/theme\/([A-Za-z0-9]+)\.xml$/.exec(part);
          return match ? [[match[1], decoder.decode(document.zip_entry_bytes(part))]] : [];
        }));
      });
    }
    return {
      creator: this.creator || 'Unknown', lastModifiedBy: this.lastModifiedBy || 'Unknown',
      lastPrinted: this.lastPrinted, created: this.created, modified: this.modified,
      properties: { ...this.properties }, worksheets, sheets: worksheets.slice(),
      definedNames: this.#definedNames.model, views: this.views,
      company: this.company, manager: this.manager, title: this.title, subject: this.subject,
      keywords: this.keywords, category: this.category, description: this.description,
      language: this.language, revision: this.revision, contentStatus: this.contentStatus,
      themes, media, calcProperties: { ...this.calcProperties },
    };
  }

  set model(input) {
    const model = workbookModelSettings(input);
    const stage = new Workbook();
    const plan = stage.#planModelWorksheets(model.worksheets, model.sheets);
    try {
      for (const key of Object.keys(corePropertyKinds)) {
        if (model[key] !== undefined) stage[key] = model[key];
      }
      if (model.properties !== undefined) stage.properties = model.properties;
      if (model.calcProperties !== undefined) stage.calcProperties = model.calcProperties;
      if (model.views !== undefined) stage.views = model.views;
      const theme = model.themes == null ? undefined : Object.getOwnPropertyDescriptor(model.themes, 'theme1')?.value;
      if (model.themes !== undefined && (model.themes === null || typeof model.themes !== 'object' ||
          Reflect.ownKeys(model.themes).some(key => key !== 'theme1') ||
          (Object.hasOwn(model.themes, 'theme1') && typeof theme !== 'string'))) {
        throw failure('UNSUPPORTED_OPTIONS', 'Expected a workbook model with at most theme1');
      }
      stage.#pendingTheme = theme;
      if (plan.length || theme !== undefined) stage.#initializeModelDocument(plan);
      const registry = imageRegistry(stage);
      const media = model.media ?? [];
      for (let index = 0; index < media.length; index += 1) {
        if (media[index] === undefined) continue;
        const { type, ...image } = media[index];
        if (type !== undefined && type !== 'image') throw failure('INVALID_VALUE', 'Invalid workbook media type');
        registry.media[index] = this.#prepareWorkbookImage(image);
      }
      registry.media.length = media.length;

      for (const { name, input } of plan) stage.getWorksheet(name).model = input;
      if (model.definedNames !== undefined) stage.definedNames.model = model.definedNames;
      this.#publishModel(stage);
    } finally {
      stage.#state?.document?.free();
    }
  }

  #planModelWorksheets(worksheets, order) {
    const reserved = new Set();
    const plan = Array.from(worksheets, input => {
      const { id, name } = worksheetModelSettings(input);
      checkedWorksheetModelId(id);
      if (id !== undefined && reserved.has(id)) throw failure('INVALID_VALUE', 'Duplicate workbook model worksheet ID');
      if (id !== undefined) reserved.add(id);
      return { id, name, input };
    });
    const names = new Set();
    let next = 1;
    for (const sheet of plan) {
      if (sheet.id === undefined) {
        while (reserved.has(next)) next += 1;
        sheet.id = next;
        reserved.add(next);
      }
      sheet.name = this.#sheetName(sheet.name, sheet.id);
      const key = sheet.name.toLowerCase();
      if (names.has(key)) throw failure('DUPLICATE_SHEET_NAME', `Worksheet name already exists: ${sheet.name}`);
      names.add(key);
    }
    const positions = new Map();
    for (const [index, entry] of (order ?? []).entries()) {
      const id = entry == null ? undefined : Object.getOwnPropertyDescriptor(entry, 'id')?.value;
      if (!reserved.has(id) || positions.has(id)) throw failure('INVALID_VALUE', 'Invalid workbook model sheet order');
      positions.set(id, index);
    }
    return plan.sort((a, b) => (positions.get(a.id) ?? -1) - (positions.get(b.id) ?? -1) || a.id - b.id);
  }

  #initializeModelDocument(plan) {
    let candidate = rustCall('WORKBOOK_CREATE_REFUSED', () => XlsxDocument.create(Math.floor(Date.now() / 1000)));
    try {
      if (this.#pendingTheme !== undefined) {
        rustCall('WORKBOOK_EDIT_REFUSED', () => candidate.set_theme_xml(this.#pendingTheme));
      }
      if (!plan.length) return;
      const initial = plan.find(({ id }) => id === 1);
      const names = new Set(plan.map(({ name }) => name.toLowerCase()));
      let template = '_model';
      for (let suffix = 1; names.has(template); suffix += 1) template = '_model' + suffix;
      rustCall('WORKBOOK_EDIT_REFUSED', () => {
        candidate.rename_initial_worksheet(initial?.name ?? template);
        for (const [index, { id, name }] of plan.entries()) {
          if (id !== 1) candidate.insert_worksheet(name, id, index);
        }
        if (!initial) candidate.remove_worksheet(template);
        for (const [key, value] of this.#pendingCoreProperties) candidate.set_core_text_property(key, value);
        if (this.#pendingDate1904) candidate.set_date1904(true);
        if (this.#pendingFullCalcOnLoad) candidate.set_full_calc_on_load(true);
      });
      if (this.#pendingViews !== undefined) writeWorkbookViews(candidate, this.#pendingViews);
      const document = candidate;
      candidate = undefined;
      this.#publishDocument(document, undefined, {}, ++this.#loadSequence);
    } finally {
      candidate?.free();
    }
  }

  #publishModel(stage) {
    const registry = imageRegistry(stage);
    if (stage.#state) {
      const { document } = stage.#state;
      const bytes = rustCall('WORKBOOK_SAVE_REFUSED', () => document.to_bytes());
      stage.#state.document = undefined;
      this.#publishDocument(document, bytes, {}, ++this.#loadSequence);
      registry.placements = new Map([...registry.placements].map(([sheet, placements]) =>
        [this.#sheetsById.get(sheet.id), placements]));
      registry.backgrounds = new Map([...registry.backgrounds].map(([sheet, imageId]) =>
        [this.#sheetsById.get(sheet.id), imageId]));
      registry.version = undefined;
    } else {
      this.#state?.document.free();
      this.#publishedLoad = ++this.#loadSequence;
      this.#state = undefined;
      this.#sheets = new Map();
      this.#sheetsById = new Map();
      this.#firstSheet = undefined;
    }
    this.#pendingCoreProperties = stage.#pendingCoreProperties;
    this.#pendingDate1904 = stage.#pendingDate1904;
    this.#pendingFullCalcOnLoad = stage.#pendingFullCalcOnLoad;
    this.#pendingDefinedNames = stage.#pendingDefinedNames;
    this.#pendingViews = stage.#pendingViews;
    this.#pendingTheme = stage.#pendingTheme;
    workbookImageRegistries.set(this, registry);
  }

  get definedNames() { return this.#definedNames; }

  #definedNameEntries() {
    if (!this.#state) return this.#pendingDefinedNames;
    return rustCall('WORKBOOK_READ_REFUSED', () => this.#state.document.global_defined_names());
  }

  #addDefinedName(location, name) {
    checkedDefinedName(name);
    const [reference, extra] = typeof location === 'string' ? parseNamedReferences(location) : [];
    if (!reference || extra) throw failure('INVALID_ADDRESS', 'Expected one named cell range');
    const entries = this.#definedNameEntries();
    let index = 0;
    while (index < entries.length && entries[index].toUpperCase() !== name.toUpperCase()) index += 2;
    const existing = entries[index + 1];
    const ranges = existing === undefined ? [] : parseNamedReferences(existing);
    if (existing !== undefined && !ranges.length) {
      throw failure('UNSUPPORTED_OPTIONS', 'Existing defined name is not a cell range');
    }
    if (ranges.some(({ sheet, s, e }) => sheet === reference.sheet &&
      s.r <= reference.s.r && s.c <= reference.s.c && e.r >= reference.e.r && e.c >= reference.e.c)) return;
    const formula = existing === undefined ? reference.range : existing + ',' + reference.range;
    this.#writeDefinedName(index, entries[index] ?? name, formula);
  }

  #getDefinedNames(location) {
    const cell = singleNamedCell(location);
    return this.#namedRanges().filter(({ ranges }) =>
      ranges.some((range) => containsNamedCell(range, cell))).map(({ name }) => name);
  }

  #removeDefinedName(location, name) {
    const cell = singleNamedCell(location);
    const entries = this.#definedNameEntries();
    for (let index = 0; index < entries.length; index += 2) {
      if (entries[index] !== name) continue;
      const ranges = parseNamedReferences(entries[index + 1]);
      if (!ranges.length) return;
      const remaining = ranges.flatMap((range) => excludeNamedCell(range, cell));
      if (remaining.length === ranges.length && remaining.every((range, i) => range === ranges[i].range)) return;
      this.#writeDefinedName(index, name, remaining.join(','));
      return;
    }
  }

  #writeDefinedName(index, name, formula) {
    if (!this.#state) {
      this.#pendingDefinedNames.splice(index, 2, ...(formula ? [name, formula] : []));
      return;
    }
    this.#editStructure(this.#state, (candidate) =>
      rustCall('WORKBOOK_EDIT_REFUSED', () => candidate.set_global_defined_name(name, formula)));
  }

  #spliceNamedRanges(sheet, axis, start, count, insert = 0, onlyName) {
    definedNameSpliceSettings({ start, count, insert });
    if (count === 0 && insert === 0) return;
    const first = start - 1;
    const after = first + count;
    const delta = insert - count;
    const entries = this.#definedNameEntries();
    const changes = [];
    const pending = [];
    for (let index = 0; index < entries.length; index += 2) {
      const [name, formula] = entries.slice(index, index + 2);
      const ranges = onlyName === undefined || name === onlyName ? parseNamedReferences(formula) : [];
      if (!ranges.some(({ sheet: owner, e }) => owner === sheet && e[axis] >= first)) {
        if (!this.#state) pending.push(name, formula);
        continue;
      }
      const next = ranges.flatMap(({ sheet: owner, s, e, source }) => {
        if (owner !== sheet || e[axis] < first) return [source];
        const sections = [];
        if (s[axis] < first) sections.push([s[axis], first - 1]);
        if (e[axis] >= after) {
          const shiftedStart = Math.max(s[axis], after) + delta;
          const shiftedEnd = e[axis] + delta;
          if (sections.length && insert === 0) sections[0][1] = shiftedEnd;
          else sections.push([shiftedStart, shiftedEnd]);
        }
        return sections.map(([from, to]) => {
          const end = { ...e, [axis]: to };
          gridAddress(end.r + 1, end.c + 1);
          return formatNamedReference(owner, { ...s, [axis]: from }, end);
        });
      }).join(',');
      if (next !== formula) changes.push(name, next);
      if (!this.#state && next) pending.push(name, next);
    }
    if (!changes.length) return;
    if (!this.#state) {
      this.#pendingDefinedNames = pending;
      return;
    }
    this.#writeDefinedNameChanges(changes);
  }

  #replaceNamedRanges(model) {
    const updated = new Map();
    for (const entry of checkedDefinedNamesModel(model)) {
      if (!entry || typeof entry !== 'object') {
        throw failure('INVALID_VALUE', 'Expected a defined name with an array of cell ranges');
      }
      const { name, ranges } = definedNameModelEntry(entry);
      checkedDefinedName(name);
      const key = name.toUpperCase();
      if (updated.has(key)) throw failure('INVALID_VALUE', 'Duplicate defined name');
      const normalized = new Set();
      for (const location of ranges) {
        const [reference, extra] = typeof location === 'string' ? parseNamedReferences(location) : [];
        if (!reference || extra) throw failure('INVALID_ADDRESS', 'Expected one named cell range');
        normalized.add(reference.range);
      }
      updated.set(key, [name, [...normalized].join(',')]);
    }

    if (!this.#state) {
      this.#pendingDefinedNames = [...updated.values()].filter(([, formula]) => formula).flat();
      return;
    }

    const entries = this.#definedNameEntries();
    const changes = [];
    for (let index = 0; index < entries.length; index += 2) {
      const [name, formula] = entries.slice(index, index + 2);
      const key = name.toUpperCase();
      if (!parseNamedReferences(formula).length) {
        if (updated.has(key)) throw failure('UNSUPPORTED_OPTIONS', 'Existing defined name is not a cell range');
        continue;
      }
      const next = updated.get(key)?.[1] ?? '';
      if (formula !== next) changes.push([name, next]);
      updated.delete(key);
    }
    for (const entry of updated.values()) {
      if (entry[1]) changes.push(entry);
    }
    if (changes.length) this.#writeDefinedNameChanges(changes.flat());
  }

  #writeDefinedNameChanges(changes) {
    this.#editStructure(this.#state, (candidate) => rustCall('WORKBOOK_EDIT_REFUSED', () =>
      candidate.set_global_defined_names(changes)));
  }

  #definedNameMatrix(name) {
    const readRanges = () => this.#rangesForName(name);
    let snapshot;
    const ranges = () => snapshot ?? readRanges();
    const cells = new Map();
    const getCell = (sheetName, row, col) => {
      const key = JSON.stringify([sheetName, row, col]);
      if (!cells.has(key)) cells.set(key, { sheetName, row, col, address: gridAddress(row, col) });
      return cells.get(key);
    };
    const forEach = callback => eachNamedCell(ranges(), cell =>
      callback(getCell(cell.sheetName, cell.row, cell.col)));
    const add = location => this.#addDefinedName(namedLocation(location), name);
    const find = (location, create = false) => {
      const cell = singleNamedCell(location);
      if (!ranges().some(range => containsNamedCell(range, cell))) {
        if (!create) return undefined;
        add(location);
      }
      const row = cell.s.r + 1;
      const col = cell.s.c + 1;
      return getCell(cell.sheet, row, col);
    };
    const matrix = {
      addCell: add,
      addCellEx: add,
      findCell: location => find(location),
      findCellEx: find,
      findCellAt: (sheetName, row, col) => {
        if (!isWorksheetGridRow(row) || !isWorksheetGridColumn(col)) return undefined;
        const cell = { sheet: sheetName, s: { r: row - 1, c: col - 1 } };
        return ranges().some(range => containsNamedCell(range, cell)) ? getCell(sheetName, row, col) : undefined;
      },
      getCell: location => find(location, true),
      getCellEx: location => find(location, true),
      getCellAt: (sheetName, row, col) => find({ sheetName, row, col }, true),
      removeCellEx: location => this.#removeDefinedName(location, name),
      spliceRows: (sheet, start, count, insert) => this.#spliceNamedRanges(sheet, 'r', start, count, insert, name),
      spliceColumns: (sheet, start, count, insert) => this.#spliceNamedRanges(sheet, 'c', start, count, insert, name),
      forEach,
      forEachInSheet: (sheetName, callback) => eachNamedCell(
        ranges().filter(range => range.sheet === sheetName), cell => callback(getCell(sheetName, cell.row, cell.col), cell.row, cell.col)),
      map: callback => {
        const result = [];
        forEach(cell => result.push(callback(cell)));
        return result;
      },
    };
    matrixRangeScans.set(matrix, () => {
      const previous = snapshot;
      snapshot = ranges();
      try {
        return matrixNamedRanges(matrix, true);
      } finally {
        snapshot = previous;
      }
    });
    return matrix;
  }

  #rangesForName(name) {
    if (typeof name !== 'string') return [];
    if (this.#state) {
      const formulas = rustCall('WORKBOOK_READ_REFUSED', () =>
        this.#state.document.global_defined_name_formulas(name));
      for (const formula of formulas) {
        const ranges = parseNamedReferences(formula);
        if (ranges.length) return ranges;
      }
    } else {
      const entries = this.#pendingDefinedNames;
      for (let index = 0; index < entries.length; index += 2) {
        if (entries[index] !== name) continue;
        const ranges = parseNamedReferences(entries[index + 1]);
        if (ranges.length) return ranges;
      }
    }
    return [];
  }

  #namedRanges() {
    const entries = this.#definedNameEntries();
    const names = [];
    for (let index = 0; index < entries.length; index += 2) {
      const ranges = parseNamedReferences(entries[index + 1]);
      if (ranges.length) names.push({ name: entries[index], ranges });
    }
    return names;
  }

  addImage(image) {
    const media = this.#prepareWorkbookImage(image);
    const registry = this.#state ? workbookImages(this, this.#state) : imageRegistry(this);
    return registry.media.push(media) - 1;
  }

  #prepareWorkbookImage(image) {
    if (image?.filename !== undefined && this[prepareImageFile]) return this[prepareImageFile](image);
    const { extension, buffer: bytes, base64: encoded } = workbookImageSettings(image);
    const base64 = typeof encoded === 'string' ? encoded.replace(/^data:image\/[^;,]+;base64,/i, '') : encoded;
    const buffer = base64 === undefined ? copyInput(bytes) : decodeBase64(base64);
    return { extension, buffer };
  }

  getImage(imageId) {
    const image = this[imageForPlacement](imageId);
    if (image?.filename !== undefined) {
      const { filename, extension, type } = image;
      return { type, extension, filename };
    }
    return image;
  }

  [imageForPlacement](imageId) {
    if (!Number.isInteger(imageId) || imageId < 0) return undefined;
    const registry = this.#state ? workbookImages(this, this.#state) : imageRegistry(this);
    const media = registry.media[imageId];
    if (!media) return undefined;
    const buffer = media.buffer?.slice() ?? rustCall('WORKBOOK_READ_REFUSED', () =>
      media.background ? this.#state.document.worksheet_background_bytes(media.sheet) :
        this.#state.document.worksheet_image_bytes(media.sheet, media.index));
    return { type: 'image', extension: media.extension, buffer, ...(media.filename === undefined ? {} : { filename: media.filename }) };
  }

  clearThemes() {
    if (!this.#state) return;
    this.#editStructure(this.#state, (candidate) =>
      rustCall('WORKBOOK_EDIT_REFUSED', () => candidate.reset_theme()));
    this.#state.themesCleared = true;
  }

  get nextId() {
    let id = 1;
    while (this.#sheetsById.has(id)) id += 1;
    return id;
  }

  [applyStreamingHyperlinks](sheet, links) {
    if (!links.length) return;
    this.#editStructure(this.#state, candidate => {
      for (const { address, hyperlink, tooltip } of links) {
        rustCall('HYPERLINK_EDIT_REFUSED', () => candidate.set_cell_hyperlink(sheet, address, hyperlink, undefined, tooltip));
      }
    });
  }

  [registerStreamingStyle](style, cache, date) {
    const fields = encodeTableColumnStyle(style);
    if (date && (style === undefined || data(style, 'numFmt') === undefined)) {
      fields.push('number_format', '1', 'mm-dd-yy');
    }
    if (!fields.length) return 0;
    const key = JSON.stringify(fields);
    if (!cache.has(key)) {
      const state = this.#state;
      const index = rustCall('STYLE_EDIT_REFUSED', () => state.document.register_streaming_style(fields));
      state.bytes = undefined;
      cache.set(key, index);
    }
    return cache.get(key);
  }

  addWorksheet(name, options) {
    if (typeof options === 'string') {
      options = { properties: { tabColor: { argb: options } } };
    } else if (options && ['argb', 'theme', 'indexed'].some((key) => Object.hasOwn(options, key))) {
      options = { properties: { tabColor: options } };
    }
    const settings = worksheetSettings(options);
    const properties = settings.properties === undefined ? undefined : prepareWorksheetProperties(settings.properties);
    const pageSetup = settings.pageSetup === undefined ? undefined : parsePageSetup(settings.pageSetup, true);
    const headerFooter = settings.headerFooter === undefined ? undefined : headerFooterSettings(settings.headerFooter);
    const filter = filterRange(settings.autoFilter);
    const views = settings.views === undefined ? undefined : parseViews(settings.views);
    const requestedState = checkedWorksheetCreationState(settings.state);
    const id = this.nextId;
    name = this.#sheetName(name, id);

    const current = this.#state;
    const candidate = current
      ? rustCall('WORKBOOK_LOAD_REFUSED', () => current.document.fork())
      : rustCall('WORKBOOK_CREATE_REFUSED', () => XlsxDocument.create(Math.floor(Date.now() / 1000)));
    let bytes;
    try {
      if (current) {
        const persistedId = current.hasRemovedWorksheet ? undefined : id;
        rustCall('WORKSHEET_CREATE_REFUSED', () => candidate.append_worksheet(name, persistedId));
      } else {
        rustCall('WORKSHEET_CREATE_REFUSED', () => candidate.rename_initial_worksheet(name));
        this.#applyPendingWorkbookSettings(candidate, 'WORKBOOK_EDIT_REFUSED', this.#pendingViews ?? []);
      }
      if (requestedState !== undefined && requestedState !== 'visible') {
        rustCall('WORKSHEET_CREATE_REFUSED', () => candidate.set_sheet_state(name, requestedState));
      }
      if (filter !== undefined) {
        rustCall('AUTOFILTER_EDIT_REFUSED', () => candidate.set_autofilter(name, filter));
      }
      writeViews(candidate, name, views ?? []);
      if (pageSetup !== undefined) writePageSetup(candidate, name, pageSetup);
      if (headerFooter !== undefined) writeHeaderFooter(candidate, name, headerFooter);
      if (properties) writeWorksheetProperties(candidate, name, properties, true);
      bytes = rustCall('WORKBOOK_SAVE_REFUSED', () => candidate.to_bytes());
    } catch (error) {
      candidate.free();
      throw error;
    }
    const state = current ?? {};
    state.document?.free();
    state.document = candidate;
    state.bytes = bytes;
    this.#state = state;

    const sheet = this.#worksheetHandle(name, id, state, true);
    this.#sheets.set(name, sheet);
    this.#sheetsById.set(id, sheet);
    if (!this.#firstSheet || this.#firstSheet.id === undefined || id < this.#firstSheet.id) this.#firstSheet = sheet;
    return sheet;
  }

  #sheetName(name, id, current) {
    if (name === undefined) name = `sheet${id}`;
    if (current && name === current.name) return name;
    if (typeof name !== 'string') throw new Error('The name has to be a string.');
    if (!name) throw new Error("The name can't be empty.");
    if (name === 'History' || !hasAllowedWorksheetNamePunctuation(name) || !hasAllowedWorksheetNameEdges(name)) {
      throw failure('INVALID_SHEET_NAME', `Invalid worksheet name: ${name}`);
    }
    name = name.slice(0, name.codePointAt(30) > 0xffff ? 30 : 31);
    if (!isWellFormedWorksheetName(name)) {
      throw failure('INVALID_SHEET_NAME', 'Worksheet name contains an unpaired surrogate');
    }
    const folded = name.toLowerCase();
    for (const existing of this.#sheets.keys()) {
      if (existing.toLowerCase() === folded) {
        throw failure('DUPLICATE_SHEET_NAME', `Worksheet name already exists: ${name}`);
      }
    }

    return name;
  }

  #worksheetHandle(name, id, state, fresh = false) {
    const check = () => {
      if (this.#state !== state || this.#sheets.get(sheet.name) !== sheet) {
        throw failure('STALE_HANDLE', 'Worksheet belongs to a previous load or was removed');
      }
      return state;
    };
    const sheet = new Worksheet(this, name, id, check, (requested) => {
      check();
      const oldName = sheet.name;
      const nextName = this.#sheetName(requested, id, sheet);
      if (nextName === oldName) return undefined;
      return {
        name: nextName,
        publish: () => {
          this.#sheets = new Map([...this.#sheets].map(([key, value]) =>
            [value === sheet ? nextName : key, value]));
        },
      };
    }, (requested) => {
      check();
      if (!isWorksheetState(requested)) {
        throw failure('INVALID_VALUE', 'Invalid worksheet state');
      }
      if (requested === sheet.state) return;
      this.#editStructure(state, (candidate) =>
        rustCall('WORKSHEET_STATE_REFUSED', () => candidate.set_sheet_state(sheet.name, requested)));
    }, fresh);
    return sheet;
  }

  #editStructure(state, action) {
    editWorkbookDocument(state, action);
  }

  removeWorksheet(id) {
    const sheet = this.getWorksheet(id);
    if (sheet) this.removeWorksheetEx(sheet);
  }

  removeWorksheetEx(sheet) {
    if (!sheet || this.#sheets.get(sheet.name) !== sheet) {
      throw failure('STALE_HANDLE', 'Worksheet belongs to another workbook or was removed');
    }
    this.#editStructure(this.#state, (candidate) =>
      rustCall('WORKSHEET_REMOVE_REFUSED', () => candidate.remove_exceljs_worksheet(sheet.name)));
    this.#state.hasRemovedWorksheet = true;
    this.#sheets.delete(sheet.name);
    this.#sheetsById.delete(sheet.id);
    if (this.#firstSheet === sheet) {
      this.#firstSheet = undefined;
      for (const entry of this.#sheetsById.values()) {
        if (!this.#firstSheet || entry.id < this.#firstSheet.id) this.#firstSheet = entry;
      }
      this.#firstSheet ??= this.#sheets.values().next().value;
    }
  }

  get worksheets() {
    return [...this.#sheets.values()];
  }

  getWorksheet(id) {
    if (id === undefined) return this.#firstSheet;
    return typeof id === 'number' ? this.#sheetsById.get(id) : this.#sheets.get(id);
  }

  eachSheet(iteratee) {
    this.worksheets.forEach((sheet) => iteratee(sheet, sheet.id));
  }
}
