export type CliStatus = {
  present: boolean;
  path: string | null;
  installedVersion: string | null;
  pinnedVersion: string;
  officialLatest: string | null;
  onPinned: boolean;
  updateAvailable: boolean;
  officialNewerUnvetted: boolean;
  autoUpdateDisabled: boolean;
  autoUpdateFlagSupported: boolean;
  archiveUrl: string;
  installScriptUrl: string;
};

export type CliProgress = {
  phase: string;
  detail: string;
};

export type CliPanelState =
  | { kind: "browser" }
  | { kind: "loading" }
  | { kind: "ready"; status: CliStatus }
  | { kind: "working"; status: CliStatus | null; progress: CliProgress | null }
  | { kind: "error"; message: string; status: CliStatus | null };

export function describePhase(phase: string): string {
  switch (phase) {
    case "preparing":
      return "Preparing";
    case "downloading":
      return "Downloading official archive";
    case "extracting":
      return "Extracting";
    case "linking":
      return "Linking agent";
    case "verifying":
      return "Verifying";
    case "done":
      return "Done";
    default: {
      const _exhaustive: string = phase;
      return _exhaustive;
    }
  }
}
