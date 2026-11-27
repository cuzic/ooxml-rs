// Run from a scratch project with the packed @ooxml-rs/xlsx and webpack installed.
import assert from 'node:assert/strict';
import { copyFile, cp, mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { gzipSync } from 'node:zlib';

const project = process.cwd();
const require = createRequire(resolve(project, 'package.json'));
const webpack = require('webpack');
const inputDir = resolve(project, 'sources');
await mkdir(inputDir, { recursive: true });

const fixtures = {
  exceljs: "export { Workbook } from '@ooxml-rs/xlsx/exceljs';",
  sheetjs: "export { read, write, stream } from '@ooxml-rs/xlsx/sheetjs';",
  both: [
    "export { Workbook } from '@ooxml-rs/xlsx/exceljs';",
    "export { read, write } from '@ooxml-rs/xlsx/sheetjs';",
  ].join('\n'),
};

const pairedFixtures = {
  ssf: "export { SSF } from '@ooxml-rs/xlsx/sheetjs';",
  ...fixtures,
  'exceljs-reference': fixtures.exceljs.replace('@ooxml-rs/xlsx/exceljs', 'exceljs'),
  'sheetjs-reference': fixtures.sheetjs.replace('@ooxml-rs/xlsx/sheetjs', 'xlsx'),
};

for (const [name, source] of Object.entries(pairedFixtures)) {
  const entry = resolve(inputDir, name + '.mjs');
  const path = resolve(project, 'dist', name);
  await writeFile(entry, source);
  const compiler = webpack({
    mode: 'production',
    target: ['web', 'es2022'],
    entry,
    experiments: { outputModule: true },
    output: { path, clean: true, filename: 'bundle.mjs', library: { type: 'module' } },
  });
  const stats = await new Promise((resolve, reject) => {
    compiler.run((error, result) => compiler.close((closeError) => {
      if (error || closeError) reject(error ?? closeError);
      else resolve(result);
    }));
  });
  const result = stats.toJson({ all: false, assets: true, errors: true, modules: true, nestedModules: true });
  assert.equal(stats.hasErrors(), false, JSON.stringify(result.errors));
  function flatten(modules) {
    return modules.flatMap((module) => [module.name, ...flatten(module.modules ?? [])]);
  }
  const names = flatten(result.modules ?? []).filter(Boolean);
  for (const adapter of ['exceljs', 'sheetjs']) {
    assert.equal(names.some((module) => module.endsWith('/src/' + adapter + '.js')),
      name === adapter || name === 'both', names.join('\n'));
  }
  const reference = name.endsWith('-reference');
  const assets = await readdir(path);
  assert.equal(assets.filter((name) => name.endsWith('.wasm')).length, reference || name === 'ssf' ? 0 : 1);
  if (reference) continue;
  for (const filename of assets) {
    const bytes = await readFile(resolve(path, filename));
    console.log(name, filename, bytes.length, gzipSync(bytes).length);
  }
}

await copyFile(new URL('./reference.html', import.meta.url), resolve(project, 'reference.html'));
await copyFile(new URL('./bundles.html', import.meta.url), resolve(project, 'bundles.html'));
await copyFile(new URL('../../../reference/fixtures/xlsx/poi/57523.xlsx', import.meta.url),
  resolve(project, '57523.xlsx'));

const deployment = resolve(project, 'deployment');
await mkdir(deployment, { recursive: true });
await copyFile(resolve(project, 'bundles.html'), resolve(deployment, 'bundles.html'));
await copyFile(resolve(project, '57523.xlsx'), resolve(deployment, '57523.xlsx'));
await cp(resolve(project, 'dist/both'), resolve(deployment, 'dist/both'), { recursive: true });
