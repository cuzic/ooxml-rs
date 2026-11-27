export function projectedZipFiles(document) {
  const entries = document.zip_entry_names();
  const sentinel = '\u0001Sh33tJ5';
  const keys = ['', sentinel, ...entries];
  const files = Object.create(null);
  const emptyClassId = '00000000000000000000000000000000';
  files[''] = { name: 'Root Entry', type: 5, color: 1, C: 1, R: -1, L: -1, size: 0, start: 0, clsid: emptyClassId };
  files[sentinel] = { name: sentinel, type: 2, content: Uint8Array.of(55, 50, 54, 50), size: 4,
    L: -1, R: -1, C: -1, color: 1, start: 0, clsid: emptyClassId };
  for (const key of entries) {
    const content = document.zip_entry_bytes(key);
    const name = key.replace(/\/$/, '').split('/').at(-1);
    const entry = { name, type: 2, content, size: content.length };
    const modified = document.zip_entry_modified(key);
    if (modified) {
      const [year, month, day, hours, minutes, seconds] = modified;
      entry.mt = new Date(year, month - 1, day, hours, minutes, seconds);
    }
    files[key] = entry;
  }
  return { keys, files };
}
