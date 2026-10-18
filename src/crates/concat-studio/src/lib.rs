// SPDX-License-Identifier: AGPL-3.0-or-later
//! Studio application services shared by native frontends.
//!
//! No Tauri, Slint, FFmpeg, Python, browser or model runtime dependency.
//! Engines and authenticated providers are connected explicitly, not on startup.

pub mod ai;
pub mod auth;
pub mod browser;
pub mod bulk_import;
pub mod edit_bridge;
pub mod generation;
pub mod health;
pub mod legacy_keys;
pub mod live;
pub mod live_runtime;
pub mod local_ai;
pub mod nexora;
pub mod opentalking;
pub mod pages;
pub mod products;
pub mod scripts;
pub mod shorts_jobs;
pub mod workflows;
