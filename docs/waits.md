# Waits & timing

Web pages settle asynchronously. htest's timing model keeps tests fast *and*
race-free without you sprinkling guesses.

## The problem with fixed sleeps

A fixed `wait: 500` is a guess. Too short and the test is flaky; too long and
every run pays the cost. Real integration tests need to wait for *conditions*,
not clocks.

## Implicit waits (automatic)

You mostly don't have to wait at all. htest re-checks conditions for you, polling
every 150 ms until they hold or the `--timeout` budget (default 5 s) runs out:

- **Before `click`/`fill`** — it waits for the target element to appear.
- **On `assert`** — it polls until presence matches your expectation.
  `exists: true` waits for the element to *appear*; `exists: false` waits for it
  to *disappear* (so "row gone after delete" is race-free).
- **On `assert … text:`** — it polls until the text matches too.

```yaml
# No wait needed — the assert polls until #dashboard shows up.
- click: "#submit"
- assert: { selector: "#dashboard", exists: true }
```

## Fast page loads

The WebDriver backend uses `pageLoadStrategy: eager`, so `goto` returns at
DOMContentLoaded instead of blocking until every image, ad, and tracker
finishes. Page load is rarely the bottleneck; anything still settling is covered
by the implicit wait above.

## `wait` — a fixed pause

An unconditional sleep in milliseconds. It always waits the full duration. Use
it only when you genuinely need a fixed delay (e.g. a timed animation with no
DOM signal to key off).

```yaml
- wait: 250        # sleep 250ms, unconditionally
```

## `wait_for` — wait for a condition

Block until a selector reaches a state, then continue immediately. This is the
one to reach for. On timeout the task **fails** — that's the point: it asserts
the condition eventually happens.

```yaml
- wait_for: "#ready"                                    # until present (shorthand)
- wait_for: { selector: ".spinner", until: absent }     # until it disappears
- wait_for: { selector: ".report", until: present, timeout: 15000 }  # per-step budget
```

| Field | Default | Meaning |
|-------|---------|---------|
| `selector` | — | What to watch (plain CSS string or structured form). |
| `until` | `present` | `present` = wait until it resolves; `absent` = wait until it resolves to nothing. |
| `timeout` | `--timeout` | Per-step override of the wait budget, in milliseconds. |

### The spinner pattern

`until: absent` is how you wait out a loading indicator before checking results:

```yaml
- click: "#load-report"
- wait_for: { selector: ".spinner", until: absent, timeout: 15000 }
- assert: { selector: ".report", exists: true }
```

## Which one do I use?

| Situation | Use |
|-----------|-----|
| Assert something is / becomes true | Just `assert` — it already polls. |
| Click/fill an element that appears late | Nothing — the implicit wait covers it. |
| Wait for a spinner/overlay to clear | `wait_for: { until: absent }` |
| Wait for async content before acting on it | `wait_for: { until: present }` |
| A slow endpoint blows the 5 s budget | Raise `--timeout`, or set a per-step `timeout`. |
| A fixed, signal-less delay (rare) | `wait: <ms>` |

> **Rule of thumb:** prefer `wait_for` over `wait`. A condition-based wait is as
> fast as the page allows and fails loudly when the condition never happens; a
> fixed sleep is either too slow or too flaky.

---

← [Selectors](selectors.md) · [Screenshots →](screenshots.md)
