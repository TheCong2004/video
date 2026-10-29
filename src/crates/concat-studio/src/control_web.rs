// SPDX-License-Identifier: AGPL-3.0-or-later
//! Owned browser model turns for Control; separate from a development login.
use crate::ai_control::Failure;
use serde::Deserialize;
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// Actual returned model response, never a claim that a project was edited.
pub struct Answer {
    pub text: String,
    pub model: String,
}

fn saved_answer(profile:&Path,provider:&str,request_id:&str)->Option<Answer> {
    let file=profile.join("turns").join(format!("{request_id}.json"));
    if std::fs::metadata(&file).ok()?.len()>300000 {return None;}
    let mut bytes=Vec::new();std::fs::File::open(file).ok()?.take(300001).read_to_end(&mut bytes).ok()?;
    if bytes.len()>300000{return None;}
    let value:serde_json::Value=serde_json::from_slice(&bytes).ok()?;
    if value["version"]!=1 || value["requestId"]!=request_id || value["provider"]!=provider || value["state"]!="completed" || value["dispatched"]!=true {return None;}
    let text=value["result"]["text"].as_str()?;let model=value["result"]["model"].as_str()?;
    if text.trim().is_empty() || text.len()>65536 || model!=format!("web:{provider}"){return None;}
    Some(Answer{text:text.into(),model:model.into()})
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Event {
    Status { message: String },
    Dispatched,
    Result { text: String, model: String },
    Error { message: String, unknown: bool },
}

/// Execute one turn. Dispatch is durable in the workflow before this call.
/// Login remains manual in the owned window; no credential or cookie extraction.
pub fn request(
    node: &Path,
    script: &Path,
    profile: &Path,
    provider: &str,
    request_id: &str,
    prompt: &str,
    cancel: &AtomicBool,
    progress: impl Fn(&str),
) -> Result<Answer, Failure> {
    let fail = |message: &str, unknown| Failure {
        message: message.into(),
        unknown,
        waiting_input: !unknown,
    };
    if !matches!(provider, "chatgpt" | "gemini" | "claude")
        || !node.is_file()
        || !script.is_file()
        || prompt.is_empty()
        || prompt.len() > 131072
        || request_id.len() > 128
        || request_id.is_empty()
        || !request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(fail("Browser AI runtime or request is invalid", false));
    }
    if cancel.load(Ordering::Acquire) {
        return Err(fail("Đã hủy trước khi mở web", false));
    }
    if let Some(answer)=saved_answer(profile,provider,request_id){progress("Đã khôi phục phản hồi đã nhận; không gửi thêm yêu cầu web.");return Ok(answer);}
    let mut command = crate::lux::make_command(node);
    command
        .arg(script)
        .arg(profile)
        .arg(provider)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| fail("Không mở được trình duyệt AI", false))?;
    #[cfg(windows)]
    let _job = match crate::local_ai::owned_job(&child) {
        Ok(job) => job,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(fail("Không quản lý được tiến trình AI riêng", false));
        }
    };
    let result = (|| {
        let output = child
            .stdout
            .take()
            .ok_or_else(|| fail("Thiếu kênh phản hồi web", false))?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| fail("Thiếu kênh điều khiển web", false))?;
        let (tx, rx) = mpsc::sync_channel(8);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = String::new();
                match reader.by_ref().take(300001).read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line.len() <= 300000 => {
                        if let Ok(event) = serde_json::from_str::<Event>(&line) {
                            if tx.send(event).is_err() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        });
        writeln!(
            input,
            "{}",
            serde_json::json!({"requestId":request_id,"prompt":prompt})
        )
        .and_then(|_| input.flush())
        .map_err(|_| fail("Không gửi được yêu cầu đến browser adapter", false))?;
        let deadline = Instant::now() + Duration::from_secs(300);
        let mut dispatched = false;
        loop {
            if cancel.load(Ordering::Acquire) || Instant::now() > deadline {
                let _ = writeln!(input, "stop");
                let _ = input.flush();
                return Err(fail(
                    "Đã yêu cầu dừng chat web; kiểm tra kết quả trước khi gửi lại",
                    dispatched,
                ));
            }
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Event::Status { message }) => {
                    progress(&message.chars().take(500).collect::<String>())
                }
                Ok(Event::Dispatched) => dispatched = true,
                Ok(Event::Result { text, model }) => {
                    if !dispatched
                        || text.trim().is_empty()
                        || text.len() > 65536
                        || model != format!("web:{provider}")
                    {
                        return Err(fail("Phản hồi web không đúng hợp đồng", dispatched));
                    }
                    return Ok(Answer { text, model });
                }
                Ok(Event::Error { message, unknown }) => {
                    return Err(Failure {
                        message: message.chars().take(500).collect(),
                        unknown: dispatched || unknown,
                        waiting_input: !dispatched && !unknown,
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => {
                    return Err(fail(
                        "Mất kết nối trình duyệt AI; không tự gửi lại",
                        dispatched,
                    ));
                }
            }
        }
    })();
    // Node first closes its owned browser normally. The job is the bounded
    // fallback on Windows, including descendants, never unrelated browsers.
    let end = Instant::now() + Duration::from_secs(5);
    while child.try_wait().ok().flatten().is_none() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    match result {
        Err(error) if error.unknown && !cancel.load(Ordering::Acquire)=>saved_answer(profile,provider,request_id).ok_or(error),
        other=>other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    #[test]
    fn saved_response_requires_completed_correlated_provider_receipt() {
        let root=std::env::temp_dir().join(format!("control-receipt-test-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(root.join("turns")).unwrap();
        let file=root.join("turns/turn-1.json");
        let mut value=serde_json::json!({"version":1,"requestId":"turn-1","provider":"chatgpt","state":"completed","dispatched":true,"result":{"text":"Received output","model":"web:chatgpt"}});
        std::fs::write(&file,value.to_string()).unwrap();
        assert_eq!(saved_answer(&root,"chatgpt","turn-1").unwrap().text,"Received output");
        assert!(saved_answer(&root,"claude","turn-1").is_none());
        assert!(saved_answer(&root,"chatgpt","turn-2").is_none());
        value["state"]="failed".into();std::fs::write(&file,value.to_string()).unwrap();
        assert!(saved_answer(&root,"chatgpt","turn-1").is_none());
        value["state"]="completed".into();value["requestId"]="old".into();std::fs::write(&file,value.to_string()).unwrap();
        assert!(saved_answer(&root,"chatgpt","turn-1").is_none());
        std::fs::remove_file(file).unwrap();std::fs::remove_dir(root.join("turns")).unwrap();std::fs::remove_dir(root).unwrap();
    }
    fn fixture(body: impl FnOnce(&Path, &Path, &Path)) {
        let node = std::env::var_os("CONCAT_TEST_NODE")
            .expect("set CONCAT_TEST_NODE to the installed Node executable");
        let root = std::env::temp_dir().join(format!(
            "concat-control-web-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let script = root.join("adapter.mjs");
        std::fs::write(&script,r#"
import {createInterface} from 'node:readline';
const lines=createInterface({input:process.stdin});
lines.on('line',line=>{
 if(line==='stop'){process.exit(0);return;}
 const request=JSON.parse(line);
 const emit=v=>process.stdout.write(JSON.stringify(v)+'\n');
 emit({kind:'dispatched'});
 if(request.prompt==='cancel'){emit({kind:'status',message:'after-dispatch'});return;}
 if(request.prompt==='unknown'){emit({kind:'error',unknown:true,message:'Unknown result'});process.exitCode=1;lines.close();process.stdin.destroy();return;}
 emit({kind:'result',text:'Actual adapter response',model:request.prompt==='bad-model'?'invented':'web:chatgpt'});
 lines.close();process.stdin.destroy();
});
"#).unwrap();
        body(Path::new(&node), &script, &root);
        std::fs::remove_file(script).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    #[ignore = "requires installed Node; protocol fixture only, not a model call"]
    fn correlated_owned_process_returns_actual_response() {
        fixture(|node, script, root| {
            let cancel = AtomicBool::new(false);
            let answer = request(
                node,
                script,
                root,
                "chatgpt",
                "request-1",
                "ok",
                &cancel,
                |_| {},
            )
            .unwrap();
            assert_eq!(answer.text, "Actual adapter response");
            assert_eq!(answer.model, "web:chatgpt");
        });
    }
    #[test]
    #[ignore = "requires installed Node; protocol fixture only, not a model call"]
    fn dispatched_failure_and_invalid_result_are_unknown() {
        fixture(|node, script, root| {
            for prompt in ["unknown", "bad-model"] {
                let error = request(
                    node,
                    script,
                    root,
                    "chatgpt",
                    "request-1",
                    prompt,
                    &AtomicBool::new(false),
                    |_| {},
                )
                .err()
                .unwrap();
                assert!(error.unknown);
                assert!(!error.waiting_input);
            }
        });
    }
    #[test]
    #[ignore = "requires installed Node; protocol fixture only, not a model call"]
    fn cancellation_after_dispatch_preserves_uncertainty() {
        fixture(|node, script, root| {
            let cancel = AtomicBool::new(false);
            let error = request(
                node,
                script,
                root,
                "chatgpt",
                "request-1",
                "cancel",
                &cancel,
                |message| {
                    assert_eq!(message, "after-dispatch");
                    cancel.store(true, Ordering::Release);
                },
            )
            .err()
            .unwrap();
            assert!(error.unknown);
        });
    }
}
