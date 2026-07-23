# Idempotency & checks

Rerunning a test suite should be safe. If a task *created* something the first
time, the second run shouldn't create it again — nor fall over because it's
already there. A task's **`idempotent`** gate makes that explicit: it checks a
predicate *before* the task runs, then decides whether to run the steps at all.

This is the "repeatability" half of htest's design, and it maps directly onto how
CI prerequisites work: *"does this user already exist? if not, create it."* Get
it right and a suite becomes a set of desired end-states you can apply as many
times as you like.

## The gate

The `idempotent` block hangs off a task and runs **before** the task's first
step. It has three fields:

- **`probe`** *(optional)* — steps that set up the page before the check.
- **`check`** *(required)* — the predicate that decides whether the resource is
  already in its target state.
- **`on_exists`** *(default `skip`)* — what to do when the predicate holds.

At its simplest, just a `check`:

```yaml
- name: create_user
  idempotent:
    check: { selector: ".row-alice", exists: true }
    on_exists: skip
  steps:
    - goto: "{{ BASE_URL }}/users/new"
    - fill: { selector: "#name", value: "alice" }
    - click: "#create"
    - assert: { selector: ".row-alice", exists: true }
```

Read it as: *"before creating alice, look for her row. If it's already there,
skip the creation steps."* The first run creates her; every rerun is a no-op.

## `check` — the predicate

`check` is a selector paired with an expected presence — it asks a single
yes/no question: *is the resource already in the state I want?*

| Field | Meaning |
|-------|---------|
| `selector` | Any selector — plain CSS or the structured form (see [Selectors](selectors.md)). |
| `exists` | Expected presence. `true` (default) = the element **should** be there for the predicate to hold; `false` = it should be **absent**. |

The gate compares what the selector resolves to against what you expected:

> **predicate holds** when `actual presence == exists`

When the predicate holds, the resource is considered "already in the target
state," and `on_exists` decides what happens. When it does **not** hold, the
steps always run — the work still needs doing.

Just like `assert`, `check` **polls**: it re-checks presence every tick until it
matches your expectation or the `--timeout` budget expires — so a probe's async
filter or render settles before the gate decides, the same "you mostly don't
wait" model as the rest of htest. And `check` never fails on its own — if the
budget expires and the page still isn't in the expected state, that simply means
"not in the target state, so do the work."

One cost is worth understanding. On the **create path** — `exists: true` with the
resource genuinely absent — there is nothing on the page to find, so `check`
polls the **full** budget before giving up and falling through to run the steps.
It waits out the clock *every* time. Keep `--timeout` sane for suites that create
many new resources, or gate the check behind a `probe` that ends the moment the
list has loaded (see next), so the poll resolves on its first tick instead.

## `probe` — search before you check

`check` only sees whatever the page currently shows. When answering *"does it
exist?"* first requires some interaction — open the list, type a name into a
filter, click search — those steps go in `probe`. They run **before** the
predicate, using the ordinary [step](actions.md) types (implicit waits included),
so `check` then inspects the *searched* result rather than a raw, un-filtered
snapshot.

```yaml
- name: create_user
  idempotent:
    probe:                                  # runs first, against the live page
      - goto: "{{ BASE_URL }}/#Users_Customers"
      - wait_for: "#Customers"
      - fill:  { selector: "#Customers .filter input[name='UserName']", value: "{{ USERNAME }}" }
      - click: "#Customers .filter button.filter-button"
      - wait_for: { selector: "#Customers .grid", until: present }
    check: { selector: { css: ".grid .row", contains: "{{ USERNAME }}" }, exists: true }
    on_exists: skip
  steps: [ ...create the user... ]
```

This is the "search among many, then check the relevant one" pattern — a 1:1 map
of a C# `CheckIfUserExists` that filters by name and then counts the matching
rows. Three things to keep in mind:

- **Probe steps run on every invocation** — the search *has* to happen to answer
  the question, including on reruns that end up skipping. Keep them
  **read-only**: navigate, filter, search. Never create or mutate anything in a
  probe, or you defeat the very idempotency it guards.
- **End the probe on a load-complete signal, not on the row.** `check` polls, so
  you rarely need an explicit wait — a filter that renders a little late still
  gets caught. But polling for the *row* means the create path (row genuinely
  absent) waits out the whole budget. To keep it snappy, end the probe with a
  `wait_for` on the list's **load-complete** signal — the moment the grid
  finishes rendering, present or empty (`bfs-data="done"`, a spinner clearing,
  and so on). Wait on *that*, never on the row itself: a row-present wait would
  hard-fail for a resource that legitimately doesn't exist yet.
- **A probe failure fails the task.** If the search itself broke, htest can't
  tell whether it's safe to skip, so it won't — it fails instead. The error names
  the offending step: `idempotency probe step 3/5 (click ...)`.

## `on_exists` — what to do when the predicate holds

Once the predicate holds, `on_exists` chooses the outcome:

| Value | Behaviour |
|-------|-----------|
| `skip` *(default)* | Skip the task's steps entirely. The task is reported as skipped, not failed. Safe reruns. |
| `continue` | Run the steps anyway. Use when the check is informational, or when the steps are themselves idempotent. |
| `fail` | Treat prior existence as an error and fail the task. Use to assert a *clean* starting state. |

```yaml
# Create-if-missing (the common case): skip when already present.
idempotent:
  check: { selector: ".row-alice", exists: true }
  on_exists: skip

# Guard a clean slate: fail if a leftover from a previous run is still there.
idempotent:
  check: { selector: ".row-alice", exists: true }
  on_exists: fail
```

## Patterns

With those three fields understood, the common shapes fall out naturally.

### Create if missing

The canonical use. Check for the thing you're about to create; skip if it's
already there.

```yaml
- name: create_user
  idempotent:
    check: { selector: ".row-{{ USERNAME }}", exists: true }
    on_exists: skip
  steps:
    - goto: "{{ BASE_URL }}/users/new"
    - fill: { selector: "#name", value: "{{ USERNAME }}" }
    - click: "#create"
    - assert: { selector: ".row-{{ USERNAME }}", exists: true }
```

### Delete if present

Flip `exists`. The predicate now "holds" when the row is **gone** — so if it's
still there, the delete steps run; if it's already gone, the task skips.

```yaml
- name: cleanup_user
  idempotent:
    check: { selector: ".row-{{ USERNAME }}", exists: false }   # want it absent
    on_exists: skip
  steps:
    - click: { css: ".row-{{ USERNAME }}", find: { css: "button", text: "Delete" } }
    - assert: { selector: ".row-{{ USERNAME }}", exists: false }
```

### Detect a row by its content

Generated ids and dynamic rows can't always be pinned down by a class. Use a
structured selector so the check keys off rendered **text** instead of markup:

```yaml
idempotent:
  check:
    selector: { css: ".grid .row", contains: "{{ USERNAME }}" }
    exists: true
  on_exists: skip
```

## Idempotency + `needs`: prerequisite chains

Idempotent gates compose. Tasks declare prerequisites with `needs`; htest builds
a run graph and executes in dependency order. Pair the two and you get exactly
the "prerequisite" model of a CI framework: each prerequisite checks whether its
resource exists and creates it only if missing, and later tasks depend on it.

```yaml
tasks:
  - name: create_default_user
    idempotent:
      check: { selector: { css: ".grid .row", contains: "{{ USERNAME }}" }, exists: true }
      on_exists: skip
    steps: [ ... ]

  - name: create_holding_account
    needs: [create_default_user]        # only after the user exists
    idempotent:
      check: { selector: { css: "#Accounts .row", contains: "{{ ACCOUNT }}" }, exists: true }
      on_exists: skip
    steps: [ ... ]
```

Run it a hundred times: the user and account are created once, and every
subsequent run skips straight through — the "prerequisite" model of a CI
framework, expressed as ordinary tasks.

## Limitations

The predicate itself is still **one selector, one presence test**. What `probe`
adds is the ability to *set up* the state to be checked (navigate, filter,
search) beforehand, which covers the common "filter then count rows" case, but it
doesn't turn the check into arbitrary logic. Two things follow from that:

- **Keep `probe` read-only.** It runs on every invocation, including reruns that
  end up skipping; a probe that created or mutated data would defeat the gate.
- **Mind the create-path cost.** `check` polls, but the create path (row absent)
  polls the full budget. When results load asynchronously, end the probe with a
  `wait_for` on the list's load-complete signal so the check resolves on its
  first tick instead of waiting out the clock.

## `check` vs `assert`

`check` and `assert` look almost identical — a selector and a presence — but they
do opposite jobs at opposite ends of a task:

| | `check` (gate) | `assert` (step) |
|--|----------------|-----------------|
| When | Before steps, once | Wherever placed, in order |
| Waits? | Yes — polls until the budget expires | Yes — polls until the budget expires |
| On predicate false | Runs the steps | Fails the task |
| Purpose | Decide *whether* to act | Verify the page *did* what you expected |

Both poll the same way; the difference is what they do with the answer. `check`
sits at the front of a task and decides *whether* to act — a false predicate
means "do the work." `assert` sits wherever you place it and *proves* the work
happened — a false predicate means the task failed. Use `check` to avoid
redundant work; use `assert` to prove the work was done.

---

← [Screenshots](screenshots.md) · [Variables & formatting →](variables.md)
