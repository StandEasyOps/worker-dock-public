import { asHeadline, asLiveState, type Headline } from "./worker";

export type LightColor = "green" | "amber" | "red";

export type LightInput = {
  headline: string;
  cliPresent: boolean;
  signedIn: boolean;
  installed: boolean;
  detail: string;
  liveState?: string;
};

export type Light = {
  color: LightColor;
  reason: string;
};

/** Lamp follows backend `headline`. Auto-start never decides the colour. */
export function connectionLight(input: LightInput): Light {
  const headline = asHeadline(input.headline);
  const live = input.liveState ? asLiveState(input.liveState) : null;
  if (live === "unresponsive") {
    return {
      color: "red",
      reason: oneLine(input.detail) || "Process is up but localhost probes failed.",
    };
  }
  switch (headline) {
    case "ready":
      if (live === "busy") {
        return { color: "green", reason: "Worker is live; an agent is running." };
      }
      if (live === "idle" || live === "live") {
        return { color: "green", reason: "Worker is live and idle." };
      }
      return { color: "green", reason: "Worker is running." };
    case "starting":
      return { color: "amber", reason: oneLine(input.detail) || "Worker is starting…" };
    case "stopping":
      return { color: "amber", reason: oneLine(input.detail) || "Worker is stopping…" };
    case "not_ready":
      if (!input.cliPresent) {
        return { color: "red", reason: "Official CLI is not installed." };
      }
      if (!input.signedIn) {
        return { color: "red", reason: "Not signed in." };
      }
      if (input.installed) {
        return { color: "red", reason: "Service is installed but not running." };
      }
      return { color: "red", reason: "Worker is not running." };
    default: {
      const _exhaustive: never = headline;
      return _exhaustive;
    }
  }
}

export function describeLight(headline: Headline): string {
  switch (headline) {
    case "ready":
      return "Ready";
    case "not_ready":
      return "Not ready";
    case "starting":
      return "Starting…";
    case "stopping":
      return "Stopping…";
    default: {
      const _exhaustive: never = headline;
      return _exhaustive;
    }
  }
}

function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

export function isRunningHeadline(headline: Headline): boolean {
  switch (headline) {
    case "ready":
    case "starting":
    case "stopping":
      return true;
    case "not_ready":
      return false;
    default: {
      const _exhaustive: never = headline;
      return _exhaustive;
    }
  }
}
