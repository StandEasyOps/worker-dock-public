<script lang="ts">
  import { describeLogSource, type LogsSnapshot } from "./logs";
  import { describeHostClass, describeProbeState, type DiagSnapshot } from "./diag";

  type Tab = "logs" | "checks";

  type Props = {
    tab: Tab;
    logs: LogsSnapshot | null;
    diag: DiagSnapshot | null;
    running: boolean;
    paused: boolean;
    mocked: boolean;
    onTab: (tab: Tab) => void;
    onClose: () => void;
    onPause: () => void;
    onCopy: () => void;
    onOpenFolder: () => void;
    onRunChecks: () => void;
  };

  let {
    tab,
    logs,
    diag,
    running,
    paused,
    mocked,
    onTab,
    onClose,
    onPause,
    onCopy,
    onOpenFolder,
    onRunChecks,
  }: Props = $props();
</script>

<aside class="drawer" aria-label="Logs and diagnostics">
  <header>
    <div class="tabs" role="tablist">
      <button type="button" class:active={tab === "logs"} onclick={() => onTab("logs")}>Logs</button>
      <button type="button" class:active={tab === "checks"} onclick={() => onTab("checks")}>Checks</button>
    </div>
    <button type="button" class="ghost" onclick={onClose}>Close</button>
  </header>

  {#if tab === "logs"}
    <div class="toolbar">
      <button type="button" class="secondary" onclick={onPause}>{paused ? "Resume" : "Pause"}</button>
      <button type="button" class="secondary" onclick={onCopy} disabled={!logs}>Copy</button>
      <button type="button" class="secondary" onclick={onOpenFolder}>Open folder</button>
    </div>
    {#if mocked && !logs}
      <p class="hint">Open with the desktop app to tail wrapper.log.</p>
    {:else if logs}
      {#each logs.sources as source (source.path)}
        <h3>{describeLogSource(source.name)}</h3>
        <pre><code>{source.exists ? source.text || "(empty)" : "(not present)"}</code></pre>
      {/each}
    {:else}
      <p class="hint">No log snapshot yet.</p>
    {/if}
  {:else}
    <div class="toolbar">
      <button type="button" class="primary" disabled={running} onclick={onRunChecks}>
        {running ? "Running checks…" : "Run checks"}
      </button>
    </div>
    {#if diag}
      <p class="hint">{diag.summary}</p>
      <ul>
        {#each diag.hosts as host (host.host)}
          <li>
            <strong>{host.host}</strong>
            · {describeHostClass(host.class)}
            · {describeProbeState(host.ok)}
            · {host.detail}
          </li>
        {/each}
      </ul>
      {#if diag.debug.ran}
        <p class="hint">
          Official debug {diag.debug.ok ? "ok" : "failed"}
          {#if diag.debug.visibility}
            · visibility {diag.debug.visibility}
          {/if}
        </p>
      {/if}
    {:else}
      <p class="hint">
        Run outbound HTTPS preflight and <code>agent worker debug --json</code> with the saved name
        and folders.
      </p>
    {/if}
  {/if}
</aside>

<style>
  .drawer {
    width: min(380px, 42vw);
    border-left: 1px solid var(--border);
    background: #fff;
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 12px 14px 16px;
    gap: 10px;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .tabs,
  .toolbar {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  h3 {
    margin: 8px 0 4px;
    font-size: 0.8rem;
    color: var(--teal);
  }

  pre {
    margin: 0;
    padding: 10px;
    background: var(--ink);
    color: #eef3f4;
    border-radius: 8px;
    font-size: 0.72rem;
    overflow: auto;
    max-height: 180px;
  }

  ul {
    margin: 0;
    padding-left: 1.1rem;
    color: var(--muted);
    font-size: 0.85rem;
  }

  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.85rem;
  }

  button {
    border: 0;
    border-radius: 8px;
    padding: 0.4rem 0.7rem;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }

  .tabs button {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--ink);
  }

  .tabs button.active {
    background: var(--teal);
    color: #fff;
    border-color: var(--teal);
  }

  .primary {
    background: var(--teal);
    color: #fff;
  }

  .secondary {
    background: transparent;
    border: 1px solid var(--border);
  }

  .ghost {
    background: transparent;
    color: var(--muted);
  }

  button:disabled {
    opacity: 0.45;
  }
</style>
