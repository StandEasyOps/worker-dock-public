export type AuthMethod = "cli_login" | "api_key";

export type AuthStatus = {
  cliPresent: boolean;
  signedIn: boolean;
  method: AuthMethod | null;
  userEmail: string | null;
  hasApiKey: boolean;
  isAuthenticated: boolean;
  message: string | null;
  browserLoginActive: boolean;
};

export type AuthProgress = {
  phase: string;
  detail: string;
  loginUrl?: string | null;
};

export type AuthPanelState =
  | { kind: "browser" }
  | { kind: "loading" }
  | { kind: "ready"; status: AuthStatus }
  | { kind: "waiting"; status: AuthStatus | null; progress: AuthProgress | null }
  | { kind: "working"; status: AuthStatus | null }
  | { kind: "error"; message: string; status: AuthStatus | null };

export function describeAuthPhase(phase: string): string {
  switch (phase) {
    case "starting":
      return "Starting official CLI login";
    case "waiting_for_url":
      return "Waiting for the sign-in URL";
    case "opening_browser":
      return "Opening the sign-in page";
    case "waiting_for_browser":
      return "Waiting for browser";
    case "done":
      return "Signed in";
    case "cancelled":
      return "Cancelled";
    default: {
      const _exhaustive: string = phase;
      return _exhaustive;
    }
  }
}

export function describeAuthMethod(method: AuthMethod | null): string {
  switch (method) {
    case "cli_login":
      return "Browser (official CLI store)";
    case "api_key":
      return "Personal API key (OS keychain)";
    case null:
      return "—";
    default: {
      const _exhaustive: never = method;
      return _exhaustive;
    }
  }
}

export function asAuthMethod(value: string | null | undefined): AuthMethod | null {
  switch (value) {
    case "cli_login":
    case "api_key":
      return value;
    default:
      return null;
  }
}
