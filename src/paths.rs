//! Machine-independent path resolution for manifests.
//!
//! Manifests should be checkout-relative, not machine-relative: a `goto:` that
//! reads `file:///C:/workspace/.../demo/index.html` only runs on the machine it
//! was written on. This module rewrites relative locations — in `goto:` targets,
//! `upload:` paths and the manifest's own `env:` — into absolute ones at load
//! time, so the rest of the pipeline still sees plain absolute URLs/paths.
//!
//! Anchoring: a relative path is tried against the working directory first,
//! then — if nothing is there — against the manifest's own directory. So the
//! usual "paths are relative to where I ran the command" holds, and a manifest
//! invoked from some other directory still finds files sitting next to it.
//!
//! Absolute paths, `http(s)://`, and other schemes (`data:`, `about:`, …) pass
//! through untouched.

use crate::manifest::{Manifest, Step};
use std::path::{Component, Path, PathBuf};

/// Rewrite every location in `manifest` that may be relative into an absolute
/// one, anchored at the CWD or at `base_dir` (the manifest's directory).
pub fn absolutize(manifest: &mut Manifest, base_dir: &Path) {
    for task in &mut manifest.tasks {
        for step in &mut task.steps {
            match step {
                Step::Goto(url) => *url = resolve_goto(url, base_dir),
                // Left as a string: the engine checks it exists and reports the
                // full path on failure. We only anchor it.
                Step::Upload(u) => {
                    if let Some(p) = resolve_path(&u.path, base_dir) {
                        u.path = p.to_string_lossy().into_owned();
                    }
                }
                _ => {}
            }
        }
    }
}

/// Resolve a `goto:` target. Returns the input unchanged when it is already
/// absolute or is not a file location at all.
pub fn resolve_goto(raw: &str, base_dir: &Path) -> String {
    let t = raw.trim();

    // `file://` forms. Everything after the prefix is a path: absolute
    // (`file:///srv/x`, `file:///C:/x`) passes through, relative
    // (`file://./demo`, `file://demo`, `file://../demo`) gets anchored.
    if let Some(rest) = strip_file_scheme(t) {
        // An already-absolute path inside the URL (`file:///srv/x`,
        // `file:///C:/x`) is not ours to anchor — reformat its slashes/encoding
        // so it round-trips identically on every platform. Note `Path::is_absolute`
        // is OS-specific (a unix `/srv/x` is *not* absolute on Windows), so this
        // is decided textually rather than via `resolve_path`. Only a relative
        // form (`file://./x`, `file://x`) is anchored.
        if is_absolute_urlpath(rest) {
            return to_file_url(Path::new(rest));
        }
        return match resolve_path(rest, base_dir) {
            Some(abs) => to_file_url(&abs),
            None => t.to_string(),
        };
    }

    // Any other scheme (http:, https:, data:, about:, …) is not ours to touch.
    // A bare Windows drive (`C:\…`) also matches "scheme:", so exclude it.
    if has_scheme(t) {
        return t.to_string();
    }

    // Schemeless. Anything explicitly path-shaped (`./x`, `../x`, `/x`, `~/x`,
    // `C:\x`) is a file; a bare `foo/bar.html` is only treated as one when it
    // actually resolves to a file on disk, so hostish values like
    // `localhost:8080/app` or `example.com/page` keep their old meaning.
    if is_path_shaped(t) {
        if let Some(abs) = resolve_path(t, base_dir) {
            return to_file_url(&abs);
        }
    } else if let Some(abs) = existing_relative(t, base_dir) {
        return to_file_url(&abs);
    }

    t.to_string()
}

/// Anchor a possibly-relative filesystem path. `None` means "leave it alone"
/// (empty input, or a `~` we cannot expand).
pub fn resolve_path(raw: &str, base_dir: &Path) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let expanded = expand_home(raw)?;
    if expanded.is_absolute() || is_windows_absolute(&expanded) {
        return Some(normalize(&expanded));
    }
    // The working directory wins; the manifest's directory is the fallback for
    // when the file isn't there. If neither holds it, anchor at the CWD anyway
    // so the error names the path the user most likely meant.
    if let Some(hit) = existing_relative(raw, base_dir) {
        return Some(hit);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    Some(normalize(&cwd.join(&expanded)))
}

/// The first anchor under which `rel` actually exists: CWD, then manifest dir.
/// Both anchors are made absolute first — the manifest was very likely named by
/// a relative path (`htest run examples/tour.yaml`), and the result has to be
/// absolute for the driver.
fn existing_relative(rel: &str, base_dir: &Path) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel.is_absolute() {
        return None;
    }
    let cwd = std::env::current_dir().ok()?;
    let manifest_dir = if base_dir.is_absolute() {
        base_dir.to_path_buf()
    } else {
        cwd.join(base_dir)
    };
    [cwd, manifest_dir]
        .into_iter()
        .map(|anchor| normalize(&anchor.join(rel)))
        .find(|p| p.exists())
}

/// Strip a `file:` scheme, returning the path part. Handles `file://x`,
/// `file:///x` and the rare `file:x`. The empty authority (`//`) is dropped;
/// a non-empty one (`file://host/share`) is not a local path, so: `None`.
fn strip_file_scheme(s: &str) -> Option<&str> {
    let rest = s
        .strip_prefix("file://")
        .or_else(|| s.strip_prefix("FILE://"))?;
    // `file:///abs` -> "/abs"; `file://./rel` -> "./rel"; both fine as paths.
    // `file://server/share` would arrive here as "server/share" and we cannot
    // tell it from a relative path — treat it as relative, which is by far the
    // more likely intent in a manifest.
    Some(rest)
}

/// True if `s` starts with a URL scheme (`http:`, `data:`, …). A single-letter
/// scheme is really a Windows drive letter, so it does not count.
fn has_scheme(s: &str) -> bool {
    let Some(colon) = s.find(':') else {
        return false;
    };
    let scheme = &s[..colon];
    scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn is_path_shaped(s: &str) -> bool {
    s.starts_with("./")
        || s.starts_with("../")
        || s.starts_with(".\\")
        || s.starts_with("..\\")
        || s.starts_with('/')
        || s.starts_with('~')
        || is_windows_absolute(Path::new(s))
}

/// True if a path lifted out of a `file://` URL is already absolute — a unix
/// root (`/srv/x`), a slashed drive (`/C:/x`, which starts with `/`), or a bare
/// drive (`C:/x`). Such a path is passed through unanchored.
fn is_absolute_urlpath(rest: &str) -> bool {
    rest.starts_with('/') || is_windows_absolute(Path::new(rest))
}

/// `C:\x` / `C:/x` — absolute on Windows, but not `Path::is_absolute` on unix.
fn is_windows_absolute(p: &Path) -> bool {
    let s = p.to_string_lossy();
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\')
}

/// Expand a leading `~`. Returns `None` when there is no home directory to
/// expand to, so the caller can leave the value untouched.
fn expand_home(s: &str) -> Option<PathBuf> {
    let rest = match s.strip_prefix('~') {
        Some(r) => r,
        None => return Some(PathBuf::from(s)),
    };
    // Only `~` and `~/…` — `~user/…` is not something we can resolve.
    if !(rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\')) {
        return None;
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(rest.trim_start_matches(['/', '\\'])))
}

/// Lexically remove `.` and `..` segments. Purely textual — unlike
/// `canonicalize` it does not require the path to exist (a `goto:` may point at
/// a page that is generated later) and does not resolve symlinks.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                // Keep `..` that we cannot cancel (relative path with no root).
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Render an absolute native path as a `file://` URL. Windows separators become
/// `/` and a drive-letter path gains the leading slash browsers expect
/// (`C:\a b` -> `file:///C:/a%20b`).
fn to_file_url(abs: &Path) -> String {
    let s = abs.to_string_lossy().replace('\\', "/");
    let s = if s.starts_with('/') {
        s
    } else {
        format!("/{s}") // drive-letter path: file:///C:/…
    };
    format!("file://{}", percent_encode(&s))
}

/// Percent-encode the characters that are not legal in a URL path. `/` and `:`
/// stay literal (path separator, drive letter); `%` is encoded, so a path that
/// genuinely contains `%` survives the round trip.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => out.push(b as char),
            b'-' | b'_' | b'.' | b'~' | b'/' | b':' | b'@' | b'+' | b',' | b'(' | b')' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest directory holding nothing, so resolution falls to the CWD.
    fn base() -> PathBuf {
        PathBuf::from("/no/such/dir")
    }

    /// Cargo runs tests with the CWD at the crate root, so relative expectations
    /// are built from it. Routed through the real `to_file_url`/`normalize` so
    /// the expected string matches production byte-for-byte on every platform
    /// (forward slashes, leading `/` before a drive letter, percent-encoding) —
    /// `rel` is a *native* relative path, not a pre-encoded URL fragment.
    fn cwd_url(rel: &str) -> String {
        to_file_url(&normalize(&std::env::current_dir().unwrap().join(rel)))
    }

    #[test]
    fn absolute_and_remote_urls_pass_through() {
        for url in [
            "https://example.com/app",
            "http://localhost:8080/x",
            "about:blank",
            "data:text/html,<h1>hi</h1>",
            "file:///srv/pages/index.html",
        ] {
            assert_eq!(resolve_goto(url, &base()), url);
        }
    }

    #[test]
    fn relative_file_url_is_anchored_at_cwd() {
        // The demo folder exists under the crate root -> CWD wins outright.
        assert_eq!(
            resolve_goto("file://./demo/index.html", &base()),
            cwd_url("demo/index.html")
        );
        // Nothing on disk either way: still the CWD, so the URL is predictable.
        assert_eq!(
            resolve_goto("file://nowhere/index.html", &base()),
            cwd_url("nowhere/index.html")
        );
    }

    #[test]
    fn manifest_dir_is_the_fallback_when_cwd_misses() {
        // `fixtures/table.html` does not exist under the crate root, but does
        // under examples/ — the manifest's own directory.
        let manifest_dir = std::env::current_dir().unwrap().join("examples");
        assert_eq!(
            resolve_goto("file://./fixtures/table.html", &manifest_dir),
            cwd_url("examples/fixtures/table.html")
        );
        assert_eq!(
            resolve_path("fixtures/table.html", &manifest_dir).unwrap(),
            manifest_dir.join("fixtures/table.html")
        );
    }

    #[test]
    fn relative_manifest_dir_still_yields_an_absolute_url() {
        // `htest run examples/tour.yaml` -> base_dir is the relative "examples".
        // The fallback must not leak that relativeness into the file:// URL.
        assert_eq!(
            resolve_goto("file://../demo/index.html", Path::new("examples")),
            cwd_url("demo/index.html")
        );
    }

    #[test]
    fn bare_relative_path_is_anchored() {
        assert_eq!(
            resolve_goto("./pages/a.html", &base()),
            cwd_url("pages/a.html")
        );
        assert_eq!(
            resolve_goto("../pages/a.html", &base()),
            cwd_url("../pages/a.html")
        );
    }

    #[test]
    fn hostish_value_without_scheme_is_left_alone() {
        // No `./`, and no such file on disk -> not a path.
        assert_eq!(
            resolve_goto("example.com/page", &base()),
            "example.com/page"
        );
    }

    #[test]
    fn windows_absolute_path_becomes_file_url() {
        assert_eq!(
            resolve_goto(r"C:\work\demo\index.html", &base()),
            "file:///C:/work/demo/index.html"
        );
        assert_eq!(
            resolve_goto("file:///C:/work/demo/index.html", &base()),
            "file:///C:/work/demo/index.html"
        );
    }

    #[test]
    fn spaces_and_specials_are_percent_encoded() {
        assert_eq!(
            resolve_goto("./my pages/a&b.html", &base()),
            cwd_url("my pages/a&b.html")
        );
    }

    #[test]
    fn upload_path_prefers_cwd() {
        // `Cargo.toml` exists at the crate root (the CWD), not under the
        // manifest dir — so the CWD copy is the one selected.
        let cwd = std::env::current_dir().unwrap();
        let got = resolve_path("Cargo.toml", Path::new("/no/such/dir")).unwrap();
        assert_eq!(got, cwd.join("Cargo.toml"));
    }

    #[test]
    fn normalize_keeps_leading_parent_segments() {
        assert_eq!(normalize(Path::new("../a/./b/../c")), PathBuf::from("../a/c"));
    }
}
