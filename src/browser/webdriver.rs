//! WebDriver backend. Drives any browser via a running WebDriver server
//! (geckodriver for Firefox, chromedriver for Chrome, etc).
//!
//! Start a server first, e.g.:
//!   geckodriver --port 4444
//!   chromedriver --port=4444
//!
//! Version note: geckodriver is largely version-independent; chromedriver must
//! match the installed Chrome's major version (Chrome-for-Testing automates
//! this). Point `--webdriver-url` at whichever server you started.

use super::{BackendOpts, Browser, BrowserKind};
use crate::error::Result;
use crate::selector::Selector;
use fantoccini::error::CmdError;
use fantoccini::elements::Element;
use fantoccini::{Client, ClientBuilder, Locator};
use serde_json::{json, Map, Value};
use std::path::Path;
use tokio::runtime::Runtime;

pub struct WebDriverBrowser {
    // `rt` is declared before `client` so it is dropped last; the Drop impl
    // uses it to close the session cleanly.
    rt: Runtime,
    client: Client,
}

/// Translate BackendOpts into WebDriver capabilities.
fn build_capabilities(opts: &BackendOpts) -> Map<String, Value> {
    let mut caps = Map::new();
    // `eager`: `goto` returns at DOMContentLoaded instead of blocking until every
    // subresource (images, ads, trackers) finishes. Huge speedup on real sites
    // like Wikipedia; the engine's implicit-wait covers anything still settling.
    caps.insert("pageLoadStrategy".into(), json!("eager"));
    match opts.browser {
        BrowserKind::Firefox => {
            caps.insert("browserName".into(), json!("firefox"));
            let mut args: Vec<String> = Vec::new();
            if opts.headless {
                args.push("-headless".into());
            }
            args.extend(opts.browser_args.iter().cloned());
            caps.insert("moz:firefoxOptions".into(), json!({ "args": args }));
        }
        BrowserKind::Chrome => {
            caps.insert("browserName".into(), json!("chrome"));
            let mut args: Vec<String> = Vec::new();
            if opts.headless {
                args.push("--headless=new".into());
            }
            args.extend(opts.browser_args.iter().cloned());
            caps.insert("goog:chromeOptions".into(), json!({ "args": args }));
        }
    }
    caps
}

impl WebDriverBrowser {
    pub fn connect(opts: &BackendOpts) -> Result<Self> {
        let url = opts.webdriver_url;
        let caps = build_capabilities(opts);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let client = rt.block_on(async {
            let client = ClientBuilder::native()
                .capabilities(caps)
                .connect(url)
                .await
                .map_err(|e| anyhow::anyhow!("connecting to WebDriver at {url}: {e}"))?;
            // Window size is applied uniformly via setWindowRect (works for
            // both browsers, headless or not).
            if let Some((w, h)) = opts.window {
                client
                    .set_window_rect(0, 0, w, h)
                    .await
                    .map_err(|e| anyhow::anyhow!("setting window size: {e}"))?;
            }
            Ok::<_, anyhow::Error>(client)
        })?;
        Ok(WebDriverBrowser { rt, client })
    }
}

/// Find elements matching `css`, scoped to `scope` (document if `None`).
async fn find_scoped(
    client: &Client,
    scope: Option<&Element>,
    css: &str,
) -> std::result::Result<Vec<Element>, CmdError> {
    match scope {
        None => client.find_all(Locator::Css(css)).await,
        Some(el) => el.find_all(Locator::Css(css)).await,
    }
}

/// Rendered text of every element, in one `execute` round trip. `innerText`
/// mirrors WebDriver's Get Element Text (visible, whitespace-collapsed), which
/// is what the previous per-element `el.text()` returned.
async fn element_texts(
    client: &Client,
    els: &[Element],
) -> std::result::Result<Vec<String>, CmdError> {
    let args: Vec<Value> = els
        .iter()
        .map(|e| serde_json::to_value(e).expect("Element serializes to a WebDriver ref"))
        .collect();
    let script = "return Array.prototype.map.call(arguments, function(e){ return e.innerText; });";
    let v = client.execute(script, args).await?;
    let texts: Vec<String> = serde_json::from_value(v).unwrap_or_default();
    Ok(texts)
}

/// Resolve a (possibly hierarchical) selector to the list of matching elements.
/// Applies, in order: CSS match at this level -> text filter -> `nth` pick ->
/// descend via `find`. Recursion is boxed because the future is self-referential.
fn resolve<'a>(
    client: &'a Client,
    scope: Option<&'a Element>,
    sel: &'a Selector,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::result::Result<Vec<Element>, CmdError>> + 'a>>
{
    Box::pin(async move {
        let mut cands = find_scoped(client, scope, sel.css_or_any()).await?;

        // Text filtering (CSS cannot do this). Fetch every candidate's text in
        // ONE round trip via `execute` — doing `el.text()` per element turns a
        // link-heavy page (Wikipedia: thousands of <a>) into thousands of
        // WebDriver requests, which was the dominant source of slowness.
        if !cands.is_empty() && (sel.contains.is_some() || sel.text.is_some()) {
            let texts = element_texts(client, &cands).await?;
            let mut kept = Vec::new();
            for (el, t) in cands.into_iter().zip(texts.into_iter()) {
                let ok = sel.contains.as_ref().map_or(true, |c| t.contains(c.as_str()))
                    && sel.text.as_ref().map_or(true, |x| t.trim() == x);
                if ok {
                    kept.push(el);
                }
            }
            cands = kept;
        }

        // Pick the Nth match.
        if let Some(n) = sel.nth {
            cands = cands.into_iter().nth(n).into_iter().collect();
        }

        // Descend into each match.
        if let Some(child) = &sel.find {
            let mut out = Vec::new();
            for el in &cands {
                let mut sub = resolve(client, Some(el), child).await?;
                out.append(&mut sub);
            }
            cands = out;
        }

        Ok(cands)
    })
}

impl WebDriverBrowser {
    fn resolve_all(&self, sel: &Selector) -> Result<Vec<Element>> {
        let v = self
            .rt
            .block_on(async { resolve(&self.client, None, sel).await })?;
        Ok(v)
    }

    fn resolve_first(&self, sel: &Selector) -> Result<Element> {
        self.resolve_all(sel)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("selector matched no elements: {}", sel.descriptor()))
    }
}

impl Browser for WebDriverBrowser {
    fn goto(&mut self, url: &str) -> Result<()> {
        self.rt.block_on(async { self.client.goto(url).await })?;
        Ok(())
    }

    fn exists(&mut self, selector: &Selector) -> Result<bool> {
        Ok(!self.resolve_all(selector)?.is_empty())
    }

    fn click(&mut self, selector: &Selector) -> Result<()> {
        let el = self.resolve_first(selector)?;
        self.rt.block_on(async { el.click().await })?;
        Ok(())
    }

    fn fill(&mut self, selector: &Selector, value: &str) -> Result<()> {
        let el = self.resolve_first(selector)?;
        // Clear then type; real key events fire input/change for frameworks.
        self.rt.block_on(async {
            let _ = el.clear().await;
            el.send_keys(value).await
        })?;
        Ok(())
    }

    fn text(&mut self, selector: &Selector) -> Result<String> {
        let el = self.resolve_first(selector)?;
        let t = self.rt.block_on(async { el.text().await })?;
        Ok(t)
    }

    fn screenshot(&mut self, path: &Path) -> Result<()> {
        let png = self.rt.block_on(async { self.client.screenshot().await })?;
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        std::fs::write(path, png)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "webdriver"
    }
}

impl Drop for WebDriverBrowser {
    fn drop(&mut self) {
        // Best-effort session close so the browser window doesn't linger.
        let client = self.client.clone();
        let _ = self.rt.block_on(async { client.close().await });
    }
}
