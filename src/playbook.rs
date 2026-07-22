//! Playbooks — the CI orchestration layer above single manifests.
//!
//! A manifest file is the base unit (a suite of tasks). A *playbook* collects
//! several of them, sets run-wide `settings`, and can declare file-level
//! ordering (`suites[].needs`) — "this suite runs only after those suites".
//!
//! `Settings` fields are all optional so three sources can be layered
//! (`overlay`): CLI flags win over the playbook file, which wins over built-in
//! defaults. `resolve` turns the merged, still-optional settings into the
//! concrete `Resolved` values the runner needs.

use crate::browser::{BrowserKind, Driver};
use crate::error::Result;
use serde::Deserialize;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

/// A playbook document.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Playbook {
    #[serde(default)]
    pub settings: Settings,
    /// The suites to run, each a manifest file with optional file-level `needs`.
    pub suites: Vec<Suite>,
}

/// One suite reference within a playbook.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    /// Path to a manifest file (relative to the playbook's directory).
    pub file: PathBuf,
    /// Other suite files that must complete before this one.
    #[serde(default)]
    pub needs: Vec<PathBuf>,
}

/// Run settings. Every field optional so CLI > playbook > default can layer.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub driver: Option<String>,
    pub webdriver_url: Option<String>,
    pub browser: Option<String>,
    pub headless: Option<bool>,
    pub window: Option<String>,
    pub browser_args: Option<Vec<String>>,
    pub env: Option<PathBuf>,
    pub screenshots: Option<PathBuf>,
    pub keep_going: Option<bool>,
    pub shot_on_fail: Option<bool>,
    /// Implicit-wait budget, milliseconds.
    pub timeout: Option<u64>,
    /// If true, htest spawns and kills the WebDriver server itself.
    pub manage_driver: Option<bool>,
    /// Override the driver executable (else looked up on PATH).
    pub driver_path: Option<PathBuf>,
    /// Port for the managed driver.
    pub driver_port: Option<u16>,
    /// Global wall-clock cap for the whole run, seconds.
    pub max_run_time: Option<u64>,
}

impl Settings {
    /// Merge `self` (higher precedence, e.g. CLI) over `base` (lower, e.g. the
    /// playbook file). Any field set on `self` wins; otherwise `base`'s is kept.
    pub fn overlay(self, base: Settings) -> Settings {
        Settings {
            driver: self.driver.or(base.driver),
            webdriver_url: self.webdriver_url.or(base.webdriver_url),
            browser: self.browser.or(base.browser),
            headless: self.headless.or(base.headless),
            window: self.window.or(base.window),
            browser_args: self.browser_args.or(base.browser_args),
            env: self.env.or(base.env),
            screenshots: self.screenshots.or(base.screenshots),
            keep_going: self.keep_going.or(base.keep_going),
            shot_on_fail: self.shot_on_fail.or(base.shot_on_fail),
            timeout: self.timeout.or(base.timeout),
            manage_driver: self.manage_driver.or(base.manage_driver),
            driver_path: self.driver_path.or(base.driver_path),
            driver_port: self.driver_port.or(base.driver_port),
            max_run_time: self.max_run_time.or(base.max_run_time),
        }
    }

    /// Collapse optional settings into concrete run values, applying defaults.
    pub fn resolve(self) -> Result<Resolved> {
        let driver = match &self.driver {
            Some(s) => Driver::from_str(s).map_err(|e| anyhow::anyhow!(e))?,
            None => Driver::Mock,
        };
        let browser = match &self.browser {
            Some(s) => BrowserKind::from_str(s).map_err(|e| anyhow::anyhow!(e))?,
            None => BrowserKind::Firefox,
        };
        let window = self.window.as_deref().map(parse_window).transpose()?;
        let driver_port = self.driver_port.unwrap_or(4444);
        let manage_driver = self.manage_driver.unwrap_or(false);

        if manage_driver && driver != Driver::WebDriver {
            anyhow::bail!("manage_driver requires `driver: webdriver`");
        }

        // When htest owns the driver, the URL is fixed by the port it spawns on.
        let webdriver_url = if manage_driver {
            format!("http://localhost:{driver_port}")
        } else {
            self.webdriver_url
                .unwrap_or_else(|| "http://localhost:4444".to_string())
        };

        Ok(Resolved {
            driver,
            webdriver_url,
            browser,
            headless: self.headless.unwrap_or(false),
            window,
            browser_args: self.browser_args.unwrap_or_default(),
            env: self.env,
            screenshots: self
                .screenshots
                .unwrap_or_else(|| PathBuf::from("screenshots")),
            keep_going: self.keep_going.unwrap_or(false),
            shot_on_fail: self.shot_on_fail.unwrap_or(false),
            timeout_ms: self.timeout.unwrap_or(5000),
            manage_driver,
            driver_path: self.driver_path,
            driver_port,
            max_run_time: self.max_run_time.map(Duration::from_secs),
        })
    }
}

/// Concrete, fully-defaulted run settings.
pub struct Resolved {
    pub driver: Driver,
    pub webdriver_url: String,
    pub browser: BrowserKind,
    pub headless: bool,
    pub window: Option<(u32, u32)>,
    pub browser_args: Vec<String>,
    pub env: Option<PathBuf>,
    pub screenshots: PathBuf,
    pub keep_going: bool,
    pub shot_on_fail: bool,
    pub timeout_ms: u64,
    pub manage_driver: bool,
    pub driver_path: Option<PathBuf>,
    pub driver_port: u16,
    pub max_run_time: Option<Duration>,
}

impl Resolved {
    /// Display name of the target browser.
    pub fn browser_name(&self) -> &'static str {
        match self.browser {
            BrowserKind::Firefox => "firefox",
            BrowserKind::Chrome => "chrome",
        }
    }
}

/// Load and parse a playbook file (no templating — settings are static).
pub fn load(path: &std::path::Path) -> Result<Playbook> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading playbook {}: {e}", path.display()))?;
    let pb: Playbook = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("parsing playbook {}: {e}", path.display()))?;
    Ok(pb)
}

/// Parse a `WxH` window spec like `1280x800`.
pub fn parse_window(s: &str) -> Result<(u32, u32)> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| anyhow::anyhow!("window must be WxH, e.g. 1280x800 (got `{s}`)"))?;
    Ok((w.trim().parse()?, h.trim().parse()?))
}
