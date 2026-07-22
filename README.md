# htest — WebDriver integration test runner

CLI that runs browser integration tests defined as YAML manifests (ansible /
[drill] style). Builds a run graph from task prerequisites, runs in dependency
order, supports idempotent tasks, `.env` variables, screenshots, **multiple
manifest files**, and **cross-file dependencies**.

Mantras: **speed** and **repeatability**. Idempotent tasks make reruns safe;
the DAG lets independent tasks run in parallel (executor coming — see roadmap).

## Documentation

Full guide in [`docs/`](docs/README.md) — also a styled HTML site
([`docs/index.html`](docs/index.html), open in a browser):

- [Getting started](docs/getting-started.md) — build, mock, real browser, flags, troubleshooting
- [Tutorial](docs/tutorial.md) — build a multi-step test end to end
- [Actions & steps](docs/actions.md) — every step type: `goto`, `click`, `fill`, `upload`, `assert`, `wait`/`wait_for`, `screenshot`
- [Selectors](docs/selectors.md) — plain CSS vs. hierarchical text-scoped selectors
- [Waits & timing](docs/waits.md) — `wait` vs `wait_for`, implicit waits
- [Screenshots](docs/screenshots.md) — the `screenshot` step and `--shot-on-fail`
- [Loops](docs/loops.md) — repeat a task over a range or a list (`user1`…`user5`)
- [Playbooks](docs/playbooks.md) — collect suites into one CI run: settings, file-level `needs`, managed driver

## Install / build

```bash
cargo build --release        # runner + mock + WebDriver backends
```

Binary: `target/release/htest`.

## Quickstart

Zero setup — the default `mock` backend needs no browser, so you can write and
debug a manifest's *structure* (steps, deps, idempotency) before touching a real
browser:

```bash
htest graph examples/manifest.yaml      # print the run order, no execution
htest run   examples/manifest.yaml      # run against the mock backend
```

Drive a real browser in three steps:

```bash
# 1. Start a WebDriver server (leave it running in its own terminal).
geckodriver --port 4444                 # Firefox

# 2. Point htest at it. Drop --headless to watch the browser work.
htest run examples/wikipedia.yaml \
  --driver webdriver --browser firefox --headless

# 3. Read the report. Screenshots (if any) land in ./screenshots.
```

**Or take the guided tour.** [`examples/tour.yaml`](examples/tour.yaml) drives a
self-contained demo app in [`demo/`](demo/) — popups, spinners, a form with a
file picker, checkbox "snake", scrolling — with no network at all (it runs over
`file://`). Great for seeing every feature execute:

```bash
geckodriver --port 4444 &
htest run examples/tour.yaml --driver webdriver --browser firefox --keep-going --shot-on-fail
# 8 pass, 1 fails on purpose (the `broken` task) to show the run continues.
```

Write your own manifest:

```yaml
# smoke.yaml
vars:
  BASE_URL: "https://example.com"
tasks:
  - name: homepage_loads
    steps:
      - goto: "{{ BASE_URL }}"
      - wait_for: "h1"                              # wait until it renders
      - assert: { selector: "h1", exists: true }
      - screenshot: "home.png"
```

```bash
htest run smoke.yaml --driver webdriver --browser firefox
```

## Commands

```bash
htest graph    <manifest>... [--env FILE]     # print run graph (no browser)
htest run      <manifest>... [flags]          # run one or more manifests
htest playbook <playbook> [override flags]    # run a suite collection (CI)
```

Multiple manifests are merged into a single run graph.

`run` flags:

| flag | default | meaning |
|------|---------|---------|
| `--driver mock\|webdriver` | `mock` | browser backend |
| `--webdriver-url <url>` | `http://localhost:4444` | WebDriver server |
| `--browser firefox\|chrome` | `firefox` | target browser (capability key) |
| `--headless` | off | launch browser headless |
| `--window <WxH>` | — | window size, e.g. `1280x800` |
| `--browser-arg <arg>` | — | extra browser CLI arg (repeatable) |
| `--env <file>` | `<manifest env:>` or `./.env` | .env to load |
| `--screenshots <dir>` | `screenshots` | screenshot output dir |
| `--keep-going` | off | don't stop at first failure |
| `--shot-on-fail` | off | screenshot each failed task |
| `--timeout <ms>` | `5000` | implicit wait: max time to re-check an assert/element |

Exit code is non-zero if any task failed. On exit — success or failure — the
WebDriver session is always closed (destructors run before the process exits),
so a failed run never leaves geckodriver's single session dangling.

### Timing & waits

Pages settle asynchronously (navigation, `fetch`, framework renders). Rather
than sprinkling fixed `wait:` guesses, the engine **implicitly waits**: before
a click/fill and on every assert it re-checks (150 ms poll) until the condition
holds or `--timeout` expires. `assert exists: false` polls until the element
*disappears* — so "row gone after delete" is race-free too. WebDriver uses
`pageLoadStrategy: eager`, so `goto` returns at DOMContentLoaded instead of
waiting on every image. Keep an explicit `wait:` only when you truly need a
fixed pause.

## Playbooks

A manifest is a *suite* — the base unit. A **playbook** collects several suites,
sets run-wide settings once, and orders them with file-level `needs`. In CI you
point at a single file:

```bash
htest playbook examples/ci.playbook.yaml
```

```yaml
settings:                    # all optional; CLI flags override these
  driver: webdriver
  browser: firefox
  headless: true
  manage_driver: true        # htest starts & kills geckodriver itself
  shot_on_fail: true
  max_run_time: 300          # seconds; remaining tasks fail if exceeded

suites:
  - file: auth.yaml
  - file: reporting.yaml
    needs: [auth.yaml]        # all of auth.yaml before any of reporting.yaml
```

- **Precedence**: CLI flag > playbook `settings` > default. Every `run` flag has
  a `playbook` counterpart (`--driver`, `--headless`, `--shot-on-fail`, …).
- **`needs` is file-level** — "all of A before all of B" — and composes with the
  per-task `needs` inside each manifest. Cycles are caught before the run.
- **`manage_driver: true`** (with `driver: webdriver`) spawns the driver, waits
  for its port, and kills it on exit; the session closes first so nothing is
  orphaned. Override the binary/port with `driver_path` / `driver_port`.
- **`max_run_time`** caps the whole run (checked between tasks).

Full reference: [`docs/playbooks.md`](docs/playbooks.md). Exit code is non-zero
if any task failed.

## Backends

The engine talks to a `Browser` trait. Backends:

- **mock** (default) — deterministic, no browser. For developing manifests +
  orchestration. Assumes every selector exists except those seeded absent via
  `HTEST_MOCK_ABSENT` (comma-separated), which drives the idempotency gate:

  ```bash
  htest run examples/manifest.yaml                       # user "exists" -> skip
  HTEST_MOCK_ABSENT=.row-alice htest run examples/manifest.yaml --keep-going
  ```

- **webdriver** — drives a real browser via a running WebDriver server. Start
  one first, then point `--webdriver-url` at it:

  ```bash
  geckodriver --port 4444          # Firefox
  chromedriver --port=4444         # Chrome
  htest run examples/manifest.yaml --driver webdriver --webdriver-url http://localhost:4444
  ```

  **Version matching**: `geckodriver` is largely version-independent — prefer
  it to avoid the pain. `chromedriver` must match Chrome's major version
  (Chrome-for-Testing / Selenium Manager automate this). You swap the *server*,
  never the manifests or the tool.

  Capabilities: `--headless`, `--window WxH`, and repeated `--browser-arg`
  are translated to the right capability key (`moz:firefoxOptions` /
  `goog:chromeOptions`). Window size is applied via `setWindowRect` after
  launch (uniform across browsers). Example:

  ```bash
  htest run examples/manifest.yaml --driver webdriver \
    --browser chrome --headless --window 1280x800 --browser-arg --disable-gpu
  ```

## Manifest

```yaml
id: app                       # optional namespace (default: file stem)
vars:                         # highest precedence (these win over .env)
  BASE_URL: "http://localhost:8080"
  USERNAME: "alice"

tasks:
  - name: login
    steps:
      - goto:  "{{ BASE_URL }}/login"
      - fill:  { selector: "#user", value: "{{ USERNAME }}" }
      - click: "#submit"
      - assert: { selector: "#dashboard", exists: true }
      - screenshot: "after-login.png"

  - name: create_user
    needs: [login]            # same-file prerequisite
    # loop: { var: n, from: 1, to: 5 }   # optional: repeat the task per item
    idempotent:
      check: { selector: ".row-{{ USERNAME }}", exists: true }
      on_exists: skip         # skip | continue | fail
    steps:
      - goto:  "{{ BASE_URL }}/users/new"
      - fill:  { selector: "#name", value: "{{ USERNAME }}" }
      - click: "#create"
      - assert: { selector: ".row-{{ USERNAME }}", exists: true }
```

### Steps

| step | form |
|------|------|
| `goto` | `goto: "<url>"` — a relative path or `file://` URL is resolved against the CWD (then the manifest's directory) |
| `click` | `click: <selector>` |
| `fill` | `fill: { selector: <selector>, value: "<v>" }` |
| `upload` | `upload: { selector: <file-input>, path: "<file>" }` — set a file `<input>`; a relative `path` resolves against the CWD, then the manifest's directory (absolute passes through) |
| `assert` | `assert: { selector: <selector>, exists: true, text: "<opt>" }` |
| `screenshot` | `screenshot: "<name.png>"` |
| `wait` | `wait: <ms>` — unconditional fixed pause (last resort) |
| `wait_for` | `wait_for: <selector>` (until present) or `wait_for: { selector: <sel>, until: present\|absent, timeout: <ms> }` |

`upload` drives a file `<input type=file>`: it resolves `path` to an absolute
native path and checks the file exists (clear error if not) before setting it
via the same mechanism as `fill`. Use a relative path so manifests stay
portable — `upload: { selector: "#avatar", path: "demo/assets/sample.txt" }`.
Local drivers only (the file must be on the same machine as the browser).

### Portable file paths

Local pages and upload files are written as relative paths, never as an
absolute path baked to one machine. On load, anything that is not an
`http(s)://`/`data:`/`about:` URL is expanded to an absolute `file://` URL:

```yaml
vars:
  BASE_URL: "file://./demo"           # relative -> absolute file:// at load
tasks:
  - name: home
    steps:
      - goto: "{{ BASE_URL }}/index.html"
      - goto: "./local/page.html"     # bare relative path works too
```

Resolution order is **the directory you run htest from, then the manifest's own
directory** if the file isn't there. So relative paths mean what they usually
mean, and a manifest run from elsewhere still finds files sitting beside it.
`~`, Windows paths (`C:\…`) and spaces are handled. A schemeless value that is
*not* path-shaped (`localhost:8080/app`) is left alone. See
[docs/actions.md](docs/actions.md#relative-file-locations).

`wait_for` is the condition-based wait — prefer it over fixed `wait:`. It polls
until the selector is `present` (default) or `absent`, then continues; on
timeout the task **fails**. `until: absent` is the spinner pattern — block until
a loading indicator clears:

```yaml
- click: "#load-report"
- wait_for: { selector: ".spinner", until: absent, timeout: 15000 }
- assert: { selector: ".report", exists: true }
```

### Selectors

A `<selector>` is either a **plain CSS string** or a **structured
hierarchical** form.

Plain CSS — all combinators work as-is (this IS hierarchy):

```yaml
click: ".tables button"        # a button anywhere inside .tables
click: ".tables > tr > .del"   # child combinator
```

Structured form adds what CSS cannot express — **text matching** and explicit
**scoping/descent**:

```yaml
selector:
  css: ".row"           # match at this level (default: any element)
  contains: "Alice"     # keep only elements whose text contains this
  text: "Exact"         # or: trimmed text equals this exactly
  nth: 0                # pick the Nth match (default: first)
  find:                 # descend INTO the match, resolve recursively
    css: "button"
    contains: "Delete"
```

That reads: "the `button` containing 'Delete' inside the `.row` whose text
contains 'Alice'" — the classic "click the delete button on Alice's row" that
plain CSS can't target. `find` nests to any depth. Resolution order per level:
CSS match → text filter → `nth` → descend. See `examples/fixtures.yaml` for a
working, browser-verified example.

### Multiple files & cross-file dependencies

Each manifest gets a **namespace**: its top-level `id:`, else the filename
stem. A task's canonical id is `namespace:name`. A `needs` entry:

- bare (`login`) → resolves within the same file's namespace.
- qualified (`base:login`) → refers to a task in another file.

```yaml
# reporting.yaml
tasks:
  - name: verify_audit_log
    needs: [manifest:edit_user]   # waits for edit_user from manifest.yaml
    steps: [ ... ]
```

```bash
htest run examples/manifest.yaml examples/reporting.yaml
```

Each file is templated with its own `vars`/`.env`; the process environment is
shared. Namespaces must be unique across the passed files.

### Idempotency

Each task may declare a `check` predicate run **before** its steps. If it holds
(resource already in the expected state), `on_exists` decides: `skip` (default),
`continue`, or `fail`. This is "create if not exists" and makes reruns safe.

### Prerequisites & blocking

`needs` defines edges; execution order is a topological sort. If a task fails,
its dependents (in any file) are **blocked** rather than run against a broken
precondition.

### Loops

`loop:` repeats a task over a range or a list. It is **expanded when the
manifest loads** — one ordinary task per item, named `<task>[<item>]` — so the
graph, the run order and the report all show the real tasks:

```yaml
- name: create_user
  loop: { var: n, from: 1, to: 5 }      # user1 … user5 (both ends included)
  needs: [login]
  steps:
    - goto: "{{ BASE_URL }}/users/new"
    - fill: { selector: "#name", value: "user{{ n }}" }
    - click: "#create"
    - assert: { selector: "#sum-name", text: "user{{ n }}" }
```

```
$ htest graph examples/loops.yaml
  layer 1 (parallel-safe): users:create_user[1], users:create_user[2], …
```

| form | meaning |
|------|---------|
| `loop: { var: n, from: 1, to: 5 }` | `1 2 3 4 5` — inclusive; `from: 0, to: 4` gives `user0…user4` |
| `loop: { var: n, from: 0, to: 10, step: 2 }` | `0 2 4 6 8 10`; a negative `step` (or `to < from`) counts down |
| `loop: { var: u, items: [alice, bob] }` | one task per item |
| `loop: [alice, bob]` | shorthand; the variable is `item` |

Inside the task, `{{ n }}` (or whatever `var:` names) is the current item, plus
`{{ loop_index }}` (0-based) and `{{ loop_index1 }}` (1-based). Bounds may
themselves be templated — `to: "{{ USER_COUNT }}"` — so a single variable
resizes the run (from `vars`, or from `.env`/`--env` when `vars` doesn't set it).

`needs: [create_user]` waits for **every** iteration; name one directly —
`needs: ["create_user[3]"]` — to depend on just that one. The loop variable may
appear anywhere in the task *except* `name:` (htest adds the `[item]` suffix
itself, and a name that varied per iteration would make `needs:` ambiguous).

Full runnable example: [`examples/loops.yaml`](examples/loops.yaml).

### Variables

Template context, low → high precedence: process env → `.env` file →
manifest `vars`. The whole document is rendered with `{{ VAR }}` before parsing;
missing variables are a hard error.

## Troubleshooting

**`Session is already started`** — geckodriver allows a single session. It means
a previous run left one open. htest closes its session on every exit (success or
failure), so this only happens after a hard kill (Ctrl-C, crash) or a run from an
older build. Restart the WebDriver server once:

```bash
# stop geckodriver, then:
geckodriver --port 4444
```

**A run is slow before it navigates** — a text-matched selector
(`text:`/`contains:`) over a broad CSS match scans many elements. Narrow the CSS
first so fewer candidates need a text check:

```yaml
# slow on a link-heavy page: every <a> considered
click: { css: "a", text: "Next" }
# fast: scope to the region that holds the link
click: { css: "#pager a", text: "Next" }
```

`goto` itself returns at DOMContentLoaded (`pageLoadStrategy: eager`), not after
every image, so page load is rarely the bottleneck.

**An assert fails intermittently** — the content is async. The engine already
polls up to `--timeout` (default 5 s), but a slow endpoint may need more. Either
raise `--timeout`, or gate on the real signal with `wait_for` (e.g. wait for the
spinner to go `absent`) instead of asserting immediately.

**`chromedriver` won't start / version mismatch** — `chromedriver`'s major
version must match installed Chrome. Prefer `geckodriver` (version-independent).
If you must use Chrome, get a matching driver from Chrome-for-Testing and point
`--webdriver-url` at it. You never change the manifests — only the server.

**`missing variable ...`** — a `{{ VAR }}` had no value. Define it in the
manifest `vars:`, a `.env` file (auto-loaded from `./.env`, or `--env FILE`), or
the process environment.

## Layout

```
src/
  main.rs          CLI (clap): graph | run, multi-file loading + namespacing
  manifest.rs      YAML schema + custom step parsing
  loops.rs         `loop:` expansion into plain tasks (+ tests)
  paths.rs         relative file/URL resolution (CWD, then manifest dir)
  selector.rs      plain-CSS or structured hierarchical selectors (+ tests)
  template.rs      .env + {{ }} rendering (minijinja)
  plan.rs          merge files -> canonical ids, resolve cross-file deps
  graph.rs         petgraph DAG: build, cycle detect, parallel layers (+ tests)
  engine.rs        run loop: idempotency gate, steps, asserts, blocking
  browser/
    mod.rs         Browser trait + backend factory
    mock.rs        deterministic mock backend
    webdriver.rs   WebDriver backend (fantoccini)
```

## Roadmap

- Concurrent executor: run each graph layer in parallel (WebDriver session pool).
- **Stretch — record mode**: launch a browser, follow-along session; left-click
  an element to emit an `assert`/idempotency check into a manifest.

[drill]: https://github.com/fcsonline/drill
