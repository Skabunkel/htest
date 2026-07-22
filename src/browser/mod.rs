use crate::error::Result;
use crate::selector::Selector;
use std::path::Path;

pub mod mock;
pub mod webdriver;

/// Backend-agnostic browser control surface. Every backend (mock or a
/// WebDriver client) implements this. The engine is written against the trait
/// and never names a concrete backend.
///
/// Kept synchronous: the WebDriver backend is async internally but blocks on a
/// runtime per call, so the engine stays simple and sequential.
pub trait Browser {
    /// Navigate to `url` and wait for load.
    fn goto(&mut self, url: &str) -> Result<()>;

    /// True if at least one element resolves from `selector`.
    fn exists(&mut self, selector: &Selector) -> Result<bool>;

    /// Click the first element resolved from `selector`.
    fn click(&mut self, selector: &Selector) -> Result<()>;

    /// Set the value of the first input resolved from `selector`.
    fn fill(&mut self, selector: &Selector, value: &str) -> Result<()>;

    /// Text content of the first element resolved from `selector`.
    fn text(&mut self, selector: &Selector) -> Result<String>;

    /// Capture a screenshot to `path` (PNG).
    fn screenshot(&mut self, path: &Path) -> Result<()>;

    /// Human-readable backend name for logs.
    fn name(&self) -> &'static str;
}

/// Which backend to drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    Mock,
    WebDriver,
}

impl std::str::FromStr for Driver {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "mock" => Ok(Driver::Mock),
            "webdriver" | "wd" => Ok(Driver::WebDriver),
            other => Err(format!("unknown driver `{other}` (mock|webdriver)")),
        }
    }
}

/// Which browser the WebDriver session targets (selects the capability key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserKind {
    Firefox,
    Chrome,
}

impl std::str::FromStr for BrowserKind {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "firefox" | "ff" | "gecko" => Ok(BrowserKind::Firefox),
            "chrome" | "chromium" => Ok(BrowserKind::Chrome),
            other => Err(format!("unknown browser `{other}` (firefox|chrome)")),
        }
    }
}

/// Options for constructing a backend.
pub struct BackendOpts<'a> {
    /// WebDriver server URL (geckodriver/chromedriver), e.g. http://localhost:4444.
    pub webdriver_url: &'a str,
    /// Which browser the session targets.
    pub browser: BrowserKind,
    /// Launch without a visible window.
    pub headless: bool,
    /// Window size (width, height); applied via setWindowRect after launch.
    pub window: Option<(u32, u32)>,
    /// Extra browser CLI args passed through to the browser options capability.
    pub browser_args: Vec<String>,
}

/// Construct a backend.
pub fn create(driver: Driver, opts: &BackendOpts) -> Result<Box<dyn Browser>> {
    match driver {
        Driver::Mock => Ok(Box::new(mock::MockBrowser::new())),
        Driver::WebDriver => Ok(Box::new(webdriver::WebDriverBrowser::connect(opts)?)),
    }
}
