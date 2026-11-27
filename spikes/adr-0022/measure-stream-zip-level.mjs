import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { Writable } from 'node:stream';
import { pathToFileURL } from 'node:url';
import { performance } from 'node:perf_hooks';

const [directory] = process.argv.slice(2);
assert.ok(directory, 'Expected the unpacked xlsx package directory');
const entry = pathToFileURL(resolve(directory, 'src/exceljs-node.js'));
const { stream } = await import(entry);

const rows = 100_000;
async function run(level, count) {
  let bytes = 0;
  const sink = new Writable({ write(chunk, encoding, callback) { bytes += chunk.length; callback(); } });
  const start = performance.now();
  const writer = new stream.xlsx.WorkbookWriter({ stream: sink, zip: { zlib: { level } } });
  const sheet = writer.addWorksheet('Data');
  for (let i = 0; i < count; i += 1) {
    sheet.addRow([i, `group-${i % 97}`, `Monthly spreadsheet report ${i % 17}`, (i * 73) % 104729]).commit();
  }
  await writer.commit();
  return { ms: Math.round(performance.now() - start), bytes };
}
const levels = [0, 1, 6, 9];
for (const level of levels) await run(level, 5000);
const samples = new Map(levels.map(level => [level, []]));
for (let round = 0; round < 5; round += 1) {
  for (let index = 0; index < levels.length; index += 1) {
    const level = levels[(round + index) % levels.length];
    samples.get(level).push(await run(level, rows));
  }
}
console.log(JSON.stringify({ node: process.version, rows, columns: 4, samples: Object.fromEntries(samples) }, null, 2));
