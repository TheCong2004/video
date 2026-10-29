use concat_studio::factory_edit::{self,EditRequest};
use std::{path::PathBuf,sync::atomic::AtomicBool};
fn repo()->PathBuf {std::env::var_os("KEDI_TEST_REPO").unwrap().into()}
fn request()->EditRequest {
 let r=repo(); EditRequest {paths:vec![r.join(".runtime/factory-shopee-check/vn-11134207-820l4-meyda3stmo0404.jpg")],seconds:0.5,size:(720,1280),script:"Khoai sâm đất. Gói một ki lô gam.".into(),audio_mode:1,voice_index:0,audio_path:r.join("dist/kedi-test/review-20261028/khoai-sam-dat-voice.mp3").display().to_string()}
}
fn ffmpeg()->PathBuf {std::env::var_os("CONCAT_TEST_FFMPEG").unwrap().into()}
fn font()->PathBuf {repo().join("src/crates/concat-text/fonts/HankenGrotesk-Regular.ttf")}
fn root()->PathBuf {repo().join(".runtime/factory-edit-adapter-check").join(format!("dựng-thử-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))}
#[test] fn owned_adapter_renders_embedded_worker_unicode_and_full_audio() {
 let r=root(); let mut progress=vec![];
 let output=factory_edit::render(&factory_edit::python(&repo()),&ffmpeg(),&font(),&request(),&r,&AtomicBool::new(false),|s|progress.push(s)).unwrap();
 assert!(output.metadata().unwrap().len()>10000); assert!(r.join("edit-manifest.json").is_file());
 assert!(progress.iter().any(|s|s.contains("dựng cảnh")));
 println!("VERIFIED_ADAPTER_OUTPUT={}",output.display());
}
#[test] fn validation_does_not_create_output_for_missing_speech() {
 let mut q=request();q.audio_mode=0;q.script.clear();let r=root();
 assert!(factory_edit::render(&factory_edit::python(&repo()),&ffmpeg(),&font(),&q,&r,&AtomicBool::new(false),|_|{}).is_err());
 assert!(!r.exists());
}
#[test] fn cancelled_worker_cannot_report_completed_video() {
 let r=root();
 assert!(factory_edit::render(&factory_edit::python(&repo()),&ffmpeg(),&font(),&request(),&r,&AtomicBool::new(true),|_|{}).is_err());
 assert!(!r.join("film.mp4").exists());
}
