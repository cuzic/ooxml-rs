// Shared-string cell reads: SheetJS `read` (default and dense) and ExcelJS
// `xlsx.load` followed by reading every cell value and text.
//
//   node measure-shared-string-reads.mjs <baseline-package> <current-package> [cells ...]
//
// Pass unpacked package directories whose runtime dependencies resolve
// (for example, copies under one node_modules). Every sample is a fresh
// process: two warm-up reads of a 1,000-cell workbook, then one timed read
// of the workload. File I/O is outside the timed region. Each process
// reports the time, peak RSS (`process.resourceUsage().maxRSS`) and a hash
// of the projected result; the hashes must agree between builds.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { crc32 } from 'node:zlib';

const samples = 5;
const modes = ['sheetjs', 'sheetjs-dense', 'exceljs'];

if (process.argv[2] === '--child') {
  const [directory, file, warmupFile, mode] = process.argv.slice(3);
  const bytes = readFileSync(file);
  const warmupBytes = readFileSync(warmupFile);
  const entry = name => pathToFileURL(resolve(directory, 'src', name)).href;
  let read;
  if (mode.startsWith('sheetjs')) {
    const XLSX = await import(entry('sheetjs-node.js'));
    const options = mode === 'sheetjs-dense' ? { dense: true } : undefined;
    // The adapter projects a sheet when it is first accessed: touch every
    // cell inside the timed read.
    read = async input => {
      const workbook = options ? XLSX.read(input, options) : XLSX.read(input);
      let characters = 0;
      for (const name of workbook.SheetNames) {
        const sheet = workbook.Sheets[name];
        if (sheet['!data']) {
          for (const row of sheet['!data']) for (const cell of row ?? []) if (cell) characters += String(cell.v).length;
        } else {
          for (const key in sheet) if (key[0] !== '!') characters += String(sheet[key].v).length;
        }
      }
      assert.ok(characters > 0);
      return workbook;
    };
  } else {
    const { Workbook } = await import(entry('exceljs-node.js'));
    read = async input => {
      const workbook = new Workbook();
      await workbook.xlsx.load(input);
      const cells = [];
      workbook.eachSheet(sheet => sheet.eachRow(row => row.eachCell(cell => {
        // A refused read (for example a phonetic item) is part of the result.
        try { cells.push([cell.address, cell.value, cell.text]); } catch (error) { cells.push([cell.address, error.code]); }
      })));
      return cells;
    };
  }
  for (let index = 0; index < 2; index++) await read(warmupBytes);
  const start = performance.now();
  const result = await read(bytes);
  const ms = performance.now() - start;
  const stable = JSON.stringify(result, (_, value) =>
    value instanceof Date ? { date: value.toISOString() } : value === undefined ? { undefined: true } : value);
  console.log(JSON.stringify({
    ms,
    rss: process.resourceUsage().maxRSS,
    hash: createHash('sha256').update(stable).digest('hex'),
  }));
  process.exit(0);
}

const directories = process.argv.slice(2, 4).map(directory => resolve(directory));
assert.equal(directories.length, 2, 'Expected unpacked baseline and current package directories');
const sizes = process.argv.slice(4).map(Number);
if (sizes.length === 0) sizes.push(10_000, 50_000, 100_000);

// A stored (uncompressed) ZIP archive.
function zip(files) {
  const local = [];
  const central = [];
  let offset = 0;
  for (const [name, text] of Object.entries(files)) {
    const nameBytes = Buffer.from(name);
    const data = Buffer.from(text);
    const header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt32LE(crc32(data), 14);
    header.writeUInt32LE(data.length, 18);
    header.writeUInt32LE(data.length, 22);
    header.writeUInt16LE(nameBytes.length, 26);
    const directory = Buffer.alloc(46);
    directory.writeUInt32LE(0x02014b50, 0);
    directory.writeUInt16LE(20, 4);
    directory.writeUInt16LE(20, 6);
    header.copy(directory, 10, 8, 30);
    directory.writeUInt32LE(offset, 42);
    local.push(header, nameBytes, data);
    central.push(directory, nameBytes);
    offset += header.length + nameBytes.length + data.length;
  }
  const centralBytes = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(Object.keys(files).length, 8);
  end.writeUInt16LE(Object.keys(files).length, 10);
  end.writeUInt32LE(centralBytes.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...local, centralBytes, end]);
}

// Ten columns of `t="s"` cells. 60% of the cells reference distinct items;
// the rest repeat a hot tenth of the table. Every 50th item is rich text
// and every 70th has phonetic runs.
function workbook(cells) {
  const columns = 10;
  const rows = Math.ceil(cells / columns);
  const unique = Math.floor(cells * 0.6);
  const items = [];
  for (let index = 0; index < unique; index++) {
    if (index % 50 === 7) {
      items.push(`<si><r><rPr><b/><sz val="11"/><color rgb="FFFF0000"/><rFont val="Calibri"/></rPr><t>Bold ${index}</t></r><r><t xml:space="preserve"> tail &amp; ${index}</t></r></si>`);
    } else if (index % 70 === 11) {
      items.push(`<si><t>漢字${index}</t><rPh sb="0" eb="2"><t>カンジ</t></rPh><phoneticPr fontId="0" type="noConversion"/></si>`);
    } else {
      items.push(`<si><t>item ${index} &lt;x&gt; &amp; y</t></si>`);
    }
  }
  const column = index => String.fromCharCode(65 + index);
  const rowXml = [];
  for (let row = 1, cell = 0; row <= rows; row++) {
    const values = [];
    for (let col = 0; col < columns && cell < cells; col++, cell++) {
      const item = cell < unique ? cell : (cell * 7919) % Math.max(1, Math.floor(unique / 10));
      values.push(`<c r="${column(col)}${row}" t="s"><v>${item}</v></c>`);
    }
    rowXml.push(`<row r="${row}">${values.join('')}</row>`);
  }
  const ns = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"';
  const rel = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
  const type = 'application/vnd.openxmlformats-officedocument.spreadsheetml';
  return zip({
    '[Content_Types].xml': `<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="${type}.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="${type}.worksheet+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="${type}.sharedStrings+xml"/><Override PartName="/xl/styles.xml" ContentType="${type}.styles+xml"/></Types>`,
    '_rels/.rels': `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${rel}/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
    'xl/workbook.xml': `<workbook ${ns} xmlns:r="${rel}"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>`,
    'xl/_rels/workbook.xml.rels': `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${rel}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="${rel}/sharedStrings" Target="sharedStrings.xml"/><Relationship Id="rId3" Type="${rel}/styles" Target="styles.xml"/></Relationships>`,
    'xl/styles.xml': `<styleSheet ${ns}><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs></styleSheet>`,
    'xl/worksheets/sheet1.xml': `<worksheet ${ns}><dimension ref="A1:${column(columns - 1)}${rows}"/><sheetData>${rowXml.join('')}</sheetData></worksheet>`,
    'xl/sharedStrings.xml': `<sst ${ns} count="${cells}" uniqueCount="${unique}">${items.join('')}</sst>`,
  });
}

const scratch = mkdtempSync(join(tmpdir(), 'sst-reads-'));
const script = fileURLToPath(import.meta.url);
const statistics = values => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = sorted.length >> 1;
  const median = sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
  return { median, min: sorted[0], max: sorted.at(-1) };
};
const format = ({ median, min, max }, digits) =>
  `${median.toFixed(digits)} (${min.toFixed(digits)}–${max.toFixed(digits)})`;
try {
  const warmup = join(scratch, 'warmup.xlsx');
  writeFileSync(warmup, workbook(1_000));
  console.log(`Node ${process.version}; ${samples} processes per build; time ms and peak RSS MiB, median (range)`);
  console.log('| Cells | Read | Time before → after | RSS before → after |');
  console.log('| ---: | --- | ---: | ---: |');
  for (const cells of sizes) {
    const file = join(scratch, `cells-${cells}.xlsx`);
    writeFileSync(file, workbook(cells));
    for (const mode of modes) {
      const runs = [[], []];
      for (let sample = 0; sample < samples; sample++) {
        // Alternate which build runs first.
        const order = sample % 2 ? [1, 0] : [0, 1];
        for (const build of order) {
          const child = spawnSync(process.execPath, [script, '--child', directories[build], file, warmup, mode], {
            encoding: 'utf8', maxBuffer: 1 << 20,
          });
          assert.equal(child.status, 0, child.stderr);
          runs[build].push(JSON.parse(child.stdout));
        }
      }
      const hashes = new Set(runs.flat().map(run => run.hash));
      assert.equal(hashes.size, 1, `${cells} ${mode}: results differ between builds`);
      const time = runs.map(list => format(statistics(list.map(run => run.ms)), 0));
      const rss = runs.map(list => format(statistics(list.map(run => run.rss / 1024)), 0));
      console.log(`| ${cells.toLocaleString('en-US')} | ${mode} | ${time[0]} → ${time[1]} | ${rss[0]} → ${rss[1]} |`);
    }
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
