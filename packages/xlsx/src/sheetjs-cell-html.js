// HTML (`h`) for SheetJS string cells and comments, assembled from text and
// run fonts that the Wasm reader decoded.

const plainEntities = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;' };
const plainSpecial = /[&<>"'\u0000-\u001f]/;

// CE 0.20.3 escapehtml: XML entities, line feeds as <br/>, and the other C0
// controls as four-digit hexadecimal references.
export function plainStringHtml(text) {
  if (!plainSpecial.test(text)) return text;
  return text.replace(/[&<>"']/g, (character) => plainEntities[character]).replace(/\n/g, '<br/>')
    .replace(/[\u0000-\u001f]/g, (character) => `&#x${character.charCodeAt(0).toString(16).padStart(4, '0')};`);
}

const runEntities = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
const underlineStyles = { double: 'double', singleAccounting: 'single-accounting', doubleAccounting: 'double-accounting' };
// Declared-field bits from the Wasm reader (see exceljs-rich-text.js).
const declaredSize = 2;
const declaredOutline = 4;
const declaredShadow = 8;

// Runs of 14 fields: text, font (empty without rPr) and declared-field bits.
// Like CE 0.20.3, empty runs are skipped and a run with properties gets a
// span whose style lists underline, declared size, outline and shadow.
export function richStringHtml(fields, offset = 0, end = fields.length) {
  const html = [];
  for (let index = offset; index < end; index += 14) {
    const [run, , size, bold, italic, strike, underline, vertical] = fields.slice(index, index + 8);
    if (!run) continue;
    let markup = run.replace(/[&<>"']/g, (character) => runEntities[character]).replace(/\r?\n/g, '<br/>');
    if (vertical === 'superscript') markup = `<sup>${markup}</sup>`;
    if (vertical === 'subscript') markup = `<sub>${markup}</sub>`;
    if (strike === 'true') markup = `<s>${markup}</s>`;
    if (italic === 'true') markup = `<i>${markup}</i>`;
    if (bold === 'true') markup = `<b>${markup}</b>`;
    if (size !== '') {
      const declared = Number(fields[index + 13]);
      const style = (underline ? 'text-decoration: underline;' : '') +
        (underlineStyles[underline] ? `text-underline-style:${underlineStyles[underline]};` : '') +
        (declared & declaredSize ? `font-size:${size}pt;` : '') +
        (declared & declaredOutline ? 'text-effect: outline;' : '') +
        (declared & declaredShadow ? 'text-shadow: auto;' : '');
      markup = `<span style="${style}">${markup}</span>`;
    }
    html.push(markup);
  }
  return html.join('');
}
