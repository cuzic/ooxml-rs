import type {
  CFB$AddOpts, CFB$Blob, CFB$Container, CFB$Entry, CFB$Utils,
  find, parse, read, version, write, writeFile,
} from 'cfb';
export const CFB: {
  version: typeof version;
  find: typeof find;
  parse: typeof parse;
  read: typeof read;
  write: typeof write;
  writeFile: typeof writeFile;
  utils: Omit<CFB$Utils, 'WarnField' | 'cfb_new' | 'cfb_add' | 'prep_blob'> & {
    cfb_new(options?: { root?: string; CLSID?: string }): CFB$Container;
    cfb_add(container: CFB$Container, name: string, contents?: CFB$Blob,
      options?: CFB$AddOpts & { CLSID?: string; ct?: Date; mt?: Date }): CFB$Entry;
    prep_blob(blob: CFB$Blob, position: number): void;
    use_zlib(zlib: object): void;
    _deflateRaw(bytes: CFB$Blob): CFB$Blob;
    _inflateRaw(bytes: CFB$Blob, uncompressedSize?: number): CFB$Blob;
    consts: Record<string, unknown>;
  };
};

