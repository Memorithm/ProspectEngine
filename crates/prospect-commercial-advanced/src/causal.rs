use core::fmt;
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CausalError {
    EmptyGraph,
    UnknownVariable(usize),
    SelfEdge(usize),
    DuplicateEdge { from: usize, to: usize },
    CyclicGraph,
    SameEndpoint,
    DuplicateAdjustmentVariable(usize),
    AdjustmentContainsEndpoint(usize),
    OutcomeIsParentOfTreatment,
}

impl fmt::Display for CausalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyGraph => formatter.write_str("causal graph must contain at least one node"),
            Self::UnknownVariable(index) => {
                write!(formatter, "unknown causal variable index {index}")
            }
            Self::SelfEdge(index) => {
                write!(formatter, "self edge is not allowed at variable {index}")
            }
            Self::DuplicateEdge { from, to } => {
                write!(formatter, "duplicate causal edge {from}->{to}")
            }
            Self::CyclicGraph => formatter.write_str("causal graph must be acyclic"),
            Self::SameEndpoint => formatter.write_str("treatment and outcome must differ"),
            Self::DuplicateAdjustmentVariable(index) => {
                write!(formatter, "duplicate adjustment variable {index}")
            }
            Self::AdjustmentContainsEndpoint(index) => {
                write!(
                    formatter,
                    "adjustment set contains treatment/outcome endpoint {index}"
                )
            }
            Self::OutcomeIsParentOfTreatment => formatter.write_str(
                "outcome is a parent of treatment; queried causal direction contradicts the graph",
            ),
        }
    }
}

impl std::error::Error for CausalError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CausalDag {
    parents: Vec<Vec<usize>>,
    children: Vec<Vec<usize>>,
}

impl CausalDag {
    pub fn new(node_count: usize, edges: &[(usize, usize)]) -> Result<Self, CausalError> {
        if node_count == 0 {
            return Err(CausalError::EmptyGraph);
        }
        let mut parents = vec![Vec::new(); node_count];
        let mut children = vec![Vec::new(); node_count];
        let mut seen = BTreeSet::new();
        for &(from, to) in edges {
            if from >= node_count {
                return Err(CausalError::UnknownVariable(from));
            }
            if to >= node_count {
                return Err(CausalError::UnknownVariable(to));
            }
            if from == to {
                return Err(CausalError::SelfEdge(from));
            }
            if !seen.insert((from, to)) {
                return Err(CausalError::DuplicateEdge { from, to });
            }
            children[from].push(to);
            parents[to].push(from);
        }
        for values in &mut parents {
            values.sort_unstable();
        }
        for values in &mut children {
            values.sort_unstable();
        }
        let graph = Self { parents, children };
        if !graph.is_acyclic() {
            return Err(CausalError::CyclicGraph);
        }
        Ok(graph)
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.parents.len()
    }

    #[must_use]
    pub fn parents(&self, variable: usize) -> Option<&[usize]> {
        self.parents.get(variable).map(Vec::as_slice)
    }

    #[must_use]
    pub fn children(&self, variable: usize) -> Option<&[usize]> {
        self.children.get(variable).map(Vec::as_slice)
    }

    #[must_use]
    pub fn descendants(&self, variable: usize) -> Vec<usize> {
        if variable >= self.node_count() {
            return Vec::new();
        }
        let mut visited = vec![false; self.node_count()];
        let mut stack = self.children[variable].clone();
        while let Some(node) = stack.pop() {
            if visited[node] {
                continue;
            }
            visited[node] = true;
            stack.extend(self.children[node].iter().copied());
        }
        visited
            .into_iter()
            .enumerate()
            .filter_map(|(index, is_descendant)| is_descendant.then_some(index))
            .collect()
    }

    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for from in 0..self.node_count() {
            for to in &self.children[from] {
                for value in [from as u64, *to as u64] {
                    for byte in value.to_le_bytes() {
                        hash ^= u64::from(byte);
                        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                    }
                }
            }
        }
        hash
    }

    fn is_acyclic(&self) -> bool {
        let mut indegree: Vec<usize> = self.parents.iter().map(Vec::len).collect();
        let mut queue: VecDeque<usize> = indegree
            .iter()
            .enumerate()
            .filter_map(|(index, degree)| (*degree == 0).then_some(index))
            .collect();
        let mut visited = 0_usize;
        while let Some(node) = queue.pop_front() {
            visited += 1;
            for child in &self.children[node] {
                indegree[*child] -= 1;
                if indegree[*child] == 0 {
                    queue.push_back(*child);
                }
            }
        }
        visited == self.node_count()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackdoorViolation {
    ContainsDescendantOfTreatment { variable: usize },
    UnblockedBackdoorPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackdoorVerdict {
    Satisfied,
    Violated(BackdoorViolation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CausalIdentificationCertificate {
    pub graph_fingerprint: u64,
    pub treatment: usize,
    pub outcome: usize,
    pub adjustment: Vec<usize>,
    pub verdict: BackdoorVerdict,
    pub assumptions: Vec<String>,
}

pub fn identify_backdoor(
    dag: &CausalDag,
    treatment: usize,
    outcome: usize,
    adjustment: &[usize],
) -> Result<CausalIdentificationCertificate, CausalError> {
    let adjustment = validate_query(dag, treatment, outcome, adjustment)?;
    let verdict = check_backdoor_criterion_validated(dag, treatment, outcome, &adjustment);
    Ok(CausalIdentificationCertificate {
        graph_fingerprint: dag.fingerprint(),
        treatment,
        outcome,
        adjustment,
        verdict,
        assumptions: vec![
            "the supplied DAG is causally correct".to_string(),
            "relevant confounders represented by the DAG are observed".to_string(),
            "positivity and downstream estimator assumptions are checked separately".to_string(),
        ],
    })
}

pub fn check_backdoor_criterion(
    dag: &CausalDag,
    treatment: usize,
    outcome: usize,
    adjustment: &[usize],
) -> Result<BackdoorVerdict, CausalError> {
    let adjustment = validate_query(dag, treatment, outcome, adjustment)?;
    Ok(check_backdoor_criterion_validated(
        dag,
        treatment,
        outcome,
        &adjustment,
    ))
}

fn check_backdoor_criterion_validated(
    dag: &CausalDag,
    treatment: usize,
    outcome: usize,
    adjustment: &[usize],
) -> BackdoorVerdict {
    let descendants: BTreeSet<usize> = dag.descendants(treatment).into_iter().collect();
    if let Some(variable) = adjustment
        .iter()
        .copied()
        .find(|variable| descendants.contains(variable))
    {
        return BackdoorVerdict::Violated(BackdoorViolation::ContainsDescendantOfTreatment {
            variable,
        });
    }
    if is_d_separated_moral(dag, treatment, outcome, adjustment, Some(treatment)) {
        BackdoorVerdict::Satisfied
    } else {
        BackdoorVerdict::Violated(BackdoorViolation::UnblockedBackdoorPath)
    }
}

pub fn canonical_adjustment_set(
    dag: &CausalDag,
    treatment: usize,
    outcome: usize,
) -> Result<Vec<usize>, CausalError> {
    validate_query(dag, treatment, outcome, &[])?;
    let parents = dag.parents[treatment].clone();
    if parents.contains(&outcome) {
        return Err(CausalError::OutcomeIsParentOfTreatment);
    }
    Ok(parents)
}

pub fn find_minimal_adjustment_sets(
    dag: &CausalDag,
    treatment: usize,
    outcome: usize,
    maximum_size: usize,
) -> Result<Vec<Vec<usize>>, CausalError> {
    validate_query(dag, treatment, outcome, &[])?;
    let descendants: BTreeSet<usize> = dag.descendants(treatment).into_iter().collect();
    let candidates: Vec<usize> = (0..dag.node_count())
        .filter(|variable| {
            *variable != treatment && *variable != outcome && !descendants.contains(variable)
        })
        .collect();
    let mut accepted: Vec<Vec<usize>> = Vec::new();
    for size in 0..=maximum_size.min(candidates.len()) {
        let mut combinations = Vec::new();
        collect_combinations(&candidates, size, 0, &mut Vec::new(), &mut combinations);
        for candidate in combinations {
            if accepted
                .iter()
                .any(|existing| existing.iter().all(|variable| candidate.contains(variable)))
            {
                continue;
            }
            if check_backdoor_criterion_validated(dag, treatment, outcome, &candidate)
                == BackdoorVerdict::Satisfied
            {
                accepted.push(candidate);
            }
        }
    }
    Ok(accepted)
}

fn collect_combinations(
    values: &[usize],
    target_size: usize,
    start: usize,
    current: &mut Vec<usize>,
    output: &mut Vec<Vec<usize>>,
) {
    if current.len() == target_size {
        output.push(current.clone());
        return;
    }
    let remaining_needed = target_size - current.len();
    if values.len().saturating_sub(start) < remaining_needed {
        return;
    }
    for index in start..values.len() {
        current.push(values[index]);
        collect_combinations(values, target_size, index + 1, current, output);
        current.pop();
    }
}

fn validate_query(
    dag: &CausalDag,
    treatment: usize,
    outcome: usize,
    adjustment: &[usize],
) -> Result<Vec<usize>, CausalError> {
    for endpoint in [treatment, outcome] {
        if endpoint >= dag.node_count() {
            return Err(CausalError::UnknownVariable(endpoint));
        }
    }
    if treatment == outcome {
        return Err(CausalError::SameEndpoint);
    }
    let mut sorted = adjustment.to_vec();
    sorted.sort_unstable();
    for pair in sorted.windows(2) {
        if pair[0] == pair[1] {
            return Err(CausalError::DuplicateAdjustmentVariable(pair[0]));
        }
    }
    for variable in &sorted {
        if *variable >= dag.node_count() {
            return Err(CausalError::UnknownVariable(*variable));
        }
        if *variable == treatment || *variable == outcome {
            return Err(CausalError::AdjustmentContainsEndpoint(*variable));
        }
    }
    Ok(sorted)
}

fn is_d_separated_moral(
    dag: &CausalDag,
    left: usize,
    right: usize,
    conditioned: &[usize],
    remove_outgoing_from: Option<usize>,
) -> bool {
    let mut ancestral = vec![false; dag.node_count()];
    let mut stack = Vec::new();
    for seed in std::iter::once(left)
        .chain(std::iter::once(right))
        .chain(conditioned.iter().copied())
    {
        if !ancestral[seed] {
            ancestral[seed] = true;
            stack.push(seed);
        }
    }
    while let Some(node) = stack.pop() {
        for parent in effective_parents(dag, node, remove_outgoing_from) {
            if !ancestral[parent] {
                ancestral[parent] = true;
                stack.push(parent);
            }
        }
    }

    let mut moral = vec![BTreeSet::<usize>::new(); dag.node_count()];
    for child in 0..dag.node_count() {
        if !ancestral[child] {
            continue;
        }
        let parents: Vec<usize> = effective_parents(dag, child, remove_outgoing_from)
            .into_iter()
            .filter(|parent| ancestral[*parent])
            .collect();
        for parent in &parents {
            moral[*parent].insert(child);
            moral[child].insert(*parent);
        }
        for first in 0..parents.len() {
            for second in (first + 1)..parents.len() {
                moral[parents[first]].insert(parents[second]);
                moral[parents[second]].insert(parents[first]);
            }
        }
    }

    let blocked: BTreeSet<usize> = conditioned.iter().copied().collect();
    let mut visited = vec![false; dag.node_count()];
    let mut queue = VecDeque::new();
    visited[left] = true;
    queue.push_back(left);
    while let Some(node) = queue.pop_front() {
        if node == right {
            return false;
        }
        for neighbour in &moral[node] {
            if blocked.contains(neighbour) || visited[*neighbour] {
                continue;
            }
            visited[*neighbour] = true;
            queue.push_back(*neighbour);
        }
    }
    true
}

fn effective_parents(
    dag: &CausalDag,
    node: usize,
    remove_outgoing_from: Option<usize>,
) -> Vec<usize> {
    dag.parents[node]
        .iter()
        .copied()
        .filter(|parent| remove_outgoing_from != Some(*parent))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confounded() -> CausalDag {
        CausalDag::new(3, &[(0, 1), (0, 2), (1, 2)]).expect("valid DAG")
    }

    #[test]
    fn confounder_is_required_for_backdoor_adjustment() {
        let dag = confounded();
        assert_eq!(
            check_backdoor_criterion(&dag, 1, 2, &[]).expect("valid query"),
            BackdoorVerdict::Violated(BackdoorViolation::UnblockedBackdoorPath)
        );
        assert_eq!(
            check_backdoor_criterion(&dag, 1, 2, &[0]).expect("valid query"),
            BackdoorVerdict::Satisfied
        );
        assert_eq!(
            canonical_adjustment_set(&dag, 1, 2).expect("canonical set"),
            vec![0]
        );
    }

    #[test]
    fn descendant_adjustment_fails_condition_one() {
        let dag = CausalDag::new(3, &[(0, 1), (1, 2)]).expect("valid DAG");
        assert_eq!(
            check_backdoor_criterion(&dag, 0, 2, &[1]).expect("valid query"),
            BackdoorVerdict::Violated(BackdoorViolation::ContainsDescendantOfTreatment {
                variable: 1
            })
        );
    }

    #[test]
    fn conditioning_on_collider_opens_path() {
        let dag = CausalDag::new(5, &[(0, 2), (1, 2), (0, 3), (1, 4)]).expect("valid DAG");
        assert_eq!(
            check_backdoor_criterion(&dag, 3, 4, &[]).expect("empty adjustment"),
            BackdoorVerdict::Satisfied
        );
        assert_eq!(
            check_backdoor_criterion(&dag, 3, 4, &[2]).expect("collider adjustment"),
            BackdoorVerdict::Violated(BackdoorViolation::UnblockedBackdoorPath)
        );
    }

    #[test]
    fn minimal_adjustment_search_and_certificate_are_reproducible() {
        let dag = confounded();
        assert_eq!(
            find_minimal_adjustment_sets(&dag, 1, 2, 2).expect("minimal sets"),
            vec![vec![0]]
        );
        let certificate = identify_backdoor(&dag, 1, 2, &[0]).expect("certificate");
        assert_eq!(certificate.verdict, BackdoorVerdict::Satisfied);
        assert_eq!(certificate.adjustment, vec![0]);
        assert_eq!(certificate.graph_fingerprint, dag.fingerprint());
        assert_eq!(certificate.assumptions.len(), 3);
    }
}
