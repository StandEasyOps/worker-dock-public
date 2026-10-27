import type { AuthStatus } from "./auth";
import type { CliStatus } from "./cli";
import type { DiagSnapshot } from "./diag";
import type { LogsSnapshot } from "./logs";
import { emptyRuntime, type WorkerSnapshot, type WorkerView } from "./worker";

export type PreviewMode =
  | "connected"
  | "ready"
  | "not-ready"
  | "amber"
  | "unsigned"
  | "error"
  | "empty"
  | "settings"
  | "logs"
  | "checks"
  | "name"
  | "folders-two"
  | "service-missing"
  | "service-stopped"
  | "service-autostart"
  | "direct"
  | "recent-agents";

export type PreviewBundle = {
  mode: PreviewMode;
  view: "home" | "settings";
  drawer: "closed" | "logs" | "checks";
  focusName: boolean;
  cli: CliStatus;
  auth: AuthStatus;
  worker: WorkerSnapshot;
  logs: LogsSnapshot;
  diag: DiagSnapshot;
};

const PINNED = "2026.10.19-6bbf2c4";

const CLI_OK: CliStatus = {
  present: true,
  path: "/home/dev/.local/bin/agent",
  installedVersion: PINNED,
  pinnedVersion: PINNED,
  officialLatest: PINNED,
  onPinned: true,
  updateAvailable: false,
  officialNewerUnvetted: false,
  autoUpdateDisabled: true,
  autoUpdateFlagSupported: true,
  archiveUrl: "https://downloads.cursor.com/lab/2026.10.19-6bbf2c4/linux/x64/agent-cli-package.tar.gz",
  installScriptUrl: "https://cursor.com/install",
};

const AUTH_IN: AuthStatus = {
  cliPresent: true,
  signedIn: true,
  method: "cli_login",
  userEmail: "dev@example.com",
  hasApiKey: false,
  isAuthenticated: true,
  message: null,
  browserLoginActive: false,
};

const AUTH_OUT: AuthStatus = {
  ...AUTH_IN,
  signedIn: false,
  method: null,
  userEmail: null,
  isAuthenticated: false,
  message: "Not logged in",
};

function foldersTwo(): WorkerView["workerDirs"] {
  return [
    {
      path: "/home/dev/repos/app",
      exists: true,
      isDirectory: true,
      gitRemote: "git@github.com:acme/app.git",
      hasRemote: true,
    },
    {
      path: "/home/dev/repos/infra",
      exists: true,
      isDirectory: true,
      gitRemote: "git@github.com:acme/infra.git",
      hasRemote: true,
    },
  ];
}

function startArgv(name: string, dirs: string[]): string[] {
  const argv = ["--disable-auto-update", "worker", "--name", name];
  for (const dir of dirs) {
    argv.push("--worker-dir", dir);
  }
  argv.push(
    "--data-dir",
    "/home/dev/.local/share/worker-dock/workers/8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60",
    "--idle-release-timeout",
    "0",
    "--management-addr",
    "127.0.0.1:18741",
    "start",
    "--verbose",
  );
  return argv;
}

function commandLine(name: string, dirs: string[]): string {
  return ["/home/dev/.local/bin/agent", ...startArgv(name, dirs)].join(" ");
}

function workerView(partial: Partial<WorkerView>): WorkerView {
  const name = partial.name ?? "my-devbox";
  const dirs = (partial.workerDirs ?? foldersTwo()).map((folder) => folder.path);
  const argv = partial.startArgv ?? startArgv(name, dirs);
  const line = partial.commandLine ?? commandLine(name, dirs);
  const base: WorkerView = {
    id: "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60",
    name,
    workerDirs: partial.workerDirs ?? foldersTwo(),
    managementPort: 18741,
    idleReleaseTimeout: 0,
    dataDir: "/home/dev/.local/share/worker-dock/workers/8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60",
    agentPath: "/home/dev/.local/bin/agent",
    startArgv: argv,
    commandLine: line,
    runtime: emptyRuntime(),
    recentAgents: [],
    currentAgentBcId: null,
    canResetIdentity: true,
  };
  return { ...base, ...partial, name, startArgv: argv, commandLine: line };
}

function snapshot(worker: WorkerView | null): WorkerSnapshot {
  return {
    configured: worker !== null,
    extraWorkerCount: 0,
    suggestedName: "dev-machine",
    configPath: "/home/dev/.config/worker-dock/config.toml",
    worker,
  };
}

const LOGS: LogsSnapshot = {
  folder: "/home/dev/.local/share/worker-dock/workers/8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60",
  sources: [
    {
      name: "wrapper",
      path: "/home/dev/.local/share/worker-dock/workers/8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60/wrapper.log",
      exists: true,
      text: "starting /home/dev/.local/bin/agent --disable-auto-update worker --name my-devbox --worker-dir /home/dev/repos/app --worker-dir /home/dev/repos/infra start --verbose\nINFO Starting in worker mode\nPrivate worker started",
      truncated: false,
    },
    {
      name: "session",
      path: "/tmp/cursor-agent-logs-1000/latest.log",
      exists: true,
      text: "bridgeUrl: https://api2.cursor.sh\nready",
      truncated: false,
    },
  ],
};

const DIAG_OK: DiagSnapshot = {
  cliPresent: true,
  signedIn: true,
  proxy: null,
  canStartNetwork: true,
  summary: "Required hosts reachable. Official worker debug completed.",
  hosts: [
    {
      host: "api2.cursor.sh",
      role: "session",
      class: "required",
      why: "Agent session",
      ok: true,
      stage: "https",
      detail: "200",
      statusCode: 200,
    },
    {
      host: "downloads.cursor.com",
      role: "packages",
      class: "required",
      why: "CLI packages",
      ok: true,
      stage: "https",
      detail: "200",
      statusCode: 200,
    },
  ],
  gitBash: null,
  debug: {
    ran: true,
    ok: true,
    skipped: null,
    exitCode: 0,
    authMethod: "cli_login",
    workerId: "pw_123",
    visibility: "visible",
    computerUse: "off",
    desktopShare: "off",
    warnings: [],
    text: "ok",
    json: "{}",
  },
};

const RECENT_AGENTS = [
  {
    bcId: "bc-00000000-0000-4000-8000-000000000001",
    claimedAt: "2026-10-21T13:42:07.123Z",
    url: "https://cursor.com/agents/bc-00000000-0000-4000-8000-000000000001",
  },
  {
    bcId: "bc-aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
    claimedAt: "2026-10-21T11:02:00.000Z",
    url: "https://cursor.com/agents/bc-aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
  },
  {
    bcId: "bc-11111111-1111-4111-8111-111111111111",
    claimedAt: "",
    url: "https://cursor.com/agents/bc-11111111-1111-4111-8111-111111111111",
  },
];

const CONNECTED_RUNTIME = {
  ...emptyRuntime(),
  state: "connected",
  headline: "ready",
  jobState: "running",
  jobName: "worker-dock@8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60.service",
  installed: true,
  enabled: true,
  autostartEnabled: true,
  linger: "enabled",
  wrapperPath: "/home/dev/.local/bin/worker-dock-run",
  canEnableLinger: false,
  restartRequired: false,
  appliedName: "my-devbox",
  appliedWorkerDirs: ["/home/dev/repos/app", "/home/dev/repos/infra"],
  canStart: false,
  canStop: true,
  canRestart: true,
  runMode: "service",
  canStartDirect: false,
  canStopDirect: false,
  canRestartDirect: false,
  readyzOk: true,
  healthzOk: true,
  readyFromLog: true,
  metricsOk: true,
  connected: true,
  sessionActive: false,
  lastActivityUnix: 1726000000,
  liveState: "idle",
  watchdogNote: "",
  watchdogRestarts: 0,
  watchdogIntended: "service",
  watchdogLastRestartUnix: null,
  detail: "Worker is live and idle (localhost /metrics).",
};

export function parsePreviewMode(raw: string | null): PreviewMode | null {
  switch (raw) {
    case "connected":
    case "ready":
    case "not-ready":
    case "amber":
    case "direct":
    case "unsigned":
    case "error":
    case "empty":
    case "settings":
    case "logs":
    case "checks":
    case "name":
    case "folders-two":
    case "service-missing":
    case "service-stopped":
    case "service-autostart":
    case "recent-agents":
      return raw;
    case "not-signed-in":
      return "unsigned";
    case "empty-folders":
      return "empty";
    default:
      return null;
  }
}

export function previewBundle(mode: PreviewMode): PreviewBundle {
  const connectedWorker = workerView({
    runtime: CONNECTED_RUNTIME,
    recentAgents: RECENT_AGENTS,
    currentAgentBcId: RECENT_AGENTS[0].bcId,
    canResetIdentity: true,
  });
  const base: PreviewBundle = {
    mode,
    view: "home",
    drawer: "closed",
    focusName: false,
    cli: CLI_OK,
    auth: AUTH_IN,
    worker: snapshot(connectedWorker),
    logs: LOGS,
    diag: DIAG_OK,
  };
  switch (mode) {
    case "connected":
    case "ready":
    case "folders-two":
    case "service-autostart":
      return base;
    case "not-ready":
      return {
        ...base,
        worker: snapshot(
          workerView({
            runtime: {
              ...emptyRuntime(),
              installed: false,
              jobState: "missing",
              canStartDirect: true,
              detail: "",
            },
          }),
        ),
      };
    case "direct":
      return {
        ...base,
        worker: snapshot(
          workerView({
            runtime: {
              ...CONNECTED_RUNTIME,
              headline: "ready",
              installed: false,
              enabled: false,
              autostartEnabled: false,
              jobName: "",
              jobState: "running",
              linger: "disabled",
              canEnableLinger: true,
              backend: "direct",
              runMode: "direct",
              canStart: false,
              canStop: false,
              canRestart: false,
              canStartDirect: false,
              canStopDirect: true,
              canRestartDirect: true,
              detail: "Localhost /readyz returned 200.",
            },
          }),
        ),
      };
    case "amber":
      return {
        ...base,
        worker: snapshot(
          workerView({
            runtime: {
              ...CONNECTED_RUNTIME,
              state: "starting",
              headline: "starting",
              jobState: "running",
              readyzOk: false,
              healthzOk: false,
              readyFromLog: false,
              metricsOk: false,
              connected: null,
              sessionActive: null,
              liveState: "starting",
              detail: "Process is up; waiting for localhost /readyz or /metrics.",
            },
          }),
        ),
      };
    case "name":
      return { ...base, focusName: true };
    case "unsigned":
      return {
        ...base,
        auth: AUTH_OUT,
        worker: snapshot(
          workerView({
            runtime: {
              ...emptyRuntime(),
              installed: false,
              jobState: "missing",
            },
          }),
        ),
      };
    case "error":
      return {
        ...base,
        worker: snapshot(
          workerView({
            runtime: {
              ...CONNECTED_RUNTIME,
              state: "error",
              headline: "not_ready",
              jobState: "failed",
              canStart: false,
              canStop: false,
              canRestart: true,
              readyzOk: false,
              healthzOk: false,
              readyFromLog: false,
              detail: "Authentication required for worker mode.",
            },
          }),
        ),
      };
    case "empty":
      return {
        ...base,
        worker: snapshot(
          workerView({
            name: "my-devbox",
            workerDirs: [],
            startArgv: [],
            commandLine: "",
            runtime: {
              ...emptyRuntime(),
              installed: false,
              jobState: "missing",
              canStartDirect: true,
            },
          }),
        ),
      };
    case "settings":
      return { ...base, view: "settings" };
    case "logs":
      return { ...base, drawer: "logs" };
    case "checks":
      return { ...base, drawer: "checks" };
    case "service-missing":
      return {
        ...base,
        worker: snapshot(
          workerView({
            runtime: {
              ...emptyRuntime(),
              installed: false,
              jobState: "missing",
              canStartDirect: true,
              detail: "",
            },
          }),
        ),
      };
    case "service-stopped":
      return {
        ...base,
        worker: snapshot(
          workerView({
            runtime: {
              ...CONNECTED_RUNTIME,
              state: "stopped",
              headline: "not_ready",
              jobState: "stopped",
              installed: true,
              enabled: false,
              autostartEnabled: false,
              canStart: true,
              canStop: false,
              canRestart: false,
              runMode: "none",
              canStartDirect: true,
              canStopDirect: false,
              readyzOk: false,
              healthzOk: false,
              readyFromLog: false,
              linger: "disabled",
              canEnableLinger: true,
              detail: "",
            },
          }),
        ),
      };
    case "recent-agents":
      return {
        ...base,
        worker: snapshot(
          workerView({
            runtime: {
              ...CONNECTED_RUNTIME,
              sessionActive: true,
              liveState: "busy",
              detail: "Localhost /metrics reports an active agent session.",
            },
            recentAgents: RECENT_AGENTS,
            currentAgentBcId: RECENT_AGENTS[0].bcId,
          }),
        ),
      };
    default: {
      const _exhaustive: never = mode;
      return _exhaustive;
    }
  }
}
