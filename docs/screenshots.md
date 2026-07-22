# Screenshots

Capture the viewport on purpose with a step, or automatically whenever a task
fails.

## On purpose — the `screenshot` step

Add a `screenshot` step wherever you want a snapshot. The value is a filename;
it's written into the screenshots directory.

```yaml
- goto: "{{ BASE_URL }}"
- wait_for: "h1"
- screenshot: "home.png"
```

The output directory is `screenshots/` by default; change it with
`--screenshots`:

```bash
htest run smoke.yaml --driver webdriver --screenshots artifacts/shots
```

The directory is created if it doesn't exist. Subdirectories in the name work
too (e.g. `screenshot: "login/step2.png"`).

## On failure — `--shot-on-fail`

Pass `--shot-on-fail` and htest captures a screenshot the moment any task fails
— no `screenshot` step required. Perfect for CI, where you want to see the page
state at the point of failure.

```bash
htest run suite.yaml --driver webdriver --shot-on-fail
```

Each failed task produces one file named `FAIL-<task-id>.png` in the screenshots
directory. The task's canonical id (`namespace:name`) is used, with `:` replaced
by `-` so it's a valid filename:

```
screenshots/
  FAIL-app-login.png          # from task  app:login
  FAIL-reporting-audit.png    # from task  reporting:audit
```

> **Combine both.** Use explicit `screenshot` steps to document key states in a
> passing run, and `--shot-on-fail` as a safety net so a failure always leaves
> visual evidence. They write to the same directory and don't collide
> (`FAIL-` prefix).

## A CI-friendly invocation

```bash
htest run suite.yaml \
  --driver webdriver --browser firefox --headless \
  --window 1280x800 \
  --screenshots artifacts \
  --shot-on-fail \
  --keep-going          # run every task; collect all failures + shots
```

With `--keep-going`, htest doesn't stop at the first failure, so one run
surfaces every failing task (and, with `--shot-on-fail`, a screenshot for each).

---

← [Waits & timing](waits.md) · [Loops →](loops.md)
