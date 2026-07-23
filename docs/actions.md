# Actions & steps

A task's `steps` are an ordered list that htest runs from top to bottom, one at a
time, in a single browser session. Each step is a **single-key map**: the key
names the action, and the value is its argument. That one rule is the whole
grammar — once you can read `- key: value`, you can read any step.

```yaml
steps:
  - goto: "{{ BASE_URL }}/login"     # key `goto`, value the URL
  - fill: { selector: "#user", value: "ada" }
  - click: "#submit"
```

Two conventions run through every step below. First, wherever a step takes a
`<selector>`, that value is either a plain CSS string or the richer structured
form — both are covered on the [Selectors](selectors.md) page. Second, the steps
that act on a specific element (`click`, `fill`, `upload`) **implicitly wait**
for that element to appear before touching it, so you rarely need to insert a
wait by hand; the [Waits & timing](waits.md) page explains the timing model in
full.

## At a glance

Every step type, shortest form first. The sections that follow introduce them in
roughly this order — the everyday ones first, the specialised ones last.

| Step | Form | What it does |
|------|------|--------------|
| `goto` | `goto: "<url>"` | Navigate to a URL (or local file). |
| `click` | `click: <selector>` | Click an element. |
| `fill` | `fill: { selector, value }` | Clear an input, then type text. |
| `assert` | `assert: { selector, exists, text }` | Check presence / text (polls). |
| `wait_for` | `wait_for: <selector>` / `{ selector, until, timeout }` | Block until present or absent. |
| `wait` | `wait: <ms>` | Fixed, unconditional pause. |
| `upload` | `upload: { selector, path }` | Set a file `<input>` to a local file. |
| `screenshot` | `screenshot: "<name.png>"` | Save a viewport image. |

## `goto` — navigate

Every task begins by putting the browser somewhere; `goto` is how you get there.
It loads a URL and blocks until the page is ready. "Ready" is deliberately
early: the WebDriver backend uses `pageLoadStrategy: eager`, so `goto` returns at
`DOMContentLoaded` — as soon as the HTML is parsed and the DOM is in place —
rather than waiting on every image, font, and tracker to finish. Your first
interaction can begin the moment the markup exists, and anything genuinely
asynchronous is better handled by the implicit waits described later than by a
slower page-load strategy.

```yaml
- goto: "{{ BASE_URL }}/login"
- goto: "file:///srv/pages/index.html"          # absolute file:// works too
- goto: "file://../demo/index.html"             # relative to this manifest
- goto: "./pages/index.html"                    # bare relative path, same thing
```

### Relative file locations

A local page does not need a machine-specific absolute path baked into the
manifest. Anything that is **not** an `http(s)://` URL (nor a `data:`, `about:`,
or `localhost:…` value) is treated as a file path and expanded to an absolute
`file://` URL when the manifest loads, so the same test runs on any checkout:

| written | treated as |
|---------|-----------|
| `file://./x.html`, `file://../x.html`, `file://x.html` | relative file path |
| `./x.html`, `../x.html`, `~/x.html`, `/srv/x.html`, `C:\x.html` | file path |
| `pages/x.html` | file path **only if** that file exists |
| `https://…`, `data:…`, `localhost:8080/app` | left untouched |

Relative paths are resolved in two attempts: first against **the directory you
run htest from**, and if the file isn't there, against **the manifest's own
directory**. This dual resolution is what lets relative paths keep their ordinary
meaning, lets a checkout run unedited on any machine, and still lets a manifest
invoked from elsewhere find the files sitting next to it. Note the one nuance in
the table: a *bare* relative like `pages/x.html` (no leading `./`, `~`, or
slash) is only rewritten to a `file://` URL if such a file actually exists —
otherwise it is left untouched, so it can still serve as a server-relative path.
Home directories (`~`) and spaces are handled for you; `./my pages/a.html`
becomes `file:///…/my%20pages/a.html`.

## `click` — click an element

`click` presses the element the selector resolves to, exactly as a user would.
Because clicking requires the target to exist, htest first waits for it to
appear, and the driver scrolls it into view before the click — so a button below
the fold, or one that renders slightly after the page loads, needs no special
handling.

```yaml
- click: "#submit"                                  # plain CSS
- click: { css: "tr", contains: "Alice", find: { css: "button", text: "Delete" } }
```

The second example is a taste of the [structured selector](selectors.md) form,
which lets you say "the Delete button *inside Alice's row*" — something plain CSS
can't express because it can't match on text.

## `fill` — type into an input

`fill` puts a value into a form field the way a person types it. It first waits
for the field, then **clears** any existing content, and finally types the value
with real key events — so `input` and `change` handlers fire and reactive
frameworks notice the change, which a bare "set the value" assignment would not
trigger. The `value` is [templated](variables.md), so variables and formatting
expand before typing.

```yaml
- fill: { selector: "#user", value: "{{ USERNAME }}" }
- fill: { selector: "#search", value: "hello world" }
```

## `assert` — check the page

`assert` is how a test states what should be true. It verifies that a selector is
present (or absent) and, optionally, that its text matches exactly. Crucially it
**polls**: rather than checking once and giving up, it keeps re-checking until the
expectation holds or the wait budget (`--timeout`) is exhausted. That makes
`assert` robust against content that arrives a beat late, and means you rarely
need an explicit wait in front of it.

- `exists` (default `true`) — wait for the element to *appear*. Set
  `exists: false` to instead wait for it to *disappear* (useful after a delete or
  a dismissed dialog).
- `text` (optional) — the element's trimmed text must *equal* this exactly, not
  merely contain it.

```yaml
- assert: { selector: "#dashboard", exists: true }
- assert: { selector: ".row-alice", exists: false }        # gone after delete
- assert: { selector: "h1", text: "Welcome, Alice" }       # exact text
```

If the expectation never holds within the budget the task fails — and that
includes a `text` value that never matches. With `--shot-on-fail` a screenshot is
captured automatically at the point of failure (see [Screenshots](screenshots.md)).

## `wait_for` — wait for a condition

`wait_for` pauses the task until the page reaches a specific DOM condition, then
continues *immediately* — no fixed delay, no guesswork. In its shorthand it waits
for a selector to become present; in the map form you choose the condition and,
optionally, a per-step timeout in milliseconds that overrides the global budget.
Prefer it over a blind `wait` whenever there is a signal to key off, and note
that a timeout here is a **failure** — that is the point: the condition you were
counting on never happened.

```yaml
- wait_for: "#ready"                                        # until present
- wait_for: { selector: ".spinner", until: absent, timeout: 15000 }
```

The full treatment — the `until: present|absent` options, the spinner pattern,
and how this interacts with the implicit waits — lives on the
[Waits & timing](waits.md) page.

## `wait` — fixed pause

`wait` sleeps for a fixed number of milliseconds, unconditionally, no matter what
the page is doing. It is the last resort: a fixed pause is either too short (and
flaky) or too long (and slow), so reach for `wait_for` whenever there is any DOM
condition to wait on, and keep `wait` for the rare signal-less delay.

```yaml
- wait: 250
```

## `upload` — set a file input

`upload` points a `<input type="file">` at a file on disk, so a test can exercise
an upload flow without a human clicking through the OS file picker. It waits for
the input and drives it through the same real-key-event mechanism as `fill`. A
relative `path` is resolved just like a relative `goto` — first against the
directory you run htest from, then the manifest's own directory — while an
absolute path passes straight through. The file is checked **before** the driver
is touched, so a missing file fails the step with a clear message rather than a
confusing browser error.

```yaml
- upload: { selector: "#avatar", path: "demo/assets/sample.txt" }
```

> **Local drivers only.** The file must live on the same machine as the browser,
> so this step won't work against a remote or cloud grid. Prefer a relative
> `path` so manifests stay portable across checkouts.

## `screenshot` — capture the viewport

`screenshot` saves a PNG of the current viewport, on demand, wherever you place
the step. Images land in the screenshots directory (`--screenshots`, default
`screenshots/`), and any subdirectories named in the filename are created for
you — handy for grouping shots by flow.

```yaml
- screenshot: "after-login.png"
- screenshot: "login/step2.png"
```

This is the *deliberate* capture; htest can also snap a shot automatically when a
step fails, via `--shot-on-fail`. Both are covered on the
[Screenshots](screenshots.md) page.

## Putting it together

With every step introduced, a realistic task reads as a straight-line script —
navigate, fill the form, attach a file, submit, wait for the work to finish,
assert the result, and capture proof:

```yaml
- goto: "{{ BASE_URL }}/signup"
- fill: { selector: "#name", value: "Ada" }
- upload: { selector: "#avatar", path: "demo/assets/sample.txt" }
- click: "#submit"
- wait_for: { selector: ".spinner", until: absent }
- assert: { selector: "#done", text: "Saved" }
- screenshot: "signup-done.png"
```

---

← [Tutorial](tutorial.md) · [Selectors →](selectors.md)
