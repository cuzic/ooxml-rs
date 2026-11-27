import { readFileSync, writeFileSync } from 'node:fs';
import { inspect } from 'node:util';
import { contracts as sheetjs, fileOutputFormats, readOptionsByFormat, writeOptionsByFormat, writeOutputTypes } from './sheetjs-option-contracts.mjs';
import { contracts as exceljs, streamingReaderModes } from './exceljs-option-contracts.mjs';

const invalidUnicode = value => `/[\\uD800-\\uDFFF]/u.test(${value})`;

const invalidUnicodeString = value => `typeof ${value} !== 'string' || ${invalidUnicode(value)}`;

const invalidDateTime = value => `!Number.isFinite(Date.prototype.getTime.call(${value}))`;

const zipCompressionMethods = ['DEFLATE', 'STORE'];

const predicates = {
  unicodeString: invalidUnicodeString,
  optionalUtf8ByteLimit: (value, limit) => `${value} !== undefined && (typeof ${value} !== 'string' ||
      ${value}.length > ${limit} || new TextEncoder().encode(${value}).length > ${limit})`,
  validDate: value => `!(${value} instanceof Date) || ${invalidDateTime(value)}`,
  validDateTime: invalidDateTime,
  nonemptyUnicodeString: value => `${invalidUnicodeString(value)} || !${value}`,
  optionalNonemptyUnicodeString: value => `${value} !== undefined && (${invalidUnicodeString(value)} || !${value})`,
  optionalUnicodeString: value => `${value} !== undefined && (${invalidUnicodeString(value)})`,
  record: value => `${value} === null || typeof ${value} !== 'object' || Array.isArray(${value})`,
  definedName: value => `typeof ${value} !== 'string' || ${value}.length > 255 ||
    !/^[\\p{L}_\\\\][\\p{L}\\p{N}_.\\\\]*$/u.test(${value}) || ${value}.toLowerCase().startsWith('_xlnm.')`,
  wellFormedUnicode: invalidUnicode,
  optionalNullableUnicodeString: value =>
    `${value} != null && (${invalidUnicodeString(value)})`,
  noWorksheetNamePunctuation: value => `/[*?:/\\\\[\\]]/.test(${value})`,
  noEdgeApostrophe: value => `${value}.startsWith("'") || ${value}.endsWith("'")`,
  objectWithOwnKeys: (value, keys) =>
    `${value} === null || typeof ${value} !== 'object' || Reflect.ownKeys(${value}).some(key => ${keys.map(key => `key !== ${JSON.stringify(key)}`).join(' && ')})`,
  undefined: value => `${value} !== undefined`,
  csvDelimiter: value => `${value}?.length !== 1`,
  csvOptionalQuote: value => `${value} && ${value}.length !== 1`,
  truthy: value => value,
  oneOf: (value, choices) => `!${JSON.stringify(choices)}.includes(${value})`,
  optionalArgbHex: value => `${value} !== undefined && (typeof ${value} !== 'string' || !/^[0-9a-f]{8}$/i.test(${value}))`,
  string: value => `typeof ${value} !== 'string'`,
  nonemptyString: value => `typeof ${value} !== 'string' || !${value}`,
  nonemptyArray: value => `!Array.isArray(${value}) || !${value}.length`,
  array: value => `!Array.isArray(${value})`,
  arrayLength: (value, length) => `!Array.isArray(${value}) || ${value}.length !== ${length}`,
  formulaOperand: value => `!Number.isFinite(${value}) && !(typeof ${value} === 'string' && ${value}.trim() && !${invalidUnicode(value)})`,
  nonblankUnicodeString: value => `${invalidUnicodeString(value)} || !${value}.trim()`,
  function: value => `typeof ${value} !== 'function'`,
  truthyFunction: value => `${value} && typeof ${value} !== 'function'`,
  truthyStringArray: value =>
    `${value} && (!Array.isArray(${value}) || Array.from(${value}).some(item => typeof item !== 'string'))`,
  nullish: value => `${value} != null`,
  optionalArray: value => `${value} !== undefined && !Array.isArray(${value})`,
  optionalArrayOrFunction: value => `${value} != null && !Array.isArray(${value}) && typeof ${value} !== 'function'`,
  falsy: value => `!${value}`,
  optionalListOf: (value, choices) => `${value} !== undefined &&
      (!Array.isArray(${value}) || Array.from(${value}).some(item => !${JSON.stringify(choices)}.includes(item)))`,
  optionalOneOf: (value, choices) => `${value} !== undefined && !${JSON.stringify(choices)}.includes(${value})`,
  nullableNonnegativeNumber: value => `${value} != null && (!Number.isFinite(${value}) || ${value} < 0)`,
  optionalNonnegativeTuple: (value, length) => `${value} !== undefined && (!Array.isArray(${value}) ||
      ${value}.length !== ${length} || Array.from(${value}).some(item => typeof item !== 'number' ||
        !Number.isFinite(item * 10) || item < 0))`,
  optionalNonnegativeNumber: value => `${value} !== undefined && (!Number.isFinite(${value}) || ${value} < 0)`,
  nonnegativeNumber: value => `!Number.isFinite(${value}) || ${value} < 0`,
  optionalIntegerRange: (value, [min, max]) =>
    `${value} !== undefined && (!Number.isInteger(${value}) || ${value} < ${min} || ${value} > ${max})`,
  numberRange: (value, [min, max]) =>
    `!Number.isFinite(${value}) || ${value} < ${min} || ${value} > ${max}`,
  numberBelow: (value, [min, max]) =>
    `!Number.isFinite(${value}) || ${value} < ${min} || ${value} >= ${max}`,
  optionalNumberRange: (value, [min, max]) =>
    `${value} !== undefined && (!Number.isFinite(${value}) || ${value} < ${min} || ${value} > ${max})`,
  nonnegativeInteger: value => `!Number.isInteger(${value}) || ${value} < 0`,
  integerRange: (value, [min, max]) => `!Number.isInteger(${value}) || ${value} < ${min} || ${value} > ${max}`,
  nullableU32: value => `${value} !== undefined && ${value} !== null &&
      (!Number.isInteger(${value}) || ${value} < 0 || ${value} > 0xffffffff)`,
  optionalExcelRotation: value => `${value} !== undefined && ${value} !== 'vertical' &&
      (!Number.isInteger(${value}) || Math.abs(${value}) > 90)`,
  optionalU32: value => `${value} !== undefined && (!Number.isInteger(${value}) || ${value} < 0 || ${value} > 0xffffffff)`,
  optionalZipCompression: value => `${value} !== undefined && (typeof ${value} !== 'string' || !${JSON.stringify(zipCompressionMethods)}.includes(${value}.toUpperCase()))`,
  boolean: value => `typeof ${value} !== 'boolean'`,
  optionalNullableString: value => `${value} !== undefined && ${value} !== null && typeof ${value} !== 'string'`,
  nullableString: value => `${value} !== null && typeof ${value} !== 'string'`,
  optionalString: value => `${value} !== undefined && typeof ${value} !== 'string'`,
  optionalBoolean: value => `${value} !== undefined && typeof ${value} !== 'boolean'`,
  optionalSheetSelectors: value => `${value} !== undefined &&
      (Array.isArray(${value}) ? ${value} : [${value}]).some(key => typeof key !== 'string' && !Number.isInteger(key))`,
  nullishNonnegativeSafeInteger: value => `${value} != null && (!Number.isSafeInteger(${value}) || ${value} < 0)`,
  optionalNonnegativeInteger: value => `${value} !== undefined && (!Number.isInteger(${value}) || ${value} < 0)`,
};

function validateRules(name, availableKeys, rules) {
  for (const [key, predicate, , choices] of rules) {
    if (!availableKeys.includes(key) || !Object.hasOwn(predicates, predicate) ||
        (predicate === 'objectWithOwnKeys' && (!Array.isArray(choices) || choices.length === 0 ||
          choices.some(choice => typeof choice !== 'string') || new Set(choices).size !== choices.length)) ||
        (['arrayLength', 'optionalNonnegativeTuple', 'optionalUtf8ByteLimit'].includes(predicate) && (!Number.isSafeInteger(choices) || choices < 0)) ||
        (['oneOf', 'optionalOneOf', 'optionalListOf'].includes(predicate) && (!Array.isArray(choices) || choices.length === 0 ||
          choices.some(choice => typeof choice !== 'string' &&
            (predicate === 'optionalListOf' || (typeof choice !== 'boolean' && !Number.isSafeInteger(choice)))))) ||
        (['integerRange', 'optionalIntegerRange', 'numberRange', 'numberBelow', 'optionalNumberRange'].includes(predicate) && (!Array.isArray(choices) || choices.length !== 2 ||
          !choices.every(Number.isSafeInteger) || choices[0] > choices[1]))) {
      throw new Error(`Invalid validation rule for ${name}.${key}`);
    }
  }
}

const defaultDeflateLevel = 6;

const projectionTransforms = {
  identity: value => value,
  zipCompression: value => `${value}.toUpperCase() === "DEFLATE"`,
  deflateLevel: value => `(${value} === undefined ? undefined : ${value} > 0 ? ${value} : ${defaultDeflateLevel})`,
  anyTruthy: (a, b) => `Boolean(${a} || ${b})`,
  allTruthy: (a, b) => `Boolean(${a} && ${b})`,
  enabledUnless: (value, excluded) => `${value} && !${excluded}`,
  truthy: value => `Boolean(${value})`,
  unlessFalse: value => `${value} !== false`,
  sheetSelection: value => `sheetSelection(${value})`,
  xlsbRowLimit: value => `(${value} ? Math.ceil(Number(${value})) : Infinity)`,
};

function generateProjection({ name, fields }, keys) {
  const exports = Object.entries(fields).map(([field, { from, transform }]) => {
    const options = Array.isArray(from) ? from : [from];
    if (!options.length || options.some(key => !keys.includes(key)) ||
        !Object.hasOwn(projectionTransforms, transform) ||
        options.length !== (['anyTruthy', 'allTruthy', 'enabledUnless'].includes(transform) ? 2 : 1)) {
      throw new Error(`Invalid projection field for ${name}.${field}`);
    }
    const expression = projectionTransforms[transform](...options.map(key => `settings.${key}`));
    return { field, expression, rowLimit: transform === 'xlsbRowLimit' };
  });
  const limits = exports.filter(({ rowLimit }) => rowLimit);
  return [
    '',
    `export function ${name}(settings) {`,
    ...limits.map(({ field, expression }) => `  const ${field} = ${expression};`),
    '  return {',
    ...exports.map(({ field, expression, rowLimit }) =>
      `    ${field}: ${rowLimit ? `${field} <= 1048576 ? Math.max(0, ${field}) : undefined` : expression},`),
    '  };',
    '}',
  ];
}

function generatePropertyDefaults(name, properties, defaults = {}) {
  return Object.entries(defaults).map(([exportName, values]) => {
    const entries = Object.entries(values);
    if (entries.some(([key]) => !properties.includes(key))) {
      throw new Error(`Unknown property default in ${name}.${exportName}`);
    }
    const fields = entries.map(([key, value]) => {
      const literal = JSON.stringify(value);
      return `  ${JSON.stringify(key)}: ${Array.isArray(value) ? `Object.freeze(${literal})` : literal},`;
    });
    return `export const ${exportName} = Object.freeze({
${fields.join('\n')}
});
`;
  }).join('');
}

// Field types keep presence separate from the value check used by the generator.
const fieldTypeChecks = {
  boolean: { required: 'boolean', optional: 'optionalBoolean' },
  integer: { required: 'integerRange', optional: 'optionalIntegerRange' },
  unicodeString: { required: 'unicodeString', optional: 'optionalUnicodeString' },
  nonblankUnicodeString: { required: 'nonblankUnicodeString' },
  nonemptyUnicodeString: { required: 'nonemptyUnicodeString', optional: 'optionalNonemptyUnicodeString' },
  enum: { required: 'oneOf', optional: 'optionalOneOf' },
};

function compileFieldContract(name, contract) {
  const { fields, variantFields, exportFieldDefaults, applyFieldDefaults, matchOptions, validateBeforeMatch, validationOrder, ...settings } = contract;
  if ((validateBeforeMatch || validationOrder) && (!matchOptions || !fields)) {
    throw new Error(`Validation ordering requires option matching fields in ${name}`);
  }
  if (!fields) return contract;
  const keys = Object.keys(fields);
  const variantKeys = Object.values(settings.variants ?? {}).flatMap(({ prefix = [], suffix = [] }) => [...prefix, ...suffix]);
  if (variantFields && (!matchOptions || Object.keys(variantFields).some(key =>
    keys.includes(key) || !variantKeys.includes(key)))) {
    throw new Error(`Invalid variant fields in ${name}`);
  }
  const entries = Object.entries({ ...fields, ...variantFields });
  if (settings.properties || settings.propertyGroups || settings.valueChecks || (exportFieldDefaults && settings.exportPropertyDefaults)) {
    throw new Error(`Conflicting field declarations in ${name}`);
  }
  const allowed = new Set(['type', 'presence', 'check', 'choices', 'range', 'length', 'message', 'code', 'default', 'defaultIfNullish', 'defaultIfFalsy', 'when']);
  for (const [key, field] of entries) {
    if (!field || typeof field !== 'object' || Array.isArray(field) ||
        Reflect.ownKeys(field).some(property => !allowed.has(property))) {
      throw new Error(`Invalid field declaration for ${name}.${key}`);
    }
  }
  const fieldDefaults = key => Object.fromEntries(entries
    .filter(([, field]) => Object.hasOwn(field, key))
    .map(([name, field]) => [name, field[key]]));
  const nullishDefaults = { ...settings.nullishDefaults, ...fieldDefaults('defaultIfNullish') };
  const truthyDefaults = { ...settings.truthyDefaults, ...fieldDefaults('defaultIfFalsy') };
  let defaults = fieldDefaults('default');
  if (applyFieldDefaults && (settings.defaults || settings.exportDefaults)) {
    throw new Error(`Conflicting field defaults in ${name}`);
  }
  if (Object.keys(defaults).length && !exportFieldDefaults && !applyFieldDefaults && !matchOptions) {
    throw new Error(`Missing export for field defaults in ${name}`);
  }
  let valueChecks = entries.flatMap(([key, { type, presence, check, choices, range, length, message, code, when }]) => {
    if (type !== undefined || presence !== undefined || range !== undefined) {
      const checks = Object.hasOwn(fieldTypeChecks, type) && fieldTypeChecks[type];
      if (check !== undefined || !checks || !Object.hasOwn(checks, presence) ||
          length !== undefined ||
          (type === 'enum' ? !Array.isArray(choices) || !choices.length : choices !== undefined) ||
          (type === 'integer' ? range === undefined : range !== undefined)) {
        throw new Error(`Invalid field type or presence for ${name}.${key}`);
      }
      check = checks[presence];
    }
    if ([check, choices, length, message, code, when].every(value => value === undefined)) return [];
    if (typeof message !== 'string' || !message || (choices !== undefined && length !== undefined)) {
      throw new Error(`Invalid field check for ${name}.${key}`);
    }
    return [[key, check, message, choices ?? range ?? length, code, when]];
  });
  if (validationOrder) {
    const checks = new Map(valueChecks.map(rule => [rule[0], rule]));
    if (validateBeforeMatch || !Array.isArray(validationOrder) || validationOrder.length !== checks.size ||
        new Set(validationOrder).size !== checks.size || validationOrder.some(key => !checks.has(key)) ||
        Object.keys(defaults).some(key => !checks.has(key))) {
      throw new Error(`Validation order must list every checked option exactly once in ${name}`);
    }
    valueChecks = validationOrder.map(key => checks.get(key));
    defaults = Object.fromEntries(validationOrder.filter(key => Object.hasOwn(defaults, key))
      .map(key => [key, defaults[key]]));
  }
  if (matchOptions) {
    if (applyFieldDefaults || exportFieldDefaults || settings.defaults || settings.keys ||
        settings.afterMatch || (validateBeforeMatch && (settings.beforeMatch || settings.variants)) ||
        Object.keys(nullishDefaults).length ||
        Object.keys(truthyDefaults).length ||
        valueChecks.some(([, , , , code, when]) => code !== undefined || when !== undefined)) {
      throw new Error(`Conflicting option matching declarations in ${name}`);
    }
    return {
      ...settings,
      keys,
      defaults,
      [validateBeforeMatch ? 'beforeMatch' : 'afterMatch']: Object.fromEntries(valueChecks.map(([key, check, message, choices]) =>
        [key, { check, message, choices }])),
    };
  }
  return {
    ...settings,
    nullishDefaults,
    truthyDefaults,
    properties: entries.map(([key]) => key),
    valueChecks,
    ...(applyFieldDefaults
      ? { defaults, exportDefaults: exportFieldDefaults }
      : exportFieldDefaults ? { exportPropertyDefaults: { [exportFieldDefaults]: defaults } } : {}),
  };
}

function expandVariantKeys(keys, variants) {
  return Object.fromEntries(Object.entries(variants).map(([variant, fields]) =>
    [variant, [...(fields.prefix ?? []), ...keys, ...(fields.suffix ?? [])]]));
}

function matchRules(rules = {}) {
  return Object.entries(rules).map(([key, { check, message, choices }]) => [key, check, message, choices]);
}

function generate(contracts) {
  const projections = Object.values(contracts).flatMap(contract => contract.projections ?? []);
  const hasSheetSelection = projections.some(({ fields }) =>
    Object.values(fields).some(({ transform }) => transform === 'sheetSelection'));
  return `// Generated by generate-sheetjs-options.mjs.
import { data, failure, optionsMatch } from './contract.js';

function collect(options, keys, settings) {
  for (const key of keys) {
    if (options && Object.hasOwn(options, key)) settings[key] = data(options, key);
  }
}

${hasSheetSelection ? `function sheetSelection(value) {
  if (value === undefined) return () => true;
  const selectors = new Set([value].flat().map(item => typeof item === 'string' ? item.toLowerCase() : item));
  return (name, index) => selectors.has(index) || selectors.has(name.toLowerCase());
}

` : ''}${Object.entries(contracts).map(([name, declaration]) => {
  const contract = compileFieldContract(name, declaration);
  if (contract.descriptorFields) {
    const { required = [], optional = [], omit = [], message } = contract.descriptorFields;
    const keys = [...required, ...optional, ...omit];
    if (new Set(keys).size !== keys.length || ((required.length || optional.length) && !message)) {
      throw new Error(`Invalid descriptor fields for ${name}`);
    }
    const checks = [...required, ...optional].map(key => {
      const absent = required.includes(key) ? `!${key} || ` : `${key} && `;
      return `  if (${absent}!Object.hasOwn(${key}, 'value')) {
    throw failure('UNSUPPORTED_OPTIONS', ${JSON.stringify(message)});
  }`;
    });
    const values = [...required, ...optional].map(key => `${key}: ${key}?.value`);
    return [
      `export function ${name}(options) {`,
      `  const { ${keys.join(', ')}, ...rest } = Object.getOwnPropertyDescriptors(options ?? {});`,
      ...checks,
      `  return { ${[...values, 'rest'].join(', ')} };`,
      '}',
    ].join('\n');
  }
  if (contract.variantOwnKeys) {
    const { common, variants, message, readDataProperty } = contract.variantOwnKeys;
    if (readDataProperty !== undefined && !common.includes(readDataProperty)) {
      throw new Error(`Expected a common data property in ${name}`);
    }
    const read = readDataProperty === undefined ? '' : `
  const descriptor = Object.getOwnPropertyDescriptor(options, ${JSON.stringify(readDataProperty)});
  if (descriptor && !Object.hasOwn(descriptor, 'value')) {
    throw failure('UNSUPPORTED_OPTIONS', ${JSON.stringify(message)});
  }
  return descriptor?.value;`;
    const keys = Object.fromEntries(Object.entries(variants).map(([variant, fields]) =>
      [variant, [...common, ...fields]]));
    return `const ${name}Keys = ${JSON.stringify(keys, null, 2)};

export function ${name}(options, variant) {
  if (!Object.hasOwn(${name}Keys, variant) ||
      Reflect.ownKeys(options).some(key => !${name}Keys[variant].includes(key))) {
    throw failure('UNSUPPORTED_OPTIONS', ${JSON.stringify(message)});
  }${read}
}`;
  }
  if (contract.numericEntries) {
    const { array, keys, absent, booleanFrom = {} } = contract.numericEntries;
    if (!['Float64Array', 'Uint8Array'].includes(array) ||
        Object.keys(booleanFrom).some(key => !keys.includes(key)) ||
        !(Number.isSafeInteger(absent) || Number.isNaN(absent))) {
      throw new Error(`Invalid numeric entry rule in ${name}`);
    }
    const read = Object.entries(booleanFrom).reduceRight((otherwise, [key, value]) =>
      `key === ${JSON.stringify(key)} ? settings[key] === ${JSON.stringify(value)} : ${otherwise}`, 'settings[key]');
    return `const ${name}Keys = ${JSON.stringify(keys)};

export function ${name}(settings) {
  if (!${name}Keys.some(key => Object.hasOwn(settings, key))) return undefined;
  return ${array}.from(${name}Keys, key =>
    Object.hasOwn(settings, key) ? Number(${read}) : ${String(absent)});
}`;
  }
  if (contract.stringEntries) {
    const { keys, aliases, inferredFlag } = contract.stringEntries;
    const { key: flag, from } = inferredFlag;
    if ([flag, from, ...Object.keys(aliases)].some(key => !keys.includes(key)) || flag === from) {
      throw new Error(`Invalid string entry rule in ${name}`);
    }
    return `const ${name}Keys = ${JSON.stringify(keys)};
const ${name}Aliases = ${JSON.stringify(aliases, null, 2)};

export function ${name}(settings) {
  const entries = [];
  for (const key of ${name}Keys) {
    if (!Object.hasOwn(settings, key)) continue;
    const value = settings[key];
    const wireValue = ${name}Aliases[key]?.[value];
    entries.push(key, wireValue ?? (typeof value === 'boolean' ? String(Number(value)) : String(value)));
  }
  if (Object.hasOwn(settings, ${JSON.stringify(from)}) && !Object.hasOwn(settings, ${JSON.stringify(flag)})) {
    entries.push(${JSON.stringify(flag)}, settings[${JSON.stringify(from)}] ? '1' : '0');
  }
  return entries;
}`;
  }
  if (contract.constant !== undefined) {
    return `export const ${name} = Object.freeze(${JSON.stringify(contract.constant, null, 2)});`;
  }
  if (contract.testArgument) {
    const { check: predicate, choices: limits } = contract.testArgument;
    validateRules(name, ['value'], [['value', predicate, undefined, limits]]);
    const constants = Object.entries(contract.exportConstants ?? {})
      .map(([key, value]) => `export const ${key} = ${JSON.stringify(value, null, 2)};

`).join('');
    return `${constants}export function ${name}(value) {
  return !(${predicates[predicate]('value', limits)});
}`;
  }
  if (contract.scalarArgument) {
    const { check: predicate, message, choices: limits, default: fallback, code = 'INVALID_VALUE' } = contract.scalarArgument;
    validateRules(name, ['value'], [['value', predicate, message, limits]]);
    if (fallback !== undefined && predicate === 'oneOf' && !limits.includes(fallback)) {
      throw new Error(`Invalid scalar argument default for ${name}`);
    }
    const parameter = fallback === undefined ? 'value' : 'value = ' + JSON.stringify(fallback);
    return `export function ${name}(${parameter}) {
  if (${predicates[predicate]('value', limits)}) {
    throw failure(${JSON.stringify(code)}, ${JSON.stringify(message)});
  }
  return value;
}`;
  }
  if (contract.enumArgument) {
    const { values, append, message } = contract.enumArgument;
    if (append.some(key => !Object.hasOwn(values, key))) throw new Error(`Invalid append option for ${name}`);
    const keys = Object.keys(values);
    return `const ${name}Values = ${JSON.stringify(values, null, 2)};

export function ${name}(value, inserting) {
  const accepted = inserting ? ${JSON.stringify(keys)} : ${JSON.stringify(append)};
  if (!accepted.includes(value)) throw failure('UNSUPPORTED_OPTIONS', ${JSON.stringify(message)});
  return ${name}Values[value];
}`;
  }
  if (contract.indexedPatch) {
    const { keys, exportKeys, exportPropertyDefaults, predicate, message, code } = contract.indexedPatch;
    if (!Object.hasOwn(predicates, predicate)) throw new Error(`Invalid indexed patch predicate for ${name}`);
    return `${generatePropertyDefaults(name, keys, exportPropertyDefaults)}export const ${exportKeys} = Object.freeze(${JSON.stringify(keys)});

export function ${name}(options, previous) {
  const next = previous.slice();
  for (const key of Reflect.ownKeys(options)) {
    const { value } = Object.getOwnPropertyDescriptor(options, key);
    const index = ${exportKeys}.indexOf(key);
    if (index === -1 || ${predicates[predicate]('value')}) {
      throw failure(${JSON.stringify(code)}, ${JSON.stringify(message)});
    }
    next[index] = value;
  }
  return next.some((value, index) => value !== previous[index]) ? next : undefined;
}`;
  }
  if (contract.properties || contract.propertyGroups) {
    const { propertyGroups = {}, defaults, defaultFrom, truthyDefaults = {}, nullishDefaults = {}, ownBooleanDefaults = {}, ownKeysError, ownKeysCode = 'UNSUPPORTED_OPTIONS', ownDataProperties, exportDefaults, exportProperties, orderedOwnDataProperties, shapeOnly, rejectArrays, exactlyOne, valueChecks = [] } = contract;
    const properties = [...(contract.properties ?? []), ...Object.values(propertyGroups).flat()];
    const propertyDefaultExports = generatePropertyDefaults(name, properties, contract.exportPropertyDefaults);
    validateRules(name, properties, valueChecks);
    const conditions = when => Array.isArray(when) ? when : [when];
    for (const [key, , , , , when] of valueChecks) {
      if (when === undefined) continue;
      const rules = conditions(when);
      const invalidCondition = rule =>
        !properties.includes(rule?.key) ||
        (ownDataProperties && properties.indexOf(rule.key) >= properties.indexOf(key)) ||
        (Object.hasOwn(rule, 'defined')
          ? typeof rule.defined !== 'boolean' || Object.hasOwn(rule, 'equals')
          : !Object.hasOwn(rule, 'equals') || !['string', 'boolean'].includes(typeof rule.equals));
      if (orderedOwnDataProperties || shapeOnly || !rules.length || rules.some(invalidCondition)) {
        throw new Error(`Invalid validation condition for ${name}.${key}`);
      }
    }
    for (const key of [...Object.keys(truthyDefaults), ...Object.keys(nullishDefaults)]) {
      if (!properties.includes(key) || Object.hasOwn(defaults ?? {}, key) || Object.hasOwn(defaultFrom ?? {}, key) ||
          (Object.hasOwn(truthyDefaults, key) && Object.hasOwn(nullishDefaults, key))) {
        throw new Error(`Invalid fallback for ${name}.${key}`);
      }
    }
    for (const [key, value] of Object.entries(ownBooleanDefaults)) {
      if (typeof value !== 'boolean' || properties.includes(key)) {
        throw new Error(`Invalid own boolean fallback for ${name}.${key}`);
      }
    }
    for (const [key, source] of Object.entries(defaultFrom ?? {})) {
      if (properties.indexOf(source) < 0 || properties.indexOf(source) >= properties.indexOf(key)) {
        throw new Error(`Invalid dependent default for ${name}.${key}`);
      }
    }
    const bindings = properties.map(key => {
      const from = defaultFrom?.[key];
      if (from !== undefined) return `${key} = ${from}`;
      return Object.hasOwn(defaults ?? {}, key) ? `${key} = ${JSON.stringify(defaults[key])}` : key;
    });
    const fields = properties.map(key => {
      if (Object.hasOwn(truthyDefaults, key)) return `${key}: ${key} || ${JSON.stringify(truthyDefaults[key])}`;
      if (Object.hasOwn(nullishDefaults, key)) return `${key}: ${key} ?? ${JSON.stringify(nullishDefaults[key])}`;
      return key;
    });
    for (const [key, value] of Object.entries(ownBooleanDefaults)) {
      const literal = JSON.stringify(key);
      fields.push(`${key}: options != null && Object.hasOwn(options, ${literal}) ? Boolean(options[${literal}]) : ${value}`);
    }
    const groupExports = Object.entries(propertyGroups)
      .map(([key, values]) => `export const ${key} = Object.freeze(${JSON.stringify(values)});\n`).join('') + propertyDefaultExports;
    const propertyExport = exportProperties
      ? `export const ${exportProperties} = Object.freeze(${JSON.stringify(properties)});\n\n` : '';
    const rejectAccessors = contract.rejectAccessors;
    if (rejectAccessors && (!ownDataProperties || !ownKeysError)) {
      throw new Error(`Accessor rejection requires own data properties and a shape error for ${name}`);
    }
    if (Array.isArray(rejectAccessors) && rejectAccessors.some(key => !properties.includes(key))) {
      throw new Error(`Unknown accessor rejection field in ${name}`);
    }
    const accessor = "!Object.hasOwn(Object.getOwnPropertyDescriptor(options, key) ?? {}, 'value')";
    const invalidKey = rejectAccessors === true ? `!keys.includes(key) || ${accessor}`
      : Array.isArray(rejectAccessors) ? `!keys.includes(key) || (${JSON.stringify(rejectAccessors)}.includes(key) && ${accessor})`
      : '!keys.includes(key)';
    const shapeCheck = ownKeysError === undefined ? '' : `  const keys = ${exportProperties ?? JSON.stringify(properties)};
  if (!options || typeof options !== 'object' || ${shapeOnly ? '(rejectArrays && Array.isArray(options)) || ' : ''}${(rejectArrays || ownDataProperties || orderedOwnDataProperties) ? 'Array.isArray(options) || ' : ''}Reflect.ownKeys(options).some(key => ${invalidKey})) {
    throw failure(${JSON.stringify(ownKeysCode)}, ${JSON.stringify(ownKeysError)});
  }
`;
    if (shapeOnly) {
      if (!ownKeysError) throw new Error(`Missing shape check for ${name}`);
      return `${groupExports}${propertyExport}export function ${name}(options, rejectArrays = false) {
${shapeCheck.trimEnd()}
}`;
    }
    const valueCheck = ([key, predicate, message, choices, code = 'UNSUPPORTED_OPTIONS', when]) => {
      const invalid = predicates[predicate](key, choices);
      const condition = when ? [...conditions(when).map(rule =>
        Object.hasOwn(rule, 'defined')
          ? `${rule.key} ${rule.defined ? '!==' : '==='} undefined`
          : `${rule.key} === ${JSON.stringify(rule.equals)}`), `(${invalid})`].join(' && ') : invalid;
      return `  if (${condition}) throw failure(${JSON.stringify(code)}, ${JSON.stringify(message)});`;
    };
    let exclusivityCheck = '';
    if (exactlyOne) {
      const { keys, presence, message } = exactlyOne;
      if (ownDataProperties || orderedOwnDataProperties || shapeOnly || keys?.length !== 2 ||
          keys[0] === keys[1] || keys.some(key => !properties.includes(key)) ||
          !['defined', 'truthy'].includes(presence) || typeof message !== 'string') {
        throw new Error(`Invalid exclusive fields for ${name}`);
      }
      const [first, second] = keys.map(key => presence === 'defined' ? `(${key} !== undefined)` : `Boolean(${key})`);
      exclusivityCheck = `
  if (${first} === ${second}) throw failure('INVALID_VALUE', ${JSON.stringify(message)});`;
    }
    if (orderedOwnDataProperties) {
      if (!ownKeysError || ownDataProperties || defaults) {
        throw new Error(`Ordered own data properties require a shape check and no defaults in ${name}`);
      }
      const checks = valueChecks.map(([key, predicate, message, choices, code = 'UNSUPPORTED_OPTIONS']) =>
        `    if (key === ${JSON.stringify(key)} && (${predicates[predicate]('value', choices)})) {
      throw failure(${JSON.stringify(code)}, ${JSON.stringify(message)});
    }`);
      let creationDefault = '';
      if (contract.creationFlag) {
        const { key, anyTruthy, unless } = contract.creationFlag;
        if (!anyTruthy.length || [key, unless, ...anyTruthy].some(field => !properties.includes(field))) {
          throw new Error(`Invalid creation flag in ${name}`);
        }
        const field = key => `settings[${JSON.stringify(key)}]`;
        creationDefault = `  if (creating && ${field(key)} === undefined &&
      (${anyTruthy.map(field).join(' || ')}) && !${field(unless)}) {
    ${field(key)} = true;
  }
`;
      }
      return `${groupExports}export function ${name}(options, project${creationDefault ? ', creating = false' : ''}) {
${shapeCheck}  const settings = {};
  for (const key of Reflect.ownKeys(options)) {
    const value = Object.getOwnPropertyDescriptor(options, key).value;
    if (value === undefined) continue;
${checks.join('\n')}
    settings[key] = project(key, value);
  }
${creationDefault}  return settings;
}`;
    }
    const read = ownDataProperties ? properties.flatMap((key, index) => {
      const descriptor = `Object.getOwnPropertyDescriptor(${ownKeysError === undefined ? "options ?? {}" : "options"}, ${JSON.stringify(key)})`;
      return [
        `  const { value: ${bindings[index]} } = ${descriptor} ?? { value: undefined };`,
        ...valueChecks.filter(rule => rule[0] === key).map(valueCheck),
      ];
    }).join('\n') : `  const {
    ${bindings.join(',\n    ')},
  } = ${ownKeysError === undefined ? 'options ?? {}' : 'options'};
${valueChecks.map(rule => valueCheck(rule) + '\n').join('')}`.trimEnd();
    const indexedDefaults = contract.indexedDefaults;
    if (indexedDefaults && !Object.hasOwn(contract.exportPropertyDefaults ?? {}, indexedDefaults)) {
      throw new Error(`Unknown indexed defaults in ${name}`);
    }
    const maskedStrings = contract.maskedStrings;
    if (maskedStrings && (!ownDataProperties || !properties.length || properties.length > 31 ||
        defaults || defaultFrom || Object.keys(truthyDefaults).length || Object.keys(nullishDefaults).length)) {
      throw new Error(`Invalid masked string fields in ${name}`);
    }
    const result = maskedStrings
      ? `{
    values: [${properties.map(key => `String(${key} ?? '')`).join(', ')}],
    changed: ${properties.map((key, index) => `(${key} === undefined ? 0 : ${1 << index})`).join(' | ')},
  }`
      : `{ ${fields.join(', ')} }`;
    const defaultExport = exportDefaults ? `export const ${exportDefaults} = Object.freeze(${JSON.stringify(defaults)});\n\n` : '';
    const body = indexedDefaults
      ? `  const next = Float64Array.of(
${properties.map((key, index) =>
        `    ${key} ?? previous?.[${index}] ?? ${indexedDefaults}[${JSON.stringify(key)}],`).join('\n')}
  );
  return previous && next.every((value, index) => value === previous[index]) ? undefined : next;`
      : `  return ${result};`;
    return `${groupExports}${defaultExport}${propertyExport}export function ${name}(options${indexedDefaults ? ', previous' : ''}) {
${shapeCheck}${read}${exclusivityCheck}
${body}
}`;
  }
  const { keys, seed, partial, configurableOmission, variants, defaults = {}, beforeMatch: beforeMatchRules,
    afterMatch: afterMatchRules, deferredCheck, projections = [], checks = {} } = contract;
  const beforeMatch = matchRules(beforeMatchRules);
  const afterMatch = matchRules(afterMatchRules);
  if (deferredCheck && (!afterMatch.length || Object.keys(defaults).length)) {
    throw new Error(`Deferred checks require validation rules without defaults in ${name}`);
  }
  const variantKeys = variants && expandVariantKeys(keys, variants);
  const availableKeys = variantKeys ? Object.values(variantKeys).flat() : keys;
  if (variants && beforeMatch.length) throw new Error(`Variant rules must validate after matching in ${name}`);
  validateRules(name, availableKeys, [...beforeMatch, ...afterMatch]);
  const check = ([key, predicate, message, choices], indent = '  ') => `${indent}if (${predicates[predicate](`settings.${key}`, choices)}) {
${indent}  throw failure('UNSUPPORTED_OPTIONS', ${JSON.stringify(message)});
${indent}}`;
  const steps = beforeMatch.length ? keys.map(key => {
    const rule = beforeMatch.find(rule => rule[0] === key);
    return `  if (options && Object.hasOwn(options, ${JSON.stringify(key)})) {
    settings.${key} = data(options, ${JSON.stringify(key)});${rule ? '\n' + check(rule, '    ') : ''}
  }`;
  }) : [`  collect(options, ${variants ? name + 'Keys[variant]' : JSON.stringify(keys)}, settings);`];
  return [
    ...(variants ? [`const ${name}Keys = ${JSON.stringify(variantKeys, null, 2)};`, ''] : []),
    `export function ${name}(options${variants ? ', variant' : ''}${seed ? ', settings' : ''}${configurableOmission ? ', allowOmission = true' : ''}) {`,
    ...(!seed ? ['  const settings = {};'] : []),
    ...steps,
    ...(partial ? [] : [`  optionsMatch(options, settings${configurableOmission ? ', allowOmission' : ''});`]),
    ...(deferredCheck ? [] : afterMatch.map(rule => check(rule))),
    ...Object.entries(defaults).map(([key, value]) => {
      if (!availableKeys.includes(key)) throw new Error(`Invalid default for ${name}.${key}`);
      return `  if (settings.${key} === undefined) settings.${key} = ${JSON.stringify(value)};`;
    }),
    '  return settings;',
    '}',
    ...(deferredCheck ? [
      '',
      `export function ${deferredCheck}(settings) {`,
      ...afterMatch.map(rule => check(rule)),
      '}',
    ] : []),
    ...Object.entries(checks).flatMap(([checkName, declarations]) => {
      const rules = matchRules(declarations);
      validateRules(checkName, availableKeys, rules);
      return ['', `export function ${checkName}(settings) {`, ...rules.map(rule => check(rule)), '}'];
    }),
    ...projections.flatMap(projection => generateProjection(projection, availableKeys)),
  ].join('\n');
}).join('\n\n')}
`;

}
function generateFileOutputFormats({ defaultFormat, extensions, writeAliases, asyncWriteAliases, textMimeTypes, xmlWorkbookMimeTypes, documentMimeTypes,
  earlyBiffVersions, compoundBiffVersions, binaryMimeTypes }) {
  return `
export const textOutputMime = new Map(${JSON.stringify(Object.entries(textMimeTypes))});

export const xmlWorkbookMime = new Map(${JSON.stringify(Object.entries(xmlWorkbookMimeTypes))});

export const documentOutputMime = Object.freeze(${JSON.stringify(documentMimeTypes, null, 2)});

export const earlyBiffVersions = new Map(${JSON.stringify(Object.entries(earlyBiffVersions))});
export const compoundBiffVersions = new Map(${JSON.stringify(Object.entries(compoundBiffVersions))});
export const binaryOutputMime = new Map(${JSON.stringify(Object.entries(binaryMimeTypes))});

const fileOutputExtensions = new Map(${JSON.stringify(Object.entries(extensions))});
const writeAliases = new Map(${JSON.stringify(Object.entries(writeAliases))});
const asyncWriteAliases = new Map(${JSON.stringify(Object.entries(asyncWriteAliases))});

export function resolveWriteFormat(format) {
  return writeAliases.get(format) ?? format;
}

export function resolveAsyncWriteFormat(format) {
  return asyncWriteAliases.get(format) ?? format;
}

export function inferFileOutputFormat(filename) {
  const extension = typeof filename === 'string' ? /\\.([a-z]+)$/i.exec(filename)?.[1].toLowerCase() : undefined;
  return fileOutputExtensions.get(extension) ?? extension ?? ${JSON.stringify(defaultFormat)};
}
`;
}

function generateWriteOutputTypes(variants) {
  return `
const writeOutputTypes = ${JSON.stringify(variants, null, 2)};

export function validateWriteOutputType(type, variant, format) {
  const { types, label } = writeOutputTypes[variant];
  if (!types.includes(type)) {
    throw failure('UNSUPPORTED_OPTIONS', 'Unsupported ' + (label ?? format.toUpperCase()) + ' output type');
  }
}
`;
}

function generateSheetOptionReference(heading, options, validationOrder) {
  const validation = {
    boolean: 'Boolean',
    optionalBoolean: 'Optional boolean',
    optionalString: 'Optional string',
    optionalOneOf: 'Optional value',
    optionalSheetSelectors: 'Optional sheet name, zero-based index, or array of either',
    optionalNonnegativeInteger: 'Optional nonnegative integer',
  };
  return [
    heading,
    '',
    '| Option | Validation | Default | Related behavior |',
    '| --- | --- | --- | --- |',
    ...Object.entries(options).map(([key, option]) => {
      const { workbookResource, projection, formats, referenceNote } = option;
      const check = typeof option.check === 'string' ? option : option.check;
      const choices = check?.choices ? ' (' + check.choices.map(value => JSON.stringify(value)).join(', ') + ')' : '';
      const defaultValue = projection?.trueWhen === 'notExplicitlyFalse' ? true : option.default;
      const fallback = defaultValue === undefined ? '—' : `\`${JSON.stringify(defaultValue)}\``;
      const behavior = [];
      if (formats) behavior.push('Accepted for ' + formats.map(format => `\`${format}\``).join(', '));
      if (projection?.rustField) behavior.push(`Projects to \`${projection.rustField}\``);
      if (projection?.as === 'xlsbRowLimit') {
        behavior.push('falsy values mean no limit. Truthy values are converted to a number, rounded up and clamped to zero or above. A rounded value above 1048576 or NaN means no limit');
      }
      if (projection?.trueWhen === 'notExplicitlyFalse') {
        behavior.push('enabled for every value except literal `false`, including `undefined` and `null`');
      }
      if (projection?.trueWhen === 'truthy') behavior.push('enabled for truthy values');
      if (projection?.alsoEnabledBy) behavior.push(`Also enabled by a truthy \`${projection.alsoEnabledBy}\``);
      if (workbookResource) {
        const { output, disabledBy } = workbookResource;
        behavior.push(`Includes \`${output}\` when enabled, except when \`${disabledBy}\` is enabled`);
      }
      if (referenceNote) behavior.push(referenceNote);
      return `| \`${key}\` | ${validation[check?.check] ?? check?.message ?? '—'}${choices} | ${fallback} | ${behavior.join(', ') || '—'} |`;
    }),
    ...(validationOrder ? [
      '',
      'When multiple option values are invalid, the first failed check in this order is reported:',
      validationOrder.map(key => '`' + key + '`').join(' → ') + '.',
    ] : []),
  ].join('\n');
}

function generateReadInputReference(declaration) {
  const { keys, beforeMatch } = compileFieldContract('readInputSettings', declaration);
  const options = Object.fromEntries(keys.map(key => [key, { check: beforeMatch[key] }]));
  return [
    generateSheetOptionReference('## SheetJS `read(data, options)` input decoding', options),
    '',
    '`readFile(filename, options)` and `readFileSync` take the path as a positional argument and infer the input type from the file bytes.',
    'The format tables below apply after decoding and format detection.',
  ].join('\n');
}

function generateTextReadReferences(declaration) {
  const { keys, variants, afterMatch, checks } = compileFieldContract('textReadSettings', declaration);
  const keysByVariant = expandVariantKeys(keys, variants);
  const formats = [
    ['plain text', 'plain'],
    ['DIF', 'plain', 'checkDifRowLimit'],
    ['SYLK', 'plain', 'checkSylkRowLimit'],
    ['EtherCalc', 'plain', 'checkEthRowLimit'],
    ['RTF', 'plain', 'checkRtfRowLimit'],
    ['SpreadsheetML', 'spreadsheetMl', 'checkSpreadsheetMlProjection'],
    ['FODS/ODS', 'openDocument', 'checkOpenDocumentSelectors', 'checkOpenDocumentRowLimit'],
  ];
  return formats.map(([format, variant, ...checkNames]) => {
    const validation = Object.assign({}, afterMatch, ...checkNames.map(name => checks[name]));
    const options = Object.fromEntries(keysByVariant[variant].map(key => [key, { check: validation[key] }]));
    return generateSheetOptionReference('## SheetJS `read(data, options)` for ' + format, options);
  });
}

function generateWriteReference(contracts) {
  const { keys, variants, afterMatch } = compileFieldContract('textWriteSettings', contracts.textWriteSettings);
  const variantKeys = expandVariantKeys(keys, variants);
  const workbookFormats = [
    ['xlsx / xlsm / xlam', 'xlsx', contracts.xlsxWriteSettings],
    ['xlsb', 'xlsb', contracts.xlsbWriteSettings],
    ['ods', 'ods', contracts.odsWriteSettings],
    ['fods', 'fods', contracts.fodsWriteSettings],
    ['xlml / xml', 'xlml', contracts.xlmlWriteSettings],
  ].map(([format, type, declaration]) => [format, type, compileFieldContract(type, declaration)]);
  const formats = [
    ...workbookFormats.map(([format, type, contract]) => [
      format, type, ['bookType', 'type', ...contract.keys], ['bookType', ...contract.keys],
    ]),
    ...Object.keys(fileOutputFormats.textMimeTypes).map(format => [format, 'text', variantKeys[format], variantKeys[format + ':file']]),
    ...Object.keys(fileOutputFormats.binaryMimeTypes).map(format => [format, 'binarySheet', variantKeys[format], variantKeys[format + ':file']]),
  ];
  const codeList = values => values.map(value => `\`${value}\``).join(', ');
  const validationTables = workbookFormats
    .filter(([, , contract]) => Object.keys({ ...contract.beforeMatch, ...contract.afterMatch }).length)
    .map(([format, , contract]) => {
      const checks = { ...contract.beforeMatch, ...contract.afterMatch };
      const options = Object.fromEntries(contract.keys.map(key => [key, { check: checks[key] }]));
      return generateSheetOptionReference('### Write option validation for ' + format, options);
    });
  const variantFormats = Object.keys(variants).filter(format => !format.endsWith(':file'));
  const variantValidation = Object.fromEntries(Object.entries(afterMatch).map(([key, check]) => [
    key, { check, formats: variantFormats.filter(format => variantKeys[format].includes(key)) },
  ]));
  validationTables.push(generateSheetOptionReference('### Write option validation for text and legacy formats', variantValidation));
  return [
    '## SheetJS `write(workbook, options)`',
    '',
    'Passing `type: "file"` requires a `file` filename and applies the file options below. This overload uses XLSX when `bookType` is omitted.',
    '',
    '### `writeFile(workbook, filename, options)` and `writeFileSync`',
    '',
    'These APIs infer the workbook format from the filename when `bookType` is omitted. `writeFileXLSX` uses the XLSX file option keys and always writes XLSX.',
    '',
    '### `writeFileAsync(filename, workbook, options, callback)`',
    '',
    'Requires a filesystem provider and a callback. Pass the callback as the third argument to omit options. This API infers the format from the filename when `bookType` is omitted, then serializes using the memory options with `type: "buffer"` supplied by the adapter. Pass the filename and callback as positional arguments.',
    '',
    '| Workbook format | Memory output types | Memory option keys | File option keys |',
    '| --- | --- | --- | --- |',
    ...formats.map(([format, variant, memoryKeys, fileKeys]) =>
      `| ${format} | ${codeList(writeOutputTypes[variant].types)} | ${codeList(memoryKeys)} | ${codeList(fileKeys)} |`),
    '',
    'The validation tables describe each declared check. A Boolean check runs when the key is supplied.',
    '',
    ...validationTables.flatMap(table => [table, '']),
  ].join('\n');
}

function generateSheetUtilityReference(contracts) {
  const entries = {
    sheetArrayInputSettings: 'utils.aoa_to_sheet / utils.sheet_add_aoa',
    sheetJsonInputSettings: 'utils.json_to_sheet / utils.sheet_add_json',
    sheetDomSettings: 'utils.table_to_sheet / utils.table_to_book / utils.sheet_add_dom',
    sheetHtmlSettings: 'utils.sheet_to_html / stream.to_html',
    sheetNewSettings: 'utils.sheet_new',
    sheetFormulaSettings: 'utils.sheet_to_formulae',
    sheetJsonSettings: 'utils.sheet_to_json / utils.sheet_to_row_object_array / stream.to_json',
    sheetCsvSettings: 'utils.sheet_to_csv / stream.to_csv',
    xlmlStreamSettings: 'stream.to_xlml',
    xlsxCellHyperlinkSettings: 'XLSX cell.l',
  };
  return [
    generatePublicOptionsReference('SheetJS worksheet utilities and streams', contracts, entries),
    '',
    'When editing an existing XLSX link, omitting `display` preserves its label. An empty string saves an empty label, and an own `display: undefined` removes it. A `display` getter or setter raises `INVALID_VALUE` without being called.',
  ].join('\n');
}

function generateExcelPublicReference(contracts) {
  return generatePublicOptionsReference('ExcelJS workbook, worksheet and streaming options', contracts, {
    workbookLoadSettings: 'workbook.xlsx.load / workbook.xlsx.read / workbook.xlsx.readFile',
    workbookOutputSettings: 'workbook.xlsx.writeBuffer / workbook.xlsx.write / workbook.xlsx.writeFile',
    zipOutputSettings: 'workbook.xlsx.write options.zip',
    zipLevelSettings: 'workbook.xlsx.write options.zip.compressionOptions',
    formulaRectangleSettings: 'worksheet.fillFormula range rectangle',
    worksheetSettings: 'workbook.addWorksheet / WorkbookWriter.addWorksheet',
    worksheetPropertySettings: 'workbook.addWorksheet options.properties / WorkbookWriter.addWorksheet options.properties',
    checkedWorksheetCreationState: 'workbook.addWorksheet options.state / WorkbookWriter.addWorksheet options.state',
    worksheetViewSettings: 'worksheet.views entries',
    pageSetupSettings: 'worksheet.pageSetup / workbook.addWorksheet options.pageSetup',
    headerFooterSettings: 'worksheet.headerFooter / workbook.addWorksheet options.headerFooter',
    sheetProtectionSettings: 'worksheet.protect options',
    tableSettings: 'worksheet.addTable',
    tableStyleSettings: 'worksheet.addTable options.style',
    tableColumnSettings: 'worksheet.addTable options.columns entries',
    csvReadSettings: 'workbook.csv.read options',
    csvReadDestinationSettings: 'workbook.csv.read destination',
    csvReadStreamSettings: 'workbook.csv.read parser configuration',
    csvParserSettings: 'workbook.csv.read options.parserOptions (browser)',
    csvWorksheetSettings: 'workbook.csv.write worksheet selection and formatting',
    csvWriteStreamSettings: 'workbook.csv.write formatter configuration',
    csvFormatterSettings: 'workbook.csv.write options.formatterOptions (browser)',
    csvWriteFileSettings: 'workbook.csv.writeFile options (Node)',
    streamingWorkbookReaderSettings: 'stream.xlsx.WorkbookReader',
    streamingWorkbookWriterSettings: 'stream.xlsx.WorkbookWriter',
    streamingZipSettings: 'stream.xlsx.WorkbookWriter options.zip',
    streamingZlibSettings: 'stream.xlsx.WorkbookWriter options.zip.zlib',
  });
}

function generateStreamingReaderReference({ fields }) {
  return [
    '## ExcelJS `stream.xlsx.WorkbookReader` modes',
    '',
    'These options use own data properties. Omitted and undefined values use the default. Inherited values and accessors are ignored.',
    '',
    '| Option | Mode | Default | Behavior |',
    '| --- | --- | --- | --- |',
    ...Object.entries(fields).flatMap(([key, { choices, default: defaultValue }]) =>
      choices.map(mode => `| \`${key}\` | \`${mode}\` | ${mode === defaultValue ? 'Yes' : ''} | ${streamingReaderModes[key][mode]} |`)),
    '',
    'After the workbook entry, worksheet listeners can inspect `reader.model.sheets` and `reader.properties.model.date1904`.',
    'Option validation runs before the input is opened or its stream is consumed. Once validation succeeds, a new parse clears the previous metadata.',
    'If the workbook cannot be opened, `read()` emits an `error` event and rejects when there is no error listener. `parse()` and async iteration reject directly.',
    'Cached and emitted rich text include font names and sizes only when declared in the workbook.',
  ].join('\n');
}

function generateStreamingWriterReference(contracts) {
  const commentLimit = contracts.checkedStreamingZipCommentLength.scalarArgument.choices;
  return [
    '## ExcelJS `stream.xlsx.WorkbookWriter` output',
    '',
    '| Setting | Behavior |',
    '| --- | --- |',
    '| `stream`, `filename` | Write to the supplied Node Writable, then fall back to a filename, then to a buffered stream if both are omitted. |',
    '| `useSharedStrings` | `addWorksheet(name, options)` inherits the workbook setting when the option is omitted or undefined. An own boolean overrides it. Accessors are rejected. |',
    '| `tabColor` | The legacy top-level setting supplies `addWorksheet` tab color unless `properties.tabColor` is specified. |',
    '| `created`, `modified` | Use the current timestamp when `created` is omitted. Use the creation timestamp when `modified` is omitted. |',
    `| \`zip.comment\` | A well-formed Unicode string of up to ${commentLimit} UTF-8 bytes. |`,
    '| `zip.store`, `zip.level`, `zip.zlib.level` | `store: true` disables compression. A nonnegative top-level level takes precedence over zlib.level. Omission or -1 falls back to zlib.level, then to the default DEFLATE level. Level 0 disables compression here. |',
  ].join('\n');
}

function generateExcelFormulaReference(contracts) {
  const { choices, default: fallback } = contracts.checkedFillFormulaMode.scalarArgument;
  const formula = contracts.readCellFormulaResult.variantOwnKeys;
  const shared = contracts.readSharedFormulaResult.variantOwnKeys;
  const keys = (contract, variant) =>
    [...contract.common, ...contract.variants[variant]].map(key => '`' + key + '`').join(', ');
  return [
    '## ExcelJS `worksheet.fillFormula(range, formula, results, shareType)`',
    '',
    '| Argument | Accepted values |',
    '| --- | --- |',
    '| `range` | Two A1 endpoints (for example `B2:B2`), or a rectangle with `top`, `left`, `bottom`, and `right` |',
    '| `formula` | Nonblank formula string |',
    '| `results` | Optional flat array, two-dimensional array, or callback receiving one-based row and column |',
    '| `shareType` | ' + choices.map(mode => '`' + mode + '`').join(', ') + ' (default `' + fallback + '`) |',
    '',
    '## ExcelJS formula objects in `Cell.value`',
    '',
    '| Formula object | Option keys |',
    '| --- | --- |',
    '| Ordinary formula | ' + keys(formula, 'ordinary') + ' |',
    '| Shared formula master or array formula | ' + keys(formula, 'range') + ' |',
    '| Shared formula member | ' + keys(shared, 'member') + ' |',
    '',
    'A range formula uses `shareType: "array"` or `"shared"` and an A1 `ref`. A one-cell range can use a single A1 address in `ref`. A shared member names its master in `sharedFormula`. The cached `result` is optional.',
  ].join('\n');
}

function generatePublicOptionsReference(title, contracts, entries) {
  return [
    '## ' + title,
    '',
    '| Public API | DSL options, validation and defaults |',
    '| --- | --- |',
    ...Object.entries(entries).map(([helper, names]) => {
      const { keys = [], fields, scalarArgument, defaults = {}, afterMatch = {}, ownBooleanDefaults = {}, nullishDefaults = {}, projections = [] } = contracts[helper];
      const options = fields ?? (scalarArgument ? { value: scalarArgument } :
        Object.fromEntries(keys.map(key => [key, afterMatch[key] ?? {}])));
      const optionKeys = new Set([...Object.keys(options), ...Object.keys(ownBooleanDefaults), ...Object.keys(nullishDefaults)]);
      const descriptions = [...optionKeys].map(key => {
        const field = options[key] ?? {};
        const details = [];
        if (field.type) {
          const type = {
            unicodeString: 'Unicode string',
            nonblankUnicodeString: 'nonblank Unicode string',
            nonemptyUnicodeString: 'nonempty Unicode string',
          }[field.type] ?? field.type;
          details.push(field.presence === 'optional' ? `optional ${type}` : `required ${type}`);
        } else {
          const validation = {
            optionalBoolean: 'optional boolean',
            optionalString: 'optional string',
            optionalUnicodeString: 'optional Unicode string',
            optionalIntegerRange: 'optional integer',
            optionalNonnegativeInteger: 'optional nonnegative integer',
            optionalNumberRange: 'optional number',
            optionalOneOf: 'optional',
            optionalZipCompression: 'optional string',
          }[field.check];
          if (validation) details.push(validation);
        }
        const range = field.range ?? (field.check?.endsWith('Range') ? field.choices : undefined);
        if (range) details.push('range ' + range.join(' to '));
        else if (field.choices) details.push('choices ' + JSON.stringify(field.choices));
        else if (field.check === 'optionalZipCompression') {
          details.push('choices ' + JSON.stringify(zipCompressionMethods) + ' (case-insensitive)');
        }
        if (field.when) {
          const conditions = Array.isArray(field.when) ? field.when : [field.when];
          const rules = conditions.map(({ key, equals, defined }) => defined === undefined
            ? key + ' = ' + JSON.stringify(equals)
            : key + (defined ? ' is defined' : ' is absent or undefined'));
          details.push('checked when ' + rules.join(' and '));
        }
        if (Object.hasOwn(field, 'default') || Object.hasOwn(defaults, key)) {
          const value = Object.hasOwn(field, 'default') ? field.default : defaults[key];
          details.push('default ' + JSON.stringify(value) + ' when omitted or undefined');
        }
        if (Object.hasOwn(ownBooleanDefaults, key)) {
          details.push('boolean default ' + JSON.stringify(ownBooleanDefaults[key]) + ' when omitted');
        }
        if (Object.hasOwn(field, 'defaultIfNullish') || Object.hasOwn(nullishDefaults, key)) {
          const value = Object.hasOwn(field, 'defaultIfNullish') ? field.defaultIfNullish : nullishDefaults[key];
          details.push('default ' + JSON.stringify(value) + ' when nullish');
        }
        if (Object.hasOwn(field, 'defaultIfFalsy')) {
          details.push('default ' + JSON.stringify(field.defaultIfFalsy) + ' when falsy after validation');
        }
        if (projections.some(({ fields }) => Object.values(fields)
          .some(({ from, transform }) => from === key && transform === 'deflateLevel'))) {
          details.push(`-1 and 0 use DEFLATE level ${defaultDeflateLevel}`);
        }
        if (details.length) return `\`${key}\` (${details.join(', ')})`;
        return `\`${key}\``;
      });
      return `| ${names.split(' / ').map(name => `\`${name}\``).join(', ')} | ${descriptions.join(', ')} |`;
    }),
  ].join('\n');
}

function generateReference(adapter, contracts) {
  return [
    `# ${adapter} adapter option contracts`,
    'The API inventory tracks public signatures and compatibility coverage.',
    ...(adapter === 'sheetjs' ? [
      generateReadInputReference(contracts.readInputSettings),
      ...Object.entries(readOptionsByFormat).map(([format, options]) => generateSheetOptionReference(
        '## SheetJS `read(data, options)` for ' + format,
        options,
        format === 'XLSX' ? contracts.xlsxReadSettings.validationOrder : undefined,
      )),
      ...generateTextReadReferences(contracts.textReadSettings),
      generateWriteReference(contracts),
      ...Object.entries(writeOptionsByFormat).map(([format, options]) => generateSheetOptionReference(
        '### Write options without effect for ' + format, options)),
      generateSheetUtilityReference(contracts),
    ] : [
      generateExcelPublicReference(contracts),
      generateStreamingReaderReference(contracts.streamingWorkbookReaderSettings),
      generateStreamingWriterReference(contracts),
      generateExcelFormulaReference(contracts),
    ]),
    ...Object.entries(contracts).map(([name, declaration]) => {
      const { keys, variants, variantOwnKeys, ...rules } = declaration;
      const expandedKeys = variants ? { keysByVariant: expandVariantKeys(keys ?? Object.keys(declaration.fields), variants) } : { keys };
      if (keys === undefined && !variants) delete expandedKeys.keys;
      const ownKeys = variantOwnKeys && {
        ...variantOwnKeys,
        variants: Object.fromEntries(Object.entries(variantOwnKeys.variants)
          .map(([variant, keys]) => [variant, [...variantOwnKeys.common, ...keys]])),
      };
      const contract = {
        ...expandedKeys,
        ...(ownKeys && { variantOwnKeys: ownKeys }),
        ...rules,
      };
      const source = inspect(contract, { depth: null, maxArrayLength: null, maxStringLength: null });
      return [`## \`${name}\``, '\`\`\`js', source, '\`\`\`'].join('\n');
    }),
    '',
  ].join('\n\n');
}

for (const [adapter, contracts] of Object.entries({ sheetjs, exceljs })) {
  const output = new URL(`./src/${adapter}-options.generated.js`, import.meta.url);
  const source = generate(contracts) + (adapter === 'sheetjs' ? generateWriteOutputTypes(writeOutputTypes) + generateFileOutputFormats(fileOutputFormats) : '');
  if (process.argv.includes('--reference')) {
    process.stdout.write(generateReference(adapter, contracts));
  } else if (process.argv.includes('--check')) {
    if (readFileSync(output, 'utf8') !== source) throw new Error(`${adapter} options need regeneration`);
  } else {
    writeFileSync(output, source);
  }
}
