export function decodeBookProperties(fields) {
  const Props = {};
  const Custprops = {};
  for (let index = 0; index < fields.length; index += 4) {
    const [section, key, kind, text] = fields.slice(index, index + 4);
    const target = section === 'custom' ? Custprops : Props;
    let value;
    if (section === 'standard' && kind === 'item') {
      target[key].push(text);
      continue;
    }
    if (section === 'standard' && kind === 'array') value = [];
    else if (['string', 'lpstr', 'bstr', 'lpwstr', 'cy', 'error'].includes(kind)) value = text;
    else if (kind === 'bool') value = text === 'true' || text === '1';
    else if (['i1', 'i2', 'i4', 'i8', 'int', 'uint'].includes(kind)) value = Number.parseInt(text, 10);
    else if (['r4', 'r8', 'decimal'].includes(kind)) value = Number.parseFloat(text);
    else if (['date', 'filetime'].includes(kind)) value = new Date(text);
    else continue;
    Object.defineProperty(target, key, { value, enumerable: true, configurable: true, writable: true });
  }
  return { Props, Custprops };
}
