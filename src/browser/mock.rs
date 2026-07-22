use super::Browser;
use crate::error::Result;
use crate::selector::Selector;
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

/// A dependency-free backend for developing and testing the runner without a
/// real browser. Deterministic and fast.
///
/// World model:
///   * Everything is assumed present, EXCEPT selectors seeded absent via the
///     `HTEST_MOCK_ABSENT` env var (comma-separated). This lets you drive the
///     idempotency gate: seed a selector absent to force the "create" path.
///   * `click`/`fill` mutate the world — they clear the absent set, modelling
///     "the page changed and things now exist" (so a post-create assert
///     passes). This makes idempotent create flows demonstrable end to end.
pub struct MockBrowser {
    absent: HashSet<String>,
    log: Vec<String>,
}

impl MockBrowser {
    pub fn new() -> Self {
        let absent = std::env::var("HTEST_MOCK_ABSENT")
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        MockBrowser {
            absent,
            log: Vec::new(),
        }
    }
}

impl Browser for MockBrowser {
    fn goto(&mut self, url: &str) -> Result<()> {
        self.log.push(format!("goto {url}"));
        Ok(())
    }

    fn exists(&mut self, selector: &Selector) -> Result<bool> {
        Ok(!self.absent.contains(&selector.descriptor()))
    }

    fn click(&mut self, selector: &Selector) -> Result<()> {
        let key = selector.descriptor();
        self.log.push(format!("click {key}"));
        // Interacting with an element only affects that element; a click does
        // not conjure unrelated selectors into existence.
        self.absent.remove(&key);
        Ok(())
    }

    fn fill(&mut self, selector: &Selector, value: &str) -> Result<()> {
        let key = selector.descriptor();
        self.log.push(format!("fill {key} = {value}"));
        self.absent.remove(&key);
        Ok(())
    }

    fn text(&mut self, _selector: &Selector) -> Result<String> {
        Ok(String::new())
    }

    fn screenshot(&mut self, path: &Path) -> Result<()> {
        // Write a minimal valid 1x1 PNG so downstream tooling sees a real file.
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let mut f = std::fs::File::create(path)?;
        f.write_all(&ONE_PX_PNG)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}

/// A 1x1 transparent PNG.
const ONE_PX_PNG: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
    0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
    0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];
