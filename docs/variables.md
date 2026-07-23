# Variables & formatting

A manifest is a **template first and YAML second**. Before htest parses a single
line of structure, it renders the whole document once — every `{{ VAR }}` is
replaced with text — and only then reads the result as YAML. That ordering is the
key to the whole page: because substitution happens *before* parsing, a
placeholder is just text and can appear *anywhere* — step values, selectors,
`needs`, screenshot names, even the `idempotent` check.

htest templates with [minijinja](https://docs.rs/minijinja) (a Jinja2 dialect)
in strict mode. This page starts with where the values come from, then works up
from the single most important fact — every value is a string — to the filters
that reshape those strings at the point of use: zero-pad a number, fix a width,
force a case, supply a default.

## Where values come from

Before rendering, htest assembles a single context — a flat set of
name-to-string pairs — from three sources. They are layered **low → high**
precedence, so when the same name appears in more than one, the **later (higher)
source wins**:

| Source | Example | Precedence |
|--------|---------|------------|
| Process environment | `BASE_URL=… htest run …` | lowest |
| `.env` file | `--env ci.env`, manifest `env:`, or `./.env` | middle |
| Manifest `vars:` | the `vars:` block at the top of the file | **highest** |

The `.env` file itself is chosen with the same "later wins" idea: an explicit
`--env` on the command line beats the manifest's own `env:` key, which in turn
beats a plain `./.env` sitting next to the manifest.

```yaml
vars:
  BASE_URL: "https://example.com"
  USERNAME: "alice"
tasks:
  - name: open
    steps:
      - goto: "{{ BASE_URL }}/users/{{ USERNAME }}"
```

Because `vars:` sits at the top, it is the natural place to pin a value for the
run; leave a name out of `vars:` when you want an environment variable or `.env`
file to be able to set it. Inside a [loop](loops.md) there is one more, even
higher layer: the loop variable (`item`/`n`, plus `loop_index` and
`loop_index1`) sits on top of this context for that one task.

> **Strict mode.** A name that no source defines is a **hard error**, never a
> silent blank — `missing variable ...`. That is deliberate: a typo fails the run
> immediately instead of quietly filling in an empty string. When a blank really
> is acceptable, say so explicitly with `|default(...)`.

## Everything is a string

This is the one fact that explains most surprises: **every value arrives as a
string.** The three sources above are all textual — environment variables and
`.env` entries are strings by nature, and even `vars: { N: 5 }` reaches the
template as the string `"5"`, not the number `5`. So before any arithmetic or
numeric formatting you must cast, with `|int` or `|float`:

```jinja
{{ N + 1 }}        {# ✗ error: adding to a string #}
{{ N|int + 1 }}    {# ✓ 6 #}
```

Keep this in mind for the rest of the page: the numeric filters below all begin
by casting, because the thing they receive is text.

## Formatting numbers — the `format` filter

To control how a number looks — pad it, fix its width, force a sign — use the
printf-style `format` filter. It takes a format spec on the left and the value
on the right, so the pattern is always *cast first, then format*. **In YAML,
quote the whole value** (it starts with `{`, which YAML would otherwise read as a
flow map), and use single quotes for the spec inside:

```yaml
- fill: { selector: "#day", value: "{{ '%02d'|format(DAY|int) }}" }
```

| Goal | Template | `N=5` → |
|------|----------|---------|
| Leading zero, 2 digits | `{{ '%02d'|format(N|int) }}` | `05` |
| 3 digits | `{{ '%03d'|format(N|int) }}` | `005` |
| Right-align, width 5 (spaces) | `{{ '%5d'|format(N|int) }}` | `‹␣␣␣␣5›` |
| Left-align, width 5 | `{{ '%-5d'|format(N|int) }}` | `‹5␣␣␣␣›` |
| Always show sign | `{{ '%+d'|format(N|int) }}` | `+5` |
| Float, 2 decimals | `{{ '%.2f'|format(N|float) }}` | `5.00` |
| Inside a larger string | `{{ 'user_%03d'|format(N|int) }}` | `user_005` |

The width and precision rules are standard printf: `%0<width>d` zero-pads to a
width, `%<width>d` space-pads to the same width, `%-<width>d` left-aligns instead
of right, and `%.<n>f` fixes the number of decimals. The last row is the useful
trick — a spec is a full string, so a literal prefix like `user_` can live right
inside it and get padded numbers in one step.

## Prefixing & building strings

Formatting handles one number; building an identifier or a URL means joining
several pieces. There are two complementary ways to do it:

```jinja
{# 1. concatenate with ~ (turns each side into text and joins) #}
{{ 'user_' ~ N }}                       {# user_5 #}
{{ BASE_URL ~ '/users/' ~ USERNAME }}   {# https://example.com/users/alice #}

{# 2. bake the prefix into a format spec — joins AND pads in one go #}
{{ 'user_%03d'|format(N|int) }}         {# user_005 #}

{# a conditional prefix, when you want padding only some of the time #}
{{ '0' ~ N if N|int < 10 else N }}      {# 05 when N<10, else N #}
```

Reach for `~` when you are stitching text together and for `format` when a piece
needs padding or a fixed width; the conditional form is the escape hatch for the
in-between cases.

## Common string filters

Filters transform a value at the point of use. Each one takes the value on its
left and returns a new one, and they **chain left to right** with `|`, so
`{{ NAME|trim|lower }}` trims *first* and then lowercases the result.

| Filter | Template | Result |
|--------|----------|--------|
| Uppercase | `{{ 'abc'|upper }}` | `ABC` |
| Lowercase | `{{ 'ABC'|lower }}` | `abc` |
| Title case | `{{ 'hello world'|title }}` | `Hello World` |
| Trim whitespace | `{{ '  x  '|trim }}` | `x` |
| Replace | `{{ 'a-b-c'|replace('-', '/') }}` | `a/b/c` |
| Default if undefined | `{{ MAYBE|default('none') }}` | `none` |
| Substring (slice) | `{{ 'abcdef'[0:3] }}` | `abc` |
| Absolute value | `{{ N|int|abs }}` | `5` |

Note the last row: `abs` is numeric, so it comes *after* an `|int` cast — the
same cast-first discipline as the `format` filter.

## Worked examples

Putting the pieces together — a zero-padded date assembled from three separate
variables, and a padded, prefixed account id generated across a
[loop](loops.md):

```yaml
vars:
  YEAR: "2026"
  MONTH: "7"
  DAY: "3"
tasks:
  - name: pick_date
    steps:
      # zero-padded ISO date -> 2026-07-03
      - fill:
          selector: "#date"
          value: "{{ YEAR }}-{{ '%02d'|format(MONTH|int) }}-{{ '%02d'|format(DAY|int) }}"

  - name: create_accounts
    loop: { var: n, from: 1, to: 12 }
    steps:
      # padded, prefixed id -> ACC-0001 … ACC-0012
      - fill: { selector: "#account", value: "ACC-{{ '%04d'|format(n|int) }}" }
```

## Gotchas

- **Values are strings.** Cast with `|int` / `|float` before arithmetic or
  numeric formatting. This is the #1 source of surprise, which is why it has its
  own section above.
- **Missing = error.** Strict mode fails loudly on an undefined variable rather
  than substituting a blank. Use `|default('...')` where an empty value is
  genuinely acceptable.
- **Filters don't mutate `vars:`.** They reshape the value at the point of use;
  the stored value the next reference sees is untouched.
- **Not everything from Jinja is available.** The `%` string operator, Python's
  `"...".format(...)`, and `rjust`/`ljust` are **not** supported — use the
  `format` filter (`{{ '%02d'|format(N|int) }}`) for all of these instead.
- **Quote values that start with `{`.** `value: {{ x }}` is invalid YAML (it
  looks like a flow map); always write `value: "{{ x }}"`.

---

← [Idempotency & checks](idempotency.md) · [Loops →](loops.md)
