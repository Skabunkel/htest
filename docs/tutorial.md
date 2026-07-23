# Tutorial

This tutorial builds a real test the way you would actually write one. You start
small — land on a page and confirm it rendered — then add exactly one new idea at
a time: acting on the page, asserting the outcome, chaining tasks with
prerequisites, making reruns safe, narrowing what a check looks at, repeating a
task with a loop, and finally splitting a suite across files. Every step uses only
the concepts introduced before it, so by the end you can read and write a complete
manifest without guessing.

> **Practice target included.** The repo ships a self-contained demo app in
> [`demo/`](../demo/README.md) (popups, spinners, a form with a file picker,
> checkbox "snake", scrolling) and a manifest that tours it,
> [`examples/tour.yaml`](../examples/tour.yaml). It runs over `file://` with no
> server or network — the ideal thing to point your first real-browser run at:
>
> ```bash
> geckodriver --port 4444 &
> htest run examples/tour.yaml --driver webdriver --browser firefox --keep-going --shot-on-fail
> ```
>
> Read the tour alongside this page: every concept below appears there in a
> runnable form.

## Anatomy of a manifest

Before writing a single step, it helps to picture the shape of the file. A
manifest is **one YAML document**. At the top level it holds an optional `id`
(the manifest's namespace), an optional `vars` block (values you can template
into steps), and a required list of `tasks`. Each task has a `name`, an optional
`needs` list and an optional `idempotent` block (both covered later), and an
ordered list of `steps` — the individual browser actions that run top to bottom.

```yaml
id: app                       # optional namespace (default: the file stem)
vars:                         # values you can template with {{ ... }}
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
```

Everything that follows is just this structure, filled in one idea at a time.

## Step 1 — navigate and confirm

Every test begins the same way: put the browser on a page, then confirm the page
actually arrived. Those are two distinct steps because loading a URL and
rendering the content you care about are two distinct events.

`goto` navigates to a URL. It returns as soon as the document reaches its initial
loaded state (DOMContentLoaded), which is *not* a guarantee that the element you
want to interact with is present yet — scripts may still be building the page.
That is why you follow it with an `assert`: an assertion does not check once and
give up, it **polls** the page until the condition holds or a timeout is reached
(see [Waits](waits.md)). So you never need a fixed pause to "let the page settle".

```yaml
- goto: "{{ BASE_URL }}/login"
- assert: { selector: "#login-form", exists: true }
```

## Step 2 — act on the page

Once you have confirmed you are in the right place, you interact with it. The
three actions you reach for most are `fill`, `click`, and `upload`, and all of
them share one convenience: they **implicitly wait** for their target to appear
before acting, so you rarely have to synchronize by hand.

- `fill` sets the value of an input. It clears the field first, then types the
  value as real key events (so any input handlers on the page fire just as they
  would for a human). The `value` is templated, so `{{ ... }}` expressions are
  substituted before typing.
- `click` clicks an element, scrolling it into view first if it is below the
  fold.
- `upload` sets a file `<input>` to a file on disk. The `path` is resolved
  against the current working directory first, then against the manifest's own
  directory, and the file must exist.

```yaml
- fill:   { selector: "#user", value: "{{ USERNAME }}" }
- fill:   { selector: "#pass", value: "{{ PASSWORD }}" }
- upload: { selector: "#avatar", path: "avatar.png" }
- click:  "#submit"
```

## Step 3 — assert the outcome

Acting is only half a test; the point is to verify what the action produced.
`assert` checks that an element is present and, optionally, that its trimmed text
is exactly equal to a value you give. Because it polls (just like the confirming
assert in Step 1), it also handles the case where the thing you are waiting on has
not appeared *yet*.

The `exists` flag flips the meaning. `exists: true` (the default) waits for the
element to be present; `exists: false` waits for it to be **gone**, which is how
you assert that a spinner, a dialog, or an error has disappeared.

```yaml
- assert: { selector: "#dashboard", exists: true }
- assert: { selector: ".error",     exists: false }
- assert: { selector: "#greeting",  exists: true, text: "Welcome, alice" }
```

> When you need to *wait for* a condition without asserting a pass/fail on it —
> for example, waiting for a spinner to clear before you read the result —
> reach for `wait_for` (and, only as a last resort, a fixed `wait: <ms>`). Both
> are summarized in the reference table below and covered fully in
> [Waits](waits.md).

## Step 4 — chain tasks with `needs`

Real suites have order: you cannot create a user before you have logged in.
`needs` declares that ordering as prerequisites. You do **not** arrange tasks by
hand — htest reads every `needs` edge and topologically sorts the whole graph, so
the order tasks appear in the file is irrelevant; only the dependencies matter.

The payoff shows up on failure. If a prerequisite fails, its dependents are
**blocked** rather than run — htest will not exercise `create_user` against a
login that never succeeded, so you are never chasing cascading failures that all
trace back to one root cause.

```yaml
tasks:
  - name: login
    steps: [ ... ]

  - name: create_user
    needs: [login]              # runs only after login passes
    steps:
      - goto:  "{{ BASE_URL }}/users/new"
      - fill:  { selector: "#name", value: "{{ USERNAME }}" }
      - click: "#create"
      - assert: { selector: ".row-{{ USERNAME }}", exists: true }
```

You can preview the resolved order — the exact layers htest computed — without
launching a browser at all:

```bash
htest graph mytest.yaml
```

## Step 5 — make it idempotent (safe reruns)

The `create_user` task above works the first time, but the second time it may
fail: the user already exists. A test you cannot rerun is a test you will learn
to distrust. An `idempotent` block fixes this by adding a **gate** that runs
*before* the task's steps.

The gate's `check` is a predicate — the same "does this element exist?" question
an `assert` asks, and it **polls** the same way. If the predicate already holds
(the resource is already in the state you were going to create), `on_exists`
decides what happens next:

- `skip` (the default) — report the task as *Skipped* and do not run the steps.
- `continue` — run the steps anyway.
- `fail` — treat the already-existing state as an error.

This is the "create if not exists" pattern. Flip `check` to `exists: false` and
you get its mirror, "delete if present".

```yaml
- name: create_user
  needs: [login]
  idempotent:
    check: { selector: ".row-{{ USERNAME }}", exists: true }
    on_exists: skip             # user already there? skip the steps
  steps:
    - goto:  "{{ BASE_URL }}/users/new"
    - fill:  { selector: "#name", value: "{{ USERNAME }}" }
    - click: "#create"
    - assert: { selector: ".row-{{ USERNAME }}", exists: true }
```

## Step 6 — narrow the gate with `probe`

A bare `check` inspects the page as it currently is. But sometimes the thing you
want to check is not visible until you *make* it visible — a user hidden in a long
list you first have to search, for instance. That is what an optional `probe`
gives you: a short sequence of **read-only** steps that run first, purely to bring
the right state onto the page, so that `check` inspects a searched or filtered
result rather than the whole page.

The probe runs before the `check`, which still runs before the task's steps.
Because a probe uses ordinary steps, it introduces no new syntax — you already
know `fill` and `click`.

```yaml
- name: create_user
  needs: [login]
  idempotent:
    probe:                                  # read-only: surface the right row
      - fill:  { selector: "#user-search", value: "{{ USERNAME }}" }
      - click: "#search"
    check: { selector: ".results .row-{{ USERNAME }}", exists: true }
    on_exists: skip
  steps:
    - goto:  "{{ BASE_URL }}/users/new"
    - fill:  { selector: "#name", value: "{{ USERNAME }}" }
    - click: "#create"
    - assert: { selector: ".row-{{ USERNAME }}", exists: true }
```

## Step 7 — repeat a task with `loop`

Creating five accounts should not mean five copy-pasted tasks. `loop:` repeats one
task over a range or a list. The expansion happens **when the manifest loads**:
htest replaces the single looped task with one ordinary task per item, named
`<task>[<item>]`. Everything downstream — the dependency graph, ordering, and
reporting — sees only plain tasks.

```yaml
- name: create_user
  loop: { var: n, from: 1, to: 5 }      # user1 … user5 (both ends included)
  needs: [login]
  steps:
    - goto:  "{{ BASE_URL }}/users/new"
    - fill:  { selector: "#name", value: "user{{ n }}" }
    - click: "#create"
    - assert: { selector: ".row-user{{ n }}", exists: true }
```

Because expansion happens up front, `htest graph` shows exactly what will run —
the same preview command from Step 4, now listing every iteration:

```bash
htest graph mytest.yaml
#   layer 1 (parallel-safe): app:create_user[1], app:create_user[2], …
```

The loop can take several forms:

| form | meaning |
|------|---------|
| `loop: { var: n, from: 1, to: 5 }` | `1 2 3 4 5` — both ends inclusive; use `from: 0, to: 4` for `user0`…`user4` |
| `loop: { var: n, from: 0, to: 10, step: 2 }` | `0 2 4 6 8 10`; a negative `step` (or `to < from`) counts down |
| `loop: { var: u, items: [alice, bob] }` | one task per item |
| `loop: [alice, bob]` | shorthand; the variable is called `item` |

Inside a looped task you get the loop variable plus `{{ loop_index }}` (0-based)
and `{{ loop_index1 }}` (1-based). Bounds may themselves be templated
(`to: "{{ USER_COUNT }}"`), so a single variable resizes the whole run. And
because the gate from Steps 5–6 is evaluated **per iteration**, a looped task can
still be idempotent — each `create_user[n]` skips only if *that* user already
exists.

Depending on a looped task works at either granularity:

```yaml
- name: report
  needs: [create_user]            # waits for every iteration
- name: audit
  needs: ["create_user[3]"]       # waits for just that one
```

The loop variable may appear anywhere in the task **except `name:`** — htest
appends `[item]` itself, and a name that changed per iteration would make `needs:`
ambiguous. The loop variable *is* allowed in `needs`, so
`needs: ["create_user[{{ n }}]"]` pairs each iteration with its counterpart. See
[`examples/loops.yaml`](../examples/loops.yaml) for a version that runs against
the demo app, and [Loops](loops.md) for the full reference.

## Step 8 — split across files

As a suite grows you will want to break it into files by area — say, one for user
management and one for reporting. Each manifest gets a **namespace**: its `id:`,
or the filename stem if you omit `id:`. A task's canonical id is
`namespace:name`. Within a file a `needs` entry stays bare; to depend on a task in
*another* file, qualify it with the namespace and a colon.

```yaml
# reporting.yaml
tasks:
  - name: verify_audit_log
    needs: [manifest:edit_user]     # waits for edit_user in manifest.yaml
    steps: [ ... ]
```

Run several manifests together by passing them all on one command line; htest
merges their graphs into a single ordering:

```bash
htest run examples/manifest.yaml examples/reporting.yaml
```

Each file is templated with its own `vars` (and its own `.env`); only the process
environment is shared across files. Namespaces must be unique across the files you
pass, so cross-file `needs` is never ambiguous.

## Variables & environments

Throughout this tutorial you have written `{{ VAR }}` and trusted it to be filled
in. Here is the rule behind it: the **whole document** is rendered as a template
before it is parsed as YAML. Values come from three sources, listed low to high
precedence: the **process environment**, then a `.env` file, then the manifest's
own `vars` block — so a `vars` entry always wins over a like-named environment
variable. A referenced variable that is defined nowhere is a **hard error**, never
a silent blank, so a typo fails loudly instead of testing the wrong thing. Point
at a specific environment file with `--env staging.env`. See
[Variables](variables.md) for filters and formatting.

## Step reference

A dense summary of every step you have met, plus the two waiting steps mentioned
along the way. This is reference material — reach for it once the concepts above
have clicked.

| Step | Form |
|------|------|
| `goto` | `goto: "<url>"` — navigate; returns at initial load |
| `click` | `click: <selector>` — clicks, scrolling into view first |
| `fill` | `fill: { selector: <selector>, value: "<v>" }` — clears, then types real key events; `value` is templated |
| `upload` | `upload: { selector: <file-input>, path: "<file>" }` — set a file `<input>` (relative path resolves against the CWD, then the manifest's directory; the file must exist) |
| `assert` | `assert: { selector: <selector>, exists: true, text: "<opt>" }` — polls until presence matches; `text` is exact, trimmed equality |
| `screenshot` | `screenshot: "<name.png>"` |
| `wait_for` | `wait_for: <selector>` (until present) or `{ selector, until: present\|absent, timeout }` — a timeout is a hard failure |
| `wait` | `wait: <ms>` — fixed pause (last resort) |

See [Selectors](selectors.md) for everything a `<selector>` can be, and
[Waits](waits.md) for `wait` vs `wait_for`.

---

← [Getting started](getting-started.md) · [Actions & steps →](actions.md)
