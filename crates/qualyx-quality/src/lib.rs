//! Test-quality depth (M6): flaky detection, an independent reviewer that flags weakened
//! tests, mutation testing (cargo-mutants) and API contract testing (Schemathesis).

pub mod contract;
pub mod flaky;
pub mod mutation;
pub mod review;
