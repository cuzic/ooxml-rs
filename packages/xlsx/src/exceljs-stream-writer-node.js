import { createReadStream, createWriteStream, mkdtempSync, appendFileSync, existsSync, rmSync, truncateSync } from 'node:fs';
import { Buffer } from 'node:buffer';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Duplex, Readable, Writable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { isDeepStrictEqual } from 'node:util';
import { XlsxDocument, XlsxStreamingStrings, XlsxZipReader, XlsxZipStream, encode_xlsx_row, encode_xlsx_typed_row } from '../wasm/ooxml_xlsx_wasm.js';
import { applyStreamingHyperlinks, applyStreamingNotes, data, failure, prepareStreamingCellModel, registerStreamingStyle, replaceCellNames, scalar, translateStreamingFormula, validateStreamingRowStyle } from './contract.js';
import { cellHyperlinkSettings, cellModelSettings, cellRichTextSettings, defaultCellStyleSettings, checkedOutline, checkedStreamingRowHeight, checkedStreamingSharedStrings, checkedStreamingZipCommentLength, isSpliceCount, isWorksheetGridColumn, isWorksheetGridRow, readCellFormulaResult, readSharedFormulaResult, rowModelSettings, streamingWorkbookWriterSettings, streamingWorksheetOptions, streamingZipSettings, streamingZlibSettings, workbookPropertySettings, worksheetIterationSettings } from './exceljs-options.generated.js';
import { csvTextOf, effectiveTypeOf, escapeCellHtml, excelDateMillis, formulaErrorCode, FormulaType, projectCellModel, textOf, typeOf, ValueType } from './exceljs-cell-value.js';
import { decode_range, encode_col, encode_range } from './sheetjs-address.js';
import { gridAddress, mergeReference, rowCellColumn, rowModelCells, worksheetCellAddress, worksheetDimensions } from './exceljs-address.js';

import { encodeRichTextRuns } from './exceljs-rich-text.js';

const encoder = new TextEncoder();
const decoder = new TextDecoder();

const dateStyle = 1;
const rowChunkSize = 64 * 1024;

function typedValue(value) {
  if (value == null) return { value: null, kind: 0 };
  if (value instanceof Date) return { value: excelDateMillis(value), kind: 2 };
  const error = formulaErrorCode(value);
  if (error !== undefined) return { value: error, kind: 1 };
  return { value: scalar(value, true), kind: 0 };
}

function streamingFormulaRange(value, shareType, row, column) {
  const ref = data(value, 'ref');
  if (typeof ref !== 'string' || ref.split(':').length > 2) {
    throw failure('INVALID_ADDRESS', 'Expected an A1 range for the formula');
  }
  const reference = mergeReference([ref]);
  const coordinates = decode_range(reference);
  if (coordinates.s.r !== row - 1 || coordinates.s.c !== column - 1) {
    throw failure('INVALID_ADDRESS', `A ${shareType} formula must start at its anchor cell`);
  }
  return { reference, coordinates };
}

function worksheetOptions(options, useSharedStrings) {
  if (!options || typeof options !== 'object' ||
      !(Object.hasOwn(options, 'useSharedStrings') || Object.hasOwn(options, 'tabColor'))) {
    return { options, useSharedStrings };
  }
  const { useSharedStrings: setting, tabColor, rest } = streamingWorksheetOptions(options);
  if (tabColor) {
    const properties = rest.properties?.value;
    rest.properties = {
      configurable: true,
      enumerable: true,
      writable: true,
      value: Object.defineProperties({ tabColor }, Object.getOwnPropertyDescriptors(properties ?? {})),
    };
  }
  return {
    options: Object.defineProperties({}, rest),
    useSharedStrings: checkedStreamingSharedStrings(setting) ?? useSharedStrings,
  };
}

function rangesOverlap(left, right) {
  return left.s.r <= right.e.r && left.e.r >= right.s.r &&
    left.s.c <= right.e.c && left.e.c >= right.s.c;
}

class BufferedOutput extends Duplex {
  _read() {}
  _write(chunk, _encoding, done) {
    this.push(chunk);
    done();
  }
  _final(done) {
    this.push(null);
    done();
  }
}

class StreamingStyledObject {
  #style;

  get style() { return this.#style ??= {}; }
  set style(value) { this.#style = value; }

  get styleForEncoding() { return this.#style; }

  get numFmt() { return this.style.numFmt; }
  set numFmt(value) { this.style.numFmt = value; }
  get font() { return this.style.font; }
  set font(value) { this.style.font = value; }
  get fill() { return this.style.fill; }
  set fill(value) { this.style.fill = value; }
  get border() { return this.style.border; }
  set border(value) { this.style.border = value; }
  get alignment() { return this.style.alignment; }
  set alignment(value) { this.style.alignment = value; }
  get protection() { return this.style.protection; }
  set protection(value) { this.style.protection = value; }
}

const directStreamingMerge = Symbol('directStreamingMerge');

class StreamingCell extends StreamingStyledObject {
  #directMaster;
  #directStyle;
  #sheet;
  #row;
  #column;
  #value;
  #mergeReferences = 0;

  constructor(sheet, row, column, value = null) {
    super();
    this.#sheet = sheet;
    this.#row = row;
    this.#column = column;
    this.#value = value;
    this.address = encode_col(column - 1) + row;
  }

  destroy() {
    super.style = undefined;
    this.#sheet = undefined;
    this.#value = undefined;
    this.#directMaster = undefined;
    delete this.address;
  }

  get worksheet() {
    if (!this.#sheet) throw new TypeError('Cell has been destroyed');
    return this.#sheet;
  }

  get workbook() { return this.#sheet.workbook; }

  get row() {
    if (!this.#sheet) throw new TypeError('Cell has been destroyed');
    return this.#row;
  }

  get col() {
    if (!this.#sheet) throw new TypeError('Cell has been destroyed');
    return this.#column;
  }

  get $col$row() { return '$' + encode_col(this.col - 1) + '$' + this.row; }

  get fullAddress() {
    return { sheetName: this.#sheet.name, address: this.address, row: this.#row, col: this.#column };
  }

  get name() { return this.names[0]; }

  set name(value) { this.names = [value]; }

  get names() { return this.workbook.definedNames.getNamesEx(this.fullAddress); }

  set names(value) { this.workbook.definedNames[replaceCellNames](this.fullAddress, value); }

  addName(name) { this.workbook.definedNames.addEx(this.fullAddress, name); }

  removeName(name) { this.workbook.definedNames.removeEx(this.fullAddress, name); }

  removeAllNames() { this.names = []; }

  get [directStreamingMerge]() { return this.#directMaster !== undefined; }

  get master() { return this.worksheet.mergeAt(this.#row, this.#column)?.master ?? this.#directMaster ?? this; }

  merge(master, ignoreStyle) {
    const sheet = this.worksheet;
    if (!(master instanceof StreamingCell) || !master.#sheet || master.#sheet !== sheet ||
        sheet.rowCommitted(this.#row) || sheet.rowCommitted(master.#row) ||
        sheet.mergeAt(this.#row, this.#column) || sheet.mergeAt(master.#row, master.#column)) {
      throw failure('MERGE_EDIT_REFUSED', 'Expected cells in uncommitted rows outside worksheet merges');
    }
    for (let ancestor = master; ancestor; ancestor = ancestor.#directMaster) {
      if (ancestor === this) throw failure('MERGE_EDIT_REFUSED', 'Cell merge would create a cycle');
    }
    const style = ignoreStyle ? this.styleForEncoding : undefined;
    this.#directMaster?.releaseMergeRef();
    this.#directMaster = master;
    this.#directStyle = !ignoreStyle;
    this.#value = null;
    super.style = style;
    master.addMergeRef();
  }

  unmerge() {
    if (!this.#directMaster) return;
    const sheet = this.worksheet;
    if (sheet.rowCommitted(this.#row)) throw new Error('Out of bounds: this row has been committed');
    this.#directMaster.releaseMergeRef();
    this.#directMaster = undefined;
    super.style = { ...sheet.getColumn(this.#column).style, ...sheet.getRow(this.#row).style };
  }

  addMergeRef() { this.#mergeReferences += 1; }
  releaseMergeRef() { this.#mergeReferences -= 1; }
  get isMerged() { return this.#directMaster !== undefined || this.#mergeReferences > 0 || this.#sheet.mergeAt(this.#row, this.#column) !== undefined; }

  isMergedTo(cell) {
    const master = this.master;
    return master !== this && master === cell;
  }

  get style() {
    if (!this.#sheet) return undefined;
    if (this.#directMaster && !this.#directStyle) return super.style;
    return this.master === this ? super.style : this.master.style;
  }

  get styleForEncoding() {
    if (this.#directMaster && !this.#directStyle) return super.styleForEncoding;
    return this.master === this ? super.styleForEncoding : this.master.styleForEncoding;
  }

  set style(value) {
    if (this.#directMaster) {
      this.#directStyle = false;
      super.style = value;
    } else if (this.master === this) super.style = value;
    else this.master.style = value;
  }

  get dataValidation() { return this.#sheet.dataValidations.find(this.address); }

  set dataValidation(value) {
    if (this.#sheet.committed) throw new Error('Worksheet is already committed');
    this.#sheet.writeCellValidation(this.address, value);
  }

  get model() {
    return projectCellModel(this, () => this.#sheet.readNoteModel(this.address, this.#row, this.#column));
  }

  set model(input) {
    if (this.#sheet.rowCommitted(this.#row)) throw new Error('Out of bounds: this row has been committed');
    if (this.master !== this) throw failure('UNSUPPORTED_OPTIONS', 'Cannot set a merged follower cell model');
    const model = cellModelSettings(input);
    if (model.address !== undefined && model.address !== this.address) {
      throw failure('INVALID_ADDRESS', 'Cell model address must match the target');
    }
    if (model.type === ValueType.Merge) throw failure('UNSUPPORTED_OPTIONS', 'Cell model cannot change merge membership');
    const { value, style, note } = this.#sheet[prepareStreamingCellModel](model);
    if (note !== undefined) this.note = note;
    this.style = style;
    this.value = value;
  }

  get note() { return this.#sheet.readNote(this.address); }

  set note(value) {
    if (this.#sheet.rowCommitted(this.#row)) throw new Error('Out of bounds: this row has been committed');
    this.#sheet.writeNote(this.#row, this.#column, this.address, value);
  }

  get type() { return this.master === this ? typeOf(this.value) : ValueType.Merge; }

  get effectiveType() { return effectiveTypeOf(this.value); }

  get formula() {
    if (this.type !== ValueType.Formula) return undefined;
    const value = this.value;
    return value.formula ?? this.#sheet.resolveSharedFormula(value.sharedFormula, this.address);
  }

  get result() { return this.type === ValueType.Formula ? this.value.result : undefined; }

  get formulaType() {
    if (this.type !== ValueType.Formula) return undefined;
    const value = this.value;
    if (value.formula) return FormulaType.Master;
    if (value.sharedFormula) return FormulaType.Shared;
    return FormulaType.None;
  }

  get text() {
    const value = this.value;
    return this.master === this ? textOf(value) : value.toString();
  }

  get html() { return escapeCellHtml(this.text); }

  get isHyperlink() { return this.type === ValueType.Hyperlink; }

  toString() { return this.text; }

  toCsvString() { return this.master === this ? csvTextOf(this.value, true) : ''; }

  get hyperlink() {
    const value = this.value;
    return value && typeof value === 'object' ? data(value, 'hyperlink') : undefined;
  }

  get value() {
    const master = this.master;
    return master === this ? this.#value : master.value;
  }

  set value(value) {
    const master = this.master;
    if (master !== this) {
      master.value = value;
      return;
    }
    if (this.#sheet.rowCommitted(master.#row)) throw new Error('Out of bounds: this row has been committed');
    master.#value = value;
    if (value !== null && typeof value === 'object') this.#sheet.markObjectValue(master.#row);
  }
}

class StreamingRow extends StreamingStyledObject {
  #sheet;
  #values;
  #cells;
  #height;
  #hidden = false;
  #outline = 0;
  #hasObjectValues = false;

  constructor(sheet, number) {
    super();
    this.#sheet = sheet;
    this.number = number;
  }

  get worksheet() { return this.#sheet; }

  destroy() {
    this.style = undefined;
    this.#sheet = undefined;
    this.#values = undefined;
    this.#cells = undefined;
  }

  get style() { return this.#sheet ? super.style : undefined; }
  set style(value) { super.style = value; }

  get height() { return this.#height; }
  set height(value) { this.#height = checkedStreamingRowHeight(value); }

  get hidden() { return this.#hidden; }
  set hidden(value) {
    if (typeof value !== 'boolean') throw failure('INVALID_VALUE', 'Expected a boolean row visibility flag');
    this.#hidden = value;
  }

  get outlineLevel() { return this.#outline; }
  set outlineLevel(value) { this.#outline = checkedOutline(value); }

  get collapsed() { return this.#outline > 0 && this.#outline >= this.#sheet.outlineLevelRow; }

  getCell(column) {
    if (!this.#sheet) throw new TypeError('Row has been destroyed');
    if (typeof column === 'string') {
      column = this.#sheet.getColumn(column).number;
    }
    if (!isWorksheetGridColumn(column)) throw failure('INVALID_ADDRESS', 'Invalid XLSX column');
    this.#cells ??= new Map();
    this.#values ??= new Map();
    if (!this.#cells.has(column)) {
      const merge = this.#sheet.mergeAt(this.number, column);
      const cell = merge?.s.r === this.number - 1 && merge.s.c === column - 1
        ? merge.master
        : new StreamingCell(this.#sheet, this.number, column, this.#values.get(column));
      this.#cells.set(column, cell);
      this.#values.set(column, null);
    }
    return this.#cells.get(column);
  }

  getCellEx(location) {
    return this.getCell(rowCellColumn(this.number, location));
  }

  findCell(column) {
    return this.#values?.has(column) || this.#sheet.mergeAt(this.number, column)
      ? this.getCell(column) : undefined;
  }

  get cellCount() {
    let last = this.#sheet.lastMergedColumn(this.number);
    for (const column of this.#values?.keys() ?? []) last = Math.max(last, column);
    return last;
  }

  eachCell(options, iteratee) {
    if (typeof options === 'function') [options, iteratee] = [undefined, options];
    if (worksheetIterationSettings(options).includeEmpty) {
      const count = this.cellCount;
      for (let column = 1; column <= count; column += 1) iteratee(this.getCell(column), column);
      return;
    }
    for (const column of [...this.#candidateColumns()].sort((left, right) => left - right)) {
      if (this.#isPopulated(column)) iteratee(this.getCell(column), column);
    }
  }

  #candidateColumns() {
    const columns = new Set(this.#values?.keys());
    for (const { s, e } of this.#sheet.rowMerges(this.number)) {
      for (let column = s.c + 1; column <= e.c + 1; column += 1) columns.add(column);
    }
    return columns;
  }

  #isPopulated(column) {
    const cell = this.#cells?.get(column);
    return this.#sheet.isMergeMember(this.number, column) ||
      (cell ? cell.value : this.#values?.get(column)) != null;
  }

  get actualCellCount() {
    let count = 0;
    for (const column of this.#candidateColumns()) {
      if (this.#isPopulated(column)) count += 1;
    }
    return count;
  }

  get dimensions() {
    let min = Infinity;
    let max = 0;
    for (const column of this.#candidateColumns()) {
      if (this.#isPopulated(column)) {
        min = Math.min(min, column);
        max = Math.max(max, column);
      }
    }
    return max ? { min, max } : null;
  }

  get hasValues() {
    if (!this.#sheet) return false;
    for (const { s, e } of this.#sheet.rowMerges(this.number)) {
      if (this.number > s.r + 1 || e.c > s.c) return true;
    }
    for (const [column, input] of this.#values ?? []) {
      const cell = this.#cells?.get(column);
      if ((cell ? cell.value : input) != null) return true;
    }
    return false;
  }

  get model() {
    const columns = [...this.#candidateColumns()].sort((a, b) => a - b);
    if (!this.height && !columns.length) return null;
    return {
      cells: columns.map(column => this.getCell(column).model),
      number: this.number,
      min: columns[0] ?? 0,
      max: columns.at(-1) ?? 0,
      height: this.height,
      style: this.style,
      hidden: this.hidden,
      outlineLevel: this.outlineLevel,
      collapsed: this.collapsed,
    };
  }

  set model(input) {
    if (this.#sheet.rowCommitted(this.number)) throw new Error('Out of bounds: this row has been committed');
    const { number, cells, height, style, hidden, outlineLevel } = rowModelSettings(input);
    if (number !== this.number) throw failure('INVALID_VALUE', 'Invalid row number in model');
    if (this.#sheet.lastMergedColumn(number)) {
      throw failure('UNSUPPORTED_OPTIONS', 'Cannot replace a streaming row model across merged cells');
    }
    const nextHeight = checkedStreamingRowHeight(height);
    const nextOutline = checkedOutline(outlineLevel ?? 0);
    this.#sheet[validateStreamingRowStyle](style);
    const updates = new Map();
    for (const { model, column } of rowModelCells(number, cells, worksheetCellAddress)) {
      updates.set(column, this.#sheet[prepareStreamingCellModel](model));
    }

    const notes = new Map(Array.from(this.#sheet.noteColumns(number) ?? [], column => [column, undefined]));
    for (const [column, update] of updates) {
      if (update.note !== undefined) notes.set(column, update.note);
    }
    this.#sheet.writeNotes(number, [...notes]);
    this.#values = new Map();
    this.#cells = undefined;
    for (const [column, update] of updates) {
      const cell = this.getCell(column);
      cell.value = update.value;
      cell.style = update.style;
    }
    this.#height = nextHeight;
    this.#hidden = hidden ?? false;
    this.#outline = nextOutline;
    this.style = style ?? undefined;
  }

  get values() {
    if (!this.#sheet) throw new TypeError('Row has been destroyed');
    const values = [];
    for (const [column, input] of this.#values ?? []) {
      const cell = this.#cells?.get(column) ?? this.#sheet.mergeAt(this.number, column)?.master;
      const value = cell ? cell.value : input;
      if (value != null) values[column] = value;
    }
    return values;
  }

  set values(values) {
    if (!values || typeof values !== 'object') {
      throw failure('INVALID_VALUE', 'Expected row values as an array or a keyed object');
    }
    const assignments = new Map();
    let hasObjectValues = false;
    const hasMerges = this.#sheet.hasMerges;
    const assign = (column, value) => {
      if (value == null) return;
      if (!isWorksheetGridColumn(column)) throw failure('INVALID_ADDRESS', 'Invalid XLSX column');
      const merge = hasMerges ? this.#sheet.mergeAt(this.number, column) : undefined;
      if (merge && this.#sheet.rowCommitted(merge.s.r + 1)) {
        throw new Error('Out of bounds: this row has been committed');
      }
      assignments.set(column, value);
      hasObjectValues ||= typeof value === 'object';
    };
    if (Array.isArray(values)) {
      const offset = Object.hasOwn(values, 0) ? 1 : 0;
      values.forEach((value, index) => assign(index + offset, value));
    } else {
      this.#sheet.eachColumnKey((column, key) => assign(column.number, data(values, key)));
    }
    if (this.#sheet.rowCommitted(this.number)) throw new Error('Out of bounds: this row has been committed');
    this.#hasObjectValues = hasObjectValues;
    this.#values = assignments;
    this.#cells = undefined;
    if (hasMerges) {
      for (const cell of this.#sheet.mergeMasters(this.number)) {
        this.#cells ??= new Map();
        this.#cells.set(cell.column, cell.master);
        cell.master.value = null;
        if (!assignments.has(cell.column)) assignments.set(cell.column, null);
      }
      for (const [column, value] of assignments) {
        if (value !== null && this.#sheet.mergeAt(this.number, column)) this.getCell(column).value = value;
      }
    }
  }

  markObjectValue() { this.#hasObjectValues = true; }

  *formulaRanges() {
    if (!this.#hasObjectValues) return;
    for (const [column, input] of this.#values ?? []) {
      const cell = this.#cells?.get(column);
      const value = cell ? cell.value : input;
      if (!value || typeof value !== 'object') continue;
      const shareType = data(value, 'shareType');
      if (data(value, 'formula') === undefined || (shareType !== 'shared' && shareType !== 'array')) continue;
      const { coordinates } = streamingFormulaRange(value, shareType, this.number, column);
      yield { ...coordinates, shareType };
    }
  }

  splice(start, count, ...inserts) {
    if (!isWorksheetGridColumn(start) || !isSpliceCount(count) ||
        !isWorksheetGridColumn(Math.max(start, start + Math.max(count, inserts.length) - 1))) {
      throw failure('INVALID_ADDRESS', 'Invalid XLSX row splice');
    }
    if (!count && !inserts.length) return;
    if (this.#sheet.rowCommitted(this.number)) throw new Error('Out of bounds: this row has been committed');
    const delta = inserts.length - count;
    const noteColumns = this.#sheet.noteColumns(this.number);
    let end = this.cellCount;
    for (const column of noteColumns ?? []) end = Math.max(end, column);
    if (!isWorksheetGridColumn(Math.max(start, end + delta))) {
      throw failure('INVALID_ADDRESS', 'Invalid XLSX row splice');
    }
    for (const { e } of this.#sheet.rowMerges(this.number)) {
      if (e.c + 1 >= start) throw failure('UNSUPPORTED_OPTIONS', 'Cannot splice a streaming row across merged cells');
    }
    this.#sheet.checkEarlierFormulaSplice(this.number, start, delta ? Infinity : start + count - 1);
    const movedRanges = new Map();
    for (const range of this.formulaRanges()) {
      const column = range.s.c + 1;
      const lastColumn = range.e.c + 1;
      const singleRow = range.e.r === range.s.r;
      if (lastColumn < start || (!delta && column >= start + count)) continue;
      if (singleRow && column >= start && lastColumn < start + count) continue;
      if (column < start + count || !singleRow) {
        throw failure('UNSUPPORTED_OPTIONS', 'Cannot splice through a streaming formula range');
      }
      const anchor = gridAddress(this.number, column + delta);
      const last = gridAddress(this.number, lastColumn + delta);
      movedRanges.set(column, { anchor, ref: encode_range(anchor, last), lastColumn, shareType: range.shareType });
    }
    const updates = new Map();
    if (delta) {
      const occupied = noteColumns
        ? new Set([...(this.#values?.keys() ?? []), ...noteColumns])
        : this.#values;
      for (const column of occupied?.keys() ?? []) {
        if (column < start) continue;
        updates.set(column, undefined);
      }
      for (const column of occupied?.keys() ?? []) {
        if (column < start + count) continue;
        const cell = this.#cells?.get(column);
        let value = cell ? cell.value : this.#values?.get(column);
        if (value && typeof value === 'object') {
          const moved = movedRanges.get(column);
          const sharedFormula = data(value, 'sharedFormula');
          if (moved) {
            const result = readCellFormulaResult(value, 'range');
            value = { formula: data(value, 'formula'), shareType: data(value, 'shareType'), ref: moved.ref,
              ...(Object.hasOwn(value, 'result') ? { result } : {}) };
          } else if (sharedFormula !== undefined) {
            const result = readSharedFormulaResult(value, 'member');
            const [, masterRow, masterColumn] = worksheetCellAddress(sharedFormula);
            const master = masterRow === this.number - 1 ? movedRanges.get(masterColumn + 1) : undefined;
            if (!master || master.shareType !== 'shared' || column <= masterColumn + 1 || column > master.lastColumn) {
              throw failure('UNSUPPORTED_OPTIONS', 'Cannot splice a shared formula member outside its moved master range');
            }
            value = { sharedFormula: master.anchor, ...(Object.hasOwn(value, 'result') ? { result } : {}) };
          }
        }
        updates.set(column + delta, {
          value,
          style: cell?.style,
          note: noteColumns?.has(column) ? this.#sheet.readNote(gridAddress(this.number, column)) : undefined,
        });
      }
    }
    inserts.forEach((value, index) => updates.set(start + index, { value }));
    if (noteColumns) {
      const notes = [];
      for (const [column, update] of updates) {
        if (noteColumns.has(column) || update?.note !== undefined) notes.push([column, update?.note]);
      }
      this.#sheet.writeNotes(this.number, notes);
    }
    for (const [column, update] of updates) {
      const cell = this.getCell(column);
      cell.value = update?.value ?? null;
      cell.style = update?.style ?? {};
    }
  }

  addPageBreak(left, right) {
    this.#sheet.addPageBreak(this.number, left, right);
  }

  commit() {
    this.#sheet.commitThrough(this.number);
  }

  encode() {
    const columns = [];
    const values = [];
    const typed = [];
    let arrayRanges;
    let sharedRanges;
    let links;
    const notes = this.#sheet.noteColumns(this.number);
    const styles = this.#sheet.stylesEnabled || notes ? [] : undefined;
    let hasStyles = false;
    let hasDates = false;
    const candidates = notes ? new Set([...this.#values?.keys() ?? [], ...notes]) : this.#values?.keys();
    for (const column of [...candidates ?? []].sort((left, right) => left - right)) {
      const cell = this.#cells?.get(column);
      const merged = this.#sheet.isMergeMember(this.number, column);
      const hasNote = notes?.has(column);
      if (merged && !hasNote) continue;
      const directMerge = cell?.[directStreamingMerge];
      const value = merged || directMerge ? null : cell ? cell.value : this.#values?.get(column);
      let style = merged ? 0 : this.#sheet.cellStyleIndex(column, this.styleForEncoding, cell?.styleForEncoding, value);
      if (value == null && hasNote && !style) style = this.#sheet.blankNoteStyle();
      if (value == null && !style && !hasNote && !directMerge) continue;
      styles?.push(style);
      hasStyles ||= style !== 0;
      columns.push(column - 1);
      if (value == null) {
        if (directMerge && !style) typed.push({ index: values.length, kind: 4 });
        values.push(null);
      } else if (typeof value === 'object') {
        if (Object.hasOwn(value, 'hyperlink')) {
          const { text, hyperlink, tooltip } = cellHyperlinkSettings(value);
          (links ??= []).push({ address: encode_col(column - 1) + this.number, hyperlink, tooltip });
          values.push(text);
          continue;
        }
        const formula = data(value, 'formula');
        if (formula !== undefined && typeof formula !== 'string') {
          throw failure('INVALID_VALUE', 'Expected a formula string');
        }
        const shareType = formula === undefined ? undefined : data(value, 'shareType');
        const rangeFormula = shareType === 'array' || shareType === 'shared';
        const sharedFormula = data(value, 'sharedFormula');
        let encodedFormula = formula === undefined ? undefined : scalar(formula, false);
        let cell;
        if (Object.hasOwn(value, 'richText')) {
          cell = { value: encodeRichTextRuns(cellRichTextSettings(value).richText), kind: 3 };
        } else if (sharedFormula !== undefined) {
          const result = readSharedFormulaResult(value, 'member');
          const [anchor] = worksheetCellAddress(sharedFormula);
          const master = this.#sheet.sharedFormula(anchor, sharedRanges);
          if (!master || this.number - 1 < master.s.r || this.number - 1 > master.e.r ||
              column - 1 < master.s.c || column - 1 > master.e.c) {
            throw failure('INVALID_VALUE', 'Shared formula master must precede this member and contain its cell');
          }
          cell = typedValue(result);
          encodedFormula = [master.index];
        } else {
          cell = formula === undefined ? typedValue(value)
            : typedValue(readCellFormulaResult(value, rangeFormula ? 'range' : 'ordinary'));
          if (rangeFormula) {
            const { reference, coordinates } = streamingFormulaRange(value, shareType, this.number, column);
            encodedFormula = [encodedFormula, reference];
            if (shareType === 'array') {
              (arrayRanges ??= []).push(coordinates);
            } else {
              sharedRanges ??= [];
              const index = this.#sheet.nextSharedIndex + sharedRanges.length;
              sharedRanges.push({ ...coordinates, anchor: encode_col(column - 1) + this.number, index, formula });
              encodedFormula.push(index);
            }
          }
        }
        typed.push({ index: values.length, kind: cell.kind, formula: encodedFormula });
        values.push(cell.value);
        if (cell.kind === 2) {
          if (styles && !this.#sheet.stylesEnabled) styles[styles.length - 1] = dateStyle;
          hasStyles ||= styles !== undefined;
          hasDates = true;
        }
      } else {
        values.push(scalar(value, true));
      }
    }
    const coordinates = Uint32Array.from(columns);
    const rowStyle = this.#sheet.styleIndex(this.styleForEncoding);
    let layout;
    if (this.#height !== undefined || this.#hidden || this.#outline !== 0 || rowStyle) {
      const fields = [this.#height ?? NaN, Number(this.#hidden), this.#outline, Number(this.collapsed)];
      if (rowStyle) fields.push(rowStyle);
      layout = Float64Array.from(fields);
    }
    const span = { left: columns[0], right: columns.at(-1) };
    if (!columns.length && !layout) return { xml: '', hasDates, ...span };
    const strings = this.#sheet.stringTable;
    if (!typed.length && !hasStyles) {
      const xml = strings
        ? strings.encode_row(this.number - 1, coordinates, values, layout)
        : encode_xlsx_row(this.number - 1, coordinates, values, layout);
      return { xml, hasDates, links, ...span };
    }
    const kinds = new Uint8Array(values.length);
    const formulas = Array(values.length).fill(undefined);
    for (const { index, kind, formula } of typed) {
      kinds[index] = kind;
      formulas[index] = formula;
    }
    this.#sheet.validateFormulaRanges(arrayRanges, sharedRanges, this.number - 1, coordinates, typed);
    const styleIndexes = hasStyles ? Uint32Array.from(styles) : undefined;
    const date1904 = hasDates && this.#sheet.workbook.properties.date1904;
    const xml = strings
      ? strings.encode_typed_row(this.number - 1, coordinates, values, kinds, formulas, dateStyle, layout, styleIndexes, date1904 || undefined)
      : encode_xlsx_typed_row(this.number - 1, coordinates, values, kinds, formulas, dateStyle, layout, styleIndexes, date1904 || undefined);
    return { xml, hasDates, arrayRanges, sharedRanges, links, ...span };
  }
}

class StreamingColumn extends StreamingStyledObject {
  #column;
  #sheet;

  constructor(sheet, column) {
    super();
    this.#sheet = sheet;
    this.#column = column;
  }

  get worksheet() { return this.#sheet; }
  get collapsed() { return this.#column.collapsed; }
  get number() { return this.#column.number; }
  get letter() { return this.#column.letter; }
  get header() { return this.#column.header; }
  get headers() { return this.#column.headers; }
  get headerCount() { return this.#column.headerCount; }
  get isCustomWidth() { return this.#column.isCustomWidth; }

  get defn() { return { ...this.#column.defn, style: this.style }; }

  set defn(value) {
    if (this.#sheet.committed || (value != null && data(value, 'header') !== undefined && this.#sheet.rowCommitted(1))) {
      throw new Error('Out of bounds: the header row has been committed');
    }
    this.#column.defn = value;
    this.style = value == null ? undefined : data(value, 'style');
    this.#sheet.writeColumnHeaders(this.number, this.#column.header);
  }

  get isDefault() {
    const { font, numFmt, alignment, border, fill, protection } = this.style;
    return !this.isCustomWidth && !this.hidden && !this.outlineLevel &&
      !(font || numFmt || alignment || border || fill || protection);
  }

  equivalentTo(other) {
    return this.width === other.width && this.hidden === other.hidden &&
      this.outlineLevel === other.outlineLevel && isDeepStrictEqual(this.style, other.style);
  }

  toString() { return this.#column.toString(); }

  set header(value) {
    if (this.#sheet.committed || (value !== undefined && this.#sheet.rowCommitted(1))) {
      throw new Error('Out of bounds: the header row has been committed');
    }
    this.#column.header = value;
    this.#sheet.writeColumnHeaders(this.number, this.#column.header);
  }
  get key() { return this.#column.key; }
  set key(value) { this.#column.key = value; }
  get width() { return this.#column.width; }
  set width(value) { this.#column.width = value; }
  get hidden() { return this.#column.hidden; }
  set hidden(value) { this.#column.hidden = value; }
  get outlineLevel() { return this.#column.outlineLevel; }
  set outlineLevel(value) { this.#column.outlineLevel = value; }

  get values() {
    const values = [];
    this.eachCell((cell, number) => {
      if (cell.type !== ValueType.Null) values[number] = cell.value;
    });
    return values;
  }

  set values(input) {
    if (input == null) return;
    if (!Array.isArray(input)) throw failure('INVALID_VALUE', 'Expected an array of column values');
    const offset = Object.hasOwn(input, 0) ? 1 : 0;
    const assignments = [];
    Array.prototype.forEach.call(input, (value, index) => assignments.push([index + offset, value]));
    for (const [row] of assignments) {
      if (!isWorksheetGridRow(row)) throw failure('INVALID_ADDRESS', 'Invalid XLSX row');
      const target = this.#sheet.mergeAt(row, this.number)?.s.r;
      if (this.#sheet.rowCommitted(row) || (target !== undefined && this.#sheet.rowCommitted(target + 1))) {
        throw new Error('Out of bounds: this row has been committed');
      }
    }
    for (const [row, value] of assignments) this.#sheet.getRow(row).getCell(this.number).value = value;
  }

  eachCell(options, iteratee) {
    if (typeof options === 'function') [options, iteratee] = [undefined, options];
    this.#sheet.eachRow(options, (row, number) => iteratee(row.getCell(this.number), number));
  }
}

class StreamingWorksheet {
  #rows = new Map();
  #lastRow = 0;
  #rowZero = 1;
  #committed = false;
  #startedData = false;
  #hasComments = false;
  #append;
  #bounds;
  #sheet;
  #workbook;
  #columnHandles = new WeakMap();
  #merges = [];
  #arrays = [];
  #shared = [];
  #nextSharedIndex = 0;
  #links = [];
  #noteColumns = new Map();
  #getStrings;
  #styleIndex;
  #blankNoteStyle;
  #useSharedStrings;

  constructor(workbook, sheet, append, getStrings, useSharedStrings, styleIndex, blankNoteStyle) {
    this.#workbook = workbook;
    this.#sheet = sheet;
    this.id = sheet.id;
    this.name = sheet.name;
    this.#append = append;
    this.#getStrings = getStrings;
    this.#styleIndex = styleIndex;
    this.#blankNoteStyle = blankNoteStyle;
    this.useSharedStrings = useSharedStrings;
  }

  get dataValidations() { return this.#sheet.dataValidations; }

  writeCellValidation(address, value) { this.#sheet.getCell(address).dataValidation = value; }

  readNote(address) { return this.#sheet.findCell(address)?.note; }

  [validateStreamingRowStyle](style) {
    this.#sheet[validateStreamingRowStyle](style);
  }

  [prepareStreamingCellModel](model) {
    return this.#sheet[prepareStreamingCellModel](model);
  }

  readNoteModel(address, row, column) {
    return this.#noteColumns.get(row)?.has(column) ? this.#sheet.getCell(address).model.comment?.note : undefined;
  }

  writeNote(row, column, address, value) {
    const cell = this.#sheet.getCell(address);
    cell.note = value;
    const columns = this.#noteColumns.get(row);
    if (cell.note === undefined) {
      columns?.delete(column);
      if (columns?.size === 0) this.#noteColumns.delete(row);
    } else if (columns) {
      columns.add(column);
    } else {
      this.#noteColumns.set(row, new Set([column]));
    }
  }

  writeNotes(row, updates) {
    if (!updates.length) return;
    this.#sheet[applyStreamingNotes](row, updates);
    const columns = this.#noteColumns.get(row) ?? new Set();
    for (const [column, value] of updates) {
      if (value === undefined) columns.delete(column);
      else {
        this.#sheet.getCell(row, column);
        columns.add(column);
      }
    }
    if (columns.size) this.#noteColumns.set(row, columns);
    else this.#noteColumns.delete(row);
  }

  noteColumns(row) { return this.#noteColumns.get(row); }

  blankNoteStyle() { return this.#blankNoteStyle(); }

  get workbook() { return this.#workbook; }

  get committed() { return this.#committed; }

  get startedData() { return this.#startedData; }

  get hasComments() { return this.#hasComments; }

  destroy() { throw new Error('Invalid Operation: destroy'); }

  addBackgroundImage(imageId) {
    if (this.#committed) throw new Error('Worksheet is already committed');
    this.#sheet.addBackgroundImage(imageId);
  }

  getBackgroundImageId() { return this.#sheet.getBackgroundImageId(); }

  get rowBreaks() { return this.#sheet.rowBreaks; }

  addPageBreak(row, left, right) {
    if (this.#committed) throw new Error('Worksheet is already committed');
    this.#sheet.getRow(row).addPageBreak(left, right);
  }

  get properties() { return this.#sheet.properties; }

  get state() { return this.#sheet.state; }
  set state(value) { this.#sheet.state = value; }

  get views() { return this.#sheet.views; }

  set views(value) {
    if (this.#committed) throw new Error('Worksheet is already committed');
    this.#sheet.views = value;
  }

  get pageSetup() { return this.#sheet.pageSetup; }
  set pageSetup(value) { this.#sheet.pageSetup = value; }

  get headerFooter() { return this.#sheet.headerFooter; }
  set headerFooter(value) { this.#sheet.headerFooter = value; }

  get autoFilter() { return this.#sheet.autoFilter; }

  set autoFilter(value) {
    if (this.#committed) throw new Error('Worksheet is already committed');
    this.#sheet.autoFilter = value;
  }

  get conditionalFormatting() { return this.#sheet.conditionalFormattings; }

  addConditionalFormatting(options) {
    if (this.#committed) throw new Error('Worksheet is already committed');
    this.#sheet.addConditionalFormatting(options);
  }

  removeConditionalFormatting(filter) {
    if (this.#committed) throw new Error('Worksheet is already committed');
    this.#sheet.removeConditionalFormatting(filter);
  }

  get sheetProtection() { return this.#sheet.sheetProtection; }

  async protect(password, options) {
    if (this.#committed) throw new Error('Worksheet is already committed');
    await this.#sheet.protect(password, options);
  }

  unprotect() {
    if (this.#committed) throw new Error('Worksheet is already committed');
    this.#sheet.unprotect();
  }

  get useSharedStrings() { return this.#useSharedStrings; }

  set useSharedStrings(value) { this.#useSharedStrings = checkedStreamingSharedStrings(value) ?? false; }

  get stylesEnabled() { return this.#styleIndex !== undefined; }

  styleIndex(style, value) { return this.#styleIndex?.(style, value) ?? 0; }

  cellStyleIndex(column, rowStyle, cellStyle, value) {
    if (!this.stylesEnabled) return 0;
    const source = this.#sheet.columns?.[column - 1];
    const columnStyle = source && this.#columnHandles.get(source)?.styleForEncoding;
    let style = [cellStyle, rowStyle, columnStyle].find(value => value !== undefined);
    if (Number(columnStyle !== undefined) + Number(rowStyle !== undefined) + Number(cellStyle !== undefined) > 1) {
      style = {};
      for (const layer of [columnStyle, rowStyle, cellStyle]) {
        if (layer === undefined) continue;
        for (const [key, setting] of Object.entries(defaultCellStyleSettings(layer))) {
          if (setting !== undefined) style[key] = setting;
        }
      }
    }
    return this.styleIndex(style, value);
  }

  get stringTable() { return this.#useSharedStrings ? this.#getStrings() : undefined; }

  get outlineLevelRow() { return this.#sheet.properties.outlineLevelRow; }

  get lastRow() { return this.#rows.get(this.#lastRow); }

  get columns() { return this.#sheet.columns?.map(column => this.#columnHandle(column)) ?? null; }

  set columns(definitions) {
    if (this.#committed) throw new Error('Worksheet is already committed');
    if (Array.isArray(definitions)) {
      for (const definition of definitions) {
        if (!definition || typeof definition !== 'object') continue;
        if (this.#rowZero > 1 && data(definition, 'header') !== undefined) {
          throw new Error('Out of bounds: the header row has been committed');
        }
      }
    }
    const styles = [];
    this.#sheet.columns = Array.isArray(definitions) ? definitions.map(definition => {
      if (!definition || typeof definition !== 'object' || !Object.hasOwn(definition, 'style')) {
        styles.push(undefined);
        return definition;
      }
      const { style, ...rest } = Object.getOwnPropertyDescriptors(definition);
      if (!Object.hasOwn(style, 'value')) throw failure('UNSUPPORTED_OPTIONS', 'Expected a data property for column style');
      styles.push(style.value);
      return Object.defineProperties({}, rest);
    }) : definitions;
    for (const column of this.#sheet.columns) {
      if (this.stylesEnabled) this.#columnHandle(column).style = styles[column.number - 1];
      this.writeColumnHeaders(column.number, column.header);
    }
  }

  writeColumnHeaders(column, header) {
    if (header === undefined) return;
    const labels = Array.isArray(header) ? header : [header];
    labels.forEach((label, index) => { this.getRow(index + 1).getCell(column).value = label; });
  }

  #columnHandle(column) {
    if (!this.#columnHandles.has(column)) this.#columnHandles.set(column, new StreamingColumn(this, column));
    return this.#columnHandles.get(column);
  }

  getColumn(column) { return this.#columnHandle(this.#sheet.getColumn(column)); }

  getColumnKey(key) {
    const column = this.#sheet.getColumnKey(key);
    return column && this.#columnHandle(column);
  }

  setColumnKey(key, column) {
    const source = column?.worksheet === this ? this.#sheet.columns?.[column.number - 1] : undefined;
    if (!source || this.#columnHandles.get(source) !== column) {
      throw failure('INVALID_VALUE', 'Expected a column from this streaming worksheet');
    }
    this.#sheet.setColumnKey(key, source);
  }

  deleteColumnKey(key) { this.#sheet.deleteColumnKey(key); }

  eachColumnKey(callback) {
    this.#sheet.eachColumnKey((column, key) => callback(this.#columnHandle(column), key));
  }

  rowCommitted(row) { return this.#committed || row < this.#rowZero; }

  get hasMerges() { return this.#merges.length !== 0; }

  isMergeMember(row, column) {
    if (!this.hasMerges) return false;
    const merge = this.mergeAt(row, column);
    return merge !== undefined && (row !== merge.s.r + 1 || column !== merge.s.c + 1);
  }

  mergeAt(row, column) {
    return this.#merges.find(({ s, e }) => row > s.r && row <= e.r + 1 && column > s.c && column <= e.c + 1);
  }

  *rowMerges(row) {
    for (const merge of this.#merges) {
      if (row > merge.s.r && row <= merge.e.r + 1) yield merge;
    }
  }

  lastMergedColumn(row) {
    let last = 0;
    for (const { e } of this.rowMerges(row)) last = Math.max(last, e.c + 1);
    return last;
  }

  *mergeMasters(row) {
    for (const merge of this.#merges) {
      if (merge.s.r + 1 === row) yield { column: merge.s.c + 1, master: merge.master };
    }
  }

  get nextSharedIndex() { return this.#nextSharedIndex; }

  sharedFormula(anchor, pending = []) {
    return pending.find(master => master.anchor === anchor) ?? this.#shared.find(master => master.anchor === anchor);
  }

  resolveSharedFormula(reference, member) {
    if (reference === undefined) return undefined;
    const [anchor] = worksheetCellAddress(reference);
    const formula = this.sharedFormula(anchor)?.formula ?? this.findCell(anchor)?.value?.formula;
    if (typeof formula !== 'string') return undefined;
    return this.#sheet[translateStreamingFormula](formula, anchor, member);
  }

  markObjectValue(row) { this.#rows.get(row)?.markObjectValue(); }

  checkEarlierFormulaSplice(row, firstColumn, lastColumn) {
    const intersects = ({ s, e }) => s.r <= row - 1 && row - 1 <= e.r &&
      s.c < lastColumn && e.c >= firstColumn - 1;
    if (this.#arrays.some(intersects) || this.#shared.some(intersects)) {
      throw failure('UNSUPPORTED_OPTIONS', 'Cannot splice through an earlier streaming formula range');
    }
    for (const [number, pending] of this.#rows) {
      if (number >= row) continue;
      for (const range of pending.formulaRanges()) {
        if (intersects(range)) {
          throw failure('UNSUPPORTED_OPTIONS', 'Cannot splice through an earlier streaming formula range');
        }
      }
    }
  }

  validateFormulaRanges(ranges = [], shared = [], row, columns, cells) {
    for (let index = 0; index < ranges.length; index++) {
      const range = ranges[index];
      if (this.#merges.some(merge => rangesOverlap(merge, range)) ||
          this.#arrays.some(array => rangesOverlap(array, range)) ||
          this.#shared.some(master => rangesOverlap(master, range)) ||
          shared.some(master => rangesOverlap(master, range)) ||
          ranges.slice(0, index).some(array => rangesOverlap(array, range))) {
        throw failure('INVALID_VALUE', 'Array formulas must not overlap merges or other array formulas');
      }
    }
    for (let index = 0; index < shared.length; index++) {
      const range = shared[index];
      if (this.#merges.some(merge => rangesOverlap(merge, range)) ||
          this.#arrays.some(array => rangesOverlap(array, range)) ||
          this.#shared.some(master => rangesOverlap(master, range)) ||
          shared.slice(0, index).some(master => rangesOverlap(master, range))) {
        throw failure('INVALID_VALUE', 'Shared formulas must not overlap merges or other formula ranges');
      }
    }
    if (ranges.length || this.#arrays.length || shared.length || this.#shared.length) {
      for (const { index, formula } of cells) {
        if (formula === undefined) continue;
        const column = columns[index];
        const isMember = ({ s, e }) => row >= s.r && row <= e.r && column >= s.c && column <= e.c &&
          (row !== s.r || column !== s.c);
        if (ranges.some(isMember) || this.#arrays.some(isMember)) {
          throw failure('INVALID_VALUE', 'Array formula members cannot contain formulas');
        }
        const master = shared.find(isMember) ?? this.#shared.find(isMember);
        if (master && (!Array.isArray(formula) || formula.length !== 1 || formula[0] !== master.index)) {
          throw failure('INVALID_VALUE', 'Shared formula range requires members of that shared formula');
        }
      }
    }
  }

  mergeCells(...cells) {
    const reference = mergeReference(cells);
    const { s, e } = decode_range(reference);
    if (this.rowCommitted(s.r + 1)) throw new Error('Out of bounds: this row has been committed');
    if (this.#arrays.some(array => rangesOverlap(array, { s, e }))) {
      throw failure('INVALID_VALUE', 'Merges must not overlap an array formula');
    }
    if (this.#shared.some(master => rangesOverlap(master, { s, e }))) {
      throw failure('INVALID_VALUE', 'Merges must not overlap a shared formula');
    }
    this.#sheet.mergeCellsWithoutStyle(reference);
    const master = this.getRow(s.r + 1).getCell(s.c + 1);
    this.#merges.push({ s, e, master });
    this.getRow(e.r + 1);
  }

  #expandBounds(top, left, bottom, right) {
    const bounds = this.#bounds ??= { top, left, bottom, right };
    bounds.top = Math.min(bounds.top, top);
    bounds.left = Math.min(bounds.left, left);
    bounds.bottom = Math.max(bounds.bottom, bottom);
    bounds.right = Math.max(bounds.right, right);
  }

  get dimensions() {
    if (!this.#bounds) return worksheetDimensions(1, 1, 1, 1, true);
    const { top, left, bottom, right } = this.#bounds;
    return worksheetDimensions(top, left + 1, bottom, right + 1);
  }

  get dimension() { return this.#bounds && this.dimensions.shortRange; }

  getRow(number) {
    if (!isWorksheetGridRow(number)) throw failure('INVALID_ADDRESS', 'Invalid XLSX row');
    if (this.#committed || number < this.#rowZero) throw new Error('Out of bounds: this row has been committed');
    if (!this.#rows.has(number)) this.#rows.set(number, new StreamingRow(this, number));
    this.#lastRow = Math.max(number, this.#lastRow);
    return this.#rows.get(number);
  }

  findRow(number) {
    return this.#rows.get(number);
  }

  findCell(reference, index) {
    const [, row, column] = worksheetCellAddress(reference, index);
    const number = row + 1;
    if (this.rowCommitted(number)) return undefined;
    const handle = this.findRow(number) ?? (this.mergeAt(number, column + 1) && this.getRow(number));
    return handle?.findCell(column + 1);
  }

  eachRow(options, iteratee) {
    if (typeof options === 'function') [options, iteratee] = [undefined, options];
    if (this.#committed) return;
    if (worksheetIterationSettings(options).includeEmpty) {
      const last = this.#lastRow;
      for (let number = this.#rowZero; number <= last && !this.#committed; number += 1) {
        if (!this.rowCommitted(number)) iteratee(this.getRow(number), number);
      }
      return;
    }
    const numbers = [...this.#rows.keys()].sort((left, right) => left - right);
    for (const number of numbers) {
      const row = this.findRow(number);
      if (row?.hasValues) iteratee(row, number);
    }
  }

  getCell(reference, index) {
    const [, row, column] = worksheetCellAddress(reference, index);
    return this.getRow(row + 1).getCell(column + 1);
  }

  addRow(values) {
    const number = this.#lastRow + 1;
    const row = this.getRow(number);
    try {
      row.values = values;
    } catch (error) {
      this.#rows.delete(number);
      this.#lastRow = number - 1;
      throw error;
    }
    return row;
  }

  commitThrough(number) {
    if (number < this.#rowZero || this.#committed) return;
    const numbers = [...this.#rows.keys()].filter(row => row <= number).sort((left, right) => left - right);
    for (const row of numbers) {
      const handle = this.#rows.get(row);
      if (!handle.worksheet) {
        if (handle.height) throw new TypeError('Row has been destroyed');
        const notes = this.#noteColumns.get(row);
        if (notes) this.writeNotes(row, Array.from(notes, column => [column, undefined]));
      }
      const { xml, hasDates, arrayRanges, sharedRanges, links, left, right } = handle.worksheet ? handle.encode() : {};
      if (xml) {
        this.#append(xml, hasDates);
        if (arrayRanges) {
          this.#arrays.push(...arrayRanges);
          for (const { s, e } of arrayRanges) this.#expandBounds(s.r + 1, s.c, e.r + 1, e.c);
        }
        if (links) this.#links.push(...links);
        if (sharedRanges) {
          this.#shared.push(...sharedRanges);
          this.#nextSharedIndex += sharedRanges.length;
        }
      }
      if (left !== undefined) {
        this.#expandBounds(row, left, row, right);
      }
      this.#startedData = true;
      this.#hasComments ||= this.#noteColumns.has(row);
      this.#rows.delete(row);
      this.#noteColumns.delete(row);
      this.#rowZero = row + 1;
      if (this.#arrays.length) this.#arrays = this.#arrays.filter(array => array.e.r >= row);
      if (this.#shared.length) this.#shared = this.#shared.filter(master => master.e.r >= row);
    }
    this.#rowZero = number + 1;
  }

  writeHyperlinks(workbook) {
    workbook[applyStreamingHyperlinks](this.name, this.#links);
    this.#links = [];
  }

  commit() {
    if (this.#committed) return;
    if (this.stylesEnabled) {
      for (const column of this.#sheet.columns ?? []) {
        const style = this.#columnHandles.get(column)?.styleForEncoding;
        if (style !== undefined) column.style = style;
      }
    }
    this.commitThrough(this.#lastRow);
    for (const { s, e } of this.#merges) this.#expandBounds(s.r + 1, s.c, e.r + 1, e.c);
    this.#committed = true;
  }
}

export class WorkbookWriter {
  views = [];

  #seed;
  #worksheets = [];
  #directory;
  #rowChunk;
  #rowChunkSheet;
  #rowChunkLength = 0;
  #rowOffsets = new Map();
  #spoolError;
  #filename;
  #compression;
  #zipComment;
  #forceZip64;
  #forceLocalTime;
  #commit;
  #stream;
  #usesDates = false;
  #hasDates = false;
  #date1904 = false;
  #properties;
  #strings;
  #useSharedStrings;
  #styleCache;
  #noteStyle;

  constructor(workbook, options = {}) {
    this.#seed = workbook;
    const writer = this;
    this.#properties = {
      get date1904() { return writer.#date1904; },
      set date1904(value) { writer.properties = { date1904: value }; },
    };
    const settings = streamingWorkbookWriterSettings(options);
    if (settings.useStyles) this.#styleCache = new Map();
    this.useSharedStrings = settings.useSharedStrings;
    if (settings.stream !== undefined && !(settings.stream instanceof Writable)) {
      throw failure('INVALID_VALUE', 'Expected a Node writable stream');
    }
    this.#filename = settings.filename;
    this.#stream = settings.stream ?? (settings.filename === undefined ? new BufferedOutput() : undefined);
    const { store, level, zlib, comment, forceZip64, forceLocalTime } = streamingZipSettings(settings.zip ?? {});
    const nestedLevel = streamingZlibSettings(zlib ?? {}).level;
    this.#zipComment = checkedStreamingZipCommentLength(comment);
    this.#forceZip64 = forceZip64;
    this.#forceLocalTime = forceLocalTime;
    this.#compression = [store, level, nestedLevel];
    this.#seed.creator = settings.creator;
    this.#seed.lastModifiedBy = settings.lastModifiedBy;
    this.#seed.created = settings.created ?? new Date();
    this.#seed.modified = settings.modified ?? this.#seed.created;
    if (settings.lastPrinted !== undefined) this.#seed.lastPrinted = settings.lastPrinted;
  }

  get stream() { return this.#stream; }

  get definedNames() { return this.#seed.definedNames; }

  get nextId() { return this.#seed.nextId; }

  get properties() { return this.#properties; }

  set properties(value) {
    const { date1904 } = workbookPropertySettings(value);
    if (date1904 === undefined || date1904 === this.#date1904) return;
    if (this.#commit || this.#hasDates) {
      throw failure('INVALID_VALUE', 'The date system must be set before committing date cells');
    }
    this.#seed.properties = { date1904 };
    this.#date1904 = date1904;
  }

  get calcProperties() { return this.#seed.calcProperties; }

  set calcProperties(value) { this.#seed.calcProperties = value; }

  get creator() { return this.#seed.creator; }
  set creator(value) { this.#seed.creator = value; }

  get lastModifiedBy() { return this.#seed.lastModifiedBy; }
  set lastModifiedBy(value) { this.#seed.lastModifiedBy = value; }

  get created() { return this.#seed.created; }
  set created(value) { this.#seed.created = value; }

  get modified() { return this.#seed.modified; }
  set modified(value) { this.#seed.modified = value; }

  get lastPrinted() { return this.#seed.lastPrinted; }
  set lastPrinted(value) { this.#seed.lastPrinted = value; }

  addImage(image) {
    if (this.#commit) throw new Error('Workbook is already committing');
    return this.#seed.addImage(image);
  }

  get media() { return this.#seed.media; }

  getImage(id) {
    const image = this.#seed.getImage(id);
    return image && { ...image, name: `image${id}.${image.extension}` };
  }

  get useSharedStrings() { return this.#useSharedStrings; }

  set useSharedStrings(value) { this.#useSharedStrings = checkedStreamingSharedStrings(value) ?? false; }

  addWorksheet(name, options) {
    if (this.#commit) throw new Error('Workbook is already committing');
    const settings = worksheetOptions(options, this.useSharedStrings);
    const sheet = this.#seed.addWorksheet(name || `sheet${this.nextId}`, settings.options);
    const streaming = new StreamingWorksheet(this, sheet, (xml, hasDates) => {
      this.#stageRow(sheet.id, xml);
      this.#hasDates ||= hasDates;
      this.#usesDates ||= hasDates && !this.#styleCache;
    }, () => this.#strings ??= new XlsxStreamingStrings(), settings.useSharedStrings,
    this.#styleCache && ((style, value) => {
      const date = value instanceof Date || (value && typeof value === 'object' && data(value, 'result') instanceof Date);
      return style === undefined && !date ? 0 : this.#seed[registerStreamingStyle](style, this.#styleCache, date);
    }), () => this.#registerBlankNoteStyle());
    this.#worksheets.push(streaming);
    return streaming;
  }

  #registerBlankNoteStyle() {
    if (this.#noteStyle !== undefined) return this.#noteStyle;
    const cache = this.#styleCache ?? new Map();
    // Keep the default date style at index 1 for rows written without useStyles.
    if (!this.#styleCache) this.#seed[registerStreamingStyle](undefined, cache, true);
    // ExcelJS 4.4.0 needs a nonzero style index to recognize an otherwise empty cell.
    this.#noteStyle = this.#seed[registerStreamingStyle]({ alignment: { vertical: 'bottom' } }, cache, false);
    return this.#noteStyle;
  }

  getWorksheet(id) {
    if (id === undefined) return this.#worksheets[0];
    return this.#worksheets.find(sheet => sheet.id === id || sheet.name === id);
  }

  commit() {
    return this.#commit ??= Promise.resolve().then(() => this.#finish());
  }

  #rowPath(id) { return join(this.#directory, `sheet${id}.xml`); }

  #stageRow(id, xml) {
    if (this.#spoolError) throw this.#spoolError;
    const length = Buffer.byteLength(xml);
    if (id !== this.#rowChunkSheet || this.#rowChunkLength + length > rowChunkSize) {
      this.#flushRows();
    }
    if (length > rowChunkSize) {
      this.#writeRows(id, xml, length);
      return;
    }
    this.#rowChunk ??= Buffer.allocUnsafe(rowChunkSize);
    this.#rowChunkSheet = id;
    this.#rowChunkLength += this.#rowChunk.write(xml, this.#rowChunkLength, length, 'utf8');
  }

  #flushRows() {
    if (!this.#rowChunkLength) return;
    this.#writeRows(this.#rowChunkSheet, this.#rowChunk.subarray(0, this.#rowChunkLength), this.#rowChunkLength);
    this.#rowChunkLength = 0;
  }

  #writeRows(id, bytes, length) {
    if (this.#spoolError) throw this.#spoolError;
    this.#directory ??= mkdtempSync(join(tmpdir(), 'ooxml-xlsx-writer-'));
    const path = this.#rowPath(id);
    const offset = this.#rowOffsets.get(id) ?? 0;
    try {
      appendFileSync(path, bytes);
    } catch (error) {
      try {
        if (existsSync(path)) truncateSync(path, offset);
      } catch (rollback) {
        this.#spoolError = new AggregateError([error, rollback], 'Could not restore worksheet spool');
        throw this.#spoolError;
      }
      throw error;
    }
    this.#rowOffsets.set(id, offset + length);
  }

  async #finish() {
    try {
      for (const sheet of this.#worksheets) sheet.commit();
      this.#flushRows();
      this.#rowChunk = undefined;
      const stringCount = this.#strings?.size() ?? 0;
      if (this.#usesDates || stringCount) {
        const marker = this.#seed.getWorksheet(this.#worksheets[0].id).getCell('A1');
        if (this.#usesDates) marker.numFmt = 'mm-dd-yy';
        if (stringCount) marker.value = 'shared string table';
      }
      for (const sheet of this.#worksheets) sheet.writeHyperlinks(this.#seed);
      if (!Array.isArray(this.views) || this.views.length) this.#seed.views = this.views;
      const bytes = await this.#seed.xlsx.writeBuffer(stringCount ? { useSharedStrings: true } : undefined);
      this.#stream ??= createWriteStream(this.#filename);
      await pipeline(Readable.from(this.#archive(bytes, stringCount), { objectMode: false }), this.#stream);
    } finally {
      this.#rowChunk = undefined;
      this.#rowChunkLength = 0;
      this.#rowOffsets.clear();
      this.#strings?.free();
      this.#strings = undefined;
      if (this.#directory) rmSync(this.#directory, { recursive: true, force: true });
    }
  }

  async *#archive(bytes, stringCount) {
    const document = this.#worksheets.length ? new XlsxDocument(bytes) : undefined;
    const reader = document ? undefined : new XlsxZipReader(bytes.length,
      (offset, length) => bytes.subarray(offset, offset + length));
    const zip = new XlsxZipStream();
    try {
      const compressed = zip.configure_compression(...this.#compression);
      if (this.#forceLocalTime !== undefined) {
        const now = new Date();
        const fields = this.#forceLocalTime
          ? [now.getFullYear(), now.getMonth() + 1, now.getDate(), now.getHours(), now.getMinutes(), now.getSeconds()]
          : [now.getUTCFullYear(), now.getUTCMonth() + 1, now.getUTCDate(), now.getUTCHours(), now.getUTCMinutes(), now.getUTCSeconds()];
        zip.set_timestamp(Uint16Array.from(fields));
      }
      if (this.#forceZip64) zip.force_zip64();
      if (this.#zipComment !== undefined) zip.set_comment(this.#zipComment);
      const dateSheet = this.#usesDates ? this.#worksheets[0] : undefined;
      if (dateSheet && document.cell_style_index(dateSheet.name, 0, 0) !== dateStyle) {
        throw new Error('Unexpected template date style');
      }
      const worksheetParts = new Map(this.#worksheets.map(sheet => [`xl/worksheets/sheet${sheet.id}.xml`, sheet]));
      const parts = document ? document.zip_entry_names() : reader.entry_names();
      if (stringCount && !parts.includes('xl/sharedStrings.xml')) throw new Error('Missing template shared strings');
      for (const part of parts) {
        const name = part.replace(/^\//, '');
        zip.start_configured_entry(name);
        const sheet = worksheetParts.get(name);
        const path = sheet && this.#directory ? this.#rowPath(sheet.id) : undefined;
        const hasRows = path && existsSync(path);
        if (sheet) {
          const xml = decoder.decode(document.zip_entry_bytes(part));
          const fragments = xml.split(/<sheetData(?:\s*\/>|>.*?<\/sheetData>)/s);
          if (fragments.length !== 2) throw new Error('Missing template worksheet data');
          const range = sheet.dimension;
          const dimension = range ? `<dimension ref="${range}"/>` : '';
          const oldDimension = /<dimension\b[^>]*\/>/;
          const before = oldDimension.test(fragments[0])
            ? fragments[0].replace(oldDimension, dimension)
            : fragments[0].replace(/<worksheet\b[^>]*>(?:<sheetPr\b[^>]*(?:\/>|>[\s\S]*?<\/sheetPr>))?/, '$&' + dimension);
          zip.write_chunk(encoder.encode(before + '<sheetData>'));
          if (hasRows) {
            if (!compressed) {
              const prefix = zip.drain();
              if (prefix.length) yield prefix;
            }
            for await (const chunk of createReadStream(path)) {
              if (compressed) {
                zip.write_chunk(chunk);
                const output = zip.drain();
                if (output.length) yield output;
              } else {
                zip.write_stored_chunk(chunk);
                yield chunk;
              }
            }
          }
          zip.write_chunk(encoder.encode('</sheetData>' + fragments[1]));
        } else if (stringCount && name === 'xl/sharedStrings.xml') {
          zip.write_chunk(encoder.encode('<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'));
          for (let start = 0; start < stringCount; start += 256) {
            zip.write_chunk(encoder.encode(this.#strings.items_xml(start, 256)));
            const output = zip.drain();
            if (output.length) yield output;
          }
          zip.write_chunk(encoder.encode('</sst>'));
        } else {
          zip.write_chunk(document ? document.zip_entry_bytes(part) : reader.read_metadata(part, 0xffffffff));
        }
        const output = zip.drain();
        if (output.length) yield output;
      }
      yield zip.finish();
    } finally {
      zip.free();
      document?.free();
      reader?.free();
    }
  }
}
