// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit real-runtime smoke: hidden Electron, no model or provider calls.
#[cfg(windows)]
#[test]
#[ignore = "Requires the prepared adjacent Hotclip checkout and installed Electron"]
fn real_hotclip_runtime_handshakes_and_quits() {
    use concat_studio::hotclip::{Event, Process};
    use std::time::{Duration, Instant};
    let data = std::env::temp_dir().join(format!("concat-hotclip-runtime-{}", std::process::id()));
    let process = Process::start(data);
    let start = Instant::now();
    let mut ready = false;
    while start.elapsed() < Duration::from_secs(35) {
        match process.poll() {
            Some(Event::Ready { hwnd, pid }) => {
                assert!(hwnd > 0 && pid > 0);
                ready = true;
                break;
            }
            Some(Event::Failed(error)) => panic!("{error}"),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    process.shutdown();
    assert!(ready, "Real Electron never became ready");
}
