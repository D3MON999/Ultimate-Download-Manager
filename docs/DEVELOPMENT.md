# Developer notes

How to build Ultimate Download Manager and make a release. For what the app does, see the [main page](../README.md).

## What's where

| Folder | What |
|---|---|
| `src/` | The windows (SvelteKit and TypeScript) |
| `src-tauri/` | The desktop app (Tauri 2 and Rust) |
| `crates/` | The download engine, torrents, video tools, archive extraction, debrid accounts, the browser helper and the phone remote |
| `extension/` | The browser extension (Chrome, Edge, Brave, Opera, Vivaldi, Firefox) |
| `tools/` | Packing the extension and making a release |

## Build

You'll need Rust (MSVC toolchain), Node.js and the Visual Studio C++ build tools.

```powershell
npm install
npm run tauri dev
```

`npm run tauri dev` runs the app with live reload. To build the installer:

```powershell
npm run tauri build
```

The installer is written to `target\release\bundle\nsis`.

## Tests

```powershell
cargo test --workspace
npx svelte-check
```

Some tests run small servers on 127.0.0.1. Security software that filters local traffic can make those fail on its own.

## Release

1. Raise `version` in `package.json`, `Cargo.toml`, `src-tauri/tauri.conf.json` and both extension manifests.
2. Run the release script:

   ```powershell
   powershell -ExecutionPolicy Bypass -File tools\release.ps1
   ```

   It builds and writes everything to upload into `release\v<version>\`: the installer, its `.sha256`, the signed `latest.json` for updates, the source zip and a release-notes draft.
3. Commit as `Ultimate Download Manager <version>`, push, and publish a GitHub release tagged `v<version>` with those files.

Updates are signed with `keys\updater.key`. Keep it backed up and never commit it: without it, no update can be published.

### Code signing

Without a code-signing certificate the installer is unsigned and Windows warns about an unknown publisher. To sign, set one of the options at the top of `src-tauri/windows/sign.ps1` before building.

### Browser extension stores

`node tools/pack-extension.mjs` (also run by every build) writes the zips to upload to `build-ext\store\`. The listing text and the reason for each permission are in [store/LISTING.md](store/LISTING.md); the privacy policy is [PRIVACY.md](../PRIVACY.md).
