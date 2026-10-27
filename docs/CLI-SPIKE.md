# Official `agent` CLI spike

Notes from one install of the official CLI on 2026-10-20. This file records what worker-dock can call. It does not copy CLI help or Cursor documentation.

Current references:

- [CLI overview](https://cursor.com/docs/cli/overview)
- [CLI parameters](https://cursor.com/docs/cli/reference/parameters)
- [CLI authentication](https://cursor.com/docs/cli/reference/authentication)
- [My Machines](https://cursor.com/docs/cloud-agent/self-hosted/my-machines)
- [Self-hosted pools](https://cursor.com/docs/cloud-agent/self-hosted/pool)
- [Cloud Agents API](https://cursor.com/docs/cloud-agent/api/endpoints)

**Machine:** Ubuntu Linux x64 cloud VM (this design run)  
**Install:** official script, no completed login

```bash
curl https://cursor.com/install -fsS | bash
```

**Installed version:** `2026.10.19-6bbf2c4`  
The installer printed a linux/x64 archive under `https://downloads.cursor.com/lab/<version>/`.

## Layout after install

| Path | Role |
| --- | --- |
| `~/.local/bin/agent` | Symlink to the versioned wrapper |
| `~/.local/bin/cursor-agent` | Same target (legacy name) |
| `~/.local/share/cursor-agent/versions/2026.10.19-6bbf2c4/` | Versioned package |
| `…/cursor-agent` | 1.1K wrapper script |
| `…/cursor-agent-sea` | 131M Node single-executable |
| `…/cursor-agent-worker-sea` | 140M worker executable |
| Package tree size | **561M** |

`~/.local/bin` was not on `PATH` until added. A GUI or service process has the same gap.

`agent login` was started once with `NO_OPEN_BROWSER=1` to see the URL shape, then killed (exit 124 from `timeout`). A dummy auth file left by a failed token probe was deleted.

## Commands this build accepted

Root help is the interactive agent plus subcommands. `agent help` printed the same text (exit 0). Commands worker-dock uses: `login`, `logout`, `status` / `whoami`, `about`, `update`, `worker start`, `worker debug`.

Also present, and not used: `team` (`list`, `switch`), `models`, `mcp`, `plugin`, `persist`, and `worker controller`.

`persist` keeps an interactive coding session across disconnects. It is not a worker supervisor.

`worker` without a pool flag is a personal My Machines worker. With a pool flag it joins an enterprise self-hosted pool and wants a service-account key. worker-dock never passes a pool flag.

`worker start` adds one flag of its own, `--verbose`. Other worker flags belong before `start`. `worker help start` matched `start --help` (exit 0).

`worker debug` prints a text preflight report, or JSON with `--json`. `worker help debug` matched `debug --help` (exit 0). Unauthenticated `--json` is still plain text.

`worker controller` is the pool autoscaler: a service-account key, a pending-request stream, a spawn hook, and optional warm idle workers. worker-dock must not invoke it. See the pool guide.

`login`, `logout`, and `update` took no extra flags in this build. `status`, `whoami`, `about`, and `team list` take `--format text|json`.

`--auth-token` / `CURSOR_AUTH_TOKEN` was accepted on `worker start` and is documented for workers, but it was absent from root help and from `worker --help`. That help listed `--auth-token-file` only.

## Flags worker-dock relies on

Behaviour below is what this build did, checked against the parameter and My Machines docs.

- `--name` sets the display name. Omitted, the CLI uses the machine hostname. Env: `CURSOR_WORKER_NAME`.
- `--worker-dir` is repeatable. The first path is the assignment identity and defaults to the current directory.
- `--data-dir` is the base for logs, artifacts, and recordings. The upstream CLI uses `/opt/cursor` when that path is writable, otherwise `~/.local/share/cursor-agent`. Env: `CURSOR_DATA_DIR`.
- `--idle-release-timeout` is seconds to stay connected after idle before exiting 0 so a supervisor can start a clean process. This build's default is 3600, not unlimited. `0` turns idle release off. Env: `CURSOR_WORKER_IDLE_RELEASE_TIMEOUT`.
- `--management-addr` opens an optional HTTP listener for `/healthz`, `/readyz`, and `/metrics`.
- `--auth-token-file` reads a worker token from a file. Prefer an environment variable to a secret on argv.
- `start --verbose` adds startup logs. It is the only `start`-specific flag.
- `--debug` on `worker` prints diagnostics before bridge mode.
- Opt-in, off unless we pass them: `--computer-use`, `--display`, `--share-desktop`, `--connect-workspace`, `--identity-socket`.

A second process on the same data directory needs `--wait`. Named workers need separate data directories.

## Machine-readable output

| Command | JSON? | Notes |
| --- | --- | --- |
| `agent status --format json` / `agent whoami --format json` | Yes | Works without login |
| `agent about --format json` | Yes | Works without login; includes a latest-version status |
| `agent worker debug --json` | Yes | Requires auth. Without auth the output is a text error even with `--json` |
| `agent team list --format json` | Yes | Requires auth |
| `agent --print --output-format json` | Yes | Interactive coding agent, not the worker |
| `agent worker start` | No | Human logs only. `--verbose` adds more text |
| `agent login` / `logout` / `update` | No | Text only |

Logged-out `status` prints a not-logged-in line and exits 0, not 1. JSON reports `status` unauthenticated, `isAuthenticated` false, both token flags false, and the same message.

Logged-out `about` exits 0. JSON fields observed: `cliVersion`, `latestVersion`, `latestStatus` (`up_to_date` on this install), `model`, `subscriptionTier` (null), `osPlatform`, `osArch`, `userEmail` (null), `terminalProgram`, `shell`, `lastRequestId`.

Logged-out `worker debug` (text and `--json`) exits 1 and asks for `agent login` or an API key. A placeholder key also exits 1. When the key comes from `CURSOR_API_KEY`, the message says so. There is no JSON body.

Names seen in the CLI's debug formatter, not from a successful signed-in report: `auth` / `method`, `bridge` (`ghostMode`, `bridgeType`, `usedPrivacyFallback`, `privacyError`), `worker` / `workerId`, `computerUse`, `desktopShare`, `visibilityProbe` (`nonPrivacy`, `privacy`, `sampleWorkers` / `sampleWorkerIds`), `warnings`, plus computer-use binary checks. Treat the schema as unstable until a signed-in report is captured.

## Status versus liveness

`agent status` / `agent whoami` report authentication, not worker health.

There is no `agent worker status`, `agent worker list`, or `agent worker stop`.

worker-dock treats a worker as alive from:

1. The OS service or task, plus the child PID.
2. Optional `--management-addr`: localhost `GET /readyz`, `GET /healthz`, and `GET /metrics`. The app uses `/readyz` and `/healthz` (plus the `Private worker started` wrapper.log line) for the connected chip, and parses documented `/metrics` gauges for live / idle / agent-running. That is status, not a network preflight.
3. After auth, `agent worker debug --json` on the Preflight screen. It still needs a saved name and folders.
4. Not `GET /v0/private-workers` with a personal key (see REST below).

## Exit codes observed

| Situation | Exit |
| --- | --- |
| `--help`, `--version`, `update` when already latest | 0 |
| `status` / `whoami` / `about` while logged out | 0 |
| `worker start` / `worker debug` with no auth | 1 |
| Invalid API key on `worker start` / `debug` | 1 |
| Unknown option, unknown subcommand, or bad `--format` | 1 |
| `team list` while logged out | 1 |
| `models` while logged out | 0 (prints an auth error) |
| `timeout` killed `agent login` | 124 (our timeout, not the CLI) |

Idle release was not exercised. Help for this build says the process exits 0 when `--idle-release-timeout` elapses. The default is 3600 seconds.

A dummy `--auth-token` on `worker start` warned that the token cannot refresh, then failed account validation. The recorded exit for that probe was 0. Do not trust the exit code alone for token failures.

## Log locations

| Source | Path |
| --- | --- |
| CLI debug sessions (this Linux spike) | `/tmp/cursor-agent-logs-<uid>/session-<iso>-<pid>-1.log` and a `latest.log` symlink. Mode `0600`. `CURSOR_AGENT_DISABLE_DEBUG_LOG` turns them off. worker-dock tails `latest.log` when it exists (Linux only; the Windows session path is unconfirmed). |
| worker-dock wrapper | Isolated `--data-dir` `wrapper.log`, rotated at 5 MB. Primary live tail. |
| `--data-dir` | Logs, artifacts, and recordings. Default `/opt/cursor` if writable, else `~/.local/share/cursor-agent`. Env `CURSOR_DATA_DIR`. |
| Windows startup failures | `%LOCALAPPDATA%\cursor-agent\worker-startup.log` when Git Bash is missing. |
| Worker stdout | Short human lines for startup, auth failure, and, with `--verbose`, the bridge host `https://api2.cursor.sh`. |
| Live workspace server | `~/.cursor-server/bin/<commit>/` after `--connect-workspace` allows a session. |

Some environments can write `/opt/cursor`. A normal Ubuntu desktop usually cannot. The upstream CLI defaults to that path when it is writable, and to `~/.local/share/cursor-agent` otherwise. worker-dock always passes its own `--data-dir`.

The CLI locks the data directory. A second worker on that directory needs `--wait`. Use one directory per named worker.

## Config files

Official CLI, not worker-dock:

| File | Role |
| --- | --- |
| `~/.cursor/cli-config.json` | CLI settings. Created on first run. Override with `CURSOR_CONFIG_DIR` or `$XDG_CONFIG_HOME/cursor/cli-config.json`. |
| `~/.config/cursor/auth.json` | Linux file credential store (`0600`) when login uses the file backend. |
| Project `.cursor/cli.json` | Permissions only. |
| `~/.cursor/mcp.json` | MCP servers the worker may load. |

Workers are flags and environment variables, not `cli-config.json`.

Proxy variables documented by Cursor: `HTTP_PROXY`, `HTTPS_PROXY`, `NODE_USE_ENV_PROXY=1`, `NODE_EXTRA_CA_CERTS`, and a CLI setting to force HTTP/1 for HTTP/2-hostile proxies.

## Updates

The CLI auto-updates by default. `agent update` on this already-current build printed a checking line and an up-to-date line, then exited 0.

`about --format json` reported `latestStatus` `up_to_date`. Other values were not observed. Packages come from `downloads.cursor.com/lab/<version>/<os>/<arch>/agent-cli-package.tar.gz`. The installer points `~/.local/bin/agent` at the new version directory and leaves older trees under `versions/`.

`update` has no documented pin flag. A changelog mentions `--disable-auto-update`. That flag was not in this build's root help. worker-dock still passes it and treats "unknown option" as unsupported.

## Login URL

`NO_OPEN_BROWSER=1 agent login` prints a `https://cursor.com` link, then waits. The path is `loginDeepControl` and the query includes `redirectTarget=cli`. Query values are single-use. Do not hard-code them. worker-dock opens only an allowlisted `https://cursor.com/loginDeepControl` URL with `redirectTarget=cli` (no other host, scheme, userinfo, or port).

## REST

Worker list and pool routes are documented on the [Cloud Agents API](https://cursor.com/docs/cloud-agent/api/endpoints). Paths still use `private-workers`. Auth is a pool service-account key. Other key types are rejected.

A personal API key cannot list workers or mint the documented user-scoped sub-token. Do not build Home status on `GET /v0/private-workers`. Creating an agent with a machine environment consumes a worker that is already running. It does not register one.

The pending-request stream and claim routes belong to `worker controller`, which this app does not call.

## Probes

| Invocation | Result |
| --- | --- |
| `agent worker start` with no auth | Exit 1, authentication required |
| Placeholder API key before `worker`, on `worker`, or after `start` | Exit 1, invalid key. `start --help` does not list the flag, but the process accepts it |
| Placeholder key in `CURSOR_API_KEY` | Exit 1, invalid key, and a line that the key came from the environment |
| `agent worker start --auth-token` with a dummy token | Accepted, refresh warning, then a validation error. Recorded exit 0 |
| Unknown worker flag or subcommand | Exit 1 |

Prefer `CURSOR_API_KEY` in the wrapper environment when a key is used. Never put the key on argv. This product's personal worker uses `agent login` and does not pass `--api-key`.

## What worker-dock can depend on

Stable enough to build on. Re-check when the pinned CLI changes:

- The install script and the Linux layout `~/.local/share/cursor-agent/versions/<ver>/` plus `~/.local/bin/agent`.
- `login`, `logout`, `status` / `whoami`, `about`, `update`, `worker start`, `worker debug`.
- `status --format json` and `about --format json` without login.
- `worker debug --json` after auth. worker-dock runs it from Preflight with `--name` and repeatable `--worker-dir`. Unauthenticated output is still the text error (exit 1).
- Auth: an `agent login` session, or `CURSOR_API_KEY` / `--api-key` for a personal user key.
- Flags we pass: `--name`, repeatable `--worker-dir`, `--data-dir`, `--idle-release-timeout`, `--management-addr`, and optionally `--computer-use`, `--display`, `--connect-workspace`, `--identity-socket`, `--share-desktop`, `--auth-token-file`, `--debug`, `start --verbose`. One personal worker is persisted and started through `worker-dock-run --supervised-run <id>` (systemd user unit or an interactive scheduled task). The app never writes `--pool` or `--api-key`.
- Localhost management endpoints. The app binds `127.0.0.1` only and probes `/readyz` and `/healthz`.
- `NO_OPEN_BROWSER=1` login URL on stdout.
- The Windows Git Bash requirement and `worker-startup.log` path, as documented by Cursor.
- Outbound hosts in the My Machines networking docs.
- Personal versus pool: omit the pool flag for My Machines.

## What is fragile

- Published parameter text and this build disagreed on the idle default (docs had said 0; this help defaulted to 3600 and exit 0). My Machines product text says workers stay up until stopped. Always pass `0` for a desktop worker.
- No worker process API. No list, status, or stop subcommand. The REST list is service-account-only.
- `--json` is not global. Unauthenticated `worker debug --json` is plain text, exit 1.
- `--auth-token` is accepted and documented, but missing from `agent --help` and `agent worker --help`. It is short-lived and does not refresh.
- Global options are accepted in more than one position. Generate one canonical argv.
- Exit codes disagree (`status` logged out is 0, `debug` logged out is 1, `models` logged out is 0).
- `--data-dir` defaults to `/opt/cursor` when that path is writable, otherwise `~/.local/share/cursor-agent`.
- One lock per data directory.
- Auto-update can change flags and JSON between worker-dock releases. `--disable-auto-update` was not in this build's root help. The app still passes it.
- Help text is not an API. Do not scrape it at runtime, and do not check it into this repo.
- Computer use, desktop share, and connect-workspace are OS-specific, opt-in, and still moving. Debug does not confirm macOS Accessibility or Screen Recording.
- Login storage differs by OS (file versus keychain). Confirm the Windows auth file path on a Windows spike before relying on it.
- The CLI is about 561M. The supervisor is a few megabytes. We do not vendor the CLI.
- Controller and pool flags are easy to pass by mistake. Never pass a pool flag in this product.
- CLI help and logs use the Cursor name throughout. This app's UI does not copy that branding.

## Reproduce

```bash
curl https://cursor.com/install -fsS | bash
export PATH="$HOME/.local/bin:$PATH"
agent --version
agent status --format json
agent about --format json
agent update
# no login:
agent worker start          # exit 1
agent worker debug --json   # exit 1, text
```
