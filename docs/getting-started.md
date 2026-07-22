# Getting started

Build the binary, try the browserless mock, then drive a real browser.

## 1. Build

```bash
cargo build --release        # runner + mock + WebDriver backends
```

The binary lands at `target/release/htest`. Put it on your `PATH`, or run it by
path.

## 2. Try it with zero setup (mock backend)

The default **mock** backend needs no browser. Use it to develop a manifest's
*structure* — steps, dependencies, idempotency — before pointing at a real
browser.

```bash
htest graph examples/manifest.yaml     # print the run order, no execution
htest run   examples/manifest.yaml     # run against the mock backend
```

The mock assumes every selector exists, except those you seed absent — which
lets you exercise the idempotency gate:

```bash
# pretend Alice's row does NOT exist yet, so create_user runs
HTEST_MOCK_ABSENT=.row-alice htest run examples/manifest.yaml --keep-going
```

## 3. Drive a real browser

htest talks to a running **WebDriver server**. Start one, leave it running, and
point htest at it.

```bash
# Terminal 1 — the driver (pick one)
geckodriver --port 4444            # Firefox
chromedriver --port=4444           # Chrome

# Terminal 2 — the run. Drop --headless to watch it.
htest run examples/wikipedia.yaml \
  --driver webdriver --browser firefox --headless
```

> **Which driver?** Prefer `geckodriver` (Firefox) — it's largely
> version-independent. `chromedriver` must match your installed Chrome's major
> version (Chrome-for-Testing / Selenium Manager automate that). You only ever
> swap the *server* — never the manifests or the tool.

## 4. Write your first manifest

```yaml
# smoke.yaml
vars:
  BASE_URL: "https://example.com"
tasks:
  - name: homepage_loads
    steps:
      - goto: "{{ BASE_URL }}"
      - wait_for: "h1"                            # wait until it renders
      - assert: { selector: "h1", exists: true }
      - screenshot: "home.png"
```

```bash
htest run smoke.yaml --driver webdriver --browser firefox
```

## Commands

```bash
htest graph <manifest>... [--env FILE]     # print run graph (no browser)
htest run   <manifest>... [flags]          # run one or more manifests
```

Pass several manifests to either command — their tasks merge into a single run
graph.

### `run` flags

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

Exit code is non-zero if any task failed. On exit — pass or fail — the WebDriver
session is always closed, so a failed run never leaves a dangling session
behind.

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `Session is already started` | A previous run was hard-killed (Ctrl-C/crash). Restart the WebDriver server once. htest closes its own session on every clean exit. |
| Slow before it navigates | A text-matched selector is scanning too many elements. Narrow the CSS first — see [Selectors](selectors.md). |
| Assert fails intermittently | Async content. Raise `--timeout`, or gate on the real signal with `wait_for` — see [Waits](waits.md). |
| `missing variable ...` | A `{{ VAR }}` had no value. Define it in `vars:`, a `.env`, or the environment. |

---

← [Overview](README.md) · [Tutorial →](tutorial.md)
