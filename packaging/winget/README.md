# winget manifest (tester only)

Package id: **`StandEasyOps.WorkerDock`**.

These YAML files are **not** submitted to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs). The MSI is unsigned. `InstallerUrl` is the placeholder host `https://get.standeasyops.com` (not provisioned). Testers should install the MSI from the `worker-dock-windows-msi` Actions artifact.

| File | ManifestType |
| --- | --- |
| `StandEasyOps.WorkerDock.yaml` | version |
| `StandEasyOps.WorkerDock.installer.yaml` | installer |
| `StandEasyOps.WorkerDock.locale.en-US.yaml` | defaultLocale |

## Scope

The WiX MSI is **per-machine** (`ALLUSERS=1`). The installer manifest sets `Scope: machine`. The template that said `Scope: user` was wrong for this MSI.

## Silent install / uninstall

```text
msiexec /i worker-dock_0.1.0_x64_en-US.msi /qn /norestart
msiexec /x {ProductCode} /qn /norestart
```

Packaging CI runs both on `windows-latest` and writes `ProductCode` into the artifact copy of the installer YAML.

## Upgrades

`UpgradeCode` `{33446F2E-DAB1-5268-912F-3F5D1835AF9C}` is pinned in `src-tauri/tauri.conf.json` (`bundle.windows.wix.upgradeCode`). WiX issues a new `ProductCode` per build; the Upgrade table removes earlier versions that share this UpgradeCode. Packaging CI sets `PackageVersion` to the same `0.1.<github.run_number>` stamped into the MSI `ProductVersion`.

## Validate

```bash
python3 packaging/winget/validate-manifests.py packaging/winget
```

Windows CI also runs `winget validate` when `winget` is on the runner, after filling `InstallerSha256` and `ProductCode` from the built MSI.
