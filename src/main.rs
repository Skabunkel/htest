//! htest — a WebDriver integration-test runner.
//!
//! Tests are YAML manifests (ansible/drill style). The CLI has two subcommands:
//! `graph` prints the computed run order without a browser, `run` executes it.
//! One invocation may load several manifests; their tasks merge into a single
//! dependency graph (see `plan`/`graph`), run in topological order by `engine`
//! against a `browser` backend (mock or WebDriver).
//!
//! Pipeline: load+template each file (`template`) → parse (`manifest`) →
//! namespace + resolve cross-file deps (`plan`) → build DAG (`graph`) → run
//! (`engine`). The browser session is always closed before the process exits —
//! see the note on `main` below.
//!
//! A `playbook` collects several manifests with run-wide settings and
//! file-level ordering; it can also spawn/kill the WebDriver server itself
//! (`driver`).

mod browser;
mod driver;
mod engine;
mod error;
mod graph;
mod loops;
mod manifest;
mod paths;
mod plan;
mod playbook;
mod selector;
mod template;

use browser::{BackendOpts, BrowserKind, Driver};
use clap::{Parser, Subcommand};
use driver::ManagedDriver;
use engine::{Engine, EngineConfig};
use error::Result;
use graph::RunGraph;
use manifest::{Manifest, ManifestHead};
use plan::LoadedFile;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "htest",
    about = "Integration test runner driving browsers over WebDriver via YAML manifests",
    version
)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the run graph and print the execution order (no browser).
    Graph {
        /// One or more manifest files.
        #[arg(required = true, num_args = 1..)]
        manifests: Vec<PathBuf>,
        /// Override the .env file for all manifests.
        #[arg(long)]
        env: Option<PathBuf>,
    },
    /// Run one or more manifests.
    Run {
        /// One or more manifest files. Tasks are merged into one run graph.
        #[arg(required = true, num_args = 1..)]
        manifests: Vec<PathBuf>,
        /// Browser backend: mock | webdriver.
        #[arg(long, default_value = "mock")]
        driver: Driver,
        /// WebDriver server URL (geckodriver/chromedriver).
        #[arg(long, default_value = "http://localhost:4444")]
        webdriver_url: String,
        /// Target browser (selects the WebDriver capability key).
        #[arg(long, default_value = "firefox")]
        browser: BrowserKind,
        /// Launch the browser headless (WebDriver only).
        #[arg(long)]
        headless: bool,
        /// Window size as WxH, e.g. 1280x800 (WebDriver only).
        #[arg(long)]
        window: Option<String>,
        /// Extra browser CLI arg (repeatable), passed through to the browser.
        #[arg(long = "browser-arg", allow_hyphen_values = true)]
        browser_arg: Vec<String>,
        /// Override the .env file for all manifests.
        #[arg(long)]
        env: Option<PathBuf>,
        /// Directory for screenshots.
        #[arg(long, default_value = "screenshots")]
        screenshots: PathBuf,
        /// Keep running after a task fails.
        #[arg(long)]
        keep_going: bool,
        /// Capture a screenshot automatically on task failure.
        #[arg(long)]
        shot_on_fail: bool,
        /// Max ms to wait for an assertion/element before giving up (implicit wait).
        #[arg(long, default_value_t = 5000)]
        timeout: u64,
    },
    /// Run a playbook: several manifests with shared settings + file-level order.
    Playbook {
        /// The playbook file.
        playbook: PathBuf,
        // --- Overrides. Any set here win over the playbook's `settings:`. ---
        /// Override the driver backend.
        #[arg(long)]
        driver: Option<Driver>,
        /// Override the WebDriver server URL.
        #[arg(long)]
        webdriver_url: Option<String>,
        /// Override the target browser.
        #[arg(long)]
        browser: Option<BrowserKind>,
        /// Force headless on.
        #[arg(long)]
        headless: bool,
        /// Force htest to spawn & kill the WebDriver server.
        #[arg(long)]
        manage_driver: bool,
        /// Force keep-going on.
        #[arg(long)]
        keep_going: bool,
        /// Force screenshot-on-fail on.
        #[arg(long)]
        shot_on_fail: bool,
        /// Override the .env file for all suites.
        #[arg(long)]
        env: Option<PathBuf>,
        /// Override the implicit-wait timeout (ms).
        #[arg(long)]
        timeout: Option<u64>,
        /// Override the global run cap (seconds).
        #[arg(long)]
        max_run_time: Option<u64>,
    },
}

/// Build the override `Settings` from playbook-subcommand CLI flags. Bare bool
/// flags can only turn a setting *on* (there's no `--no-headless`), so `false`
/// means "not overridden" and leaves the playbook value in place.
#[allow(clippy::too_many_arguments)]
fn cli_overrides(
    driver: Option<Driver>,
    webdriver_url: Option<String>,
    browser: Option<BrowserKind>,
    headless: bool,
    manage_driver: bool,
    keep_going: bool,
    shot_on_fail: bool,
    env: Option<PathBuf>,
    timeout: Option<u64>,
    max_run_time: Option<u64>,
) -> playbook::Settings {
    playbook::Settings {
        driver: driver.map(|d| match d {
            Driver::Mock => "mock".to_string(),
            Driver::WebDriver => "webdriver".to_string(),
        }),
        webdriver_url,
        browser: browser.map(|b| match b {
            BrowserKind::Firefox => "firefox".to_string(),
            BrowserKind::Chrome => "chrome".to_string(),
        }),
        headless: headless.then_some(true),
        window: None,
        browser_args: None,
        env,
        screenshots: None,
        keep_going: keep_going.then_some(true),
        shot_on_fail: shot_on_fail.then_some(true),
        timeout,
        manage_driver: manage_driver.then_some(true),
        driver_path: None,
        driver_port: None,
        max_run_time,
    }
}

fn main() {
    // NOTE: never call `std::process::exit` while a browser backend is still in
    // scope — it skips destructors, so `Drop for WebDriverBrowser` (which closes
    // the WebDriver session) never runs and the next run fails with "Session is
    // already started". `real_main` returns the failure flag *after* the backend
    // has dropped; we exit here, once every destructor has run.
    match real_main() {
        Ok(failed) => {
            if failed {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    }
}

/// Returns `Ok(true)` if any task failed (caller maps that to a non-zero exit).
fn real_main() -> Result<bool> {
    match Cli::parse().cmd {
        Command::Graph { manifests, env } => {
            let files = load_all(&manifests, env.as_deref())?;
            let planned = plan::assemble(&files)?;
            let rg = RunGraph::build(&planned)?;
            print_graph(&rg);
            Ok(false)
        }
        Command::Run {
            manifests,
            driver,
            webdriver_url,
            browser,
            headless,
            window,
            browser_arg,
            env,
            screenshots,
            keep_going,
            shot_on_fail,
            timeout,
        } => {
            let files = load_all(&manifests, env.as_deref())?;
            let planned = plan::assemble(&files)?;
            let rg = RunGraph::build(&planned)?;
            let order = rg.run_order();

            let window = window.as_deref().map(parse_window).transpose()?;
            let opts = BackendOpts {
                webdriver_url: &webdriver_url,
                browser,
                headless,
                window,
                browser_args: browser_arg,
            };
            let mut b = browser::create(driver, &opts)?;
            println!("driver: {}  tasks: {}", b.name(), order.len());

            let cfg = EngineConfig {
                screenshot_dir: screenshots,
                screenshot_on_fail: shot_on_fail,
                keep_going,
                wait_timeout: std::time::Duration::from_millis(timeout),
                max_run_time: None,
            };
            let failed = {
                let mut eng = Engine::new(b.as_mut(), cfg);
                let report = eng.run(&planned, &order)?;
                print_report(&report);
                report.failed()
            };
            // `b` drops at the end of this arm, closing the WebDriver session,
            // *before* main() decides whether to exit non-zero.
            Ok(failed)
        }
        Command::Playbook {
            playbook: pb_path,
            driver,
            webdriver_url,
            browser,
            headless,
            manage_driver,
            keep_going,
            shot_on_fail,
            env,
            timeout,
            max_run_time,
        } => {
            let pb = playbook::load(&pb_path)?;
            let overrides = cli_overrides(
                driver,
                webdriver_url,
                browser,
                headless,
                manage_driver,
                keep_going,
                shot_on_fail,
                env,
                timeout,
                max_run_time,
            );
            let settings = overrides.overlay(pb.settings).resolve()?;

            // Suite paths are relative to the playbook's own directory.
            let base_dir = pb_path.parent().map(Path::to_path_buf).unwrap_or_default();
            let resolve_path = |p: &Path| -> PathBuf {
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    base_dir.join(p)
                }
            };

            // Load every suite, recording file -> namespace so we can compile
            // file-level `needs` into task-level prerequisites.
            let mut files: Vec<LoadedFile> = Vec::new();
            let mut ns_by_key: std::collections::HashMap<PathBuf, String> =
                std::collections::HashMap::new();
            let mut seen_ns = std::collections::HashSet::new();
            for suite in &pb.suites {
                let path = resolve_path(&suite.file);
                let key = normalize(&path);
                let file = load_manifest(&path, settings.env.as_deref())?;
                if !seen_ns.insert(file.namespace.clone()) {
                    anyhow::bail!(
                        "duplicate suite namespace `{}` (set a distinct top-level `id:`)",
                        file.namespace
                    );
                }
                ns_by_key.insert(key, file.namespace.clone());
                files.push(file);
            }

            // Translate suite `needs` (by file) into (namespace, [namespace]).
            let mut file_needs: Vec<(String, Vec<String>)> = Vec::new();
            for suite in &pb.suites {
                if suite.needs.is_empty() {
                    continue;
                }
                let dep_ns = ns_by_key[&normalize(&resolve_path(&suite.file))].clone();
                let mut on = Vec::new();
                for need in &suite.needs {
                    let key = normalize(&resolve_path(need));
                    let ns = ns_by_key.get(&key).ok_or_else(|| {
                        anyhow::anyhow!(
                            "suite `{}` needs `{}`, which is not listed in the playbook",
                            suite.file.display(),
                            need.display()
                        )
                    })?;
                    on.push(ns.clone());
                }
                file_needs.push((dep_ns, on));
            }

            let mut planned = plan::assemble(&files)?;
            plan::add_file_dependencies(&mut planned, &file_needs)?;
            let rg = RunGraph::build(&planned)?;
            let order = rg.run_order();

            // Optionally own the driver lifecycle. `_driver` must outlive `b`
            // (declared first → dropped last) so the session closes before the
            // driver process is killed.
            let _driver: Option<ManagedDriver> = if settings.manage_driver {
                let d = ManagedDriver::start(
                    settings.browser,
                    settings.driver_path.as_deref(),
                    settings.driver_port,
                )?;
                println!("managed driver: {} ({})", settings.browser_name(), d.url);
                Some(d)
            } else {
                None
            };
            let url = _driver
                .as_ref()
                .map(|d| d.url.clone())
                .unwrap_or_else(|| settings.webdriver_url.clone());

            let opts = BackendOpts {
                webdriver_url: &url,
                browser: settings.browser,
                headless: settings.headless,
                window: settings.window,
                browser_args: settings.browser_args.clone(),
            };
            let mut b = browser::create(settings.driver, &opts)?;
            println!("driver: {}  suites: {}  tasks: {}", b.name(), files.len(), order.len());

            let cfg = EngineConfig {
                screenshot_dir: settings.screenshots.clone(),
                screenshot_on_fail: settings.shot_on_fail,
                keep_going: settings.keep_going,
                wait_timeout: std::time::Duration::from_millis(settings.timeout_ms),
                max_run_time: settings.max_run_time,
            };
            let failed = {
                let mut eng = Engine::new(b.as_mut(), cfg);
                let report = eng.run(&planned, &order)?;
                print_report(&report);
                report.failed()
            };
            drop(b); // close the session before `_driver` kills the server.
            Ok(failed)
        }
    }
}

/// Normalize a path for map keys: canonicalize if it exists, else use as-is.
fn normalize(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Load, template, and parse every manifest, tagging each with its namespace.
fn load_all(paths: &[PathBuf], env_override: Option<&Path>) -> Result<Vec<LoadedFile>> {
    let mut out = Vec::with_capacity(paths.len());
    let mut seen_ns = std::collections::HashSet::new();
    for p in paths {
        let file = load_manifest(p, env_override)?;
        if !seen_ns.insert(file.namespace.clone()) {
            anyhow::bail!(
                "duplicate manifest namespace `{}` (set a distinct top-level `id:`)",
                file.namespace
            );
        }
        out.push(file);
    }
    Ok(out)
}

/// Load one manifest. Namespace = manifest `id:` or the file stem.
fn load_manifest(path: &Path, env_override: Option<&Path>) -> Result<LoadedFile> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;

    // 1. Pre-parse (no templating) to discover id/env/vars.
    let head: ManifestHead = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("parsing head of {}: {e}", path.display()))?;

    // Relative locations inside the manifest are anchored at the CWD, falling
    // back to this directory, so a checkout runs the same on any machine.
    // See `paths`.
    let base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();

    // 2. Resolve the .env path: CLI override > manifest `env:` > ./.env.
    let env_path = env_override
        .map(PathBuf::from)
        .or_else(|| {
            head.env
                .as_deref()
                .map(|e| paths::resolve_path(e, &base_dir).unwrap_or_else(|| PathBuf::from(e)))
        })
        .or_else(|| {
            let d = Path::new(".env");
            d.exists().then(|| d.to_path_buf())
        });

    // 3. Build the template context.
    let ctx = template::build_context(env_path.as_deref(), &head.vars)?;

    // 4. Render and parse. A manifest with `loop:` needs a different context per
    //    iteration, so it is rendered task-by-task and expanded into plain
    //    tasks; anything else is rendered as one document. Then absolutize the
    //    file locations the tasks refer to.
    let (mut manifest, loop_groups): (Manifest, loops::LoopGroups) = if loops::present(&raw) {
        loops::render_document(&raw, &ctx)
            .map_err(|e| anyhow::anyhow!("expanding {}: {e:#}", path.display()))?
    } else {
        let rendered = template::render(&raw, &ctx)?;
        let m = serde_yaml::from_str(&rendered)
            .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
        (m, loops::LoopGroups::new())
    };
    paths::absolutize(&mut manifest, &base_dir);

    // 5. Namespace: explicit id, else file stem.
    let namespace = head.id.clone().unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "default".into())
    });

    Ok(LoadedFile {
        namespace,
        manifest,
        loop_groups,
    })
}

/// Parse a `WxH` window spec like `1280x800`.
fn parse_window(s: &str) -> Result<(u32, u32)> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| anyhow::anyhow!("window must be WxH, e.g. 1280x800 (got `{s}`)"))?;
    Ok((w.trim().parse()?, h.trim().parse()?))
}

fn print_graph(rg: &RunGraph) {
    let layers = rg.layers();
    println!("run graph ({} layers):", layers.len());
    for (i, layer) in layers.iter().enumerate() {
        println!("  layer {i} (parallel-safe): {}", layer.join(", "));
    }
}

fn print_report(report: &engine::RunReport) {
    use engine::Status::*;
    println!("\nresults:");
    for r in &report.results {
        let mark = match r.status {
            Passed => "PASS",
            Skipped => "SKIP",
            Blocked => "BLOCK",
            Failed => "FAIL",
        };
        let ms = r.duration.as_millis();
        print!("  [{mark}] {} ({ms} ms)", r.name);
        if let Some(d) = &r.detail {
            print!(" — {d}");
        }
        println!();
    }
    let (p, s, b, f) = report.counts();
    println!("\n{p} passed, {s} skipped, {b} blocked, {f} failed");
}
