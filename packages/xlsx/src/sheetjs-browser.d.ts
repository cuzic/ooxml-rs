import sheetjs from './sheetjs.js';
import { CFB } from './sheetjs-cfb.js';

export * from './sheetjs.js';
export { CFB };

declare const browser: typeof sheetjs & { CFB: typeof CFB };
export default browser;
