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
                        detail: Some(e.to_string()),
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
            if self.gate_skips(task, idem)? {
                return Ok(None);
            }
        }
        for step in &task.steps {
            self.run_step(task, step)?;
        }
        Ok(Some(()))
    }

    /// Evaluate the idempotency gate. Returns true if the task should be skipped.
    fn gate_skips(&mut self, task: &Task, idem: &Idempotent) -> Result<bool> {
        let Check { selector, exists } = &idem.check;
        let actual = self.browser.exists(selector)?;
        let satisfied = actual == *exists; // predicate holds -> "already exists"
        if !satisfied {
            return Ok(false); // resource not in expected state -> do the work
        }
        match idem.on_exists {
            OnExists::Skip => Ok(true),
            OnExists::Continue => Ok(false),
            OnExists::Fail => Err(HtError::Assertion {
                task: task.name.clone(),
                detail: format!(
                    "idempotency gate on_exists=fail: `{selector}` already satisfied"
                ),
            }
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

    fn run_step(&mut self, task: &Task, step: &Step) -> Result<()> {
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
            Step::WaitFor(w) => self.run_wait_for(task, w),
            Step::Screenshot(name) => {
                let path = self.cfg.screenshot_dir.join(name);
                self.browser.screenshot(&path)
            }
            Step::Assert(a) => self.run_assert(task, a),
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
    fn run_wait_for(&mut self, task: &Task, w: &WaitForArgs) -> Result<()> {
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
                return Err(HtError::Assertion {
                    task: task.name.clone(),
                    detail: format!(
                        "wait_for `{}` until {:?} timed out after {} ms (present={present})",
                        w.selector,
                        w.until,
                        budget.as_millis()
                    ),
                }
                .into());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn run_assert(&mut self, task: &Task, a: &AssertArgs) -> Result<()> {
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
            return Err(HtError::Assertion {
                task: task.name.clone(),
                detail: format!(
                    "expected `{}` exists={}, was {}",
                    a.selector, a.exists, present
                ),
            }
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
                return Err(HtError::Assertion {
                    task: task.name.clone(),
                    detail: format!(
                        "text of `{}`: expected {expected:?}, was {actual:?}",
                        a.selector
                    ),
                }
                .into());
            }
        }
        Ok(())
    }
}
