# Getting started

Build the binary, try the browserless mock, then drive a real browser.

Before any syntax, the mental model. You describe a test as a YAML
**manifest** — a list of tasks, each a sequence of steps. htest reads the
manifest, builds a **run graph** from the prerequisites you declare between
tasks, and executes that graph against a **backend**. There are two backends,
and the whole point is that they are interchangeable: you develop and shape a
manifest against the browserless **mock**, then swap in a real **WebDriver**
browser to run it for real. Nothing but the backend changes. This page walks
that path end to end — build, mock, real browser, your first manifest — and
finishes with the full CLI and a troubleshooting table.

## 1. Build

htest is a single self-contained binary; one build gives you the runner and
both backends.

```bash
cargo build --release        # runner + mock + WebDriver backends
```

The binary lands at `target/release/htest`. Put it on your `PATH`, or run it by
path. Every example below assumes it is on your `PATH`.

## 2. Try it with zero setup (mock backend)

The fastest way to learn htest is the **mock** backend, which is the default and
needs no browser at all.

Because it never touches a browser, the mock is instant and completely
deterministic — ideal for developing a manifest's *structure* (its steps,
dependencies, and idempotency gates) before you point it at anything real. Start
with `graph` to see the order htest derived, then `run` to execute it:

```bash
htest graph examples/manifest.yaml     # print the run order, no execution
htest run   examples/manifest.yaml     # run against the mock backend
```

The mock's rule is simple: it assumes every selector exists, *except* the ones
you explicitly seed as absent through the `HTEST_MOCK_ABSENT` environment
variable (comma-separated). A `click` or `fill` on an absent selector removes it
from the absent set — modelling "it exists now" — which is exactly how you
exercise the idempotency gate without a live page:

```bash
# pretend Alice's row does NOT exist yet, so create_user runs
HTEST_MOCK_ABSENT=.row-alice htest run examples/manifest.yaml --keep-going
```

## 3. Drive a real browser

When the structure is right, run the same manifest against a real browser. htest
does not launch or manage the browser itself — it talks to a running **WebDriver
server**, so you start one, leave it running, and point htest at it.

```bash
# Terminal 1 — the driver (pick one)
geckodriver --port 4444            # Firefox
chromedriver --port=4444           # Chrome

# Terminal 2 — the run. Drop --headless to watch it.
htest run examples/wikipedia.yaml \
  --driver webdriver --browser firefox --headless
```

> **Which driver?** Prefer `geckodriver` (Firefox) — it's largely
> version-independent, so it keeps working across browser updates.
> `chromedriver` must match your installed Chrome's major version
> (Chrome-for-Testing / Selenium Manager automate that pairing). The manifest
> and the tool never change between backends — you only ever swap the *server*.

## 4. Write your first manifest

With both backends in hand, here is the smallest manifest worth writing — a
smoke test that opens a page, waits for it to render, asserts something is
there, and captures proof.

```yaml
# smoke.yaml
vars:
  BASE_URL: "https://example.com"
tasks:
  - name: homepage_loads
    steps:
      - goto: "{{ BASE_URL }}"                    # navigate
      - wait_for: "h1"                            # wait until it renders
      - assert: { selector: "h1", exists: true }  # check it is there
      - screenshot: "home.png"                    # capture proof
```

`vars:` holds values you can reference as `{{ VAR }}` anywhere in the document;
`{{ BASE_URL }}` is substituted before the YAML is parsed. Run it against a real
browser:

```bash
htest run smoke.yaml --driver webdriver --browser firefox
```

From here, each guide page takes one of these ideas further:
[Actions](actions.md) covers every step type, [Selectors](selectors.md) covers
targeting elements, and [Waits](waits.md) explains why there are no fixed
sleeps.

## Commands

htest has two commands you will use day to day (a third, `playbook`, is covered
under [Playbooks](playbooks.md)).

```bash
htest graph <manifest>... [--env FILE]     # print run graph (no browser)
htest run   <manifest>... [flags]          # run one or more manifests
```

`graph` is a dry run: it builds and prints the execution order without touching
a browser, which is the quickest way to confirm your `needs` prerequisites
resolve the way you expect. Pass several manifests to either command — their
tasks merge into a single run graph, so tasks in one file can depend on tasks in
another.

### `run` flags

Every flag has a sensible default, so a bare `htest run manifest.yaml` already
works against the mock. Reach for these when you move to a real browser or tune
a run:

| Flag | Default | Meaning |
|------|---------|---------|
| `--driver mock\|webdriver` | `mock` | Browser backend. |
| `--webdriver-url <url>` | `http://localhost:4444` | WebDriver server URL. |
| `--browser firefox\|chrome` | `firefox` | Target browser (capability key). |
| `--headless` | off | Launch without a visible window. |
| `--window <WxH>` | — | Window size, e.g. `1280x800`. |
| `--browser-arg <arg>` | — | Extra browser CLI arg (repeatable). |
| `--env <file>` | manifest `env:` or `./.env` | .env file to load. |
| `--screenshots <dir>` | `screenshots` | Screenshot output directory. |
| `--keep-going` | off | Don't stop at the first failure. |
| `--shot-on-fail` | off | Screenshot each failed task ([details](screenshots.md)). |
| `--timeout <ms>` | `5000` | Implicit wait budget ([details](waits.md)). |

Exit code is non-zero if any task failed, which makes htest safe to drop into
CI. On exit — pass or fail — the WebDriver session is always closed, so a failed
run never leaves a dangling session behind.

## Troubleshooting

Most first-run problems fall into four buckets. Match the symptom, apply the
fix.

| Symptom | Fix |
|---------|-----|
| `Session is already started` | A previous run was hard-killed (Ctrl-C/crash). Restart the WebDriver server once. htest closes its own session on every clean exit. |
| Slow before it navigates | A text-matched selector is scanning too many elements. Narrow the CSS first — see [Selectors](selectors.md). |
| Assert fails intermittently | Async content. Raise `--timeout`, or gate on the real signal with `wait_for` — see [Waits](waits.md). |
| `missing variable ...` | A `{{ VAR }}` had no value. Define it in `vars:`, a `.env`, or the environment. |

---

← [Overview](README.md) · [Tutorial →](tutorial.md)
