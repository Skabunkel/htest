//! Execution engine — runs planned tasks in topological order against a
//! `Browser`, sequentially. (The graph exposes parallel layers for a future
//! concurrent executor; this loop is one-at-a-time.)
//!
//! Per task: the idempotency gate may skip it; otherwise each step runs in
//! order. A failed task marks its dependents `Blocked` (they never touch a
//! broken precondition). Timing is handled by implicit waits — clicks/fills
//! wait for their target, asserts poll until the expected presence holds or
//! `wait_timeout` elapses — so manifests rarely need fixed `wait:` pauses.

use crate::browser::Browser;
use crate::error::{HtError, Result};
use crate::manifest::{
    AssertArgs, Check, Idempotent, OnExists, Step, Task, Until, UploadArgs, WaitForArgs,
};
use crate::plan::PlannedTask;
use anyhow::Context;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct EngineConfig {
    /// Directory for screenshots. Step `screenshot: name.png` resolves here.
    pub screenshot_dir: PathBuf,
    /// Take a screenshot automatically when a task fails.
    pub screenshot_on_fail: bool,
    /// Continue running remaining tasks after a failure.
    pub keep_going: bool,
    /// Implicit wait: how long to keep re-checking an assertion / element
    /// before giving up. Pages settle asynchronously (navigation, fetch,
    /// framework renders); a fixed `wait:` step is a guess, this is a bound.
    pub wait_timeout: Duration,
    /// Global wall-clock budget for the whole run. Checked *between* tasks: once
    /// exceeded, every remaining task is failed without running (a sync
    /// WebDriver call already in flight can't be interrupted). `None` = no cap.
    pub max_run_time: Option<Duration>,
}

/// How often the implicit-wait loop re-checks the browser.
const POLL_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Passed,
    Skipped,
    /// A prerequisite failed or was itself blocked, so this task never ran.
    Blocked,
    Failed,
}

pub struct TaskResult {
    pub name: String,
    pub status: Status,
    pub duration: Duration,
    pub detail: Option<String>,
}

pub struct RunReport {
    pub results: Vec<TaskResult>,
}

impl RunReport {
    pub fn failed(&self) -> bool {
        self.results.iter().any(|r| r.status == Status::Failed)
    }
    /// (passed, skipped, blocked, failed)
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let mut p = 0;
        let mut s = 0;
        let mut b = 0;
        let mut f = 0;
        for r in &self.results {
            match r.status {
                Status::Passed => p += 1,
                Status::Skipped => s += 1,
                Status::Blocked => b += 1,
                Status::Failed => f += 1,
            }
        }
        (p, s, b, f)
    }
}

pub struct Engine<'a> {
    browser: &'a mut dyn Browser,
    cfg: EngineConfig,
}

impl<'a> Engine<'a> {
    pub fn new(browser: &'a mut dyn Browser, cfg: EngineConfig) -> Self {
        Engine { browser, cfg }
    }

    /// Run tasks in the given order (topological). Sequential; the graph
    /// exposes parallel layers for a future concurrent executor. `order` and
    /// task `needs` are canonical ids (`namespace:name`).
    pub fn run(&mut self, planned: &[PlannedTask], order: &[String]) -> Result<RunReport> {
        let by_id: std::collections::HashMap<&str, &PlannedTask> =
            planned.iter().map(|p| (p.id.as_str(), p)).collect();

        let mut results = Vec::new();
        // Tasks that failed or were blocked; their dependents can't run.
        let mut bad: std::collections::HashSet<String> = std::collections::HashSet::new();

        let run_start = Instant::now();
        let deadline = self.cfg.max_run_time.map(|d| run_start + d);

        for id in order {
            let planned_task = by_id[id.as_str()];
            let task = planned_task.task;

            // Global time budget: once blown, fail everything still pending
            // rather than run it. Checked here so an in-flight task finishes.
            if let Some(dl) = deadline {
                if Instant::now() >= dl {
                    bad.insert(id.clone());
                    results.push(TaskResult {
                        name: id.clone(),
                        status: Status::Failed,
                        duration: Duration::ZERO,
                        detail: Some(format!(
                            "run aborted: exceeded max run time ({} s)",
                            self.cfg.max_run_time.unwrap().as_secs()
                        )),
                    });
                    continue;
                }
            }

            // Prerequisite gate: if any `needs` is bad, block this task too.
            if let Some(dep) = planned_task.needs.iter().find(|d| bad.contains(d.as_str())) {
                bad.insert(id.clone());
                results.push(TaskResult {
                    name: id.clone(),
                    status: Status::Blocked,
                    duration: Duration::ZERO,
                    detail: Some(format!("prerequisite `{dep}` did not pass")),
                });
                continue;
            }

            let start = Instant::now();
            let outcome = self.run_task(task);
            let duration = start.elapsed();

            let result = match outcome {
                Ok(Some(())) => TaskResult {
                    name: id.clone(),
                    status: Status::Passed,
                    duration,
                    detail: None,
                },
                Ok(None) => TaskResult {
                    name: id.clone(),
                    status: Status::Skipped,
                    duration,
                    detail: Some("idempotency gate: already satisfied".into()),
                },
                Err(e) => {
                    if self.cfg.screenshot_on_fail {
                        let safe = id.replace(':', "-");
                        let p = self.cfg.screenshot_dir.join(format!("FAIL-{safe}.png"));
                        let _ = self.browser.screenshot(&p);
                    }
                    TaskResult {
                        name: id.clone(),
                        status: Status::Failed,
                        duration,
                        // `{:#}` prints the whole anyhow chain (`step 3/6
                        // (click …): selector matched no elements: …`), not just
                        // the outermost context, so the report pinpoints the step.
                        detail: Some(format!("{e:#}")),
                    }
                }
            };

            let failed = result.status == Status::Failed;
            if failed {
                bad.insert(id.clone());
            }
            results.push(result);
            if failed && !self.cfg.keep_going {
                break;
            }
        }

        Ok(RunReport { results })
    }

    /// Returns Ok(Some(())) on run, Ok(None) on idempotent skip, Err on failure.
    fn run_task(&mut self, task: &Task) -> Result<Option<()>> {
        if let Some(idem) = &task.idempotent {
            if self.gate_skips(idem)? {
                return Ok(None);
            }
        }
        let total = task.steps.len();
        for (i, step) in task.steps.iter().enumerate() {
            // Localize any failure to the exact step: which action, which
            // position. The task id is already the report row, so it isn't
            // repeated here.
            self.run_step(step)
                .with_context(|| format!("step {}/{total} ({})", i + 1, step.describe()))?;
        }
        Ok(Some(()))
    }

    /// Evaluate the idempotency gate. Returns true if the task should be skipped.
    fn gate_skips(&mut self, idem: &Idempotent) -> Result<bool> {
        // Probe steps run first so the check can inspect a searched/filtered
        // page (type a username into a filter, click search, *then* look for the
        // row). A probe failure fails the whole gate — the existence question
        // couldn't be answered, so we can't safely skip.
        let total = idem.probe.len();
        for (i, step) in idem.probe.iter().enumerate() {
            self.run_step(step).with_context(|| {
                format!("idempotency probe step {}/{total} ({})", i + 1, step.describe())
            })?;
        }

        let Check { selector, exists } = &idem.check;
        // Poll until the presence matches expectation or the wait budget
        // expires — same implicit-wait model as `assert`, so a probe's async
        // filter/render settles before we decide. The one cost: the "create"
        // path (`exists: true`, resource genuinely absent) waits the full
        // budget before falling through to run the steps.
        let deadline = Instant::now() + self.cfg.wait_timeout;
        let actual = loop {
            let a = self.browser.exists(selector)?;
            if a == *exists || Instant::now() >= deadline {
                break a;
            }
            std::thread::sleep(POLL_INTERVAL);
        };
        let satisfied = actual == *exists; // predicate holds -> "already exists"
        if !satisfied {
            return Ok(false); // resource not in expected state -> do the work
        }
        match idem.on_exists {
            OnExists::Skip => Ok(true),
            OnExists::Continue => Ok(false),
            OnExists::Fail => Err(HtError::Assertion(format!(
                "idempotency gate on_exists=fail: `{selector}` already satisfied"
            ))
            .into()),
        }
    }

    /// Poll `browser.exists(sel)` until it is true or the wait budget expires.
    /// Best-effort: on timeout we return and let the caller's action surface the
    /// real error (e.g. click's "matched no elements"), so the message stays
    /// specific to the operation that needed the element.
    fn wait_present(&mut self, sel: &crate::selector::Selector) -> Result<()> {
        let deadline = Instant::now() + self.cfg.wait_timeout;
        loop {
            if self.browser.exists(sel)? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn run_step(&mut self, step: &Step) -> Result<()> {
        match step {
            Step::Goto(url) => self.browser.goto(url),
            Step::Click(sel) => {
                self.wait_present(sel)?;
                self.browser.click(sel)
            }
            Step::Fill(f) => {
                self.wait_present(&f.selector)?;
                self.browser.fill(&f.selector, &f.value)
            }
            Step::Upload(u) => self.run_upload(u),
            Step::Wait(ms) => {
                std::thread::sleep(Duration::from_millis(*ms));
                Ok(())
            }
            Step::WaitFor(w) => self.run_wait_for(w),
            Step::Screenshot(name) => {
                let path = self.cfg.screenshot_dir.join(name);
                self.browser.screenshot(&path)
            }
            Step::Assert(a) => self.run_assert(a),
        }
    }

    /// `upload`: point a file `<input>` at a local file. The path was already
    /// anchored at load time (`paths`); here it is made absolute for the driver
    /// — which needs a native absolute path — and checked to exist before
    /// handing it to the driver, so a bad path fails with a clear message
    /// rather than a cryptic WebDriver error. Uses the same `send_keys`
    /// mechanism as `fill`, which is how WebDriver sets file inputs.
    fn run_upload(&mut self, u: &UploadArgs) -> Result<()> {
        let abs = std::path::absolute(&u.path)
            .map_err(|e| anyhow::anyhow!("resolving upload path `{}`: {e}", u.path))?;
        if !abs.is_file() {
            anyhow::bail!("upload file not found: {}", abs.display());
        }
        self.wait_present(&u.selector)?;
        self.browser.fill(&u.selector, &abs.to_string_lossy())
    }

    /// Explicit `wait_for`: block until the selector reaches the wanted state
    /// or the (optionally overridden) timeout elapses. Unlike the implicit
    /// pre-action wait, a timeout here is a hard failure — that's the point.
    fn run_wait_for(&mut self, w: &WaitForArgs) -> Result<()> {
        let want_present = matches!(w.until, Until::Present);
        let budget = w
            .timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(self.cfg.wait_timeout);
        let deadline = Instant::now() + budget;
        loop {
            let present = self.browser.exists(&w.selector)?;
            if present == want_present {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(HtError::Assertion(format!(
                    "wait_for `{}` until {:?} timed out after {} ms (present={present})",
                    w.selector,
                    w.until,
                    budget.as_millis()
                ))
                .into());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn run_assert(&mut self, a: &AssertArgs) -> Result<()> {
        // Poll until the presence matches expectation (element appears for
        // exists=true, or disappears for exists=false) or the budget expires.
        let deadline = Instant::now() + self.cfg.wait_timeout;
        let present = loop {
            let p = self.browser.exists(&a.selector)?;
            if p == a.exists || Instant::now() >= deadline {
                break p;
            }
            std::thread::sleep(POLL_INTERVAL);
        };
        if present != a.exists {
            return Err(HtError::Assertion(format!(
                "expected `{}` exists={}, was {}",
                a.selector, a.exists, present
            ))
            .into());
        }
        if let Some(expected) = &a.text {
            // Likewise let the text settle (framework renders can lag layout).
            let deadline = Instant::now() + self.cfg.wait_timeout;
            let actual = loop {
                let t = self.browser.text(&a.selector)?;
                if &t == expected || Instant::now() >= deadline {
                    break t;
                }
                std::thread::sleep(POLL_INTERVAL);
            };
            if &actual != expected {
                return Err(HtError::Assertion(format!(
                    "text of `{}`: expected {expected:?}, was {actual:?}",
                    a.selector
                ))
                .into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::mock::MockBrowser;
    use crate::manifest::Check;
    use crate::selector::Selector;
    use std::path::Path;

    fn engine(browser: &mut MockBrowser) -> Engine<'_> {
        Engine::new(
            browser,
            EngineConfig {
                screenshot_dir: PathBuf::from("."),
                screenshot_on_fail: false,
                keep_going: false,
                wait_timeout: Duration::ZERO,
                max_run_time: None,
            },
        )
    }

    fn row(user: &str) -> Selector {
        Selector {
            css: Some(".grid .row".into()),
            contains: Some(user.into()),
            ..Default::default()
        }
    }

    /// The probe runs *before* the check: here the probe interacts with the very
    /// row the check looks for, flipping it into existence in the mock world. If
    /// the check ran first (or the probe were ignored) the gate would not skip.
    #[test]
    fn probe_runs_before_check() {
        // Seed the row absent -> "user not found" until the probe touches it.
        let mut b = MockBrowser::with_absent([".grid .row:contains(alice)".to_string()]);

        let idem = Idempotent {
            probe: vec![Step::Click(row("alice"))], // mock: click makes it present
            check: Check {
                selector: row("alice"),
                exists: true,
            },
            on_exists: OnExists::Skip,
        };
        assert!(engine(&mut b).gate_skips(&idem).unwrap());
    }

    /// A probe that searches for a *different* user leaves the wanted row absent,
    /// so the check fails to hold and the gate does the work (no skip).
    #[test]
    fn probe_that_does_not_find_the_row_does_not_skip() {
        let mut b = MockBrowser::with_absent([".grid .row:contains(bob)".to_string()]);

        let idem = Idempotent {
            probe: vec![Step::Click(Selector::css("#filter button"))], // unrelated
            check: Check {
                selector: row("bob"),
                exists: true,
            },
            on_exists: OnExists::Skip,
        };
        assert!(!engine(&mut b).gate_skips(&idem).unwrap());
    }

    /// The check polls (like `assert`): a row that renders a beat late — absent
    /// on the first read, present on the next — is still caught, so the gate
    /// skips rather than redundantly re-creating.
    #[test]
    fn check_polls_until_the_row_appears() {
        // Absent on read #1, present on read #2+ (models an async filter render).
        struct LateRow {
            reads: usize,
        }
        impl Browser for LateRow {
            fn goto(&mut self, _: &str) -> Result<()> {
                Ok(())
            }
            fn exists(&mut self, _: &Selector) -> Result<bool> {
                self.reads += 1;
                Ok(self.reads >= 2)
            }
            fn click(&mut self, _: &Selector) -> Result<()> {
                Ok(())
            }
            fn fill(&mut self, _: &Selector, _: &str) -> Result<()> {
                Ok(())
            }
            fn text(&mut self, _: &Selector) -> Result<String> {
                Ok(String::new())
            }
            fn screenshot(&mut self, _: &Path) -> Result<()> {
                Ok(())
            }
            fn name(&self) -> &'static str {
                "late-row"
            }
        }

        let mut b = LateRow { reads: 0 };
        // A budget long enough to poll at least once more after the first miss.
        let mut eng = Engine::new(
            &mut b,
            EngineConfig {
                screenshot_dir: PathBuf::from("."),
                screenshot_on_fail: false,
                keep_going: false,
                wait_timeout: Duration::from_secs(2),
                max_run_time: None,
            },
        );
        let idem = Idempotent {
            probe: vec![],
            check: Check {
                selector: row("alice"),
                exists: true,
            },
            on_exists: OnExists::Skip,
        };
        assert!(eng.gate_skips(&idem).unwrap());
    }
}
