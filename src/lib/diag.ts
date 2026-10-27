export type HostClass = "required" | "artifact" | "optional";

export type HostProbe = {
  host: string;
  role: string;
  class: string;
  why: string;
  ok: boolean;
  stage: string;
  detail: string;
  statusCode: number | null;
};

export type CheckView = {
  name: string;
  ok: boolean;
  required: boolean;
  detail: string;
};

export type DebugView = {
  ran: boolean;
  ok: boolean;
  skipped: string | null;
  exitCode: number | null;
  authMethod: string | null;
  workerId: string | null;
  visibility: string | null;
  computerUse: string | null;
  desktopShare: string | null;
  warnings: string[];
  text: string;
  json: string | null;
};

export type DiagSnapshot = {
  cliPresent: boolean;
  signedIn: boolean;
  proxy: string | null;
  canStartNetwork: boolean;
  summary: string;
  hosts: HostProbe[];
  gitBash: CheckView | null;
  debug: DebugView;
};

export type DiagPanelState =
  | { kind: "browser" }
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "ready"; snapshot: DiagSnapshot }
  | { kind: "error"; message: string; snapshot: DiagSnapshot | null };

export function asHostClass(value: string): HostClass {
  switch (value) {
    case "required":
    case "artifact":
    case "optional":
      return value;
    default:
      return "optional";
  }
}

export function describeHostClass(value: string): string {
  const hostClass = asHostClass(value);
  switch (hostClass) {
    case "required":
      return "Required";
    case "artifact":
      return "Artifact";
    case "optional":
      return "Optional";
    default: {
      const _exhaustive: never = hostClass;
      return _exhaustive;
    }
  }
}

export function describeProbeState(ok: boolean): string {
  switch (ok) {
    case true:
      return "Reachable";
    case false:
      return "Failed";
    default: {
      const _exhaustive: never = ok;
      return _exhaustive;
    }
  }
}
