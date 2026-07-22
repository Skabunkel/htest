# htest documentation

Browser integration tests defined as YAML manifests — ansible/drill style.
htest builds a run graph from task prerequisites, runs in dependency order, and
makes reruns safe with idempotent tasks. Mantras: **speed** and
**repeatability**.

> There's also a styled HTML version of these docs — open [`index.html`](index.html)
> in a browser (`cargo doc`-style, but for the manifest language).

## Guide

| Page | What it covers |
|------|----------------|
| [Getting started](getting-started.md) | Build, run the mock backend, drive a real browser, CLI flags, troubleshooting. |
| [Tutorial](tutorial.md) | Build a real multi-step test end to end: navigate → act → assert → chain → idempotency → `loop` → multi-file. |
| [Actions & steps](actions.md) | Reference for every step type: `goto`, `click`, `fill`, `upload`, `assert`, `wait`/`wait_for`, `screenshot`. |
| [Selectors](selectors.md) | Plain CSS vs. structured hierarchical selectors; text matching; the "delete button on Alice's row" pattern. |
| [Waits & timing](waits.md) | Implicit waits, `pageLoadStrategy: eager`, and `wait` vs `wait_for`. |
| [Screenshots](screenshots.md) | The `screenshot` step and automatic `--shot-on-fail` captures. |
| [Loops](loops.md) | `loop:` over a range or a list: forms, the loop variable, `needs` fan-out, naming rules, idempotent reruns. |
| [Playbooks](playbooks.md) | Collect suites into one CI run: settings, file-level `needs`, managed driver, `max_run_time`. |

## 30-second taste

```yaml
# smoke.yaml
vars:
  BASE_URL: "https://example.com"
tasks:
  - name: homepage_loads
    steps:
      - goto: "{{ BASE_URL }}"
      - wait_for: "h1"
      - assert: { selector: "h1", exists: true }
      - screenshot: "home.png"
```

```bash
cargo build --release
geckodriver --port 4444 &
target/release/htest run smoke.yaml --driver webdriver --browser firefox
```
