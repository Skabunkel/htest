# Screenshots

A screenshot is a PNG of the browser viewport written to disk. htest gives you
two ways to capture one: a step you place deliberately to document a state you
care about, and a flag that fires automatically whenever a task fails. Both write
into the same directory, and you'll usually want them together. This page builds
up from the simple, deliberate case to a full CI invocation.

## On purpose — the `screenshot` step

The `screenshot` step captures the viewport at exactly the point in a task where
you place it. Its value is a filename, and htest writes the PNG under the
screenshots directory:

```yaml
- goto: "{{ BASE_URL }}"
- wait_for: "h1"
- screenshot: "home.png"
```

Because it's an ordinary step, it fires in sequence with everything around it —
here, only after `wait_for: "h1"` confirms the page has rendered, so the image
captures the settled page rather than a half-drawn one. Use it to document key
states in a passing run: a filled-in form before submit, a dashboard after
login, a report once it loads.

## The output directory

Every screenshot — deliberate or automatic — lands in one directory, so it's
worth setting deliberately. It defaults to `screenshots/`; change it with
`--screenshots`:

```bash
htest run smoke.yaml --driver webdriver --screenshots artifacts/shots
```

Two conveniences make this painless in practice:

- The directory is **created if it doesn't exist**, so you never have to
  pre-make it in a fresh checkout or CI workspace.
- **Subdirectories in the filename work**, so you can organize shots by flow.
  `screenshot: "login/step2.png"` writes `login/step2.png` beneath the
  screenshots directory, creating the `login/` folder as needed.

## On failure — `--shot-on-fail`

The screenshot you most want is the one you didn't think to ask for: the page as
it looked at the moment something broke. Pass `--shot-on-fail` and htest captures
exactly that — automatically, the moment **any** task fails, with no
`screenshot` step required:

```bash
htest run suite.yaml --driver webdriver --shot-on-fail
```

Each failed task produces one file named `FAIL-<task-id>.png` in the screenshots
directory. The name is derived from the task's canonical id `namespace:name`,
with the `:` replaced by `-` so it's a valid filename — task `app:login` becomes
`FAIL-app-login.png`:

```
screenshots/
  FAIL-app-login.png          # from task  app:login
  FAIL-reporting-audit.png    # from task  reporting:audit
```

## Combine both

The two mechanisms are complementary, not competing, and they coexist cleanly.
Use deliberate `screenshot` steps to document the states you expect in a passing
run, and keep `--shot-on-fail` switched on as a safety net so a failure always
leaves visual evidence behind. They share one directory and never collide,
because the automatic captures all carry the `FAIL-` prefix while your
deliberate names don't.

## A CI-friendly invocation

Put it together for continuous integration, where you're running headless and
want a complete picture from a single run:

```bash
htest run suite.yaml \
  --driver webdriver --browser firefox --headless \
  --window 1280x800 \
  --screenshots artifacts \
  --shot-on-fail \
  --keep-going          # run every task; collect all failures + shots
```

The pieces working together: `--headless` with a fixed `--window 1280x800` gives
reproducible framing on a machine with no display; `--screenshots artifacts`
drops everything into a directory your CI can archive; and `--keep-going` is what
makes a single run worth archiving — instead of stopping at the first failure,
htest runs *every* task, so one run surfaces every failing task and, with
`--shot-on-fail`, a screenshot for each.

---

← [Waits & timing](waits.md) · [Idempotency & checks →](idempotency.md)
