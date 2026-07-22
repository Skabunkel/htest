//! Manifest schema — the YAML a test file deserializes into.
//!
//! `Manifest` is the whole document; `Task` is one unit of work with optional
//! `needs` (prerequisites) and an `idempotent` gate. A task's `steps` are
//! `Step`s. Steps and `wait_for` use ansible-style single-key maps
//! (`- goto: url`), which serde's default enum representation can't express, so
//! both have hand-written `Deserialize` impls. `ManifestHead` is a cheap
//! pre-parse used to read `id`/`env`/`vars` before templating the document.

use crate::selector::Selector;
use serde::Deserialize;

/// Top-level test manifest (YAML). Ansible/drill style.
/// `env`/`vars` are consumed by `ManifestHead` during the pre-parse; they are
/// retained here so the full struct round-trips.
#[derive(Debug, Deserialize)]
pub struct Manifest {
    /// Namespace for cross-file references. Defaults to the file stem.
    #[serde(default)]
    #[allow(dead_code)]
    pub id: Option<String>,
    /// Optional path to a .env file to load before templating.
    #[serde(default)]
    #[allow(dead_code)]
    pub env: Option<String>,
    /// Inline variables. Highest precedence in the template context.
    #[serde(default)]
    #[allow(dead_code)]
    pub vars: std::collections::BTreeMap<String, String>,
    /// The tasks to run. Order here is irrelevant; `needs` drives execution.
    pub tasks: Vec<Task>,
}

/// Lightweight pre-parse used only to discover `env`/`vars` before templating.
/// Ignores `tasks` (which may contain `{{ }}` placeholders that are not yet
/// valid for their eventual use but are always valid YAML strings).
#[derive(Debug, Deserialize)]
pub struct ManifestHead {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub env: Option<String>,
    #[serde(default)]
    pub vars: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct Task {
    pub name: String,
    /// Names of tasks that must complete before this one.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Optional idempotency gate: check a predicate first, then decide.
    #[serde(default)]
    pub idempotent: Option<Idempotent>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
pub struct Idempotent {
    /// Predicate evaluated before any step runs.
    pub check: Check,
    /// What to do when the predicate is satisfied (resource already exists).
    #[serde(default)]
    pub on_exists: OnExists,
}

#[derive(Debug, Deserialize)]
pub struct Check {
    pub selector: Selector,
    /// Expected presence. `true` = element should exist for the check to pass.
    #[serde(default = "default_true")]
    pub exists: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnExists {
    /// Skip the task's steps entirely (default). Safe reruns.
    Skip,
    /// Run the steps anyway.
    Continue,
    /// Treat prior existence as an error.
    Fail,
}

impl Default for OnExists {
    fn default() -> Self {
        OnExists::Skip
    }
}

/// A single action. Each YAML list entry is a single-key map, e.g.
/// `- goto: "{{BASE_URL}}"` or `- fill: { selector: "#n", value: "x" }`.
#[derive(Debug)]
pub enum Step {
    Goto(String),
    Click(Selector),
    Fill(FillArgs),
    /// Set a file `<input>` to a file on disk (see `UploadArgs::path`).
    Upload(UploadArgs),
    Assert(AssertArgs),
    Screenshot(String),
    /// Milliseconds to sleep (unconditional, fixed pause).
    Wait(u64),
    /// Wait until a selector is present/absent (spinners, async content).
    WaitFor(WaitForArgs),
}

// Custom deserialize: serde_yaml renders externally-tagged enums as `!tags`,
// but we want an ansible-style single-key map (`- goto: url`). So parse a
// one-entry map manually and dispatch on the key.
impl<'de> Deserialize<'de> for Step {
    fn deserialize<D>(de: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let map: std::collections::BTreeMap<String, serde_yaml::Value> =
            Deserialize::deserialize(de)?;
        if map.len() != 1 {
            return Err(D::Error::custom(format!(
                "each step must be a single-key map, got {} keys",
                map.len()
            )));
        }
        let (key, val) = map.into_iter().next().unwrap();
        fn conv<T: serde::de::DeserializeOwned, E: serde::de::Error>(
            v: serde_yaml::Value,
        ) -> std::result::Result<T, E> {
            serde_yaml::from_value(v).map_err(E::custom)
        }
        Ok(match key.as_str() {
            "goto" => Step::Goto(conv::<_, D::Error>(val)?),
            "click" => Step::Click(conv::<_, D::Error>(val)?),
            "screenshot" => Step::Screenshot(conv::<_, D::Error>(val)?),
            "wait" => Step::Wait(conv::<_, D::Error>(val)?),
            "wait_for" => Step::WaitFor(conv::<_, D::Error>(val)?),
            "fill" => Step::Fill(conv::<_, D::Error>(val)?),
            "upload" => Step::Upload(conv::<_, D::Error>(val)?),
            "assert" => Step::Assert(conv::<_, D::Error>(val)?),
            other => {
                return Err(D::Error::custom(format!("unknown step `{other}`")))
            }
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct FillArgs {
    pub selector: Selector,
    pub value: String,
}

/// Arguments to an `upload` step. Accepts the map form
/// (`upload: { selector: "#f", path: "assets/a.png" }`). A relative `path` is
/// resolved against the working directory, then the manifest's own directory
/// (see `paths`); absolute paths pass through.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadArgs {
    pub selector: Selector,
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct AssertArgs {
    pub selector: Selector,
    #[serde(default = "default_true")]
    pub exists: bool,
    /// If set, the element's text must equal this value.
    #[serde(default)]
    pub text: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Which state to wait for.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Until {
    /// Wait until the selector resolves to at least one element.
    Present,
    /// Wait until the selector resolves to nothing (e.g. a spinner clearing).
    Absent,
}

impl Default for Until {
    fn default() -> Self {
        Until::Present
    }
}

/// Arguments to a `wait_for` step. Accepts a bare CSS string shorthand
/// (`wait_for: "#ready"` → wait until present) or the full map form
/// (`wait_for: { selector: <sel>, until: absent, timeout: 10000 }`).
#[derive(Debug)]
pub struct WaitForArgs {
    pub selector: Selector,
    pub until: Until,
    /// Per-step override of the implicit-wait budget, in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl<'de> Deserialize<'de> for WaitForArgs {
    fn deserialize<D>(de: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let v = serde_yaml::Value::deserialize(de)?;
        // Shorthand: a bare selector (string) waits until present.
        if v.is_string() {
            let selector: Selector = serde_yaml::from_value(v).map_err(D::Error::custom)?;
            return Ok(WaitForArgs {
                selector,
                until: Until::Present,
                timeout_ms: None,
            });
        }
        #[derive(Deserialize)]
        struct Raw {
            selector: Selector,
            #[serde(default)]
            until: Until,
            #[serde(default, rename = "timeout")]
            timeout_ms: Option<u64>,
        }
        let raw: Raw = serde_yaml::from_value(v).map_err(D::Error::custom)?;
        Ok(WaitForArgs {
            selector: raw.selector,
            until: raw.until,
            timeout_ms: raw.timeout_ms,
        })
    }
}
