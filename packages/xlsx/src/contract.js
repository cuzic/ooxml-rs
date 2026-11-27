export function failure(code, message, cause) {
  return Object.assign(new Error(message, { cause }), { code });
}

export function rustCall(code, action) {
  try {
    return action();
  } catch (error) {
    if (error?.code) throw error;
    throw failure(code, error?.message ?? String(error), error);
  }
}

export function copyInput(input) {
  if (input instanceof ArrayBuffer) return new Uint8Array(input).slice();
  if (input instanceof Uint8Array) return new Uint8Array(input);
  throw failure('INVALID_INPUT', 'Expected ArrayBuffer or Uint8Array');
}

export function address(value) {
  if (typeof value !== 'string' || !/^[A-Za-z]+[0-9]+$/.test(value)) {
    throw failure('INVALID_ADDRESS', 'Expected a cell address');
  }

  const canonical = value.toUpperCase();
  const parts = /^([A-Z]{1,3})([1-9][0-9]{0,6})$/.exec(canonical);
  if (parts && Number(parts[2]) <= 1048576) {
    let column = 0;
    for (const letter of parts[1]) {
      column = column * 26 + letter.charCodeAt(0) - 64;
    }
    if (column <= 16384) return [canonical, Number(parts[2]) - 1, column - 1];
  }
  throw failure('INVALID_ADDRESS', `Invalid XLSX cell address: ${value}`);
}

export function scalar(value, allowBoolean) {
  if (typeof value === 'number' && Number.isFinite(value)) return value === 0 ? 0 : value;
  if (typeof value === 'boolean' && allowBoolean) return value;
  if (typeof value === 'string' && !/[\uD800-\uDFFF]/u.test(value)) return value;
  throw failure('INVALID_VALUE', 'Expected a supported scalar cell value');
}

export function optionsMatch(options, expected, allowOmission = true) {
  if (options === undefined && allowOmission) return;
  if (options !== null && typeof options === 'object' && !Array.isArray(options)) {
    const keys = Reflect.ownKeys(options);
    if (keys.length === Object.keys(expected).length && keys.every((key) => {
      const descriptor = Object.getOwnPropertyDescriptor(options, key);
      return Object.hasOwn(expected, key) && Object.hasOwn(descriptor, 'value') &&
        descriptor.value === expected[key];
    })) return;
  }
  throw failure('UNSUPPORTED_OPTIONS', 'Unsupported XLSX options');
}


export function binaryStringBytes(input) {
  const bytes = new Uint8Array(input.length);
  for (let index = 0; index < input.length; index += 1) bytes[index] = input.charCodeAt(index);
  return bytes;
}

export function decodeBase64(input) {
  if (typeof input !== 'string') throw failure('INVALID_INPUT', 'Expected a Base64 XLSX string');
  try {
    const normalized = input.replace(/[-_]/g, character => character === '-' ? '+' : '/');
    if (typeof Uint8Array.fromBase64 === 'function') return Uint8Array.fromBase64(normalized);
    return binaryStringBytes(atob(normalized));
  } catch (error) {
    throw failure('INVALID_INPUT', 'Invalid Base64 XLSX input', error);
  }
}

export function data(object, key) {
  return Object.getOwnPropertyDescriptor(object, key)?.value;
}

export const imageFileReader = Symbol('imageFileReader');
export const prepareImageFile = Symbol('prepareImageFile');

export const registerStreamingStyle = Symbol('registerStreamingStyle');
export const prepareStreamingCellModel = Symbol('prepareStreamingCellModel');
export const applyStreamingNotes = Symbol('applyStreamingNotes');
export const applyStreamingHyperlinks = Symbol('applyStreamingHyperlinks');

export const translateStreamingFormula = Symbol('translateStreamingFormula');
export const replaceCellNames = Symbol('replaceCellNames');

export const validateStreamingRowStyle = Symbol('validateStreamingRowStyle');
