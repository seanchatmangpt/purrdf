//! Dependency graph representation, cycle detection, and topological layering
//! for knowledge hook execution.

use std::collections::{BTreeMap, BTreeSet};

/// A dependency graph over hooks.
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    /// Outgoing edges: hook -> set of dependencies (prerequisites).
    dependencies: BTreeMap<String, BTreeSet<String>>,
    /// Reverse edges: hook -> set of dependents.
    dependents: BTreeMap<String, BTreeSet<String>>,
    /// All registered hook nodes.
    nodes: BTreeSet<String>,
}

impl DependencyGraph {
    /// Create an empty dependency graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a hook and its dependencies.
    pub fn add_node(&mut self, hook_id: impl Into<String>, dependencies: impl IntoIterator<Item = impl Into<String>>) {
        let hook_id = hook_id.into();
        self.nodes.insert(hook_id.clone());
        let deps: BTreeSet<String> = dependencies.into_iter().map(Into::into).collect();

        for dep in &deps {
            self.nodes.insert(dep.clone());
            self.dependents.entry(dep.clone()).or_default().insert(hook_id.clone());
        }

        self.dependencies.insert(hook_id, deps);
    }

    /// Detect if the graph contains any cycle. Returns `Some(cycle_path)` if a cycle exists.
    pub fn detect_cycle(&self) -> Option<Vec<String>> {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Color {
            White,
            Gray,
            Black,
        }

        let mut colors: BTreeMap<String, Color> = self.nodes.iter().map(|n| (n.clone(), Color::White)).collect();
        let mut path = Vec::new();

        fn dfs(
            node: &str,
            deps: &BTreeMap<String, BTreeSet<String>>,
            colors: &mut BTreeMap<String, Color>,
            path: &mut Vec<String>,
        ) -> Option<Vec<String>> {
            colors.insert(node.to_string(), Color::Gray);
            path.push(node.to_string());

            if let Some(node_deps) = deps.get(node) {
                for dep in node_deps {
                    match colors.get(dep) {
                        Some(Color::Gray) => {
                            let mut cycle = path.clone();
                            cycle.push(dep.clone());
                            return Some(cycle);
                        }
                        Some(Color::White) => {
                            if let Some(c) = dfs(dep, deps, colors, path) {
                                return Some(c);
                            }
                        }
                        _ => {}
                    }
                }
            }

            colors.insert(node.to_string(), Color::Black);
            path.pop();
            None
        }

        for node in &self.nodes {
            if colors.get(node) == Some(&Color::White) {
                if let Some(cycle) = dfs(node, &self.dependencies, &mut colors, &mut path) {
                    return Some(cycle);
                }
            }
        }

        None
    }

    /// Kahn's algorithm: topological sort returning layers of hooks that can run in parallel.
    pub fn topological_layers(&self) -> Result<Vec<Vec<String>>, Vec<String>> {
        if let Some(cycle) = self.detect_cycle() {
            return Err(cycle);
        }

        let mut in_degree: BTreeMap<String, usize> = self.nodes.iter().map(|n| (n.clone(), 0)).collect();
        for deps in self.dependencies.values() {
            for dep in deps {
                if let Some(cnt) = in_degree.get_mut(dep) {
                    *cnt += 1;
                }
            }
        }

        let mut queue: Vec<String> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(n, _)| n.clone())
            .collect();

        let mut layers = Vec::new();

        while !queue.is_empty() {
            let current_layer = queue.clone();
            queue.clear();

            for node in &current_layer {
                if let Some(deps) = self.dependencies.get(node) {
                    for dep in deps {
                        if let Some(cnt) = in_degree.get_mut(dep.as_str()) {
                            *cnt = cnt.saturating_sub(1);
                            if *cnt == 0 {
                                queue.push(dep.clone());
                            }
                        }
                    }
                }
            }

            layers.push(current_layer);
        }

        Ok(layers)
    }
}
