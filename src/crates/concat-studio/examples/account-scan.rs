//! Explicit online diagnostic. Never downloads media or reads personal browser stores.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).ok_or("mode: direct | browser")?;
    let url = args.get(2).ok_or("explicit account URL required")?;
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let timeout = args
        .get(4)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    let limit = args
        .get(5)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(timeout));
        flag.store(true, Ordering::Relaxed);
    });
    let result = if mode == "browser" || mode == "covers" {
        concat_studio::download_browser::list_tiktok(
            url,
            limit,
            std::path::Path::new(
                args.get(3)
                    .ok_or("explicit app-owned profile directory required")?,
            ),
            cancel,
        )
    } else {
        concat_studio::lux::list_account_videos(url, limit, cancel)
    };
    let rows = result?;
    if mode == "covers" {
        let covers = rows.iter().filter(|r| !r.thumbnail.is_empty()).count();
        let first = rows
            .iter()
            .find(|r| !r.thumbnail.is_empty())
            .ok_or("No rendered cover URLs")?;
        let bytes = concat_studio::lux::fetch_thumbnail(&first.thumbnail)?;
        let destination = args
            .get(6)
            .ok_or("Explicit thumbnail QA destination required")?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        println!(
            "COVER=PASS; {covers} public covers; first cover {} bytes; no media download",
            bytes.len()
        );
    }
    println!(
        "ACCOUNT_LIST=PASS; {} distinct public video links; no media downloaded",
        rows.len()
    );
    for row in rows.into_iter().take(10) {
        println!(
            "{} | {}",
            row.url,
            row.title.chars().take(80).collect::<String>()
        );
    }
    Ok(())
}
