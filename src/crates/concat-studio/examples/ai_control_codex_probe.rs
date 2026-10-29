// SPDX-License-Identifier: AGPL-3.0-or-later
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("Usage: probe <codex-executable> <isolated-runtime-root>".into());
    }
    let health = concat_studio::codex_runtime::probe(
        std::path::Path::new(&args[1]),
        std::path::Path::new(&args[2]),
    )?;
    println!(
        "{}",
        serde_json::to_string(&health).map_err(|e| e.to_string())?
    );
    Ok(())
}
