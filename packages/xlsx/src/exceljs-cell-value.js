import { data, failure } from './contract.js';
import { isExcelErrorCode } from './exceljs-options.generated.js';

export const ValueType = {
  Null: 0, Merge: 1, Number: 2, String: 3, Date: 4, Hyperlink: 5,
  Formula: 6, SharedString: 7, RichText: 8, Boolean: 9, Error: 10,
};

export const FormulaType = { None: 0, Master: 1, Shared: 2 };

export function excelDateMillis(value) {
  const millis = Date.prototype.getTime.call(value);
  if (!Number.isFinite(millis)) throw failure('INVALID_VALUE', 'Expected a valid Date');
  return millis;
}

export function formulaErrorCode(value) {
  if (value == null || typeof value !== 'object' || Reflect.ownKeys(value).length !== 1) return undefined;
  const code = data(value, 'error');
  return isExcelErrorCode(code) ? code : undefined;
}

export function typeOf(value) {
  if (value == null) return ValueType.Null;
  if (value instanceof Date) return ValueType.Date;
  if (typeof value === 'number') return ValueType.Number;
  if (typeof value === 'string') return ValueType.String;
  if (typeof value === 'boolean') return ValueType.Boolean;
  if (value.richText !== undefined) return ValueType.RichText;
  if (value.hyperlink !== undefined) return ValueType.Hyperlink;
  if (value.formula !== undefined || value.sharedFormula !== undefined) return ValueType.Formula;
  return ValueType.Error;
}

export function effectiveTypeOf(value) {
  const type = typeOf(value);
  if (type !== ValueType.Formula) return type;
  const resultType = typeOf(value.result);
  return resultType === ValueType.Boolean || resultType === ValueType.Error ? ValueType.Null : resultType;
}

export function textOf(value) {
  if (value == null) return '';
  if (value.richText !== undefined) return value.richText.map((run) => run.text).join('');
  if (value.hyperlink !== undefined) return value.text;
  if (value.formula !== undefined || value.sharedFormula !== undefined) return String(value.result || '');
  if (value.error !== undefined) return value.error;
  return String(value);
}

const htmlEntities = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' };

export function escapeCellHtml(text) {
  return text.replace(/[&<>"]/g, character => htmlEntities[character]);
}

export function csvTextOf(value, quoteRichText = false) {
  if (typeof value === 'string' || (quoteRichText && value?.richText !== undefined)) {
    return '"' + textOf(value).replaceAll('"', '""') + '"';
  }
  if (typeof value === 'boolean') return Number(value);
  if (value?.hyperlink !== undefined) return value.hyperlink;
  if (value instanceof Date) return value.toISOString();
  return textOf(value);
}

export function projectCellModel(cell, readNote) {
  const master = cell.master;
  const value = master === cell ? cell.value : undefined;
  const type = master === cell ? typeOf(value) : ValueType.Merge;
  const model = { address: cell.address, type };
  switch (type) {
    case ValueType.Merge:
      model.master = master.address;
      break;
    case ValueType.Null:
      break;
    case ValueType.Formula: {
      const { shareType, ref, formula, sharedFormula, result } = value;
      Object.assign(model, { shareType, ref, formula, sharedFormula, result });
      break;
    }
    case ValueType.Hyperlink: {
      const { text, hyperlink, tooltip } = value;
      Object.assign(model, { text, hyperlink });
      if (tooltip) model.tooltip = tooltip;
      break;
    }
    case ValueType.RichText:
      model.type = ValueType.String;
      model.value = value;
      break;
    default:
      model.value = value;
  }
  model.style = cell.style;
  const note = readNote(true);
  if (note !== undefined) model.comment = { type: 'note', note };
  return model;
}
