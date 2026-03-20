//! Wave computation — topological sort of children by dependencies.

use super::plan::ChildPlan;
use std::collections::HashSet;

/// Compute dispatch waves: each wave contains children whose dependencies
/// are all satisfied by previous waves. Children with no dependencies go first.
pub fn compute_waves(children: &[ChildPlan]) -> Vec<Vec<usize>> {
    let labels: Vec<&str> = children.iter().map(|c| c.label.as_str()).collect();
    let mut completed: HashSet<&str> = HashSet::new();
    let mut assigned: HashSet<usize> = HashSet::new();
    let mut waves: Vec<Vec<usize>> = Vec::new();

    loop {
        let mut wave: Vec<usize> = Vec::new();
        for (i, child) in children.iter().enumerate() {
            if assigned.contains(&i) {
                continue;
            }
            let deps_met = child.depends_on.iter().all(|d| completed.contains(d.as_str()));
            if deps_met {
                wave.push(i);
            }
        }

        if wave.is_empty() {
            if assigned.len() < children.len() {
                // Circular dependency — force remaining into one wave
                let remaining: Vec<usize> = (0..children.len())
                    .filter(|i| !assigned.contains(i))
                    .collect();
                waves.push(remaining);
            }
            break;
        }

        for &i in &wave {
            assigned.insert(i);
            completed.insert(labels[i]);
        }
        waves.push(wave);
    }

    waves
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleave::plan::ChildPlan;

    fn child(label: &str, deps: &[&str]) -> ChildPlan {
        ChildPlan {
            label: label.to_string(),
            description: String::new(),
            scope: vec![],
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn independent_children_single_wave() {
        let children = vec![child("a", &[]), child("b", &[]), child("c", &[])];
        let waves = compute_waves(&children);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0], vec![0, 1, 2]);
    }

    #[test]
    fn linear_chain() {
        let children = vec![child("a", &[]), child("b", &["a"]), child("c", &["b"])];
        let waves = compute_waves(&children);
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0], vec![0]);
        assert_eq!(waves[1], vec![1]);
        assert_eq!(waves[2], vec![2]);
    }

    #[test]
    fn fan_out() {
        let children = vec![
            child("base", &[]),
            child("a", &["base"]),
            child("b", &["base"]),
            child("c", &["base"]),
        ];
        let waves = compute_waves(&children);
        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0], vec![0]);
        assert_eq!(waves[1], vec![1, 2, 3]);
    }
}
