# Actions & steps

A task's `steps` are an ordered list. Each step is a single-key map — the key
names the action, the value is its argument. Here's every step type.

Where a step takes a `<selector>` it's either a plain CSS string or the
structured form — see [Selectors](selectors.md). Steps that act on an element
(`click`, `fill`, `upload`) **implicitly wait** for it to appear first; see
[Waits](waits.md).

## At a glance

| Step | Form | What it does |
|------|------|--------------|
| `goto` | `goto: "<url>"` | Navigate to a URL. |
| `click` | `click: <selector>` | Click an element. |
| `fill` | `fill: { selector, value }` | Clear an input, then type text. |
| `upload` | `upload: { selector, path }` | Set a file `<input>` to a local file. |
| `assert` | `assert: { selector, exists, text }` | Check presence / text (polls). |
| `wait_for` | `wait_for: <selector>` / `{ selector, until, timeout }` | Block until present or absent. |
| `wait` | `wait: <ms>` | Fixed, unconditional pause. |
| `screenshot` | `screenshot: "<name.png>"` | Save a viewport image. |

## `goto` — navigate

Load a URL. The WebDriver backend uses `pageLoadStrategy: eager`, so it returns
at DOMContentLoaded rather than waiting on every image and tracker.

```yaml
- goto: "{{ BASE_URL }}/login"
- goto: "file:///srv/pages/index.html"          # absolute file:// works too
- goto: "file://../demo/index.html"             # relative to this manifest
- goto: "./pages/index.html"                    # bare relative path, same thing
```

### Relative file locations

A local page does not need a machine-specific absolute path. Anything that is
not an `http(s)://` (or `data:`, `about:`, …) URL is treated as a file path and
expanded to an absolute `file://` URL when the manifest loads:

| written | treated as |
|---------|-----------|
| `file://./x.html`, `file://../x.html`, `file://x.html` | relative file path |
| `./x.html`, `../x.html`, `~/x.html`, `/srv/x.html`, `C:\x.html` | file path |
| `pages/x.html` | file path **only if** that file exists |
| `https://…`, `data:…`, `localhost:8080/app` | left untouched |

Relative paths are resolved against **the directory you run htest from** first;
if the file isn't there, against **the manifest's own directory**. So relative
paths mean what they normally mean, a checkout runs on any machine without
editing, and a manifest invoked from somewhere else still finds the files next
to it. `~` and spaces are handled (`./my pages/a.html` →
`file:///…/my%20pages/a.html`).

## `click` — click

Click the element the selector resolves to. Waits for it to appear first, and
the driver scrolls it into view — so a button below the fold is fine.

```yaml
- click: "#submit"                                  # plain CSS
- click: { css: "tr", contains: "Alice", find: { css: "button", text: "Delete" } }
```

## `fill` — type into an input

Clears the field, then types the value with real key events (so `input` /
`change` handlers and frameworks react). Waits for the field first. Templating
applies to `value`.

```yaml
- fill: { selector: "#user", value: "{{ USERNAME }}" }
- fill: { selector: "#search", value: "hello world" }
```

## `upload` — set a file input

Points a `<input type=file>` at a file on disk. A relative `path` is resolved
like a relative `goto` — against the directory you run htest from, then the
manifest's own directory (an absolute path passes through); the file must exist
or the step fails with a clear message before the driver is touched.

```yaml
- upload: { selector: "#avatar", path: "demo/assets/sample.txt" }
```

> **Local drivers only.** The file must live on the same machine as the browser.
> Prefer a relative `path` so manifests stay portable across checkouts.

## `assert` — check the page

Verify a selector's presence and, optionally, its exact text. It *polls* until
the expectation holds or the wait budget expires, so you rarely need an explicit
wait before it.

- `exists` (default `true`) — wait for the element to appear; `exists: false`
  waits for it to *disappear*.
- `text` (optional) — the element's trimmed text must equal this exactly.

```yaml
- assert: { selector: "#dashboard", exists: true }
- assert: { selector: ".row-alice", exists: false }        # gone after delete
- assert: { selector: "h1", text: "Welcome, Alice" }       # exact text
```

On failure the task fails; with `--shot-on-fail` a screenshot is captured. A
`text` that never matches within the budget is a failure too.

## `wait_for` — wait for a condition

Block until a selector is `present` (default) or `absent`, then continue
immediately. Prefer it over `wait`. On timeout the task **fails**.

```yaml
- wait_for: "#ready"                                        # until present
- wait_for: { selector: ".spinner", until: absent, timeout: 15000 }
```

Full treatment — including the spinner pattern — in [Waits & timing](waits.md).

## `wait` — fixed pause

Sleep a fixed number of milliseconds, unconditionally. A last resort for
signal-less delays; reach for `wait_for` instead whenever there's a DOM
condition to key off.

```yaml
- wait: 250
```

## `screenshot` — capture the viewport

Save a PNG into the screenshots directory (`--screenshots`, default
`screenshots/`). Subdirectories in the name are created as needed. See
[Screenshots](screenshots.md) for automatic `--shot-on-fail` captures.

```yaml
- screenshot: "after-login.png"
- screenshot: "login/step2.png"
```

## Putting it together

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
