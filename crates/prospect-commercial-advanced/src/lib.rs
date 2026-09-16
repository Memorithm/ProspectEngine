#![forbid(unsafe_code)]

//! Advanced commercial decision engines layered on top of `prospect-commercial`.
//!
//! These engines evaluate caller-supplied assumptions and observations. They do
//! not fabricate forecasts, causal effects, probabilities, elasticities or
//! financial guarantees.

pub mod adaptive;
pub mod adaptive_bayes;
pub mod causal;
pub mod causal_advanced;
pub mod constraint_propagation;
pub mod customer;
pub mod evolution;
pub mod finance;
pub mod forecast_advanced;
pub mod integer_solver;
pub mod inventory;
pub mod learning;
pub mod market;
pub mod mcda;
pub mod next_best_action;
pub mod operations;
pub mod optimization;
pub mod pricing;
pub mod robust;
pub mod solver;
pub mod stress;
pub mod threshold;
pub mod timeseries;
