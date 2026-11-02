// SPDX-License-Identifier: AGPL-3.0-or-later
//! Studio application services shared by native frontends.
//!
//! No Tauri, Slint, FFmpeg, Python, browser or model runtime dependency.
//! Engines and authenticated providers are connected explicitly, not on startup.

pub mod ai;
pub mod auth;
pub mod browser;
pub mod bulk_import;
pub mod download_browser;
pub mod edit_bridge;
pub mod generation;
pub mod health;
pub mod hotclip;
pub mod kedi_live;
pub mod kedi_control;
pub mod kedi_entitlements;
pub mod kedi_outbox;
pub mod legacy_keys;
pub mod lipsync;
pub mod live;
pub mod live_adapters;
pub mod live_brain;
pub mod live_director;
pub mod live_runtime;
pub mod temple_live;
pub mod temple_runtime;
pub mod live_scene;
pub mod live_voice;
pub mod local_ai;
pub mod local_avatars;
pub mod lux;
pub mod motion_generation;
pub mod native_pipeline;
pub mod nexora;
pub mod opentalking;
pub mod pages;
pub mod production;
pub mod production_pages;
pub mod production_runtime;
pub mod production_worker;
pub mod products;
pub mod promax_avatars;
pub mod promax_worker;
mod promax_worker_ledger;
mod promax_worker_protocol;
pub mod scripts;
pub mod shorts_jobs;
pub mod storyboard;
pub mod storyboard_render;
pub mod video_library;
pub mod workflows;
pub mod provider_registry;
pub mod kedi_factory;
pub mod factory_product;
pub mod factory_edit;
pub mod ai_control;
pub mod control_plan;
pub mod control_batch;
pub mod campaign;
pub mod control_display;
pub mod control_web;
pub mod codex_runtime;
