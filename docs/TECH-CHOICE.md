# Tech choice: worker-dock desktop stack

**Recommendation: Rust + Tauri 2 (Rust core, small Svelte web UI).**  
The tentative direction is sound. The hard parts of this product are process supervision, OS services, keychain, packaging, and a thin control surface — not a large interactive editor. Tauri 2 is the only option that is strong on all of MSI/winget, `.deb`/apt, small installers, and a Rust core we will write anyway. The main tax is Linux WebKitGTK quality, which we should treat as a first-class test target rather than a reason to switch stacks.

This comparison is for **worker-dock**: a developer tray/control app on Windows 10/11 and Ubuntu 22.04/24.04 that wraps the official `agent` CLI. It is not a general “best GUI framework” ranking.

Sources checked 2026-10-20: [Tauri 2 Windows installer](https://v2.tauri.app/distribute/windows-installer/), [Tauri 2 Debian](https://v2.tauri.app/distribute/debian/), [Tauri updater plugin](https://v2.tauri.app/plugin/updater/), [winget-pkgs authoring](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest), and a 2026-10 public size comparison of Electron / Tauri / Avalonia installers.

## Decision criteria

| Criterion | Why it matters here |
| --- | --- |
| Packaging (MSI / winget) | Windows distribution is a launch requirement. winget-pkgs accepts MSI/WiX (and EXE). MSI must be built on Windows. |
| Packaging (deb / apt) | Ubuntu distribution is a launch requirement. A stock `.deb` plus a signed apt repo is the intended channel. |
| Installer and runtime size | Developers already install a ~500 MB official CLI. The supervisor should stay small. |
| Linux desktop quality | Tray, dark theme, file dialogs, and WebKitGTK quirks decide whether Ubuntu feels finished. |
| Dev speed | One person or a small team must ship an MVP that talks to systemd, Task Scheduler, and the OS keychain. |
| Maintainability | The CLI surface will drift. The app must stay a thin, testable supervisor. |

## Options

### 1. Tauri 2 (recommended)

Rust host process + OS webview (WebView2 on Windows, WebKitGTK 4.1 on Linux, WKWebView on macOS later). Official bundler produces WiX MSI, NSIS EXE, and Debian packages. Typical app installer is a few megabytes if WebView2 is already on the machine (Windows 10 1803+ / Windows 11).

**Strengths**

- MSI via WiX and `.deb` are first-class. winget can consume the signed MSI. apt can consume the `.deb`.
- Small download. Matches a tray utility, not an IDE.
- The core work (spawn `agent`, write systemd units, talk to Task Scheduler, keyring, TLS preflight) is Rust-shaped. Tauri does not force a second systems language.
- Tray, updater (signed artifacts, cannot be disabled), and deep-link/browser-open plugins exist.
- Later macOS `.dmg` + notarisation is a documented Tauri path.

**Weaknesses**

- Three rendering engines. Ubuntu quality depends on distro WebKitGTK. Build on Ubuntu 22.04 (or older glibc) so 22.04/24.04 both run; see Tauri’s Debian glibc warning.
- GUI apps on Linux/macOS do not inherit shell `PATH`. The official installer puts `agent` in `~/.local/bin`. We must resolve the binary ourselves (and/or use `fix-path-env-rs`).
- MSI cannot be cross-compiled on Linux. CI needs `windows-latest` for WiX.
- WebView2 bootstrapper is the default; winget/offline users are happier with `embedBootstrapper` (~1.8 MB) than a network fetch.
- Two languages (Rust + a small TS UI). Acceptable if the UI stays a settings/logs shell.

**Verdict:** Best default. Keep the web UI small. The owner chose **Svelte** (official `create-tauri-app` `svelte-ts` template: Svelte 5 + SvelteKit static SPA + TypeScript + Vite) over vanilla TypeScript. Business logic still stays in Rust.

### 2. Pure Rust GUI (egui / iced / Slint)

Immediate-mode or retained native widgets, one language, no webview.

**Strengths**

- Smallest conceptual surface. Excellent for a tray + form + log view if we accept custom widgets.
- No WebKitGTK. Linux look is “our theme,” not “the distro browser.”
- Same Rust core as Tauri without the IPC/webview layer.

**Weaknesses**

- No official MSI/WiX or Debian bundler at Tauri’s level. We would own cargo-wix / cargo-deb, Start Menu shortcuts, uninstall, upgrades, and tray integration per crate.
- winget and apt are doable but all custom. That is schedule risk on a packaging-heavy product.
- Linux desktop polish (HiDPI, a11y, file pickers, notifications) is crate-specific and thinner than GTK/Qt/WebView.
- Dev speed for settings forms and log search is worse than HTML/CSS.
- Slint is the most “product UI” of the three, but licensing/tooling and packager story still lag Tauri.

**Verdict:** Attractive if the UI stays a single window and we later regret WebKitGTK. Not the fastest path to winget + apt. Do not start here.

### 3. .NET Avalonia

C# UI drawn with Skia. Same pixels on Windows and Linux. Native AOT builds in the ~10–15 MB range in public 2026 measurements.

**Strengths**

- Best pixel-identical desktop UI of the four. Linux does not depend on WebKitGTK.
- Windows packaging and Task Scheduler APIs are familiar in .NET.
- One language if the team is C#-first.
- Reasonable size (much smaller than Electron, larger than Tauri).

**Weaknesses**

- MSI + winget + signed `.deb` + apt repo is not a built-in product the way Tauri bundling is. We would assemble WiX, `dotnet publish`, and Debian packaging ourselves.
- Linux desktop integration (tray, systemd user units from the app, secret-service) is possible but less documented than Tauri plugins + Rust crates (`keyring`, `zbus`).
- Stand Easy Ops is not specified as a .NET shop. Introducing a second long-term runtime (and Azure Trusted Signing around `dotnet` publish) is a hiring/oncall cost.
- macOS later is supported, but notarisation + Homebrew still need custom work.

**Verdict:** Strong if the team is already Avalonia/.NET. For a greenfield Rust-friendly supervisor, it is a detour.

### 4. Electron

Chromium + Node. Fastest UI iteration. Official builders for MSI/NSIS and `.deb`.

**Strengths**

- Best Linux desktop predictability (one Chromium everywhere).
- Huge tray/auto-update/installer lore. winget and apt are well-trodden.
- Fastest path to a polished settings/logs UI.

**Weaknesses**

- Installer commonly 100 MB+; idle memory is Chromium-class. Users already pay ~500 MB for the official CLI. Shipping another Chromium for a tray supervisor is a product smell.
- Process supervision and Windows/Linux services in Node are weaker and easier to get wrong than Rust.
- Auto-update (Squirrel/electron-updater) is fine, but code signing + SmartScreen still dominate the Windows story.
- Maintainability: a large JS surface around a systems problem.

**Verdict:** Reject for v1 unless Linux WebKitGTK proves unshippable and Avalonia is also rejected.

## Comparison

| | Tauri 2 | egui / iced / Slint | Avalonia | Electron |
| --- | --- | --- | --- | --- |
| MSI / winget | First-class WiX MSI; build on Windows CI | DIY WiX | DIY WiX / MSIX | Mature electron-builder MSI/NSIS |
| deb / apt | First-class `.deb` (WebKitGTK deps) | DIY cargo-deb | DIY | Mature `.deb` |
| Typical installer | ~2–8 MB (WebView2 not bundled) | Small binary, weak installer story | ~10–15 MB AOT | ~80–150 MB |
| Linux desktop | WebKitGTK 4.1; test early | Custom look; weaker a11y | Consistent Skia; good | Best consistency |
| Dev speed | Fast for a small web UI + Rust core | Slower forms/logs | Fast if C# fluent | Fastest UI |
| Maintainability | Two languages, thin UI | One language | One language | Large JS + Chromium |
| Tray / updater | Official plugins | Crate-by-crate | Community | Excellent |
| Fit for services + keychain | Excellent (Rust) | Excellent (Rust) | Good (.NET) | Weakest |

## Challenges to the tentative direction

1. **This is a supervisor, not a web app.** If the UI is only “status, four forms, a log pane, a tray menu,” a pure-Rust GUI could work. We still pick Tauri because packaging and file/log UI are cheaper there, not because we need a rich SPA.
2. **Linux is the quality risk, not Windows.** Budget real Ubuntu 22.04 + 24.04 tray/WebKit testing in MVP. If WebKitGTK is unacceptable, the fallback is Avalonia or a pure-Rust window — not Electron.
3. **Do not put business logic in the webview.** CLI install, service files, keychain, preflight, and log tailing live in Rust. The UI is a view. That keeps an egui rewrite possible later.
4. **MSI is a Windows-runner problem.** The GitHub Actions matrix must include `windows-latest` for WiX; Linux CI cannot produce the winget artifact.

## Recommendation

Use **Tauri 2 with a Rust core and a small Svelte UI**.

- Bundle targets: WiX MSI (`embedBootstrapper`) and Debian `.deb`.
- UI: Svelte 5 + SvelteKit (static SPA) + TypeScript + Vite, from the official `create-tauri-app` `svelte-ts` template. Do not grow a large client-side framework surface; screens stay thin views over Rust.
- Systems: `keyring` (Credential Manager / libsecret), systemd user units, Windows Task Scheduler, `reqwest` TLS preflight.
- Updates: CLI updates are pinned official archives plus a vetted UI action; `--disable-auto-update` on every `agent` spawn. Do not pipe the live install script. The Tauri updater plugin (signed GitHub Releases) waits until Azure Trusted Signing exists; this slice ships unsigned tester `.deb` / MSI only.
- Revisit only if Ubuntu WebKitGTK or tray integration fails acceptance testing.

The design PR has been accepted. Implementation follows this document and [DESIGN.md](DESIGN.md).
