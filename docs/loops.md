# Loops

Five user accounts shouldn't mean five copy-pasted tasks. `loop:` repeats one
task over a range or a list — and expands it into real, individually reported
tasks before the run starts.

## The idea

A `loop:` on a task is **expanded when the manifest loads**: htest renders the
task once per item, with the loop variable in scope, and emits one ordinary task
per iteration named `<task>[<item>]`. Nothing downstream knows loops exist — the
graph, the run order, the report and the exit code all see plain tasks.

That is the whole design in one sentence, and it is what makes loops
predictable: *there is no loop at run time.*

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

A task that creates `user1` … `user5`, then a task that waits for all of them:

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

Check the expansion before running anything — `graph` needs no browser:

```
$ htest graph examples/loops.yaml
run graph (3 layers):
  layer 0 (parallel-safe): users:open_app
  layer 1 (parallel-safe): users:check_page[form], users:check_page[index], users:check_page[popup], users:create_user[1], users:create_user[2], users:create_user[3], users:create_user[4], users:create_user[5]
  layer 2 (parallel-safe): users:report
```

## Loop forms

| Form | Items |
|------|-------|
| `loop: { var: n, from: 1, to: 5 }` | `1 2 3 4 5` — **both ends included** |
| `loop: { var: n, from: 0, to: 4 }` | `0 1 2 3 4` — for `user0`…`user4` |
| `loop: { var: n, from: 0, to: 10, step: 2 }` | `0 2 4 6 8 10` |
| `loop: { var: n, from: 3, to: 1 }` | `3 2 1` — counts down when `to < from` |
| `loop: { var: u, items: [alice, bob] }` | `alice bob` |
| `loop: [alice, bob]` | shorthand — the variable is called `item` |

Ranges are inclusive on purpose: `from: 1, to: 5` is "user1 through user5",
which is what everyone means when they say it. Use `from: 0, to: 4` for a
zero-based set.

## What's in scope inside the task

| Variable | Value |
|----------|-------|
| `{{ n }}` (whatever `var:` names; `item` by default) | the current item |
| `{{ loop_index }}` | iteration number, 0-based |
| `{{ loop_index1 }}` | iteration number, 1-based |

These sit on top of the normal template context (process env → `.env` → manifest
`vars`), and work *anywhere* in the task: step values, selectors, `needs`, the
`idempotent` check, screenshot names.

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

The bounds are templated like everything else, so a single variable resizes the
whole suite:

```yaml
vars:
  LAST_USER: "5"
tasks:
  - name: create_user
    loop: { var: n, from: 1, to: "{{ LAST_USER }}" }
    steps: [ ... ]
```

> Manifest `vars` have the **highest** precedence — they win over `.env` and
> `--env`. To size a run from a dotenv, leave the variable out of `vars:`
> entirely.

## Depending on a looped task

`needs` works both ways, because the iterations are real tasks with real names:

```yaml
- name: report
  needs: [create_user]            # waits for EVERY iteration

- name: audit
  needs: ["create_user[3]"]       # waits for just that one
```

A looped task can also depend on *its own* matching iteration in another loop —
the loop variable is in scope in `needs` too:

```yaml
- name: verify_user
  loop: { var: n, from: 1, to: 5 }
  needs: ["create_user[{{ n }}]"]   # verify_user[3] waits only for create_user[3]
  steps: [ ... ]
```

That is the difference between a fan-in (`needs: [create_user]`, one bottleneck)
and a per-item chain (`create_user[{{ n }}]`, five independent pipelines) —
worth choosing deliberately, since the graph's parallel layers reflect it.

## Naming rules

htest appends `[item]` to each iteration itself, so the loop variable **must not
appear in `name:`**. A name that changed per iteration would leave no stable base
name for `needs:` to refer to, so it is rejected up front:

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
they render the same for every iteration.

## Loops and idempotency

An `idempotent` block is evaluated per iteration, so "create if not exists"
scales to a whole set without extra work — a rerun skips the users that already
exist and creates only the missing ones:

```yaml
- name: create_user
  loop: { var: n, from: 1, to: 5 }
  idempotent:
    check: { selector: ".row-user{{ n }}", exists: true }
    on_exists: skip
  steps: [ ... ]
```

Try it against the mock backend, which seeds "already exists" from an env var —
here users 1 and 2 exist, user 3 doesn't:

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
  task simply doesn't exist — and a `needs:` on it resolves to nothing rather
  than failing.
- **10,000 iterations is the cap.** A typo like `to: 1000000` is rejected with a
  clear message instead of building a graph that never finishes.
- **Items are strings.** Numbers keep their plain form (`1`, not `1.0`);
  anything that isn't a string, number or bool is rejected.
- **Task order in the file is still irrelevant.** Iterations are independent of
  each other unless you wire them with `needs`.
- **Document-level Jinja blocks.** A manifest using `loop:` is parsed as YAML
  before templating, so it must be valid YAML on its own — `{% for %}` blocks
  spanning lines belong in a manifest without `loop:`.

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

← [Screenshots](screenshots.md) · [Playbooks →](playbooks.md)
