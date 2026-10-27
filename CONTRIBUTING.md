# Contributing

worker-dock is provided as-is under the MIT licence. We don't accept issues or pull requests. Fork it and build your own.

[SECURITY.md](SECURITY.md) is the only contact route.

## Build

Rust (the channel in `rust-toolchain.toml`), Node.js 22+, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

Ubuntu also needs WebKitGTK 4.1:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev pkg-config
```

Windows needs the MSVC C++ workload, WebView2, and (for MSI) the VBSCRIPT optional feature.

```bash
npm install
npm run tauri dev
```

## Tests

```bash
npm run check
npm run build
cd src-tauri && cargo test
```

`npm run tauri build` produces a `.deb` on Ubuntu and an MSI on Windows. MSI cannot be built on Linux.
