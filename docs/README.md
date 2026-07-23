# htest documentation

htest runs browser integration tests that you define as YAML **manifests** —
ansible/drill style. Instead of a script that spells out every step in order,
you declare tasks and the prerequisites between them; htest assembles a **run
graph**, executes it in dependency order, and makes reruns safe with
**idempotent** tasks. Two mantras drive every design decision: **speed** (eager
page loads, batched DOM queries, and implicit waits instead of fixed sleeps) and
**repeatability** (idempotency gates, deterministic ordering, and sessions that
always close cleanly).

> There's also a styled HTML version of these docs — open [`index.html`](index.html)
> in a browser (`cargo doc`-style, but for the manifest language).

## Guide

The guide is ordered so each page builds on the last: start at the top and read
down, or jump straight to a topic. Every page opens with the concept in plain
language, then works toward the advanced cases at the bottom.

| Page | What it covers |
|------|----------------|
| [Getting started](getting-started.md) | Build, run the mock backend, drive a real browser, CLI flags, troubleshooting. |
| [Tutorial](tutorial.md) | Build a real multi-step test end to end: navigate → act → assert → chain → idempotency → `loop` → multi-file. |
| [Actions & steps](actions.md) | Reference for every step type: `goto`, `click`, `fill`, `upload`, `assert`, `wait`/`wait_for`, `screenshot`. |
| [Selectors](selectors.md) | Plain CSS vs. structured hierarchical selectors; text matching; the "delete button on Alice's row" pattern. |
| [Waits & timing](waits.md) | Implicit waits, `pageLoadStrategy: eager`, and `wait` vs `wait_for`. |
| [Screenshots](screenshots.md) | The `screenshot` step and automatic `--shot-on-fail` captures. |
| [Idempotency & checks](idempotency.md) | Safe reruns: the `idempotent` gate, the `check` predicate, `on_exists` modes, prerequisite chains. |
| [Variables & formatting](variables.md) | The template context and precedence, plus filters to zero-pad, prefix, case, and default `{{ VAR }}` values. |
| [Loops](loops.md) | `loop:` over a range or a list: forms, the loop variable, `needs` fan-out, naming rules, idempotent reruns. |
| [Playbooks](playbooks.md) | Collect suites into one CI run: settings, file-level `needs`, managed driver, `max_run_time`. |

## 30-second taste

A whole test, top to bottom: declare a variable, open a page, wait for it to
render, assert it loaded, and capture a screenshot — then build and run it
against a real Firefox.

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
