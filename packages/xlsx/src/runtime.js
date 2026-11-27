import init, { XlsxDocument, XlsbArchive, biff_document_properties, read_biff_shared_strings, write_biff_shared_strings, write_biff_property_streams, document_property_parts, write_xlsb_cells, write_xlsb_shared_strings, write_xlsb_sheet_header, write_xlsb_sheet_end, write_xlsb_styles, write_xlsb_workbook, read_ods_parts, write_ods, write_zip_entries } from '../wasm/ooxml_xlsx_wasm.js';

await init();

export { XlsxDocument, XlsbArchive, biff_document_properties, read_biff_shared_strings, write_biff_shared_strings, write_biff_property_streams, document_property_parts, write_xlsb_cells, write_xlsb_shared_strings, write_xlsb_sheet_header, write_xlsb_sheet_end, write_xlsb_styles, write_xlsb_workbook, read_ods_parts, write_ods, write_zip_entries };
