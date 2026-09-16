use crate::optimization::ObjectiveDirection;
use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq)]
pub enum EvolutionError {
    InvalidPopulationSize,
    InvalidGenerationCount,
    InvalidDimensions,
    InvalidBounds,
    InvalidRatePpm(u32),
    InvalidMutationSigma,
    ObjectiveWidthMismatch,
    NonFiniteObjective,
}

impl fmt::Display for EvolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPopulationSize => {
                formatter.write_str("NSGA-II population size must be at least two")
            }
            Self::InvalidGenerationCount => {
                formatter.write_str("NSGA-II generation count must be non-zero")
            }
            Self::InvalidDimensions => {
                formatter.write_str("NSGA-II genome and objective dimensions must be non-zero")
            }
            Self::InvalidBounds => {
                formatter.write_str("NSGA-II bounds must be finite with lower < upper")
            }
            Self::InvalidRatePpm(value) => write!(
                formatter,
                "NSGA-II probability must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::InvalidMutationSigma => {
                formatter.write_str("NSGA-II mutation sigma must be finite and non-negative")
            }
            Self::ObjectiveWidthMismatch => {
                formatter.write_str("objective function returned the wrong width")
            }
            Self::NonFiniteObjective => {
                formatter.write_str("objective function returned a non-finite value")
            }
        }
    }
}

impl std::error::Error for EvolutionError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nsga2Config {
    pub population_size: usize,
    pub generations: usize,
    pub crossover_rate_ppm: u32,
    pub mutation_rate_ppm: u32,
    pub mutation_sigma: f64,
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub seed: u64,
}

impl Default for Nsga2Config {
    fn default() -> Self {
        Self {
            population_size: 64,
            generations: 50,
            crossover_rate_ppm: 900_000,
            mutation_rate_ppm: 100_000,
            mutation_sigma: 0.1,
            lower_bound: -1.0,
            upper_bound: 1.0,
            seed: 0x5052_4f53_5045_4354,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Nsga2Individual {
    pub genome: Vec<f64>,
    pub objectives: Vec<f64>,
    pub rank: usize,
    pub crowding_distance: f64,
}

pub fn run_nsga2<F>(
    config: Nsga2Config,
    genome_dimensions: usize,
    directions: &[ObjectiveDirection],
    evaluate: F,
) -> Result<Vec<Nsga2Individual>, EvolutionError>
where
    F: Fn(&[f64]) -> Vec<f64>,
{
    validate_config(config, genome_dimensions, directions)?;
    let mut rng = DeterministicRng::new(config.seed);
    let mut population: Vec<Nsga2Individual> = (0..config.population_size)
        .map(|_| Nsga2Individual {
            genome: (0..genome_dimensions)
                .map(|_| rng.uniform(config.lower_bound, config.upper_bound))
                .collect(),
            objectives: Vec::new(),
            rank: 0,
            crowding_distance: 0.0,
        })
        .collect();
    evaluate_population(&mut population, directions.len(), &evaluate)?;

    for _ in 0..config.generations {
        rank_and_crowd(&mut population, directions);
        let mut offspring = Vec::with_capacity(config.population_size);
        while offspring.len() < config.population_size {
            let left = tournament(&population, &mut rng);
            let right = tournament(&population, &mut rng);
            let (mut first, mut second) = crossover(
                &population[left].genome,
                &population[right].genome,
                config,
                &mut rng,
            );
            mutate(&mut first, config, &mut rng);
            mutate(&mut second, config, &mut rng);
            offspring.push(Nsga2Individual {
                genome: first,
                objectives: Vec::new(),
                rank: 0,
                crowding_distance: 0.0,
            });
            if offspring.len() < config.population_size {
                offspring.push(Nsga2Individual {
                    genome: second,
                    objectives: Vec::new(),
                    rank: 0,
                    crowding_distance: 0.0,
                });
            }
        }
        evaluate_population(&mut offspring, directions.len(), &evaluate)?;
        population.append(&mut offspring);
        rank_and_crowd(&mut population, directions);
        population.sort_by(compare_survival);
        population.truncate(config.population_size);
    }

    rank_and_crowd(&mut population, directions);
    population.sort_by(compare_survival);
    Ok(population)
}

fn validate_config(
    config: Nsga2Config,
    genome_dimensions: usize,
    directions: &[ObjectiveDirection],
) -> Result<(), EvolutionError> {
    if config.population_size < 2 {
        return Err(EvolutionError::InvalidPopulationSize);
    }
    if config.generations == 0 {
        return Err(EvolutionError::InvalidGenerationCount);
    }
    if genome_dimensions == 0 || directions.is_empty() {
        return Err(EvolutionError::InvalidDimensions);
    }
    if !config.lower_bound.is_finite()
        || !config.upper_bound.is_finite()
        || config.lower_bound >= config.upper_bound
    {
        return Err(EvolutionError::InvalidBounds);
    }
    for value in [config.crossover_rate_ppm, config.mutation_rate_ppm] {
        if value > PROBABILITY_SCALE_PPM {
            return Err(EvolutionError::InvalidRatePpm(value));
        }
    }
    if !config.mutation_sigma.is_finite() || config.mutation_sigma < 0.0 {
        return Err(EvolutionError::InvalidMutationSigma);
    }
    Ok(())
}

fn evaluate_population<F>(
    population: &mut [Nsga2Individual],
    objective_count: usize,
    evaluate: &F,
) -> Result<(), EvolutionError>
where
    F: Fn(&[f64]) -> Vec<f64>,
{
    for individual in population {
        let objectives = evaluate(&individual.genome);
        if objectives.len() != objective_count {
            return Err(EvolutionError::ObjectiveWidthMismatch);
        }
        if objectives.iter().any(|value| !value.is_finite()) {
            return Err(EvolutionError::NonFiniteObjective);
        }
        individual.objectives = objectives;
    }
    Ok(())
}

fn crossover(
    left: &[f64],
    right: &[f64],
    config: Nsga2Config,
    rng: &mut DeterministicRng,
) -> (Vec<f64>, Vec<f64>) {
    if left.len() <= 1 || rng.ppm() >= config.crossover_rate_ppm {
        return (left.to_vec(), right.to_vec());
    }
    let point = 1 + rng.index(left.len() - 1);
    let mut first = left[..point].to_vec();
    first.extend_from_slice(&right[point..]);
    let mut second = right[..point].to_vec();
    second.extend_from_slice(&left[point..]);
    (first, second)
}

fn mutate(genome: &mut [f64], config: Nsga2Config, rng: &mut DeterministicRng) {
    let scale = (config.upper_bound - config.lower_bound) * config.mutation_sigma;
    for gene in genome {
        if rng.ppm() < config.mutation_rate_ppm {
            *gene = (*gene + rng.standard_normal() * scale)
                .clamp(config.lower_bound, config.upper_bound);
        }
    }
}

fn tournament(population: &[Nsga2Individual], rng: &mut DeterministicRng) -> usize {
    let first = rng.index(population.len());
    let second = rng.index(population.len());
    match compare_survival(&population[first], &population[second]) {
        Ordering::Less => first,
        Ordering::Greater => second,
        Ordering::Equal => first.min(second),
    }
}

fn compare_survival(left: &Nsga2Individual, right: &Nsga2Individual) -> Ordering {
    left.rank
        .cmp(&right.rank)
        .then_with(|| {
            right
                .crowding_distance
                .partial_cmp(&left.crowding_distance)
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| compare_genomes(&left.genome, &right.genome))
}

fn compare_genomes(left: &[f64], right: &[f64]) -> Ordering {
    left.iter()
        .zip(right)
        .map(|(a, b)| a.total_cmp(b))
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or_else(|| left.len().cmp(&right.len()))
}

fn rank_and_crowd(population: &mut [Nsga2Individual], directions: &[ObjectiveDirection]) {
    let fronts = non_dominated_fronts(population, directions);
    for (rank, front) in fronts.iter().enumerate() {
        for &index in front {
            population[index].rank = rank;
            population[index].crowding_distance = 0.0;
        }
        assign_crowding(population, front);
    }
}

fn non_dominated_fronts(
    population: &[Nsga2Individual],
    directions: &[ObjectiveDirection],
) -> Vec<Vec<usize>> {
    let mut domination_count = vec![0_usize; population.len()];
    let mut dominates_set = vec![Vec::<usize>::new(); population.len()];
    let mut first_front = Vec::new();
    for left in 0..population.len() {
        for right in 0..population.len() {
            if left == right {
                continue;
            }
            if dominates(&population[left], &population[right], directions) {
                dominates_set[left].push(right);
            } else if dominates(&population[right], &population[left], directions) {
                domination_count[left] += 1;
            }
        }
        if domination_count[left] == 0 {
            first_front.push(left);
        }
    }
    let mut fronts = Vec::new();
    let mut current = first_front;
    while !current.is_empty() {
        current.sort_unstable();
        let mut next = Vec::new();
        for index in &current {
            for dominated in &dominates_set[*index] {
                domination_count[*dominated] -= 1;
                if domination_count[*dominated] == 0 {
                    next.push(*dominated);
                }
            }
        }
        next.sort_unstable();
        next.dedup();
        fronts.push(current);
        current = next;
    }
    fronts
}

fn dominates(
    left: &Nsga2Individual,
    right: &Nsga2Individual,
    directions: &[ObjectiveDirection],
) -> bool {
    let mut strictly_better = false;
    for ((left_value, right_value), direction) in left
        .objectives
        .iter()
        .zip(&right.objectives)
        .zip(directions)
    {
        let ordering = match direction {
            ObjectiveDirection::Minimize => right_value.total_cmp(left_value),
            ObjectiveDirection::Maximize => left_value.total_cmp(right_value),
        };
        if ordering == Ordering::Less {
            return false;
        }
        if ordering == Ordering::Greater {
            strictly_better = true;
        }
    }
    strictly_better
}

fn assign_crowding(population: &mut [Nsga2Individual], front: &[usize]) {
    if front.len() <= 2 {
        for index in front {
            population[*index].crowding_distance = f64::INFINITY;
        }
        return;
    }
    let objective_count = population[front[0]].objectives.len();
    for objective in 0..objective_count {
        let mut ordered = front.to_vec();
        ordered.sort_by(|left, right| {
            population[*left].objectives[objective]
                .total_cmp(&population[*right].objectives[objective])
                .then_with(|| left.cmp(right))
        });
        let first = ordered[0];
        let last = *ordered.last().expect("front is non-empty");
        population[first].crowding_distance = f64::INFINITY;
        population[last].crowding_distance = f64::INFINITY;
        let minimum = population[first].objectives[objective];
        let maximum = population[last].objectives[objective];
        let range = maximum - minimum;
        if range == 0.0 {
            continue;
        }
        for window in ordered.windows(3) {
            let middle = window[1];
            if population[middle].crowding_distance.is_infinite() {
                continue;
            }
            let previous = population[window[0]].objectives[objective];
            let next = population[window[2]].objectives[objective];
            population[middle].crowding_distance += (next - previous).abs() / range.abs();
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.state = value;
        value.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn unit(&mut self) -> f64 {
        let bits = self.next_u64() >> 11;
        (bits as f64) / ((1_u64 << 53) as f64)
    }

    fn uniform(&mut self, lower: f64, upper: f64) -> f64 {
        lower + self.unit() * (upper - lower)
    }

    fn index(&mut self, upper: usize) -> usize {
        debug_assert!(upper > 0);
        (self.next_u64() % u64::try_from(upper).expect("usize fits u64")) as usize
    }

    fn ppm(&mut self) -> u32 {
        (self.next_u64() % u64::from(PROBABILITY_SCALE_PPM)) as u32
    }

    fn standard_normal(&mut self) -> f64 {
        let first = self.unit().max(f64::MIN_POSITIVE);
        let second = self.unit();
        (-2.0 * first.ln()).sqrt() * (std::f64::consts::TAU * second).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Nsga2Config {
        Nsga2Config {
            population_size: 24,
            generations: 12,
            crossover_rate_ppm: 900_000,
            mutation_rate_ppm: 250_000,
            mutation_sigma: 0.08,
            lower_bound: -1.0,
            upper_bound: 3.0,
            seed: 42,
        }
    }

    #[test]
    fn generational_nsga2_is_reproducible_and_elitist() {
        let evaluate = |genome: &[f64]| {
            let x = genome[0];
            vec![x * x, (x - 2.0) * (x - 2.0)]
        };
        let first = run_nsga2(
            config(),
            1,
            &[ObjectiveDirection::Minimize, ObjectiveDirection::Minimize],
            evaluate,
        )
        .expect("valid NSGA-II run");
        let second = run_nsga2(
            config(),
            1,
            &[ObjectiveDirection::Minimize, ObjectiveDirection::Minimize],
            evaluate,
        )
        .expect("reproducible NSGA-II run");
        assert_eq!(first, second);
        assert_eq!(first.len(), 24);
        assert!(
            first
                .iter()
                .all(|candidate| candidate.objectives.iter().all(|v| v.is_finite()))
        );
        assert!(first.iter().filter(|candidate| candidate.rank == 0).count() >= 2);
    }

    #[test]
    fn objective_width_mismatch_fails_closed() {
        let error = run_nsga2(
            config(),
            1,
            &[ObjectiveDirection::Minimize, ObjectiveDirection::Minimize],
            |_| vec![1.0],
        )
        .expect_err("wrong objective width must fail");
        assert_eq!(error, EvolutionError::ObjectiveWidthMismatch);
    }
}
