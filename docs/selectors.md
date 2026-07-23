# Selectors

A selector is how htest points at an element on the page. There are two forms,
and they build on each other: a **plain CSS string**, which is the foundation and
covers most cases, and a **structured hierarchical** form that layers on the two
things CSS cannot do — matching by text, and scoping a search inside an earlier
match. This page starts with plain CSS and works up to the structured form and
its finer points.

## Plain CSS

Anywhere a step takes a selector, a bare string is treated as a CSS selector and
passed straight to the driver. Nothing is added or reinterpreted, so the full CSS
vocabulary is available — including every combinator. Descendant (`.a .b`), child
(`.a > .b`), sibling, `:nth-child`, and the rest all work, which means plain CSS
*already* expresses hierarchy.

```yaml
click: "#submit"
click: ".tables button"        # a button anywhere inside .tables
click: ".tables > tr > .del"   # child combinator
assert: { selector: "nav .user-menu", exists: true }
```

Reach for plain CSS whenever the target is uniquely addressable by its structure,
classes, or ids. It is the fastest path htest has: a single query against the
DOM, with no scanning of element text.

### Attribute selectors (generated ids)

Modern frameworks often emit ids like `Email_a1b2c3` — a stable prefix followed
by a random, per-render suffix. Hard-coding `#Email_a1b2c3` is fragile because it
won't survive the next render, and a structural selector such as
`.container > .value > span > input` is too broad because it matches every input
on the page (28 of them, say). CSS **attribute operators** pin exactly the one you
want by matching the *stable part* of the attribute, with no fuzzy guessing:

```yaml
# id STARTS WITH "Email_"     (^=)
click: '.container > .value > span > input[id^="Email_"]'

# id CONTAINS "Email"         (*=)
click: '.container > .value > span > input[id*="Email"]'

# id ENDS WITH "_email"       ($=)
fill:
  selector: 'input[id$="_email"]'
  value: "user@example.com"
```

| Operator | Matches when the attribute… |
|----------|-----------------------------|
| `[id^="Email_"]` | starts with `Email_` |
| `[id*="Email"]` | contains `Email` anywhere |
| `[id$="_email"]` | ends with `_email` |
| `[name="email"]` | equals `email` exactly |

These operators work on *any* attribute, not just `id` — `[name^=…]`,
`[data-test*=…]`, and so on. Quote the whole selector in YAML so the `[` and `"`
characters aren't misread by the parser. And if several elements still match after
this, you have two ways to disambiguate: add a CSS pseudo-class like
`:nth-of-type(n)`, or step up to the structured form below and use its `nth`
field.

## Structured (hierarchical) form

Two needs push past what CSS can express: matching an element by its **text
content**, and saying "find *this*, then descend into it and find *that*." The
structured form adds exactly those. Instead of a string, you write a small map
whose fields describe one level of the search; `find` lets a level nest another
inside it.

```yaml
selector:
  css: ".row"           # match at this level (default: any element)
  contains: "Alice"     # keep only elements whose text contains this
  text: "Exact"         # OR: trimmed text equals this exactly
  nth: 0                # pick the Nth match (default: first)
  find:                 # descend INTO the match, resolve recursively
    css: "button"
    contains: "Delete"
```

| Field | Meaning |
|-------|---------|
| `css` | CSS to match at this level. Omit to match any element (`*`). |
| `contains` | Keep only elements whose text *contains* this substring (case-sensitive). |
| `text` | Keep only elements whose trimmed text *equals* this exactly. |
| `nth` | Pick the Nth surviving match (0-based). Default: the first. |
| `find` | A nested selector, resolved *inside* each match. Nests to any depth. |

Only these fields are accepted; an unknown key is rejected rather than ignored, so
a typo like `contain:` is caught immediately instead of silently matching nothing.

## Nesting with `find`

`find` is the "descend into" operator, and it is what makes the structured form
*hierarchical*. It takes another full selector and resolves it **inside** each
element the current level matched — not against the whole document. So the outer
selector chooses a *scope*, and `find` searches only within that scope. Because
each `find` is itself a complete structured selector, it can nest again.

The map form can be written inline on a single line:

```yaml
# "the Delete button inside the <tr> whose text contains Alice"
click: { css: "tr", contains: "Alice", find: { css: "button", text: "Delete" } }
```

Read it left to right: match every `tr` → keep the one whose text contains
"Alice" → descend into that row → find the `button` whose text is exactly
"Delete".

### Why not just CSS?

A CSS descendant combinator (`tr button`) also descends — but it can only filter
by structure, never by **text**, at either end. `find` can filter by text at every
level, because each level is a full structured selector: you can pin the row by
the name it contains *and* the button by its label. The rule of thumb: use plain
CSS when structure alone uniquely identifies the target, and reach for `find` the
moment text is what disambiguates a level.

### The classic example

The motivating case is "click the Delete button on Alice's row." Plain CSS can't
express it, because every row has an identical Delete button and nothing
structural separates Alice's. The structured form reads exactly as you'd say it
out loud:

```yaml
click:
  css: ".row"                       # every row...
  contains: "Alice"                 # ...keep the one containing "Alice"...
  find:                             # ...then, inside it,
    css: "button"                   # the button...
    contains: "Delete"              # ...that says Delete.
```

This example is verified live in `examples/fixtures.yaml`.

### Nesting deeper

`find` nests to any depth — each level scopes the one below it — and combines with
`nth` to pick among siblings at whatever level they appear. That lets you walk
several containers deep while staying anchored the whole way down:

```yaml
# Inside the "Billing" card, inside its second row, click the "Edit" link.
click:
  css: ".card"
  contains: "Billing"        # the Billing card...
  find:
    css: ".row"
    nth: 1                    # ...its 2nd row (0-based)...
    find:
      css: "a"
      text: "Edit"            # ...the Edit link within.
```

Every level runs the full resolution order below before handing its match on to
the next `find`.

## Resolution order

Within a single level, the fields never fight over precedence: htest always
applies them in one fixed order, so the result is predictable no matter how you
write the map.

> CSS match → text filter (`contains`/`text`) → `nth` pick → descend via `find`

In words: first the `css` narrows the candidates, then the text filter keeps only
those whose text matches, then `nth` selects one of the survivors, and finally
`find` descends into it. Knowing this order is what lets you reason about a
selector like the Billing example above with confidence.

## `contains` vs `text`

Both filter by text, but they differ in strictness, and the choice matters.
`contains` is a *substring* test; `text` is an *exact*, trimmed-equality test.
Prefer `text` whenever a substring could be ambiguous — a shorter title is often a
prefix of a longer one:

```yaml
# DANGER: "Sailor Moon" also matches "Sailor Moon R", "Sailor Moon S"...
click: { css: "#bodyContent a", contains: "Sailor Moon" }

# SAFE: only the link whose text is exactly "Sailor Moon"
click: { css: "#bodyContent a", text: "Sailor Moon" }
```

## Performance

Text matching is not free, so it pays to understand the cost. To apply `contains`
or `text`, htest must read the text of *every* element the CSS matched — on a
link-heavy page, `css: "a"` can mean thousands of candidates. htest fetches all of
that text in a single round trip to the browser, so the network overhead is fixed
rather than per-element, but you'll still go faster by narrowing the CSS first so
there are fewer candidates to read in the first place.

```yaml
# slow: every <a> on the page considered
click: { css: "a", text: "Next" }
# fast: only anchors inside the pager
click: { css: "#pager a", text: "Next" }
```

## Where selectors are used

Both forms — plain CSS and structured — are accepted identically everywhere htest
references an element: the [`click`](actions.md) and [`fill`](actions.md) steps
(the latter via `fill.selector`), `upload.selector`, `assert.selector`,
[`wait_for`](waits.md) (its shorthand string and its `selector` field), and the
idempotency `check.selector`. Learn the two forms once and they apply across the
whole manifest.

---

← [Actions & steps](actions.md) · [Waits & timing →](waits.md)
