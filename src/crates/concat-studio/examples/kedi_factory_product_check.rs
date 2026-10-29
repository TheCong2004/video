//! Import a supplied Shopee product image and render it through the actual Factory.
use concat_studio::{factory_product, kedi_factory, storyboard_render};
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let link = factory_product::parse_shopee_link(args.get(1).ok_or("full product URL required")?)?;
    let root = PathBuf::from(args.get(2).ok_or("new output directory required")?);
    if root.exists() {
        return Err("existing output is never overwritten".into());
    }
    std::fs::create_dir_all(&root)?;
    let mut paths = vec![];
    for id in &link.image_ids {
        let path = root.join(format!("{id}.jpg"));
        std::fs::write(&path, factory_product::fetch_shopee_image(id)?)?;
        paths.push(path);
    }
    std::fs::write(root.join("source.json"), serde_json::to_vec_pretty(&link)?)?;
    let board = kedi_factory::product_board(
        &paths,
        &link.title_hint,
        "",
        "",
        5.0,
        None,
    )?;
    let ffmpeg =
        PathBuf::from(std::env::var_os("CONCAT_TEST_FFMPEG").ok_or("CONCAT_TEST_FFMPEG required")?);
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
        &AtomicBool::new(false),
        |_, _| {},
    )?;
    kedi_factory::verify_media(&ffmpeg.with_file_name("ffprobe.exe"), &output)?;
    println!(
        "SHOPEE_PRODUCT_SMOKE=PASS shop={} item={} images={} price=unverified output={}",
        link.shop_id,
        link.item_id,
        paths.len(),
        output.display()
    );
    Ok(())
}
