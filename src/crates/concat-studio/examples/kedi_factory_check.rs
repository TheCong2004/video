//! Explicit public-link smoke check through the same adapters used by Factory.
//! Never reads browser cookies or publishes to an account.
use concat_studio::{kedi_factory, lux, storyboard_render};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let url = args.get(1).ok_or("public video URL required")?;
    let root = PathBuf::from(
        args.get(2)
            .ok_or("explicit new output directory required")?,
    );
    if root.exists() {
        return Err("use a new directory; existing files are never overwritten".into());
    }
    std::fs::create_dir_all(&root)?;
    let ffmpeg =
        PathBuf::from(std::env::var_os("CONCAT_TEST_FFMPEG").ok_or("CONCAT_TEST_FFMPEG required")?);
    let probe = ffmpeg.with_file_name("ffprobe.exe");
    let cancel = Arc::new(AtomicBool::new(false));
    let rows = kedi_factory::resolve_url(url, false, cancel.clone())?;
    let video = rows.first().ok_or("no actual video metadata")?;
    println!(
        "ACTUAL_METADATA title={} provider={} duration={} streams={}",
        video.title,
        video.site,
        video.duration_seconds,
        video.streams.len()
    );
    let paths = lux::download_video(
        &lux::LuxDownloadOptions {
            url: video.url.clone(),
            stream_id: video.streams.first().map(|s| s.id.clone()),
            output_dir: Some(root.join("downloads")),
            cancel: cancel.clone(),
            ..Default::default()
        },
        |_| {},
    )?;
    for path in &paths {
        kedi_factory::verify_media(&probe, path)?;
    }
    let board = kedi_factory::product_board(&paths, "", "", "Kiểm tra tải và dựng", 1.0, None)?;
    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../concat-text/fonts/HankenGrotesk-Regular.ttf");
    let output = storyboard_render::render_board(
        &ffmpeg,
        &font,
        &board,
        &root.join("render"),
        storyboard_render::RenderSize {
            width: 720,
            height: 1280,
        },
        &cancel,
        |_, _| {},
    )?;
    kedi_factory::verify_media(&probe, &output)?;
    println!("FACTORY_PUBLIC_SMOKE=PASS output={}", output.display());
    Ok(())
}
