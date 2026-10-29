//! Explicit integration check, linked against the already-built Studio library.
//! Requests the installed Nexora manager window through its single-instance
//! handler; never starts a browser profile or submits a generation job.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = std::path::PathBuf::from(
        std::env::var_os("APPDATA").ok_or("APPDATA missing")?,
    ).join("app.concat.editor");
    let settings = concat_studio::nexora::NexoraSettings::load(&config)
        .map_err(|_| "Nexora configuration unavailable")?;
    let client = settings.client().map_err(|_| "Nexora client unavailable")?;
    let mut before = client.profiles().map_err(|_| "Nexora catalogue unavailable")?
        .into_iter().map(|p| p.id).collect::<Vec<_>>();
    let mut after = settings.show().map_err(|_| "Nexora show request failed")?
        .into_iter().map(|p| p.id).collect::<Vec<_>>();
    before.sort();
    after.sort();
    if before != after { return Err("Profile catalogue changed during check".into()); }
    println!("NEXORA_SHOW_REQUEST=PASS; profile catalogue unchanged; no browser or generation job");
    Ok(())
}
