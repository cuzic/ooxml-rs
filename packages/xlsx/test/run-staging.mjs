// Run from the scratch Playwright project against an all-features Wasm build.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

const require = createRequire(resolve(process.cwd(), 'package.json'));
const { chromium, firefox } = require('playwright');
const [engine, origin, sampleCount = '1'] = process.argv.slice(2);
const samples = Number(sampleCount);
assert.ok(Number.isSafeInteger(samples) && samples > 0, 'Expected a positive sample count');
assert.ok(engine === 'chromium' || engine === 'firefox');
const type = engine === 'firefox' ? firefox : chromium;
const browser = await type.launch({ executablePath: process.env.OOXML_BROWSER_BINARY, timeout: 15000 });
try {
  const page = await browser.newPage();
  for (const fixture of ['scalar', 'sparse']) {
    for (let sample = 0; sample < samples; sample += 1) {
      const order = sample % 2 ? [true, false] : [false, true];
      for (const candidate of order) {
        await page.goto(origin + '/fixtures/');
        const result = await page.evaluate(async ({ fixture, candidate }) => {
          const input = new Uint8Array(await (await fetch('./' + fixture + '.xlsx')).arrayBuffer());
          const initial = performance.now();
          const wasm = await import('../wasm/ooxml_xlsx_wasm.js');
          const { memory } = await wasm.default();
          const initialized = performance.now();
          let read;
          let write;
          let save;
          let close;
          if (candidate) {
            const document = new wasm.AtomicXlsxDocument(input);
            read = () => document.read_scalar('Sheet1', 0, 0);
            write = (value) => document.write_scalar('Sheet1', 0, 0, value);
            save = () => document.to_bytes();
            close = () => document.free();
          } else {
            const { Workbook } = await import('../src/exceljs.js');
            const book = await new Workbook().xlsx.load(input);
            const cell = book.getWorksheet('Sheet1').getCell('A1');
            read = () => cell.value;
            write = (value) => { cell.value = value; };
            save = () => book.xlsx.writeBuffer();
            close = () => {};
          }
          try {
            const loaded = performance.now();
            for (let number = 701; number <= 710; number++) write(number);
            const edited = performance.now();
            if (read() !== 710) throw Error('last setter not visible');
            const bytes = await save();
            const saved = performance.now();
            const reopened = new wasm.XlsxDocument(bytes);
            try {
              if (reopened.read_scalar('Sheet1', 0, 0, true) !== 710) {
                throw Error('last setter missing from saved output');
              }
            } finally {
              reopened.free();
            }
            return {
              fixture, candidate, bytes: input.byteLength,
              initializationMs: initialized - initial,
              loadMs: loaded - initialized,
              tenSettersMs: edited - loaded,
              readAndSaveMs: saved - edited,
              wasmMemoryHighWaterBytes: memory.buffer.byteLength,
            };
          } finally {
            close();
          }
        }, { fixture, candidate });
        console.log(engine, browser.version(), JSON.stringify({ sample, ...result }));
      }
    }
  }
} finally {
  await browser.close();
}
