# Waits & timing

Web pages don't finish loading at a single, tidy moment. Markup arrives, scripts
run, requests come back, and the DOM keeps rearranging itself for a while after
the address bar stops spinning. A test that assumes everything is ready the
instant a page "loads" is really racing that settling process — and it will lose
that race intermittently, which is the worst kind of test failure.

htest's timing model is built around one idea: **wait for conditions, not
clocks.** Rather than guessing *how long* something takes, you describe the
*state* you're waiting for, and htest polls until that state is true. Done well,
this keeps a suite both fast — it proceeds the instant the condition holds — and
race-free — it never proceeds before the condition holds.

## The problem with fixed sleeps

A fixed pause is a bet against the network, and you lose either way. A
`wait: 500` says "I think this takes about half a second." If the real work
finishes in 50 ms, every run donates the other 450 ms to nothing; multiplied
across a suite that tax adds up fast. If the work occasionally takes 600 ms — a
cold cache, a slow CI runner, a busy backend — the test flakes, and a flaky test
quickly becomes a test nobody trusts.

The fix isn't a *longer* guess; there's no number that is both fast and safe.
The fix is to stop guessing. Everything below is about replacing "wait N
milliseconds" with "wait until X is true."

## Implicit waits (automatic)

Most waiting happens without you asking for it. Whenever htest interacts with the
page or evaluates an expectation, it re-checks the underlying condition on a
loop — polling every **150 ms** until the condition holds or the `--timeout`
budget (default **5 s**) runs out. In practice this means you can write steps in
the order a user would perform them and let the runner absorb the timing:

- **Before `click`/`fill`** — htest waits for the target element to appear before
  acting on it, so you don't have to wait for a button that renders a beat late.
- **On `assert`** — it polls until presence matches your expectation.
  `exists: true` waits for the element to *appear*; `exists: false` waits for it
  to *disappear* (so "the row is gone after I deleted it" is race-free rather
  than a snapshot taken a millisecond too early).
- **On `assert … text:`** — it polls until the element's text matches too, not
  just until the element exists — handy for values that populate asynchronously.
- **On an idempotency `check`** — it polls the same way before deciding whether
  to skip, so a probe's async filter or render settles before the gate makes its
  call (see [Idempotency](idempotency.md)).

Because the assertion itself polls, the common "act, then verify the result
shows up" flow needs no explicit wait at all:

```yaml
# No wait needed — the assert polls until #dashboard shows up.
- click: "#submit"
- assert: { selector: "#dashboard", exists: true }
```

## Eager page loads

Navigation shouldn't hold your test hostage to the slowest thing on the page. The
WebDriver backend uses `pageLoadStrategy: eager`, so `goto` returns at
**DOMContentLoaded** — once the HTML is parsed and the DOM is ready — instead of
blocking until every image, ad, and third-party tracker has finished. Page load
is therefore rarely the bottleneck, and anything still settling after
`DOMContentLoaded` is covered by the implicit waits above.

## `wait` — a fixed pause

`wait` is the guess you were warned about, kept only for the rare case where no
condition exists to key off. It's an unconditional sleep, measured in
milliseconds, and it *always* waits the full duration regardless of what the page
is doing:

```yaml
- wait: 250        # sleep 250ms, unconditionally
```

Reach for it only when there is genuinely no DOM signal to observe — a timed CSS
animation with no end event, say, or a debounce you can't otherwise detect.
Whenever the thing you're actually waiting for *does* leave a mark in the DOM,
prefer `wait_for`.

## `wait_for` — wait for a condition

`wait_for` is the tool you should reach for first: it blocks until a selector
reaches a given state, then continues **immediately** — no longer than
necessary, no shorter than correct. Crucially, on timeout the task **fails**, and
that's by design: `wait_for` doesn't just wait, it *asserts* that the condition
eventually happens.

```yaml
- wait_for: "#ready"                                    # until present (shorthand)
- wait_for: { selector: ".spinner", until: absent }     # until it disappears
- wait_for: { selector: ".report", until: present, timeout: 15000 }  # per-step budget
```

The shorthand form (`wait_for: "#ready"`) waits until the selector is present.
The map form takes three fields:

| Field | Default | Meaning |
|-------|---------|---------|
| `selector` | — | What to watch (plain CSS string or the structured form). |
| `until` | `present` | `present` = wait until the selector resolves to an element; `absent` = wait until it resolves to nothing. |
| `timeout` | `--timeout` | Per-step override of the wait budget, in milliseconds. Use it when one step legitimately needs longer than the suite default. |

### The spinner pattern

The most common use of `until: absent` is waiting out a loading indicator before
you inspect the results behind it. Wait for the spinner to *leave*, then assert
on what it was covering:

```yaml
- click: "#load-report"
- wait_for: { selector: ".spinner", until: absent, timeout: 15000 }
- assert: { selector: ".report", exists: true }
```

This reads exactly like the human workflow — "click load, wait for the spinner to
go away, then check the report is there" — and the per-step `timeout` gives the
slow report room to arrive without loosening the budget for the whole suite.

## Which one do I use?

When in doubt, work down this table from the top; the first row that matches your
situation is almost always the right answer:

| Situation | Use |
|-----------|-----|
| Assert something is / becomes true | Just `assert` — it already polls. |
| Click/fill an element that appears late | Nothing — the implicit wait covers it. |
| Wait for a spinner/overlay to clear | `wait_for: { until: absent }` |
| Wait for async content before acting on it | `wait_for: { until: present }` |
| A slow endpoint blows the 5 s budget | Raise `--timeout`, or set a per-step `timeout`. |
| A fixed, signal-less delay (rare) | `wait: <ms>` |

> **Rule of thumb:** prefer `wait_for` over `wait`. A condition-based wait is as
> fast as the page allows *and* fails loudly when the condition never happens; a
> fixed sleep is either too slow or too flaky, and it never tells you which.

---

← [Selectors](selectors.md) · [Screenshots →](screenshots.md)
