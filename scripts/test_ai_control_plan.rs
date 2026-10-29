//! Isolated compiler tests; this does not register a native model planner.
#![allow(dead_code)]
mod ai_control {
    pub use concat_studio::ai_control::*;
}
#[path = "../src/crates/concat-studio/src/control_plan.rs"]
mod control_plan;
