import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [baseline, current, mode = 'shared'] = process.argv.slice(2);
assert.ok(baseline && current, 'Expected the unpacked baseline and current package directories');
assert.ok(['array', 'shared'].includes(mode), 'Expected shared or array formula mode');
const packages = [baseline, current];
const versions = await Promise.all(packages.map(async directory => {
  const entry = pathToFileURL(resolve(directory, 'src/exceljs-node.js'));
  return (await import(entry)).Workbook;
}));
const [, Workbook] = versions;
const results = Array(400).fill(3);

for (const links of [0, 40, 400]) {
  const input = new Workbook();
  const initialSheet = input.addWorksheet('Bench');
  for (let index = 0; index < links; index++) {
    const row = Math.floor(index / 20) + 1;
    const col = index % 20 + 1;
    initialSheet.getCell(row, col).value = { text: 'replaced', hyperlink: 'https://example.org/replaced' };
  }
  initialSheet.getCell('U1').value = { text: 'kept', hyperlink: 'https://example.org/kept' };
  const bytes = await input.xlsx.writeBuffer();
  const samples = [[], []];
  for (let attempt = -8; attempt < 36; attempt++) {
    const output = [];
    for (const i of attempt % 2 ? [1, 0] : [0, 1]) {
      const book = await new versions[i]().xlsx.load(bytes);
      const sheet = book.getWorksheet();
      const start = performance.now();
      sheet.fillFormula('A1:T20', '1+2', results, mode);
      const elapsed = performance.now() - start;
      if (attempt >= 0) samples[i].push(elapsed);
      if (attempt === 0) {
        for (let row = 1; row <= 20; row++) {
          for (let col = 1; col <= 20; col++) {
            const cell = sheet.getCell(row, col);
            const resultCell = mode === 'shared' || (row === 1 && col === 1);
            assert.equal(resultCell ? cell.result : cell.value, 3);
            assert.equal(cell.hyperlink, undefined);
          }
        }
        assert.equal(sheet.getCell('U1').hyperlink, 'https://example.org/kept');
        output[i] = await book.xlsx.writeBuffer();
      }
    }
    if (attempt === 0) assert.deepStrictEqual(output[0], output[1]);
  }
  const summaries = samples.map(series => {
    series.sort((a, b) => a - b);
    return { median: ((series[17] + series[18]) / 2).toFixed(2), p95: series[34].toFixed(2) };
  });
  console.log(JSON.stringify({ links, bytes: bytes.byteLength, before: summaries[0], after: summaries[1] }));
}
