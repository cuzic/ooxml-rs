// Assembles the XLSX read projection of workbook.xml metadata (`Workbook`) from Wasm results.
import { data, failure } from './contract.js';
import { checkedWorkbookViews, workbookViewAttributes, xlsxWorkbookViewSettings } from './sheetjs-options.generated.js';

// CE 0.20.3 copies the root's default namespace. Only transitional SpreadsheetML
// workbooks are read, so it is always this one.
export const workbookNamespace = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';

const viewAttributes = Object.entries(workbookViewAttributes);

// `fields` holds the workbookView attributes in `workbookViewAttributes` order,
// '' when absent. Like CE 0.20.3, a present attribute is kept as a raw string with
// a lowercase alias, the WBViewDef keys are parsed or defaulted, and defaults follow
// the attributes. Other attributes (tabRatio, showSheetTabs...) are not exposed by Wasm.
export function projectedWorkbookViews(fields) {
  const views = [];
  for (let offset = 0; offset < fields.length; offset += viewAttributes.length) {
    const view = {};
    const defaults = [];
    viewAttributes.forEach(([name, { default: fallback, integer }], index) => {
      const value = fields[offset + index];
      if (value === '') {
        if (fallback !== undefined) defaults.push([name, fallback]);
        return;
      }
      view[name] = integer ? Number.parseInt(value, 10) : value;
      view[name.toLowerCase()] ??= value;
    });
    for (const [name, value] of defaults) view[name] = value;
    views.push(view);
  }
  return views;
}

// CE 0.20.3 keeps each <sheet> attribute: sheetId as a raw string with its
// lowercase alias. Wasm exposes sheet IDs of worksheets only; r:id is not exposed.
export function projectedSheetEntries(names, visibility, worksheetNames, worksheetIds) {
  const ids = new Map(worksheetNames.map((name, index) => [name, worksheetIds[index]]));
  return names.map((name, index) => {
    const id = ids.get(name);
    return id === undefined ? { name, Hidden: visibility[index] } :
      { name, sheetId: String(id), sheetid: String(id), Hidden: visibility[index] };
  });
}

// Workbook.Views of a new XLSX workbook: CE 0.20.3 writes Views[0].RTL as
// rightToLeft on every worksheet view. Returns undefined without Views[0].
export function workbookRightToLeft(metadata) {
  if (!metadata || !Object.hasOwn(metadata, 'Views')) return undefined;
  const views = checkedWorkbookViews(data(metadata, 'Views'));
  const settings = [];
  for (let index = 0; index < views.length; index += 1) {
    const view = data(views, index);
    settings.push(view == null ? undefined : xlsxWorkbookViewSettings(view));
  }
  return settings[0] && Boolean(settings[0].RTL);
}

// A loaded workbook keeps its worksheet XML. Its read projection has no Views or
// sheet CodeName, so these are additions that Rust cannot write yet.
export function checkLoadedWorkbookMetadata(metadata, hasVba) {
  if (!metadata) return;
  if (Object.hasOwn(metadata, 'Views')) {
    throw failure('UNSUPPORTED_STRUCTURE', 'Writing Workbook.Views to a loaded workbook is unsupported');
  }
  // CE 0.20.3 writes Sheets[].CodeName as sheetPr codeName for VBA workbooks.
  if (hasVba && (data(metadata, 'Sheets') ?? []).some((entry) => entry && data(entry, 'CodeName'))) {
    throw failure('UNSUPPORTED_STRUCTURE', 'Writing a worksheet CodeName is unsupported');
  }
}
