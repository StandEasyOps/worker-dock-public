#!/usr/bin/env python3
"""Validate StandEasyOps.WorkerDock winget YAML against official 1.6.0 schemas.

Does not submit to winget-pkgs. Used by Packaging CI.
"""

from __future__ import annotations

import argparse
import hashlib
import sys
from pathlib import Path

import jsonschema
import yaml

SCHEMA_DIR = Path(__file__).resolve().parent / "schema"
MANIFESTS = {
    "version": ("StandEasyOps.WorkerDock.yaml", "manifest.version.1.6.0.json"),
    "installer": (
        "StandEasyOps.WorkerDock.installer.yaml",
        "manifest.installer.1.6.0.json",
    ),
    "defaultLocale": (
        "StandEasyOps.WorkerDock.locale.en-US.yaml",
        "manifest.defaultLocale.1.6.0.json",
    ),
}

PINNED_UPGRADE_CODE = "{33446F2E-DAB1-5268-912F-3F5D1835AF9C}"


def load_yaml(path: Path) -> dict:
    with path.open(encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        raise SystemExit(f"{path}: expected a mapping")
    return data


def validate_file(manifest_dir: Path, kind: str) -> dict:
    name, schema_name = MANIFESTS[kind]
    path = manifest_dir / name
    schema_path = SCHEMA_DIR / schema_name
    data = load_yaml(path)
    with schema_path.open(encoding="utf-8") as handle:
        schema = yaml.safe_load(handle) if schema_path.suffix == ".yaml" else None
    if schema is None:
        import json

        schema = json.loads(schema_path.read_text(encoding="utf-8"))
    jsonschema.validate(instance=data, schema=schema)
    print(f"OK schema {kind}: {path}")
    return data


def check_msi_product_version(ver: str) -> None:
    parts = str(ver).split(".")
    if len(parts) != 3 or not all(part.isdigit() for part in parts):
        raise SystemExit(f"PackageVersion must be major.minor.build, got {ver}")
    if any(int(part) > 65535 for part in parts):
        raise SystemExit(f"PackageVersion field exceeds 65535: {ver}")


def check_consistency(version: dict, installer: dict, locale: dict) -> None:
    ident = version["PackageIdentifier"]
    ver = version["PackageVersion"]
    check_msi_product_version(ver)
    if ident != "StandEasyOps.WorkerDock":
        raise SystemExit(f"PackageIdentifier must be StandEasyOps.WorkerDock, got {ident}")
    for label, doc in (("installer", installer), ("locale", locale)):
        if doc.get("PackageIdentifier") != ident:
            raise SystemExit(f"{label} PackageIdentifier mismatch")
        if doc.get("PackageVersion") != ver:
            raise SystemExit(f"{label} PackageVersion mismatch")
    if installer.get("Scope") != "machine":
        raise SystemExit("installer Scope must be machine (MSI ALLUSERS=1)")
    switches = installer.get("InstallerSwitches") or {}
    silent = str(switches.get("Silent") or "")
    if "/qn" not in silent:
        raise SystemExit("InstallerSwitches.Silent must include /qn")
    entries = (installer.get("Installers") or [{}])[0].get("AppsAndFeaturesEntries") or []
    upgrade = None
    if entries:
        upgrade = entries[0].get("UpgradeCode")
    if (upgrade or "").upper() != PINNED_UPGRADE_CODE:
        raise SystemExit(f"UpgradeCode must be {PINNED_UPGRADE_CODE}, got {upgrade}")
    display = entries[0].get("DisplayName") if entries else None
    if display != "worker-dock":
        raise SystemExit(f"AppsAndFeatures DisplayName must be worker-dock, got {display}")
    if locale.get("PackageName") != "worker-dock":
        raise SystemExit(f"PackageName must stay worker-dock, got {locale.get('PackageName')}")
    print("OK consistency: id, version, machine scope, /qn, UpgradeCode, display name")


def check_msi_sha256(installer: dict, msi: Path) -> None:
    digest = hashlib.sha256(msi.read_bytes()).hexdigest().upper()
    listed = (installer.get("Installers") or [{}])[0].get("InstallerSha256", "")
    if listed.upper() != digest:
        raise SystemExit(
            f"InstallerSha256 {listed} does not match {msi} ({digest})"
        )
    print(f"OK InstallerSha256 matches {msi.name}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "manifest_dir",
        nargs="?",
        default=str(Path(__file__).resolve().parent),
    )
    parser.add_argument("--msi", type=Path, help="Require InstallerSha256 to match this MSI")
    args = parser.parse_args()
    manifest_dir = Path(args.manifest_dir)
    version = validate_file(manifest_dir, "version")
    installer = validate_file(manifest_dir, "installer")
    locale = validate_file(manifest_dir, "defaultLocale")
    check_consistency(version, installer, locale)
    if args.msi:
        check_msi_sha256(installer, args.msi)
    return 0


if __name__ == "__main__":
    sys.exit(main())
