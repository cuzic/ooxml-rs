import { failure } from './contract.js';

let parseNodeXml;

export function setXmlParser(parse) {
  parseNodeXml = parse;
}

export function xmlRoot(input, format) {
  if (typeof DOMParser !== 'function' && !parseNodeXml) {
    throw failure('UNSUPPORTED_ENVIRONMENT', `${format} reading requires DOMParser`);
  }
  if (/<!DOCTYPE/i.test(input)) throw failure('INVALID_INPUT', `${format} cannot contain a document type`);
  let document;
  try {
    document = parseNodeXml ? parseNodeXml(input) : new DOMParser().parseFromString(input, 'application/xml');
  } catch (error) {
    throw failure('INVALID_INPUT', `Invalid ${format} XML`, error);
  }
  const root = document.documentElement;
  if (!root || root.localName === 'parsererror') throw failure('INVALID_INPUT', `Invalid ${format} XML`);
  return root;
}
