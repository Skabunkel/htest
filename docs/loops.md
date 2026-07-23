# Loops

Five user accounts shouldn't mean five copy-pasted tasks. `loop:` repeats one
task over a range or a list — and, crucially, it does so *before the run starts*,
expanding into real, individually reported tasks rather than looping at run time.

## The idea

A `loop:` on a task is **expanded when the manifest loads**, not when it runs.
htest renders the task once per item — with the loop variable in scope for that
render — and emits one ordinary task per iteration, named `<task>[<item>]`.
From that point on nothing downstream knows a loop was ever involved: the graph,
the run order, the report and the exit code all see plain, independent tasks.

That is the whole design in one sentence, and it is what makes loops
predictable: *there is no loop at run time.* Everything you already know about
tasks — how `needs` wires them, how the report lists them, how a failure blocks
dependents — applies unchanged to each expanded iteration.

```yaml
# manifest
- name: create_user
  loop: { var: n, from: 1, to: 5 }
  steps:
    - fill: { selector: "#name", value: "user{{ n }}" }
```

```
# what actually runs
create_user[1]   create_user[2]   create_user[3]   create_user[4]   create_user[5]
```

## Quick start

A task that creates `user1` … `user5`, then a second task that waits for all of
them to finish before taking a screenshot:

```yaml
- name: create_user
  loop: { var: n, from: 1, to: 5 }      # both ends included
  needs: [open_app]
  steps:
    - goto:  "{{ BASE_URL }}/form.html"
    - fill:  { selector: "#name",  value: "user{{ n }}" }
    - fill:  { selector: "#email", value: "user{{ n }}@example.com" }
    - click: "#submit"
    - wait_for: "#summary"
    - assert: { selector: "#sum-name", text: "user{{ n }}" }

- name: report
  needs: [create_user]                  # every iteration must pass first
  steps:
    - screenshot: "users-report.png"
```

Because expansion happens at load time, you can inspect exactly what will run
without touching a browser — `graph` prints the fully expanded task list:

```
$ htest graph examples/loops.yaml
run graph (3 layers):
  layer 0 (parallel-safe): users:open_app
  layer 1 (parallel-safe): users:check_page[form], users:check_page[index], users:check_page[popup], users:create_user[1], users:create_user[2], users:create_user[3], users:create_user[4], users:create_user[5]
  layer 2 (parallel-safe): users:report
```

## Loop forms

The item list can come from a numeric range or an explicit list. These are the
accepted shapes:

| Form | Items |
|------|-------|
| `loop: { var: n, from: 1, to: 5 }` | `1 2 3 4 5` — **both ends included** |
| `loop: { var: n, from: 0, to: 4 }` | `0 1 2 3 4` — for `user0`…`user4` |
| `loop: { var: n, from: 0, to: 10, step: 2 }` | `0 2 4 6 8 10` |
| `loop: { var: n, from: 3, to: 1 }` | `3 2 1` — counts down when `to < from` |
| `loop: { var: u, items: [alice, bob] }` | `alice bob` |
| `loop: [alice, bob]` | shorthand — the variable is called `item` |

Ranges are inclusive at both ends on purpose: `from: 1, to: 5` reads as "user1
through user5", which is what people mean when they say it out loud. For a
zero-based set use `from: 0, to: 4`. A range counts down on its own when
`to < from` (or when `step` is negative), so you never have to special-case the
direction.

## What's in scope inside the task

Each render gets three extra names on top of the usual context — the current
item and its position:

| Variable | Value |
|----------|-------|
| `{{ n }}` (whatever `var:` names; `item` by default) | the current item |
| `{{ loop_index }}` | iteration number, 0-based |
| `{{ loop_index1 }}` | iteration number, 1-based |

These sit on top of the normal template context (process env → `.env` → manifest
`vars`), so they are the highest-precedence names for that one task. And because
the whole document is a template, they work *anywhere* in the task: step values,
selectors, `needs`, the `idempotent` check, screenshot names.

```yaml
- name: check_row
  loop: { var: u, items: [alice, bob] }
  steps:
    - assert:
        selector: { css: ".row", contains: "{{ u }}" }
        exists: true
    - screenshot: "row-{{ loop_index1 }}-{{ u }}.png"
```

### Sizing a run from one variable

Because the bounds are templated like everything else, a single variable can
resize the whole suite without editing the loop:

```yaml
vars:
  LAST_USER: "5"
tasks:
  - name: create_user
    loop: { var: n, from: 1, to: "{{ LAST_USER }}" }
    steps: [ ... ]
```

> Manifest `vars` have the **highest** precedence — they win over `.env` and
> `--env`. So if you want to drive the size from a dotenv or the environment
> (say, a smaller set locally and the full set in CI), leave the variable out of
> `vars:` entirely; otherwise the `vars:` value will always shadow it.

## Depending on a looped task

Because every iteration is a real task with a real name, `needs` can point at the
whole group or at a single member — and that choice shapes how much runs in
parallel:

```yaml
- name: report
  needs: [create_user]            # waits for EVERY iteration
- name: audit
  needs: ["create_user[3]"]       # waits for just that one
```

A looped task can also depend on *its own* matching iteration in another loop,
because the loop variable is in scope in `needs` too:

```yaml
- name: verify_user
  loop: { var: n, from: 1, to: 5 }
  needs: ["create_user[{{ n }}]"]   # verify_user[3] waits only for create_user[3]
  steps: [ ... ]
```

That is the difference between a **fan-in** — `needs: [create_user]`, where one
task waits behind the whole group and becomes a single bottleneck — and a
**per-item chain** — `create_user[{{ n }}]`, which builds five independent
pipelines that run side by side. It is worth choosing deliberately, because the
two produce visibly different parallel layers in `htest graph`.

## Naming rules

htest appends `[item]` to each iteration itself, so the loop variable **must not
appear in `name:`**. The reason is `needs`: a name that changed on every
iteration would leave no stable base name for other tasks to depend on, so htest
rejects it up front rather than producing an un-referenceable task:

```yaml
- name: "user{{ n }}"                 # ✗ rejected
  loop: { var: n, from: 1, to: 2 }
```

```
error: expanding users.yaml: manifest error: looped task `user1` renders a
different name (`user2`) on another iteration: `n` must not appear in `name:`
— htest appends `[item]` to each iteration itself
```

Global variables in a name are fine (`name: "create_{{ ENVIRONMENT }}_user"`) —
they render the same on every iteration, so the base name stays stable.

## Loops and idempotency

Because expansion produces independent tasks, an `idempotent` block is evaluated
**per iteration** — so "create if not exists" scales to a whole set for free. A
rerun checks each user individually, skips the ones that already exist and
creates only the missing ones:

```yaml
- name: create_user
  loop: { var: n, from: 1, to: 5 }
  idempotent:
    check: { selector: ".row-user{{ n }}", exists: true }
    on_exists: skip
  steps: [ ... ]
```

You can watch this happen against the mock backend, which seeds "already exists"
from an env var — here users 1 and 2 already exist and are skipped, while user 3
is missing and gets created:

```
$ HTEST_MOCK_ABSENT=.row-user3 htest run users.yaml
results:
  [SKIP] users:create_user[1] (0 ms) — idempotency gate: already satisfied
  [SKIP] users:create_user[2] (0 ms) — idempotency gate: already satisfied
  [PASS] users:create_user[3] (0 ms)

1 passed, 2 skipped, 0 blocked, 0 failed
```

## Gotchas

- **An empty range is legal.** `from: 5, to: 1, step: 1` yields no items, so the
  task simply doesn't exist — and a `needs:` pointing at it resolves to nothing
  rather than failing. This makes a zero-sized loop a safe default.
- **10,000 iterations is the cap.** A typo like `to: 1000000` is rejected up
  front with a clear message, instead of building a graph that would never
  finish.
- **Items are strings.** Numbers keep their plain form (`1`, not `1.0`); only
  strings, numbers and bools are allowed as items — anything else is rejected.
- **File order stays irrelevant.** Iterations are independent of one another
  unless you explicitly wire them with `needs`; their order in the file changes
  nothing.
- **Document-level Jinja blocks don't mix with `loop:`.** A manifest that uses
  `loop:` is parsed as YAML *before* templating, so it must be valid YAML on its
  own. Multi-line `{% for %}` blocks that span the document belong in a manifest
  *without* `loop:`.

## Errors you might see

| Message | Cause |
|---------|-------|
| ``` `loop:` needs either `items:` or both `from:` and `to:` ``` | A range is missing an end. |
| ``` unknown `loop:` key `count` ``` | Only `var`, `items`, `from`, `to`, `step` are accepted. |
| ``` `loop: to:` is not a number: `x` ``` | A bound (or the variable it came from) isn't an integer. |
| ``` `loop: step:` must not be 0 ``` | A zero step would never terminate. |
| ``` … must not appear in `name:` ``` | The loop variable is used in the task name. |
| ``` two looped tasks are both named `x` ``` | Two loops share a base name; rename one. |

## Full example

[`examples/loops.yaml`](../examples/loops.yaml) runs against the bundled demo
app — no server, no network:

```bash
geckodriver --port 4444 &
htest graph examples/loops.yaml     # inspect the expansion
htest run   examples/loops.yaml --driver webdriver --browser firefox --headless
```

```
results:
  [PASS] users:open_app (90 ms)
  [PASS] users:check_page[form] (26 ms)
  [PASS] users:check_page[index] (14 ms)
  [PASS] users:check_page[popup] (16 ms)
  [PASS] users:create_user[1] (235 ms)
  [PASS] users:create_user[2] (413 ms)
  [PASS] users:create_user[3] (150 ms)
  [PASS] users:create_user[4] (151 ms)
  [PASS] users:create_user[5] (149 ms)
  [PASS] users:report (39 ms)

10 passed, 0 skipped, 0 blocked, 0 failed
```

> **Rule of thumb:** loop over the *data*, not the workflow. One task per user,
> per plan, per locale — then let `needs` express what has to happen before and
> after. Because expansion happens at load time, `htest graph` always tells you
> exactly what you're about to run.

---

← [Variables & formatting](variables.md) · [Playbooks →](playbooks.md)
