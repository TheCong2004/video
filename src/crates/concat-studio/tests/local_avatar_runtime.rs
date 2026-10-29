// SPDX-License-Identifier: AGPL-3.0-or-later
//! Real owned-process readiness regression (explicit local-runtime opt-in).
#[test]
fn actual_owned_runtime_reports_preparation_ready() {
    if std::env::var_os("CONCAT_LOCAL_AVATAR_RUNTIME_TEST").is_none() {
        return;
    }
    let data = std::env::temp_dir().join(format!("concat-runtime-ready-{}", std::process::id()));
    let client = concat_studio::local_avatars::client(&data).unwrap();
    let result = client.readiness().unwrap();
    concat_studio::local_avatars::shutdown();
    assert!(
        result.ready,
        "missing={:?}; diagnostic directory={}",
        result.missing,
        data.display()
    );
}
