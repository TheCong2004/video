// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicitly-owned TikTok LIVE connector process supervisor.
//!
//! Nothing starts on construction. A caller must ask to connect, and only the
//! exact child process created here is ever stopped or terminated.

use crate::live::{LiveCommand, LiveFrame, LiveFrameKind, MAX_FRAME_BYTES, parse_frame};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

const READY_TIMEOUT: Duration = Duration::from_secs(12);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const FRAME_QUEUE_CAPACITY: usize = 256;
const REQUEST_COUNTER_LIMIT: u64 = u64::MAX - 1024;

/// Explicit paths to a Node runtime and the connector entrypoint.
#[derive(Debug, Clone)]
pub struct LiveRuntimeConfig {
    /// Node executable (bundled with Concat in packaged builds).
    pub node: PathBuf,
    /// Connector's compiled JSONL entrypoint.
    pub entrypoint: PathBuf,
    /// Working directory containing the connector's production dependencies.
    pub working_directory: PathBuf,
}

impl LiveRuntimeConfig {
    /// Construct a runtime configuration and verify all filesystem inputs.
    pub fn new(
        node: impl Into<PathBuf>,
        entrypoint: impl Into<PathBuf>,
        working_directory: impl Into<PathBuf>,
    ) -> Result<Self, LiveRuntimeError> {
        let config = Self {
            node: node.into(),
            entrypoint: entrypoint.into(),
            working_directory: working_directory.into(),
        };
        if !config.node.is_file()
            || !config.entrypoint.is_file()
            || !config.working_directory.is_dir()
        {
            return Err(LiveRuntimeError::RuntimeMissing);
        }
        Ok(config)
    }

    /// Resolve the connector from a packaged resource directory.
    pub fn from_resources(
        resources: &Path,
        node_executable: &str,
    ) -> Result<Self, LiveRuntimeError> {
        if !matches!(node_executable, "node" | "node.exe") {
            return Err(LiveRuntimeError::Configuration);
        }
        let roots = [
            resources.join("tiktok-live-connector"),
            resources.join("resources/tiktok-live-connector"),
        ];
        let nodes = [
            resources.join("node").join(node_executable),
            resources.join(node_executable),
        ];
        for node in nodes {
            if !node.is_file() {
                continue;
            }
            for root in &roots {
                let entrypoint = root.join("dist/artcraftLiveRuntime.js");
                if entrypoint.is_file() {
                    return Self::new(node, entrypoint, root);
                }
            }
        }
        Err(LiveRuntimeError::RuntimeMissing)
    }

    /// Find packaged resources first, then the source-tree connector for local development.
    /// Explicit environment overrides make this portable across workspaces and install layouts.
    pub fn discover() -> Result<Self, LiveRuntimeError> {
        let node_override = std::env::var_os("CONCAT_TIKTOK_LIVE_NODE").map(PathBuf::from);
        if let Some(connector) = std::env::var_os("CONCAT_TIKTOK_LIVE_CONNECTOR") {
            let connector = PathBuf::from(connector);
            let entrypoint = if connector.is_dir() {
                connector.join("dist/artcraftLiveRuntime.js")
            } else {
                connector.clone()
            };
            let working_directory = entrypoint
                .parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .ok_or(LiveRuntimeError::Configuration)?;
            let node = node_override
                .or_else(find_node_on_path)
                .ok_or(LiveRuntimeError::RuntimeMissing)?;
            return Self::new(node, entrypoint, working_directory);
        }

        let mut bases = Vec::new();
        if let Ok(executable) = std::env::current_exe() {
            bases.extend(executable.ancestors().map(Path::to_path_buf));
        }
        if let Ok(directory) = std::env::current_dir() {
            bases.extend(directory.ancestors().map(Path::to_path_buf));
        }
        let mut resource_roots = Vec::new();
        let mut source_connector_roots = Vec::new();
        for base in &bases {
            resource_roots.push(base.join("resources"));
            resource_roots.push(base.join("artcraft/crates/desktop/artcraft/resources"));
            // In a local debug checkout, Concat and ArtCraft are siblings under
            // the same workspace root. Keep release discovery self-contained:
            // distributed builds must use their packaged resources instead.
            if cfg!(debug_assertions) {
                source_connector_roots.push(base.join("artcraft/tools/tiktok-live-connector"));
            }
        }

        let node = node_override
            .or_else(|| {
                resource_roots
                    .iter()
                    .map(|root| root.join("node/node.exe"))
                    .find(|path| path.is_file())
            })
            .or_else(find_node_on_path)
            .ok_or(LiveRuntimeError::RuntimeMissing)?;
        for resources in &resource_roots {
            let roots = [
                resources.join("tiktok-live-connector"),
                resources.join("resources/tiktok-live-connector"),
            ];
            if let Some(root) = roots
                .iter()
                .find(|root| root.join("dist/artcraftLiveRuntime.js").is_file())
            {
                return Self::new(node, root.join("dist/artcraftLiveRuntime.js"), root);
            }
        }
        if cfg!(debug_assertions) {
            if let Some(root) = find_source_tree_connector(&source_connector_roots) {
                return Self::new(node, root.join("dist/artcraftLiveRuntime.js"), root);
            }
        }
        Err(LiveRuntimeError::RuntimeMissing)
    }
}

fn source_connector_is_runnable(root: &Path) -> bool {
    root.join("dist/artcraftLiveRuntime.js").is_file() && root.join("node_modules").is_dir()
}

fn find_source_tree_connector(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|root| source_connector_is_runnable(root))
        .cloned()
}

fn find_node_on_path() -> Option<PathBuf> {
    let executable = if cfg!(windows) { "node.exe" } else { "node" };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(executable))
        .find(|candidate| candidate.is_file())
}

/// Sanitized runtime operation failures; no child output or raw chat is exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveRuntimeError {
    /// Invalid resource path or executable name.
    Configuration,
    /// Connector or Node files are missing.
    RuntimeMissing,
    /// A process state could not be proven.
    ProcessStateUnknown,
    /// Connector process could not be spawned or used.
    ProcessFailed,
    /// Connector did not announce readiness before the fixed deadline.
    ReadyTimeout,
    /// A LIVE session is already active or transitioning.
    SessionAlreadyActive,
    /// Operation requires an active connected session.
    SessionNotConnected,
    /// Runtime is starting, stopping, or otherwise not ready.
    RuntimeNotReady,
    /// Connector frame violated the bounded protocol.
    ProtocolInvalid,
    /// Command request IDs exhausted.
    RequestIdExhausted,
}

/// Public runtime state safe to display in the native UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRuntimeStatus {
    /// `stopped`, `starting`, `ready`, `connecting`, `connected`, `stopping`,
    /// `disconnected`, `degraded`, or `failed`.
    pub state: String,
    /// PID of the child process launched by this supervisor.
    pub pid: Option<u32>,
    /// Public TikTok username, if a session was requested.
    pub username: Option<String>,
    /// Stable, sanitized failure code.
    pub error_code: Option<&'static str>,
}

struct Inner {
    status: LiveRuntimeStatus,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    process_ready: bool,
    generation: u64,
    next_request: u64,
    frames: Option<Receiver<LiveFrame>>,
}

/// Owns one connector child, its pipes and a bounded validated event queue.
pub struct LiveRuntime {
    config: LiveRuntimeConfig,
    inner: Arc<Mutex<Inner>>,
}

impl LiveRuntime {
    /// Construct a stopped supervisor. This does not launch Node or contact TikTok.
    pub fn new(config: LiveRuntimeConfig) -> Self {
        Self {
            config,
            inner: Arc::new(Mutex::new(Inner {
                status: LiveRuntimeStatus {
                    state: "stopped".into(),
                    pid: None,
                    username: None,
                    error_code: None,
                },
                child: None,
                stdin: None,
                process_ready: false,
                generation: 0,
                next_request: 1,
                frames: None,
            })),
        }
    }

    /// Read the last state reported by the owned runtime.
    pub fn status(&self) -> Result<LiveRuntimeStatus, LiveRuntimeError> {
        self.inner
            .lock()
            .map(|inner| inner.status.clone())
            .map_err(|_| LiveRuntimeError::ProcessStateUnknown)
    }

    /// Connect to one explicitly supplied TikTok username.
    pub fn start(&self, username: &str) -> Result<LiveRuntimeStatus, LiveRuntimeError> {
        let command = self.command(|request_id| LiveCommand::Start {
            request_id,
            username: username.to_owned(),
        })?;
        self.ensure_process()?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
        match inner.status.state.as_str() {
            "connecting" | "connected" | "degraded" => {
                return Err(LiveRuntimeError::SessionAlreadyActive);
            }
            "starting" | "stopping" => return Err(LiveRuntimeError::RuntimeNotReady),
            "ready" | "stopped" | "failed" | "disconnected" if inner.process_ready => {}
            _ => return Err(LiveRuntimeError::RuntimeNotReady),
        }
        write_command(&mut inner, &command)?;
        inner.status.state = "connecting".into();
        inner.status.username = Some(username.trim_start_matches('@').to_owned());
        inner.status.error_code = None;
        Ok(inner.status.clone())
    }

    /// Inspect a public username without opening a LIVE session.
    pub fn inspect(&self, username: &str) -> Result<(), LiveRuntimeError> {
        let command = self.command(|request_id| LiveCommand::Inspect {
            request_id,
            username: username.to_owned(),
        })?;
        self.ensure_process()?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
        if !inner.process_ready {
            return Err(LiveRuntimeError::RuntimeNotReady);
        }
        write_command(&mut inner, &command)
    }

    /// Send a chat message only after the connector reports a connected session.
    pub fn send_message(&self, content: &str) -> Result<(), LiveRuntimeError> {
        let command = self.command(|request_id| LiveCommand::SendMessage {
            request_id,
            content: content.to_owned(),
        })?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
        if inner.status.state != "connected" {
            return Err(LiveRuntimeError::SessionNotConnected);
        }
        write_command(&mut inner, &command)
    }

    /// Request disconnection; the process remains owned until shutdown.
    pub fn stop_session(&self) -> Result<LiveRuntimeStatus, LiveRuntimeError> {
        let command = self.command(|request_id| LiveCommand::Stop { request_id })?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
        if !inner.process_ready {
            return Err(LiveRuntimeError::RuntimeNotReady);
        }
        write_command(&mut inner, &command)?;
        inner.status.state = "stopping".into();
        inner.status.username = None;
        inner.status.error_code = None;
        Ok(inner.status.clone())
    }

    /// Read one validated event without waiting; call from an application worker.
    pub fn try_next_frame(&self) -> Result<Option<LiveFrame>, LiveRuntimeError> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
        let Some(frames) = inner.frames.as_ref() else {
            return Ok(None);
        };
        match frames.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => Ok(None),
        }
    }

    /// Stop the owned connector process, bounded to three seconds before killing it.
    pub fn shutdown(&self) -> Result<LiveRuntimeStatus, LiveRuntimeError> {
        let mut child = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
            inner.generation = inner.generation.wrapping_add(1);
            if inner.process_ready {
                let command = LiveCommand::Shutdown {
                    request_id: next_request_id(&mut inner)?,
                };
                if let Ok(bytes) = command.encode() {
                    let _ = write_command(&mut inner, &bytes);
                }
            }
            inner.stdin.take();
            inner.process_ready = false;
            inner.frames.take();
            inner.status.state = "stopping".into();
            inner.status.username = None;
            inner.child.take()
        };
        let failed = if let Some(process) = child.as_mut() {
            terminate_bounded(process).is_err()
        } else {
            false
        };
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
        if failed {
            inner.status.state = "degraded".into();
            inner.status.error_code = Some("LIVE_RUNTIME_TERMINATION_FAILED");
            inner.child = child;
            return Err(LiveRuntimeError::ProcessFailed);
        }
        inner.status = LiveRuntimeStatus {
            state: "stopped".into(),
            pid: None,
            username: None,
            error_code: None,
        };
        Ok(inner.status.clone())
    }

    fn command(
        &self,
        build: impl FnOnce(String) -> LiveCommand,
    ) -> Result<Vec<u8>, LiveRuntimeError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
        let request_id = next_request_id(&mut inner)?;
        build(request_id)
            .encode()
            .map_err(|_| LiveRuntimeError::ProtocolInvalid)
    }

    fn ensure_process(&self) -> Result<(), LiveRuntimeError> {
        let generation = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
            let child_state = match inner.child.as_mut() {
                Some(child) => Some(child.try_wait()),
                None => None,
            };
            if let Some(wait) = child_state {
                match wait {
                    Ok(None) if inner.process_ready => return Ok(()),
                    Ok(None) => return Err(LiveRuntimeError::RuntimeNotReady),
                    Ok(Some(_)) => {
                        inner.child.take();
                        inner.stdin.take();
                        inner.frames.take();
                        inner.process_ready = false;
                        inner.status = LiveRuntimeStatus {
                            state: "stopped".into(),
                            pid: None,
                            username: None,
                            error_code: None,
                        };
                    }
                    Err(_) => {
                        inner.status.state = "degraded".into();
                        inner.status.error_code = Some("LIVE_RUNTIME_PROCESS_STATE_UNKNOWN");
                        return Err(LiveRuntimeError::ProcessStateUnknown);
                    }
                }
            }
            if inner.status.state == "starting" {
                return Err(LiveRuntimeError::RuntimeNotReady);
            }
            inner.generation = inner.generation.wrapping_add(1);
            inner.status.state = "starting".into();
            inner.status.error_code = None;
            inner.generation
        };

        let spawn = Command::new(&self.config.node)
            .arg(&self.config.entrypoint)
            .current_dir(&self.config.working_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match spawn {
            Ok(child) => child,
            Err(_) => {
                self.fail_generation(generation, "LIVE_RUNTIME_SPAWN_FAILED");
                return Err(LiveRuntimeError::ProcessFailed);
            }
        };
        let pid = child.id();
        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            self.fail_generation(generation, "LIVE_RUNTIME_STDIN_UNAVAILABLE");
            return Err(LiveRuntimeError::ProcessFailed);
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            self.fail_generation(generation, "LIVE_RUNTIME_STDOUT_UNAVAILABLE");
            return Err(LiveRuntimeError::ProcessFailed);
        };
        let (frame_tx, frame_rx) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
        let (ready_tx, ready_rx) = mpsc::channel();
        {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
            if inner.generation != generation {
                let _ = child.kill();
                let _ = child.wait();
                return Err(LiveRuntimeError::RuntimeNotReady);
            }
            inner.status.pid = Some(pid);
            inner.child = Some(child);
            inner.stdin = Some(stdin);
            inner.frames = Some(frame_rx);
        }
        let state = Arc::clone(&self.inner);
        let reader = thread::Builder::new()
            .name("concat-tiktok-live-reader".into())
            .spawn(move || {
                read_runtime(stdout, state, generation, frame_tx, ready_tx);
            });
        if reader.is_err() {
            self.fail_generation(generation, "LIVE_RUNTIME_READER_SPAWN_FAILED");
            self.terminate_generation(generation);
            return Err(LiveRuntimeError::ProcessFailed);
        }
        match ready_rx.recv_timeout(READY_TIMEOUT) {
            Ok(Ok(())) => {
                let mut inner = self
                    .inner
                    .lock()
                    .map_err(|_| LiveRuntimeError::ProcessStateUnknown)?;
                if inner.generation != generation {
                    return Err(LiveRuntimeError::RuntimeNotReady);
                }
                inner.process_ready = true;
                if inner.status.state == "starting" {
                    inner.status.state = "ready".into();
                }
                Ok(())
            }
            Ok(Err(error)) => {
                self.fail_generation(generation, "LIVE_RUNTIME_PROTOCOL_INVALID");
                self.terminate_generation(generation);
                Err(error)
            }
            Err(_) => {
                self.fail_generation(generation, "LIVE_RUNTIME_READY_TIMEOUT");
                self.terminate_generation(generation);
                Err(LiveRuntimeError::ReadyTimeout)
            }
        }
    }

    fn fail_generation(&self, generation: u64, code: &'static str) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.generation == generation {
                inner.process_ready = false;
                inner.status.state = "failed".into();
                inner.status.error_code = Some(code);
            }
        }
    }

    fn terminate_generation(&self, generation: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.generation == generation {
                inner.stdin.take();
                inner.frames.take();
                if let Some(child) = inner.child.as_mut() {
                    let _ = terminate_bounded(child);
                }
                inner.child.take();
                inner.process_ready = false;
                inner.status.pid = None;
            }
        }
    }
}

impl Drop for LiveRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn write_command(inner: &mut Inner, bytes: &[u8]) -> Result<(), LiveRuntimeError> {
    let stdin = inner
        .stdin
        .as_mut()
        .ok_or(LiveRuntimeError::RuntimeNotReady)?;
    stdin
        .write_all(bytes)
        .and_then(|()| stdin.write_all(b"\n"))
        .and_then(|()| stdin.flush())
        .map_err(|_| LiveRuntimeError::ProcessFailed)
}

fn next_request_id(inner: &mut Inner) -> Result<String, LiveRuntimeError> {
    if inner.next_request >= REQUEST_COUNTER_LIMIT {
        return Err(LiveRuntimeError::RequestIdExhausted);
    }
    let id = format!("concat-live-{}", inner.next_request);
    inner.next_request += 1;
    Ok(id)
}

fn read_runtime(
    stdout: impl Read,
    inner: Arc<Mutex<Inner>>,
    generation: u64,
    sender: SyncSender<LiveFrame>,
    ready: mpsc::Sender<Result<(), LiveRuntimeError>>,
) {
    let mut reader = BufReader::new(stdout);
    let mut ready_sent = false;
    loop {
        let line = match read_bounded_line(&mut reader) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => {
                if !ready_sent {
                    let _ = ready.send(Err(error));
                }
                if let Ok(mut state) = inner.lock() {
                    if state.generation == generation {
                        state.status.state = "failed".into();
                        state.status.error_code = Some("LIVE_RUNTIME_PROTOCOL_INVALID");
                    }
                }
                return;
            }
        };
        let frame = match parse_frame(&line) {
            Ok(frame) => frame,
            Err(_) => {
                if !ready_sent {
                    let _ = ready.send(Err(LiveRuntimeError::ProtocolInvalid));
                }
                if let Ok(mut state) = inner.lock() {
                    if state.generation == generation {
                        state.status.state = "failed".into();
                        state.status.error_code = Some("LIVE_RUNTIME_PROTOCOL_INVALID");
                    }
                }
                return;
            }
        };
        if frame.kind == LiveFrameKind::Ready && !ready_sent {
            ready_sent = true;
            let _ = ready.send(Ok(()));
        }
        if frame.kind == LiveFrameKind::Status {
            if let Some(status) = frame
                .value
                .get("status")
                .and_then(serde_json::Value::as_str)
            {
                if let Ok(mut state) = inner.lock() {
                    if state.generation == generation {
                        state.status.state = status.to_owned();
                        if matches!(status, "disconnected" | "stopped" | "failed") {
                            state.status.username = None;
                        }
                    }
                }
            }
        }
        if sender.send(frame).is_err() {
            return;
        }
    }
    if !ready_sent {
        let _ = ready.send(Err(LiveRuntimeError::ProcessFailed));
    }
    if let Ok(mut state) = inner.lock() {
        if state.generation == generation {
            state.process_ready = false;
            state.status.state = "stopped".into();
            state.status.pid = None;
        }
    }
}

fn read_bounded_line(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, LiveRuntimeError> {
    let mut line = Vec::with_capacity(1024);
    loop {
        let available = reader
            .fill_buf()
            .map_err(|_| LiveRuntimeError::ProcessFailed)?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some(line))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        if line.len().saturating_add(count) > MAX_FRAME_BYTES + 1 {
            return Err(LiveRuntimeError::ProtocolInvalid);
        }
        let done = newline.is_some();
        line.extend_from_slice(&available[..count]);
        reader.consume(count);
        if done {
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Some(line));
        }
    }
}

fn terminate_bounded(child: &mut Child) -> std::io::Result<()> {
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait()?;
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn resource_resolution_requires_only_expected_files() {
        let root = temp_root();
        let resources = root.join("resources");
        let connector = resources.join("tiktok-live-connector");
        let node = resources.join("node");
        fs::create_dir_all(connector.join("dist")).expect("connector dir");
        fs::create_dir_all(&node).expect("node dir");
        let node_path = node.join(if cfg!(windows) { "node.exe" } else { "node" });
        fs::write(&node_path, b"fixture").expect("node fixture");
        fs::write(connector.join("dist/artcraftLiveRuntime.js"), b"fixture")
            .expect("entrypoint fixture");
        let resolved = LiveRuntimeConfig::from_resources(
            &resources,
            if cfg!(windows) { "node.exe" } else { "node" },
        )
        .expect("resolve fixture");
        assert_eq!(resolved.working_directory, connector);
        assert_eq!(
            LiveRuntimeConfig::from_resources(&resources, "powershell.exe").err(),
            Some(LiveRuntimeError::Configuration)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn debug_source_discovery_requires_built_entrypoint_and_dependencies() {
        let root = temp_root();
        let connector = root.join("artcraft/tools/tiktok-live-connector");
        fs::create_dir_all(connector.join("dist")).expect("connector dist");
        fs::write(connector.join("dist/artcraftLiveRuntime.js"), b"fixture")
            .expect("connector entrypoint");
        assert!(!source_connector_is_runnable(&connector));
        fs::create_dir_all(connector.join("node_modules")).expect("connector dependencies");
        assert!(source_connector_is_runnable(&connector));
        assert_eq!(
            find_source_tree_connector(std::slice::from_ref(&connector)),
            Some(connector.clone())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn supervisor_handshakes_sends_and_terminates_only_its_fixture_child() {
        let Ok(node) = find_node() else { return };
        let root = temp_root();
        fs::create_dir_all(&root).expect("fixture dir");
        let entrypoint = root.join("fixture.js");
        fs::write(&entrypoint, r#"
const readline = require('readline');
const line = (value) => process.stdout.write(JSON.stringify(value) + '\n');
line({protocolVersion:1,type:'runtime.ready',runtime:'artcraft-tiktok-live',runtimeVersion:'test',nodeVersion:process.version});
const input = readline.createInterface({input:process.stdin});
input.on('line',(raw)=>{ const command=JSON.parse(raw); if(command.type==='start'){ line({protocolVersion:1,type:'runtime.status',status:'connected'}); } else if(command.type==='stop'){ line({protocolVersion:1,type:'runtime.status',status:'disconnected'}); } else if(command.type==='sendMessage'){ line({protocolVersion:1,type:'command.accepted',requestId:command.requestId}); } else if(command.type==='shutdown'){ process.exit(0); } });
"#).expect("write connector fixture");
        let runtime =
            LiveRuntime::new(LiveRuntimeConfig::new(node, &entrypoint, &root).expect("config"));
        assert_eq!(
            runtime.start("@creator").expect("connect request").state,
            "connecting"
        );
        wait_for_state(&runtime, "connected");
        runtime
            .send_message("xin chào")
            .expect("send through connected session");
        assert_eq!(
            runtime.stop_session().expect("stop request").state,
            "stopping"
        );
        wait_for_state(&runtime, "disconnected");
        assert_eq!(
            runtime
                .start("creator")
                .expect("reconnect after stop")
                .state,
            "connecting"
        );
        wait_for_state(&runtime, "connected");
        assert_eq!(
            runtime.shutdown().expect("shutdown owned child").state,
            "stopped"
        );
        assert_eq!(runtime.status().expect("stopped state").pid, None);
        let _ = fs::remove_dir_all(root);
    }

    fn wait_for_state(runtime: &LiveRuntime, wanted: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if runtime.status().is_ok_and(|status| status.state == wanted) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("runtime did not reach {wanted}");
    }

    fn find_node() -> Result<PathBuf, ()> {
        let executable = if cfg!(windows) { "node.exe" } else { "node" };
        std::env::split_paths(&std::env::var_os("PATH").ok_or(())?)
            .map(|directory| directory.join(executable))
            .find(|candidate| candidate.is_file())
            .ok_or(())
    }

    fn temp_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "concat-live-runtime-{}-{nonce}",
            std::process::id()
        ))
    }
}
