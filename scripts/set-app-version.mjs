#!/usr/bin/env node
/**
 * Stamp or verify the worker-dock app version across the files that must stay
 * in lockstep: package.json, package-lock.json, Cargo.toml, Cargo.lock,
 * tauri.conf.json, and the winget tester manifests.
 *
 * Packaging CI calls this with 0.1.<github.run_number> so each .deb / MSI
 * upgrades the previous one. The committed baseline stays 0.1.0.
 *
 * Windows WiX MajorUpgrade accepts only major.minor.build, each field <= 65535.
 */

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const MSI_FIELD_MAX = 65535;
const VERSION_RE = /^(\d+)\.(\d+)\.(\d+)$/;

const PATHS = {
  packageJson: "package.json",
  packageLock: "package-lock.json",
  cargoToml: "src-tauri/Cargo.toml",
  cargoLock: "src-tauri/Cargo.lock",
  tauriConf: "src-tauri/tauri.conf.json",
  wingetVersion: "packaging/winget/StandEasyOps.WorkerDock.yaml",
  wingetInstaller: "packaging/winget/StandEasyOps.WorkerDock.installer.yaml",
  wingetLocale: "packaging/winget/StandEasyOps.WorkerDock.locale.en-US.yaml",
};

export function parseMsiProductVersion(raw) {
  const match = VERSION_RE.exec(String(raw).trim());
  if (!match) {
    throw new Error(
      `version must be major.minor.build (Windows MajorUpgrade), got ${JSON.stringify(raw)}`,
    );
  }
  const parts = match.slice(1).map((part) => Number(part));
  if (parts.some((value) => !Number.isInteger(value) || value > MSI_FIELD_MAX)) {
    throw new Error(`each version field must be an integer <= ${MSI_FIELD_MAX}, got ${raw}`);
  }
  return { major: parts[0], minor: parts[1], build: parts[2], text: `${parts[0]}.${parts[1]}.${parts[2]}` };
}

function read(rel) {
  return readFileSync(join(ROOT, rel), "utf8");
}

function write(rel, text) {
  writeFileSync(join(ROOT, rel), text.endsWith("\n") ? text : `${text}\n`);
}

function firstCargoPackageVersion(text) {
  const match = /^version = "([^"]+)"/m.exec(text);
  if (!match) {
    throw new Error("Cargo.toml is missing a package version");
  }
  return match[1];
}

const CARGO_LOCK_PACKAGE = /name = "worker-dock"\r?\nversion = "([^"]+)"/;

function cargoLockPackageVersion(text) {
  const match = CARGO_LOCK_PACKAGE.exec(text);
  if (!match) {
    throw new Error("Cargo.lock is missing the worker-dock package version");
  }
  return match[1];
}

function stampCargoLock(text, version) {
  if (!CARGO_LOCK_PACKAGE.test(text)) {
    throw new Error("Cargo.lock is missing the worker-dock package version");
  }
  return text.replace(CARGO_LOCK_PACKAGE, (block) =>
    block.replace(/version = "[^"]+"/, `version = "${version}"`),
  );
}

function yamlPackageVersion(text) {
  const match = /^PackageVersion:\s*(\S+)/m.exec(text);
  if (!match) {
    throw new Error("winget YAML is missing PackageVersion");
  }
  return match[1];
}

function collectVersions() {
  const packageJson = JSON.parse(read(PATHS.packageJson));
  const packageLock = JSON.parse(read(PATHS.packageLock));
  return {
    [PATHS.packageJson]: packageJson.version,
    [PATHS.packageLock]: packageLock.version,
    "package-lock.json#packages[\"\"]": packageLock.packages?.[""]?.version,
    [PATHS.cargoToml]: firstCargoPackageVersion(read(PATHS.cargoToml)),
    [PATHS.cargoLock]: cargoLockPackageVersion(read(PATHS.cargoLock)),
    [PATHS.tauriConf]: JSON.parse(read(PATHS.tauriConf)).version,
    [PATHS.wingetVersion]: yamlPackageVersion(read(PATHS.wingetVersion)),
    [PATHS.wingetInstaller]: yamlPackageVersion(read(PATHS.wingetInstaller)),
    [PATHS.wingetLocale]: yamlPackageVersion(read(PATHS.wingetLocale)),
  };
}

export function checkSync(expected) {
  const versions = collectVersions();
  const unique = [...new Set(Object.values(versions))];
  if (unique.length !== 1) {
    const detail = Object.entries(versions)
      .map(([file, version]) => `  ${file}: ${version}`)
      .join("\n");
    throw new Error(`app version files are out of sync:\n${detail}`);
  }
  const current = unique[0];
  parseMsiProductVersion(current);
  if (expected && current !== expected) {
    throw new Error(`expected app version ${expected}, found ${current}`);
  }
  return current;
}

function setJsonVersion(rel, version) {
  const data = JSON.parse(read(rel));
  data.version = version;
  write(rel, `${JSON.stringify(data, null, 2)}\n`);
}

function stampWinget(text, version) {
  return text
    .replace(/^PackageVersion:\s*\S+/m, `PackageVersion: ${version}`)
    .replace(/worker-dock_\d+\.\d+\.\d+_x64_en-US\.msi/g, `worker-dock_${version}_x64_en-US.msi`)
    .replace(/releases\/v\d+\.\d+\.\d+\//g, `releases/v${version}/`);
}

export function setAppVersion(raw) {
  const { text: version } = parseMsiProductVersion(raw);
  setJsonVersion(PATHS.packageJson, version);

  const lock = JSON.parse(read(PATHS.packageLock));
  lock.version = version;
  if (lock.packages?.[""]) {
    lock.packages[""].version = version;
  }
  write(PATHS.packageLock, `${JSON.stringify(lock, null, 2)}\n`);

  write(PATHS.cargoToml, read(PATHS.cargoToml).replace(/^version = "[^"]+"/m, `version = "${version}"`));
  write(PATHS.cargoLock, stampCargoLock(read(PATHS.cargoLock), version));

  const tauri = JSON.parse(read(PATHS.tauriConf));
  tauri.version = version;
  write(PATHS.tauriConf, `${JSON.stringify(tauri, null, 2)}\n`);

  for (const rel of [PATHS.wingetVersion, PATHS.wingetInstaller, PATHS.wingetLocale]) {
    write(rel, stampWinget(read(rel), version));
  }
  return checkSync(version);
}

function selfTest() {
  const ok = parseMsiProductVersion("0.1.42");
  if (ok.text !== "0.1.42" || ok.build !== 42) {
    throw new Error("parseMsiProductVersion failed on 0.1.42");
  }
  const rejects = ["0.1", "0.1.2.3", "v0.1.2", "0.1.65536", "latest", "0.1.0-beta"];
  for (const value of rejects) {
    let threw = false;
    try {
      parseMsiProductVersion(value);
    } catch {
      threw = true;
    }
    if (!threw) {
      throw new Error(`expected rejection for ${value}`);
    }
  }
  const crlf = 'name = "worker-dock"\r\nversion = "0.1.0"\r\n';
  if (cargoLockPackageVersion(crlf) !== "0.1.0") {
    throw new Error("Cargo.lock parser must accept CRLF (Windows checkout)");
  }
  const stamped = stampCargoLock(crlf, "0.1.9");
  if (!stamped.includes("version = \"0.1.9\"") || !stamped.includes("\r\n")) {
    throw new Error("Cargo.lock stamp must keep CRLF and update the version");
  }
  const current = checkSync();
  console.log(`self-test ok; committed version ${current}`);
}

function usage() {
  console.log(`Usage:
  node scripts/set-app-version.mjs <major.minor.build>
  node scripts/set-app-version.mjs --check-sync [expected]
  node scripts/set-app-version.mjs --self-test`);
}

const args = process.argv.slice(2);
try {
  if (args[0] === "--self-test") {
    selfTest();
  } else if (args[0] === "--check-sync") {
    const current = checkSync(args[1]);
    console.log(`app version in sync: ${current}`);
  } else if (args[0] === "-h" || args[0] === "--help" || args.length === 0) {
    usage();
    if (args.length === 0) {
      process.exit(1);
    }
  } else if (args[0].startsWith("-")) {
    throw new Error(`unknown flag ${args[0]}`);
  } else {
    const version = setAppVersion(args[0]);
    console.log(`stamped app version ${version}`);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
