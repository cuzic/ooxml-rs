import { EventEmitter } from 'node:events';
import { createWriteStream, closeSync, fstatSync, mkdtempSync, openSync, readSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { setImmediate } from 'node:timers/promises';
import { XlsxDocument, XlsxZipReader, excel_hyperlink_address } from '../wasm/ooxml_xlsx_wasm.js';
import { address, data, failure, rustCall } from './contract.js';
import { checkedOutline, checkedStreamingRowHeight, isSpliceCount, isWorksheetGridColumn, rowModelSettings, streamingReaderCellModelSettings, streamingReaderOptions, streamingWorkbookReaderSettings, validateCellStyle, worksheetIterationSettings } from './exceljs-options.generated.js';
import { decodeAlignment, decodeBorder, decodeFill, decodeProtection } from './exceljs-style-read.js';
import { applyDeclaredFontFields, decodeDeclaredFont, decodeDeclaredRichTextRuns, decodeRichTextRuns } from './exceljs-rich-text.js';
import { rowCellColumn, rowModelCells } from './exceljs-address.js';
import { RelationshipType } from './exceljs.js';
import { FormulaType, ValueType, csvTextOf, effectiveTypeOf, escapeCellHtml, projectCellModel, textOf, typeOf } from './exceljs-cell-value.js';
import { encode_col } from './sheetjs-address.js';
import { dateFromIso } from './sheetjs-date.js';
import { formatTable } from './sheetjs-format.js';
import { is_date } from './sheetjs-ssf.js';

function workbookParts(archive) {
  const manifest = archive.workbook_manifest(true);
  const [date1904, strings, styles, names] = manifest;
  const sheets = [];
  for (let index = 4; index < manifest.length; index += 4) {
    const [id, name, state, path] = manifest.slice(index, index + 4);
    sheets.push({ id, name, state, path });
  }
  return { date1904, strings, styles, names, sheets };
}

function decodeSharedString(value, emit = false) {
  if (!Array.isArray(value)) return value;
  const [fields, flags] = value;
  const richText = decodeRichTextRuns(fields);
  for (const [index, run] of richText.entries()) {
    if (run.font) {
      applyDeclaredFontFields(run.font, flags[index]);
      if (!Object.keys(run.font).length) delete run.font;
    }
    if (!run.font && emit) run.font = null;
  }
  return { richText };
}

function readStyles(archive, path) {
  const records = archive.cell_styles(path);
  const readGroup = (index, decode) => records[index].map(fields => fields && decode(fields));
  const fonts = readGroup(0, decodeDeclaredFont);
  const fills = readGroup(1, decodeFill);
  const borders = readGroup(2, decodeBorder);
  const styles = [];
  for (let index = 3; index < records.length; index += 6) {
    const [format, fontId, fillId, borderId, alignment, protection] = records.slice(index, index + 6);
    const groups = {
      // ExcelJS leaves numFmtId 0 (General) out of the style model.
      numFmt: typeof format === 'number' ? format === 0 ? undefined : formatTable[format] : format,
      font: fonts[fontId],
      fill: fills[fillId],
      border: borders[borderId],
      alignment: decodeAlignment(alignment),
      protection: protection && decodeProtection(protection),
    };
    styles.push(Object.fromEntries(Object.entries(groups).filter(([, value]) => value !== undefined)));
  }
  return styles;
}

function readValue(kind, text, format, context) {
  if (text === undefined) return null;
  switch (kind) {
    case 's': {
      const index = Number(text);
      return context.strings ? context.strings[index] : { sharedString: index };
    }
    case 'inlineStr': return Array.isArray(text) ? { richText: decodeDeclaredRichTextRuns(text) } : text;
    case 'str': return text;
    case 'e': return { error: text };
    case 'b': return text === '1' || text === 'true';
    case 'd': return dateFromIso(text, context.date1904);
    default: {
      const value = Number(text);
      if (format && is_date(format)) return new Date(Math.round((value - 25569 + (context.date1904 ? 1462 : 0)) * 86400000));
      return value;
    }
  }
}

function sheetRelationships(archive, path) {
  const records = archive.sheet_hyperlink_relationships(path);
  if (records === undefined) return undefined;
  const relationships = new Map();
  for (let index = 0; index < records.length; index += 3) {
    const [rId, target, targetMode] = records.slice(index, index + 3);
    relationships.set(rId, { type: RelationshipType.Styles, rId, target, targetMode });
  }
  return relationships;
}

function* hyperlinkRecords(rows) {
  const records = rows.take_hyperlinks();
  for (let index = 0; index < records.length; index += 9) {
    const [ref, top, left, bottom, right, rId, location, display, tooltip] = records.slice(index, index + 9);
    yield { ref, top, left, bottom, right, rId, location, display, tooltip };
  }
}

function cachedHyperlinks(archive, path, relationships) {
  const scanner = archive.open_rows(path);
  const cells = new Map();
  const ranges = [];
  try {
    scanner.capture_hyperlinks(true);
    scanner.next_row();
    for (const { top, left, bottom, right, rId, location, display, tooltip } of hyperlinkRecords(scanner)) {
      const hyperlink = excel_hyperlink_address(relationships?.get(rId)?.target, location);
      if (!hyperlink) continue;
      const link = { top, left, bottom, right, hyperlink, display, tooltip };
      if (top === bottom && left === right) cells.set(encode_col(left - 1) + top, link);
      else ranges.push(link);
    }
  } finally {
    scanner.free();
  }
  return (address, row, column) => cells.get(address) ?? ranges.find(link =>
    row >= link.top && row <= link.bottom && column >= link.left && column <= link.right);
}

class HyperlinkReader extends EventEmitter {
  #relationships;

  constructor(workbook, id, relationships) {
    super();
    this.workbook = workbook;
    this.id = id;
    this.options = workbook.options;
    this.#relationships = relationships;
  }

  get count() { return 0; }

  async read() {
    try {
      for (const relationship of this.#relationships.values()) this.emit('hyperlink', relationship);
      this.emit('finished');
    } catch (error) {
      this.emit('error', error);
    }
  }
}

class WorksheetReadColumn {
  #key;

  constructor(worksheet, number) {
    this.worksheet = worksheet;
    this.number = number;
    this.style = {};
    this.hidden = false;
    this.outlineLevel = 0;
    this.collapsed = false;
  }

  get letter() { return encode_col(this.number - 1); }

  get numFmt() { return this.style.numFmt; }

  get font() { return this.style.font; }
  get fill() { return this.style.fill; }
  get border() { return this.style.border; }
  get alignment() { return this.style.alignment; }
  get protection() { return this.style.protection; }

  get key() { return this.#key; }

  set key(value) {
    if (this.#key !== undefined && this.worksheet.getColumnKey(this.#key) === this) {
      this.worksheet.deleteColumnKey(this.#key);
    }
    this.#key = value;
    if (value !== undefined) this.worksheet.setColumnKey(value, this);
  }
}

class WorksheetReadDimensions {
  model = {};

  get top() { return this.model.top ?? 1; }
  get left() { return this.model.left ?? 1; }
  get bottom() { return this.model.bottom ?? 1; }
  get right() { return this.model.right ?? 1; }
  get tl() { return encode_col(this.left - 1) + this.top; }
  get br() { return encode_col(this.right - 1) + this.bottom; }
  get range() { return this.tl + ':' + this.br; }

  expandRow(row) {
    const { min, max } = row.dimensions ?? {};
    if (min === undefined) return;
    this.model.top ??= row.number;
    this.model.bottom = row.number;
    this.model.left = Math.min(this.model.left ?? min, min);
    this.model.right = Math.max(this.model.right ?? max, max);
  }
}

const readerCellState = Symbol('readerCellState');
const readerCellStyleFields = ['numFmt', 'font', 'fill', 'border', 'alignment', 'protection'];

const markReaderCellChanged = Symbol('markReaderCellChanged');
const resolveReaderSharedFormula = Symbol('resolveReaderSharedFormula');

function destroyedReaderCell() {
  throw new TypeError('Cell has been destroyed');
}

function liveReaderCellState(cell) {
  const state = cell[readerCellState];
  if (!state || state.destroyed) destroyedReaderCell();
  return state;
}

function readerCellModelValue(model) {
  const { type, value, rawValue, formula, sharedFormula, result, shareType, ref, text, hyperlink, tooltip } = model;
  if (type === ValueType.Null) return null;
  if (type === ValueType.Formula && (typeof formula === 'string' || typeof sharedFormula === 'string')) {
    const encoded = sharedFormula === undefined ? { formula } : { sharedFormula };
    if (result !== undefined) encoded.result = result;
    if (shareType !== undefined) Object.assign(encoded, { shareType, ref });
    return encoded;
  }
  if (type === ValueType.Hyperlink && typeof hyperlink === 'string') {
    return { text, hyperlink, ...(tooltip === undefined ? {} : { tooltip }) };
  }
  if (type === ValueType.String && Number.isSafeInteger(rawValue?.sharedString) && rawValue.sharedString >= 0) {
    return { sharedString: rawValue.sharedString };
  }
  if (type === typeOf(value) || (type === ValueType.String && typeOf(value) === ValueType.RichText)) return value;
  throw failure('INVALID_VALUE', 'Cell model type does not match its value');
}

function bindReaderCellStyleFields(cell) {
  for (const key of readerCellStyleFields) {
    Object.defineProperty(cell, key, {
      get: () => cell.style[key], set: value => { cell.style[key] = value; }, configurable: true, enumerable: true,
    });
  }
}

function setReaderCellValue(cell, value) {
  const anchor = cell[readerCellState].formulaAnchor;
  if (anchor) {
    anchor.value = value;
    Object.defineProperty(cell, 'value', {
      get: () => anchor.value,
      set: value => { anchor.value = value; },
      configurable: true,
      enumerable: true,
    });
  } else {
    Object.defineProperty(cell, 'value', { value, configurable: true, enumerable: true, writable: true });
  }
}

class WorksheetReadCell {
  get row() { return liveReaderCellState(this).row.number; }

  get col() { return liveReaderCellState(this).column; }

  get worksheet() { return liveReaderCellState(this).row.worksheet; }

  get workbook() { return this.worksheet.workbook; }

  get fullAddress() {
    return { sheetName: this.worksheet.name, address: this.address, row: this.row, col: this.col };
  }

  get $col$row() { return '$' + encode_col(this.col - 1) + '$' + this.row; }

  get master() { return liveReaderCellState(this).master ?? this; }

  get model() {
    if (this.master === this && this.value?.sharedString !== undefined) {
      return {
        address: this.address, type: ValueType.String, value: JSON.stringify(this.value),
        rawValue: this.value, style: this.style,
      };
    }
    return projectCellModel(this, () => this[readerCellState].note);
  }

  set model(input) {
    const state = liveReaderCellState(this);
    const model = streamingReaderCellModelSettings(input);
    if (model.address !== undefined && address(model.address)[0] !== this.address) {
      throw failure('INVALID_ADDRESS', 'Cell model address must match the target');
    }
    const value = readerCellModelValue(model);
    const style = model.style ?? {};
    validateCellStyle(style, true);
    if (model.comment !== undefined && (model.comment?.type !== 'note' || !model.comment.note)) {
      throw failure('INVALID_VALUE', 'Expected a worksheet note model');
    }
    state.master?.releaseMergeRef();
    state.master = undefined;
    state.row[markReaderCellChanged](this, state.column, false);
    setReaderCellValue(this, value);
    Object.defineProperty(this, 'style', { value: style, configurable: true, enumerable: true, writable: true });
    bindReaderCellStyleFields(this);
    if (model.comment !== undefined) state.note = model.comment.note;
  }

  get type() {
    if (this.master !== this) return ValueType.Merge;
    const value = this.value;
    return value?.sharedString !== undefined ? ValueType.String : typeOf(value);
  }

  get effectiveType() {
    const master = this.master;
    if (master !== this) return master.effectiveType;
    const value = this.value;
    return value?.sharedString !== undefined ? ValueType.String : effectiveTypeOf(value);
  }

  get formula() {
    if (this.type !== ValueType.Formula) return undefined;
    const value = this.value;
    if (value.formula != null) return value.formula;
    const state = this[readerCellState];
    return state.row[resolveReaderSharedFormula](value.sharedFormula, this.address, state);
  }

  get result() { return this.type === ValueType.Formula ? this.value.result : undefined; }

  get hyperlink() { return this.type === ValueType.Hyperlink ? this.value.hyperlink : undefined; }

  get formulaType() {
    if (this.type !== ValueType.Formula) return undefined;
    return this.value.sharedFormula === undefined ? FormulaType.Master : FormulaType.Shared;
  }

  get text() {
    const value = this.value;
    if (this.master !== this) return value.toString();
    return value?.sharedString !== undefined ? JSON.stringify(value) : textOf(value);
  }

  get html() { return escapeCellHtml(this.text).replaceAll("'", '&apos;'); }

  get isHyperlink() { return this.type === ValueType.Hyperlink; }

  toString() { return this.text; }

  toCsvString() {
    if (this.master !== this) return '';
    const value = this.value;
    if (value?.richText !== undefined) throw new TypeError('Cannot convert rich text cell to CSV');
    return value?.sharedString !== undefined ? JSON.stringify(value) : csvTextOf(value);
  }

  get isMerged() {
    const state = this[readerCellState];
    if (state.destroyed && state.mergeReferences <= 0) destroyedReaderCell();
    return state.mergeReferences > 0 || state.master !== undefined;
  }

  isMergedTo(master) {
    const state = this[readerCellState];
    if (state.destroyed) destroyedReaderCell();
    return state.master !== undefined && state.master === master;
  }

  addMergeRef() { this[readerCellState].mergeReferences += 1; }

  releaseMergeRef() { this[readerCellState].mergeReferences -= 1; }

  merge(master, ignoreStyle) {
    const state = this[readerCellState];
    if (state.destroyed || !state.row.worksheet) destroyedReaderCell();
    const target = master?.[readerCellState];
    if (!target || target.destroyed) throw failure('MERGE_EDIT_REFUSED', 'Expected a live streaming reader cell');
    for (let ancestor = master; ancestor; ancestor = ancestor[readerCellState]?.master) {
      if (ancestor === this) throw failure('MERGE_EDIT_REFUSED', 'Cell merge would create a cycle');
    }
    let style = ignoreStyle ? this.style : master.style;
    state.master?.releaseMergeRef();
    state.master = master;
    if (state.formulaAnchor) state.formulaAnchor.value = undefined;
    master.addMergeRef();
    state.row[markReaderCellChanged](this, state.column, false);
    Object.defineProperties(this, {
      value: {
        get: () => state.master.value,
        set: value => { state.master.value = value; },
        configurable: true,
        enumerable: true,
      },
      style: {
        get: () => style,
        set: value => { style = value; },
        configurable: true,
        enumerable: true,
      },
    });
    bindReaderCellStyleFields(this);
  }

  unmerge() {
    const state = this[readerCellState];
    if (state.destroyed) destroyedReaderCell();
    if (!state.master) return;
    const { row, column } = state;
    const style = { ...row.worksheet.getColumn(column).style, ...row.style };
    state.master.releaseMergeRef();
    state.master = undefined;
    setReaderCellValue(this, null);
    Object.defineProperty(this, 'style', { value: style, configurable: true, enumerable: true, writable: true });
  }

  destroy() {
    const state = this[readerCellState];
    if (state.destroyed) return;
    state.row[markReaderCellChanged](this, state.column, true);
    state.destroyed = true;
    if (state.formulaAnchor) state.formulaAnchor.value = undefined;
    state.master = undefined;
    state.row = undefined;
    delete this.address;
    delete this.style;
    for (const key of ['value', 'formula', 'result', 'hyperlink']) {
      Object.defineProperty(this, key, {
        get: destroyedReaderCell, set: destroyedReaderCell, configurable: true, enumerable: true,
      });
    }
  }
}

class WorksheetReadRow {
  #cells = new Map();
  #cellCount = 0;
  #orderedCells = true;
  #dimensions;
  #hasDestroyedCell = false;
  #cellValuesMayHaveChanged = false;
  #sharedFormulas;
  #formulaAnchors;

  constructor(worksheet, row, context, sharedFormulas, hyperlinkAt) {
    this.worksheet = worksheet;
    this.#sharedFormulas = sharedFormulas;
    const [number, height, styleId, hidden, outlineLevel, collapsed] = row;
    this.number = number;
    this.height = height;
    this.hidden = hidden;
    this.outlineLevel = outlineLevel;
    this.collapsed = collapsed;
    this.style = { ...context.styles?.[styleId] };
    Object.assign(this, this.style);
    for (let index = 6; index < row.length; index += 6) {
      const [column, kind, style, text, formula, shared] = row.slice(index, index + 6);
      const address = encode_col(column - 1) + this.number;
      // ExcelJS reads a cell with format 0 as unstyled.
      const cellStyle = style ? { ...context.styles?.[style] } : {};
      const { numFmt } = cellStyle;
      const result = readValue(kind, text, numFmt, context);
      let value = result;
      if (formula !== undefined) {
        if (Array.isArray(formula)) {
          value = { formula: formula[0], shareType: shared === undefined ? 'array' : 'shared', ref: formula[1] };
        } else {
          value = shared !== undefined && !formula ? { sharedFormula: sharedFormulas.anchors.get(shared) } : { formula };
        }
        if (text !== undefined) value.result = result;
        if (formula && shared !== undefined) {
          const anchor = { address, value };
          sharedFormulas.anchors.set(shared, address);
          sharedFormulas.sources.set(address, anchor);
          (this.#formulaAnchors ??= new Map()).set(column, anchor);
        }
      }
      const link = hyperlinkAt?.(address, this.number, column);
      if (link) {
        const text = value == null ? link.display ?? link.hyperlink : String(value);
        value = { text, hyperlink: link.hyperlink };
        if (link.tooltip !== undefined) value.tooltip = link.tooltip;
      }
      if (value != null) {
        this.#dimensions ??= { min: column, max: column };
        this.#dimensions.max = column;
      }
      this.#orderedCells &&= column > this.#cellCount;
      this.#cellCount = Math.max(this.#cellCount, column);
      this.#cells.set(column, {
        address, value, ...cellStyle, style: cellStyle,
      });
    }
  }

  destroy() {
    delete this.worksheet;
    delete this.style;
    this.#cells = undefined;
    this.#dimensions = undefined;
  }

  get dimensions() {
    if (!this.#cells) throw new TypeError('Row has been destroyed');
    if (this.#hasDestroyedCell) throw new TypeError('Cell has been destroyed');
    if (!this.#cellValuesMayHaveChanged) return this.#dimensions;
    let min = Infinity;
    let max = 0;
    for (const [column, cell] of this.#cells) {
      if (!this.#hasCellValue(cell)) continue;
      min = Math.min(min, column);
      max = Math.max(max, column);
    }
    return max ? { min, max } : undefined;
  }

  get hasValues() {
    if (!this.#cells) return false;
    for (const cell of this.#cells.values()) {
      if (this.#hasCellValue(cell)) return true;
    }
    return false;
  }

  #hasCellValue(cell) {
    return cell.value != null || cell[readerCellState]?.master !== undefined;
  }

  [resolveReaderSharedFormula](anchor, member, state) {
    const source = this.#sharedFormulas?.sources.get(anchor);
    const text = source?.value?.formula;
    if (text === undefined) return undefined;
    const from = source.address;
    const cached = state.formulaCache;
    if (cached?.from === from && cached.member === member && cached.text === text) return cached.formula;
    const formula = rustCall('CELL_READ_REFUSED', () =>
      XlsxDocument.translate_detached_formula(text, from, member, this.#sharedFormulas.names));
    state.formulaCache = { from, member, text, formula };
    return formula;
  }

  [markReaderCellChanged](cell, column, destroyed) {
    if (this.#cells?.get(column) !== cell) return;
    this.#cellValuesMayHaveChanged = true;
    this.#hasDestroyedCell ||= destroyed;
  }

  #exposeCell(cell, column) {
    if (!cell[readerCellState]) {
      this.#cellValuesMayHaveChanged = true;
      Object.setPrototypeOf(cell, WorksheetReadCell.prototype);
      const formulaAnchor = this.#formulaAnchors?.get(column);
      Object.defineProperty(cell, readerCellState, {
        value: { row: this, column, master: undefined, mergeReferences: 0, destroyed: false, formulaAnchor },
      });
      if (formulaAnchor) setReaderCellValue(cell, cell.value);
    }
    return cell;
  }

  #cellsInColumnOrder() {
    return this.#orderedCells ? this.#cells : [...this.#cells].sort(([a], [b]) => a - b);
  }

  get model() {
    if (!this.#cells.size && !this.height) return null;
    let min = 0;
    const cells = [];
    for (const [column, cell] of this.#cellsInColumnOrder()) {
      if (!min) min = column;
      cells.push(this.#exposeCell(cell, column).model);
    }
    return {
      cells, number: this.number, min, max: this.#cellCount,
      height: this.height, style: this.style, hidden: this.hidden,
      outlineLevel: this.outlineLevel, collapsed: this.collapsed,
    };
  }

  set model(input) {
    if (!this.#cells) throw new TypeError('Row has been destroyed');
    const model = rowModelSettings(input);
    if (model.number !== this.number) throw failure('INVALID_VALUE', 'Invalid row number in model');
    const height = checkedStreamingRowHeight(model.height) || undefined;
    const outlineLevel = checkedOutline(model.outlineLevel ?? 0);
    const style = model.style ?? {};
    validateCellStyle(style, true);
    this.#replaceCells(staged => {
      for (const { column, model: cell } of rowModelCells(this.number, model.cells, address, streamingReaderCellModelSettings)) {
        staged.getCell(column).model = cell;
      }
    });
    this.height = height;
    this.hidden = model.hidden ?? false;
    this.outlineLevel = outlineLevel;
    this.collapsed = outlineLevel > 0;
    this.style = style;
    for (const key of readerCellStyleFields) this[key] = style[key];
  }

  get values() {
    const values = [];
    for (const [column, cell] of this.#cells) {
      if (cell.value != null) values[column] = cell.value;
    }
    return values;
  }

  set values(input) {
    this.#replaceCells(staged => {
      const assign = (column, value) => {
        if (value === undefined) return;
        const cell = staged.getCell(column);
        cell.value = value;
      };
      if (Array.isArray(input)) {
        const offset = Number(Object.hasOwn(input, '0'));
        for (const key of Object.keys(input)) {
          const index = Number(key);
          if (Number.isInteger(index) && index >= 0 && index < input.length && String(index) === key) {
            assign(index + offset, data(input, key));
          }
        }
      } else if (input != null) {
        if (typeof input !== 'object') throw failure('INVALID_VALUE', 'Expected row values as an array or a keyed object');
        this.worksheet.eachColumnKey((column, key) => assign(column.number, data(input, key)));
      }
    });
  }

  #detachFormulaAnchor(column) {
    const anchor = this.#formulaAnchors?.get(column);
    if (!anchor) return;
    const cell = this.#cells.get(column);
    const state = cell?.[readerCellState];
    if (state) {
      state.formulaAnchor = undefined;
      if (!state.destroyed && !state.master) setReaderCellValue(cell, cell.value);
    }
    anchor.value = undefined;
  }

  #replaceCells(populate) {
    if (!this.#cells) throw new TypeError('Row has been destroyed');
    const staged = new WorksheetReadRow(this.worksheet, [this.number], {}, this.#sharedFormulas);
    populate(staged);
    for (const cell of staged.#cells.values()) cell[readerCellState].row = this;
    for (const [column, anchor] of this.#formulaAnchors ?? []) {
      this.#detachFormulaAnchor(column);
      const nextCell = staged.#cells.get(column);
      if (nextCell) {
        nextCell[readerCellState].formulaAnchor = anchor;
        setReaderCellValue(nextCell, nextCell.value);
      }
    }
    this.#cells = staged.#cells;
    this.#cellCount = staged.#cellCount;
    this.#orderedCells = staged.#orderedCells;
    this.#hasDestroyedCell = false;
    this.#cellValuesMayHaveChanged = true;
  }

  splice(start, count, ...inserts) {
    if (!this.#cells) throw new TypeError('Row has been destroyed');
    if (!isWorksheetGridColumn(start) || !isSpliceCount(count) ||
        !isWorksheetGridColumn(Math.max(start, start + Math.max(count, inserts.length) - 1))) {
      throw failure('INVALID_ADDRESS', 'Invalid row splice range');
    }
    const delta = inserts.length - count;
    const keep = start + count;
    const end = this.#cellCount;
    if (delta > 0 && keep <= end && !isWorksheetGridColumn(end + delta)) {
      throw failure('INVALID_ADDRESS', 'Row splice exceeds the XLSX column limit');
    }
    const updates = new Map();
    if (delta !== 0) {
      const first = delta < 0 ? start + inserts.length : keep + delta;
      const last = delta < 0 ? end : end + delta;
      for (let column = first; column <= last; column += 1) {
        const source = this.#cells.get(column - delta);
        if (source) {
          updates.set(column, {
            value: source.value, style: source.style, note: source[readerCellState]?.note,
            anchor: this.#formulaAnchors?.get(column - delta),
          });
        } else if (delta > 0 || this.#cells.has(column)) {
          updates.set(column, delta > 0 ? undefined : { value: null, style: {} });
        }
      }
    }
    for (let index = 0; index < inserts.length; index += 1) {
      updates.set(start + index, { value: inserts[index] ?? null, style: {} });
    }
    if (this.#formulaAnchors) {
      for (const column of updates.keys()) {
        this.#detachFormulaAnchor(column);
        this.#formulaAnchors.delete(column);
      }
    }
    for (const [column, update] of updates) {
      if (!update) {
        this.#cells.delete(column);
        continue;
      }
      const cell = this.getCell(column);
      cell.value = update.value;
      cell.style = update.style;
      const state = cell[readerCellState];
      state.note = update.note;
      if (update.anchor) {
        const anchor = update.anchor;
        anchor.address = cell.address;
        this.#formulaAnchors.set(column, anchor);
        state.formulaAnchor = anchor;
        if (!state.master) setReaderCellValue(cell, cell.value);
      }
      bindReaderCellStyleFields(cell);
    }
    if (delta > 0 && keep <= end) this.#cellCount = Math.max(this.#cellCount, end + delta);
    this.#cellValuesMayHaveChanged = true;
  }

  getCell(column) {
    if (typeof column === 'string') {
      column = this.worksheet.getColumn(column).number;
    }
    if (!isWorksheetGridColumn(column)) throw failure('INVALID_ADDRESS', 'Invalid XLSX column');
    if (!this.#cells.has(column)) {
      this.#orderedCells &&= column > this.#cellCount;
      this.#cellCount = Math.max(this.#cellCount, column);
      this.#cells.set(column, {
        address: encode_col(column - 1) + this.number, value: null, style: {},
      });
    }
    return this.#exposeCell(this.#cells.get(column), column);
  }

  getCellEx(location) { return this.getCell(rowCellColumn(this.number, location)); }

  findCell(column) {
    const cell = this.#cells.get(column);
    return cell && this.#exposeCell(cell, column);
  }

  get cellCount() {
    if (!this.#cells) throw new TypeError('Row has been destroyed');
    return this.#cellCount;
  }

  get actualCellCount() {
    let count = 0;
    for (const cell of this.#cells.values()) {
      if (this.#hasCellValue(cell)) count += 1;
    }
    return count;
  }

  eachCell(options, callback) {
    if (typeof options === 'function') [options, callback] = [undefined, options];
    if (worksheetIterationSettings(options).includeEmpty) {
      const count = this.cellCount;
      for (let column = 1; column <= count; column += 1) callback(this.getCell(column), column);
      return;
    }
    for (const [column, cell] of this.#cellsInColumnOrder()) {
      if (this.#hasCellValue(cell)) callback(this.#exposeCell(cell, column), column);
    }
  }
}

class WorksheetReader extends EventEmitter {
  #context;
  #path;
  #mode;
  #relationships;
  #columns = null;
  #keys = new Map();
  #dimensions = new WorksheetReadDimensions();

  constructor(workbook, sheet, context, mode, relationships) {
    super();
    this.workbook = workbook;
    this.id = sheet.id;
    this.name = sheet.name;
    this.state = sheet.state;
    this.#context = context;
    this.#path = sheet.path;
    this.#mode = mode;
    this.#relationships = relationships;
  }

  get columns() { return this.#columns; }

  get dimensions() { return this.#dimensions; }

  getColumn(column) {
    if (typeof column === 'string') {
      const keyed = this.getColumnKey(column);
      if (keyed) return keyed;
      if (!/^[a-z]{1,3}$/i.test(column)) throw failure('INVALID_ADDRESS', 'Expected column letters');
      column = address(column + '1')[2] + 1;
    }
    if (!isWorksheetGridColumn(column)) throw failure('INVALID_ADDRESS', 'Invalid XLSX column');
    this.#columns ??= [];
    while (this.#columns.length < column) {
      this.#columns.push(new WorksheetReadColumn(this, this.#columns.length + 1));
    }
    return this.#columns[column - 1];
  }

  getColumnKey(key) { return this.#keys.get(key); }

  setColumnKey(key, column) { this.#keys.set(key, column); }

  deleteColumnKey(key) { this.#keys.delete(key); }

  eachColumnKey(callback) {
    for (const [key, column] of this.#keys) callback(column, key);
  }

  destroy() { throw new Error('Invalid Operation: destroy'); }

  #readColumns(rows, context) {
    const records = rows.take_columns();
    for (let index = 0; index < records.length; index += 7) {
      const [first, last, width, style, hidden, outlineLevel, collapsed] = records.slice(index, index + 7);
      const columnStyle = context.styles?.[style];
      for (let number = first; number <= last; number++) {
        const column = this.getColumn(number);
        column.width = width;
        column.style = { ...columnStyle };
        column.hidden = hidden;
        column.outlineLevel = outlineLevel;
        column.collapsed = collapsed;
      }
    }
  }

  async *#iterate(events) {
    const context = this.#context;
    if (!context.archive) throw new Error('Workbook reader is closed');
    const hyperlinkAt = this.#mode === 'cache'
      ? cachedHyperlinks(context.archive, this.#path, this.#relationships)
      : undefined;
    const rows = context.archive.open_rows(this.#path);
    const sharedFormulas = { anchors: new Map(), sources: new Map(), names: context.names };
    let count = 0;
    let readColumns = false;
    this.#dimensions.model = {};
    try {
      if (events && this.#mode === 'emit') rows.capture_hyperlinks(false);
      while (context.archive) {
        const row = rows.next_row(true);
        if (!readColumns) {
          this.#readColumns(rows, context);
          readColumns = true;
        }
        if (row.length) {
          if (row.length > 6) this.getColumn(row[row.length - 6]);
          const value = new WorksheetReadRow(this, row, context, sharedFormulas, hyperlinkAt);
          this.#dimensions.expandRow(value);
          yield events ? [{ eventType: 'row', value }] : value;
          if (++count % 256 === 0) await setImmediate();
        } else {
          for (const { ref, rId } of hyperlinkRecords(rows)) {
            yield [{ eventType: 'hyperlink', value: { ref, rId } }];
          }
          return;
        }
      }
      throw new Error('Workbook reader is closed');
    } finally {
      rows.free();
    }
  }

  parse() { return this.#iterate(true); }

  [Symbol.asyncIterator]() { return this.#iterate(false); }

  async read() {
    try {
      for await (const events of this.parse()) {
        for (const { eventType, value } of events) this.emit(eventType, value);
      }
      this.emit('finished');
    } catch (error) {
      this.emit('error', error);
    }
  }
}

export class WorkbookReader extends EventEmitter {
  static Options = streamingReaderOptions;

  constructor(input, options = {}) {
    super();
    this.input = input;
    this.options = streamingWorkbookReaderSettings(options);
  }

  async *parse(input = this.input, options) {
    if (options !== undefined) this.options = streamingWorkbookReaderSettings(options);
    const settings = this.options;
    this.sharedStrings = undefined;
    this.properties = undefined;
    this.model = undefined;
    let directory, fd;
    const context = {};
    try {
      let path = input;
      if (input instanceof Readable) {
        directory = mkdtempSync(join(tmpdir(), 'ooxml-xlsx-reader-'));
        path = join(directory, 'source.xlsx');
        await pipeline(input, createWriteStream(path));
      } else if (typeof path !== 'string') {
        throw failure('INVALID_INPUT', 'Expected an XLSX filename or a Node Readable stream');
      }
      fd = openSync(path, 'r');
      const { size } = fstatSync(fd);
      const buffer = Buffer.allocUnsafe(65536);
      const archive = new XlsxZipReader(size, (offset, length) => (
        buffer.subarray(0, readSync(fd, buffer, 0, length, offset))
      ));
      context.archive = archive;
      const parts = workbookParts(archive);
      if (settings.entries === 'emit') this.emit('entry', { type: 'workbook' });
      context.date1904 = parts.date1904;
      this.properties = { model: { date1904: parts.date1904 } };
      context.names = parts.names.length ? parts.names : undefined;
      this.model = { sheets: parts.sheets };
      if (parts.strings) {
        if (settings.entries === 'emit') this.emit('entry', { type: 'shared-strings' });
        if (settings.sharedStrings === 'cache') {
          const strings = archive.shared_text(parts.strings, true);
          for (let index = 0; index < strings.length; index++) {
            strings[index] = decodeSharedString(strings[index]);
          }
          context.strings = this.sharedStrings = strings;
        }
        if (settings.sharedStrings === 'emit') {
          const strings = archive.shared_text(parts.strings, true);
          for (const [index, entry] of strings.entries()) {
            yield { eventType: 'shared-strings', value: { index, text: decodeSharedString(entry, true) } };
          }
        }
      }
      if (parts.styles) {
        if (settings.entries === 'emit') this.emit('entry', { type: 'styles' });
        if (settings.styles === 'cache') context.styles = readStyles(archive, parts.styles);
      }
      for (const sheet of parts.sheets) {
        if (settings.entries === 'emit') this.emit('entry', { type: 'worksheet', id: String(sheet.id) });
        const relationships = settings.hyperlinks !== 'ignore' || settings.entries === 'emit'
          ? sheetRelationships(archive, sheet.path)
          : undefined;
        if (relationships) {
          if (settings.entries === 'emit') this.emit('entry', { type: 'hyperlinks', id: String(sheet.id) });
          if (settings.hyperlinks === 'emit') {
            yield { eventType: 'hyperlinks', value: new HyperlinkReader(this, String(sheet.id), relationships) };
          }
        }
        if (settings.worksheets === 'emit') {
          yield { eventType: 'worksheet', value: new WorksheetReader(this, sheet, context, settings.hyperlinks, relationships) };
        }
      }
    } finally {
      try {
        context.archive?.free();
      } finally {
        context.archive = undefined;
        try {
          if (fd !== undefined) closeSync(fd);
        } finally {
          if (directory) rmSync(directory, { force: true, recursive: true });
        }
      }
    }
  }

  async *[Symbol.asyncIterator]() {
    for await (const { eventType, value } of this.parse()) {
      if (eventType === 'worksheet') yield value;
    }
  }

  async read(input, options) {
    try {
      for await (const { eventType, value } of this.parse(input, options)) {
        this.emit(eventType, value);
        if (eventType === 'worksheet') await value.read();
      }
      this.emit('end');
      this.emit('finished');
    } catch (error) {
      this.emit('error', error);
    }
  }
}
