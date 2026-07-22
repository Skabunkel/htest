//! `loop:` — repeat a task over a range or a list.
//!
//! A looped task is **expanded at load time** into ordinary tasks, one per
//! item, each templated with the loop variable. Nothing downstream (plan, graph,
//! engine) knows loops exist; they only ever see plain tasks.
//!
//! ```yaml
//! - name: create_user
//!   loop: { var: n, from: 1, to: 5 }      # -> create_user[1] … create_user[5]
//!   steps:
//!     - fill: { selector: "#name", value: "user{{ n }}" }
//! ```
//!
//! Expansion needs a *different* template context per iteration, so a manifest
//! that uses `loop:` is rendered task-by-task rather than as one document: the
//! raw YAML is parsed structurally first (placeholders are still just strings),
//! then each task node is rendered with the context its iteration needs. A
//! manifest without `loop:` takes the original whole-document path.

use crate::error::{HtError, Result};
use crate::manifest::{Manifest, Task};
use crate::template;
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;

/// Base task name -> the names of its expanded iterations, in order. Lets
/// `needs: [create_user]` fan out to every iteration (see `plan::assemble`).
pub type LoopGroups = BTreeMap<String, Vec<String>>;

/// Guard against a typo (`from: 1, to: 1000000`) turning into a run that never
/// finishes. Well above any plausible real loop.
const MAX_ITERATIONS: usize = 10_000;

/// Cheap check for whether a document uses `loop:` at all. Only such documents
/// take the structural path, so existing manifests keep their exact behaviour
/// (including document-level Jinja blocks, which are not valid YAML on their
/// own and so could not survive a structural pre-parse).
pub fn present(raw: &str) -> bool {
    raw.lines().any(|l| {
        let t = l.trim_start().trim_start_matches("- ").trim_start();
        t == "loop:" || t.starts_with("loop: ")
    })
}

/// Render a manifest that contains `loop:`, expanding every looped task.
pub fn render_document(raw: &str, ctx: &BTreeMap<String, String>) -> Result<(Manifest, LoopGroups)> {
    let doc: Value = serde_yaml::from_str(raw)?;
    let nodes = doc
        .get("tasks")
        .and_then(Value::as_sequence)
        .ok_or_else(|| HtError::Manifest("manifest has no `tasks:` list".into()))?;

    let mut tasks = Vec::new();
    let mut groups = LoopGroups::new();

    for node in nodes {
        let mut map = node
            .as_mapping()
            .cloned()
            .ok_or_else(|| HtError::Manifest("each task must be a map".into()))?;

        let Some(spec_node) = map.remove(Value::from("loop")) else {
            tasks.push(render_task(&Value::Mapping(map), ctx)?);
            continue;
        };

        // The spec itself may be templated (`to: "{{ USER_COUNT }}"`), so
        // render it before reading the bounds.
        let spec = LoopSpec::parse(&render_value(&spec_node, ctx)?)?;
        let (base, expanded) = expand(&map, &spec, ctx)?;
        if groups.contains_key(&base) {
            return Err(HtError::Manifest(format!(
                "two looped tasks are both named `{base}`"
            ))
            .into());
        }
        groups.insert(base, expanded.iter().map(|t| t.name.clone()).collect());
        tasks.extend(expanded);
    }

    // `id`/`env`/`vars` were already consumed from the raw head by the caller;
    // the parsed manifest only needs its tasks.
    Ok((
        Manifest {
            id: None,
            env: None,
            vars: BTreeMap::new(),
            tasks,
        },
        groups,
    ))
}

/// Expand one looped task. Returns the base name and one task per item, named
/// `base[item]`.
fn expand(
    task_node: &Mapping,
    spec: &LoopSpec,
    ctx: &BTreeMap<String, String>,
) -> Result<(String, Vec<Task>)> {
    if spec.items.len() > MAX_ITERATIONS {
        return Err(HtError::Manifest(format!(
            "loop over `{}` would produce {} tasks (limit {MAX_ITERATIONS})",
            spec.var,
            spec.items.len()
        ))
        .into());
    }

    let node = Value::Mapping(task_node.clone());
    let mut base: Option<String> = None;
    let mut out = Vec::with_capacity(spec.items.len());

    for (i, item) in spec.items.iter().enumerate() {
        let mut ictx = ctx.clone();
        ictx.insert(spec.var.clone(), item.clone());
        ictx.insert("loop_index".into(), i.to_string());
        ictx.insert("loop_index1".into(), (i + 1).to_string());

        let mut task = render_task(&node, &ictx)?;
        // Every iteration must agree on the base name: the `[item]` suffix is
        // what distinguishes them, so a name that varies per iteration would
        // make `needs:` ambiguous.
        match &base {
            None => base = Some(task.name.clone()),
            Some(b) if *b != task.name => {
                return Err(HtError::Manifest(format!(
                    "looped task `{b}` renders a different name (`{}`) on another \
                     iteration: `{}` must not appear in `name:` — htest appends \
                     `[item]` to each iteration itself",
                    task.name, spec.var
                ))
                .into())
            }
            Some(_) => {}
        }
        task.name = format!("{}[{item}]", task.name);
        out.push(task);
    }

    // A range that yields nothing (`from: 5, to: 1`) expands to no tasks; that
    // is legal — a run sized to zero by a variable should not be an error — but
    // the base name is still needed so `needs:` on it resolves to nothing
    // instead of "unknown task". With no item to bind, take it from `name:`.
    let base = match base {
        Some(b) => b,
        None => empty_loop_name(task_node, spec, ctx)?,
    };
    Ok((base, out))
}

/// The name of a loop that produced no iterations. Rendered with the plain
/// context: there is no item to bind, which is exactly why the loop variable is
/// not allowed in `name:`.
fn empty_loop_name(
    task_node: &Mapping,
    spec: &LoopSpec,
    ctx: &BTreeMap<String, String>,
) -> Result<String> {
    let name = task_node
        .get(Value::from("name"))
        .ok_or_else(|| bad("a task needs a `name:`"))?;
    let rendered = render_value(name, ctx).map_err(|_| {
        bad(&format!(
            "a looped task's `name:` must not use `{}` (the loop is empty, so \
             there is no item to render it with)",
            spec.var
        ))
    })?;
    scalar(&rendered)
}

/// Render one task node with the given context and parse it back into a `Task`.
fn render_task(node: &Value, ctx: &BTreeMap<String, String>) -> Result<Task> {
    let rendered = render_value(node, ctx)?;
    serde_yaml::from_value(rendered).map_err(|e| HtError::Manifest(format!("parsing task: {e}")).into())
}

/// Round-trip a YAML node through the templating engine: serialize, render,
/// re-parse. Quoting is preserved by the serializer, so a `{{ }}` that expands
/// to a plain string stays a string.
fn render_value(node: &Value, ctx: &BTreeMap<String, String>) -> Result<Value> {
    let text = serde_yaml::to_string(node)?;
    let rendered = template::render(&text, ctx)?;
    Ok(serde_yaml::from_str(&rendered)?)
}

/// A parsed `loop:` clause: the variable name plus the concrete items to
/// iterate, already rendered as strings.
#[derive(Debug, PartialEq, Eq)]
struct LoopSpec {
    var: String,
    items: Vec<String>,
}

impl LoopSpec {
    /// Accepted forms:
    /// * `loop: [alice, bob]` — bare list, variable defaults to `item`
    /// * `loop: { var: u, items: [alice, bob] }`
    /// * `loop: { var: n, from: 1, to: 5 }` — inclusive on both ends
    /// * `loop: { var: n, from: 0, to: 10, step: 2 }` — `step` may be negative
    fn parse(v: &Value) -> Result<Self> {
        if let Some(seq) = v.as_sequence() {
            return Ok(LoopSpec {
                var: "item".into(),
                items: seq.iter().map(scalar).collect::<Result<_>>()?,
            });
        }
        let map = v
            .as_mapping()
            .ok_or_else(|| bad("`loop:` must be a list or a map"))?;

        for k in map.keys() {
            let k = k.as_str().unwrap_or_default();
            if !matches!(k, "var" | "items" | "from" | "to" | "step") {
                return Err(bad(&format!("unknown `loop:` key `{k}`")).into());
            }
        }

        let var = match map.get(Value::from("var")) {
            Some(v) => scalar(v)?,
            None => "item".to_string(),
        };

        if let Some(items) = map.get(Value::from("items")) {
            let seq = items
                .as_sequence()
                .ok_or_else(|| bad("`loop: items:` must be a list"))?;
            return Ok(LoopSpec {
                var,
                items: seq.iter().map(scalar).collect::<Result<_>>()?,
            });
        }

        let (Some(from), Some(to)) = (map.get(Value::from("from")), map.get(Value::from("to")))
        else {
            return Err(bad(
                "`loop:` needs either `items:` or both `from:` and `to:` \
                 (e.g. `loop: { var: n, from: 1, to: 5 }`)",
            )
            .into());
        };
        let from = int(from, "from")?;
        let to = int(to, "to")?;
        let step = match map.get(Value::from("step")) {
            Some(s) => int(s, "step")?,
            None if to < from => -1, // a descending range needs no explicit step
            None => 1,
        };
        if step == 0 {
            return Err(bad("`loop: step:` must not be 0").into());
        }

        // Inclusive of both ends — `from: 1, to: 5` is five iterations, which is
        // what "user1-user5" means to everyone except a programmer.
        let mut items = Vec::new();
        let mut n = from;
        while (step > 0 && n <= to) || (step < 0 && n >= to) {
            items.push(n.to_string());
            if items.len() > MAX_ITERATIONS {
                break; // `expand` reports the limit with the task's own name.
            }
            n += step;
        }
        Ok(LoopSpec { var, items })
    }
}

fn bad(msg: &str) -> HtError {
    HtError::Manifest(msg.to_string())
}

/// A loop item as a string. Numbers keep their plain form (`1`, not `1.0`).
fn scalar(v: &Value) -> Result<String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        _ => Err(bad("loop items must be strings or numbers").into()),
    }
}

/// An integer bound, accepting the string form a template leaves behind
/// (`to: "{{ COUNT }}"` renders to `"5"`).
fn int(v: &Value, field: &str) -> Result<i64> {
    match v {
        Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| bad(&format!("`loop: {field}:` must be a whole number")).into()),
        Value::String(s) => s
            .trim()
            .parse()
            .map_err(|_| bad(&format!("`loop: {field}:` is not a number: `{s}`")).into()),
        _ => Err(bad(&format!("`loop: {field}:` must be a number")).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn spec(yaml: &str) -> LoopSpec {
        LoopSpec::parse(&serde_yaml::from_str(yaml).unwrap()).unwrap()
    }

    #[test]
    fn inclusive_range() {
        assert_eq!(
            spec("{ var: n, from: 1, to: 5 }"),
            LoopSpec {
                var: "n".into(),
                items: vec!["1".into(), "2".into(), "3".into(), "4".into(), "5".into()],
            }
        );
        assert_eq!(spec("{ var: n, from: 0, to: 4 }").items.first().unwrap(), "0");
    }

    #[test]
    fn stepped_and_descending_ranges() {
        assert_eq!(spec("{ from: 0, to: 10, step: 5 }").items, ["0", "5", "10"]);
        assert_eq!(spec("{ from: 3, to: 1 }").items, ["3", "2", "1"]);
        assert!(spec("{ from: 5, to: 1, step: 1 }").items.is_empty());
    }

    #[test]
    fn item_lists() {
        assert_eq!(
            spec("[alice, bob]"),
            LoopSpec {
                var: "item".into(),
                items: vec!["alice".into(), "bob".into()],
            }
        );
        assert_eq!(spec("{ var: u, items: [alice, bob] }").var, "u");
    }

    #[test]
    fn templated_bounds_arrive_as_strings() {
        assert_eq!(spec(r#"{ var: n, from: "1", to: "3" }"#).items, ["1", "2", "3"]);
    }

    #[test]
    fn bad_specs_are_rejected() {
        for yaml in [
            "{ var: n, from: 1 }",         // no `to`
            "{ var: n, from: 1, to: x }",  // not a number
            "{ var: n, count: 5 }",        // unknown key
            "{ from: 1, to: 5, step: 0 }", // infinite
            "7",                           // not a list or map
        ] {
            assert!(
                LoopSpec::parse(&serde_yaml::from_str(yaml).unwrap()).is_err(),
                "should have been rejected: {yaml}"
            );
        }
    }

    #[test]
    fn expands_one_task_per_item() {
        let raw = r##"
tasks:
  - name: create_user
    loop: { var: n, from: 1, to: 3 }
    steps:
      - fill: { selector: "#name", value: "user{{ n }}" }
"##;
        let (m, groups) = render_document(raw, &ctx()).unwrap();
        let names: Vec<_> = m.tasks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["create_user[1]", "create_user[2]", "create_user[3]"]);
        assert_eq!(groups["create_user"], names);
        // The loop variable really reached the step.
        match &m.tasks[2].steps[0] {
            crate::manifest::Step::Fill(f) => assert_eq!(f.value, "user3"),
            other => panic!("unexpected step {other:?}"),
        }
    }

    #[test]
    fn loop_index_is_available() {
        let raw = r##"
tasks:
  - name: t
    loop: [a, b]
    steps:
      - fill: { selector: "#i", value: "{{ loop_index }}-{{ loop_index1 }}-{{ item }}" }
"##;
        let (m, _) = render_document(raw, &ctx()).unwrap();
        let values: Vec<String> = m
            .tasks
            .iter()
            .map(|t| match &t.steps[0] {
                crate::manifest::Step::Fill(f) => f.value.clone(),
                other => panic!("unexpected step {other:?}"),
            })
            .collect();
        assert_eq!(values, ["0-1-a", "1-2-b"]);
    }

    #[test]
    fn plain_tasks_pass_through_untouched() {
        let raw = r##"
vars:
  WHO: ada
tasks:
  - name: plain
    steps:
      - goto: "https://example.com"
  - name: looped
    loop: [1]
    steps:
      - goto: "https://example.com/{{ item }}"
"##;
        let (m, groups) = render_document(raw, &ctx()).unwrap();
        assert_eq!(m.tasks.len(), 2);
        assert_eq!(m.tasks[0].name, "plain");
        assert_eq!(m.tasks[1].name, "looped[1]");
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn loop_variable_in_name_is_rejected() {
        let raw = r##"
tasks:
  - name: "user{{ n }}"
    loop: { var: n, from: 1, to: 2 }
    steps: []
"##;
        let err = render_document(raw, &ctx()).unwrap_err().to_string();
        assert!(err.contains("must not appear in `name:`"), "{err}");
    }

    #[test]
    fn an_empty_loop_keeps_its_name_so_needs_can_resolve() {
        let raw = r##"
tasks:
  - name: nothing
    loop: { var: n, from: 5, to: 1, step: 1 }
    steps: []
"##;
        let (m, groups) = render_document(raw, &ctx()).unwrap();
        assert!(m.tasks.is_empty());
        assert_eq!(groups["nothing"], Vec::<String>::new());
    }

    #[test]
    fn needs_may_reference_a_sibling_iteration() {
        let raw = r##"
tasks:
  - name: step
    loop: { var: n, from: 2, to: 3 }
    needs: ["step[{{ n }}]"]
    steps: []
"##;
        let (m, _) = render_document(raw, &ctx()).unwrap();
        assert_eq!(m.tasks[0].needs, ["step[2]"]);
        assert_eq!(m.tasks[1].needs, ["step[3]"]);
    }

    #[test]
    fn present_detects_the_key_only() {
        assert!(present("  - loop: [1, 2]\n"));
        assert!(present("    loop:\n      var: n\n"));
        assert!(!present("  - name: loop_over_users\n"));
        assert!(!present("  - fill: { selector: \"#x\", value: \"loop: no\" }\n"));
    }
}
