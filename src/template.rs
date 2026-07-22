//! Templating — renders a manifest's `{{ VAR }}` placeholders before parsing.
//!
//! `build_context` layers variables low→high: process env → `.env` file →
//! manifest `vars`. `render` fills the whole document with minijinja in strict
//! mode, so a missing variable is a hard error rather than a silent blank.

use crate::error::Result;
use minijinja::Environment;
use std::collections::BTreeMap;
use std::path::Path;

/// Build the template context from three layers (low -> high precedence):
///   1. process environment
///   2. the .env file (if present)
///   3. manifest `vars`
pub fn build_context(
    env_file: Option<&Path>,
    manifest_vars: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut ctx: BTreeMap<String, String> = std::env::vars().collect();

    if let Some(path) = env_file {
        if path.exists() {
            for item in dotenvy::from_path_iter(path)? {
                let (k, v) = item?;
                ctx.insert(k, v);
            }
        }
    }

    for (k, v) in manifest_vars {
        ctx.insert(k.clone(), v.clone());
    }

    Ok(ctx)
}

/// Render the raw manifest text with `{{ VAR }}` substitution.
/// Rendering the whole document once means placeholders work anywhere.
pub fn render(raw: &str, ctx: &BTreeMap<String, String>) -> Result<String> {
    let mut env = Environment::new();
    // Fail loudly on a missing variable instead of silently emitting "".
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template("manifest", raw)?;
    let tmpl = env.get_template("manifest")?;
    let out = tmpl.render(ctx)?;
    Ok(out)
}
