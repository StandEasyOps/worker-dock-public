<script lang="ts">
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { DISPLAY_NAME } from "./brand";
  import { describePhase, type CliProgress, type CliStatus } from "./cli";
  import type { WorkerSnapshot } from "./worker";

  type Props = {
    cli: CliStatus | null;
    cliBusy: boolean;
    cliProgress: CliProgress | null;
    snapshot: WorkerSnapshot | null;
    working: boolean;
    mocked: boolean;
    onInstallCli: () => void;
    onUpdateCli: () => void;
    onOpenConfig: () => void;
  };

  let {
    cli,
    cliBusy,
    cliProgress,
    snapshot,
    working,
    mocked,
    onInstallCli,
    onUpdateCli,
    onOpenConfig,
  }: Props = $props();

  let appVersion = $state("0.1.0");

  $effect(() => {
    if (!isTauri()) {
      return;
    }
    void invoke<string>("app_version")
      .then((value) => {
        if (value) {
          appVersion = value;
        }
      })
      .catch(() => {
        // Preview and missing-command fallback stay on the committed baseline.
      });
  });
</script>

<section class="settings">
  <section class="card" id="about">
    <h2>About</h2>
    <dl>
      <div>
        <dt>Name</dt>
        <dd>{DISPLAY_NAME}</dd>
      </div>
      <div>
        <dt>App version</dt>
        <dd>{appVersion}</dd>
      </div>
      <div>
        <dt>Publisher</dt>
        <dd>Stand Easy Ops</dd>
      </div>
    </dl>
    <p class="hint">Unsigned tester. Works with the official Cursor CLI.</p>
  </section>

  <section class="card">
    <h2>Official agent CLI</h2>
    <p>Install the pinned official archive. Auto-update stays off. Works with the official Cursor CLI.</p>
    {#if cliBusy}
      <p class="hint">{cliProgress ? describePhase(cliProgress.phase) : "Working"}… {cliProgress?.detail ?? ""}</p>
    {:else if cli}
      <dl>
        <div>
          <dt>Installed</dt>
          <dd>{cli.present ? cli.installedVersion ?? "yes" : "Not installed"}</dd>
        </div>
        <div>
          <dt>Pinned</dt>
          <dd>{cli.pinnedVersion}</dd>
        </div>
        <div>
          <dt>Path</dt>
          <dd><code>{cli.path ?? "—"}</code></dd>
        </div>
      </dl>
      <div class="actions">
        {#if !cli.present}
          <button type="button" class="primary" disabled={working} onclick={onInstallCli}>Install official CLI</button>
        {:else if cli.updateAvailable}
          <button type="button" class="primary" disabled={working} onclick={onUpdateCli}>Install vetted update</button>
        {/if}
      </div>
      {#if cli.officialNewerUnvetted}
        <p class="hint">Official latest is newer and not vetted. worker-dock will not install it.</p>
      {/if}
    {:else}
      <p class="hint">CLI status is unavailable{mocked ? " in this preview" : ""}.</p>
    {/if}
  </section>

  <section class="card">
    <h2>Paths</h2>
    <p>Config: <code>{snapshot?.configPath ?? "—"}</code></p>
    <p>Worker data: <code>{snapshot?.worker?.dataDir || "assigned on first save"}</code></p>
    <div class="actions">
      <button type="button" class="secondary" onclick={onOpenConfig}>Open config folder</button>
    </div>
  </section>

  <section class="card">
    <h2>Health telemetry</h2>
    <label class="toggle">
      <input type="checkbox" disabled checked={false} />
      <span>Send app and worker health, plus crash reports, to Stand Easy Ops</span>
    </label>
    <p class="hint">
      Not built. M2 scope only: opt-in telemetry to Stand Easy Ops’ own backend. Default off. The
      official CLI is not used as a telemetry pipe.
    </p>
  </section>
</section>

<style>
  .settings {
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

  h2 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 1.05rem;
    color: var(--teal);
  }

  p,
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.88rem;
  }

  dl {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px 16px;
    margin: 0;
  }

  dt {
    color: var(--muted);
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  dd {
    margin: 2px 0 0;
  }

  .actions {
    display: flex;
    gap: 8px;
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
  }

  .primary {
    background: var(--teal);
    color: #fff;
  }

  .secondary {
    background: transparent;
    border: 1px solid var(--border);
  }

  .toggle {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  code {
    font-family: var(--font-mono);
    font-size: 0.8rem;
    word-break: break-all;
  }
</style>
