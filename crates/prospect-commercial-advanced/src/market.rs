use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarketError {
    EmptyProblem,
    RaggedPayoffMatrix,
    ProbabilityMassMismatch { actual_ppm: u64 },
    InvalidBidProbability(u32),
    NegativeField(&'static str),
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for MarketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProblem => formatter.write_str("market decision problem must not be empty"),
            Self::RaggedPayoffMatrix => {
                formatter.write_str("competitive payoff matrix must be rectangular")
            }
            Self::ProbabilityMassMismatch { actual_ppm } => write!(
                formatter,
                "competitor probabilities must sum to {PROBABILITY_SCALE_PPM} ppm, got {actual_ppm} ppm"
            ),
            Self::InvalidBidProbability(value) => write!(
                formatter,
                "bid win probability must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::NegativeField(field) => write!(formatter, "{field} must be non-negative"),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
        }
    }
}

impl std::error::Error for MarketError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompetitiveResponseProblem {
    /// Rows are our actions, columns are competitor responses.
    pub payoff_minor: Vec<Vec<i64>>,
    /// Optional explicit probability for each competitor response.
    pub competitor_response_probability_ppm: Option<Vec<u32>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompetitiveActionValue {
    pub action_index: usize,
    pub expected_payoff_minor_trunc: Option<i128>,
    pub worst_case_payoff_minor: i64,
    pub best_case_payoff_minor: i64,
}

pub fn evaluate_competitive_actions(
    problem: &CompetitiveResponseProblem,
) -> Result<Vec<CompetitiveActionValue>, MarketError> {
    if problem.payoff_minor.is_empty() || problem.payoff_minor[0].is_empty() {
        return Err(MarketError::EmptyProblem);
    }
    let response_count = problem.payoff_minor[0].len();
    if problem
        .payoff_minor
        .iter()
        .any(|row| row.len() != response_count)
    {
        return Err(MarketError::RaggedPayoffMatrix);
    }
    if let Some(probabilities) = &problem.competitor_response_probability_ppm {
        if probabilities.len() != response_count {
            return Err(MarketError::RaggedPayoffMatrix);
        }
        let actual_ppm: u64 = probabilities.iter().map(|value| u64::from(*value)).sum();
        if actual_ppm != u64::from(PROBABILITY_SCALE_PPM) {
            return Err(MarketError::ProbabilityMassMismatch { actual_ppm });
        }
    }

    problem
        .payoff_minor
        .iter()
        .enumerate()
        .map(|(action_index, row)| {
            let expected = if let Some(probabilities) = &problem.competitor_response_probability_ppm {
                let weighted = row
                    .iter()
                    .zip(probabilities)
                    .try_fold(0_i128, |sum, (payoff, probability)| {
                        let term = i128::from(*payoff)
                            .checked_mul(i128::from(*probability))
                            .ok_or(MarketError::ArithmeticOverflow("competitive expected payoff"))?;
                        sum.checked_add(term)
                            .ok_or(MarketError::ArithmeticOverflow("competitive expected payoff"))
                    })?;
                Some(weighted / i128::from(PROBABILITY_SCALE_PPM))
            } else {
                None
            };
            Ok(CompetitiveActionValue {
                action_index,
                expected_payoff_minor_trunc: expected,
                worst_case_payoff_minor: *row.iter().min().expect("validated non-empty row"),
                best_case_payoff_minor: *row.iter().max().expect("validated non-empty row"),
            })
        })
        .collect()
}

pub fn best_expected_competitive_action(
    problem: &CompetitiveResponseProblem,
) -> Result<Option<CompetitiveActionValue>, MarketError> {
    let values = evaluate_competitive_actions(problem)?;
    Ok(values
        .into_iter()
        .filter(|value| value.expected_payoff_minor_trunc.is_some())
        .max_by_key(|value| (value.expected_payoff_minor_trunc, value.worst_case_payoff_minor)))
}

pub fn maximin_competitive_action(
    problem: &CompetitiveResponseProblem,
) -> Result<CompetitiveActionValue, MarketError> {
    evaluate_competitive_actions(problem)?
        .into_iter()
        .max_by_key(|value| (value.worst_case_payoff_minor, value.best_case_payoff_minor))
        .ok_or(MarketError::EmptyProblem)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BidCandidate {
    pub bid_minor: i64,
    pub win_probability_ppm: u32,
    /// Economic value created if the contract/opportunity is won, before paying the bid.
    pub gross_value_if_won_minor: i64,
    pub participation_cost_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BidValue {
    pub bid_minor: i64,
    pub win_probability_ppm: u32,
    pub profit_if_won_minor: i128,
    pub expected_profit_minor_trunc: i128,
}

pub fn evaluate_first_price_bid(candidate: BidCandidate) -> Result<BidValue, MarketError> {
    if candidate.win_probability_ppm > PROBABILITY_SCALE_PPM {
        return Err(MarketError::InvalidBidProbability(candidate.win_probability_ppm));
    }
    for (field, value) in [
        ("bid_minor", candidate.bid_minor),
        ("gross_value_if_won_minor", candidate.gross_value_if_won_minor),
        ("participation_cost_minor", candidate.participation_cost_minor),
    ] {
        if value < 0 {
            return Err(MarketError::NegativeField(field));
        }
    }
    let profit_if_won = i128::from(candidate.gross_value_if_won_minor)
        .checked_sub(i128::from(candidate.bid_minor))
        .and_then(|value| value.checked_sub(i128::from(candidate.participation_cost_minor)))
        .ok_or(MarketError::ArithmeticOverflow("bid profit"))?;
    let loss_if_lost = -i128::from(candidate.participation_cost_minor);
    let win_weight = i128::from(candidate.win_probability_ppm);
    let lose_weight = i128::from(PROBABILITY_SCALE_PPM - candidate.win_probability_ppm);
    let expected_weighted = profit_if_won
        .checked_mul(win_weight)
        .and_then(|value| value.checked_add(loss_if_lost * lose_weight))
        .ok_or(MarketError::ArithmeticOverflow("bid expected profit"))?;
    Ok(BidValue {
        bid_minor: candidate.bid_minor,
        win_probability_ppm: candidate.win_probability_ppm,
        profit_if_won_minor: profit_if_won,
        expected_profit_minor_trunc: expected_weighted / i128::from(PROBABILITY_SCALE_PPM),
    })
}

pub fn select_best_first_price_bid(
    candidates: &[BidCandidate],
) -> Result<BidValue, MarketError> {
    if candidates.is_empty() {
        return Err(MarketError::EmptyProblem);
    }
    let mut best: Option<BidValue> = None;
    for candidate in candidates {
        let value = evaluate_first_price_bid(*candidate)?;
        if best.as_ref().is_none_or(|current| {
            value.expected_profit_minor_trunc > current.expected_profit_minor_trunc
                || (value.expected_profit_minor_trunc == current.expected_profit_minor_trunc
                    && value.bid_minor < current.bid_minor)
        }) {
            best = Some(value);
        }
    }
    Ok(best.expect("non-empty candidates"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn competitive_engine_supports_expected_and_robust_views() {
        let problem = CompetitiveResponseProblem {
            payoff_minor: vec![vec![100, 20], vec![70, 60]],
            competitor_response_probability_ppm: Some(vec![500_000, 500_000]),
        };
        let expected = best_expected_competitive_action(&problem)
            .expect("valid game")
            .expect("probabilities supplied");
        assert_eq!(expected.action_index, 1);
        assert_eq!(expected.expected_payoff_minor_trunc, Some(65));
        let robust = maximin_competitive_action(&problem).expect("valid game");
        assert_eq!(robust.action_index, 1);
        assert_eq!(robust.worst_case_payoff_minor, 60);
    }

    #[test]
    fn bid_engine_selects_expected_profit_not_highest_win_rate() {
        let best = select_best_first_price_bid(&[
            BidCandidate {
                bid_minor: 80,
                win_probability_ppm: 900_000,
                gross_value_if_won_minor: 100,
                participation_cost_minor: 2,
            },
            BidCandidate {
                bid_minor: 50,
                win_probability_ppm: 500_000,
                gross_value_if_won_minor: 100,
                participation_cost_minor: 2,
            },
        ])
        .expect("valid bids");
        assert_eq!(best.bid_minor, 50);
        assert_eq!(best.expected_profit_minor_trunc, 23);
    }
}
