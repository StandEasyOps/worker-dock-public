<script lang="ts">
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onDestroy, onMount } from "svelte";
  import { asAuthMethod, type AuthProgress, type AuthStatus } from "./auth";
  import { type CliProgress, type CliStatus } from "./cli";
  import { previewStartCommand } from "./command";
  import type { DiagSnapshot } from "./diag";
  import Drawer from "./Drawer.svelte";
  import Home from "./Home.svelte";
  import { DISPLAY_NAME } from "./brand";
  import {
    CURSOR_AGENTS_URL,
    CURSOR_DOCS_URL,
    STANDEASYOPS_URL,
    agentChatUrl,
    localReadyzUrl,
  } from "./links";
  import { copyPayload, type LogsSnapshot } from "./logs";
  import { parsePreviewMode, previewBundle, type PreviewMode } from "./preview";
  import Settings from "./Settings.svelte";
  import { connectionLight, describeLight, isRunningHeadline } from "./status";
  import { asHeadline, sameFolders, type WorkerDir, type WorkerSnapshot } from "./worker";

  type View = "home" | "settings";
  type DrawerTab = "logs" | "checks";

  let view = $state<View>("home");
  let drawer = $state<DrawerTab | "closed">("closed");
  let preview = $state<PreviewMode | null>(null);
  let cli = $state<CliStatus | null>(null);
  let auth = $state<AuthStatus | null>(null);
  let snapshot = $state<WorkerSnapshot | null>(null);
  let logs = $state<LogsSnapshot | null>(null);
  let diag = $state<DiagSnapshot | null>(null);
  let name = $state("");
  let folders = $state<WorkerDir[]>([]);
  let apiKey = $state("");
  let working = $state(false);
  let cliBusy = $state(false);
  let authBusy = $state(false);
  let authProgress = $state<AuthProgress | null>(null);
  let cliProgress = $state<CliProgress | null>(null);
  let diagRunning = $state(false);
  let paused = $state(false);
  let copied = $state(false);
  let copiedDiagnostics = $state(false);
  let message = $state<string | null>(null);
  let restartOpen = $state(false);
  let resetOpen = $state(false);
  let listenAuth: UnlistenFn | undefined;
  let listenCli: UnlistenFn | undefined;
  let listenWorker: UnlistenFn | undefined;
  let poll: ReturnType<typeof setInterval> | undefined;
  let lastAuthAt = 0;
  let lastTrayKey: string | null = null;
  const AUTH_INTERVAL_MS = 20_000;
  const WORKER_INTERVAL_MS = 2_000;

  const mocked = $derived(preview !== null || !isTauri());
  const worker = $derived(snapshot?.worker ?? null);
  const headline = $derived(asHeadline(worker?.runtime.headline ?? "not_ready"));
  const light = $derived(
    connectionLight({
      headline,
      cliPresent: cli?.present ?? auth?.cliPresent ?? false,
      signedIn: auth?.signedIn ?? false,
      installed: worker?.runtime.installed ?? false,
      detail: worker?.runtime.detail ?? message ?? "",
      liveState: worker?.runtime.liveState,
    }),
  );

  onMount(() => {
    const mode = parsePreviewMode(new URL(window.location.href).searchParams.get("preview"));
    document.title = DISPLAY_NAME;
    if (mode) {
      applyPreview(mode);
      return;
    }
    if (!isTauri()) {
      return;
    }
    void boot();
    poll = setInterval(() => {
      void refreshQuiet();
    }, WORKER_INTERVAL_MS);
    window.addEventListener("focus", onWindowFocus);
    document.addEventListener("visibilitychange", onVisibilityChange);
  });

  onDestroy(() => {
    listenAuth?.();
    listenCli?.();
    listenWorker?.();
    if (poll) {
      clearInterval(poll);
    }
    window.removeEventListener("focus", onWindowFocus);
    document.removeEventListener("visibilitychange", onVisibilityChange);
  });

  function onWindowFocus(): void {
    void refreshAuth(true);
  }

  function onVisibilityChange(): void {
    if (document.visibilityState === "visible") {
      void refreshAuth(true);
    }
  }

  function applyPreview(mode: PreviewMode): void {
    const bundle = previewBundle(mode);
    preview = mode;
    view = bundle.view;
    drawer = bundle.drawer;
    cli = bundle.cli;
    auth = bundle.auth;
    snapshot = bundle.worker;
    logs = bundle.logs;
    diag = bundle.diag;
    name = bundle.worker.worker?.name || bundle.worker.suggestedName;
    folders = bundle.worker.worker?.workerDirs ?? [];
  }

  async function boot(): Promise<void> {
    listenAuth = await listen<AuthProgress>("auth-progress", (event) => {
      authProgress = event.payload;
    });
    listenCli = await listen<CliProgress>("cli-progress", (event) => {
      cliProgress = event.payload;
    });
    listenWorker = await listen("worker-changed", () => {
      void refreshQuiet();
    });
    await refreshAll();
  }

  async function refreshAll(): Promise<void> {
    if (!isTauri()) {
      return;
    }
    try {
      cli = await invoke<CliStatus>("cli_status");
      await refreshAuth(true);
      applySnapshot(await invoke<WorkerSnapshot>("worker_get"));
      if (drawer === "logs" && !paused) {
        logs = await invoke<LogsSnapshot>("logs_read");
      }
      await syncTray();
    } catch (error) {
      message = String(error);
    }
  }

  async function refreshQuiet(): Promise<void> {
    if (!isTauri() || working) {
      return;
    }
    try {
      if (Date.now() - lastAuthAt >= AUTH_INTERVAL_MS) {
        await refreshAuth(false);
      }
      const next = await invoke<WorkerSnapshot>("worker_get");
      snapshot = next;
      if (drawer === "logs" && !paused) {
        logs = await invoke<LogsSnapshot>("logs_read");
      }
      await syncTray();
    } catch {
      // Keep the last good frame while polling.
    }
  }

  async function refreshAuth(force: boolean): Promise<void> {
    if (!isTauri()) {
      return;
    }
    auth = normalizeAuth(await invoke<AuthStatus>("auth_status", { force }));
    lastAuthAt = Date.now();
    await syncTray();
  }

  function applySnapshot(next: WorkerSnapshot): void {
    snapshot = next;
    if (next.worker) {
      name = next.worker.name;
      folders = next.worker.workerDirs;
    } else if (!name) {
      name = next.suggestedName;
    }
  }

  function normalizeAuth(status: AuthStatus): AuthStatus {
    return { ...status, method: asAuthMethod(status.method) };
  }

  async function syncTray(): Promise<void> {
    if (!isTauri()) {
      return;
    }
    try {
      const view = {
        color: light.color,
        reason: light.reason,
        headline: describeLight(headline),
        canStart: worker?.runtime.canStart ?? false,
        canStop: worker?.runtime.canStop ?? false,
        canStartDirect: worker?.runtime.canStartDirect ?? false,
        canStopDirect: worker?.runtime.canStopDirect ?? false,
      };
      const key = JSON.stringify(view);
      if (key === lastTrayKey) {
        return;
      }
      lastTrayKey = key;
      await invoke("tray_update", { view });
    } catch {
      // Tray is optional on headless sessions.
    }
  }

  async function runWorker(
    command:
      | "worker_save"
      | "worker_install"
      | "worker_uninstall"
      | "worker_start"
      | "worker_stop"
      | "worker_restart"
      | "worker_set_autostart"
      | "worker_enable_linger"
      | "worker_start_direct"
      | "worker_stop_direct"
      | "worker_restart_direct"
      | "worker_reset_identity",
    extra?: Record<string, unknown>,
  ): Promise<void> {
    if (preview) {
      message = `Preview only — ${command} is mocked.`;
      return;
    }
    working = true;
    message = null;
    try {
      const next = await invoke<WorkerSnapshot>(command, extra);
      applySnapshot(next);
      await syncTray();
    } catch (error) {
      message = String(error);
    } finally {
      working = false;
    }
  }

  async function save(thenRestart: boolean): Promise<void> {
    const running = isRunningHeadline(asHeadline(worker?.runtime.headline ?? "not_ready"));
    const dirtyNow =
      name.trim() !== (worker?.name ?? "") ||
      !sameFolders(
        folders.map((folder) => folder.path),
        worker?.workerDirs.map((folder) => folder.path) ?? [],
      );
    if (preview) {
      message = "Preview only — save is mocked.";
      return;
    }
    await runWorker("worker_save", { request: { name, workerDirs: folders.map((f) => f.path) } });
    if (thenRestart || (running && dirtyNow)) {
      restartOpen = true;
    }
  }

  async function browse(): Promise<void> {
    if (preview || !isTauri()) {
      message = "Folder picker needs the desktop app.";
      return;
    }
    try {
      const picked = await invoke<string | null>("worker_pick_dir");
      if (!picked || folders.some((folder) => folder.path === picked)) {
        return;
      }
      folders = [
        ...folders,
        { path: picked, exists: true, isDirectory: true, gitRemote: null, hasRemote: undefined },
      ];
    } catch (error) {
      message = String(error);
    }
  }

  async function connect(): Promise<void> {
    if (preview) {
      message = "Preview only — Connect to Cursor is mocked.";
      return;
    }
    if (apiKey.trim()) {
      working = true;
      try {
        auth = normalizeAuth(await invoke<AuthStatus>("auth_save_api_key", { key: apiKey }));
        apiKey = "";
      } catch (error) {
        message = String(error);
      } finally {
        working = false;
      }
      return;
    }
    authBusy = true;
    try {
      auth = normalizeAuth(await invoke<AuthStatus>("auth_start_browser_login"));
    } catch (error) {
      message = String(error);
    } finally {
      authBusy = false;
      authProgress = null;
    }
  }

  const liveCommand = $derived(
    previewStartCommand({
      agentPath: worker?.agentPath ?? "agent",
      name,
      dirs: folders.map((folder) => folder.path),
      dataDir: worker?.dataDir ?? "",
      managementPort: worker?.managementPort ?? 18741,
      idleReleaseTimeout: worker?.idleReleaseTimeout ?? 0,
    }).commandLine,
  );

  async function openExternal(url: string): Promise<void> {
    if (preview || !isTauri()) {
      window.open(url, "_blank", "noopener,noreferrer");
      return;
    }
    try {
      await invoke("open_external_url", { url });
    } catch (error) {
      message = String(error);
    }
  }

  async function copyDiagnostics(): Promise<void> {
    if (preview) {
      message = "Preview only — copy diagnostics is mocked.";
      return;
    }
    try {
      const text = await invoke<string>("worker_copy_diagnostics");
      await navigator.clipboard.writeText(text);
      copiedDiagnostics = true;
      setTimeout(() => {
        copiedDiagnostics = false;
      }, 1500);
    } catch (error) {
      message = String(error);
    }
  }

  async function copyCommand(): Promise<void> {
    const text = liveCommand || worker?.commandLine || "";
    if (!text) {
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => {
        copied = false;
      }, 1500);
    } catch {
      message = "Could not copy the command line.";
    }
  }
</script>

<div class="app">
  <header class="top">
    <div class="brand">
      <svg class="mark" viewBox="0 0 60 50" role="img" aria-label="Stand Easy Ops">
        <title>Stand Easy Ops mark</title>
        <path
          fill="#fff"
          d="M30 20L60 50L45.86 50L30 34.14L14.14 50L0 50L30 20ZM37.5 7.5C37.5 11.64 34.14 15 30 15C25.86 15 22.5 11.64 22.5 7.5C22.5 3.36 25.86 0 30 0C34.14 0 37.5 3.36 37.5 7.5Z"
        />
      </svg>
      <div>
        <h1>{DISPLAY_NAME}</h1>
        <p>Stand Easy Ops</p>
      </div>
    </div>
    <div class="top-actions">
      <button
        type="button"
        class="docs"
        onclick={() => void openExternal(CURSOR_DOCS_URL)}
      >
        Cursor Docs
      </button>
      <button
        type="button"
        class="docs"
        onclick={() => void openExternal(STANDEASYOPS_URL)}
      >
        standeasyops.com
      </button>
      <div class="status" title={light.reason}>
        <span class="lamp {light.color}" aria-hidden="true"></span>
        <div>
          <strong>{describeLight(headline)}</strong>
          <p>{light.reason}</p>
        </div>
      </div>
    </div>
  </header>

  <nav>
    <button type="button" class:active={view === "home"} onclick={() => (view = "home")}>Home</button>
    <button type="button" class:active={view === "settings"} onclick={() => (view = "settings")}>Settings</button>
    <span class="spacer"></span>
    <button
      type="button"
      class:active={drawer === "logs"}
      onclick={() => (drawer = drawer === "logs" ? "closed" : "logs")}
    >
      Logs
    </button>
    <button
      type="button"
      class:active={drawer === "checks"}
      onclick={() => (drawer = drawer === "checks" ? "closed" : "checks")}
    >
      Checks
    </button>
  </nav>

  <div class="body">
    <main>
      {#if view === "home"}
        <Home
          {snapshot}
          {auth}
          {authBusy}
          {authProgress}
          {apiKey}
          {name}
          {folders}
          {working}
          {message}
          {mocked}
          focusName={preview === "name"}
          onName={(value) => (name = value)}
          onApiKey={(value) => (apiKey = value)}
          onSave={() => void save(false)}
          onBrowse={() => void browse()}
          onRemove={(index) => {
            folders = folders.filter((_, current) => current !== index);
          }}
          onInstall={() => void runWorker("worker_install")}
          onUninstall={() => void runWorker("worker_uninstall")}
          onStart={() => void runWorker("worker_start")}
          onStop={() => void runWorker("worker_stop")}
          onRestart={() => void runWorker("worker_restart")}
          onStartDirect={() => void runWorker("worker_start_direct")}
          onStopDirect={() => void runWorker("worker_stop_direct")}
          onRestartDirect={() => void runWorker("worker_restart_direct")}
          onAutostart={(enabled) => void runWorker("worker_set_autostart", { enabled })}
          onLinger={() => void runWorker("worker_enable_linger")}
          onConnect={() => void connect()}
          onCancelLogin={() => {
            void invoke<AuthStatus>("auth_cancel_browser_login").then((status) => {
              auth = normalizeAuth(status);
              authBusy = false;
            });
          }}
          onSignOut={() => {
            void invoke<AuthStatus>("auth_sign_out").then((status) => {
              auth = normalizeAuth(status);
            });
          }}
          onCopyCommand={() => void copyCommand()}
          onOpenAgents={() => {
            const current = worker?.currentAgentBcId
              ? agentChatUrl(worker.currentAgentBcId)
              : CURSOR_AGENTS_URL;
            void openExternal(current ?? CURSOR_AGENTS_URL);
          }}
          onOpenLocal={() =>
            void openExternal(localReadyzUrl(worker?.managementPort ?? 18741))}
          onOpenAgent={(bcId) => {
            const url = agentChatUrl(bcId);
            if (url) {
              void openExternal(url);
            }
          }}
          onCopyDiagnostics={() => void copyDiagnostics()}
          onResetIdentity={() => {
            resetOpen = true;
          }}
          {copied}
          {copiedDiagnostics}
        />
      {:else}
        <Settings
          {cli}
          {cliBusy}
          {cliProgress}
          {snapshot}
          {working}
          {mocked}
          onInstallCli={() => {
            cliBusy = true;
            void invoke<CliStatus>("cli_install")
              .then((status) => {
                cli = status;
              })
              .finally(() => {
                cliBusy = false;
              });
          }}
          onUpdateCli={() => {
            cliBusy = true;
            void invoke<CliStatus>("cli_update")
              .then((status) => {
                cli = status;
              })
              .finally(() => {
                cliBusy = false;
              });
          }}
          onOpenConfig={() => {
            void invoke("worker_open_config_folder");
          }}
        />
      {/if}
    </main>
    {#if drawer !== "closed"}
      <Drawer
        tab={drawer}
        {logs}
        {diag}
        running={diagRunning}
        {paused}
        {mocked}
        onTab={(tab) => (drawer = tab)}
        onClose={() => (drawer = "closed")}
        onPause={() => (paused = !paused)}
        onCopy={() => {
          if (logs) {
            void navigator.clipboard.writeText(copyPayload(logs));
          }
        }}
        onOpenFolder={() => {
          void invoke("logs_open_folder");
        }}
        onRunChecks={() => {
          diagRunning = true;
          void invoke<DiagSnapshot>("diag_run")
            .then((result) => {
              diag = result;
            })
            .finally(() => {
              diagRunning = false;
            });
        }}
      />
    {/if}
  </div>

  <footer>Works with the official Cursor CLI</footer>
</div>

{#if resetOpen}
  <div class="modal" role="dialog" aria-labelledby="reset-title">
    <div class="sheet">
      <h2 id="reset-title">Reset worker identity?</h2>
      <p>
        There is no <code>agent worker reset</code> command. This stops the worker,
        deletes files in the isolated <code>--data-dir</code> (where the official CLI
        stores the logical worker id), then starts again with the same
        <code>--name</code> and <code>--worker-dir</code> flags. Global CLI files under
        <code>~/.local/share/cursor-agent</code> are not touched.
      </p>
      <div class="actions">
        <button
          type="button"
          class="primary"
          onclick={() => {
            resetOpen = false;
            void runWorker("worker_reset_identity");
          }}>Reset identity</button
        >
        <button type="button" class="secondary" onclick={() => (resetOpen = false)}>Cancel</button>
      </div>
    </div>
  </div>
{/if}

{#if restartOpen}
  <div class="modal" role="dialog" aria-labelledby="restart-title">
    <div class="sheet">
      <h2 id="restart-title">Restart the service?</h2>
      <p>
        The supervisor reads <code>--name</code> and every <code>--worker-dir</code> when the job
        starts. Restart now to apply the saved command.
      </p>
      <div class="actions">
        <button
          type="button"
          class="primary"
          onclick={() => {
            restartOpen = false;
            void runWorker("worker_restart");
          }}>Restart now</button
        >
        <button type="button" class="secondary" onclick={() => (restartOpen = false)}>Later</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .app {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--paper);
  }

  .top {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    align-items: center;
    padding: 14px 18px;
    background: var(--teal);
    color: #fff;
  }

  .brand {
    display: flex;
    gap: 12px;
    align-items: center;
  }

  .mark {
    width: 36px;
    height: 30px;
    flex-shrink: 0;
    display: block;
  }

  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 1.2rem;
    letter-spacing: -0.03em;
  }

  .brand p,
  .status p {
    margin: 0;
    font-size: 0.8rem;
    opacity: 0.85;
  }

  .top-actions {
    display: flex;
    gap: 14px;
    align-items: center;
  }

  .docs {
    border: 1px solid rgb(255 255 255 / 0.4);
    background: transparent;
    color: #fff;
    border-radius: 8px;
    padding: 0.45rem 0.8rem;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }

  .docs:hover {
    background: rgb(255 255 255 / 0.12);
  }

  .status {
    display: flex;
    gap: 10px;
    align-items: center;
    max-width: 22rem;
  }

  .status strong {
    display: block;
    font-size: 0.85rem;
  }

  .lamp {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    flex-shrink: 0;
    box-shadow: 0 0 0 3px rgb(255 255 255 / 0.18);
  }

  .lamp.green {
    background: #2ecc71;
  }

  .lamp.amber {
    background: #f1c40f;
  }

  .lamp.red {
    background: #e74c3c;
  }

  nav {
    display: flex;
    gap: 6px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--border);
    background: #fff;
  }

  nav button {
    border: 0;
    background: transparent;
    padding: 0.4rem 0.7rem;
    border-radius: 7px;
    font: inherit;
    font-weight: 600;
    color: var(--ink);
    cursor: pointer;
  }

  nav button.active {
    background: #e6eef0;
    color: var(--teal);
  }

  .spacer {
    flex: 1;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  main {
    flex: 1;
    min-width: 0;
    overflow: auto;
    padding: 16px 18px 20px;
  }

  footer {
    padding: 10px 18px;
    color: var(--muted);
    font-size: 0.78rem;
    border-top: 1px solid var(--border);
    background: #fff;
  }

  .modal {
    position: fixed;
    inset: 0;
    background: rgb(18 23 31 / 0.45);
    display: grid;
    place-items: center;
    padding: 20px;
  }

  .sheet {
    background: #fff;
    border-radius: 12px;
    padding: 18px;
    max-width: 26rem;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .sheet h2 {
    margin: 0;
    font-family: var(--font-display);
    color: var(--teal);
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .primary,
  .secondary {
    border: 0;
    border-radius: 8px;
    padding: 0.55rem 0.9rem;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }

  .primary {
    background: var(--teal);
    color: #fff;
  }

  .secondary {
    background: transparent;
    border: 1px solid var(--border);
  }
</style>
