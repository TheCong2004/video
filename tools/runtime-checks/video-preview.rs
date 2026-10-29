//! Exact thumbnail module check without regenerating the whole editor UI.
#[path = "../../src/crates/concat/src/studio_video_preview.rs"]
mod studio_video_preview;
mod studio_scenes {
    pub(crate) fn ffmpeg() -> Result<std::path::PathBuf, String> {
        std::env::var_os("CONCAT_PREVIEW_FFMPEG")
            .map(std::path::PathBuf::from)
            .ok_or("explicit QA ffmpeg required".into())
    }
}
fn main() {
    let args = std::env::args_os().collect::<Vec<_>>();
    let root = std::path::Path::new(args.get(2).expect("explicit QA cache required"));
    if args.get(1).is_some_and(|a| a == "--remote") {
        use std::io::Read;
        use sha2::{Digest, Sha256};
        let mut input = String::new();
        std::io::stdin().take(1024 * 1024).read_to_string(&mut input).unwrap();
        let rows: serde_json::Value = serde_json::from_str(&input).expect("explicit public cover rows");
        let rows = rows.as_array().expect("cover rows array");
        assert_eq!(rows.len(), 10);
        let mut distinct = std::collections::HashSet::new();
        for row in rows {
            let preview = studio_video_preview::remote(row["thumbnail"].as_str().unwrap(), row["url"].as_str().unwrap(), 0, root);
            assert!(preview.width > 0 && preview.width <= 320 && preview.height > 0 && preview.height <= 180, "every live cover must decode");
            distinct.insert(Sha256::digest(&preview.pixels));
        }
        assert!(distinct.len() > 1, "live covers must not all be one repeated fixture");
        println!("EXACT_REMOTE_PREVIEW=PASS all 10 public account covers decoded; {} distinct pixel hashes; no video downloads", distinct.len());
        return;
    }
    studio_video_preview::cover_smoke(
        std::path::Path::new(args.get(1).expect("explicit AVIF required")),
        root,
    );
    if let Some(video) = args.get(3) {
        let preview = studio_video_preview::local(std::path::Path::new(video), root);
        assert!(preview.width > 0 && preview.height > 0);
        assert_ne!(preview.duration, "Video");
        let second = studio_video_preview::local(std::path::Path::new(video), root);
        assert_eq!(preview.pixels, second.pixels);
    }
    println!(
        "EXACT_PREVIEW_MODULE=PASS actual AVIF, local MP4, bounded pixels and cache; no originals changed"
    );
}
