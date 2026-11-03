//! Deciding which plugins load, and in what order.
//!
//! Rules: a plugin in `depend` must be present (and itself loadable) or the
//! dependent is rejected; a plugin in `softdepend` only affects order, if present;
//! `loadbefore` orders this plugin ahead of the named one, if present. Names match
//! ignoring case. Among plugins with no ordering constraint, `STARTUP` plugins go
//! before `POSTWORLD` ones, then by name, so the result is deterministic.
//!
//! Soft dependencies count as ordering edges, so a cycle that passes through one
//! is still a cycle and the plugins involved are rejected.

use std::collections::{BTreeSet, HashMap};

use crate::meta::{LoadOrder, PluginMeta};

/// Why a plugin will not load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// A required dependency is not installed.
    MissingDependency(String),
    /// A required dependency was itself rejected.
    DependencyRejected(String),
    /// The plugin is in, or depends on, a dependency cycle.
    Cycle,
}

/// The outcome of resolving a set of plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// Plugin names in the order they should load.
    pub order: Vec<String>,
    /// Plugins that will not load, in input order.
    pub rejected: Vec<(String, RejectReason)>,
}

/// Resolves load order and rejections for `plugins`.
///
/// Plugin names must be unique (ignoring case).
pub fn resolve(plugins: &[PluginMeta]) -> Resolution {
    let index: HashMap<String, usize> = plugins
        .iter()
        .enumerate()
        .map(|(i, meta)| (meta.name.to_ascii_lowercase(), i))
        .collect();
    let find = |name: &str| index.get(&name.to_ascii_lowercase()).copied();
    let mut rejected: Vec<Option<RejectReason>> = vec![None; plugins.len()];

    loop {
        let mut changed = false;
        for (i, meta) in plugins.iter().enumerate() {
            if rejected[i].is_some() {
                continue;
            }
            for dep in &meta.depend {
                let reason = match find(dep) {
                    None => Some(RejectReason::MissingDependency(dep.clone())),
                    Some(j) if rejected[j].is_some() => {
                        Some(RejectReason::DependencyRejected(plugins[j].name.clone()))
                    }
                    Some(_) => None,
                };
                if reason.is_some() {
                    rejected[i] = reason;
                    changed = true;
                    break;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut edges: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (i, meta) in plugins.iter().enumerate() {
        if rejected[i].is_some() {
            continue;
        }
        for dep in meta.depend.iter().chain(&meta.softdepend) {
            if let Some(j) = find(dep).filter(|j| rejected[*j].is_none()) {
                edges.insert((j, i));
            }
        }
        for later in &meta.loadbefore {
            if let Some(j) = find(later).filter(|j| rejected[*j].is_none()) {
                edges.insert((i, j));
            }
        }
    }

    let mut indegree = vec![0_usize; plugins.len()];
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); plugins.len()];
    for &(from, to) in &edges {
        indegree[to] += 1;
        successors[from].push(to);
    }
    let key = |i: usize| -> (LoadOrder, String, usize) {
        (plugins[i].load, plugins[i].name.to_ascii_lowercase(), i)
    };
    let mut ready: BTreeSet<(LoadOrder, String, usize)> = (0..plugins.len())
        .filter(|i| rejected[*i].is_none() && indegree[*i] == 0)
        .map(key)
        .collect();
    let mut order_idx = Vec::new();
    while let Some(next) = ready.pop_first() {
        let i = next.2;
        order_idx.push(i);
        for &succ in &successors[i] {
            indegree[succ] -= 1;
            if indegree[succ] == 0 {
                ready.insert(key(succ));
            }
        }
    }
    for i in 0..plugins.len() {
        if rejected[i].is_none() && !order_idx.contains(&i) {
            rejected[i] = Some(RejectReason::Cycle);
        }
    }

    Resolution {
        order: order_idx.iter().map(|i| plugins[*i].name.clone()).collect(),
        rejected: rejected
            .into_iter()
            .enumerate()
            .filter_map(|(i, reason)| reason.map(|r| (plugins[i].name.clone(), r)))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(name: &str) -> PluginMeta {
        PluginMeta {
            name: name.to_owned(),
            version: "1".to_owned(),
            main: format!("x.{name}"),
            api_version: None,
            load: LoadOrder::PostWorld,
            depend: Vec::new(),
            softdepend: Vec::new(),
            loadbefore: Vec::new(),
            description: None,
            authors: Vec::new(),
        }
    }

    fn with(mut meta: PluginMeta, f: impl FnOnce(&mut PluginMeta)) -> PluginMeta {
        f(&mut meta);
        meta
    }

    fn names(resolution: &Resolution) -> Vec<&str> {
        resolution.order.iter().map(String::as_str).collect()
    }

    #[test]
    fn independent_plugins_load_by_name() {
        let result = resolve(&[plugin("Zeta"), plugin("alpha"), plugin("Mid")]);
        assert_eq!(names(&result), ["alpha", "Mid", "Zeta"]);
        assert!(result.rejected.is_empty());
    }

    #[test]
    fn startup_plugins_load_before_postworld() {
        let early = with(plugin("Zzz"), |m| m.load = LoadOrder::Startup);
        let result = resolve(&[plugin("Aaa"), early]);
        assert_eq!(names(&result), ["Zzz", "Aaa"]);
    }

    #[test]
    fn dependencies_load_first() {
        let app = with(plugin("App"), |m| m.depend = vec!["core".to_owned()]);
        let result = resolve(&[app, plugin("Core")]);
        assert_eq!(names(&result), ["Core", "App"]);
    }

    #[test]
    fn soft_dependencies_order_when_present_and_are_ignored_when_absent() {
        let app = with(plugin("App"), |m| {
            m.softdepend = vec!["Extras".to_owned(), "Ghost".to_owned()];
        });
        let result = resolve(&[app, plugin("Extras")]);
        assert_eq!(names(&result), ["Extras", "App"]);
        assert!(result.rejected.is_empty());
    }

    #[test]
    fn loadbefore_puts_this_plugin_first() {
        let first = with(plugin("Zed"), |m| m.loadbefore = vec!["Alpha".to_owned()]);
        let result = resolve(&[plugin("Alpha"), first]);
        assert_eq!(names(&result), ["Zed", "Alpha"]);
    }

    #[test]
    fn missing_hard_dependencies_reject_and_cascade() {
        let a = with(plugin("A"), |m| m.depend = vec!["Missing".to_owned()]);
        let b = with(plugin("B"), |m| m.depend = vec!["A".to_owned()]);
        let result = resolve(&[a, b, plugin("C")]);
        assert_eq!(names(&result), ["C"]);
        assert_eq!(
            result.rejected,
            [
                ("A".to_owned(), RejectReason::MissingDependency("Missing".to_owned())),
                ("B".to_owned(), RejectReason::DependencyRejected("A".to_owned())),
            ]
        );
    }

    #[test]
    fn cycles_are_rejected_but_unrelated_plugins_still_load() {
        let a = with(plugin("A"), |m| m.depend = vec!["B".to_owned()]);
        let b = with(plugin("B"), |m| m.depend = vec!["A".to_owned()]);
        let tail = with(plugin("Tail"), |m| m.depend = vec!["A".to_owned()]);
        let result = resolve(&[a, b, tail, plugin("Free")]);
        assert_eq!(names(&result), ["Free"]);
        let rejected: Vec<&str> = result.rejected.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(rejected, ["A", "B", "Tail"]);
        assert!(result.rejected.iter().all(|(_, r)| *r == RejectReason::Cycle));
    }

    #[test]
    fn self_dependency_is_a_cycle() {
        let a = with(plugin("A"), |m| m.depend = vec!["a".to_owned()]);
        let result = resolve(&[a]);
        assert!(result.order.is_empty());
        assert_eq!(result.rejected, [("A".to_owned(), RejectReason::Cycle)]);
    }

    #[test]
    fn chains_resolve_transitively() {
        let c = with(plugin("C"), |m| m.depend = vec!["B".to_owned()]);
        let b = with(plugin("B"), |m| m.depend = vec!["A".to_owned()]);
        let result = resolve(&[c, b, plugin("A")]);
        assert_eq!(names(&result), ["A", "B", "C"]);
    }
}
