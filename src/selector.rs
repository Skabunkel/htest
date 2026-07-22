//! Selectors. A selector is either a plain CSS string (passed straight to the
//! driver, so all CSS combinators — descendant `.a .b`, child `.a > .b`,
//! sibling, `:nth-child`, etc. — work as-is) OR a structured, hierarchical
//! form that adds what CSS cannot express: text matching and explicit scoping.
//!
//! YAML:
//! ```yaml
//! click: "#submit"                     # plain CSS (unchanged)
//!
//! click:                               # structured
//!   css: ".tables"                     # match at this level (default: any)
//!   contains: "Alice"                  # element text must contain this
//!   nth: 0                             # pick the Nth match (default: first)
//!   find:                              # descend INTO the match (recursive)
//!     css: "button"
//!     contains: "Delete"
//! ```
//! The example above means: "the `button` containing 'Delete' inside the
//! `.tables` element whose text contains 'Alice'".

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selector {
    /// CSS matched at this level. `None` means "any element" (`*`).
    pub css: Option<String>,
    /// Keep only elements whose text contains this substring.
    pub contains: Option<String>,
    /// Keep only elements whose trimmed text equals this exactly.
    pub text: Option<String>,
    /// Pick the Nth (0-based) match after filtering.
    pub nth: Option<usize>,
    /// Descend into each match and resolve this selector within it.
    pub find: Option<Box<Selector>>,
}

impl Selector {
    pub fn css<S: Into<String>>(css: S) -> Self {
        Selector {
            css: Some(css.into()),
            ..Default::default()
        }
    }

    /// True if this is a bare CSS selector with no extra semantics.
    fn is_plain_css(&self) -> bool {
        self.contains.is_none()
            && self.text.is_none()
            && self.nth.is_none()
            && self.find.is_none()
    }

    /// The CSS to match at this level (`*` if unspecified).
    pub fn css_or_any(&self) -> &str {
        self.css.as_deref().unwrap_or("*")
    }

    /// A stable, human-readable key. For a bare CSS selector this is exactly
    /// the CSS string (so the mock backend and `HTEST_MOCK_ABSENT` keep
    /// working unchanged).
    pub fn descriptor(&self) -> String {
        if self.is_plain_css() {
            return self.css_or_any().to_string();
        }
        let mut s = self.css_or_any().to_string();
        if let Some(c) = &self.contains {
            s.push_str(&format!(":contains({c})"));
        }
        if let Some(t) = &self.text {
            s.push_str(&format!(":text({t})"));
        }
        if let Some(n) = self.nth {
            s.push_str(&format!(":nth({n})"));
        }
        if let Some(f) = &self.find {
            s.push_str(" >> ");
            s.push_str(&f.descriptor());
        }
        s
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.descriptor())
    }
}

/// Deserialize from either a scalar string or a map.
impl<'de> Deserialize<'de> for Selector {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SelVisitor;

        // Mirrors the map form; `deny_unknown_fields` catches typos.
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            #[serde(default)]
            css: Option<String>,
            #[serde(default)]
            contains: Option<String>,
            #[serde(default)]
            text: Option<String>,
            #[serde(default)]
            nth: Option<usize>,
            #[serde(default)]
            find: Option<Box<Selector>>,
        }

        impl<'de> Visitor<'de> for SelVisitor {
            type Value = Selector;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a CSS string or a selector map")
            }

            fn visit_str<E>(self, v: &str) -> Result<Selector, E>
            where
                E: serde::de::Error,
            {
                Ok(Selector::css(v))
            }

            fn visit_map<A>(self, map: A) -> Result<Selector, A::Error>
            where
                A: MapAccess<'de>,
            {
                let f = Fields::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(Selector {
                    css: f.css,
                    contains: f.contains,
                    text: f.text,
                    nth: f.nth,
                    find: f.find,
                })
            }
        }

        de.deserialize_any(SelVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_string_is_plain_css() {
        let s: Selector = serde_yaml::from_str("\".tables button\"").unwrap();
        assert_eq!(s, Selector::css(".tables button"));
        assert_eq!(s.descriptor(), ".tables button");
    }

    #[test]
    fn nested_structured_selector() {
        let yaml = "css: .tables\ncontains: Alice\nfind:\n  css: button\n  contains: Delete\n";
        let s: Selector = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(s.css.as_deref(), Some(".tables"));
        assert_eq!(s.contains.as_deref(), Some("Alice"));
        let child = s.find.unwrap();
        assert_eq!(child.css.as_deref(), Some("button"));
        assert_eq!(child.contains.as_deref(), Some("Delete"));
    }

    #[test]
    fn unknown_field_rejected() {
        assert!(serde_yaml::from_str::<Selector>("csss: x\n").is_err());
    }
}
