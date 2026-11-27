import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const packages = process.argv.slice(2);
assert.equal(packages.length, 2, 'Expected the unpacked baseline and current package directories');
const versions = await Promise.all(packages.map(async directory => {
  const entry = pathToFileURL(resolve(directory, 'src/exceljs-node.js'));
  return (await import(entry)).Workbook;
}));
const [, Workbook] = versions;
const input = new Workbook();
const initialSheet = input.addWorksheet('Merges');
for (let row = 1; row <= 300; row++) initialSheet.mergeCells(row, 1, row, 2);
initialSheet.mergeCells('D1:E1');
const bytes = await input.xlsx.writeBuffer();

for (const [selection, removed] of [['C1:C300', 0], ['B1', 1], ['B1:B300', 300]]) {
  const samples = [[], []];
  for (let sample = -8; sample < 36; sample++) {
    const outputs = [];
    for (const i of sample % 2 ? [1, 0] : [0, 1]) {
      const book = await new versions[i]().xlsx.load(bytes);
      const sheet = book.getWorksheet();
      const start = performance.now();
      sheet.unMergeCells(selection);
      const elapsed = performance.now() - start;
      if (sample >= 0) samples[i].push(elapsed);
      if (sample === 0) {
        for (let row = 1; row <= 300; row++) {
          assert.equal(sheet.getCell(row, 2).isMerged, row > removed);
        }
        assert.equal(sheet.getCell('E1').isMerged, true);
        outputs[i] = await book.xlsx.writeBuffer();
      }
    }
    if (sample === 0) {
      assert.deepEqual(outputs[0], outputs[1]);
      if (!removed) assert.deepEqual(outputs[1], bytes);
    }
  }
  const stats = samples.map(series => {
    series.sort((a, b) => a - b);
    return { median: ((series[17] + series[18]) / 2).toFixed(3), p95: series[34].toFixed(3) };
  });
  console.log(JSON.stringify({ selection, removed, before: stats[0], after: stats[1] }));
}
