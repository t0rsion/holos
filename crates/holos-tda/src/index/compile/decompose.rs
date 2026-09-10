use std::collections::{BTreeMap, BTreeSet};

use crate::{EdgeKey, Error, Result, RipsParams};

use super::super::model::IndexParams;
use super::Scope;

pub(super) struct TreeSpec {
    pub(super) scope: Scope,
    pub(super) separator: Vec<usize>,
    pub(super) children: Vec<TreeSpec>,
}

pub(super) struct SeparatorSearch<'a> {
    topology: &'a [EdgeKey],
    params: IndexParams,
    pub(super) checked: usize,
    pub(super) complete: bool,
}

impl<'a> SeparatorSearch<'a> {
    pub(super) fn new(topology: &'a [EdgeKey], params: IndexParams) -> Self {
        Self {
            topology,
            params,
            checked: 0,
            complete: true,
        }
    }

    pub(super) fn decompose(&mut self, scope: Scope) -> TreeSpec {
        if scope.vertices.len() <= self.params.leaf_vertices || !self.complete {
            return TreeSpec {
                scope,
                separator: Vec::new(),
                children: Vec::new(),
            };
        }
        let components = scope_components(&scope, &[], self.topology);
        let choice = if components.len() > 1 {
            Some((Vec::new(), components))
        } else {
            self.find_separator(&scope)
        };
        let Some((separator, components)) = choice else {
            return TreeSpec {
                scope,
                separator: Vec::new(),
                children: Vec::new(),
            };
        };
        let children = components
            .into_iter()
            .map(|component| {
                let mut vertices = separator.clone();
                vertices.extend(component);
                vertices.sort_unstable();
                vertices.dedup();
                let members: BTreeSet<_> = vertices.iter().copied().collect();
                let edge_positions = scope
                    .edge_positions
                    .iter()
                    .copied()
                    .filter(|&position| {
                        let edge = self.topology[position];
                        members.contains(&edge.u) && members.contains(&edge.v)
                    })
                    .collect();
                self.decompose(Scope {
                    vertices,
                    edge_positions,
                })
            })
            .collect();
        TreeSpec {
            scope,
            separator,
            children,
        }
    }

    fn find_separator(&mut self, scope: &Scope) -> Option<(Vec<usize>, Vec<Vec<usize>>)> {
        let maximum = self
            .params
            .max_separator_width
            .min(scope.vertices.len().saturating_sub(2));
        for width in 1..=maximum {
            let mut positions: Vec<_> = (0..width).collect();
            let mut best: Option<(usize, Vec<usize>, Vec<Vec<usize>>)> = None;
            loop {
                if self.checked == self.params.separator_search_limit {
                    self.complete = false;
                    break;
                }
                self.checked += 1;
                let separator: Vec<_> = positions
                    .iter()
                    .map(|&position| scope.vertices[position])
                    .collect();
                let components = scope_components(scope, &separator, self.topology);
                if components.len() > 1 {
                    let largest = components.iter().map(Vec::len).max().unwrap_or(0);
                    let replace = best
                        .as_ref()
                        .map(|(old, old_separator, _)| {
                            (largest, &separator) < (*old, old_separator)
                        })
                        .unwrap_or(true);
                    if replace {
                        best = Some((largest, separator, components));
                    }
                }
                if !next_combination(&mut positions, scope.vertices.len()) {
                    break;
                }
            }
            if let Some((_, separator, components)) = best {
                return Some((separator, components));
            }
            if !self.complete {
                break;
            }
        }
        None
    }
}

pub(super) fn validate_params(_params: &RipsParams, index: IndexParams) -> Result<()> {
    if index.leaf_vertices < 2 {
        return Err(Error::InvalidInput(
            "index leaf_vertices must be at least 2".into(),
        ));
    }
    if index.separator_search_limit == 0 {
        return Err(Error::InvalidInput(
            "index separator_search_limit must be positive".into(),
        ));
    }
    Ok(())
}

fn scope_components(scope: &Scope, separator: &[usize], topology: &[EdgeKey]) -> Vec<Vec<usize>> {
    let excluded: BTreeSet<_> = separator.iter().copied().collect();
    let members: BTreeSet<_> = scope.vertices.iter().copied().collect();
    let adjacency = scope_adjacency(scope, topology, &members, &excluded);
    let mut seen = BTreeSet::new();
    let mut components = Vec::new();
    for &root in adjacency.keys() {
        if seen.insert(root) {
            components.push(walk_component(root, &adjacency, &mut seen));
        }
    }
    components.sort_unstable();
    components
}

fn scope_adjacency(
    scope: &Scope,
    topology: &[EdgeKey],
    members: &BTreeSet<usize>,
    excluded: &BTreeSet<usize>,
) -> BTreeMap<usize, Vec<usize>> {
    let mut adjacency = BTreeMap::<usize, Vec<usize>>::new();
    for &vertex in &scope.vertices {
        if !excluded.contains(&vertex) {
            adjacency.insert(vertex, Vec::new());
        }
    }
    for &position in &scope.edge_positions {
        let edge = topology[position];
        if members.contains(&edge.u)
            && members.contains(&edge.v)
            && !excluded.contains(&edge.u)
            && !excluded.contains(&edge.v)
        {
            adjacency.get_mut(&edge.u).unwrap().push(edge.v);
            adjacency.get_mut(&edge.v).unwrap().push(edge.u);
        }
    }
    adjacency
}

fn walk_component(
    root: usize,
    adjacency: &BTreeMap<usize, Vec<usize>>,
    seen: &mut BTreeSet<usize>,
) -> Vec<usize> {
    let mut stack = vec![root];
    let mut component = Vec::new();
    while let Some(vertex) = stack.pop() {
        component.push(vertex);
        for &neighbor in &adjacency[&vertex] {
            if seen.insert(neighbor) {
                stack.push(neighbor);
            }
        }
    }
    component.sort_unstable();
    component
}

fn next_combination(positions: &mut [usize], universe: usize) -> bool {
    for index in (0..positions.len()).rev() {
        let maximum = universe - (positions.len() - index);
        if positions[index] < maximum {
            positions[index] += 1;
            for next in index + 1..positions.len() {
                positions[next] = positions[next - 1] + 1;
            }
            return true;
        }
    }
    false
}
