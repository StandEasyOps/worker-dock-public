export type WorkerDir = {
  path: string;
  exists: boolean;
  isDirectory: boolean;
  gitRemote?: string | null;
  hasRemote?: boolean;
};

export type Lifecycle = "stopped" | "starting" | "stopping" | "connected" | "error";

export type Headline = "ready" | "not_ready" | "starting" | "stopping";

export type LiveState =
  | "offline"
  | "starting"
  | "stopping"
  | "live"
  | "idle"
  | "busy"
  | "unresponsive"
  | "error";

export type RuntimeView = {
  state: string;
  headline: string;
  jobState: string;
  backend: string;
  jobName: string;
  installed: boolean;
  enabled: boolean;
  autostartEnabled: boolean;
  linger: string;
  wrapperPath: string;
  canEnableLinger: boolean;
  restartRequired: boolean;
  appliedName: string | null;
  appliedWorkerDirs: string[];
  canStart: boolean;
  canStop: boolean;
  canRestart: boolean;
  runMode: string;
  canStartDirect: boolean;
  canStopDirect: boolean;
  canRestartDirect: boolean;
  autostartNote: string;
  readyzOk: boolean;
  healthzOk: boolean;
  readyFromLog: boolean;
  metricsOk: boolean;
  connected: boolean | null;
  sessionActive: boolean | null;
  lastActivityUnix: number | null;
  liveState: string;
  watchdogNote: string;
  watchdogRestarts: number;
  watchdogIntended: string;
  watchdogLastRestartUnix: number | null;
  detail: string;
};

export type RecentAgent = {
  bcId: string;
  claimedAt: string;
  url: string;
};

export type WorkerView = {
  id: string;
  name: string;
  workerDirs: WorkerDir[];
  managementPort: number;
  idleReleaseTimeout: number;
  dataDir: string;
  agentPath: string;
  startArgv: string[];
  commandLine: string;
  runtime: RuntimeView;
  recentAgents: RecentAgent[];
  currentAgentBcId: string | null;
  canResetIdentity: boolean;
};

export type WorkerSnapshot = {
  configured: boolean;
  extraWorkerCount: number;
  suggestedName: string;
  configPath: string;
  worker: WorkerView | null;
};

export type StartCommand = {
  agentPath: string;
  startArgv: string[];
  commandLine: string;
};

export type WorkerPanelState =
  | { kind: "browser" }
  | { kind: "loading" }
  | { kind: "ready"; snapshot: WorkerSnapshot }
  | { kind: "working"; snapshot: WorkerSnapshot | null }
  | { kind: "error"; message: string; snapshot: WorkerSnapshot | null };

export function asLifecycle(state: string): Lifecycle {
  switch (state) {
    case "stopped":
    case "starting":
    case "stopping":
    case "connected":
    case "error":
      return state;
    case "running":
      return "connected";
    case "failed":
      return "error";
    default:
      return "stopped";
  }
}

export function asHeadline(raw: string): Headline {
  switch (raw) {
    case "ready":
    case "not_ready":
    case "starting":
    case "stopping":
      return raw;
    default:
      return "not_ready";
  }
}

export function describeHeadline(headline: string): string {
  const value = asHeadline(headline);
  switch (value) {
    case "ready":
      return "Ready";
    case "not_ready":
      return "Not ready";
    case "starting":
      return "Starting…";
    case "stopping":
      return "Stopping…";
    default: {
      const _exhaustive: never = value;
      return _exhaustive;
    }
  }
}

export function asLiveState(raw: string): LiveState {
  switch (raw) {
    case "offline":
    case "starting":
    case "stopping":
    case "live":
    case "idle":
    case "busy":
    case "unresponsive":
    case "error":
      return raw;
    default:
      return "offline";
  }
}

export function describeLiveState(raw: string): string {
  const state = asLiveState(raw);
  switch (state) {
    case "offline":
      return "Offline";
    case "starting":
      return "Starting…";
    case "stopping":
      return "Stopping…";
    case "live":
      return "Live";
    case "idle":
      return "Live · idle";
    case "busy":
      return "Live · agent running";
    case "unresponsive":
      return "Unresponsive";
    case "error":
      return "Error";
    default: {
      const _exhaustive: never = state;
      return _exhaustive;
    }
  }
}

export function describeLifecycle(state: string): string {
  const lifecycle = asLifecycle(state);
  switch (lifecycle) {
    case "stopped":
      return "Stopped";
    case "starting":
      return "Starting";
    case "stopping":
      return "Stopping";
    case "connected":
      return "Connected";
    case "error":
      return "Error";
    default: {
      const _exhaustive: never = lifecycle;
      return _exhaustive;
    }
  }
}

export function describeBackend(backend: string): string {
  switch (backend) {
    case "systemd-user":
      return "systemd user unit";
    case "scheduled-task":
      return "interactive-user scheduled task";
    case "direct":
      return "in-app child process";
    default: {
      const _exhaustive: string = backend;
      return backend || "per-user service";
    }
  }
}

export function folderHint(folder: WorkerDir): string {
  if (!folder.exists) {
    return "Missing";
  }
  if (!folder.isDirectory) {
    return "Not a folder";
  }
  if (folder.hasRemote === false) {
    return "No git remote";
  }
  return "Ready";
}

export function sameFolders(left: string[], right: string[]): boolean {
  if (left.length !== right.length) {
    return false;
  }
  return left.every((path, index) => path === right[index]);
}

export function emptyRuntime(): RuntimeView {
  return {
    state: "stopped",
    headline: "not_ready",
    jobState: "missing",
    backend: "systemd-user",
    jobName: "",
    installed: false,
    enabled: false,
    autostartEnabled: false,
    linger: "disabled",
    wrapperPath: "",
    canEnableLinger: true,
    restartRequired: false,
    appliedName: null,
    appliedWorkerDirs: [],
    canStart: false,
    canStop: false,
    canRestart: false,
    runMode: "none",
    canStartDirect: false,
    canStopDirect: false,
    canRestartDirect: false,
    autostartNote:
      "The systemd user unit starts at login when enabled. Start-at-boot without an open session needs linger (loginctl enable-linger).",
    readyzOk: false,
    healthzOk: false,
    readyFromLog: false,
    metricsOk: false,
    connected: null,
    sessionActive: null,
    lastActivityUnix: null,
    liveState: "offline",
    watchdogNote: "",
    watchdogRestarts: 0,
    watchdogIntended: "none",
    watchdogLastRestartUnix: null,
    detail: "",
  };
}
