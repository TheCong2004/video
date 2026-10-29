// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit interactive PKCE diagnostic using the production auth adapter.
//! Prints stages/status only, never URLs, auth codes, identities or credentials.
use concat_studio::auth::{AuthManager, LoginAttempt};
use std::{path::PathBuf, sync::atomic::AtomicBool};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = std::env::var("CONCAT_PROMAX_ORIGIN")
        .unwrap_or_else(|_| "https://promax-backend.thecong2610.workers.dev".into());
    let identity = std::env::var("CONCAT_SUPABASE_URL")
        .unwrap_or_else(|_| "https://wcecwdoclzfiwyaavocy.supabase.co".into());
    let public_key = std::env::var("CONCAT_SUPABASE_PUBLISHABLE_KEY")
        .unwrap_or_else(|_| "sb_publishable_-WkSHYM-rjBTAqRXBkX-wA_vi-UyK3l".into());
    let config = PathBuf::from(std::env::var_os("APPDATA").ok_or("APPDATA missing")?)
        .join("app.concat.editor");
    let manager = AuthManager::new(&backend, &config).map_err(|e| format!("AUTH-CONFIG: {e:?}"))?;
    let attempt =
        LoginAttempt::new(&identity, &public_key).map_err(|e| format!("AUTH-START: {e:?}"))?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(attempt.authorize_url())
            .creation_flags(0x08000000)
            .spawn()?;
    }
    #[cfg(not(windows))]
    return Err("This explicit diagnostic is Windows-only".into());
    println!("AUTH-STAGE: waiting for Google callback in the browser (up to 5 minutes)");
    let cancel = AtomicBool::new(false);
    let code = attempt
        .wait(&cancel)
        .map_err(|e| format!("AUTH-CALLBACK: {e:?}"))?;
    println!("AUTH-STAGE: callback received; verifying identity and Promax exchange");
    manager
        .complete_login(&identity, &public_key, code, &cancel)
        .map_err(|e| format!("AUTH-EXCHANGE: {e:?}"))?;
    println!("AUTH-STAGE: verified session saved with DPAPI; checking Promax acceptance");
    manager
        .account()
        .map_err(|e| format!("AUTH-RESTORE: {e:?}"))?;
    println!("AUTH-RESULT: verified session accepted; restart Concat to restore it");
    Ok(())
}
