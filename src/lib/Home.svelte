<script lang="ts">
  import { onMount } from "svelte";
  import { previewStartCommand } from "./command";
  import { describeAuthMethod, type AuthProgress, type AuthStatus } from "./auth";
  import {
    asHeadline,
    asLiveState,
    describeBackend,
    describeLiveState,
    folderHint,
    sameFolders,
    type WorkerDir,
    type WorkerSnapshot,
  } from "./worker";
  import { isRunningHeadline } from "./status";

  type Props = {
    snapshot: WorkerSnapshot | null;
    auth: AuthStatus | null;
    authBusy: boolean;
    authProgress: AuthProgress | null;
    apiKey: string;
    name: string;
    folders: WorkerDir[];
    working: boolean;
    message: string | null;
    mocked: boolean;
    focusName: boolean;
    onName: (value: string) => void;
    onApiKey: (value: string) => void;
    onSave: () => void;
    onBrowse: () => void;
    onRemove: (index: number) => void;
    onInstall: () => void;
    onUninstall: () => void;
    onStart: () => void;
    onStop: () => void;
    onRestart: () => void;
    onStartDirect: () => void;
    onStopDirect: () => void;
    onRestartDirect: () => void;
    onAutostart: (enabled: boolean) => void;
    onLinger: () => void;
    onConnect: () => void;
    onCancelLogin: () => void;
    onSignOut: () => void;
    onCopyCommand: () => void;
    onOpenAgents: () => void;
    onOpenLocal: () => void;
    onOpenAgent: (bcId: string) => void;
    onCopyDiagnostics: () => void;
    onResetIdentity: () => void;
    copied: boolean;
    copiedDiagnostics: boolean;
  };

  let {
    snapshot,
    auth,
    authBusy,
    authProgress,
    apiKey,
    name,
    folders,
    working,
    message,
    mocked,
    focusName,
    onName,
    onApiKey,
    onSave,
    onBrowse,
    onRemove,
    onInstall,
    onUninstall,
    onStart,
    onStop,
    onRestart,
    onStartDirect,
    onStopDirect,
    onRestartDirect,
    onAutostart,
    onLinger,
    onConnect,
    onCancelLogin,
    onSignOut,
    onCopyCommand,
    onOpenAgents,
    onOpenLocal,
    onOpenAgent,
    onCopyDiagnostics,
    onResetIdentity,
    copied,
    copiedDiagnostics,
  }: Props = $props();

  const worker = $derived(snapshot?.worker ?? null);
  const runtime = $derived(worker?.runtime);
  const installed = $derived(runtime?.installed ?? false);
  const headline = $derived(asHeadline(runtime?.headline ?? "not_ready"));
  const runMode = $derived(runtime?.runMode ?? "none");
  const running = $derived(isRunningHeadline(headline) || runMode === "service" || runMode === "direct");
  const savedName = $derived(worker?.name ?? "");
  const savedFolders = $derived(worker?.workerDirs.map((folder) => folder.path) ?? []);
  const dirty = $derived(name.trim() !== savedName || !sameFolders(folders.map((f) => f.path), savedFolders));
  const live = $derived(
    previewStartCommand({
      agentPath: worker?.agentPath ?? "agent",
      name,
      dirs: folders.map((folder) => folder.path),
      dataDir: worker?.dataDir ?? "",
      managementPort: worker?.managementPort ?? 18741,
      idleReleaseTimeout: worker?.idleReleaseTimeout ?? 0,
    }),
  );
  const commandLine = $derived(
    dirty || !worker?.commandLine ? live.commandLine : worker.commandLine,
  );
  const managementPort = $derived(worker?.managementPort ?? 18741);
  const liveState = $derived(asLiveState(runtime?.liveState ?? "offline"));
  const recentAgents = $derived(worker?.recentAgents ?? []);
  const currentAgentUrl = $derived(
    worker?.currentAgentBcId
      ? `https://cursor.com/agents/${worker.currentAgentBcId}`
      : "https://cursor.com/agents",
  );
  const needsRestart = $derived(runtime?.restartRequired ?? (dirty && running));
  let nameInput: HTMLInputElement | undefined = $state();

  onMount(() => {
    if (focusName) {
      nameInput?.focus();
    }
  });
</script>

<section class="home">
  {#if message}
    <p class="banner" role="alert">{message}</p>
  {/if}
  {#if needsRestart}
    <p class="banner warn">
      Name or folders changed. Restart the service so it launches with the new
      <code>--name</code> and <code>--worker-dir</code> flags.
    </p>
  {/if}
  {#if mocked}
    <p class="hint">Preview — this screen is mocked. Live Tauri commands are wired on the desktop app.</p>
  {/if}

  <section class="card" aria-labelledby="name-heading">
    <div class="card-head">
      <div>
        <h2 id="name-heading">Worker name</h2>
        <p>Passed as <code>--name</code>. Defaults to this machine’s hostname. Saved in worker config.</p>
      </div>
      <div class="actions name-links">
        <button type="button" class="secondary" onclick={onOpenAgents}>Open in Agents</button>
        <button type="button" class="secondary" onclick={onOpenLocal}>Local status</button>
      </div>
    </div>
    <label class="field" for="worker-name">
      <span>Name</span>
      <input
        id="worker-name"
        bind:this={nameInput}
        name="worker-name"
        type="text"
        maxlength="64"
        autocomplete="off"
        spellcheck="false"
        value={name}
        oninput={(event) => onName(event.currentTarget.value)}
      />
    </label>
    <p class="hint">
      <strong>Open in Agents</strong> opens
      {#if worker?.currentAgentBcId}
        the current claim
        <code>{currentAgentUrl}</code>.
      {:else}
        <code>https://cursor.com/agents</code>. Pick this machine under My Machines.
      {/if}
    </p>
    <p class="hint">
      Live status is localhost <code>/readyz</code>, <code>/healthz</code>, and
      documented <code>/metrics</code> gauges — not a dashboard.
      <strong>Local status</strong> opens
      <code>http://127.0.0.1:{managementPort}/readyz</code>.
    </p>
    <div class="pills" aria-label="Live worker status">
      <span class="pill {liveState === 'busy' || liveState === 'idle' || liveState === 'live' ? 'on' : liveState === 'unresponsive' || liveState === 'error' ? 'bad' : 'off'}">
        {describeLiveState(liveState)}
      </span>
      {#if runtime?.readyzOk}
        <span class="pill on">/readyz</span>
      {/if}
      {#if runtime?.metricsOk}
        <span class="pill on">/metrics</span>
      {/if}
      {#if runtime?.connected === true}
        <span class="pill on">connected</span>
      {:else if runtime?.connected === false}
        <span class="pill off">not connected</span>
      {/if}
    </div>
    {#if runtime?.detail}
      <p class="hint">{runtime.detail}</p>
    {/if}
    {#if runtime?.watchdogNote}
      <p class="banner warn" role="status">{runtime.watchdogNote}</p>
    {/if}
    <div class="actions">
      <button type="button" class="secondary" disabled={working} onclick={onCopyDiagnostics}>
        {copiedDiagnostics ? "Copied diagnostics" : "Copy diagnostics"}
      </button>
      <button
        type="button"
        class="secondary"
        disabled={working || !worker?.canResetIdentity}
        onclick={onResetIdentity}
      >
        Reset worker identity
      </button>
    </div>
    <div class="recent" aria-labelledby="recent-agents-heading">
      <h3 id="recent-agents-heading">Recent agents</h3>
      {#if recentAgents.length === 0}
        <p class="hint">No claimed agents in wrapper.log yet.</p>
      {:else}
        <ul>
          {#each recentAgents as agent (agent.bcId)}
            <li>
              <div>
                <code>{agent.bcId}</code>
                <span class="meta">{agent.claimedAt || "Time unknown"}</span>
              </div>
              <button type="button" class="secondary" onclick={() => onOpenAgent(agent.bcId)}>
                Open
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </section>

  <section class="card" aria-labelledby="folders-heading">
    <div class="card-head">
      <div>
        <h2 id="folders-heading">Repo folders</h2>
        <p>Each local folder is a <code>--worker-dir</code>. First folder is assignment identity.</p>
      </div>
      <button type="button" class="secondary" disabled={working} onclick={onBrowse}>Add folder</button>
    </div>
    {#if folders.length === 0}
      <div class="empty" data-state="folders-empty">
        <p>No folders yet.</p>
        <p class="hint">Add a local checkout. The native folder picker is used — not a typed path.</p>
      </div>
    {:else}
      <ul class="folders">
        {#each folders as folder, index (folder.path)}
          <li>
            <div>
              <code>{folder.path}</code>
              <span class="meta">
                {index === 0 ? "Assignment identity" : `Folder ${index + 1}`}
                · {folderHint(folder)}
                {#if folder.gitRemote}
                  · {folder.gitRemote}
                {:else if folder.exists && folder.isDirectory && folder.hasRemote === false}
                  · <span class="flag">No git remote</span>
                {/if}
              </span>
            </div>
            <button type="button" class="ghost" onclick={() => onRemove(index)}>Remove</button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section class="card" aria-labelledby="direct-heading">
    <h2 id="direct-heading">Run without service</h2>
    <p>
      Starts the official CLI as a child of worker-dock, same
      <code>--name</code> and <code>--worker-dir</code> flags as the service.
      <strong>Quit UI</strong> stops it. Closing the window to the tray does not.
      Service mode and this mode cannot run at the same time.
    </p>
    <div class="pills" aria-label="Direct worker state">
      <span class="pill {runMode === 'direct' ? 'on' : 'off'}">
        {runMode === "direct" ? "Running without service" : "Not running as child"}
      </span>
    </div>
    <div class="actions primary-row">
      <button
        type="button"
        class="primary"
        disabled={working || !runtime?.canStartDirect || folders.length === 0 || !name.trim()}
        onclick={onStartDirect}
      >
        Start worker (without service)
      </button>
      <button type="button" class="danger" disabled={working || !runtime?.canStopDirect} onclick={onStopDirect}>
        Stop worker (without service)
      </button>
      <button
        type="button"
        class="secondary"
        disabled={working || !runtime?.canRestartDirect}
        onclick={onRestartDirect}
      >
        Restart worker (without service)
      </button>
    </div>
  </section>

  <section class="card service" aria-labelledby="service-heading">
    <div class="card-head">
      <div>
        <h2 id="service-heading">Service setup</h2>
        <p>
          {#if runtime}
            {describeBackend(runtime.backend)}
            {#if runtime.jobName}
              · <code>{runtime.jobName}</code>
            {/if}
          {:else}
            Install a per-user background job. Closing this window does not stop it.
          {/if}
        </p>
      </div>
      <div class="pills" aria-label="Service state">
        <span class="pill {installed ? 'on' : 'off'}">{installed ? "Installed" : "Not installed"}</span>
        <span class="pill {running && runMode === 'service' ? 'on' : 'off'}">
          {runMode === "service" ? "Service running" : "Service stopped"}
        </span>
        <span class="pill {runtime?.enabled || runtime?.autostartEnabled ? "on" : "off"}">
          Auto-start {runtime?.enabled || runtime?.autostartEnabled ? "on" : "off"}
        </span>
      </div>
    </div>

    {#if !installed}
      <div class="actions primary-row">
        <button
          type="button"
          class="primary"
          disabled={working || folders.length === 0 || !name.trim() || runMode === "direct"}
          onclick={onInstall}
        >
          Install as a service
        </button>
      </div>
      <p class="hint">
        Ubuntu: systemd user unit. Windows: interactive-user scheduled task (not SYSTEM). The job
        runs <code>worker-dock-run</code>, which execs the official CLI with
        <code>--name</code> and every <code>--worker-dir</code> before <code>start</code>.
      </p>
    {:else}
      <div class="actions primary-row">
        <button type="button" class="primary" disabled={working || !runtime?.canStart} onclick={onStart}>
          Start
        </button>
        <button type="button" class="danger" disabled={working || !runtime?.canStop} onclick={onStop}>Stop</button>
        <button type="button" class="secondary" disabled={working || !runtime?.canRestart} onclick={onRestart}>
          Restart
        </button>
        <button type="button" class="ghost" disabled={working} onclick={onUninstall}>Uninstall service</button>
      </div>
      <label class="toggle">
        <input
          type="checkbox"
          checked={runtime?.enabled || runtime?.autostartEnabled || false}
          disabled={working}
          onchange={(event) => onAutostart(event.currentTarget.checked)}
        />
        <span>Start automatically at login</span>
      </label>
      <p class="hint">{runtime?.autostartNote}</p>
      {#if runtime?.backend === "systemd-user" && runtime.canEnableLinger}
        <div class="actions">
          <button type="button" class="secondary" disabled={working} onclick={onLinger}>
            Allow start at boot (linger)
          </button>
        </div>
      {/if}
    {/if}
  </section>

  <section class="card command" aria-labelledby="command-heading">
    <div class="card-head">
      <div>
        <h2 id="command-heading">Command that will run</h2>
        <p>
          Read-only. Worker flags sit before <code>start</code> — not
          <code>agent worker start --name …</code>. No <code>--pool</code> or
          <code>--api-key</code>.
        </p>
      </div>
      <button type="button" class="secondary" disabled={!commandLine} onclick={onCopyCommand}>
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
    {#if commandLine}
      <pre class="cmdline"><code>{commandLine}</code></pre>
    {:else}
      <p class="hint">Add a name and at least one folder to preview the argv the supervisor will exec.</p>
    {/if}
  </section>

  <section class="card" aria-labelledby="connect-heading">
    <h2 id="connect-heading">Connect to Cursor</h2>
    {#if authBusy}
      <p class="hint">
        {authProgress?.detail || "Waiting for browser…"}
      </p>
      {#if authProgress?.loginUrl}
        <p class="hint"><code>{authProgress.loginUrl}</code></p>
      {/if}
      <button type="button" class="secondary" onclick={onCancelLogin}>Cancel</button>
    {:else if auth?.signedIn}
      <p>
        Signed in as {auth.userEmail ?? "this account"}
        · {describeAuthMethod(auth.method)}
      </p>
      <button type="button" class="secondary" disabled={working} onclick={onSignOut}>Sign out</button>
    {:else}
      <p>Browser login or a personal API key in the OS keychain. Team Pools are not supported.</p>
      <div class="actions">
        <button type="button" class="primary" disabled={working || !auth?.cliPresent} onclick={onConnect}>
          Connect to Cursor
        </button>
      </div>
      <label class="field" for="personal-api-key">
        <span>Personal API key</span>
        <input
          id="personal-api-key"
          name="personal-api-key"
          type="password"
          autocomplete="off"
          spellcheck="false"
          value={apiKey}
          oninput={(event) => onApiKey(event.currentTarget.value)}
        />
      </label>
      <p class="hint">Stored only in the OS keychain. Never written to config.toml or placed on argv.</p>
    {/if}
  </section>

  <div class="actions">
    <button type="button" class="secondary" disabled={working || !dirty} onclick={onSave}>Save worker</button>
  </div>
</section>

<style>
  .home {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .card {
    background: #fff;
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 16px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .card-head {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    align-items: flex-start;
  }

  h2 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 1.05rem;
    font-weight: 600;
    letter-spacing: -0.02em;
    color: var(--teal);
  }

  p,
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.88rem;
  }

  .banner {
    margin: 0;
    padding: 10px 12px;
    border-radius: 8px;
    background: #fdecea;
    color: #7a1f1a;
    font-size: 0.9rem;
  }

  .banner.warn {
    background: #fff6e0;
    color: #6a4b00;
  }

  .pills {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .pill {
    border-radius: 999px;
    padding: 0.15rem 0.55rem;
    font-size: 0.75rem;
    font-weight: 600;
    border: 1px solid var(--border);
  }

  .pill.on {
    color: #1f6b3a;
    border-color: #b7d7c2;
    background: #eef7f1;
  }

  .pill.off {
    color: var(--muted);
  }

  .pill.bad {
    color: #7a1f1a;
    border-color: #e8c4c0;
    background: #fdecea;
  }

  .actions,
  .primary-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }

  .name-links {
    flex-shrink: 0;
  }

  .recent h3 {
    margin: 6px 0 0;
    font-family: var(--font-display);
    font-size: 0.92rem;
    font-weight: 600;
    color: var(--teal);
  }

  .recent ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .recent li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    align-items: center;
    padding: 8px 0;
    border-top: 1px solid var(--border);
  }

  .recent li:first-child {
    border-top: 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field span {
    font-size: 0.72rem;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
  }

  input[type="text"],
  input[type="password"] {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0.65rem 0.75rem;
    font: inherit;
    color: var(--ink);
    background: #fff;
  }

  input:focus {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .folders {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }

  .folders li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 0;
    border-top: 1px solid var(--border);
  }

  .meta {
    display: block;
    color: var(--muted);
    font-size: 0.8rem;
  }

  .flag {
    color: #9a6b00;
    font-weight: 600;
  }

  .empty {
    border: 1px dashed var(--border);
    border-radius: 10px;
    padding: 22px 16px;
    text-align: center;
    background: #f7fafb;
  }

  .cmdline {
    margin: 0;
    padding: 12px;
    border-radius: 8px;
    background: var(--ink);
    color: #f4f7f8;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    line-height: 1.45;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-all;
  }

  .toggle {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 0.92rem;
  }

  button {
    border: 0;
    border-radius: 8px;
    padding: 0.6rem 0.95rem;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .primary {
    background: var(--teal);
    color: #fff;
  }

  .secondary {
    background: transparent;
    color: var(--ink);
    border: 1px solid var(--border);
  }

  .danger {
    background: #c0392b;
    color: #fff;
  }

  .ghost {
    background: transparent;
    color: var(--muted);
  }

  code {
    font-family: var(--font-mono);
    font-size: 0.8rem;
  }
</style>
