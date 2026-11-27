import { readFileSync } from 'node:fs';
import { initSync } from '../wasm/ooxml_xlsx_wasm.js';

initSync({ module: readFileSync(new URL('../wasm/ooxml_xlsx_wasm_bg.wasm', import.meta.url)) });
