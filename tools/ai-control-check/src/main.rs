//! Compile-only harness, NOT a production runtime or proof of editor execution.
//! Compile the actual UI/adapter without rechecking the enormous entire window.
#![allow(dead_code)]
mod ui {slint::include_modules!();}
#[path="../../../src/crates/concat/src/studio_ai_control.rs"]mod studio_ai_control;
mod studio_local_ai {pub fn client()->Result<concat_studio::workflows::WorkflowClient,String>{Err("compile-only harness has no credentials".into())}}
mod ai_editing {
    pub fn external_context(_: &crate::ui::App)->Result<serde_json::Value,String>{Err("compile-only harness has no live editor".into())}
    pub fn apply_control(_: &crate::ui::App,_:&str,_:&serde_json::Value)->Result<serde_json::Value,String>{Err("compile-only harness has no live editor".into())}
}
mod studio_scenes {
    pub fn ffmpeg()->Result<std::path::PathBuf,String>{Err("compile-only harness".into())}
    pub fn font()->Result<std::path::PathBuf,String>{Err("compile-only harness".into())}
}
fn main(){eprintln!("Use cargo check. This compile-only harness does not execute workflows.");}
