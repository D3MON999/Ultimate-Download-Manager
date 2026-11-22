// Packs the browser extension into its two shipped folders (bundled by the installer):
//   build-ext/extension-chromium  Chrome, Edge, Brave, Opera, Vivaldi (manifest.json)
//   build-ext/extension-firefox   Firefox and its forks (manifest.firefox.json as manifest.json)
// and the zips to upload to the stores (build-ext/store):
//   ultimate-download-manager-chromium.zip  Chrome Web Store, Edge Add-ons, Opera add-ons: the
//                                           manifest without its "key" (the stores sign it)
//   ultimate-download-manager-firefox.zip   addons.mozilla.org
// Run from the project root: node tools/pack-extension.mjs
import { cpSync, rmSync, mkdirSync, renameSync, existsSync, readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { deflateRawSync, crc32 } from "node:zlib";

const src = "extension";
const out = "build-ext";
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const chromium = join(out, "extension-chromium");
cpSync(src, chromium, { recursive: true });
rmSync(join(chromium, "manifest.firefox.json"), { force: true });

const firefox = join(out, "extension-firefox");
cpSync(src, firefox, { recursive: true });
rmSync(join(firefox, "manifest.json"), { force: true });
renameSync(join(firefox, "manifest.firefox.json"), join(firefox, "manifest.json"));

for (const d of [chromium, firefox]) {
  if (!existsSync(join(d, "manifest.json"))) throw new Error(`${d} has no manifest.json`);
}
// The stores' copies.
const store = join(out, "store");
mkdirSync(store, { recursive: true });
const forStore = join(out, "store-chromium");
cpSync(chromium, forStore, { recursive: true });
const manifest = JSON.parse(readFileSync(join(forStore, "manifest.json"), "utf8"));
delete manifest.key;
writeFileSync(join(forStore, "manifest.json"), JSON.stringify(manifest, null, 4) + "\n");
// A zip of every file under `dir` (deflated, paths with "/"): Node's own zlib, no outside tool.
const zip = (dir, name) => {
  const files = [];
  const walk = (d) => {
    for (const e of readdirSync(d)) {
      const p = join(d, e);
      if (statSync(p).isDirectory()) walk(p);
      else files.push(p);
    }
  };
  walk(dir);
  const parts = [];
  const central = [];
  let offset = 0;
  for (const f of files.sort()) {
    const data = readFileSync(f);
    const packed = deflateRawSync(data, { level: 9 });
    const path = Buffer.from(relative(dir, f).split("\\").join("/"));
    const crc = crc32(data) >>> 0;
    const head = Buffer.alloc(30);
    head.writeUInt32LE(0x04034b50, 0);
    head.writeUInt16LE(20, 4);
    head.writeUInt16LE(0x0800, 6); // UTF-8 names
    head.writeUInt16LE(8, 8); // deflate
    head.writeUInt32LE(0x00210000, 10); // 1980-01-01 00:00
    head.writeUInt32LE(crc, 14);
    head.writeUInt32LE(packed.length, 18);
    head.writeUInt32LE(data.length, 22);
    head.writeUInt16LE(path.length, 26);
    parts.push(head, path, packed);
    const dirEntry = Buffer.alloc(46);
    dirEntry.writeUInt32LE(0x02014b50, 0);
    dirEntry.writeUInt16LE(20, 4);
    dirEntry.writeUInt16LE(20, 6);
    dirEntry.writeUInt16LE(0x0800, 8);
    dirEntry.writeUInt16LE(8, 10);
    dirEntry.writeUInt32LE(0x00210000, 12);
    dirEntry.writeUInt32LE(crc, 16);
    dirEntry.writeUInt32LE(packed.length, 20);
    dirEntry.writeUInt32LE(data.length, 24);
    dirEntry.writeUInt16LE(path.length, 28);
    dirEntry.writeUInt32LE(offset, 42);
    central.push(dirEntry, path);
    offset += head.length + path.length + packed.length;
  }
  const dirBytes = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(files.length, 8);
  end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(dirBytes.length, 12);
  end.writeUInt32LE(offset, 16);
  writeFileSync(join(store, name), Buffer.concat([...parts, dirBytes, end]));
};
zip(forStore, "ultimate-download-manager-chromium.zip");
zip(firefox, "ultimate-download-manager-firefox.zip");
rmSync(forStore, { recursive: true, force: true });

console.log(`packed ${chromium}, ${firefox} and the store zips in ${store}`);
