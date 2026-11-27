import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const output = new URL('./wasm/', import.meta.url);
const temporary = mkdtempSync(join(tmpdir(), 'ooxml-cfb-'));
try {
  execFileSync('npm', ['pack', 'cfb@1.2.2', '--ignore-scripts', '--pack-destination', temporary, '--silent']);
  const tarball = join(temporary, 'cfb-1.2.2.tgz');
  const integrity = createHash('sha512').update(readFileSync(tarball)).digest('base64');
  if (integrity !== 'KfdUZsSOw19/ObEWasvBP/Ac4reZvAGauZhs6S/gqNhXhI7cKwvlH7ulj+dOEYnca4bm4SGo8C1bTAQvnTjgQA==') {
    throw new Error('Unexpected integrity for cfb@1.2.2');
  }

  const unpack = entry => execFileSync('tar', ['-xOf', tarball, 'package/' + entry], { encoding: 'utf8' });
  const upstream = unpack('dist/cfb.js');
  const footer = "if(typeof require !== 'undefined' && typeof module !== 'undefined' && typeof DO_NOT_EXPORT_CFB === 'undefined') { module.exports = CFB; }";
  if (!upstream.trimEnd().endsWith(footer)) throw new Error('Unexpected CFB distribution footer');
  const fileSystem = "function get_fs() { return fs || (fs = require('fs')); }";
  if (!upstream.includes(fileSystem)) throw new Error('Unexpected CFB filesystem loader');
  const source = upstream.trimEnd().slice(0, -footer.length)
    .replace(fileSystem, "function get_fs() { throw new Error('CFB file paths require Node.js'); }");
  mkdirSync(output, { recursive: true });
  writeFileSync(new URL('cfb-browser.js', output), source + 'export default CFB;\n');
  writeFileSync(new URL('CFB-LICENSE', output), unpack('dist/LICENSE'));
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
