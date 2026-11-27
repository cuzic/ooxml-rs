import './runtime-node.js';

import { DOMParser } from '@xmldom/xmldom';
import CFB from 'cfb';
import { setXmlParser } from './sheetjs-xml.js';
import { parseHtmlTables } from './sheetjs-html-node.js';
import { setHtmlParser } from './sheetjs-dom.js';
import { setCompoundFormat } from './sheetjs-xls.js';
import sheetjs from './sheetjs.js';

setHtmlParser(parseHtmlTables);
setXmlParser(text => new DOMParser({
  onError(_level, message) { throw new Error(message); },
}).parseFromString(text, 'application/xml'));

setCompoundFormat(CFB);

export * from './sheetjs.js';
export { CFB };
export default { ...sheetjs, CFB };
