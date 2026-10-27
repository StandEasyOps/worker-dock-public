# worker-dock

worker-dock is provided as-is under the MIT licence; we don't accept issues or pull requests, so fork it and build your own (the release workflow builds the Windows MSI and Ubuntu .debs on a fork's own Actions).

**worker-dock** is a desktop app by Stand Easy Ops for Windows and Ubuntu (macOS later). It installs, runs, and manages personal self-hosted workers for the official `agent` CLI (`agent login`, `agent worker start`, `agent worker debug`).

The app supervises the official CLI. It does not replace the worker protocol, and it does not use “Cursor” in the product name. The window, tray, and installers say **worker-dock**. App icons and the header mark are the Stand Easy Ops tile and A1 mark. **standeasyops.com** sits next to Cursor Docs. Wording such as “works with the official Cursor CLI” is fine.

This repository has the Tauri 2 + Svelte app, official `agent` CLI lifecycle, sign-in, one named personal worker, per-user start/stop, Logs, Preflight, and GitHub Actions that build **unsigned** Ubuntu `.deb` and Windows MSI tester artifacts. Signing, apt hosting, and winget publish are not in this slice.

## Stack

- Rust + [Tauri 2](https://v2.tauri.app/)
- Svelte 5 / SvelteKit (static SPA adapter) + TypeScript + Vite
- Bundle targets: Debian `.deb` (Ubuntu) and WiX MSI (Windows)
- Identifier: `ops.standeasy.worker-dock`
- Publisher: Stand Easy Ops

## Documentation

| Document | What it covers |
| --- | --- |
| [docs/DESIGN.md](docs/DESIGN.md) | Goals, architecture, OS service model, auth, config, UI, updates, packaging, security, risks, and milestones |
| [docs/TECH-CHOICE.md](docs/TECH-CHOICE.md) | Tauri 2 vs pure-Rust GUI vs Avalonia vs Electron, with a recommendation |
| [docs/CLI-SPIKE.md](docs/CLI-SPIKE.md) | What a live official CLI install accepted, with links to the Cursor docs |
| [packaging/README.md](packaging/README.md) | Unsigned CI artifacts, placeholder `get.standeasyops.com` URLs, winget id, signing hook |

## Develop

Requires [Rust](https://www.rust-lang.org/tools/install) (see `rust-toolchain.toml`), Node.js 22+, and the [Tauri Linux or Windows prerequisites](https://v2.tauri.app/start/prerequisites/).

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

The window is Home plus Settings. Home leads with worker name (`--name`) and repo folders, then Run without service and Service setup. Next to the name: **Open in Agents** (`https://cursor.com/agents`) and **Local status** (`http://127.0.0.1:<port>/readyz`; no local dashboard). Live status is localhost `/readyz` plus documented `/metrics` gauges (live / idle / agent running). A watchdog restarts a user-started worker if it exits or stops responding; **Copy diagnostics** exports redacted logs; **Reset worker identity** wipes only the isolated `--data-dir` and starts again with the same flags. The top bar shows **worker-dock**, the Stand Easy Ops mark, a Ready / Not ready lamp, **Cursor Docs**, and **standeasyops.com**. Auto-start is its own toggle — it does not change Ready to amber. Logs and Checks sit in a drawer. Closing the window leaves an installed service running; quitting the app stops a child worker. A second launch focuses the existing window (single-instance). On Ubuntu, tray left-click opens the menu only; Open unminimizes the window and does not hide it again. `apt upgrade` may stop the invoking user's leftover UI so the old tray icon does not stay next to the new one; it does not stop other users' windows or `worker-dock-run`. If the installer cannot tell which user invoked it, running windows are left alone until the next launch. If two icons remain, use **Quit UI** on the stale one. The CLI package is about 500 MB. Team Pools and service-account keys are out of scope.

CI on pull requests runs `cargo test` (Ubuntu 22.04 + Windows) and `npm run check` / `build`. Packaging CI stamps `0.1.<run_number>` so each unsigned `.deb` / MSI upgrades the last install. Tag `vX.Y.Z` (must match the committed app version) builds an unsigned `.deb` and MSI and attaches them to a draft GitHub Release. Public URLs use the placeholder host `https://get.standeasyops.com` (not provisioned). winget id `StandEasyOps.WorkerDock` is reserved in docs only. Azure Trusted Signing is a commented hook — artifacts are unsigned.

Frontend and Rust checks:

```bash
npm run check
npm run build
cd src-tauri && cargo test
```

`npm run tauri build` produces a `.deb` on Ubuntu and an MSI on Windows. MSI cannot be built on Linux. macOS is out of scope.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), and [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).
