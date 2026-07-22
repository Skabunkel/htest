//! Assemble one or more loaded manifests into a single flat task list with
//! canonical `namespace:name` ids, resolving cross-file dependencies.

use crate::error::{HtError, Result};
use crate::loops::LoopGroups;
use crate::manifest::{Manifest, Task};
use std::collections::{HashMap, HashSet};

/// A manifest after templating, tagged with its namespace.
pub struct LoadedFile {
    pub namespace: String,
    pub manifest: Manifest,
    /// Looped task name -> its expanded iteration names (empty if no `loop:`).
    pub loop_groups: LoopGroups,
}

/// A task lifted into the global plan.
pub struct PlannedTask<'a> {
    /// Canonical id: `namespace:name`.
    pub id: String,
    /// Local task name (for display).
    #[allow(dead_code)]
    pub name: String,
    /// Dependencies as canonical ids.
    pub needs: Vec<String>,
    pub task: &'a Task,
}

/// Qualify a `needs` entry against the current namespace. An entry that
/// already contains `:` is treated as fully qualified (cross-file).
fn qualify(dep: &str, ns: &str) -> String {
    if dep.contains(':') {
        dep.to_string()
    } else {
        format!("{ns}:{dep}")
    }
}

/// Resolve one `needs` entry to the canonical ids it stands for. Naming a
/// looped task by its base name (`needs: [create_user]`) means *every*
/// iteration; naming an iteration directly (`create_user[3]`) means just it.
fn resolve_dep(dep: &str, ns: &str, groups: &HashMap<&str, &LoopGroups>) -> Vec<String> {
    let id = qualify(dep, ns);
    let (dep_ns, name) = id.split_once(':').unwrap_or((ns, &id));
    if let Some(names) = groups.get(dep_ns).and_then(|g| g.get(name)) {
        return names.iter().map(|n| format!("{dep_ns}:{n}")).collect();
    }
    vec![id]
}

/// Build the global plan, validating unique ids and resolvable deps.
pub fn assemble(files: &[LoadedFile]) -> Result<Vec<PlannedTask<'_>>> {
    let mut planned = Vec::new();

    let groups: HashMap<&str, &LoopGroups> = files
        .iter()
        .map(|f| (f.namespace.as_str(), &f.loop_groups))
        .collect();

    for f in files {
        for task in &f.manifest.tasks {
            let id = format!("{}:{}", f.namespace, task.name);
            let needs = task
                .needs
                .iter()
                .flat_map(|d| resolve_dep(d, &f.namespace, &groups))
                .collect();
            planned.push(PlannedTask {
                id,
                name: task.name.clone(),
                needs,
                task,
            });
        }
    }

    // Unique canonical ids.
    let mut seen = HashSet::new();
    for p in &planned {
        if !seen.insert(p.id.clone()) {
            return Err(HtError::Manifest(format!("duplicate task id `{}`", p.id)).into());
        }
    }

    // Every dependency must resolve to a known id.
    for p in &planned {
        for dep in &p.needs {
            if !seen.contains(dep) {
                return Err(HtError::UnknownDep(p.id.clone(), dep.clone()).into());
            }
        }
    }

    Ok(planned)
}

/// Return the namespace part of a canonical `namespace:name` id.
fn ns_of(id: &str) -> &str {
    id.split_once(':').map(|(ns, _)| ns).unwrap_or(id)
}

/// Layer playbook file-level dependencies onto an assembled plan.
///
/// `file_needs` is `(dependent_namespace, [depended_namespace, ...])`. For each
/// entry, every task in a depended namespace becomes a prerequisite of every
/// task in the dependent one — "all of A before all of B". This is what a
/// playbook's suite-level `needs` compiles down to; any resulting cycle is
/// caught later by `RunGraph::build`.
pub fn add_file_dependencies(
    planned: &mut [PlannedTask],
    file_needs: &[(String, Vec<String>)],
) -> Result<()> {
    // namespace -> all task ids in it.
    let mut by_ns: HashMap<&str, Vec<String>> = HashMap::new();
    for p in planned.iter() {
        by_ns.entry(ns_of(&p.id)).or_default().push(p.id.clone());
    }

    // Resolve each dependent namespace to the concrete extra prerequisite ids.
    let mut extra: HashMap<String, Vec<String>> = HashMap::new();
    for (dependent, depended) in file_needs {
        let bucket = extra.entry(dependent.clone()).or_default();
        for base in depended {
            if base == dependent {
                continue; // a suite depending on itself adds nothing.
            }
            match by_ns.get(base.as_str()) {
                Some(ids) => bucket.extend(ids.iter().cloned()),
                None => {
                    return Err(HtError::Manifest(format!(
                        "playbook suite `{dependent}` needs `{base}`, which is not in the playbook"
                    ))
                    .into())
                }
            }
        }
    }

    for p in planned.iter_mut() {
        if let Some(add) = extra.get(ns_of(&p.id)) {
            for id in add {
                if !p.needs.contains(id) {
                    p.needs.push(id.clone());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file whose `create_user` task was expanded into three iterations.
    fn looped_file() -> LoadedFile {
        let manifest: Manifest = serde_yaml::from_str(
            "tasks:\n\
             - {name: login}\n\
             - {name: \"create_user[1]\", needs: [login]}\n\
             - {name: \"create_user[2]\", needs: [login]}\n\
             - {name: \"create_user[3]\", needs: [login]}\n\
             - {name: report, needs: [create_user]}\n\
             - {name: audit, needs: [\"create_user[2]\"]}\n",
        )
        .unwrap();
        let mut loop_groups = LoopGroups::new();
        loop_groups.insert(
            "create_user".into(),
            vec![
                "create_user[1]".into(),
                "create_user[2]".into(),
                "create_user[3]".into(),
            ],
        );
        LoadedFile {
            namespace: "f".into(),
            manifest,
            loop_groups,
        }
    }

    fn needs_of<'a>(planned: &'a [PlannedTask], name: &str) -> &'a [String] {
        &planned.iter().find(|p| p.name == name).unwrap().needs
    }

    #[test]
    fn base_name_fans_out_to_every_iteration() {
        let files = [looped_file()];
        let planned = assemble(&files).unwrap();
        assert_eq!(
            needs_of(&planned, "report"),
            ["f:create_user[1]", "f:create_user[2]", "f:create_user[3]"]
        );
    }

    #[test]
    fn a_single_iteration_can_be_named_directly() {
        let files = [looped_file()];
        let planned = assemble(&files).unwrap();
        assert_eq!(needs_of(&planned, "audit"), ["f:create_user[2]"]);
    }
}
