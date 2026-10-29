// SPDX-License-Identifier: AGPL-3.0-or-later
//! Versioned campaign control, independent of window lifetime. Engine/account
//! implementations remain owned by their existing adapters.
pub mod engine;
pub mod facebook;
pub mod model;
pub mod progress;
pub mod quality;
pub mod schedule;
pub mod store;
pub mod worker;
pub use model::*;
pub use store::CampaignStore;
#[cfg(test)]
mod tests;
