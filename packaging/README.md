# Packaging

Unsigned tester artifacts for worker-dock. This folder documents download URLs, the reserved winget id, and the Azure Trusted Signing hook. It does **not** provision DNS, apt hosting, or a winget-pkgs PR.

## Artifacts

| Platform | Bundle | CI job | Notes |
| --- | --- | --- | --- |
| Ubuntu 22.04 | `.deb` | `Packaging` → `linux-deb` / `Release` → `linux-deb` | glibc baseline. Does not ship the official `agent` CLI. |
| Ubuntu 24.04 | `.deb` | `Packaging` → `linux-deb` | Same unsigned `.deb` recipe on `ubuntu-24.04`. Does not ship the official `agent` CLI. |
| Windows | WiX MSI | `Packaging` → `windows-msi` / `Release` → `windows-msi` | Built on `windows-latest` only. WebView2 `embedBootstrapper`. |

Triggers:

- `CI` on every pull request: `cargo test` (Linux + Windows) and `npm run check` / `build` (Linux).
- `Packaging` on pull requests, non-tag pushes, and `workflow_dispatch`: unsigned `.deb` on `ubuntu-22.04` and `ubuntu-24.04`, plus a WiX MSI on `windows-latest`. Uploads GitHub Actions artifacts only (`worker-dock-linux-deb-ubuntu-22.04`, `worker-dock-linux-deb-ubuntu-24.04`, `worker-dock-windows-msi`). Does **not** create a GitHub Release, sign, or publish.
- `Release` on tag `vMAJOR.MINOR.PATCH`, and on `workflow_dispatch` of that tag ref: tests, then unsigned bundles, then a **draft** GitHub Release. `workflow_dispatch` on a branch fails the stamp step.

The release tag is stamped before `tauri build`. It must match `^v\d+\.\d+\.\d+$` with each field ≤ 65535, and the tag text must be `v` plus that version (`v0.1.26` is valid; `v01.2.3` is not). The committed baseline stays `0.1.0`. Tag `v0.1.26` publishes `worker-dock_0.1.26_amd64.deb` and `worker-dock_0.1.26_x64_en-US.msi`; `latest.json` uses those names.

## Versioning

Packaging CI stamps **`0.1.<github.run_number>`** into `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and the winget `PackageVersion` fields before `tauri build`. That is a valid Windows WiX `MajorUpgrade` product version (`major.minor.build`, each field ≤ 65535). The pinned `UpgradeCode` `{33446F2E-DAB1-5268-912F-3F5D1835AF9C}` does not change. A newer run therefore upgrades an older tester `.deb` / MSI.

Release CI stamps the git tag instead of `github.run_number` (`v0.1.26` → `0.1.26`) and uploads `worker-dock_<version>_amd64.deb` and `worker-dock_<version>_x64_en-US.msi`.

The committed files stay at `0.1.0`. Stamp locally with `npm run set-version -- 0.1.N`. Packaging artifact names include the stamped version, for example `worker-dock_0.1.N_ubuntu-24.04_amd64.deb` and `worker-dock_0.1.N_x64_en-US.msi`. Settings → About and the tray tooltip show the same version.

## Public URLs (placeholders)

Configured in [`urls.toml`](urls.toml):

| Use | URL |
| --- | --- |
| Downloads | `https://get.standeasyops.com` |
| Apt repo (later) | `https://get.standeasyops.com/apt` |

Example apt line for later (repo is **not** created here):

```text
deb [signed-by=/usr/share/keyrings/worker-dock.gpg] https://get.standeasyops.com/apt ubuntu jammy main
```

`latest.json` written by the release job uses these hosts and empty signatures. The Tauri updater plugin is **not** enabled. Empty signatures must not be used for in-app updates.

## winget

Reserved id: **`StandEasyOps.WorkerDock`**.

Multi-file tester manifests live in [`winget/`](winget/) (`version`, `installer`, `defaultLocale`). The MSI is **per-machine** (`ALLUSERS=1`, manifest `Scope: machine`). Silent install/uninstall is `msiexec /qn /norestart`. `UpgradeCode` `{33446F2E-DAB1-5268-912F-3F5D1835AF9C}` is pinned in `src-tauri/tauri.conf.json`. Do **not** submit these files to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs): the MSI is unsigned and `InstallerUrl` is the placeholder host `https://get.standeasyops.com`. Packaging CI fills `InstallerSha256` / `ProductCode` into the `worker-dock-windows-msi` artifact and validates the official 1.6.0 JSON schema (`winget validate` when available).

## Signing hook (disabled)

Unsigned on purpose for this MVP slice. When Stand Easy Ops has Azure Trusted Signing:

1. Create GitHub Environment `release`.
2. Store tenant/client/secret, Trusted Signing account/profile/endpoint. The short-lived cert must never land on the runner disk.
3. Set `SIGNING_ENABLED=true` and uncomment the AzureSignTool step in `.github/workflows/release.yml`.
4. Sign the embedded exe **and** the MSI. Timestamp: `http://timestamp.acs.microsoft.com/`.
5. Apt: GPG-sign `Release` / the `.deb` in a later M2 job. Not in this workflow.
6. Then enable the Tauri updater (ed25519; cannot be disabled) against signed `latest.json`.

## Local build

```bash
# Ubuntu
npm ci
npm run tauri build -- --bundles deb

# Windows (PowerShell)
npm ci
npm run tauri build -- --bundles msi
```

The Ubuntu `.deb` includes [`deb/postinst`](deb/postinst): on `configure` it signals only the invoking desktop user's `worker-dock` GUI (`pkill -u <uid> -x worker-dock`). If the root script cannot identify that user, it leaves running windows alone. It does not stop `worker-dock-run` or restart systemd user units.

MSI cannot be built on Linux. Building MSI requires the VBSCRIPT optional Windows feature (`light.exe`).
