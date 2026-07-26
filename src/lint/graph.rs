// src/lint/graph.rs
//
// Transition-graph reachability. This is the part of static analysis a
// consumer cannot do for itself by grepping its own config: "which states can
// precede state S" and "does every path from A to B cross a given edge" are
// properties of the state machine, not of any one file.
//
// ## Index
// - Edge                  — one transition edge (from, to, command, transition index)
// - Graph                 — adjacency over transition edges
// - [graph-build]         Graph::build() / build_filtered() — build adjacency, optionally dropping edges
// - [graph-reachable]     Graph::reachable_from()  — forward closure (>= 1 edge)
// - [graph-ancestors]     Graph::ancestors_of()    — states that can reach a target (reverse closure)
// - [graph-path]          Graph::path_between()    — shortest example path, for diagnostics
// - [graph-outgoing]      Graph::outgoing()        — edges leaving a state

use std::collections::{HashMap, HashSet, VecDeque};

use crate::config::{ProtocolConfig, TransitionConfig};

/// One transition, viewed as a directed edge.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub command: String,
    /// Index into `ProtocolConfig::transitions` — lets a finding point back at
    /// the exact declaration.
    pub index: usize,
}

/// Adjacency over the transition graph.
///
/// Built fresh per analysis; protocols are small (tens of edges), so the maps
/// are rebuilt rather than cached.
pub struct Graph {
    edges: Vec<Edge>,
    out: HashMap<String, Vec<usize>>,
    inc: HashMap<String, Vec<usize>>,
}

impl Graph {
    // [graph-build]
    /// Build the full transition graph.
    pub fn build(config: &ProtocolConfig) -> Self {
        Self::build_filtered(config, |_| true)
    }

    /// Build the graph from the transitions `keep` accepts.
    ///
    /// Dropping a set of edges and re-testing reachability is how L3 decides
    /// whether a boundary can be routed around: remove every edge that carries
    /// the boundary tag, and see whether the target is still reachable.
    pub fn build_filtered<F>(config: &ProtocolConfig, keep: F) -> Self
    where
        F: Fn(&TransitionConfig) -> bool,
    {
        let mut edges = Vec::new();
        for (index, t) in config.transitions.iter().enumerate() {
            if keep(t) {
                edges.push(Edge {
                    from: t.from.clone(),
                    to: t.to.clone(),
                    command: t.command.clone(),
                    index,
                });
            }
        }

        let mut out: HashMap<String, Vec<usize>> = HashMap::new();
        let mut inc: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in edges.iter().enumerate() {
            out.entry(e.from.clone()).or_default().push(i);
            inc.entry(e.to.clone()).or_default().push(i);
        }

        Graph { edges, out, inc }
    }

    // [graph-outgoing]
    /// Edges leaving `state`.
    pub fn outgoing(&self, state: &str) -> Vec<&Edge> {
        self.out
            .get(state)
            .map(|ids| ids.iter().map(|&i| &self.edges[i]).collect())
            .unwrap_or_default()
    }

    // [graph-reachable]
    /// States reachable from `start` by one or more edges.
    ///
    /// `start` itself is included only when a cycle leads back to it.
    pub fn reachable_from(&self, start: &str) -> HashSet<&str> {
        self.closure(start, &self.out, |e| e.to.as_str())
    }

    // [graph-ancestors]
    /// States that can reach `target` by one or more edges — the states that
    /// can precede it in any run.
    ///
    /// `target` itself is included only when it sits on a cycle.
    pub fn ancestors_of(&self, target: &str) -> HashSet<&str> {
        self.closure(target, &self.inc, |e| e.from.as_str())
    }

    /// Shared BFS closure over either direction of the graph.
    fn closure<'a, F>(
        &'a self,
        start: &str,
        adjacency: &'a HashMap<String, Vec<usize>>,
        step: F,
    ) -> HashSet<&'a str>
    where
        F: Fn(&'a Edge) -> &'a str,
    {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();

        // Seed with the neighbours of `start` rather than `start` itself, so a
        // state only appears in its own closure when a cycle returns to it.
        if let Some(ids) = adjacency.get(start) {
            for &i in ids {
                let next = step(&self.edges[i]);
                if seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }

        while let Some(state) = queue.pop_front() {
            if let Some(ids) = adjacency.get(state) {
                for &i in ids {
                    let next = step(&self.edges[i]);
                    if seen.insert(next) {
                        queue.push_back(next);
                    }
                }
            }
        }

        seen
    }

    // [graph-path]
    /// A shortest path of edges from `from` to `to`, if one exists.
    ///
    /// Used to print a concrete route-around in an L3 finding — "here is the
    /// path that skips the boundary" is far more actionable than "a path
    /// exists".
    pub fn path_between(&self, from: &str, to: &str) -> Option<Vec<&Edge>> {
        // prev[state] = edge index used to arrive at state
        let mut prev: HashMap<&str, usize> = HashMap::new();
        let mut seen: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();
        queue.push_back(from);
        seen.insert(from);

        while let Some(state) = queue.pop_front() {
            let Some(ids) = self.out.get(state) else {
                continue;
            };
            for &i in ids {
                let edge = &self.edges[i];
                let next = edge.to.as_str();
                if next == to {
                    // Walk the predecessor chain back to `from`.
                    let mut chain = vec![edge];
                    let mut cursor = state;
                    while cursor != from {
                        let e = &self.edges[prev[cursor]];
                        chain.push(e);
                        cursor = e.from.as_str();
                    }
                    chain.reverse();
                    return Some(chain);
                }
                if seen.insert(next) {
                    prev.insert(next, i);
                    queue.push_back(next);
                }
            }
        }

        None
    }
}

/// Render a path of edges as `a -(cmd)-> b -(cmd)-> c`.
pub fn format_path(path: &[&Edge]) -> String {
    if path.is_empty() {
        return String::new();
    }
    let mut s = path[0].from.clone();
    for edge in path {
        s.push_str(&format!(" -({})-> {}", edge.command, edge.to));
    }
    s
}
