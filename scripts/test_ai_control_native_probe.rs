//! Verify the exact native probe used by Control, with actual supplied assets.
use std::path::PathBuf;
fn asset(name: &str) -> PathBuf {
    std::env::var_os(name)
        .unwrap_or_else(|| panic!("Missing {name}"))
        .into()
}
#[test]
fn native_probe_reads_real_image_and_narration() {
    let image = concat_media::probe::probe(asset("CONCAT_CONTROL_TEST_IMAGE")).unwrap();
    let v = image.require_video().unwrap();
    assert!(v.width > 0 && v.height > 0);
    let voice = concat_media::probe::probe(asset("CONCAT_CONTROL_TEST_VOICE")).unwrap();
    assert!(voice.audio.is_some());
    assert!(voice.duration.unwrap().as_f64() > 1.0);
}
#[test]
fn native_probe_validates_the_actual_control_output() {
    let output = concat_media::probe::probe(asset("CONCAT_CONTROL_TEST_OUTPUT")).unwrap();
    let v = output.require_video().unwrap();
    assert_eq!((v.width, v.height), (720, 1280));
    assert!(output.audio.is_some());
    let duration = output.duration.unwrap().as_f64();
    assert!((duration - 6.0).abs() < 0.2);
    println!(
        "VERIFIED_NATIVE_OUTPUT={}x{} duration={} audio=true",
        v.width, v.height, duration
    );
}
#[test]
fn missing_output_never_has_a_successful_native_probe() {
    let absent =
        asset("KEDI_TEST_REPO").join(".runtime/ai-control-output-that-was-never-rendered.mp4");
    assert!(!absent.exists());
    assert!(concat_media::probe::probe(absent).is_err());
}
