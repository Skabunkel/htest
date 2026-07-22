# Playbooks

A manifest is a suite of tasks — the base unit you build on. A *playbook*
collects several suites, sets run-wide settings once, and orders them. In CI you
point at one file.

```bash
htest playbook examples/ci.playbook.yaml
```

## Shape

Two top-level keys: `settings` (optional) and `suites` (required).

```yaml
settings:
  driver: webdriver
  browser: firefox
  headless: true
  manage_driver: true      # htest starts & stops geckodriver itself
  shot_on_fail: true
  max_run_time: 300        # abort the rest if the whole run exceeds 300s

suites:
  - file: auth.yaml
  - file: reporting.yaml
    needs: [auth.yaml]     # all of auth.yaml before any of reporting.yaml
```

## Suites & file-level `needs`

Each entry under `suites` is a manifest `file` (path relative to the playbook's
own directory). Its optional `needs` lists *other suite files* that must finish
first.

`needs` is **file-level**: every task in a needed suite becomes a prerequisite
of every task in the dependent suite — "all of A before all of B". This is
coarser than a manifest's per-task `needs` (`namespace:task`), and the two
compose: task-level edges within files still hold, and the playbook adds the
cross-file ones. Any cycle you create is caught before the run starts.

> A suite's namespace is its manifest's top-level `id:` (falling back to the
> file stem). Two suites with the same namespace is an error — give each a
> distinct `id:`.

## Settings & precedence

Settings layer in one direction: **CLI flag > playbook `settings` > built-in
default**. Any flag you pass to `htest playbook` overrides the same field in the
file, so one playbook serves both local and CI runs.

```bash
# Playbook says driver: mock; force a real headless Firefox for CI:
htest playbook ci.playbook.yaml --driver webdriver --browser firefox --headless --shot-on-fail
```

| Setting | Default | Meaning |
|---------|---------|---------|
| `driver` | `mock` | `mock` or `webdriver`. |
| `webdriver_url` | `http://localhost:4444` | Remote driver URL (ignored when `manage_driver` is on). |
| `browser` | `firefox` | `firefox` or `chrome`. |
| `headless` | `false` | Run without a visible window. |
| `window` | — | Viewport as `WxH`, e.g. `1280x800`. |
| `browser_args` | — | Extra args passed to the browser. |
| `env` | — | Path to a dotenv file for template variables. |
| `screenshots` | `screenshots` | Output directory for screenshots. |
| `shot_on_fail` | `false` | Capture the viewport whenever a task fails. |
| `keep_going` | `false` | Run every task instead of stopping at the first failure. |
| `timeout` | `5000` | Implicit-wait budget, milliseconds. |
| `manage_driver` | `false` | htest spawns and kills the driver (requires `driver: webdriver`). |
| `driver_path` | on `PATH` | Override the driver executable. |
| `driver_port` | `4444` | Port the managed driver listens on. |
| `max_run_time` | — | Wall-clock cap for the whole run, seconds. Once exceeded, remaining tasks fail. |

## Managed driver

Set `manage_driver: true` (with `driver: webdriver`) and htest starts the driver
for you — `geckodriver` for Firefox, `chromedriver` for Chrome — waits for its
port to accept connections, and kills it when the run ends. No separate "start
the driver" step in your CI job.

```yaml
settings:
  driver: webdriver
  browser: firefox
  manage_driver: true
  driver_port: 4444           # optional; default 4444
  driver_path: geckodriver    # optional; else found on PATH
```

The session is always closed before the driver process is killed, so no browser
is left orphaned between runs.

## Failing the whole run on time

`max_run_time` is a global wall-clock budget in seconds, checked between tasks.
If the run exceeds it, every remaining task is marked failed with a "run aborted"
reason — so a hung suite can't stall a CI job indefinitely.

## Exit code

Like `htest run`, the process exits non-zero if any task failed — the single
signal a CI pipeline needs.

---

← [Loops](loops.md) · [Overview](README.md)
