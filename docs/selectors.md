# Selectors

How htest finds elements. A selector is either a **plain CSS string**, or a
**structured hierarchical** form that adds what CSS can't express.

## Plain CSS

Anywhere a step takes a selector, a bare string is a CSS selector, passed
straight through. All combinators work — this *is* hierarchy.

```yaml
click: "#submit"
click: ".tables button"        # a button anywhere inside .tables
click: ".tables > tr > .del"   # child combinator
assert: { selector: "nav .user-menu", exists: true }
```

Reach for CSS whenever the target is uniquely addressable by
structure/classes/ids. It's the fastest path — one query, no text scan.

## Structured (hierarchical) form

CSS cannot match on *text content*, and cannot say "descend into this match,
then find that." The structured form adds both.

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
| `css` | CSS to match at this level. Omit to match any element. |
| `contains` | Keep only elements whose text *contains* this substring (case-sensitive). |
| `text` | Keep only elements whose trimmed text *equals* this exactly. |
| `nth` | Pick the Nth surviving match (0-based). Default: the first. |
| `find` | A nested selector, resolved *inside* each match. Nests to any depth. |

## Nesting with `find`

`find` is the "descend into" operator. It takes another selector and resolves it
**inside** each element the current level matched — not against the whole
document. So the outer selector picks a *scope*, and `find` searches only within
it.

The map form can be written inline on one line:

```yaml
# "the Delete button inside the <tr> whose text contains Alice"
click: { css: "tr", contains: "Alice", find: { css: "button", text: "Delete" } }
```

Read it left to right: match every `tr` → keep the one containing "Alice" →
descend into that row → find the `button` whose text is exactly "Delete".

### Why not just CSS?

A CSS descendant combinator (`tr button`) also descends, but it can't filter by
**text** at either level. `find` can — each level is a full structured selector,
so you can pin the row by its text *and* the button by its text. Use plain CSS
when structure alone is enough; reach for `find` the moment text disambiguates a
level.

### Nesting deeper

`find` nests to any depth — each level scopes the next. Combine with `nth` to
pick among siblings at any level.

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

Each level runs the full resolution order below before handing its match(es) to
the next `find`.

## Resolution order

At each level, htest applies the fields in a fixed order:

> CSS match → text filter (`contains`/`text`) → `nth` pick → descend via `find`

## The classic example

"Click the Delete button on Alice's row." Plain CSS can't target it — there's a
Delete button on every row. The structured form can:

```yaml
click:
  css: ".row"                       # every row...
  contains: "Alice"                 # ...keep the one containing "Alice"...
  find:                             # ...then, inside it,
    css: "button"                   # the button...
    contains: "Delete"              # ...that says Delete.
```

This reads exactly as intended and is verified live in
`examples/fixtures.yaml`.

## `contains` vs `text`

`contains` is a substring test; `text` is an exact (trimmed) equality test.
Prefer `text` when a substring would be ambiguous:

```yaml
# DANGER: "Sailor Moon" also matches "Sailor Moon R", "Sailor Moon S"...
click: { css: "#bodyContent a", contains: "Sailor Moon" }

# SAFE: only the link whose text is exactly "Sailor Moon"
click: { css: "#bodyContent a", text: "Sailor Moon" }
```

## Performance

Text matching (`contains`/`text`) has to read the text of every element the CSS
matched. On a link-heavy page, `css: "a"` can mean thousands of candidates.
htest fetches all their text in a single round trip, but you'll still go faster
by narrowing the CSS first — scope to the region that holds the target.

```yaml
# slow: every <a> on the page considered
click: { css: "a", text: "Next" }
# fast: only anchors inside the pager
click: { css: "#pager a", text: "Next" }
```

## Where selectors are used

Everywhere an element is referenced: `click`, `fill.selector`,
`assert.selector`, `wait_for` (and its `selector`), and the idempotency
`check.selector`. The same two forms work in all of them.

---

← [Actions & steps](actions.md) · [Waits & timing →](waits.md)
