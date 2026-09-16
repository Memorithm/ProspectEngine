use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;

const MAX_ASSORTMENT_PRODUCTS: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CustomerError {
    EmptyPlan,
    ProbabilityOutOfRange(u32),
    NegativeField(&'static str),
    UpliftExceedsChurn,
    ArithmeticOverflow(&'static str),
    TooManyProducts { actual: usize, maximum: usize },
    EnumerationBudgetTooSmall { required: u64, maximum: u64 },
    NoFeasibleAssortment,
}

impl fmt::Display for CustomerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPlan => formatter.write_str("customer plan must not be empty"),
            Self::ProbabilityOutOfRange(value) => write!(
                formatter,
                "probability must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::NegativeField(field) => write!(formatter, "{field} must be non-negative"),
            Self::UpliftExceedsChurn => formatter.write_str(
                "retention uplift cannot exceed the baseline churn probability in this model",
            ),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
            Self::TooManyProducts { actual, maximum } => write!(
                formatter,
                "assortment has {actual} products; bounded exact solver supports at most {maximum}"
            ),
            Self::EnumerationBudgetTooSmall { required, maximum } => write!(
                formatter,
                "assortment enumeration requires {required} subsets but budget is {maximum}"
            ),
            Self::NoFeasibleAssortment => formatter.write_str("no feasible assortment exists"),
        }
    }
}

impl std::error::Error for CustomerError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomerPeriod {
    /// Conditional probability that an active customer remains active into this period.
    pub retention_probability_ppm: u32,
    pub margin_if_active_minor: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomerLifetimePlan {
    pub periods: Vec<CustomerPeriod>,
    pub discount_rate_ppm: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomerLifetimeValue {
    pub clv_minor_trunc: i128,
    pub expected_margin_by_period_minor_trunc: Vec<i128>,
    pub terminal_survival_probability_ppm: u32,
}

pub fn customer_lifetime_value(
    plan: &CustomerLifetimePlan,
) -> Result<CustomerLifetimeValue, CustomerError> {
    if plan.periods.is_empty() {
        return Err(CustomerError::EmptyPlan);
    }
    for period in &plan.periods {
        if period.retention_probability_ppm > PROBABILITY_SCALE_PPM {
            return Err(CustomerError::ProbabilityOutOfRange(
                period.retention_probability_ppm,
            ));
        }
        if period.margin_if_active_minor < 0 {
            return Err(CustomerError::NegativeField("margin_if_active_minor"));
        }
    }
    let scale = i128::from(PROBABILITY_SCALE_PPM);
    let discount_denominator = scale
        .checked_add(i128::from(plan.discount_rate_ppm))
        .ok_or(CustomerError::ArithmeticOverflow(
            "CLV discount denominator",
        ))?;
    let mut survival_ppm = i128::from(PROBABILITY_SCALE_PPM);
    let mut discount_factor_ppm = scale;
    let mut expected_by_period = Vec::with_capacity(plan.periods.len());
    let mut total = 0_i128;

    for period in &plan.periods {
        survival_ppm = survival_ppm
            .checked_mul(i128::from(period.retention_probability_ppm))
            .ok_or(CustomerError::ArithmeticOverflow("CLV survival"))?
            / scale;
        let expected_margin = i128::from(period.margin_if_active_minor)
            .checked_mul(survival_ppm)
            .ok_or(CustomerError::ArithmeticOverflow("CLV expected margin"))?
            / scale;
        let discounted = expected_margin
            .checked_mul(discount_factor_ppm)
            .ok_or(CustomerError::ArithmeticOverflow("CLV discounted margin"))?
            / scale;
        expected_by_period.push(discounted);
        total = total
            .checked_add(discounted)
            .ok_or(CustomerError::ArithmeticOverflow("CLV total"))?;
        discount_factor_ppm = discount_factor_ppm
            .checked_mul(scale)
            .ok_or(CustomerError::ArithmeticOverflow("CLV discount factor"))?
            / discount_denominator;
    }

    Ok(CustomerLifetimeValue {
        clv_minor_trunc: total,
        expected_margin_by_period_minor_trunc: expected_by_period,
        terminal_survival_probability_ppm: u32::try_from(survival_ppm)
            .expect("survival probability remains in ppm range"),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChurnRetentionIntervention {
    pub baseline_churn_probability_ppm: u32,
    /// Absolute churn probability reduction attributable to the intervention.
    pub retention_uplift_ppm: u32,
    pub retained_customer_value_minor: i64,
    pub incentive_cost_minor: i64,
    pub contact_cost_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChurnRetentionValue {
    pub expected_saved_value_minor_trunc: i128,
    pub total_certain_cost_minor: i128,
    pub expected_incremental_value_minor_trunc: i128,
    pub residual_churn_probability_ppm: u32,
}

pub fn evaluate_churn_retention(
    intervention: ChurnRetentionIntervention,
) -> Result<ChurnRetentionValue, CustomerError> {
    if intervention.baseline_churn_probability_ppm > PROBABILITY_SCALE_PPM {
        return Err(CustomerError::ProbabilityOutOfRange(
            intervention.baseline_churn_probability_ppm,
        ));
    }
    if intervention.retention_uplift_ppm > intervention.baseline_churn_probability_ppm {
        return Err(CustomerError::UpliftExceedsChurn);
    }
    for (field, value) in [
        (
            "retained_customer_value_minor",
            intervention.retained_customer_value_minor,
        ),
        ("incentive_cost_minor", intervention.incentive_cost_minor),
        ("contact_cost_minor", intervention.contact_cost_minor),
    ] {
        if value < 0 {
            return Err(CustomerError::NegativeField(field));
        }
    }
    let saved = i128::from(intervention.retained_customer_value_minor)
        .checked_mul(i128::from(intervention.retention_uplift_ppm))
        .ok_or(CustomerError::ArithmeticOverflow("saved churn value"))?
        / i128::from(PROBABILITY_SCALE_PPM);
    let cost = i128::from(intervention.incentive_cost_minor)
        .checked_add(i128::from(intervention.contact_cost_minor))
        .ok_or(CustomerError::ArithmeticOverflow("retention cost"))?;
    Ok(ChurnRetentionValue {
        expected_saved_value_minor_trunc: saved,
        total_certain_cost_minor: cost,
        expected_incremental_value_minor_trunc: saved - cost,
        residual_churn_probability_ppm: intervention.baseline_churn_probability_ppm
            - intervention.retention_uplift_ppm,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromotionCandidate {
    pub price_minor: i64,
    pub expected_demand_units: u64,
    pub unit_variable_cost_minor: i64,
    pub campaign_fixed_cost_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromotionValue {
    pub revenue_minor: i128,
    pub operating_profit_minor: i128,
    pub contribution_margin_minor_per_unit: i128,
}

pub fn evaluate_promotion(candidate: PromotionCandidate) -> Result<PromotionValue, CustomerError> {
    for (field, value) in [
        ("price_minor", candidate.price_minor),
        (
            "unit_variable_cost_minor",
            candidate.unit_variable_cost_minor,
        ),
        (
            "campaign_fixed_cost_minor",
            candidate.campaign_fixed_cost_minor,
        ),
    ] {
        if value < 0 {
            return Err(CustomerError::NegativeField(field));
        }
    }
    let revenue = i128::from(candidate.expected_demand_units)
        .checked_mul(i128::from(candidate.price_minor))
        .ok_or(CustomerError::ArithmeticOverflow("promotion revenue"))?;
    let variable_cost = i128::from(candidate.expected_demand_units)
        .checked_mul(i128::from(candidate.unit_variable_cost_minor))
        .ok_or(CustomerError::ArithmeticOverflow("promotion variable cost"))?;
    let profit = revenue
        .checked_sub(variable_cost)
        .and_then(|value| value.checked_sub(i128::from(candidate.campaign_fixed_cost_minor)))
        .ok_or(CustomerError::ArithmeticOverflow("promotion profit"))?;
    Ok(PromotionValue {
        revenue_minor: revenue,
        operating_profit_minor: profit,
        contribution_margin_minor_per_unit: i128::from(candidate.price_minor)
            - i128::from(candidate.unit_variable_cost_minor),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssortmentProduct {
    pub expected_margin_minor: i64,
    pub capital_required_minor: i64,
    pub slot_units: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssortmentProblem {
    pub products: Vec<AssortmentProduct>,
    pub maximum_capital_minor: i64,
    pub maximum_slot_units: u32,
    pub maximum_assignments: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssortmentSolution {
    pub selected_products: Vec<bool>,
    pub expected_margin_minor: i128,
    pub capital_used_minor: i128,
    pub slot_units_used: u64,
}

pub fn select_assortment_exact(
    problem: &AssortmentProblem,
) -> Result<AssortmentSolution, CustomerError> {
    if problem.products.is_empty() {
        return Err(CustomerError::EmptyPlan);
    }
    if problem.maximum_capital_minor < 0
        || problem
            .products
            .iter()
            .any(|product| product.expected_margin_minor < 0 || product.capital_required_minor < 0)
    {
        return Err(CustomerError::NegativeField("assortment economics"));
    }
    if problem.products.len() > MAX_ASSORTMENT_PRODUCTS {
        return Err(CustomerError::TooManyProducts {
            actual: problem.products.len(),
            maximum: MAX_ASSORTMENT_PRODUCTS,
        });
    }
    let required = 1_u64 << problem.products.len();
    if required > problem.maximum_assignments {
        return Err(CustomerError::EnumerationBudgetTooSmall {
            required,
            maximum: problem.maximum_assignments,
        });
    }
    let mut best: Option<AssortmentSolution> = None;
    for mask in 0..required {
        let mut selected = Vec::with_capacity(problem.products.len());
        let mut margin = 0_i128;
        let mut capital = 0_i128;
        let mut slots = 0_u64;
        for (index, product) in problem.products.iter().enumerate() {
            let active = mask & (1_u64 << index) != 0;
            selected.push(active);
            if active {
                margin = margin
                    .checked_add(i128::from(product.expected_margin_minor))
                    .ok_or(CustomerError::ArithmeticOverflow("assortment margin"))?;
                capital = capital
                    .checked_add(i128::from(product.capital_required_minor))
                    .ok_or(CustomerError::ArithmeticOverflow("assortment capital"))?;
                slots = slots
                    .checked_add(u64::from(product.slot_units))
                    .ok_or(CustomerError::ArithmeticOverflow("assortment slots"))?;
            }
        }
        if capital > i128::from(problem.maximum_capital_minor)
            || slots > u64::from(problem.maximum_slot_units)
        {
            continue;
        }
        let candidate = AssortmentSolution {
            selected_products: selected,
            expected_margin_minor: margin,
            capital_used_minor: capital,
            slot_units_used: slots,
        };
        if best.as_ref().is_none_or(|current| {
            candidate.expected_margin_minor > current.expected_margin_minor
                || (candidate.expected_margin_minor == current.expected_margin_minor
                    && candidate.capital_used_minor < current.capital_used_minor)
        }) {
            best = Some(candidate);
        }
    }
    best.ok_or(CustomerError::NoFeasibleAssortment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clv_accounts_for_survival_and_discounting() {
        let value = customer_lifetime_value(&CustomerLifetimePlan {
            periods: vec![
                CustomerPeriod {
                    retention_probability_ppm: 800_000,
                    margin_if_active_minor: 1_000,
                },
                CustomerPeriod {
                    retention_probability_ppm: 500_000,
                    margin_if_active_minor: 1_000,
                },
            ],
            discount_rate_ppm: 0,
        })
        .expect("valid CLV");
        assert_eq!(value.expected_margin_by_period_minor_trunc, vec![800, 400]);
        assert_eq!(value.clv_minor_trunc, 1_200);
        assert_eq!(value.terminal_survival_probability_ppm, 400_000);
    }

    #[test]
    fn churn_intervention_values_incremental_saves() {
        let value = evaluate_churn_retention(ChurnRetentionIntervention {
            baseline_churn_probability_ppm: 300_000,
            retention_uplift_ppm: 100_000,
            retained_customer_value_minor: 10_000,
            incentive_cost_minor: 500,
            contact_cost_minor: 100,
        })
        .expect("valid retention intervention");
        assert_eq!(value.expected_saved_value_minor_trunc, 1_000);
        assert_eq!(value.expected_incremental_value_minor_trunc, 400);
        assert_eq!(value.residual_churn_probability_ppm, 200_000);
    }

    #[test]
    fn promotion_uses_supplied_response_without_inventing_elasticity() {
        let value = evaluate_promotion(PromotionCandidate {
            price_minor: 900,
            expected_demand_units: 120,
            unit_variable_cost_minor: 400,
            campaign_fixed_cost_minor: 10_000,
        })
        .expect("valid promotion");
        assert_eq!(value.revenue_minor, 108_000);
        assert_eq!(value.operating_profit_minor, 50_000);
    }

    #[test]
    fn assortment_solver_obeys_capital_and_slots() {
        let solution = select_assortment_exact(&AssortmentProblem {
            products: vec![
                AssortmentProduct {
                    expected_margin_minor: 100,
                    capital_required_minor: 60,
                    slot_units: 1,
                },
                AssortmentProduct {
                    expected_margin_minor: 90,
                    capital_required_minor: 40,
                    slot_units: 1,
                },
                AssortmentProduct {
                    expected_margin_minor: 80,
                    capital_required_minor: 30,
                    slot_units: 1,
                },
            ],
            maximum_capital_minor: 70,
            maximum_slot_units: 2,
            maximum_assignments: 8,
        })
        .expect("bounded assortment");
        assert_eq!(solution.selected_products, vec![false, true, true]);
        assert_eq!(solution.expected_margin_minor, 170);
        assert_eq!(solution.capital_used_minor, 70);
    }
}
