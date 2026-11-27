import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { performance, PerformanceObserver } from 'node:perf_hooks';
import { join, resolve } from 'node:path';
import { setImmediate } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';

async function load(directory) {
  const root = pathToFileURL(resolve(directory) + '/');
  const start = performance.now();
  const api = await import(new URL('src/sheetjs-node.js', root));
  const initMs = performance.now() - start;
  const wasm = await import(new URL('wasm/ooxml_xlsx_wasm.js', root));
  return {
    api, initMs,
    wasmBytes: statSync(new URL('wasm/ooxml_xlsx_wasm_bg.wasm', root)).size,
    capacity: () => wasm.initSync().memory.buffer.byteLength,
  };
}


function summary(values) {
  const quantile = (sorted, q) => sorted[Math.ceil(sorted.length * q) - 1];
  const sorted = values.toSorted((a, b) => a - b);
  // Fixed seed makes these descriptive bootstrap intervals reproducible.
  let seed = 1;
  const boot = Array.from({ length: 1000 }, () => {
    const sample = values.map(() => {
      seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
      return values[Math.floor(seed / 2 ** 32 * values.length)];
    }).sort((a, b) => a - b);
    return quantile(sample, 0.95);
  }).sort((a, b) => a - b);
  return { medianMs: quantile(sorted, 0.5), p95Ms: quantile(sorted, 0.95),
    p95IntervalMs: [quantile(boot, 0.025), quantile(boot, 0.975)] };
}

function compareProjection(actual, expected, name) {
  // Later adapters expose metadata that the original read benchmark did not.
  // Check the added values for these fixtures before comparing the shared shape.
  const { Props, Custprops, Deps, Workbook, ...rest } = actual;
  const comparable = { ...rest, Workbook: { ...Workbook } };
  for (const [key, value, fixtureValue] of [
    ['Props', Props, { Worksheets: actual.SheetNames.length, SheetNames: actual.SheetNames }],
    ['Custprops', Custprops, {}],
    ['Deps', Deps, {}],
  ]) {
    if (Object.hasOwn(expected, key)) comparable[key] = value;
    else if (Object.hasOwn(actual, key)) assert.deepEqual(value, fixtureValue, name + ' ' + key);
  }
  if (!Object.hasOwn(expected.Workbook, 'Names') && Object.hasOwn(Workbook, 'Names')) {
    assert.deepEqual(Workbook.Names, [], name + ' defined names');
    delete comparable.Workbook.Names;
  }
  if (name === 'mixed-sst-cutoff') {
    const cells = actual.Sheets.Data['!data'].map((row, r) => row?.map((cell, c) => {
      if (!cell || !Object.hasOwn(cell, 'r') || Object.hasOwn(expected.Sheets.Data['!data'][r]?.[c] ?? {}, 'r')) {
        return cell;
      }
      const { r: runs, ...plain } = cell;
      assert.equal(plain.t, 's');
      assert.deepEqual(runs, [{ ich: 0, ifnt: 0 }]);
      return plain;
    }));
    comparable.Sheets = Object.assign(Object.create(null), actual.Sheets, {
      Data: { ...actual.Sheets.Data, '!data': cells },
    });
  }
  assert.deepEqual(comparable, expected, name + ' projection differed');
}

const args = process.argv.slice(2);
if (args[0] === '--worker') {
  const [, directory, path, rawOptions] = args;
  const { api, capacity, initMs, wasmBytes } = await load(directory);
  const bytes = readFileSync(path);
  const options = JSON.parse(rawOptions);
  const initialCapacity = capacity();
  const read = () => api.read(bytes, options);
  for (let index = 0; index < 20; index++) read();
  const gc = [];
  const observer = new PerformanceObserver(list => gc.push(...list.getEntries()));
  observer.observe({ entryTypes: ['gc'] });
  const samples = [];
  const starts = [];
  for (let index = 0; index < 200; index++) {
    const start = performance.now();
    read();
    samples.push(performance.now() - start);
    starts.push(start);
  }
  await setImmediate();
  await setImmediate();
  observer.disconnect();
  const gcSamples = samples.filter((duration, index) => gc.some(entry =>
    entry.startTime < starts[index] + duration && entry.startTime + entry.duration > starts[index]));
  const p95 = summary(samples);
  process.stdout.write(JSON.stringify({ ...p95, initMs, wasmBytes, initialCapacity, capacity: capacity(),
    gcSamples: gcSamples.length, gcAtOrAboveP95: gcSamples.filter(duration => duration >= p95.p95Ms).length }));
} else {
  const catalogue = args[0] === '--catalogue';
  const [baseline, spike] = (catalogue ? args.slice(1) : args).map(path => resolve(path));
  const { api } = await load(baseline);
  const candidate = await load(spike);
  const scratch = mkdtempSync(join(tmpdir(), 'adr0022-bench-'));
  function fixture(rows, columns, stride, mixed) {
    const grid = Array.from({ length: rows }, (_, row) =>
      Array.from({ length: columns }, (_, col) => {
        if ((row * columns + col) % stride) return undefined;
        return mixed && col % 3 === 1 ? '売上 ' + (row % 32) : row + col;
      }));
    const sheet = api.utils.aoa_to_sheet(grid);
    if (mixed) {
      for (let row = 0; row < rows; row++) sheet['C' + (row + 1)].f = 'A' + (row + 1) + '+2';
    }
    sheet['!rows'] = [{ hpt: 31, level: 2 }];
    sheet['!cols'] = [{ width: 22.75 }];
    return sheet;
  }
  const cases = catalogue ? [
    { name: 'catalogue-small', rows: 1, columns: 1, options: { bookSheets: true } },
    { name: 'catalogue-names', rows: 1, columns: 1, definedNames: 10000, options: { bookSheets: true } },
    { name: 'catalogue-props', rows: 1, columns: 1, definedNames: 10000, options: { bookProps: true } },
  ] : [
    { name: 'small', rows: 20, columns: 8, stride: 1 },
    { name: 'sparse', rows: 1500, columns: 48, stride: 19 },
    { name: 'dense', rows: 400, columns: 250, stride: 1 },
    { name: 'selected', rows: 400, columns: 250, stride: 1, options: { sheets: 'Small' } },
    { name: 'bookSheets', rows: 400, columns: 250, stride: 1, options: { bookSheets: true } },
    { name: 'mixed-inline', rows: 500, columns: 36, stride: 1, mixed: true },
    { name: 'mixed-sst-cutoff', rows: 500, columns: 36, stride: 1, mixed: true, bookSST: true, options: { sheetRows: 200 } },
  ];
  console.log(JSON.stringify({ node: process.version, warmup: 20, samples: 200 }));
  try {
    const inputs = cases.map(({ name, rows, columns, stride, mixed, bookSST, options, definedNames }) => {
      const sheet = catalogue ? api.utils.aoa_to_sheet([[1]]) : fixture(rows, columns, stride, mixed);
      const book = api.utils.book_new(sheet, 'Data');
      if (!catalogue && (options?.sheets || options?.bookSheets)) {
        api.utils.book_append_sheet(book, fixture(20, 8, 1), 'Small');
      }
      if (definedNames) {
        book.Workbook = { Names: Array.from({ length: definedNames }, (_, index) => ({
          Name: 'BenchmarkName_' + index, Ref: 'Data!$A$1', Comment: 'Defined name for the catalogue benchmark',
        })) };
      }
      const bytes = api.write(book, { bookType: 'xlsb', type: 'buffer', bookSST: Boolean(bookSST) });
      const path = join(scratch, name + '.xlsb');
      writeFileSync(path, bytes);
      const settings = catalogue ? options : { cellStyles: true, cellNF: true, dense: true, ...options };
      if (catalogue) {
        const full = api.read(bytes);
        assert.equal(full.Workbook.Names.length, definedNames ?? 0, name + ' defined name count');
        assert.deepEqual(candidate.api.read(bytes), full, name + ' full read differed');
        assert.deepEqual(candidate.api.read(bytes, settings), api.read(bytes, settings), name + ' metadata differed');
      } else {
        compareProjection(candidate.api.read(bytes, settings), api.read(bytes, settings), name);
      }
      return { name, path, rows, columns, bytes: bytes.length, settings };
    });
    for (let round = 1; round <= 2; round++) {
      for (const { name, path, rows, columns, bytes, settings } of inputs) {
        const results = {};
        for (const key of round === 1 ? ['baseline', 'spike'] : ['spike', 'baseline']) {
          const worker = spawnSync(process.execPath, [import.meta.filename, '--worker',
            key === 'baseline' ? baseline : spike, path, JSON.stringify(settings)], { encoding: 'utf8' });
          if (worker.status !== 0) throw new Error(worker.stderr, { cause: worker.error });
          results[key] = JSON.parse(worker.stdout);
        }
        const { baseline: base, spike: next } = results;
        const pass = next.medianMs <= base.medianMs * 1.15 && next.p95Ms <= base.p95Ms * 1.25 &&
          next.capacity <= base.capacity * 1.25;
        console.log(JSON.stringify({ name, round, rows, columns, bytes, ...results, pass }));
      }
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}
