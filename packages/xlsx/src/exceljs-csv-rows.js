import { csvWorksheetSettings, csvWriteStreamSettings } from './exceljs-options.generated.js';

export const csvScalarCells = Symbol('csvScalarCells');

function formatValue(value, options, formatDate) {
  if (!value || typeof value !== 'object') return value;
  if (value.text || value.hyperlink) return value.hyperlink || value.text;
  if (value.formula || value.result) return value.result || '';
  if (value instanceof Date) return formatDate(value, options);
  if (value.error) return value.error;
  return JSON.stringify(value);
}

function* projectedRows(sheet, fields) {
  for (let index = 0; index < fields.length;) {
    const number = fields[index];
    const values = [];
    while (index < fields.length && fields[index] === number) {
      const column = fields[index + 1];
      const date = fields[index + 2];
      const value = fields[index + 3];
      values[column] = date === null ? sheet.getCell(number, column + 1).value : date ? new Date(value) : value;
      index += 4;
    }
    yield { number, values };
  }
}

function documentRows(sheet) {
  const rows = [];
  sheet.eachRow(row => rows.push(row));
  return rows;
}

export function worksheetRows(workbook, options, formatDate) {
  const { sheetName, sheetId, includeEmptyRows, map, dateFormat, dateUTC } = csvWorksheetSettings(options);
  const sheet = workbook.getWorksheet(sheetName || sheetId);
  const dateOptions = { dateFormat, dateUTC };
  const mapValue = map || (value => formatValue(value, dateOptions, formatDate));
  return (function* rows() {
    if (!sheet) return;
    // ExcelJS starts counting leading empty rows after row 1.
    let nextRow = 2;
    const fields = !map && !csvWriteStreamSettings(options).formatterOptions?.transform ? sheet[csvScalarCells]() : [];
    const populated = fields.length ? projectedRows(sheet, fields) : documentRows(sheet);
    for (const row of populated) {
      if (includeEmptyRows) {
        while (nextRow < row.number) {
          yield [];
          nextRow += 1;
        }
      }
      yield (fields.length ? row.values : row.values.slice(1)).map(mapValue);
      nextRow = row.number + 1;
    }
  })();
}
