/** Display quoting only. The service execs `startArgv` as tokens, never this string. */
export function quoteArg(arg: string): string {
  if (arg.length > 0 && /^[A-Za-z0-9_./:=-]+$/.test(arg)) {
    return arg;
  }
  return `'${arg.replace(/'/g, `'\\''`)}'`;
}

export function formatCommandLine(agentPath: string, startArgv: string[]): string {
  return [agentPath || "agent", ...startArgv].map(quoteArg).join(" ");
}

/**
 * Live preview matching `personal_start_command` / worker_get `commandLine`.
 * Worker flags (`--name`, each `--worker-dir`) sit **before** `start`.
 * Never `agent worker start --name …`. Never `--pool` / `--api-key`.
 */
export function previewStartCommand(input: {
  agentPath: string;
  name: string;
  dirs: string[];
  dataDir: string;
  managementPort: number;
  idleReleaseTimeout: number;
}): { startArgv: string[]; commandLine: string } {
  const name = input.name.trim();
  const dirs = input.dirs.map((dir) => dir.trim()).filter(Boolean);
  if (!name || dirs.length === 0) {
    return { startArgv: [], commandLine: "" };
  }
  const startArgv = ["--disable-auto-update", "worker", "--name", name];
  for (const dir of dirs) {
    startArgv.push("--worker-dir", dir);
  }
  if (input.dataDir) {
    startArgv.push("--data-dir", input.dataDir);
  }
  startArgv.push("--idle-release-timeout", String(input.idleReleaseTimeout));
  startArgv.push("--management-addr", `127.0.0.1:${input.managementPort || 18741}`);
  startArgv.push("start", "--verbose");
  return {
    startArgv,
    commandLine: formatCommandLine(input.agentPath || "agent", startArgv),
  };
}

export function isNameBeforeStart(startArgv: string[]): boolean {
  const nameAt = startArgv.indexOf("--name");
  const startAt = startArgv.indexOf("start");
  return nameAt >= 0 && startAt >= 0 && nameAt < startAt;
}
