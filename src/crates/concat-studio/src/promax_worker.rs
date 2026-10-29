//! Authenticated central Promax transport. This worker never schedules stages.
pub use crate::promax_worker_protocol::WorkerIdentity;
use crate::{
    auth::AuthManager,
    promax_worker_ledger::{Claim, Ledger, validate_receipt},
    promax_worker_protocol::{Envelope, cancellation_matches},
};
use serde_json::{Value, json};
use std::{
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{
    Message, WebSocket, client::IntoClientRequest, protocol::WebSocketConfig,
    stream::MaybeTlsStream,
};

/// Called only after authenticated routing, validation and a durable claim.
pub trait ProductionHandler: Send + Sync + 'static {
    fn capabilities(&self) -> Vec<String>;
    fn validate(&self, command: &Value) -> Result<(), String>;
    /// Success: compact {output,checkpoint?}. Confirmed cancellation additionally
    /// uses {status:"cancelled",confirmed:true}. Err means UNKNOWN, never retry.
    fn execute(&self, command: Value, cancel: Arc<AtomicBool>) -> Result<Value, String>;
}

pub struct CentralWorker {
    identity: WorkerIdentity,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<String>>,
}
impl CentralWorker {
    /// Connect via Nexora's native relay. No separate Concat account or exported
    /// Promax credential is needed; the relay enforces the installation identity.
    pub fn via_nexora(
        browser: crate::browser::BrowserClient,
        profile: String,
        ledger_root: PathBuf,
        handler: Arc<dyn ProductionHandler>,
    ) -> Result<Self, String> {
        let identity = browser.production_worker_identity(&profile)?;
        let url = browser.production_worker_url(&profile)?;
        let ledger = Ledger::open(&ledger_root)?;
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new("connecting".into()));
        let owned = Self {
            identity: identity.clone(),
            stop: stop.clone(),
            status: state.clone(),
        };
        let connect: Connector = Arc::new(move |expected| {
            if &browser.production_worker_identity(&profile)? != expected {
                return Err("PROMAX_IDENTITY_CHANGED".into());
            }
            let mut req = url
                .as_str()
                .into_client_request()
                .map_err(|_| "NEXORA_ORIGIN_INVALID")?;
            req.headers_mut().insert(
                "X-Concat-Integration",
                "1".parse().map_err(|_| "NEXORA_ORIGIN_INVALID")?,
            );
            let port = url.port_or_known_default().ok_or("NEXORA_ORIGIN_INVALID")?;
            let tcp = TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_secs(5),
            )
            .map_err(|_| "NEXORA_WORKER_BRIDGE_REQUIRED")?;
            tcp.set_read_timeout(Some(Duration::from_secs(45)))
                .map_err(|_| "WORKER_SOCKET_UNAVAILABLE")?;
            tcp.set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|_| "WORKER_SOCKET_UNAVAILABLE")?;
            let config = WebSocketConfig::default()
                .write_buffer_size(0)
                .max_write_buffer_size(2 * 1024 * 1024)
                .max_message_size(Some(1024 * 1024))
                .max_frame_size(Some(1024 * 1024));
            let (mut socket, _) = tungstenite::client(req, MaybeTlsStream::Plain(tcp)).map_err(
                |error| match error {
                    tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response))
                        if response.status().as_u16() == 503
                            && response.body().as_deref().is_some_and(|body| {
                                body.len() <= 8192
                                    && serde_json::from_slice::<Value>(body).ok().is_some_and(|v| {
                                        v["error"]["code"] == "PROMAX_TRANSPORT_NOT_CONFIGURED"
                                    })
                            }) =>
                    {
                        "PROMAX_TRANSPORT_NOT_CONFIGURED"
                    }
                    _ => "NEXORA_WORKER_BRIDGE_REQUIRED",
                },
            )?;
            socket.set_config(|c| *c = config);
            read_timeout(&mut socket, Duration::from_millis(200))?;
            Ok(socket)
        });
        thread::Builder::new()
            .name("promax-production-worker".into())
            .spawn(move || supervise(connect, identity, ledger, handler, stop, state))
            .map_err(|_| "WORKER_THREAD_UNAVAILABLE")?;
        Ok(owned)
    }
    /// May refresh authentication: call off the desktop UI thread.
    pub fn identity(manager: &AuthManager) -> Result<WorkerIdentity, String> {
        let auth = manager
            .worker_auth()
            .map_err(|_| "PROMAX_DEVICE_AUTH_REQUIRED")?;
        Ok(WorkerIdentity {
            client_id: format!("concat-production-{}", auth.device_id),
            device_id: auth.device_id,
            tenant_id: auth.tenant_id,
            user_id: auth.user_id,
        })
    }
    /// Does not launch a browser or log in; requires an existing verified session.
    pub fn start(
        manager: Arc<AuthManager>,
        ledger_root: PathBuf,
        handler: Arc<dyn ProductionHandler>,
    ) -> Result<Self, String> {
        let identity = Self::identity(&manager)?;
        let ledger = Ledger::open(&ledger_root)?;
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new("connecting".into()));
        let owned = Self {
            identity: identity.clone(),
            stop: stop.clone(),
            status: status.clone(),
        };
        thread::Builder::new()
            .name("promax-production-worker".into())
            .spawn(move || {
                supervise(
                    Arc::new(move |identity| connect(&manager, identity)),
                    identity,
                    ledger,
                    handler,
                    stop,
                    status,
                )
            })
            .map_err(|_| "WORKER_THREAD_UNAVAILABLE")?;
        Ok(owned)
    }
    pub fn worker_identity(&self) -> &WorkerIdentity {
        &self.identity
    }
    pub fn status(&self) -> String {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "unavailable".into())
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Drop for CentralWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;
type Connector = Arc<dyn Fn(&WorkerIdentity) -> Result<Socket, String> + Send + Sync>;
struct Active {
    envelope: Envelope,
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<Value, String>>,
    deadline: Instant,
}
fn status(slot: &Mutex<String>, value: &str) {
    if let Ok(mut slot) = slot.lock() {
        *slot = value.into();
    }
}
fn send(socket: &mut Socket, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_string(value).map_err(|_| "WORKER_FRAME_INVALID")?;
    if bytes.len() > 65536 {
        return Err("WORKER_FRAME_TOO_LARGE".into());
    }
    socket
        .send(Message::Text(bytes.into()))
        .map_err(|_| "WORKER_DISCONNECTED".into())
}
fn read_timeout(socket: &mut Socket, timeout: Duration) -> Result<(), String> {
    let stream = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream,
        MaybeTlsStream::NativeTls(stream) => stream.get_mut(),
        _ => return Err("WORKER_TLS_UNSUPPORTED".into()),
    };
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|_| "WORKER_SOCKET_UNAVAILABLE".into())
}
fn connect(manager: &AuthManager, identity: &WorkerIdentity) -> Result<Socket, String> {
    let auth = manager
        .worker_auth()
        .map_err(|_| "PROMAX_DEVICE_AUTH_REQUIRED")?;
    connect_auth(auth, identity)
}

fn connect_auth(
    auth: crate::auth::WorkerAuth,
    identity: &WorkerIdentity,
) -> Result<Socket, String> {
    if auth.device_id != identity.device_id
        || auth.tenant_id != identity.tenant_id
        || auth.user_id != identity.user_id
    {
        return Err("PROMAX_IDENTITY_CHANGED".into());
    }
    let mut url = url::Url::parse(&auth.origin).map_err(|_| "PROMAX_ORIGIN_INVALID")?;
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => return Err("PROMAX_ORIGIN_INVALID".into()),
    };
    url.set_scheme(scheme)
        .map_err(|_| "PROMAX_ORIGIN_INVALID")?;
    url.set_path("/api/v1/artcraft/transport/ws");
    url.query_pairs_mut()
        .append_pair("clientType", "artcraft_worker")
        .append_pair("clientId", &identity.client_id)
        .append_pair("deviceId", &identity.device_id);
    let host = url.host_str().ok_or("PROMAX_ORIGIN_INVALID")?;
    let port = url.port_or_known_default().ok_or("PROMAX_ORIGIN_INVALID")?;
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| "PROMAX_UNAVAILABLE")?;
    let mut tcp = None;
    for address in addresses.take(8) {
        if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_secs(5)) {
            tcp = Some(stream);
            break;
        }
    }
    let tcp = tcp.ok_or("PROMAX_UNAVAILABLE")?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|_| "WORKER_SOCKET_UNAVAILABLE")?;
    tcp.set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| "WORKER_SOCKET_UNAVAILABLE")?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "PROMAX_ORIGIN_INVALID")?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", auth.access)
            .parse()
            .map_err(|_| "PROMAX_DEVICE_AUTH_REQUIRED")?,
    );
    let config = WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .write_buffer_size(0)
        .max_write_buffer_size(2 * 1024 * 1024)
        .max_message_size(Some(1024 * 1024))
        .max_frame_size(Some(128 * 1024));
    // Direct handshake only: no redirect-following helper and no token query.
    let (mut socket, _) = tungstenite::client_tls_with_config(request, tcp, Some(config), None)
        .map_err(|_| "PROMAX_WORKER_CONNECT_FAILED")?;
    read_timeout(&mut socket, Duration::from_millis(250))?;
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn active(envelope: Envelope) -> Active {
        let (_, result) = mpsc::sync_channel(1);
        Active {
            envelope,
            cancel: Arc::new(AtomicBool::new(false)),
            result,
            deadline: Instant::now() + Duration::from_secs(5),
        }
    }
    #[test]
    fn handler_errors_and_unconfirmed_cancellation_keep_unknown_claims() {
        let root =
            std::env::temp_dir().join(format!("concat-worker-finish-{:x}", rand::random::<u128>()));
        let ledger = Ledger::open(&root).unwrap();
        let (_, envelope) = crate::promax_worker_protocol::fixture();
        ledger.claim(&envelope).unwrap();
        assert_eq!(
            finish(
                active(envelope.clone()),
                Err("SECRET_PROVIDER_RESPONSE".into()),
                &ledger
            )
            .err()
            .unwrap(),
            "WORKER_OUTCOME_UNKNOWN"
        );
        assert!(
            finish(
                active(envelope.clone()),
                Ok(json!({"status":"cancelled","confirmed":false})),
                &ledger
            )
            .is_err()
        );
        assert!(matches!(ledger.claim(&envelope).unwrap(), Claim::Unknown));
        drop(ledger);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn confirmed_cancel_is_persisted_before_wire_delivery() {
        let root =
            std::env::temp_dir().join(format!("concat-worker-cancel-{:x}", rand::random::<u128>()));
        let ledger = Ledger::open(&root).unwrap();
        let (_, envelope) = crate::promax_worker_protocol::fixture();
        ledger.claim(&envelope).unwrap();
        let result = finish(
            active(envelope.clone()),
            Ok(json!({"status":"cancelled","confirmed":true})),
            &ledger,
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.1["status"], "cancelled");
        assert!(matches!(ledger.claim(&envelope).unwrap(), Claim::Replay(_)));
        drop(ledger);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn loopback_wire_uses_bearer_header_then_existing_ack_and_result_contract() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (identity, envelope) = crate::promax_worker_protocol::fixture();
        let server_identity = identity.clone();
        let server_envelope = envelope.clone();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut socket = tungstenite::accept_hdr(
                stream,
                |request: &tungstenite::handshake::server::Request,
                 response: tungstenite::handshake::server::Response| {
                    assert_eq!(request.headers()["authorization"], "Bearer fixture-access");
                    assert!(!request.uri().query().unwrap_or("").contains("token"));
                    assert_eq!(request.uri().path(), "/api/v1/artcraft/transport/ws");
                    assert!(
                        request
                            .uri()
                            .query()
                            .unwrap_or("")
                            .contains("clientType=artcraft_worker")
                    );
                    Ok(response)
                },
            )
            .unwrap();
            socket.send(Message::Text(json!({"type":"connection.ready","clientId":server_identity.client_id,"tenantId":server_identity.tenant_id,"connectionGeneration":2}).to_string().into())).unwrap();
            socket
                .send(Message::Text(
                    json!({"type":"command.deliver","envelope":server_envelope})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            let ack: Value =
                serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(ack["type"], "delivery.ack");
            assert_eq!(ack["ack"]["accepted"], true);
            let result: Value =
                serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(result["type"], "command.result");
            assert_eq!(result["result"]["kind"], "artcraft.workflow.step.result");
            assert_eq!(result["result"]["payload"]["fencingToken"], 1);
        });
        let auth = crate::auth::WorkerAuth {
            origin,
            access: "fixture-access".into(),
            tenant_id: identity.tenant_id.clone(),
            user_id: identity.user_id.clone(),
            device_id: identity.device_id.clone(),
        };
        let mut socket = connect_auth(auth, &identity).unwrap();
        let ready: Value = serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(ready["connectionGeneration"], 2);
        let delivery: Value =
            serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
        let delivered: Envelope = serde_json::from_value(delivery["envelope"].clone()).unwrap();
        delivered.validate(&identity, 2).unwrap();
        send(&mut socket, &delivered.ack(&identity, 2, true, "")).unwrap();
        send(
            &mut socket,
            &delivered.frame(
                &identity,
                2,
                delivered.terminal("completed", json!({"output":{"resourceId":"asset-1"}})),
            ),
        )
        .unwrap();
        server.join().unwrap();
    }
}
fn finish(
    active: Active,
    result: Result<Value, String>,
    ledger: &Ledger,
) -> Result<Option<(Envelope, Value)>, String> {
    let receipt = result.map_err(|_| "WORKER_OUTCOME_UNKNOWN")?;
    validate_receipt(&receipt)?;
    let cancelled = receipt.get("status").and_then(Value::as_str) == Some("cancelled");
    if receipt.get("status").is_some()
        && !matches!(
            receipt["status"].as_str(),
            Some("completed" | "cancelled" | "failed")
        )
    {
        return Err("WORKER_OUTCOME_UNKNOWN".into());
    }
    if cancelled && receipt.get("confirmed").and_then(Value::as_bool) != Some(true) {
        return Err("WORKER_OUTCOME_UNKNOWN".into());
    }
    let failed = receipt["status"] == "failed";
    if failed && receipt["confirmed"] != true {
        return Err("WORKER_OUTCOME_UNKNOWN".into());
    }
    if !cancelled && !failed && !receipt.get("output").is_some_and(Value::is_object) {
        return Err("WORKER_RESULT_INVALID".into());
    }
    let payload = active.envelope.terminal(
        if cancelled {
            "cancelled"
        } else if failed {
            "failed"
        } else {
            "completed"
        },
        receipt,
    );
    ledger.finish(&active.envelope, &payload)?;
    Ok(Some((active.envelope, payload)))
}
fn supervise(
    connector: Connector,
    identity: WorkerIdentity,
    ledger: Ledger,
    handler: Arc<dyn ProductionHandler>,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<String>>,
) {
    let mut active: Option<Active> = None;
    let mut quarantined = false;
    while !stop.load(Ordering::Acquire) {
        status(
            &state,
            if quarantined {
                "outcome_unknown"
            } else {
                "connecting"
            },
        );
        let mut socket = match connector(&identity) {
            Ok(socket) => socket,
            Err(error) => {
                if error == "PROMAX_TRANSPORT_NOT_CONFIGURED" {
                    status(&state, "PROMAX_TRANSPORT_NOT_CONFIGURED");
                    return;
                }
                for _ in 0..20 {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                continue;
            }
        };
        let mut generation = 0u64;
        let mut heartbeat = Instant::now();
        let mut last_server = Instant::now();
        let connected_at = Instant::now();
        loop {
            if stop.load(Ordering::Acquire) {
                if let Some(a) = &active {
                    a.cancel.store(true, Ordering::Release);
                }
                let _ = socket.close(None);
                return;
            }
            if let Some(a) = &active {
                if Instant::now() >= a.deadline {
                    a.cancel.store(true, Ordering::Release);
                }
            }
            let terminal = active.as_ref().map(|a| a.result.try_recv());
            match terminal {
                Some(Ok(result)) => {
                    if let Some(a) = active.take() {
                        match finish(a, result, &ledger) {
                            Ok(Some((envelope, payload))) => {
                                if generation > 0
                                    && send(
                                        &mut socket,
                                        &envelope.frame(&identity, generation, payload),
                                    )
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            _ => {
                                quarantined = true;
                                status(&state, "outcome_unknown");
                            }
                        }
                    }
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    active.take();
                    quarantined = true;
                    status(&state, "outcome_unknown");
                }
                _ => {}
            }
            if generation == 0 && connected_at.elapsed() > Duration::from_secs(15) {
                break;
            }
            if last_server.elapsed() > Duration::from_secs(45) {
                break;
            }
            if generation > 0 && heartbeat.elapsed() >= Duration::from_secs(5) {
                let capabilities = if quarantined {
                    vec![]
                } else {
                    handler.capabilities()
                };
                if capabilities.len() > 8
                    || capabilities
                        .iter()
                        .any(|s| !s.starts_with("artcraft.workflow.stage.") || s.len() > 128)
                {
                    quarantined = true;
                }
                let ready = !quarantined && !capabilities.is_empty();
                if send(&mut socket,&json!({"type":"heartbeat","connectionGeneration":generation,"readiness":{"status":if ready{"ready"}else{"standby"},"capabilities":if ready{capabilities}else{vec![]}}})).is_err(){break;}
                status(
                    &state,
                    if quarantined {
                        "outcome_unknown"
                    } else if active.is_some() {
                        "running"
                    } else if ready {
                        "ready"
                    } else {
                        "standby"
                    },
                );
                heartbeat = Instant::now();
            }
            let frame = match socket.read() {
                Ok(Message::Text(text)) => {
                    last_server = Instant::now();
                    match serde_json::from_str::<Value>(&text) {
                        Ok(value) => value,
                        Err(_) => break,
                    }
                }
                Ok(Message::Ping(data)) => {
                    if socket.send(Message::Pong(data)).is_err() {
                        break;
                    }
                    continue;
                }
                Ok(Message::Pong(_)) => {
                    last_server = Instant::now();
                    continue;
                }
                Ok(Message::Close(_))
                | Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    break;
                }
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(_) => break,
                _ => continue,
            };
            if generation == 0
                && frame.get("type").and_then(Value::as_str) != Some("connection.ready")
            {
                break;
            }
            match frame.get("type").and_then(Value::as_str) {
                Some("connection.ready") => {
                    if generation != 0
                        || frame["clientId"] != identity.client_id
                        || frame["tenantId"] != identity.tenant_id
                    {
                        break;
                    }
                    generation = frame["connectionGeneration"].as_u64().unwrap_or(0);
                    if generation == 0 {
                        break;
                    }
                    heartbeat = Instant::now() - Duration::from_secs(5);
                }
                Some("heartbeat.ack") => {
                    if frame["connectionGeneration"].as_u64() != Some(generation) {
                        break;
                    }
                }
                Some("command.cancel") => {
                    if let Some(a) = &active {
                        if cancellation_matches(&a.envelope, &frame) {
                            a.cancel.store(true, Ordering::Release);
                        }
                    }
                }
                Some("result.ack") => {
                    if let Some(id) = frame["commandId"].as_str() {
                        let _ = ledger.acknowledged(id);
                    }
                }
                Some("command.deliver") => {
                    let envelope: Envelope = match serde_json::from_value(frame["envelope"].clone())
                    {
                        Ok(value) => value,
                        Err(_) => break,
                    };
                    // Never block the transport on a handler mutex while a
                    // previous command executes; cancellation/heartbeats must flow.
                    if active.is_some() || quarantined {
                        if send(
                            &mut socket,
                            &envelope.ack(&identity, generation, false, "WORKER_BUSY_OR_UNKNOWN"),
                        )
                        .is_err()
                        {
                            break;
                        }
                        continue;
                    }
                    let checked = envelope.validate(&identity, generation).and_then(|_| {
                        if !handler.capabilities().contains(&envelope.capability) {
                            return Err("WORKER_CAPABILITY_MISMATCH".into());
                        }
                        handler
                            .validate(&envelope.payload.command)
                            .map_err(|_| "WORKER_COMMAND_INVALID".into())
                    });
                    if let Err(reason) = checked {
                        if send(
                            &mut socket,
                            &envelope.ack(&identity, generation, false, &reason),
                        )
                        .is_err()
                        {
                            break;
                        }
                        continue;
                    }
                    if active.is_some() || quarantined {
                        if send(
                            &mut socket,
                            &envelope.ack(&identity, generation, false, "WORKER_BUSY_OR_UNKNOWN"),
                        )
                        .is_err()
                        {
                            break;
                        }
                        continue;
                    }
                    match ledger.claim(&envelope) {
                        Ok(Claim::Replay(payload)) => {
                            if send(&mut socket, &envelope.ack(&identity, generation, true, ""))
                                .and_then(|_| {
                                    send(
                                        &mut socket,
                                        &envelope.frame(&identity, generation, payload),
                                    )
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Ok(Claim::New) => {
                            if send(&mut socket, &envelope.ack(&identity, generation, true, ""))
                                .is_err()
                            {
                                break;
                            }
                            let cancel = Arc::new(AtomicBool::new(false));
                            let task_cancel = cancel.clone();
                            let command = envelope.payload.command.clone();
                            let task_handler = handler.clone();
                            let (tx, rx) = mpsc::sync_channel(1);
                            let deadline = chrono::DateTime::parse_from_rfc3339(&envelope.deadline)
                                .map(|v| v.timestamp())
                                .unwrap_or(0);
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_or(0, |v| v.as_secs())
                                as i64;
                            let duration = Duration::from_secs(
                                deadline.saturating_sub(now).clamp(0, 3600) as u64,
                            );
                            if thread::Builder::new()
                                .name("promax-stage".into())
                                .spawn(move || {
                                    let _ = tx.send(task_handler.execute(command, task_cancel));
                                })
                                .is_err()
                            {
                                quarantined = true;
                                continue;
                            }
                            active = Some(Active {
                                envelope,
                                cancel,
                                result: rx,
                                deadline: Instant::now() + duration,
                            });
                        }
                        _ => {
                            if send(
                                &mut socket,
                                &envelope.ack(
                                    &identity,
                                    generation,
                                    false,
                                    "WORKER_LEDGER_REJECTED_UNKNOWN",
                                ),
                            )
                            .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(a) = &active {
            a.cancel.store(true, Ordering::Release);
        }
        // Never retry execution after transport loss. The claim survives;
        // an eventual terminal receipt may be persisted and replayed safely.
        let _ = socket.close(None);
    }
    if let Some(a) = &active {
        a.cancel.store(true, Ordering::Release);
    }
    status(&state, "stopped");
}
