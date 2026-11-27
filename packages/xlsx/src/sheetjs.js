import { codepageTable, set_cptable } from './sheetjs-codepage.js';
export { set_cptable };
import { decodeBookProperties } from './sheetjs-book-properties.js';
import { projectedZipFiles } from './sheetjs-zip-files.js';
import { marginEdits, marginKeys, readAutofilter } from './sheetjs-print-settings.js';
import { columnCharacters, columnPixels, columnWidth, defaultDigitWidth, inferDigitWidth } from './sheetjs-column-width.js';
import { dateFromIso, dateFromSerial, isoCellText, serialFromDate } from './sheetjs-date.js';
import { address, binaryStringBytes, copyInput, data, decodeBase64, failure, rustCall, scalar } from './contract.js';
import { decode_range, encode_cell, encode_range } from './sheetjs-address.js';
import { errorCode, errorName, readDelimited } from './sheetjs-csv-input.js';
import { formatted, numberFormatCode } from './sheetjs-format.js';
import { plainStringHtml, richStringHtml } from './sheetjs-cell-html.js';
import { sheet_to_csv, sheet_to_html, tabSeparatedText } from './sheetjs-utils.js';
import { book_append_sheet, book_new } from './sheetjs-book.js';
import { htmlTables, table_to_sheet } from './sheetjs-dom.js';
import { XlsxDocument, XlsbArchive, biff_document_properties, read_ods_parts, write_ods, write_zip_entries } from './runtime.js';
import { isXlmlText, readXlml } from './sheetjs-xlml-read.js';
import { isDifText, readDif, writeDif } from './sheetjs-dif.js';
import { isFodsText, readFods, writeFods, writeOdfMetadata } from './sheetjs-fods.js';
import { isRtfText, readRtf, writeRtf } from './sheetjs-rtf.js';
import { isEthText, readEth, writeEth } from './sheetjs-eth.js';
import { isSylkText, readSylk, writeSylk } from './sheetjs-sylk.js';
import { compoundPart, compoundStream, compoundWorkbook, readXls, writeXls } from './sheetjs-xls.js';
import { readXlsb } from './sheetjs-xlsb.js';
import { checkLoadedWorkbookMetadata, projectedSheetEntries, projectedWorkbookViews, workbookNamespace, workbookRightToLeft } from './sheetjs-workbook-metadata.js';
import { asyncFileOptionFields, byteReadOptionFields, binaryOutputMime, compoundBiffVersions, earlyBiffVersions, writeFileOptionFields, checkedAsyncFileCallback, checkedDownloadFilename, checkedLegacyComments, checkedMergeList, checkedZipEntryName, compoundContainerSettings, compoundReadSettings, dbfReadSettings, documentOutputMime, documentPropertyOverrideModes, earlyBiffReadSettings, fodsWriteSettings, inferFileOutputFormat, isWellFormedWorksheetName, isBiffDateProperty, isBiffListProperty, isBiffDerivedProperty, isDocumentDateProperty, isValidDocumentPropertyDate, isXlsbDerivedProperty, writableProperties, isWorkbookVisibilityMetadata, isWorksheetColumn, isWorksheetRow, isWorksheetVisibility, isWorksheetVisibilityEntry, isWritableNumberFormat, isXlmlImportedCellShape, isXlsxCellShape, lotusReadSettings, odsWriteSettings, readInputSettings, resolveAsyncWriteFormat, resolveWriteFormat, textOutputMime, xmlWorkbookMime, textReadSettings, textWriteSettings, validateWorksheetColumnLayout, validateWorksheetRowLayout, validateWriteOutputType, workbookDateSystemSettings, worksheetVisibilityStates, worksheetColumnSizeKeys, worksheetRowSizeKeys,
  xlsbReadSettings, xlsbWriteSettings, xlmlWriteSettings, xlsxCellCommentSettings, xlsxCellHyperlinkSettings, xlsxDefinedNameSettings, xlsxReadSettings, xlsxSheetProjectionSettings, xlsxWorkbookProjectionSettings, xlsxWorkbookResourceSettings, xlsxWriteSettings } from './sheetjs-options.generated.js';
import { xlsbParts } from './sheetjs-xlsb-write.js';
import { isEarlyBiff, readEarlyBiff, writeEarlyBiff } from './sheetjs-biff.js';
import { isDbf, readDbf, writeDbf } from './sheetjs-dbf.js';
import { isWk1, isWk3, readWk1, readWk3, writeWk1, writeWk3 } from './sheetjs-lotus.js';
import { xlmlChunks } from './sheetjs-xlml.js';

import * as utils from './sheetjs-public-utils.js';
import * as stream from './sheetjs-stream.js';
import * as SSF from './sheetjs-ssf.js';

export { utils, stream, SSF };
export const version = '0.20.3';

const states = new WeakMap();

function rangeContains(reference, row, column) {
  const [first, last = first] = reference.split(':');
  const [, top, left] = address(first);
  const [, bottom, right] = address(last);
  return row >= top && row <= bottom && column >= left && column <= right;
}

function cellType(value) {
  if (typeof value === 'number') return 'n';
  return typeof value === 'boolean' ? 'b' : 's';
}

function unsupported(name, coordinate) {
  return failure('UNSUPPORTED_CELL', `${name}!${coordinate} cannot be edited as a scalar`);
}

function withDocument(bytes, action) {
  const document = rustCall('WORKBOOK_LOAD_REFUSED', () => new XlsxDocument(bytes));
  try {
    return action(document);
  } finally {
    document.free();
  }
}

function projectedHyperlinks(entries) {
  const links = [];
  for (let index = 0; index < entries.length; index += 4) {
    const [ref, Target, tooltip, display] = entries.slice(index, index + 4);
    // CE 0.20.3 keeps the element's attributes. Rust joins the relationship
    // target and location, so only a location-only link (`#location`, no
    // second `#`) is told apart here; r:id, Rel of external links and other
    // raw attributes are not exposed by Wasm yet.
    const internal = Target.startsWith('#') && !Target.includes('#', 1);
    const link = internal ? { ref, location: Target.slice(1) } : { ref };
    if (display) link.display = display.slice(1);
    link.Target = Target;
    if (internal) link.Rel = { Target, TargetMode: 'Internal' };
    if (tooltip) link.Tooltip = tooltip;
    links.push({ ...decode_range(ref), link });
  }
  return links;
}

function extendRange(range, row, column) {
  range.s.r = Math.min(range.s.r, row);
  range.s.c = Math.min(range.s.c, column);
  range.e.r = Math.max(range.e.r, row);
  range.e.c = Math.max(range.e.c, column);
}

// One object per hyperlink element, shared by every cell of its range as in
// CE 0.20.3; `entry.link` stays the unexposed baseline for edit detection.
function sharedLink(entry) {
  entry.view ??= { ...entry.link, ...(entry.link.Rel ? { Rel: { ...entry.link.Rel } } : {}) };
  return entry.view;
}

function applyMargins(document, name, margins) {
  if (margins) rustCall('SHEET_LAYOUT_EDIT_REFUSED', () => document.set_margins(name, margins));
}

function sheetLayout(fields) {
  const rows = [];
  const columns = [];
  let digits;
  for (let offset = 0; offset < fields.length; offset += 6) {
    const [kind, first, last, size, hidden, outline] = fields.slice(offset, offset + 6);
    const column = kind === 'column';
    const target = column ? columns : rows;
    const start = Number(first);
    const end = Number(last);
    if (!Number.isInteger(start) || !Number.isInteger(end) || start < 1 || end < start ||
        end > (column ? 16384 : 1048576)) continue;
    const metadata = {};
    if (column) {
      // CE 0.20.3 order: the <col> attributes (width as a number, null when absent
      // after its JSON copy; hidden parsed when present; outlineLevel raw with its
      // lowercase alias, then level), then the derived sizes. Other raw attributes
      // (customWidth, style, bestFit...) are not exposed by Wasm.
      const width = Number(size);
      metadata.width = size === '' || Number.isNaN(width) ? null : width;
      if (hidden !== '') metadata.hidden = hidden === '1' || hidden === 'true';
      if (outline !== '') {
        Object.assign(metadata, { outlineLevel: outline, outlinelevel: outline, level: Number(outline) || 0 });
      }
      if (metadata.width) {
        digits ??= inferDigitWidth(metadata.width);
        metadata.wpx = columnPixels(metadata.width, digits);
        metadata.wch = columnCharacters(metadata.wpx, digits);
        metadata.MDW = digits;
      }
    } else {
      if (size !== '') Object.assign(metadata, { hpt: Number(size), hpx: Number(size) });
      if (hidden === '1' || hidden === 'true') metadata.hidden = true;
      if (outline !== '') metadata.level = Number(outline);
    }
    for (let index = start - 1; index < end; index += 1) target[index] = { ...metadata };
  }
  return { rows: rows.length ? rows : undefined, columns: columns.length ? columns : undefined };
}

function tintedColor(rgb, tint) {
  if (!tint) return rgb;
  const channels = rgb.match(/../g).map((channel) => Number.parseInt(channel, 16) / 255);
  const min = Math.min(...channels);
  const max = Math.max(...channels);
  const lightness = (min + max) / 2;
  const adjusted = tint < 0 ? lightness * (1 + tint) : 1 - (1 - lightness) * (1 - tint);
  const scale = max === min ? 0 : Math.min(adjusted, 1 - adjusted) / Math.min(lightness, 1 - lightness);
  return channels.map((channel) => Math.round((adjusted + (channel - lightness) * scale) * 255)
    .toString(16).padStart(2, '0')).join('').toUpperCase();
}

function projectedFill(fields, palette, rawThemeColors) {
  const [patternType, foreground, background] = fields;
  if (patternType === undefined) return undefined;
  if (patternType === 'angle' || patternType === 'path') return Object.freeze({});
  function color(encoded, isBackground) {
    if (!encoded) return undefined;
    const [kind, value, tint] = encoded.split(':');
    if (kind === 'rgb') return Object.freeze({ rgb: value.slice(-6) });
    if (kind === 'theme') {
      const theme = Number(value);
      const adjustment = Number(tint);
      const result = { theme };
      if (adjustment) result.tint = adjustment;
      if (theme && palette[theme]) {
        result.rgb = tintedColor(palette[theme], adjustment);
        // 0.20.3 adds the untinted theme color under WTF.
        if (rawThemeColors) result.raw_rgb = palette[theme];
      }
      return Object.freeze(result);
    }
    if (kind === 'indexed' && isBackground) return Object.freeze({ indexed: Number(value) });
    return Object.freeze({});
  }
  const fill = { patternType };
  if (foreground) fill.fgColor = color(foreground, false);
  if (background) fill.bgColor = color(background, true);
  return Object.freeze(fill);
}

function projectFormulaValue([scalar, error, isoText], options) {
  const isoDate = dateFromIso(isoText, options.date1904);
  const value = isoDate ? serialFromDate(isoDate.getTime(), options.date1904) : scalar;
  const date = options.cellDates ? isoDate ?? (
    typeof value === 'number' && options.isDateStyle(options.style)
      ? dateFromSerial(value, options.date1904) : undefined) : undefined;
  const cell = { t: error !== undefined ? 'e' : date ? 'd' : value === undefined ? 'n' : cellType(value),
    v: error !== undefined ? errorCode(error) : date ?? value };
  if (cell.t === 's' && options.xml !== undefined) cell.r = options.xml;
  if (options.cellHTML && cell.t === 's') cell.h = plainStringHtml(cell.v);
  if (options.fill !== undefined) cell.s = options.fill;
  const { z, displayFormat } = options.formatForStyle(options.style);
  if (options.cellNF) cell.z = z;
  if (options.cellText && error !== undefined) cell.w = error;
  if (options.cellText && value !== undefined) projectCellDisplay(cell, displayFormat, options);
  return cell;
}

function projectFormula(formula, result, options) {
  const cell = projectFormulaValue(result, options);
  if (options.cellFormula) cell.f = options.xlfn ? formula : formula.replaceAll('_xlfn.', '');
  return cell;
}

function projectCellDisplay(cell, displayFormat, options) {
  try {
    cell.w = formatted({ ...cell, z: displayFormat }, cell.v, options);
  } catch (error) {
    if (error.code !== 'UNSUPPORTED_FORMAT') throw error;
  }
}

function projectArrayResult(key, array, result, options) {
  const cell = projectFormulaValue(result, options);
  if (options.cellFormula) {
    cell.F = array.range;
    if (key === array.anchor) cell.f = options.xlfn ? array.formula : array.formula.replaceAll('_xlfn.', '');
  }
  if (key === array.anchor && array.dynamic) cell.D = true;
  return cell;
}

function projectMetadataEditableCell(storage, slot, cell, comments, linked) {
  const fields = Object.keys(cell);
  for (const field of fields) {
    Object.defineProperty(cell, field, { writable: false, configurable: false });
  }
  if (comments) cell.c = copyComments(comments);
  if (linked) cell.l = sharedLink(linked);
  const get = () => cell;
  Object.defineProperty(storage, slot, { enumerable: true, configurable: true, get });
  return { get, metadataCell: cell, fieldCount: fields.length, comments, linked };
}

// Rich text and strings with phonetic data: the text and declared runs come
// from Rust, `r` is the stored shared-string markup (inline strings have none).
function projectRichText(document, name, row, column, options) {
  let fields;
  try {
    fields = document.sheetjs_string_cell(name, row, column);
  } catch {
    return undefined;
  }
  if (!fields.length) return undefined;
  const value = fields[0];
  const cell = { t: 's', v: value };
  if (options.fill !== undefined) cell.s = options.fill;
  if (options.xml !== undefined) cell.r = options.xml;
  if (options.cellText) cell.w = value;
  if (options.cellHTML) cell.h = fields.length > 1 ? richStringHtml(fields, 1) : plainStringHtml(value);
  if (options.z !== undefined) cell.z = options.z;
  return cell;
}

// Copies projected comments; the array keeps the VML visibility as `hidden`.
function copyComments(comments) {
  const copy = comments.map(comment => ({ ...comment }));
  if (comments.hidden !== undefined) copy.hidden = comments.hidden;
  return copy;
}

// Legacy notes as CE 0.20.3 projects them: author, text, stored `text`
// markup, T:false and, with cellHTML, nonempty HTML; plus VML visibility.
function projectedNotes(document, name, cellHTML) {
  const notes = new Map();
  const fields = document.sheet_notes(name);
  for (let index = 0; index < fields.length;) {
    const [key, a, t, xml, runCount] = fields.slice(index, index + 5);
    const runs = index + 5;
    index = runs + runCount * 14;
    const comment = { a, t, r: xml ?? '', T: false };
    if (cellHTML) {
      const h = runCount ? richStringHtml(fields, runs, index) : plainStringHtml(t);
      if (h) comment.h = h;
    }
    const entries = notes.get(key);
    if (entries) entries.push(comment);
    else notes.set(key, [comment]);
  }
  // VML Note shapes without a comment become plain stubs in CE 0.20.3.
  notes.vmlOnly = [];
  if (notes.size) {
    const visibility = document.note_visibility(name);
    for (let index = 0; index < visibility.length; index += 2) {
      const entries = notes.get(visibility[index]);
      if (entries) entries.hidden = !visibility[index + 1];
      else notes.vmlOnly.push(visibility[index]);
    }
  }
  return notes;
}

function project(bytes, name, { sheetRows, dense, cellNF, cellText, cellDates, sheetStubs, nodim, cellStyles, cellFormula, cellHTML, xlfn, dateNF, WTF }) {
  return withDocument(bytes, (document) => {
    // A usable declared extent (`A1:B2`); a missing or single-cell `dimension` is not one.
    const declared = document.sheet_range(name);
    const autofilter = document.autofilter(name);
    const margins = document.explicit_margins(name);
    const merges = document.merged_ranges(name);
    const ranges = document.guarded_ranges(name);
    // Merged cells keep their own values; only multi-cell formula results are unreadable.
    const resultRanges = document.formula_result_ranges(name);
    const arrays = [];
    const arrayFields = document.array_formulas(name);
    for (let index = 0; index < arrayFields.length; index += 4) {
      const [anchor, range, formula, dynamic] = arrayFields.slice(index, index + 4);
      arrays.push({ anchor, range, formula, dynamic: dynamic === 'true', ...decode_range(range) });
    }
    const date1904 = document.date1904();
    const numberFormats = cellNF || cellText;
    const formatStylesRequested = numberFormats || cellDates;
    const cells = document.sparse_cells(name, sheetStubs, nodim, Math.min(sheetRows, 1048576), formatStylesRequested, cellStyles);
    let coordinates;
    let values;
    let specialValues;
    let formulaValues;
    let formatStyles;
    let fillStyles;
    let sharedStrings;
    let extent;
    try {
      extent = cells.take_extent();
      coordinates = cells.take_coordinates();
      sharedStrings = cells.take_shared_strings();
      values = cells.take_values();
      specialValues = cells.take_special_values();
      formulaValues = cells.take_formulas();
      if (formatStylesRequested) formatStyles = cells.take_format_styles();
      if (cellStyles) fillStyles = cells.take_fill_styles();
    } finally {
      cells.free();
    }
    // CE 0.20.3 derives !ref from the data when there is no usable dimension.
    const original = declared ?? (extent.length
      ? encode_range({ s: { r: extent[0], c: extent[1] }, e: { r: extent[2], c: extent[3] } }) : undefined);
    const storedCellCount = values.length;
    // Shared-string markup (`r`) and HTML, read once per distinct item.
    const sharedIndices = [...new Set(sharedStrings.filter((_, index) => index % 2))];
    const sharedMarkup = new Map();
    if (sharedIndices.length) {
      const markup = document.shared_string_items_xml(Uint32Array.from(sharedIndices));
      sharedIndices.forEach((item, index) => sharedMarkup.set(item, markup[index]));
    }
    const sharedHtml = new Map();
    let sharedIndex = 0;
    const links = projectedHyperlinks(document.sheet_hyperlinks(name, true));
    const notes = projectedNotes(document, name, cellHTML);
    const linkedCells = new Map();
    const bounds = !nodim && original && (notes.size || links.length) ? decode_range(original) : undefined;
    const clipped = nodim || bounds;
    if (notes.size || (links.length && clipped)) {
      const stubs = new Set();
      const rowLimit = sheetRows ? sheetRows - 1 : 1048575;
      const lastRow = Math.min(bounds?.e.r ?? 1048575, rowLimit);
      // CE 0.20.3 drops comments at rows >= sheetRows and stubs the rest,
      // widening !ref (below). VML-only Note shapes and links make stubs
      // that never widen it; those stay clipped to !ref and sheetRows.
      for (const key of notes.keys()) {
        const [, row, column] = address(key);
        if (row <= rowLimit) stubs.add(row * 16384 + column);
      }
      for (const key of clipped ? notes.vmlOnly : []) {
        const [, row, column] = address(key);
        if (row <= lastRow && (!bounds || (row >= bounds.s.r && column >= bounds.s.c && column <= bounds.e.c))) {
          stubs.add(row * 16384 + column);
        }
      }
      for (const entry of clipped ? links : []) {
        const { s, e } = entry;
        const firstColumn = Math.max(s.c, bounds?.s.c ?? 0);
        const lastColumn = Math.min(e.c, bounds?.e.c ?? 16383);
        if (firstColumn > lastColumn) continue;
        const endRow = Math.min(e.r, lastRow);
        for (let row = Math.max(s.r, bounds?.s.r ?? 0); row <= endRow; row++) {
          for (let column = firstColumn; column <= lastColumn; column++) {
            const coordinate = row * 16384 + column;
            linkedCells.set(coordinate, entry);
            stubs.add(coordinate);
          }
        }
      }
      for (let index = 0; index < coordinates.length && stubs.size; index += 2) {
        stubs.delete(coordinates[index] * 16384 + coordinates[index + 1]);
      }
      if (stubs.size) {
        const combined = new Uint32Array(coordinates.length + stubs.size * 2);
        combined.set(coordinates);
        let index = coordinates.length;
        for (const coordinate of stubs) {
          combined[index++] = Math.floor(coordinate / 16384);
          combined[index++] = coordinate % 16384;
          values.push(null);
        }
        coordinates = combined;
      }
    }
    const formats = new Map();
    const formatForStyle = style => {
      if (!numberFormats) return {};
      if (style === undefined) return { z: 'General', displayFormat: 'General' };
      if (!formats.has(style)) {
        const z = document.sheetjs_number_format_for_style(name, style);
        const displayFormat = cellText && dateNF !== undefined && z === 'm/d/yy'
          ? document.sheetjs_number_format_for_style(name, style, dateNF) : z;
        formats.set(style, { z, displayFormat });
      }
      return formats.get(style);
    };
    const dateStyles = new Map();
    const isDateStyle = style => {
      if (style === undefined) return false;
      if (!dateStyles.has(style)) dateStyles.set(style, document.is_date_style(name, style));
      return dateStyles.get(style);
    };
    const base = new Map();
    const fills = new Map();
    const palette = cellStyles ? document.sheetjs_theme_colors() : [];
    const target = Object.create(null);
    const grid = dense ? [] : undefined;
    if (grid) target['!data'] = grid;
    const layout = cellStyles ? sheetLayout(document.sheet_layout(name)) : {};
    if (layout.rows) target['!rows'] = layout.rows.map((entry) => ({ ...entry }));
    if (layout.columns) target['!cols'] = layout.columns.map((entry) => ({ ...entry }));
    if (declared !== undefined || nodim) target['!ref'] = declared;
    let maxRow = 0;
    let maxColumn = 0;
    let specialIndex = 0;
    let formulaIndex = 0;
    let noteExtent;
    for (let index = 0; index < values.length; index += 1) {
      const row = coordinates[index * 2];
      const column = coordinates[index * 2 + 1];
      const key = encode_cell({ r: row, c: column });
      const value = values[index];
      const style = formatStyles?.[index];
      const formulaOffset = formulaValues[formulaIndex] === index ? formulaIndex : undefined;
      if (formulaOffset !== undefined) formulaIndex += 5;
      const formulaId = formulaOffset === undefined ? undefined : formulaValues[formulaOffset + 1];
      const formulaResult = formulaOffset === undefined ? undefined : formulaValues.slice(formulaOffset + 2, formulaOffset + 5);
      const array = typeof formulaId === 'number' ? arrays[formulaId]
        : index >= storedCellCount ? arrays.find(({ s, e }) => row >= s.r && row <= e.r && column >= s.c && column <= e.c)
        : undefined;
      let error;
      let isoText;
      let sharedItem;
      if (sharedStrings[sharedIndex] === index) {
        sharedItem = sharedStrings[sharedIndex + 1];
        sharedIndex += 2;
      }
      if (specialValues[specialIndex] === index) {
        if (!array) {
          if (specialValues[specialIndex + 1]) isoText = specialValues[specialIndex + 2];
          else error = specialValues[specialIndex + 2];
        }
        specialIndex += 3;
      }
      const isoDate = dateFromIso(isoText, date1904);
      let storage = target;
      let slot = key;
      if (grid) {
        grid[row] ??= [];
        storage = grid[row];
        slot = column;
      }
      let fill;
      const stored = index < storedCellCount;
      if (fillStyles && stored) {
        const fillStyle = fillStyles[index] ?? 0;
        if (!fills.has(fillStyle)) fills.set(fillStyle, projectedFill(document.sheetjs_style_fill(fillStyle), palette, WTF));
        fill = fills.get(fillStyle);
      }
      const comments = notes.get(key);
      const covered = nodim || (bounds && row >= bounds.s.r && row <= bounds.e.r &&
        column >= bounds.s.c && column <= bounds.e.c);
      const linked = covered ? linkedCells.get(row * 16384 + column)
        : links.findLast(({ s, e }) => row >= s.r && row <= e.r && column >= s.c && column <= e.c);
      // Only a comment on an otherwise missing cell widens !ref in CE 0.20.3.
      // The sheetRows extent still counts link/VML stubs so they stay editable.
      const widening = !stored && !array && comments !== undefined && !linked;
      if (nodim ? stored || array || widening : sheetRows) {
        maxRow = Math.max(maxRow, row);
        maxColumn = Math.max(maxColumn, column);
      }
      if (widening && !nodim) {
        noteExtent ??= { s: { r: row, c: column }, e: { r: row, c: column } };
        extendRange(noteExtent, row, column);
      }
      if (array) {
        const result = projectArrayResult(key, array, formulaResult ?? [], { cellDates, date1904, cellFormula, cellHTML, xml: sharedMarkup.get(sharedItem), cellNF: cellNF && stored, cellText, xlfn, dateNF, fill, style, formatForStyle, isDateStyle });
        base.set(key, projectMetadataEditableCell(storage, slot, result, comments, linked));
      } else if (value === undefined && error === undefined && isoDate === undefined) {
        const formula = formulaOffset === undefined ? undefined : projectFormula(
          formulaId, formulaResult,
          { cellDates, date1904, cellFormula, cellHTML, xml: sharedMarkup.get(sharedItem), cellNF, cellText, xlfn, dateNF, fill, style, formatForStyle, isDateStyle });
        if (formula && cellFormula) {
          if (linked) formula.l = sharedLink(linked);
          if (comments) formula.c = copyComments(comments);
          storage[slot] = formula;
          base.set(key, { formula: formula.f, t: formula.t, v: formula.v instanceof Date ?
            formula.v.getTime() : formula.v, z: formula.z, comments, linked, fill });
        } else {
          const cell = formula ?? (!resultRanges.some((range) => rangeContains(range, row, column))
            ? projectRichText(document, name, row, column, { cellHTML, cellText, fill, xml: sharedMarkup.get(sharedItem),
              z: cellNF && stored ? formatForStyle(style).z : undefined }) : undefined);
          if (cell) {
            base.set(key, projectMetadataEditableCell(storage, slot, cell, comments, linked));
          } else {
            const get = () => { throw unsupported(name, key); };
            Object.defineProperty(storage, slot, { enumerable: true, configurable: true, get });
            base.set(key, { get });
          }
        }
      } else {
        const date = cellDates ? isoDate ?? (typeof value === 'number' && isDateStyle(style)
          ? dateFromSerial(value, date1904) : undefined) : undefined;
        const displayValue = isoDate ? serialFromDate(isoDate.getTime(), date1904) : value;
        const t = error !== undefined ? 'e' : value === null ? 'z' : date ? 'd' : cellType(displayValue);
        const cell = t === 'z' ? { t } : { t, v: error !== undefined ? errorCode(error) : date ?? displayValue };
        if (t === 's') {
          const xml = sharedMarkup.get(sharedItem);
          if (xml !== undefined) cell.r = xml;
          if (cellHTML && (sharedItem === undefined || xml !== undefined)) {
            let html = sharedItem === undefined ? undefined : sharedHtml.get(sharedItem);
            if (html === undefined) {
              html = plainStringHtml(cell.v);
              if (sharedItem !== undefined) sharedHtml.set(sharedItem, html);
            }
            cell.h = html;
          }
        }
        if (fill !== undefined) cell.s = fill;
        if (linked) cell.l = sharedLink(linked);
        if (comments) cell.c = copyComments(comments);
        const { z, displayFormat } = formatForStyle(style);
        if (cellNF && stored) cell.z = z;
        if (cellText && error !== undefined) cell.w = error;
        else if (cellText && t !== 'z') projectCellDisplay(cell, displayFormat, { date1904 });
        storage[slot] = cell;
        base.set(key, { t, v: date ? date.getTime() : cell.v, z: cell.z, linked, comments, fill, isoDate: isoDate !== undefined, isoTime: isoDate?.getTime() });
      }
    }
    let reference = original;
    let fullReference;
    if (nodim) {
      reference = encode_range({ s: { r: 0, c: 0 }, e: { r: maxRow, c: maxColumn } });
    } else if (original && sheetRows) {
      const extent = decode_range(original);
      if (extent.e.r >= sheetRows) {
        extent.e.r = Math.min(sheetRows - 1, maxRow);
        extent.e.c = Math.min(extent.e.c, maxColumn);
        extent.s.r = Math.min(extent.s.r, extent.e.r);
        extent.s.c = Math.min(extent.s.c, extent.e.c);
        fullReference = original;
        reference = encode_range(extent);
      }
    }
    if (noteExtent) {
      const extent = reference === undefined ? noteExtent : decode_range(reference);
      extendRange(extent, noteExtent.s.r, noteExtent.s.c);
      extendRange(extent, noteExtent.e.r, noteExtent.e.c);
      reference = encode_range(extent);
    }
    if (reference !== undefined) {
      target['!ref'] = reference;
      if (fullReference !== undefined) target['!fullref'] = fullReference;
    }
    if (autofilter !== undefined) target['!autofilter'] = { ref: autofilter };
    if (merges.length) target['!merges'] = merges.map(decode_range);
    if (margins.length) target['!margins'] = Object.fromEntries(marginKeys.map((key, index) => [key, margins[index]]));
    const sheet = grid ? target : new Proxy(target, {
      get(object, key) {
        if (!Object.hasOwn(object, key) && typeof key === 'string' && /^[A-Z]{1,3}[1-9][0-9]{0,6}$/.test(key)) {
          const [, row, column] = address(key);
          if (reference && rangeContains(reference, row, column) &&
              resultRanges.some((range) => rangeContains(range, row, column))) {
            throw unsupported(name, key);
          }
        }
        return Reflect.get(object, key);
      },
    });
    return { sheet, base, reference, fullReference, ranges, grid, date1904, original, autofilter, merges, layout, margins: margins.length ? margins : undefined };
  });
}

function readBytes(input, type) {
  if (type === 'base64') return decodeBase64(input);
  if (type === 'binary') {
    if (typeof input !== 'string') throw failure('INVALID_INPUT', 'Expected an encoded XLSX string');
    return binaryStringBytes(input);
  }
  if ((type === undefined || type === 'array') && (Array.isArray(input) || input instanceof Int8Array)) {
    return Uint8Array.from(input);
  }
  return copyInput(input);
}

function isHtmlText(text) {
  return /^\s*<(?:!doctype\s+html|html|head|body|table)(?=[\s>])/i.test(text);
}

function readHtml(text, options) {
  const workbook = book_new();
  const tables = htmlTables(text);
  if (!tables.length) throw failure('INVALID_INPUT', 'HTML input has no tables');
  for (const [index, table] of tables.entries()) {
    const sheet = table_to_sheet(table, { ...options, cellText: options.cellText !== false });
    book_append_sheet(workbook, sheet, 'Sheet' + (index + 1));
  }
  workbook.bookType = 'html';
  return workbook;
}

function utf8DelimitedText(bytes, sample, options) {
  const control = /[\u0000-\u0008\u000b\u000c\u000e-\u001f]/;
  if (control.test(sample)) return undefined;
  const separator = options && data(options, 'FS');
  if (!(options && data(options, 'PRN')) && !/[,;|\t\r\n]/.test(sample) &&
      !(typeof separator === 'string' && separator && sample.includes(separator[0]))) return undefined;
  try {
    const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    return control.test(text) ? undefined : text;
  } catch {
    return undefined;
  }
}

const textCodepages = new Map([
  [65001, 'utf-8'], [1200, 'utf-16le'], [1201, 'utf-16be'],
  [932, 'shift_jis'], [936, 'gbk'], [949, 'euc-kr'], [950, 'big5'],
  [874, 'windows-874'], [10000, 'macintosh'], [20866, 'koi8-r'], [21866, 'koi8-u'],
  [28592, 'iso-8859-2'], [28605, 'iso-8859-15'], [54936, 'gb18030'],
  ...Array.from({ length: 9 }, (_, index) => [1250 + index, `windows-${1250 + index}`]),
]);

function decodeTextCodepage(bytes, codepage) {
  if (codepageTable && ![65001, 1200, 1201].includes(codepage)) {
    try {
      const text = codepageTable.utils.decode(codepage, bytes);
      if (typeof text !== 'string') throw new TypeError('Expected decoded text');
      return text;
    } catch (error) {
      throw failure('INVALID_INPUT', `Unable to decode text codepage ${codepage}`, error);
    }
  }
  if (codepage === 28591) return binaryString(bytes);
  const encoding = textCodepages.get(codepage);
  if (!encoding) throw failure('UNSUPPORTED_OPTIONS', `Unsupported text codepage ${codepage}`);
  try {
    return new TextDecoder(encoding, { fatal: true }).decode(bytes);
  } catch {
    throw failure('INVALID_INPUT', `Invalid text for codepage ${codepage}`);
  }
}

function readText(input, options, odfParts) {
  const spreadsheetMl = typeof input === 'string' && isXlmlText(input);
  const openDocument = typeof input === 'string' && isFodsText(input);
  const settings = textReadSettings(options, openDocument ? 'openDocument' : spreadsheetMl ? 'spreadsheetMl' : 'plain');
  if (typeof input !== 'string') throw failure('INVALID_INPUT', 'Expected delimited text');
  if (spreadsheetMl) return readXlml(input, settings);
  if (openDocument) return readFods(input, settings, odfParts);
  if (isDifText(input)) return readDif(input, settings);
  if (isSylkText(input)) return readSylk(input, settings);
  if (isEthText(input)) return readEth(input, settings);
  if (isRtfText(input)) return readRtf(input, settings);
  return isHtmlText(input) ? readHtml(input, settings) : readDelimited(input, settings);
}

function readBookProperties(document, bookSheets) {
  const result = decodeBookProperties(document.document_properties());
  if (bookSheets) result.SheetNames = [...document.sheet_names()];
  return result;
}

function attachReadOnlyProperties(workbook, bytes) {
  let properties;
  const dates = [];
  for (const key of ['Props', 'Custprops']) {
    Object.defineProperty(workbook, key, { enumerable: true, get() {
      if (!properties) {
        properties = withDocument(bytes, (document) => readBookProperties(document, false));
        for (const bag of Object.values(properties)) {
          for (const value of Object.values(bag)) {
            if (value instanceof Date) dates.push([value, value.getTime()]);
            if (Array.isArray(value)) Object.freeze(value);
          }
          Object.freeze(bag);
        }
      }
      return properties[key];
    } });
  }
  return () => {
    if (dates.some(([date, time]) => !Object.is(Date.prototype.getTime.call(date), time))) {
      throw failure('UNSUPPORTED_STRUCTURE', 'Workbook document properties are read-only');
    }
  };
}

function projectedDependencies(fields) {
  if (fields === undefined) return Object.freeze({});
  const entries = [];
  let entry;
  for (let index = 0; index < fields.length; index += 2) {
    const [key, value] = fields.slice(index, index + 2);
    if (key === '') {
      entry = {};
      entries.push(entry);
    } else {
      Object.defineProperty(entry, key, { value, enumerable: true, configurable: true, writable: true });
    }
  }
  let sheetId = 1;
  for (const entry of entries) {
    sheetId = entry.i || sheetId;
    entry.i = sheetId;
    Object.freeze(entry);
  }
  return Object.freeze(entries);
}

let fileSystem;

export function set_fs(fs) {
  fileSystem = fs;
}

export function readFile(filename, options) {
  if (!fileSystem?.readFileSync) {
    throw failure('UNSUPPORTED_ENVIRONMENT', 'File reads require a filesystem supplied through set_fs');
  }
  const { rest } = byteReadOptionFields(options);
  return read(fileSystem.readFileSync(filename), Object.defineProperties({}, rest));
}

export { readFile as readFileSync };

function zipEntries(container) {
  if (Array.isArray(container?.FullPaths) && Array.isArray(container.FileIndex)) {
    const { FullPaths: paths, FileIndex: entries } = container;
    const root = paths[0];
    if (typeof root !== 'string' || !root.endsWith('/') || paths.length !== entries.length) {
      throw failure('INVALID_INPUT', 'Invalid ZIP container paths');
    }
    return paths.slice(1).map((path, index) => {
      if (typeof path !== 'string' || !path.startsWith(root)) {
        throw failure('INVALID_INPUT', 'Invalid ZIP container entry path');
      }
      return [path.slice(root.length), entries[index + 1]];
    });
  }
  if (container?.files && typeof container.files === 'object') return Object.entries(container.files);
  throw failure('INVALID_INPUT', 'Expected a ZIP container');
}

function zipEntryBytes(entry) {
  if (entry?.dir || entry?.type === 1) return new Uint8Array();
  const content = entry?.content ?? entry?._data;
  if (typeof content === 'string') return readBytes(content, 'binary');
  if (content !== undefined && (content instanceof ArrayBuffer || content instanceof Uint8Array || content instanceof Int8Array || Array.isArray(content))) {
    return readBytes(content);
  }
  for (const method of ['asUint8Array', 'asNodeBuffer', 'asBinary']) {
    if (typeof entry?.[method] === 'function') {
      const value = entry[method]();
      return readBytes(value, typeof value === 'string' ? 'binary' : undefined);
    }
  }
  throw failure('INVALID_INPUT', 'Expected decompressed ZIP entry bytes');
}

export function parse_zip(container, options) {
  const entries = zipEntries(container).filter(([name]) => name && name !== '\u0001Sh33tJ5');
  const { rest } = byteReadOptionFields(options);
  const settings = Object.defineProperties({}, rest);
  const names = entries.map(([name]) => name.replaceAll('\\', '/'));
  let hasManifest = false;
  let nested = -1;
  for (const [index, name] of names.entries()) {
    const normalized = name.toLowerCase();
    if (normalized === '[content_types].xml' || normalized === 'meta-inf/manifest.xml') {
      hasManifest = true;
      break;
    }
    if (nested === -1 && normalized === 'index.zip') nested = index;
  }
  if (!hasManifest && nested !== -1) return read(zipEntryBytes(entries[nested][1]), settings);
  names.forEach(checkedZipEntryName);
  const content = entries.map(([, entry]) => zipEntryBytes(entry));
  return read(zipFromParts(names, content, undefined, 'WORKBOOK_LOAD_REFUSED'), settings);
}

function zipFromParts(names, content, compression, errorCode) {
  const offsets = new Uint32Array(names.length + 1);
  let size = 0;
  content.forEach((bytes, index) => {
    size += bytes.length;
    if (size > 0xffffffff) throw failure('INVALID_INPUT', 'ZIP container is too large');
    offsets[index + 1] = size;
  });
  const bytes = new Uint8Array(size);
  content.forEach((part, index) => bytes.set(part, offsets[index]));
  return rustCall(errorCode, () => write_zip_entries(names, offsets, bytes, compression));
}

export function parse_xlscfb(container, options) {
  const stream = compoundStream(container);
  if (!stream) throw failure('INVALID_INPUT', 'Compound document has no Excel workbook stream');
  const settings = compoundReadSettings(options);
  const workbook = settings.bookProps && !settings.bookSheets
    ? {} : readXls(readBytes(stream), settings, decodeTextCodepage);
  function readProperties(name, documentSummary) {
    const stream = compoundPart(container, ['\u0005' + name.toLowerCase(), '!' + name.toLowerCase()]);
    if (!stream?.length && !stream?.byteLength) return [];
    const fields = biff_document_properties(readBytes(stream), documentSummary);
    if (fields === undefined && settings.WTF) {
      throw failure('INVALID_INPUT', 'Malformed XLS ' + name + ' property stream');
    }
    return fields ?? [];
  }
  const documentSummary = readProperties('DocumentSummaryInformation', true);
  const summary = readProperties('SummaryInformation', false);
  const { Props } = decodeBookProperties([...summary, ...documentSummary]);
  workbook.Props = workbook.Custprops = Props;
  if (settings.bookFiles) workbook.cfb = container;
  return workbook;
}

export function read(input, options) {
  const { codepage, type } = readInputSettings(options);
  if (type === 'file') return readFile(input, options);
  if (type === 'string') return readText(input, options);
  const bytes = readBytes(input, type ?? (typeof input === 'string' ? 'base64' : undefined));
  if (isEarlyBiff(bytes)) {
    const settings = earlyBiffReadSettings(options);
    return readEarlyBiff(bytes, settings, part => decodeTextCodepage(part, codepage || 1252));
  }
  const wk3 = isWk3(bytes);
  if (wk3 || isWk1(bytes)) {
    const settings = lotusReadSettings(options);
    return (wk3 ? readWk3 : readWk1)(bytes, settings, part => codepage ? decodeTextCodepage(part, codepage) : binaryString(part));
  }
  if (isDbf(bytes)) {
    const settings = dbfReadSettings(options);
    return readDbf(bytes, settings, decodeTextCodepage);
  }
  if ((bytes[0] === 255 && bytes[1] === 254) || (bytes[0] === 254 && bytes[1] === 255)) {
    const decoder = new TextDecoder(bytes[0] === 255 ? 'utf-16le' : 'utf-16be');
    return readText(decoder.decode(bytes), options);
  }
  const utf8 = new TextDecoder();
  const utf8Bom = bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf;
  const sample = utf8.decode(bytes.subarray(0, 1024));
  if (utf8Bom) return readText(utf8.decode(bytes), options);
  const zipSignature = bytes[0] === 0x50 && bytes[1] === 0x4b &&
    (bytes[2] === 3 && bytes[3] === 4 || bytes[2] === 5 && bytes[3] === 6 || bytes[2] === 7 && bytes[3] === 8);
  if (zipSignature) {
    const [content, styles, metadata] = rustCall('WORKBOOK_LOAD_REFUSED', () => read_ods_parts(bytes));
    if (content !== undefined) {
      const book = readText(content, options, { styles, metadata });
      book.bookType = 'ods';
      return book;
    }
  }
  if (zipSignature) {
    const archive = rustCall('WORKBOOK_LOAD_REFUSED', () => XlsbArchive.open(bytes));
    if (archive) {
      try {
        return readXlsb(archive, xlsbReadSettings(options));
      } finally {
        archive.free();
      }
    }
  }
  const compound = bytes[0] === 0xd0 && bytes[1] === 0xcf && bytes[2] === 0x11 && bytes[3] === 0xe0;
  if (compound) {
    const workbook = rustCall('WORKBOOK_LOAD_REFUSED', () => compoundWorkbook(bytes));
    if (workbook) {
      const { raw } = compoundContainerSettings(compoundReadSettings(options));
      return parse_xlscfb(raw ? rustCall('WORKBOOK_LOAD_REFUSED', () => compoundWorkbook(bytes, true)) : workbook, options);
    }
  }
  const container = zipSignature || compound;
  if (codepage && !container) {
    const text = decodeTextCodepage(bytes, codepage);
    if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/.test(text)) {
      throw failure('INVALID_INPUT', 'The input contains binary data');
    }
    return readText(text, options);
  }
  if (isRtfText(sample)) return readText(new TextDecoder('windows-1252').decode(bytes), options);
  if (isHtmlText(sample) || isXlmlText(sample) || isDifText(sample) || isSylkText(sample) || isEthText(sample) || isFodsText(sample)) return readText(utf8.decode(bytes), options);
  const delimited = utf8DelimitedText(bytes, sample, options);
  if (delimited !== undefined) return readText(delimited, options);
  const settings = xlsxReadSettings(options);
  const { bookSheets, bookProps } = settings;
  if (bookProps) return withDocument(bytes, (document) => readBookProperties(document, bookSheets));
  const resources = xlsxWorkbookResourceSettings(settings);
  const [names, entries, visibility, date1904, vba, chain, zip, xmlType, worksheetNames, worksheetIds, bookViews] = withDocument(bytes, (document) =>
    [document.sheet_names(), document.defined_names(), document.sheet_visibility(), document.date1904(),
      resources.vba ? document.vba_project() : undefined,
      resources.dependencies ? document.calculation_chain() : undefined,
      resources.files ? projectedZipFiles(document) : undefined, document.xml_workbook_type(),
      document.worksheet_names(), document.worksheet_ids(), document.workbook_views()]);
  const definedNames = [];
  for (let index = 0; index < entries.length; index += 4) {
    const [Name, Ref, scope, comment] = entries.slice(index, index + 4);
    const entry = { Name, Ref };
    if (scope !== '') entry.Sheet = Number(scope);
    if (comment !== '') entry.Comment = comment;
    definedNames.push(Object.freeze(entry));
  }
  const visibleNames = bookSheets ? [] : names.filter(xlsxWorkbookProjectionSettings(settings).selectSheet);
  const sheets = bookSheets ? undefined : Object.create(null);
  const projections = new Map();
  const sheetSettings = sheets && xlsxSheetProjectionSettings(settings);
  for (const name of visibleNames) {
    Object.defineProperty(sheets, name, {
      enumerable: true,
      get() {
        if (!projections.has(name)) {
          projections.set(name, rustCall('SHEET_READ_REFUSED', () => project(bytes, name, sheetSettings)));
        }
        return projections.get(name).sheet;
      },
    });
  }
  const workbook = { SheetNames: [...names] };
  if (sheets) workbook.Sheets = sheets;
  const nameMetadata = definedNames.length ? Object.freeze(definedNames) : undefined;
  workbook.Workbook = {
    WBProps: { date1904 }, WBView: projectedWorkbookViews(bookViews),
    Sheets: projectedSheetEntries(names, visibility, worksheetNames, worksheetIds),
  };
  if (nameMetadata) workbook.Workbook.Names = nameMetadata;
  workbook.Workbook.xmlns = workbookNamespace;
  const checkProperties = sheets && attachReadOnlyProperties(workbook, bytes);
  if (sheets) Object.defineProperty(workbook, 'bookType', { enumerable: true, value: 'xlsx' });
  if (sheets) Object.defineProperty(workbook, 'Deps', { enumerable: true, value: projectedDependencies(chain) });
  if (vba !== undefined) {
    Object.defineProperty(workbook, 'vbaraw', { enumerable: true, get: () => vba.slice() });
  }
  if (zip) {
    Object.defineProperties(workbook, {
      keys: { enumerable: true, value: Object.freeze(zip.keys) },
      files: { enumerable: true, value: Object.freeze(zip.files) },
    });
  }
  states.set(workbook, { bytes, names, sheets, projections, visibleNames, nameMetadata, visibility, date1904, checkProperties, hasVba: vba !== undefined, hasZip: zip !== undefined, xmlType });
  return workbook;
}

function indexedDescriptors(array, max) {
  if (!Array.isArray(array) || array.length > max) {
    throw failure('UNSUPPORTED_STRUCTURE', 'Expected a dense worksheet array within XLSX bounds');
  }
  return Reflect.ownKeys(array).filter((key) => key !== 'length').map((key) => {
    if (typeof key !== 'string' || !/^(0|[1-9][0-9]*)$/.test(key) || Number(key) >= max) {
      throw failure('UNSUPPORTED_STRUCTURE', 'Unexpected dense worksheet array property');
    }
    return [Number(key), Object.getOwnPropertyDescriptor(array, key)];
  });
}

function cellDescriptors(sheet, grid) {
  const cells = new Map();
  for (const key of Reflect.ownKeys(sheet)) {
    if (key === '!ref' || key === '!fullref' || key === '!autofilter' || key === '!merges' || key === '!rows' || key === '!cols' || key === '!margins' || (grid && key === '!data')) continue;
    if (grid) throw failure('UNSUPPORTED_STRUCTURE', 'Dense worksheets use !data for cells');
    cells.set(key, Object.getOwnPropertyDescriptor(sheet, key));
  }
  if (grid) {
    if (data(sheet, '!data') !== grid) {
      throw failure('UNSUPPORTED_STRUCTURE', 'The original !data array must be preserved');
    }
    for (const [r, rowDescriptor] of indexedDescriptors(grid, 1048576)) {
      const row = rowDescriptor.value;
      if (row === undefined && Object.hasOwn(rowDescriptor, 'value')) continue;
      for (const [c, descriptor] of indexedDescriptors(row, 16384)) {
        cells.set(encode_cell({ r, c }), descriptor);
      }
    }
  }
  return cells;
}

function readMerges(sheet) {
  if (!Object.hasOwn(sheet, '!merges')) return [];
  const ranges = checkedMergeList(data(sheet, '!merges'));
  const merges = ranges.map((range) => {
    const first = range?.s;
    const last = range?.e;
    for (const point of [first, last]) {
      if (!isWorksheetRow(point?.r) || !isWorksheetColumn(point?.c)) {
        throw failure('INVALID_VALUE', 'Expected valid merged range endpoints');
      }
    }
    if (first.r > last.r || first.c > last.c) {
      throw failure('INVALID_VALUE', 'Expected ordered merged range endpoints');
    }
    return encode_range(range);
  });
  if (new Set(merges).size !== merges.length) {
    throw failure('INVALID_VALUE', 'Duplicate merged ranges');
  }
  return merges;
}

// A new workbook keeps covered cell values, as SheetJS writes every cell of its model;
// merging over existing workbook values stays refused.
function applyMerges(document, name, removed, added, keepCovered = false) {
  for (const range of removed) {
    rustCall('MERGE_EDIT_REFUSED', () => document.unmerge_range(name, range));
  }
  for (const range of added) {
    rustCall('MERGE_EDIT_REFUSED', () => keepCovered
      ? document.merge_preserving_covered_values(name, range) : document.merge_range(name, range));
  }
}

function readCellLink(link) {
  return link == null ? null : xlsxCellHyperlinkSettings(link);
}

function changedCellLink(cell, existing, name, key) {
  const input = data(cell, 'l');
  const next = readCellLink(input);
  const previous = existing?.link;
  if (next && !Object.hasOwn(input, 'display')) next.display = previous?.display;
  const sameTarget = next?.Target === previous?.Target;
  const sameDisplay = next?.display === previous?.display;
  if (sameTarget && sameDisplay && next?.Tooltip === previous?.Tooltip) return undefined;
  if (existing && (existing.s.r !== existing.e.r || existing.s.c !== existing.e.c)) {
    throw failure('UNSUPPORTED_CELL', `${name}!${key} belongs to a range hyperlink`);
  }
  return next && { ...next, tooltipOnly: sameTarget && sameDisplay };
}

function applyCellLink(document, name, reference, link) {
  rustCall('HYPERLINK_EDIT_REFUSED', () => {
    if (link?.tooltipOnly) {
      document.set_cell_hyperlink_tooltip(name, reference, link.Tooltip);
    } else {
      document.clear_cell_hyperlink(name, reference);
      if (link) document.set_cell_hyperlink(name, reference, link.Target, undefined, link.Tooltip, link.display);
    }
  });
}

function readCellComments(comments) {
  return comments == null ? [] : Array.from(checkedLegacyComments(comments), xlsxCellCommentSettings);
}

function changedCellComments(cell, previous) {
  const comments = readCellComments(data(cell, 'c'));
  return comments.length !== previous.length ||
    comments.some(({ a, t }, index) => a !== previous[index].a || t !== previous[index].t)
    ? comments : undefined;
}

function applyCellComments(document, name, row, column, comments, previous = []) {
  rustCall('COMMENT_EDIT_REFUSED', () => {
    if (comments.length === 1 && previous.length === 1 && previous[0].a === comments[0].a) {
      document.set_cell_note(name, row, column, comments[0].t);
    } else {
      document.set_sheetjs_comments(name, row, column, comments.flatMap(({ a, t }) => [a, t]));
    }
  });
}

function layoutEdits(sheet, original = {}) {
  const changes = [];
  for (const [key, baseline, sizeKeys, limit, validate] of [
    ['!rows', original.rows, worksheetRowSizeKeys, 1048576, validateWorksheetRowLayout],
    ['!cols', original.columns, worksheetColumnSizeKeys, 16384, validateWorksheetColumnLayout],
  ]) {
    const entries = data(sheet, key);
    if (entries === undefined) continue;
    for (const [index, descriptor] of indexedDescriptors(entries, limit)) {
      const entry = descriptor.value;
      if (entry == null) continue;
      validate(entry);
      const prior = baseline?.[index];
      const requireChange = (name) => {
        const value = data(entry, name);
        // A null with no prior value is absent, as CE 0.20.3 writes it (its
        // reader gives width: null for a <col> without width).
        return value === prior?.[name] || (value === null && prior?.[name] == null) ? undefined : value;
      };
      const requestedSizes = sizeKeys.map(requireChange);
      if (requestedSizes.some((value) => value !== undefined && (!Number.isFinite(value) || value < 0))) {
        throw failure('INVALID_VALUE', 'Invalid worksheet size');
      }
      let size = requestedSizes[0] ?? requestedSizes[1];
      if (key === '!cols') {
        const [width, pixels, characters] = requestedSizes;
        const digits = data(entry, 'MDW') ?? prior?.MDW ?? defaultDigitWidth;
        if (!Number.isInteger(digits) || digits < 1) throw failure('INVALID_VALUE', 'Invalid maximum digit width');
        size = width ?? (pixels !== undefined ? columnWidth(columnCharacters(pixels, digits), digits) :
          characters !== undefined ? columnWidth(characters, digits) : undefined);
      }
      const hidden = requireChange('hidden');
      const level = requireChange('level');
      if ((size !== undefined && (!Number.isFinite(size) || size < 0)) ||
          (hidden !== undefined && typeof hidden !== 'boolean') ||
          (level !== undefined && (!Number.isInteger(level) || level < 0 || level > 7))) {
        throw failure('INVALID_VALUE', 'Invalid worksheet size, visibility or outline');
      }
      if (size !== undefined || hidden !== undefined || level !== undefined) {
        changes.push([key, index, size, hidden, level]);
      }
    }
  }
  return changes;
}

function applyLayout(document, name, changes) {
  for (const [key, index, size, hidden, level] of changes) {
    rustCall('SHEET_LAYOUT_EDIT_REFUSED', () => {
      if (key === '!rows') document.set_row_layout(name, index, size, hidden, level);
      else document.set_column_layout(name, index, size, hidden, level);
    });
  }
}

function encodeFormulaResult(type, input, date1904, cellDates) {
  let result;
  let resultType;
  if (input != null) {
    if (type === 'e') {
      resultType = 'e';
      result = errorName(input);
      if (!result) throw failure('INVALID_VALUE', 'Unknown SheetJS error code');
    } else if (type === 'd') {
      if (!(input instanceof Date) || !Number.isFinite(Date.prototype.getTime.call(input))) {
        throw failure('INVALID_VALUE', 'Expected a valid Date');
      }
      if (cellDates) {
        resultType = 'd';
        result = isoCellText(input);
      } else {
        result = serialFromDate(Date.prototype.getTime.call(input), date1904);
      }
    } else {
      result = scalar(input, true);
      if (type !== cellType(result)) throw failure('INVALID_VALUE', 'Cell type and value do not match');
    }
  }
  return { result, resultType };
}

function recordArrayCell(anchors, followers, row, column, { array, dynamic, formula, type, input }, date1904, cellDates) {
  if (array === undefined && dynamic === undefined) return false;
  const { s, e } = typeof array === 'string' ? decode_range(array) : {};
  if (!isWorksheetRow(s?.r) || !isWorksheetRow(e?.r) ||
      !isWorksheetColumn(s?.c) || !isWorksheetColumn(e?.c) ||
      s.r > row || e.r < row || s.c > column || e.c < column ||
      (dynamic !== undefined && typeof dynamic !== 'boolean')) {
    throw failure('UNSUPPORTED_CELL', 'Expected an array range containing the cell');
  }
  const reference = encode_range({ s, e });
  if (s.r !== row || s.c !== column) {
    if (formula !== undefined || dynamic !== undefined) {
      throw failure('UNSUPPORTED_CELL', 'Array formula and dynamic flag belong on the anchor');
    }
    followers.add(reference);
    return false;
  }
  if (typeof formula !== 'string' || !formula.trim()) {
    throw failure('UNSUPPORTED_CELL', 'Expected an array formula on its anchor cell');
  }
  const { result, resultType } = encodeFormulaResult(type, input, date1904, cellDates);
  anchors.set(reference, { formula: scalar(formula, false), dynamic: dynamic === true, result, resultType });
  return true;
}

function checkArrayFollowers(anchors, followers) {
  for (const reference of followers) {
    if (!anchors.has(reference)) throw failure('UNSUPPORTED_CELL', 'Array range is missing its formula anchor');
  }
}

function applyArrayFormulas(document, name, anchors) {
  for (const [reference, { formula, dynamic, result, resultType }] of anchors) {
    rustCall('CELL_EDIT_REFUSED', () => document.write_array_formula(name, reference, formula, dynamic, result, resultType));
  }
}

function pending(name, { sheet, base, reference, fullReference, ranges, grid, original, autofilter, merges, layout, margins }, date1904, cellDates) {
  const changes = [];
  const arrayAnchors = new Map();
  const arrayFollowers = new Set();
  if (data(sheet, '!ref') !== reference || Object.hasOwn(sheet, '!ref') !== (reference !== undefined)) {
    throw failure('UNSUPPORTED_RANGE', `${name}: the original !ref must be preserved`);
  }
  if (data(sheet, '!fullref') !== fullReference || Object.hasOwn(sheet, '!fullref') !== (fullReference !== undefined)) {
    throw failure('UNSUPPORTED_RANGE', `${name}: the original !fullref must be preserved`);
  }
  const layoutChanges = layoutEdits(sheet, layout);
  const cells = cellDescriptors(sheet, grid);
  for (const key of base.keys()) {
    if (!cells.has(key)) throw failure('UNSUPPORTED_REMOVAL', `${name}!${key} was removed`);
  }
  for (const [key, descriptor] of cells) {
    const [canonical, row, column] = address(key);
    if (canonical !== key) throw failure('INVALID_ADDRESS', `${name}!${key} must use canonical A1 notation`);
    const before = base.get(key);
    if (before?.get) {
      if (descriptor.get !== before.get || descriptor.set !== undefined || !descriptor.enumerable) {
        throw unsupported(name, key);
      }
      if (before.metadataCell) {
        const expectedCount = before.fieldCount + Number(Object.hasOwn(before.metadataCell, 'c')) +
          Number(Object.hasOwn(before.metadataCell, 'l'));
        if (Reflect.ownKeys(before.metadataCell).length !== expectedCount) {
          throw unsupported(name, key);
        }
        const previousComments = before.comments ?? [];
        const comments = changedCellComments(before.metadataCell, previousComments);
        const link = changedCellLink(before.metadataCell, before.linked, name, key);
        if (comments !== undefined || link !== undefined) changes.push({ row, column, comments, previousComments, link });
      }
      continue;
    }
    const cell = descriptor.value;
    if (!descriptor.enumerable || !isXlsxCellShape(cell) || !Object.hasOwn(cell, 't')) {
      throw failure('INVALID_VALUE', `${name}!${key} must contain t and v`);
    }
    if (Object.hasOwn(cell, 's') && data(cell, 's') !== before?.fill) {
      throw failure('UNSUPPORTED_CELL', `${name}!${key} fill metadata is read-only`);
    }
    const previousComments = before?.comments ?? [];
    const comments = changedCellComments(cell, previousComments);
    const link = changedCellLink(cell, before?.linked, name, key);
    let format;
    if (Object.hasOwn(cell, 'z') && data(cell, 'z') !== before?.z) {
      format = numberFormatCode(data(cell, 'z'));
      if (!isWritableNumberFormat(format) || !format) {
        throw failure('UNSUPPORTED_FORMAT', `${name}!${key} requires a number format code`);
      }
    }
    const type = data(cell, 't');
    const input = data(cell, 'v');
    let value;
    let explicitDate;
    let formula;
    let error;
    const nextFormula = data(cell, 'f');
    const array = data(cell, 'F');
    const arrayAnchor = recordArrayCell(arrayAnchors, arrayFollowers, row, column,
      { array, dynamic: data(cell, 'D'), formula: nextFormula, type, input }, date1904, cellDates);
    if (array !== undefined && type === 'd' && before?.t !== 'd' && format === undefined) format = 'm/d/yy';
    const removedFormula = nextFormula === undefined && before?.formula !== undefined;
    if (arrayAnchor) {
      // The anchor's value is installed with its formula.
    } else if (array !== undefined && input == null) {
      if (before && (before.t !== 'z' || removedFormula)) value = null;
    } else if (nextFormula !== undefined) {
      if (typeof nextFormula !== 'string' || !nextFormula.trim()) {
        throw failure('UNSUPPORTED_CELL', `${name}!${key} requires a nonempty formula`);
      }
      const cache = input instanceof Date ? Date.prototype.getTime.call(input) : input;
      const isNew = before?.formula === undefined;
      const formulaChanged = nextFormula !== before?.formula;
      const cacheChanged = input == null ? !isNew && before.v != null
        : isNew || type !== before.t || !Object.is(cache, before.v);
      if (formulaChanged || cacheChanged) {
        const text = formulaChanged ? scalar(nextFormula, false) : undefined;
        const result = encodeFormulaResult(type, cacheChanged ? input : undefined, date1904, cellDates);
        formula = { text, ...result };
        if (result.result !== undefined && type === 'd' && before?.t !== 'd' && format === undefined) {
          format = 'm/d/yy';
        }
      }
    } else if (type === 'e') {
      if (removedFormula || before?.t !== 'e' || before.v !== input) {
        error = errorName(input);
        if (!error) throw failure('INVALID_VALUE', 'Unknown SheetJS error code');
      }
    } else if (type === 'd') {
      if ((before?.t !== 'd' && array === undefined) || !(input instanceof Date)) {
        throw failure('UNSUPPORTED_CELL', `${name}!${key} requires an existing date format`);
      }
      const milliseconds = Date.prototype.getTime.call(input);
      if (!Number.isFinite(milliseconds)) throw failure('INVALID_VALUE', 'Expected a valid Date');
      const writeIso = cellDates ?? before?.isoDate;
      if (removedFormula || milliseconds !== before?.v || Boolean(writeIso) !== Boolean(before?.isoDate)) {
        if (writeIso) explicitDate = isoCellText(input);
        else value = serialFromDate(milliseconds, date1904);
      }
    } else if (input == null && (type === 'z' || removedFormula)) {
      if (removedFormula || (before && before.t !== 'z')) value = null;
    } else {
      const next = scalar(input, true);
      if (cellType(next) !== type) throw failure('INVALID_VALUE', `${name}!${key} has mismatched t and v`);
      if (removedFormula || before?.t !== type || !Object.is(before.v, next)) value = next;
      else if (cellDates === false && before.isoDate) value = serialFromDate(before.isoTime, date1904);
    }
    if (array === undefined && value === undefined && explicitDate === undefined && formula === undefined && error === undefined && format === undefined && link === undefined && comments === undefined) continue;
    if (!reference || !rangeContains(reference, row, column) || !original || !rangeContains(original, row, column)) {
      throw failure('UNSUPPORTED_RANGE', `${name}!${key} is outside the original !ref`);
    }
    if (ranges.some((range) => rangeContains(range, row, column))) {
      throw unsupported(name, key);
    }
    changes.push({ row, column, value, explicitDate, format, link, comments,
      previousComments, formula, error, replaceIso: before?.isoDate && typeof value === 'number',
      replaceSpecial: removedFormula || before?.t === 'e' });
  }
  checkArrayFollowers(arrayAnchors, arrayFollowers);
  for (const array of arrayAnchors.keys()) {
    const { s, e } = decode_range(array);
    if ([s, e].some(({ r, c }) => !rangeContains(reference, r, c) || !rangeContains(original, r, c))) {
      throw failure('UNSUPPORTED_RANGE', `${name}!${array} is outside the original !ref`);
    }
  }
  const filter = readAutofilter(sheet);
  const nextMerges = readMerges(sheet);
  return {
    edits: changes, arrayAnchors, layoutChanges, marginChanges: marginEdits(sheet, margins), filter, filterChanged: filter !== autofilter,
    removed: merges.filter((range) => !nextMerges.includes(range)),
    added: nextMerges.filter((range) => !merges.includes(range)),
  };
}

function visibilityEdits(workbook, names, nameMetadata, original) {
  if (!Object.hasOwn(workbook, 'Workbook')) return [];
  const metadata = data(workbook, 'Workbook');
  if (!isWorkbookVisibilityMetadata(metadata) ||
      data(metadata, 'Names') !== nameMetadata) {
    throw failure('UNSUPPORTED_STRUCTURE', 'Expected worksheet visibility metadata');
  }
  const edits = [];
  for (const [index, descriptor] of indexedDescriptors(data(metadata, 'Sheets') ?? [], names.length)) {
    const entry = descriptor.value;
    if (!isWorksheetVisibilityEntry(entry) ||
        (Object.hasOwn(entry, 'name') && data(entry, 'name') !== names[index])) {
      throw failure('UNSUPPORTED_STRUCTURE', 'Expected a worksheet visibility entry');
    }
    if (!Object.hasOwn(entry, 'Hidden')) continue;
    const value = data(entry, 'Hidden');
    if (!isWorksheetVisibility(value)) {
      throw failure('INVALID_VALUE', 'Expected a worksheet visibility of 0, 1, or 2');
    }
    if (value !== original?.[index]) edits.push([names[index], value]);
  }
  return edits.sort((a, b) => a[1] - b[1]);
}

function dateSystem(workbook, original = false) {
  const metadata = data(workbook, 'Workbook');
  if (!metadata || !Object.hasOwn(metadata, 'WBProps')) return original;
  return workbookDateSystemSettings(data(metadata, 'WBProps')).date1904 ?? original;
}

function applyVisibility(document, changes) {
  for (const [name, value] of changes) {
    const state = worksheetVisibilityStates[value];
    if (rustCall('WORKSHEET_STATE_REFUSED', () => document.sheet_state(name)) !== state) {
      rustCall('WORKSHEET_STATE_REFUSED', () => document.set_sheet_state(name, state));
    }
  }
}

function propertyEntries(properties, xlsb = false) {
  if (properties === undefined) return [];
  if (!properties || typeof properties !== 'object' || Array.isArray(properties)) {
    throw failure('UNSUPPORTED_STRUCTURE', 'Expected an object for workbook Props');
  }
  const entries = [];
  for (const key of Reflect.ownKeys(properties)) {
    const property = Object.hasOwn(writableProperties, key) ? writableProperties[key] : undefined;
    const descriptor = Object.getOwnPropertyDescriptor(properties, key);
    if (xlsb && isXlsbDerivedProperty(key) && Object.hasOwn(descriptor, 'value')) continue;
    if (!property || !Object.hasOwn(descriptor, 'value')) {
      throw failure('UNSUPPORTED_STRUCTURE', 'Unsupported document property');
    }
    const value = descriptor.value;
    if (value == null || value === '') continue;
    let text;
    if (isDocumentDateProperty(property) && value instanceof Date) {
      if (!isValidDocumentPropertyDate(value)) {
        throw failure('INVALID_VALUE', 'Expected a valid document property date');
      }
      text = Date.prototype.toISOString.call(value).replace(/\.\d{3}Z$/, 'Z');
    } else {
      const checked = scalar(value, true);
      text = typeof checked === 'boolean' ? String(Number(checked)) : String(checked);
    }
    entries.push([property, text]);
  }
  return entries;
}

function customPropertyFields(properties, biff = false, primary) {
  if (properties === undefined) return [];
  if (!properties || typeof properties !== 'object' || Array.isArray(properties)) {
    throw failure('UNSUPPORTED_STRUCTURE', 'Expected an object for workbook Custprops');
  }
  const fields = [];
  for (const key of Reflect.ownKeys(properties)) {
    const descriptor = Object.getOwnPropertyDescriptor(properties, key);
    if (typeof key !== 'string' || !key || !Object.hasOwn(descriptor, 'value')) {
      throw failure('INVALID_VALUE', 'Expected a named custom property value');
    }
    if (biff && primary && Object.hasOwn(primary, key)) continue;
    if (biff && isBiffListProperty(key) && Array.isArray(descriptor.value)) {
      for (const item of descriptor.value) {
        if (typeof item !== 'string') throw failure('INVALID_VALUE', 'Expected BIFF document property names');
        fields.push(key, 'item', scalar(item, false));
      }
      continue;
    }
    if (biff && isBiffDerivedProperty(key)) continue;
    const name = scalar(key, false);
    let value = descriptor.value;
    if (biff) {
      if (value == null) continue;
      if (isBiffDateProperty(key) && typeof value === 'string') value = new Date(value);
    }
    if (value instanceof Date) {
      const time = Date.prototype.getTime.call(value);
      if (!Number.isFinite(time)) throw failure('INVALID_VALUE', 'Expected a valid custom property date');
      fields.push(name, 'filetime', String(biff ? time : Math.floor(time / 1000)));
      continue;
    }
    const checked = scalar(value, true);
    const kind = typeof checked === 'number'
      ? ((checked | 0) === checked ? 'i4' : 'r8')
      : typeof checked === 'boolean' ? 'bool' : 'lpwstr';
    fields.push(name, kind, String(checked));
  }
  return fields;
}

function forEachPropertyOverride(entries, hasExisting, apply) {
  for (const [key, value] of entries) {
    const mode = documentPropertyOverrideModes[key];
    if (mode === 'omit' || (mode === 'ifMissing' && hasExisting(key))) continue;
    apply(key, value);
  }
}

function applyPropertyOverrides(document, entries, hasDate) {
  forEachPropertyOverride(entries, hasDate, (key, value) => {
    rustCall('WORKBOOK_EDIT_REFUSED', () => {
      if (document.core_text_property(key) !== value) document.set_core_text_property(key, value);
    });
  });
}

function serializeWorkbook(document, { compression, ignoreEC, themeXLSX, bookSST }) {
  if (themeXLSX) rustCall('WORKBOOK_EDIT_REFUSED', () => document.set_theme_xml(themeXLSX));
  if (ignoreEC !== undefined) {
    rustCall('WORKBOOK_EDIT_REFUSED', () => document.set_ignore_numbers_stored_as_text(ignoreEC));
  }
  return rustCall('WORKBOOK_SAVE_REFUSED', () => {
    if (bookSST !== undefined) return document.to_bytes_with_string_storage(bookSST, false, compression, undefined);
    return compression === undefined ? document.to_bytes() : document.to_bytes_with_compression(compression);
  }).buffer;
}

function writeNew(workbook, { properties: overrides = [], bookSST, compression, ignoreEC, themeXLSX, cellDates, bookType }) {
  const names = data(workbook, 'SheetNames');
  const sheets = data(workbook, 'Sheets');
  const sourceType = data(workbook, 'bookType');
  const xlmlBook = sourceType === 'xlml';
  const textBook = xlmlBook || sourceType === 'html' || sourceType === 'dif' || sourceType === 'sylk' || sourceType === 'rtf' || sourceType === 'fods' || sourceType === 'ods' || sourceType === 'dbf' || sourceType === 'xlsb';
  if (!Array.isArray(names) || !sheets || Reflect.ownKeys(workbook).length !== 2 + Number(Object.hasOwn(workbook, 'Workbook')) + Number(Object.hasOwn(workbook, 'Props')) + Number(Object.hasOwn(workbook, 'Custprops')) + Number(textBook) + Number(xlmlBook && Object.hasOwn(workbook, 'SSF'))) {
    throw failure('UNSUPPORTED_STRUCTURE', 'Expected a scalar workbook');
  }
  if (names.length === 0) throw new Error('Workbook is empty');
  if (new Set(names).size !== names.length || Reflect.ownKeys(sheets).length !== names.length) {
    throw failure('UNSUPPORTED_STRUCTURE', 'Worksheet names and worksheet map must agree');
  }
  const properties = propertyEntries(data(workbook, 'Props'));
  const customFields = customPropertyFields(data(workbook, 'Custprops'));
  const metadata = data(workbook, 'Workbook');
  const definedNames = metadata ? data(metadata, 'Names') : undefined;
  if (definedNames !== undefined && !Array.isArray(definedNames)) {
    throw failure('UNSUPPORTED_STRUCTURE', 'Expected a list of workbook defined names');
  }
  const visibility = visibilityEdits(workbook, names, definedNames);
  const rightToLeft = workbookRightToLeft(metadata);
  const date1904 = dateSystem(workbook);
  // The draft supplies a complete snapshot. Intermediate writes must retain its results.
  const document = rustCall('WORKBOOK_CREATE_REFUSED', () =>
    XlsxDocument.create(Math.floor(Date.now() / 1000), true));
  try {
    if (bookType && bookType !== 'xlsx') {
      rustCall('WORKBOOK_EDIT_REFUSED', () => document.set_template_workbook_type(bookType));
    }
    if (!bookSST) rustCall('WORKBOOK_EDIT_REFUSED', () => document.prune_empty_template_shared_strings());
    for (const [key, value] of properties) {
      rustCall('WORKBOOK_EDIT_REFUSED', () => document.set_core_text_property(key, value));
    }
    if (customFields.length) {
      rustCall('WORKBOOK_EDIT_REFUSED', () => document.add_custom_properties(customFields));
    }
    applyPropertyOverrides(document, overrides, (key) => properties.some(([property]) => property === key));
    if (date1904) rustCall('WORKBOOK_EDIT_REFUSED', () => document.set_date1904(true));
    names.forEach((name, index) => {
      const sheet = data(sheets, name);
      if (typeof name !== 'string' || !name || !sheet || typeof sheet !== 'object') {
        throw failure('UNSUPPORTED_STRUCTURE', 'Expected named worksheets');
      }
      if (!isWellFormedWorksheetName(name)) {
        throw failure('INVALID_SHEET_NAME', 'Worksheet name contains an unpaired surrogate');
      }
      if (index === 0) rustCall('WORKSHEET_CREATE_REFUSED', () => document.rename_initial_worksheet(name));
      else rustCall('WORKSHEET_CREATE_REFUSED', () => document.append_worksheet(name));
    });
    const nameScopes = new Set();
    const nameFields = [];
    const nameScopeIds = [];
    const nameComments = [];
    for (const definedName of definedNames ?? []) {
      const { Name, Ref, Sheet: sheet, Comment: comment } = xlsxDefinedNameSettings(definedName);
      const name = scalar(Name, false);
      const formula = String(scalar(Ref, false));
      if (!formula || (sheet !== undefined && sheet >= names.length)) {
        throw failure('INVALID_VALUE', 'Expected a defined name, reference and optional worksheet scope');
      }
      const key = `${sheet ?? ''}:${name.replace(/[A-Z]/g, (letter) => letter.toLowerCase())}`;
      if (nameScopes.has(key)) throw failure('INVALID_VALUE', 'Duplicate defined name in the same scope');
      nameScopes.add(key);
      nameFields.push(name, formula);
      nameScopeIds.push(sheet === undefined ? 0 : sheet + 1);
      nameComments.push(comment === undefined ? undefined : scalar(comment, false));
    }
    if (nameFields.length) {
      rustCall('WORKBOOK_EDIT_REFUSED', () =>
        document.set_defined_names(nameFields, Uint32Array.from(nameScopeIds), nameComments));
    }

    const validCellShape = xlmlBook ? isXlmlImportedCellShape : isXlsxCellShape;
    names.forEach((name) => {
      const sheet = data(sheets, name);

      const arrayAnchors = new Map();
      const arrayFollowers = new Set();
      const scalarPositions = new Set();
      const scalarCoordinates = [];
      const scalarValues = [];
      function flushScalars() {
        if (!scalarValues.length) return;
        const coordinates = Uint32Array.from(scalarCoordinates);
        rustCall('CELL_EDIT_REFUSED', () => document.write_scalar_batch(name, coordinates, scalarValues, true));
        scalarPositions.clear();
        scalarCoordinates.length = 0;
        scalarValues.length = 0;
      }
      function writeCell(row, column, cell) {
        if (cell === undefined) return;
        if (!validCellShape(cell)) {
          throw failure('UNSUPPORTED_CELL', 'Unsupported new workbook cell metadata');
        }
        const type = data(cell, 't');
        const input = data(cell, 'v');
        const formula = data(cell, 'f');
        const array = data(cell, 'F');
        const dynamic = data(cell, 'D');
        const format = data(cell, 'z');
        const code = numberFormatCode(format ?? (type === 'd' ? 'm/d/yy' : undefined));
        if ((format !== undefined || type === 'd') && !isWritableNumberFormat(code)) {
          throw failure('UNSUPPORTED_FORMAT', 'Expected a supported number format code');
        }
        if ((type === 'n' || type === 's' || type === 'b') &&
            formula === undefined && array === undefined && dynamic === undefined && code === undefined &&
            data(cell, 'l') == null && data(cell, 'c') == null &&
            isWorksheetRow(row) && isWorksheetColumn(column)) {
          const value = scalar(input, true);
          if (type !== cellType(value)) throw failure('INVALID_VALUE', 'Cell type and value do not match');
          const position = row * 16384 + column;
          if (scalarPositions.has(position)) flushScalars();
          scalarPositions.add(position);
          scalarCoordinates.push(row, column);
          scalarValues.push(value);
          return;
        }
        flushScalars();
        const arrayAnchor = recordArrayCell(arrayAnchors, arrayFollowers, row, column,
          { array, dynamic, formula, type, input }, date1904, cellDates);
        if (array === undefined && formula !== undefined) {
          if (typeof formula !== 'string' || !formula.trim()) {
            throw failure('UNSUPPORTED_CELL', 'Expected a nonempty formula');
          }
          const text = scalar(formula, false);
          const { result, resultType } = encodeFormulaResult(type, input, date1904, cellDates);
          rustCall('CELL_EDIT_REFUSED', () => document.write_formula(name, row, column, text, result, resultType));
        } else if (arrayAnchor || (array !== undefined && input == null)) {
          // Anchor results are written with their formulas. Placeholders have no value.
        } else if (type === 'e') {
          const error = errorName(input);
          if (!error) throw failure('INVALID_VALUE', 'Unknown SheetJS error code');
          rustCall('CELL_EDIT_REFUSED', () => document.write_excel_error(name, row, column, error));
        } else if (type === 'd') {
          if (!(input instanceof Date) || !Number.isFinite(Date.prototype.getTime.call(input))) {
            throw failure('INVALID_VALUE', 'Expected a valid Date');
          }
          if (cellDates) {
            const iso = isoCellText(input);
            rustCall('CELL_EDIT_REFUSED', () => document.write_iso_date(name, row, column, iso));
          } else {
            const serial = serialFromDate(Date.prototype.getTime.call(input), date1904);
            rustCall('CELL_EDIT_REFUSED', () => document.write_scalar(name, row, column, serial, false));
          }
        } else if (type !== 'z' || (input != null && input !== '')) {
          const value = scalar(input, true);
          if (type !== cellType(value)) throw failure('INVALID_VALUE', 'Cell type and value do not match');
          rustCall('CELL_EDIT_REFUSED', () => document.write_scalar(name, row, column, value, false));
        }
        if (code !== undefined) {
          rustCall('CELL_EDIT_REFUSED', () => document.set_number_format(name, row, column, code));
        }
        const link = readCellLink(data(cell, 'l'));
        if (link) applyCellLink(document, name, encode_cell({ r: row, c: column }), link);
        const comments = readCellComments(data(cell, 'c'));
        if (comments.length) applyCellComments(document, name, row, column, comments);
      }

      const dense = data(sheet, '!data');
      for (const key of Reflect.ownKeys(sheet)) {
        if (key === '!ref' || key === '!fullref' || key === '!data' || key === '!autofilter' || key === '!merges') continue;
        if (key === '!rows' || key === '!cols' || key === '!margins') continue;
        if (dense !== undefined || typeof key !== 'string') {
          throw failure('UNSUPPORTED_STRUCTURE', 'Unexpected worksheet property');
        }
        const [, row, column] = address(key);
        writeCell(row, column, data(sheet, key));
      }
      if (dense !== undefined) dense.forEach((row, index) => row?.forEach((cell, col) => writeCell(index, col, cell)));
      flushScalars();
      checkArrayFollowers(arrayAnchors, arrayFollowers);
      applyArrayFormulas(document, name, arrayAnchors);
      applyLayout(document, name, layoutEdits(sheet));
      applyMargins(document, name, marginEdits(sheet));
      applyMerges(document, name, [], readMerges(sheet), true);
      const filter = readAutofilter(sheet);
      if (filter !== undefined) {
        rustCall('AUTOFILTER_EDIT_REFUSED', () => document.set_autofilter(name, filter));
      }
    });
    if (rightToLeft) {
      for (const name of names) {
        rustCall('SHEET_LAYOUT_EDIT_REFUSED', () => document.set_sheet_view(name, 0, 'normal', 0, 0, undefined, undefined, true));
      }
    }
    applyVisibility(document, visibility);
    if (bookSST) rustCall('WORKBOOK_EDIT_REFUSED', () => document.prune_empty_template_shared_strings());
    return serializeWorkbook(document, { compression, ignoreEC: ignoreEC ?? true, themeXLSX });
  } finally {
    document.free();
  }
}

function writeArray(workbook, settings = {}) {
  const { properties: overrides = [], compression, ignoreEC, bookSST, themeXLSX, cellDates, bookType } = settings;
  const state = states.get(workbook);
  if (!state && workbook && typeof workbook === 'object') return writeNew(workbook, settings);
  const names = state && data(workbook, 'SheetNames');
  if (!state || data(workbook, 'Sheets') !== state.sheets || !Array.isArray(names) ||
      names.length !== state.names.length || state.names.some((name, index) => data(names, index) !== name) ||
      (state.sheets && Reflect.ownKeys(state.sheets).length !== state.visibleNames.length) ||
      Reflect.ownKeys(workbook).length !== (state.sheets ? 6 : 1) + Number(Object.hasOwn(workbook, 'Workbook')) + Number(state.hasVba) + 2 * Number(state.hasZip)) {
    throw failure('UNSUPPORTED_STRUCTURE', 'Expected the original loaded workbook structure');
  }
  if (bookType && bookType !== 'xlsx' && bookType !== state.xmlType) {
    throw failure('UNSUPPORTED_OPTIONS', 'Changing the format of a loaded workbook is unsupported');
  }
  state.checkProperties?.();
  const date1904 = dateSystem(workbook, state.date1904);
  const dateSystemChanged = date1904 !== state.date1904;
  if (cellDates !== undefined && state.sheets) {
    for (const name of state.visibleNames) void state.sheets[name];
  }
  const changes = [...state.projections]
    .map(([name, projection]) => [name, pending(name, projection, date1904, cellDates)])
    .filter(([, { edits, layoutChanges, marginChanges, filterChanged, removed, added }]) => edits.length || layoutChanges.length || marginChanges || filterChanged || removed.length || added.length);
  const visibility = visibilityEdits(workbook, names, state.nameMetadata, state.visibility);
  checkLoadedWorkbookMetadata(data(workbook, 'Workbook'), state.hasVba);
  if (changes.length === 0 && visibility.length === 0 && !dateSystemChanged && overrides.length === 0 && compression === undefined && ignoreEC === undefined && !themeXLSX && bookSST === undefined) return state.bytes.slice().buffer;
  return withDocument(state.bytes, (document) => {
    applyPropertyOverrides(document, overrides, (key) =>
      rustCall('WORKBOOK_EDIT_REFUSED', () => document.core_text_property(key) !== undefined));
    if (dateSystemChanged) rustCall('WORKBOOK_EDIT_REFUSED', () => document.set_date1904(date1904));
    for (const [name, { edits, arrayAnchors, layoutChanges, marginChanges, filter, filterChanged, removed, added }] of changes) {
      const valueEdits = edits.filter(({ value, replaceSpecial, replaceIso }) => value !== undefined && !replaceSpecial && !replaceIso);
      if (valueEdits.length) {
        const coordinates = Uint32Array.from(valueEdits.flatMap(({ row, column }) => [row, column]));
        const values = valueEdits.map(({ value }) => value);
        rustCall('CELL_EDIT_REFUSED', () => document.write_scalar_batch(name, coordinates, values));
      }
      for (const { row, column, value, explicitDate, format, link, comments, previousComments, formula, error, replaceSpecial, replaceIso } of edits) {
        if (replaceIso) {
          rustCall('CELL_EDIT_REFUSED', () => document.write_iso_date_as_number(name, row, column, value));
        } else if (replaceSpecial && value !== undefined) {
          rustCall('CELL_EDIT_REFUSED', () => document.write_scalar(name, row, column, value, false, true));
        }
        if (explicitDate !== undefined) {
          rustCall('CELL_EDIT_REFUSED', () => document.write_iso_date(name, row, column, explicitDate));
        }
        if (error !== undefined) rustCall('CELL_EDIT_REFUSED', () => document.write_excel_error(name, row, column, error));
        if (formula !== undefined) {
          rustCall('CELL_EDIT_REFUSED', () =>
            document.write_formula_with_result(name, row, column, formula.text, formula.result, formula.resultType));
        }
        if (format !== undefined) {
          rustCall('CELL_EDIT_REFUSED', () => document.set_number_format(name, row, column, format));
        }
        if (link !== undefined) applyCellLink(document, name, encode_cell({ r: row, c: column }), link);
        if (comments !== undefined) applyCellComments(document, name, row, column, comments, previousComments);
      }
      applyArrayFormulas(document, name, arrayAnchors);
      applyLayout(document, name, layoutChanges);
      applyMargins(document, name, marginChanges);
      applyMerges(document, name, removed, added);
      if (filterChanged) {
        rustCall('AUTOFILTER_EDIT_REFUSED', () => document.set_autofilter(name, filter));
      }
    }
    applyVisibility(document, visibility);
    return serializeWorkbook(document, settings);
  });
}

function binaryString(bytes) {
  const chunks = [];
  for (let index = 0; index < bytes.length; index += 16384) {
    chunks.push(String.fromCharCode(...bytes.subarray(index, index + 16384)));
  }
  return chunks.join('');
}

function writeBinarySheet(workbook, settings, format) {
  if (format === 'wk3') return writeWk3(workbook);
  if (compoundBiffVersions.has(format)) {
    const version = compoundBiffVersions.get(format);
    if (version < 8) return writeXls(workbook, version, settings);
    const props = bookProperties(workbook, 'Props');
    const custom = bookProperties(workbook, 'Custprops');
    const properties = [...customPropertyFields(props, true), ...customPropertyFields(custom, true, props)];
    return writeXls(workbook, version, { ...settings, bookType: format }, properties);
  }
  const sheet = selectOutputSheet(workbook, settings);
  if (format === 'dbf') return writeDbf(sheet, settings, codepageTable?.utils, dateSystem(workbook));
  if (format === 'wk1') return writeWk1(sheet);
  return writeEarlyBiff(sheet, earlyBiffVersions.get(format), dateSystem(workbook), settings.WTF);
}

function writePrn(source) {
  const reference = data(source, '!ref');
  if (!reference) return '';
  const { s, e } = decode_range(reference);
  const grid = data(source, '!data');
  const lines = [];
  for (let row = s.r; row <= e.r; row += 1) {
    let line = '';
    for (let column = s.c; column <= e.c; column += 1) {
      const cell = grid ? grid[row]?.[column] : source[encode_cell({ r: row, c: column })];
      if (cell?.v == null) {
        line += '          ';
      } else {
        const text = String(cell.w || formatted(cell, cell.v));
        line += text.slice(0, 10).padEnd(10) + (column === 0 ? ' ' : '');
      }
    }
    lines.push(line);
  }
  return lines.join('\n');
}

// A loaded XLSX exposes its document properties through read-only accessors. As for XLSX
// output, a changed property date is refused.
function bookProperties(workbook, key) {
  if (workbook == null) return undefined;
  const descriptor = Object.getOwnPropertyDescriptor(workbook, key);
  const state = descriptor?.get && states.get(workbook);
  if (!state) return descriptor?.value;
  const value = workbook[key];
  state.checkProperties?.();
  return value;
}

// 0.20.3 write and writeFile check a truthy sheet option for every format before writing.
function checkWriteSheet(workbook, options) {
  const sheet = options && data(options, 'sheet');
  const names = workbook == null ? undefined : data(workbook, 'SheetNames');
  if (!sheet || !Array.isArray(names) || !names.length) return;
  if (!names[typeof sheet === 'number' ? sheet : names.indexOf(sheet)]) {
    throw new Error(`Sheet not found: ${sheet} : ${typeof sheet}`);
  }
}

function selectOutputSheet(workbook, options) {
  const names = data(workbook, 'SheetNames');
  if (!Array.isArray(names) || !names.length) throw new Error('Workbook is empty');
  const { sheet } = options;
  const name = typeof sheet === 'number' ? names[sheet] : sheet ?? names[0];
  if (!names.includes(name)) throw new Error(`Sheet not found: ${sheet} : ${typeof sheet}`);
  const source = workbook.Sheets?.[name];
  if (!source) throw failure('SHEET_READ_REFUSED', `Worksheet is not loaded: ${name}`);
  return source;
}

function writeText(workbook, options, format) {
  const source = selectOutputSheet(workbook, options);
  if (format === 'html') return sheet_to_html(source, options);
  if (format === 'prn') return writePrn(source);
  if (format === 'dif') return writeDif(source, options);
  if (format === 'eth') return writeEth(source);
  if (format === 'rtf') return writeRtf(source);
  if (format === 'sylk' || format === 'slk') return writeSylk(source, dateSystem(workbook));
  return format === 'txt' ? tabSeparatedText(source, options) : sheet_to_csv(source, options);
}

function encodeTextOutput(text, format) {
  if (format !== 'txt') return new TextEncoder().encode(format === 'csv' ? '\ufeff' + text : text);
  const bytes = new Uint8Array((text.length + 1) * 2);
  const words = new DataView(bytes.buffer);
  words.setUint16(0, 0xfeff, true);
  for (let index = 0; index < text.length; index += 1) {
    words.setUint16((index + 1) * 2, text.charCodeAt(index), true);
  }
  return bytes;
}

function encodeOutput(bytes, type) {
  if (type === 'array') return bytes.buffer;
  if (type === 'buffer') return bytes;
  if (type === 'base64' && typeof bytes.toBase64 === 'function') return bytes.toBase64();
  const binary = binaryString(bytes);
  return type === 'base64' ? btoa(binary) : binary;
}

function writeFileOptions(options) {
  const { file, rest } = writeFileOptionFields(options);
  return [checkedDownloadFilename(file), Object.defineProperties({}, rest)];
}

function checkXlsxWriteOptions(options, expected, allowOmission = true) {
  xlsxWriteSettings(options, expected, allowOmission);
  return { bookType: expected.bookType, properties: propertyEntries(expected.Props), compression: expected.compression, ignoreEC: expected.ignoreEC, bookSST: expected.bookSST, themeXLSX: expected.themeXLSX, cellDates: expected.cellDates };
}

function writeOds(workbook, options, expected) {
  odsWriteSettings(options, expected);
  const content = new TextEncoder().encode(writeFods(workbook, true));
  return rustCall('WORKBOOK_SAVE_REFUSED', () => write_ods(content, new TextEncoder().encode(writeOdfMetadata(workbook)), expected.compression === true));
}

function writeXlml(workbook, options, expected) {
  xlmlWriteSettings(options, expected);
  return Array.from(xlmlChunks(workbook, expected)).join('');
}

function writeXlsb(workbook, options, expected) {
  xlsbWriteSettings(options, expected);
  if (!Array.isArray(workbook?.SheetNames) || !workbook.SheetNames.length) throw failure('INVALID_INPUT', 'Workbook is empty');
  const props = bookProperties(workbook, 'Props');
  const properties = new Map(propertyEntries(props, true));
  forEachPropertyOverride(propertyEntries(expected.Props), key => properties.has(key),
    (key, value) => properties.set(key, value));
  const custom = bookProperties(workbook, 'Custprops');
  const parts = xlsbParts(workbook, expected, [...properties].flat(), custom === props ? [] : customPropertyFields(custom));
  return zipFromParts(parts.map(([name]) => name), parts.map(([, bytes]) => bytes),
    expected.compression === true, 'WORKBOOK_SAVE_REFUSED');
}

export function write(workbook, options) {
  const type = options && data(options, 'type');
  if (type === 'file') {
    const [filename, settings] = writeFileOptions(options);
    return writeFileOutput(workbook, filename, settings, 'xlsx');
  }
  const requested = options && data(options, 'bookType');
  const format = resolveWriteFormat(requested);
  checkWriteSheet(workbook, options);
  if (binaryOutputMime.has(format)) {
    const settings = textWriteSettings(options, format);
    validateWriteOutputType(type, 'binarySheet', format);
    return encodeOutput(writeBinarySheet(workbook, settings, format), type);
  }
  if (format === 'xlsb') {
    validateWriteOutputType(type, 'xlsb');
    return encodeOutput(writeXlsb(workbook, options, { bookType: requested, type }), type);
  }
  if (format === 'ods') {
    validateWriteOutputType(type, 'ods');
    return encodeOutput(writeOds(workbook, options, { bookType: requested, type }), type);
  }
  if (format === 'fods') {
    fodsWriteSettings(options, { bookType: requested, type });
    validateWriteOutputType(type, 'fods');
    const text = writeFods(workbook);
    return type === 'string' ? text : encodeOutput(new TextEncoder().encode(text), type);
  }
  if (format === 'xlml' || format === 'xml') {
    validateWriteOutputType(type, 'xlml');
    const text = writeXlml(workbook, options, { bookType: requested, type });
    return type === 'string' ? text : encodeOutput(new TextEncoder().encode(text), type);
  }
  if (textOutputMime.has(format)) {
    const settings = textWriteSettings(options, format);
    validateWriteOutputType(type, 'text');
    const text = writeText(workbook, settings, format);
    return type === 'string' ? text : encodeOutput(encodeTextOutput(text, format), type);
  }
  if (requested !== undefined && !xmlWorkbookMime.has(requested)) {
    throw failure('UNSUPPORTED_OPTIONS', 'Unsupported workbook output format');
  }
  const expected = options && Object.hasOwn(options, 'bookType')
    ? { bookType: requested, type } : { type };
  const settings = checkXlsxWriteOptions(options, expected, false);
  return writeXlsxBytes(workbook, type, settings);
}

function writeXlsxBytes(workbook, type, settings) {
  validateWriteOutputType(type, 'xlsx');
  return encodeOutput(new Uint8Array(writeArray(workbook, settings)), type);
}

export function writeXLSX(workbook, options) {
  const type = options && data(options, 'type');
  if (type === 'file') {
    const [filename, settings] = writeFileOptions(options);
    return writeFileXLSX(workbook, filename, settings);
  }
  const expected = options && Object.hasOwn(options, 'bookType')
    ? { type, bookType: data(options, 'bookType') } : { type };
  const settings = checkXlsxWriteOptions(options, expected, false);
  settings.bookType = 'xlsx';
  return writeXlsxBytes(workbook, type, settings);
}

function downloadOutput(filename, bytes, mime) {
  checkedDownloadFilename(filename);
  if (fileSystem?.writeFileSync) return fileSystem.writeFileSync(filename, new Uint8Array(bytes()));
  if (typeof document === 'undefined' || typeof URL.createObjectURL !== 'function') {
    throw failure('UNSUPPORTED_ENVIRONMENT', 'Downloads require a browser document');
  }
  const blob = new Blob([bytes()], { type: mime });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  try {
    link.href = url;
    link.download = filename;
    link.hidden = true;
    document.body.append(link);
    link.click();
  } finally {
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
}

export function writeFileAsync(filename, workbook, options, callback) {
  if (!fileSystem?.writeFile) {
    throw failure('UNSUPPORTED_ENVIRONMENT', 'Asynchronous file writes require a filesystem supplied through set_fs');
  }
  if (typeof options === 'function') {
    callback = options;
    options = undefined;
  }
  checkedAsyncFileCallback(callback);
  const { bookType, rest } = asyncFileOptionFields(options);
  const format = resolveAsyncWriteFormat(bookType || inferFileOutputFormat(filename));
  const settings = Object.defineProperties({ bookType: format, type: 'buffer' }, rest);
  return fileSystem.writeFile(filename, write(workbook, settings), callback);
}

export function writeFile(workbook, filename, options) {
  return writeFileOutput(workbook, filename, options);
}

function writeFileOutput(workbook, filename, options, defaultFormat) {
  const explicit = options && Object.hasOwn(options, 'bookType');
  const requested = explicit ? data(options, 'bookType') : undefined;
  const format = resolveWriteFormat(requested || defaultFormat || inferFileOutputFormat(filename));
  checkWriteSheet(workbook, options);
  const binaryMime = binaryOutputMime.get(format);
  if (binaryMime) {
    const settings = textWriteSettings(options, format + ':file');
    downloadOutput(filename, () => writeBinarySheet(workbook, settings, format), binaryMime);
    return;
  }
  if (format === 'xlsb') {
    downloadOutput(filename, () => writeXlsb(workbook, options, explicit ? { bookType: requested } : {}),
      documentOutputMime.xlsb);
    return;
  }
  if (format === 'ods') {
    downloadOutput(filename, () => writeOds(workbook, options, explicit ? { bookType: requested } : {}),
      documentOutputMime.ods);
    return;
  }
  if (format === 'fods') {
    fodsWriteSettings(options, explicit ? { bookType: requested } : {});
    downloadOutput(filename, () => new TextEncoder().encode(writeFods(workbook)),
      documentOutputMime.fods);
    return;
  }
  if (format === 'xlml' || format === 'xml') {
    const settings = explicit ? { bookType: requested } : {};
    downloadOutput(filename, () => new TextEncoder().encode(writeXlml(workbook, options, settings)),
      documentOutputMime.xlml);
    return;
  }
  const textMime = textOutputMime.get(format);
  if (textMime) {
    const settings = textWriteSettings(options, format + ':file');
    downloadOutput(filename, () => encodeTextOutput(writeText(workbook, settings, format), format), textMime);
    return;
  }
  const mime = xmlWorkbookMime.get(format);
  if (!mime) throw failure('UNSUPPORTED_OPTIONS', 'Unsupported download file format');
  const settings = checkXlsxWriteOptions(options, explicit ? { bookType: requested } : {});
  settings.bookType = format;
  downloadOutput(filename, () => writeArray(workbook, settings), mime);
}

export { writeFile as writeFileSync };

export function writeFileXLSX(workbook, filename, options) {
  const expected = options && Object.hasOwn(options, 'bookType') ? { bookType: data(options, 'bookType') } : {};
  const settings = checkXlsxWriteOptions(options, expected);
  settings.bookType = 'xlsx';
  downloadOutput(filename, () => writeArray(workbook, settings), xmlWorkbookMime.get('xlsx'));
}

export default {
  parse_zip, parse_xlscfb, read, readFile, readFileSync: readFile, write, writeFile, writeFileSync: writeFile,
  writeFileAsync, writeXLSX, writeFileXLSX, set_fs, set_cptable, utils, stream, SSF,
};
