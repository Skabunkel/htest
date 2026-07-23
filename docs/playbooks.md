# Playbooks

A manifest is a *suite* of tasks — the base unit you build on. A **playbook** is
the layer above: it collects several suites into one run, sets the run-wide
settings once, and orders the suites relative to each other. The payoff is a
single entry point — CI (or you) points at one file and gets the whole test run,
configured consistently:

```bash
htest playbook examples/ci.playbook.yaml
```

## Shape

A playbook has just two top-level keys: `settings` (optional) and `suites`
(required). `settings` configures the whole run; `suites` lists the manifests to
run and how they depend on each other.

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

The rest of the page walks down from here: first the `suites` list and how
cross-file ordering works, then `settings` and where their values come from, and
finally the two settings that most change how a CI run behaves — the managed
driver and the wall-clock cap.

## Suites & file-level `needs`

Each entry under `suites` names a manifest `file` (resolved relative to the
playbook's *own* directory, not your working directory). Its optional `needs`
lists *other suite files* that must finish before this one starts.

The important thing to understand is that this `needs` is **file-level**, coarser
than the per-task `needs` inside a manifest. When suite B needs suite A, *every*
task in A becomes a prerequisite of *every* task in B — "all of A before all of
B". The two levels compose cleanly: the task-level edges *within* each file still
hold exactly as written, and the playbook simply adds these cross-file edges on
top. Any cycle you introduce across files is caught before the run starts.

> A suite's namespace is its manifest's top-level `id:` (falling back to the file
> stem if there is no `id:`). This namespace is how tasks are reported and
> referenced, so two suites resolving to the *same* namespace is an error — give
> each a distinct `id:`.

## Settings & precedence

`settings` in the file is only one of three layers. They resolve in one
direction — **CLI flag > playbook `settings` > built-in default** — so a value
you pass on the command line always wins, and a field you set nowhere falls back
to its default. That is what lets a single playbook serve both local and CI runs:
keep the file configured for local work and override just the CI-specific fields
at the command line.

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

By default you are responsible for having a WebDriver running before htest
connects. Setting `manage_driver: true` (with `driver: webdriver`) hands that job
to htest instead: it starts the right driver for your browser — `geckodriver`
for Firefox, `chromedriver` for Chrome — waits for its port to accept
connections, and kills it when the run ends. That removes the separate "start the
driver" step from your CI job.

```yaml
settings:
  driver: webdriver
  browser: firefox
  manage_driver: true
  driver_port: 4444           # optional; default 4444
  driver_path: geckodriver    # optional; else found on PATH
```

Shutdown is ordered carefully: the browser session is always closed *before* the
driver process is killed, so a run never leaves an orphaned browser behind
between invocations.

## Failing the whole run on time

A hung page or a driver that stops responding could otherwise stall a CI job
indefinitely. `max_run_time` is the guard: a global wall-clock budget in seconds,
checked *between* tasks. Once the elapsed time exceeds it, every remaining task
is marked failed with a "run aborted" reason rather than being attempted — so the
run always terminates and reports.

## Exit code

Finally, the signal CI actually consumes. Like `htest run`, the `htest playbook`
process exits non-zero if any task failed — one bit that tells the pipeline
whether to go green or red.

---

← [Loops](loops.md) · [Overview](README.md)
