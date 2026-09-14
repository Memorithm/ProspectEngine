#![forbid(unsafe_code)]

use prospect_core::ProspectiveEngine;
use tdi_core::{Action, ExploreError, SignatureError, State, StateError, TdiSignature, TransitionSystem, explore};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmptySchedule;

#[derive(Debug)]
pub enum TdiProspectError<E> {
    Intervention(StateError),
    Explore(ExploreError<E>),
    Signature(SignatureError),
}

/// Thin adapter over the exact, frozen TDI finite-state primitives.
///
/// ProspectEngine owns orchestration and policy. It does not reimplement the
/// scientific exploration/signature logic from `tdi-core`.
pub struct TdiEngine<'a, S> {
    system: &'a S,
    actions: &'a [Action],
}

impl<'a, S> TdiEngine<'a, S> {
    pub fn new(system: &'a S, actions: &'a [Action]) -> Result<Self, EmptySchedule> {
        if actions.is_empty() {
            return Err(EmptySchedule);
        }
        Ok(Self { system, actions })
    }

    #[must_use]
    pub const fn actions(&self) -> &[Action] {
        self.actions
    }
}

impl<S> TdiEngine<'_, S>
where
    S: TransitionSystem,
{
    fn signature_from_state(&self, state: State) -> Result<TdiSignature, TdiProspectError<S::Error>> {
        let report = explore(self.system, state, self.actions).map_err(TdiProspectError::Explore)?;
        TdiSignature::from_report(&report).map_err(TdiProspectError::Signature)
    }
}

impl<S> ProspectiveEngine<State, Action> for TdiEngine<'_, S>
where
    S: TransitionSystem,
{
    type Signature = TdiSignature;
    type Error = TdiProspectError<S::Error>;

    fn baseline(&self, state: &State) -> Result<Self::Signature, Self::Error> {
        self.signature_from_state(*state)
    }

    fn evaluate(
        &self,
        state: &State,
        intervention: &Action,
    ) -> Result<Self::Signature, Self::Error> {
        let intervened = intervention
            .apply(*state)
            .map_err(TdiProspectError::Intervention)?;
        self.signature_from_state(intervened)
    }
}

#[cfg(test)]
mod tests {
    use prospect_core::ProspectiveEngine;
    use tdi_core::{Action, State, TableSystem};

    use super::{EmptySchedule, TdiEngine};

    #[test]
    fn rejects_an_empty_prospective_schedule() {
        let system = TableSystem::new(1).expect("valid system");
        assert!(matches!(TdiEngine::new(&system, &[]), Err(EmptySchedule)));
    }

    #[test]
    fn compares_baseline_and_intervened_future_without_reimplementing_tdi() {
        let zero = State::new(0, 1).expect("valid state");
        let one = State::new(1, 1).expect("valid state");
        let mut system = TableSystem::new(1).expect("valid system");
        system
            .insert(zero, Action::Noop, vec![zero])
            .expect("transition");
        system
            .insert(one, Action::Noop, vec![zero])
            .expect("transition");

        let schedule = [Action::Noop];
        let engine = TdiEngine::new(&system, &schedule).expect("non-empty schedule");
        let baseline = engine.baseline(&zero).expect("baseline signature");
        let intervened = engine
            .evaluate(&zero, &Action::Flip { node: 0 })
            .expect("intervened signature");

        assert_eq!(baseline.reachable_profile(), &[1]);
        assert_eq!(baseline.path_profile(), &[1]);
        assert_eq!(baseline.return_profile()[0].components_u128(), Some((1, 1)));
        assert_eq!(intervened.reachable_profile(), &[1]);
        assert_eq!(intervened.path_profile(), &[1]);
        assert_eq!(intervened.return_profile()[0].components_u128(), Some((0, 1)));
    }
}
