import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const directories = process.argv.slice(2);
assert.equal(directories.length, 2, 'Expected the unpacked baseline and current package directories');
const versions = await Promise.all(directories.map(async directory => {
  const entry = pathToFileURL(resolve(directory, 'src/exceljs-node.js'));
  return (await import(entry)).Workbook;
}));
const [, Workbook] = versions;
const input = new Workbook();
input.addWorksheet('Rules');
const bytes = await input.xlsx.writeBuffer();

for (const count of [1, 20, 200]) {
  const rules = Array.from({ length: count }, (_, index) => ({
    type: 'cellIs', operator: 'greaterThan', formulae: [index],
    stopIfTrue: index % 2 === 0, style: { font: { bold: true } },
  }));
  const samples = [[], []];
  for (let sample = -8; sample < 36; sample++) {
    const outputs = [];
    for (const i of sample % 2 ? [1, 0] : [0, 1]) {
      const book = await new versions[i]().xlsx.load(bytes);
      const sheet = book.getWorksheet();
      const start = performance.now();
      sheet.addConditionalFormatting({ ref: 'A1:T20', rules });
      const elapsed = performance.now() - start;
      if (sample >= 0) samples[i].push(elapsed);
      if (sample === 0) {
        const [block] = sheet.conditionalFormattings;
        assert.equal(block.rules.length, count);
        assert.deepEqual(block.rules.map(rule => rule.formulae[0]), rules.map(rule => String(rule.formulae[0])));
        outputs[i] = await book.xlsx.writeBuffer();
      }
    }
    if (sample === 0) assert.deepEqual(outputs[0], outputs[1]);
  }
  const stats = samples.map(series => {
    series.sort((a, b) => a - b);
    return { median: ((series[17] + series[18]) / 2).toFixed(3), p95: series[34].toFixed(3) };
  });
  console.log(JSON.stringify({ count, before: stats[0], after: stats[1] }));
}
