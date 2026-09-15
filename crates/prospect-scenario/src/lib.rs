#![forbid(unsafe_code)]

pub mod controlled;

use prospect_core::{DecisionPolicy, ProspectiveEngine, Scenario, ScenarioId, SignatureMetric};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioOutcome<I, S> {
    scenario: Scenario<I>,
    signature: S,
}

impl<I, S> ScenarioOutcome<I, S> {
    #[must_use]
    pub const fn scenario(&self) -> &Scenario<I> {
        &self.scenario
    }

    #[must_use]
    pub const fn signature(&self) -> &S {
        &self.signature
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchResult<I, S> {
    baseline: S,
    outcomes: Vec<ScenarioOutcome<I, S>>,
}

impl<I, S> BatchResult<I, S> {
    #[must_use]
    pub const fn baseline(&self) -> &S {
        &self.baseline
    }

    #[must_use]
    pub fn outcomes(&self) -> &[ScenarioOutcome<I, S>] {
        &self.outcomes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioScore<Score> {
    pub scenario_id: ScenarioId,
    pub score: Score,
}

pub fn evaluate_batch<E, State, Intervention>(
    engine: &E,
    state: &State,
    scenarios: Vec<Scenario<Intervention>>,
) -> Result<BatchResult<Intervention, E::Signature>, E::Error>
where
    E: ProspectiveEngine<State, Intervention> + ?Sized,
{
    let baseline = engine.baseline(state)?;
    let mut outcomes = Vec::with_capacity(scenarios.len());

    for scenario in scenarios {
        let signature = engine.evaluate(state, scenario.intervention())?;
        outcomes.push(ScenarioOutcome {
            scenario,
            signature,
        });
    }

    Ok(BatchResult { baseline, outcomes })
}

#[must_use]
pub fn score_against_baseline<I, S, M>(
    batch: &BatchResult<I, S>,
    metric: &M,
) -> Vec<ScenarioScore<M::Score>>
where
    M: SignatureMetric<S> + ?Sized,
{
    batch
        .outcomes()
        .iter()
        .map(|outcome| ScenarioScore {
            scenario_id: outcome.scenario().id().clone(),
            score: metric.compare(batch.baseline(), outcome.signature()),
        })
        .collect()
}

pub fn best_by_policy<'a, I, S, P>(
    batch: &'a BatchResult<I, S>,
    policy: &P,
) -> Option<(&'a ScenarioOutcome<I, S>, P::Score)>
where
    P: DecisionPolicy<S> + ?Sized,
{
    let mut outcomes = batch.outcomes().iter();
    let first = outcomes.next()?;
    let mut best = first;
    let mut best_score = policy.utility(first.signature());

    for candidate in outcomes {
        let score = policy.utility(candidate.signature());
        if score > best_score {
            best = candidate;
            best_score = score;
        }
    }

    Some((best, best_score))
}

#[cfg(test)]
mod tests {
    use prospect_core::{DecisionPolicy, ProspectiveEngine, Scenario, ScenarioId, SignatureMetric};

    use super::{best_by_policy, evaluate_batch, score_against_baseline};

    struct AdditiveEngine;

    impl ProspectiveEngine<i32, i32> for AdditiveEngine {
        type Signature = i32;
        type Error = core::convert::Infallible;

        fn baseline(&self, state: &i32) -> Result<Self::Signature, Self::Error> {
            Ok(*state)
        }

        fn evaluate(
            &self,
            state: &i32,
            intervention: &i32,
        ) -> Result<Self::Signature, Self::Error> {
            Ok(*state + *intervention)
        }
    }

    struct AbsoluteDistance;

    impl SignatureMetric<i32> for AbsoluteDistance {
        type Score = i32;

        fn compare(&self, reference: &i32, candidate: &i32) -> Self::Score {
            (candidate - reference).abs()
        }
    }

    struct PreferHigher;

    impl DecisionPolicy<i32> for PreferHigher {
        type Score = i32;

        fn utility(&self, signature: &i32) -> Self::Score {
            *signature
        }
    }

    #[test]
    fn evaluates_and_ranks_candidate_interventions() {
        let scenarios = vec![
            Scenario::new(ScenarioId::new("small").expect("id"), 2),
            Scenario::new(ScenarioId::new("large").expect("id"), 7),
        ];

        let batch = evaluate_batch(&AdditiveEngine, &10, scenarios).expect("infallible");
        assert_eq!(*batch.baseline(), 10);

        let scores = score_against_baseline(&batch, &AbsoluteDistance);
        assert_eq!(scores[0].score, 2);
        assert_eq!(scores[1].score, 7);

        let (best, score) = best_by_policy(&batch, &PreferHigher).expect("non-empty batch");
        assert_eq!(best.scenario().id().as_str(), "large");
        assert_eq!(score, 17);
    }
}
