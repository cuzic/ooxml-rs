import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const directories = process.argv.slice(2);
assert.equal(directories.length, 2, 'Expected unpacked baseline and current package directories');
const builds = await Promise.all(directories.map(async directory => {
  const moduleUrl = file => pathToFileURL(resolve(directory, 'src', file));
  await import(moduleUrl('runtime-node.js'));
  return (await import(moduleUrl('exceljs.js'))).Workbook;
}));

const rowCount = 1200;
const warmupPairs = 4;
const measuredPairs = 20;
const workloads = [
  {
    name: 'date',
    cell: index => new Date(Date.UTC(2024, 0, index + 1, 7, 8, 9)),
    options: { dateUTC: true, dateFormat: 'dddd MMMM DD YYYY HH:mm:ss' },
  },
  {
    name: 'hyperlink',
    cell: index => index % 100 === 0
      ? { text: 'Link ' + index, hyperlink: 'https://example.org/report#row-' + index } : index,
    options: {},
  },
  {
    name: 'mixed',
    cell: index => {
      if (index % 100 === 0) return { richText: [{ text: 'Rich ' + index }] };
      if (index % 100 === 1) return { error: '#DIV/0!' };
      return index;
    },
    options: {},
  },
  {
    name: 'array',
    cell: index => index + 1,
    prepare: sheet => sheet.fillFormula('A1:A2', 'TRANSPOSE(B1:C1)', [7, 8], 'array'),
    options: {},
  },
  {
    name: 'merge',
    cell: index => index + 1,
    prepare: sheet => sheet.mergeCellsWithoutStyle('A1:B2'),
    options: {},
  },
  { name: 'formula', cell: index => ({ formula: 'ROW()', result: index + 1 }), options: {} },
];

for (const { name, cell, prepare, options } of workloads) {
  const source = new builds[1]();
  const sheet = source.addWorksheet('Input');
  sheet.addRows(Array.from({ length: rowCount }, (_, index) => [cell(index)]));
  prepare?.(sheet);
  const bytes = await source.xlsx.writeBuffer();
  const books = await Promise.all(builds.map(async Workbook => new Workbook().xlsx.load(bytes)));
  const timings = [[], []];

  for (let pair = -warmupPairs; pair < measuredPairs; pair++) {
    const outputs = [];
    for (const build of pair % 2 === 0 ? [0, 1] : [1, 0]) {
      const start = performance.now();
      outputs[build] = await books[build].csv.writeBuffer({ sheetName: 'Input', ...options });
      const elapsed = performance.now() - start;
      if (pair >= 0) timings[build].push(elapsed);
    }
    assert.deepEqual(outputs[0], outputs[1], `${name} CSV changed at pair ${pair}`);
  }

  const [baseline, current] = timings.map(series => {
    series.sort((left, right) => left - right);
    const middle = series.length / 2;
    return {
      medianMs: (series[middle - 1] + series[middle]) / 2,
      p95Ms: series[Math.ceil(series.length * 0.95) - 1],
    };
  });
  console.log({ workload: name, rows: rowCount, baseline, current });
}
