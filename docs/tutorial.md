# Tutorial

Build a real test from scratch: land on a page, act on it, assert the result,
then chain tasks with prerequisites and make reruns safe.

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

## Anatomy of a manifest

A manifest is one YAML document. Top level holds an optional `id` (namespace),
`vars`, and a list of `tasks`. Each task has a `name`, optional `needs` /
`idempotent`, and an ordered list of `steps`.

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
```

## Step 1 — navigate and confirm

Every test starts by landing somewhere and confirming it rendered. `goto`
returns at DOMContentLoaded; the `assert` then polls until the element is really
there (see [Waits](waits.md)), so you don't need a fixed pause.

```yaml
- goto: "{{ BASE_URL }}/login"
- assert: { selector: "#login-form", exists: true }
```

## Step 2 — act

`fill` types into an input (it clears first); `click` clicks. Both implicitly
wait for their target to appear before acting.

```yaml
- fill:  { selector: "#user", value: "{{ USERNAME }}" }
- fill:  { selector: "#pass", value: "{{ PASSWORD }}" }
- click: "#submit"
```

## Step 3 — assert the outcome

`assert` checks presence, and optionally exact text. Use `exists: false` to
assert something is *gone* — it polls until the element disappears.

```yaml
- assert: { selector: "#dashboard", exists: true }
- assert: { selector: ".error", exists: false }
- assert: { selector: "#greeting", exists: true, text: "Welcome, alice" }
```

## Step 4 — chain tasks with `needs`

`needs` declares prerequisites. htest topologically sorts the graph, so order in
the file doesn't matter — dependencies do. If a prerequisite fails, its
dependents are **blocked** rather than run against a broken state.

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

Preview the resolved order without running anything:

```bash
htest graph mytest.yaml
```

## Step 5 — make it idempotent (safe reruns)

An `idempotent` block runs a `check` predicate *before* the steps. If the
resource is already in the expected state, `on_exists` decides what happens:
`skip` (default), `continue`, or `fail`. This is "create if not exists" — the
key to repeatable runs.

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

## Step 6 — repeat a task with `loop`

Creating five accounts shouldn't mean five copy-pasted tasks. `loop:` repeats
one task over a range or a list; htest expands it **when the manifest loads**
into one ordinary task per item, named `<task>[<item>]`.

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

Because expansion happens up front, `htest graph` shows exactly what will run:

```bash
htest graph mytest.yaml
#   layer 1 (parallel-safe): app:create_user[1], app:create_user[2], …
```

| form | meaning |
|------|---------|
| `loop: { var: n, from: 1, to: 5 }` | `1 2 3 4 5` — inclusive; use `from: 0, to: 4` for `user0`…`user4` |
| `loop: { var: n, from: 0, to: 10, step: 2 }` | `0 2 4 6 8 10`; a negative `step` (or `to < from`) counts down |
| `loop: { var: u, items: [alice, bob] }` | one task per item |
| `loop: [alice, bob]` | shorthand; the variable is called `item` |

Inside the task you also get `{{ loop_index }}` (0-based) and `{{ loop_index1 }}`
(1-based). Bounds can be templated (`to: "{{ USER_COUNT }}"`), so a single
variable resizes the run — from `vars`, or from `.env`/`--env` when `vars`
doesn't set it (manifest `vars` win over a dotenv).

Depending on a looped task works both ways:

```yaml
- name: report
  needs: [create_user]            # waits for every iteration
- name: audit
  needs: ["create_user[3]"]       # waits for just that one
```

The loop variable may appear anywhere in the task **except `name:`** — htest
appends `[item]` itself, and a name that changed per iteration would make
`needs:` ambiguous. See [`examples/loops.yaml`](../examples/loops.yaml) for a
runnable version against the demo app, and [Loops](loops.md) for the full
reference.

## Step 7 — split across files

Each manifest gets a **namespace** (its `id:`, else the filename stem). A task's
canonical id is `namespace:name`. A `needs` entry is bare (same file) or
qualified with `:` (another file).

```yaml
# reporting.yaml
tasks:
  - name: verify_audit_log
    needs: [manifest:edit_user]     # waits for edit_user in manifest.yaml
    steps: [ ... ]
```

```bash
htest run examples/manifest.yaml examples/reporting.yaml
```

Each file is templated with its own `vars`/`.env`; the process environment is
shared. Namespaces must be unique across the files you pass.

## Variables & environments

The whole document is rendered with `{{ VAR }}` before parsing. Precedence, low
→ high: process env → `.env` file → manifest `vars`. A missing variable is a
hard error (never a silent blank). Point at a specific environment with
`--env staging.env`.

## Step reference

| Step | Form |
|------|------|
| `goto` | `goto: "<url>"` |
| `click` | `click: <selector>` |
| `fill` | `fill: { selector: <selector>, value: "<v>" }` |
| `upload` | `upload: { selector: <file-input>, path: "<file>" }` — set a file `<input>` (relative path resolves against the CWD, then the manifest's directory) |
| `assert` | `assert: { selector: <selector>, exists: true, text: "<opt>" }` |
| `screenshot` | `screenshot: "<name.png>"` |
| `wait` | `wait: <ms>` — fixed pause (last resort) |
| `wait_for` | `wait_for: <selector>` or `{ selector, until: present\|absent, timeout }` |

See [Selectors](selectors.md) for what a `<selector>` can be, and
[Waits](waits.md) for `wait` vs `wait_for`.

---

← [Getting started](getting-started.md) · [Actions & steps →](actions.md)
