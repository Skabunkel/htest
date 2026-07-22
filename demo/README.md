# htest playground

A tiny, self-contained web app used as the test bed for htest. It has **no
server and no network dependencies** — every page is static HTML with inline
JS, so you open it straight over `file://`. The reference manifest
[`../examples/tour.yaml`](../examples/tour.yaml) drives every page here.

## Pages

| Page | Exercises | Key selectors |
|------|-----------|---------------|
| `index.html` | Landing + nav bar | `#home-title`, `#nav-*` |
| `popup.html` | Modal added to / removed from the DOM | `#open-popup`, `#popup`, `#popup-text`, `#close-popup` |
| `spinner.html` | Loader that clears after ~1.5 s | `#load`, `#spinner`, `#result` |
| `form.html` | Text, checkbox, radio, file picker → summary | `#name`, `#email`, `#bio`, `#subscribe`, `#plan-pro`, `#avatar`, `#submit`, `#sum-*` |
| `snake.html` | 5×5 checkbox grid; tick the winning path | `#cb-<r>-<c>`, `#snake-win` |
| `scroll.html` | Button far below the fold | `#bottom-btn`, `#reached` |

Elements the tour asserts on presence/absence (`#popup`, `#spinner`, `#result`,
`#snake-win`, `#reached`, `#summary`) are **added to and removed from the DOM**
rather than merely hidden, so `assert exists: true/false` and
`wait_for … until: absent` mean what they say.

## Run the tour

```bash
# 1. Start a driver
geckodriver --port 4444 &

# 2. Watch it run in a visible browser, slowly, and keep going past the
#    deliberate failure (the `broken` task):
htest run examples/tour.yaml \
  --driver webdriver --browser firefox \
  --keep-going --shot-on-fail

# Headless (CI): add --headless
```

The run ends with **8 passed, 1 failed** — `broken` fails on purpose to show
the run continues and `finish` still executes. Screenshots (including
`FAIL-tour-broken.png`) land in `screenshots/`.

## Paths

`tour.yaml` uses two path vars:

Both are relative paths, so nothing needs editing — the tour runs from any
checkout on any machine. htest resolves a relative path against the directory
you run it from, falling back to the manifest's own directory when the file
isn't there; here the fallback is what makes `../demo` resolve, so the tour also
works when run from outside the repo.

- `BASE_URL` — this folder as a relative `file://` URL: `file://../demo`,
  expanded to an absolute `file://` URL at load time.
- `AVATAR` — the file the form's picker uploads (`../demo/assets/sample.txt`),
  resolved the same way. The `upload` step turns it into an absolute path and
  checks it exists before handing it to the driver.
