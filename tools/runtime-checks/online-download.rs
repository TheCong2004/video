//! Runs the real adapter against an explicit public video or a local HTTP fixture.
//! Does not access browser cookies, submit generation jobs or import into user projects.
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let mode = args.get(1).ok_or("mode required")?;
    let url = args.get(2).ok_or("URL required")?;
    if mode == "probe" {
        let rows = concat_studio::lux::probe_video_info(url, false)?;
        println!("PROBE=PASS {} video, {} formats", rows.len(), rows[0].streams.len());
        return Ok(());
    }
    let output = std::path::PathBuf::from(args.get(3).ok_or("explicit output folder required")?);
    let cancel = Arc::new(AtomicBool::new(false));
    let trigger = cancel.clone();
    let cancelling = mode == "cancel";
    if cancelling { std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_millis(1500)); trigger.store(true, Ordering::Relaxed); }); }
    let started = std::time::Instant::now();
    let result = concat_studio::lux::download_video(&concat_studio::lux::LuxDownloadOptions {
        url: url.clone(), stream_id: Some("yt:auto".into()), output_dir: Some(output), cancel,
        ..Default::default()
    }, |_| {});
    if cancelling {
        assert_eq!(result.unwrap_err(), "DOWNLOAD_CANCELLED");
        assert!(started.elapsed() < std::time::Duration::from_secs(8));
        println!("CANCEL=PASS in {:?}", started.elapsed());
    } else {
        let paths = result?;
        assert!(!paths.is_empty());
        for path in paths { assert!(path.metadata()?.len() > 0); println!("VIDEO={}", path.display()); }
        println!("DOWNLOAD=PASS");
    }
    Ok(())
}
