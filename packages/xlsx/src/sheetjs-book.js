import { hasAllowedWorksheetNameEdges, hasAllowedWorksheetNamePunctuation, isWorksheetVisibility } from './sheetjs-options.generated.js';
export function book_new(sheet, name) {
  const book = { SheetNames: [], Sheets: {} };
  if (sheet) book_append_sheet(book, sheet, name || 'Sheet1');
  return book;
}

export function book_append_sheet(book, sheet, name, roll) {
  const names = book.SheetNames;
  if (!name) {
    let index = 1;
    while (index <= names.length && names[index - 1] === `Sheet${index}`) index += 1;
    if (index > names.length && index <= 65535) {
      name = `Sheet${index}`;
    } else {
      const used = new Set(names);
      for (; index <= 65535; index += 1) {
        const candidate = `Sheet${index}`;
        if (!used.has(candidate)) {
          name = candidate;
          break;
        }
      }
    }
  }
  if (!name || book.SheetNames.length >= 65535) throw new Error('Too many worksheets');
  if (roll && names.includes(name) && name.length < 32) {
    const used = new Set(names);
    const suffix = /\d+$/.exec(name);
    const prefix = suffix ? name.slice(0, suffix.index) || name : name;
    for (let index = Number(suffix?.[0] ?? 0) + 1; index <= 65535; index += 1) {
      name = prefix + index;
      if (!used.has(name)) break;
    }
  }
  if (name.length > 31) throw new Error('Sheet name cannot exceed 31 chars');
  if (!hasAllowedWorksheetNameEdges(name)) {
    throw new Error("Sheet name cannot start or end with apostrophe (')");
  }
  if (name.toLowerCase() === 'history') throw new Error("Sheet name cannot be 'History'");
  if (!hasAllowedWorksheetNamePunctuation(name)) throw new Error('Sheet name cannot contain : \\ / ? * [ ]');
  if (names.includes(name)) throw new Error(`Worksheet with name |${name}| already exists!`);
  Object.defineProperty(book.Sheets, name, { value: sheet, enumerable: true, configurable: true, writable: true });
  book.SheetNames.push(name);
  return name;
}

export function book_set_sheet_visibility(book, sheet, visibility) {
  book.Workbook ??= {};
  book.Workbook.Sheets ??= [];
  let index;
  if (typeof sheet === 'number') {
    if (!Number.isInteger(sheet) || sheet < 0 || sheet >= book.SheetNames.length) {
      throw new Error(`Cannot find sheet # ${sheet}`);
    }
    index = sheet;
  } else if (typeof sheet === 'string') {
    index = book.SheetNames.indexOf(sheet);
    if (index === -1) throw new Error(`Cannot find sheet name |${sheet}|`);
  } else {
    throw new Error(`Cannot find sheet |${sheet}|`);
  }
  book.Workbook.Sheets[index] ??= {};
  if (!isWorksheetVisibility(visibility)) {
    throw new Error(`Bad sheet visibility setting ${visibility}`);
  }
  book.Workbook.Sheets[index].Hidden = visibility;
}
