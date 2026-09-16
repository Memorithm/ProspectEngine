use crate::causal::{BackdoorVerdict, CausalDag, CausalError, check_backdoor_criterion};
use core::fmt;
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdvancedCausalError {
    Base(CausalError),
    SameVariable,
    UnknownVariable(usize),
    AdjustmentContainsEndpoint(usize),
}

impl fmt::Display for AdvancedCausalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Base(error) => write!(formatter, "{error}"),
            Self::SameVariable => formatter.write_str("causal front-door/IV variables must be distinct"),
            Self::UnknownVariable(index) => write!(formatter, "unknown causal variable index {index}"),
            Self::AdjustmentContainsEndpoint(index) => {
                write!(formatter, "instrument adjustment contains an endpoint {index}")
            }
        }
    }
}

impl std::error::Error for AdvancedCausalError {}

impl From<CausalError> for AdvancedCausalError {
    fn from(value: CausalError) -> Self {
        Self::Base(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrontDoorViolation {
    MediatorNotDownstreamOfTreatment,
    OutcomeNotDownstreamOfMediator,
    DirectedTreatmentOutcomePathBypassesMediator,
    TreatmentMediatorBackdoorOpen,
    MediatorOutcomeBackdoorNotBlockedByTreatment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrontDoorVerdict {
    Satisfied,
    Violated(FrontDoorViolation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontDoorCertificate {
    pub graph_fingerprint: u64,
    pub treatment: usize,
    pub mediator: usize,
    pub outcome: usize,
    pub verdict: FrontDoorVerdict,
    pub assumptions: Vec<String>,
}

pub fn identify_front_door(
    dag: &CausalDag,
    treatment: usize,
    mediator: usize,
    outcome: usize,
) -> Result<FrontDoorCertificate, AdvancedCausalError> {
    validate_distinct(dag, &[treatment, mediator, outcome])?;
    let verdict = if !has_directed_path(dag, treatment, mediator, None) {
        FrontDoorVerdict::Violated(FrontDoorViolation::MediatorNotDownstreamOfTreatment)
    } else if !has_directed_path(dag, mediator, outcome, None) {
        FrontDoorVerdict::Violated(FrontDoorViolation::OutcomeNotDownstreamOfMediator)
    } else if has_directed_path(dag, treatment, outcome, Some(mediator)) {
        FrontDoorVerdict::Violated(
            FrontDoorViolation::DirectedTreatmentOutcomePathBypassesMediator,
        )
    } else if check_backdoor_criterion(dag, treatment, mediator, &[])?
        != BackdoorVerdict::Satisfied
    {
        FrontDoorVerdict::Violated(FrontDoorViolation::TreatmentMediatorBackdoorOpen)
    } else if check_backdoor_criterion(dag, mediator, outcome, &[treatment])?
        != BackdoorVerdict::Satisfied
    {
        FrontDoorVerdict::Violated(
            FrontDoorViolation::MediatorOutcomeBackdoorNotBlockedByTreatment,
        )
    } else {
        FrontDoorVerdict::Satisfied
    };

    Ok(FrontDoorCertificate {
        graph_fingerprint: dag.fingerprint(),
        treatment,
        mediator,
        outcome,
        verdict,
        assumptions: vec![
            "the supplied DAG is causally correct".to_string(),
            "the mediator represents the full observed front-door channel being tested".to_string(),
            "positivity and downstream statistical estimation assumptions are validated separately"
                .to_string(),
        ],
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstrumentViolation {
    NoDirectedInstrumentTreatmentPath,
    NoDirectedTreatmentOutcomePath,
    ExclusionRestrictionViolated,
    InstrumentOutcomeBackdoorOpen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstrumentVerdict {
    Satisfied,
    Violated(InstrumentViolation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstrumentalVariableCertificate {
    pub graph_fingerprint: u64,
    pub instrument: usize,
    pub treatment: usize,
    pub outcome: usize,
    pub adjustment: Vec<usize>,
    pub verdict: InstrumentVerdict,
    pub assumptions: Vec<String>,
}

/// Checks a conservative graphical sufficient condition for an instrument.
///
/// The check requires relevance (`Z -> ... -> X`), a directed treatment-outcome
/// path, no directed instrument-outcome path that bypasses treatment, and no
/// open backdoor path from the instrument to outcome after the supplied
/// pre-treatment adjustment. This is deliberately narrower than every valid IV
/// identification strategy.
pub fn identify_instrumental_variable(
    dag: &CausalDag,
    instrument: usize,
    treatment: usize,
    outcome: usize,
    adjustment: &[usize],
) -> Result<InstrumentalVariableCertificate, AdvancedCausalError> {
    validate_distinct(dag, &[instrument, treatment, outcome])?;
    let mut adjustment = adjustment.to_vec();
    adjustment.sort_unstable();
    adjustment.dedup();
    for variable in &adjustment {
        if *variable >= dag.node_count() {
            return Err(AdvancedCausalError::UnknownVariable(*variable));
        }
        if [instrument, treatment, outcome].contains(variable) {
            return Err(AdvancedCausalError::AdjustmentContainsEndpoint(*variable));
        }
    }

    let verdict = if !has_directed_path(dag, instrument, treatment, None) {
        InstrumentVerdict::Violated(InstrumentViolation::NoDirectedInstrumentTreatmentPath)
    } else if !has_directed_path(dag, treatment, outcome, None) {
        InstrumentVerdict::Violated(InstrumentViolation::NoDirectedTreatmentOutcomePath)
    } else if has_directed_path(dag, instrument, outcome, Some(treatment)) {
        InstrumentVerdict::Violated(InstrumentViolation::ExclusionRestrictionViolated)
    } else if check_backdoor_criterion(dag, instrument, outcome, &adjustment)?
        != BackdoorVerdict::Satisfied
    {
        InstrumentVerdict::Violated(InstrumentViolation::InstrumentOutcomeBackdoorOpen)
    } else {
        InstrumentVerdict::Satisfied
    };

    Ok(InstrumentalVariableCertificate {
        graph_fingerprint: dag.fingerprint(),
        instrument,
        treatment,
        outcome,
        adjustment,
        verdict,
        assumptions: vec![
            "the supplied DAG is causally correct".to_string(),
            "instrument relevance is structural; weak-instrument strength is assessed statistically later"
                .to_string(),
            "the graphical exclusion/exogeneity checks are sufficient conditions, not a universal IV theorem"
                .to_string(),
        ],
    })
}

fn validate_distinct(dag: &CausalDag, variables: &[usize]) -> Result<(), AdvancedCausalError> {
    for variable in variables {
        if *variable >= dag.node_count() {
            return Err(AdvancedCausalError::UnknownVariable(*variable));
        }
    }
    for left in 0..variables.len() {
        for right in (left + 1)..variables.len() {
            if variables[left] == variables[right] {
                return Err(AdvancedCausalError::SameVariable);
            }
        }
    }
    Ok(())
}

fn has_directed_path(
    dag: &CausalDag,
    source: usize,
    target: usize,
    excluded_node: Option<usize>,
) -> bool {
    if excluded_node == Some(source) || excluded_node == Some(target) {
        return false;
    }
    let mut visited = vec![false; dag.node_count()];
    let mut queue = VecDeque::new();
    visited[source] = true;
    queue.push_back(source);
    while let Some(node) = queue.pop_front() {
        if node == target {
            return true;
        }
        let Some(children) = dag.children(node) else {
            continue;
        };
        for child in children {
            if excluded_node == Some(*child) || visited[*child] {
                continue;
            }
            visited[*child] = true;
            queue.push_back(*child);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_door_certificate_accepts_canonical_structure_with_xy_confounding() {
        // U=0, X=1, M=2, Y=3. U confounds X/Y, while M mediates X -> Y.
        let dag = CausalDag::new(4, &[(0, 1), (0, 3), (1, 2), (2, 3)])
            .expect("valid front-door DAG");
        let certificate = identify_front_door(&dag, 1, 2, 3).expect("front-door certificate");
        assert_eq!(certificate.verdict, FrontDoorVerdict::Satisfied);
    }

    #[test]
    fn front_door_rejects_direct_path_bypassing_mediator() {
        let dag = CausalDag::new(4, &[(0, 1), (0, 3), (1, 2), (2, 3), (1, 3)])
            .expect("valid DAG");
        let certificate = identify_front_door(&dag, 1, 2, 3).expect("front-door query");
        assert_eq!(
            certificate.verdict,
            FrontDoorVerdict::Violated(
                FrontDoorViolation::DirectedTreatmentOutcomePathBypassesMediator
            )
        );
    }

    #[test]
    fn iv_certificate_accepts_relevant_exogenous_excluded_instrument() {
        // U=0 confounds X=2 and Y=3; Z=1 affects X but has no other route to Y.
        let dag = CausalDag::new(4, &[(0, 2), (0, 3), (1, 2), (2, 3)])
            .expect("valid IV DAG");
        let certificate = identify_instrumental_variable(&dag, 1, 2, 3, &[])
            .expect("IV certificate");
        assert_eq!(certificate.verdict, InstrumentVerdict::Satisfied);
    }

    #[test]
    fn iv_certificate_rejects_direct_effect_on_outcome() {
        let dag = CausalDag::new(4, &[(0, 2), (0, 3), (1, 2), (2, 3), (1, 3)])
            .expect("valid DAG");
        let certificate = identify_instrumental_variable(&dag, 1, 2, 3, &[])
            .expect("IV query");
        assert_eq!(
            certificate.verdict,
            InstrumentVerdict::Violated(InstrumentViolation::ExclusionRestrictionViolated)
        );
    }

    #[test]
    fn iv_certificate_rejects_instrument_outcome_confounding() {
        // W=0 confounds Z=1 and Y=4; U=2 confounds X=3/Y=4.
        let dag = CausalDag::new(
            5,
            &[(0, 1), (0, 4), (2, 3), (2, 4), (1, 3), (3, 4)],
        )
        .expect("valid DAG");
        let certificate = identify_instrumental_variable(&dag, 1, 3, 4, &[])
            .expect("IV query");
        assert_eq!(
            certificate.verdict,
            InstrumentVerdict::Violated(InstrumentViolation::InstrumentOutcomeBackdoorOpen)
        );
    }
}
