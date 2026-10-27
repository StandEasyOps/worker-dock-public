export type LogSourceName = "wrapper" | "session" | "startup";

export type LogSourceView = {
  name: string;
  path: string;
  exists: boolean;
  text: string;
  truncated: boolean;
};

export type LogsSnapshot = {
  folder: string;
  sources: LogSourceView[];
};

export type LogsPanelState =
  | { kind: "browser" }
  | { kind: "loading" }
  | { kind: "ready"; snapshot: LogsSnapshot }
  | { kind: "error"; message: string; snapshot: LogsSnapshot | null };

export function asLogSourceName(name: string): LogSourceName {
  switch (name) {
    case "wrapper":
    case "session":
    case "startup":
      return name;
    default:
      return "wrapper";
  }
}

export function describeLogSource(name: string): string {
  const source = asLogSourceName(name);
  switch (source) {
    case "wrapper":
      return "Wrapper";
    case "session":
      return "CLI session";
    case "startup":
      return "Startup";
    default: {
      const _exhaustive: never = source;
      return _exhaustive;
    }
  }
}

export function copyPayload(snapshot: LogsSnapshot): string {
  return snapshot.sources
    .map((source) => {
      const heading = `${describeLogSource(source.name)} (${source.path})`;
      if (!source.exists) {
        return `${heading}\n(not present)`;
      }
      if (!source.text.trim()) {
        return `${heading}\n(empty)`;
      }
      return `${heading}\n${source.text}`;
    })
    .join("\n\n");
}
