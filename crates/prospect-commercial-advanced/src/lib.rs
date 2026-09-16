#![forbid(unsafe_code)]

//! Advanced commercial decision engines layered on top of `prospect-commercial`.
//!
//! These engines evaluate caller-supplied assumptions and observations. They do
//! not fabricate forecasts, causal effects, probabilities, elasticities or
//! financial guarantees.

pub mod finance;
pub mod inventory;
pub mod mcda;
pub mod next_best_action;
pub mod pricing;
pub mod robust;
pub mod threshold;
