// Run from the scratch test project with Playwright installed.
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

const require = createRequire(resolve(process.cwd(), 'package.json'));
const { chromium, firefox } = require('playwright');
const [engine, address, artifact, mode] = process.argv.slice(2);
assert.ok(engine === 'chromium' || engine === 'firefox');
const type = engine === 'firefox' ? firefox : chromium;
const browser = await type.launch({ executablePath: process.env.OOXML_BROWSER_BINARY, timeout: 15000 });
try {
  const page = await browser.newPage();
  if (mode === 'mutation') {
    const source = await readFile(resolve(artifact, 'src/exceljs.js'), 'utf8');
    const mutation = source.replace(
      '  let bytes;\n  try {\n    action(candidate);\n    bytes =',
      '  let bytes;\n  try {\n    action(candidate);\n    const previous = state.document;\n    state.document = candidate;\n    bytes =',
    ).replace(
      '  } catch (error) {\n    candidate.free();\n    throw mapError ? mapError(error) : error;\n  }\n  const previous = state.document;\n  state.document = candidate;\n  state.bytes = bytes;\n  previous.free();',
      '  } catch (error) {\n    throw mapError ? mapError(error) : error;\n  }\n  state.bytes = bytes;',
    );
    assert.notEqual(source, mutation);
    await page.route('**/src/exceljs.js', (route) => route.fulfill({
      contentType: 'application/javascript', body: mutation,
    }));
  }
  await page.goto(address);
  await page.waitForFunction(() => document.body.innerText !== 'Running');
  const result = await page.locator('body').innerText();
  if (mode === 'mutation') {
    assert.match(result, /^FAIL Error: serialization failure changed commit/);
    console.log(engine, browser.version(), 'mutation rejected');
  } else {
    assert.equal(result, 'PASS');
    const { sheetOutput, committed } = await page.evaluate(() => globalThis.xlsxTestOutput);
    await writeFile(resolve(artifact, 'sheet-output.xlsx'), Uint8Array.from(sheetOutput));
    await writeFile(resolve(artifact, 'excel-output.xlsx'), Uint8Array.from(committed));
    console.log(engine, browser.version(), result);
  }
} finally {
  await browser.close();
}
