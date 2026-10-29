// SPDX-License-Identifier: AGPL-3.0-or-later
//! Durable control-plane state. Executors use existing services, never UI simulations.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

/// Runtime policies are owned by the user interface, not model output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    FullAuto,
    Supervised,
    Manual,
}
/// Persisted states distinguish unknown side effects from recoverable failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Running,
    WaitingInput,
    WaitingApproval,
    Paused,
    Done,
    Failed,
    Cancelled,
    Unknown,
}
/// Actual capability contract, including execution and recovery policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Capability {
    pub name: String,
    pub description: String,
    pub module: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub scope: String,
    pub read_only: bool,
    pub replay_safe: bool,
    pub approval_required: bool,
    pub timeout_seconds: u64,
}
/// Explicit registry. Merely having a source file never enables a capability.
#[derive(Clone, Default)]
pub struct Registry(BTreeMap<String, Capability>);
impl Registry {
    /// Reject duplicate registrations and invalid limits.
    pub fn register(&mut self, cap: Capability) -> Result<(), String> {
        if cap.name.is_empty()
            || cap.name.len() > 128
            || !(1..=1800).contains(&cap.timeout_seconds)
            || self.0.contains_key(&cap.name)
            || !cap.input_schema.is_object()
            || !cap.output_schema.is_object()
        {
            return Err("Invalid or duplicate capability".into());
        }
        self.0.insert(cap.name.clone(), cap);
        Ok(())
    }
    /// Enumerate only installed executors.
    pub fn list(&self) -> Vec<Capability> {
        self.0.values().cloned().collect()
    }
    /// Discover one registered capability.
    pub fn get(&self, name: &str) -> Result<&Capability, String> {
        self.0
            .get(name)
            .ok_or_else(|| format!("Capability unavailable: {name}"))
    }
    /// Enforce the registry's bounded JSON contract at the executor boundary.
    pub fn validate(&self, name: &str, value: &Value, output: bool) -> Result<(), String> {
        if serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
            return Err("Capability data exceeds 1 MiB".into());
        }
        let cap = self.get(name)?;
        validate_json(
            value,
            if output {
                &cap.output_schema
            } else {
                &cap.input_schema
            },
            0,
        )
    }
}
fn validate_json(value: &Value, schema: &Value, depth: u32) -> Result<(), String> {
    if depth > 32 {
        return Err("Capability data nesting limit".into());
    }
    let matches = |kind: &str| match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    };
    if let Some(kind) = schema.get("type") {
        let valid = kind.as_str().is_some_and(matches)
            || kind
                .as_array()
                .is_some_and(|a| a.iter().any(|k| k.as_str().is_some_and(matches)));
        if !valid {
            return Err("Capability value has the wrong type".into());
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for k in required {
                if !k.as_str().is_some_and(|k| object.contains_key(k)) {
                    return Err("Missing required capability field".into());
                }
            }
        }
        for (key, v) in object {
            if let Some(s) = schema["properties"].get(key) {
                validate_json(v, s, depth + 1)?;
            } else if schema["additionalProperties"] == false {
                return Err(format!("Unknown capability field: {key}"));
            }
        }
    }
    if let Some(array) = value.as_array() {
        if schema["minItems"]
            .as_u64()
            .is_some_and(|n| array.len() < (n as usize))
            || schema["maxItems"]
                .as_u64()
                .is_some_and(|n| array.len() > (n as usize))
        {
            return Err("Capability array size is invalid".into());
        }
        if let Some(items) = schema.get("items") {
            for v in array {
                validate_json(v, items, depth + 1)?;
            }
        }
    }
    if let Some(s) = value.as_str() {
        let n = s.chars().count() as u64;
        if schema["minLength"].as_u64().is_some_and(|m| n < m)
            || schema["maxLength"].as_u64().is_some_and(|m| n > m)
        {
            return Err("Capability text length is invalid".into());
        }
    }
    if let Some(n) = value.as_f64() {
        if !n.is_finite()
            || schema["minimum"].as_f64().is_some_and(|m| n < m)
            || schema["maximum"].as_f64().is_some_and(|m| n > m)
        {
            return Err("Capability number is out of range".into());
        }
    }
    Ok(())
}
/// Dependency-aware workflow step; inputs are data, never executable source.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Step {
    pub id: String,
    pub tool: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub input: Value,
    pub status: Status,
    #[serde(default)]
    pub output: Option<Value>,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub approval_digest: Option<String>,
}
/// One durable source of workflow state, consumed by GUI and MCP alike.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Workflow {
    pub version: u32,
    pub id: String,
    pub command: String,
    pub mode: Mode,
    pub revision: u64,
    pub paused: bool,
    pub cancelled: bool,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub activity: Vec<String>,
}
/// Revision-bound dispatch receipt, minted only after the running state is durable.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct Ticket {
    pub workflow: String,
    pub step: String,
    pub revision: u64,
    pub attempt: u32,
    pub digest: String,
    pub tool: String,
    pub input: Value,
}
/// Inputs + workflow revision bind approvals and returned worker results.
pub fn digest(input: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(input).unwrap_or_default())
    )
}

/// Extract explicit numeric minute/second goals; product quantities are ignored.
pub fn requested_seconds(command: &str) -> Result<Option<f64>, String> {
    let chars: Vec<char> = command.to_lowercase().chars().collect();
    let mut index = 0;
    let mut total = 0.0;
    let mut found = false;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len()
            && (chars[index].is_ascii_digit() || matches!(chars[index], '.' | ','))
        {
            index += 1;
        }
        let numeric: String = chars[start..index]
            .iter()
            .collect::<String>()
            .replace(',', ".");
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        let start = index;
        while index < chars.len() && chars[index].is_alphabetic() {
            index += 1;
        }
        let unit: String = chars[start..index].iter().collect();
        let scale = match unit.as_str() {
            "giờ" | "gio" | "hour" | "hours" => 3600.0,
            "phút" | "phut" | "minute" | "minutes" | "min" => 60.0,
            "giây" | "giay" | "second" | "seconds" | "sec" | "s" => 1.0,
            _ => continue,
        };
        let value = numeric
            .parse::<f64>()
            .map_err(|_| "Thời lượng không hợp lệ")?;
        total += value * scale;
        found = true;
    }
    if found && (!total.is_finite() || !(1.0..=600.0).contains(&total)) {
        return Err("Workflow hiện hỗ trợ yêu cầu thời lượng trong 1–600 giây".into());
    }
    Ok(found.then_some(total))
}

impl Workflow {
    /// Validate the entire DAG before anything can run.
    pub fn new(
        command: String,
        mode: Mode,
        steps: Vec<Step>,
        registry: &Registry,
    ) -> Result<Self, String> {
        if command.trim().is_empty()
            || command.len() > 16000
            || steps.is_empty()
            || steps.len() > 512
        {
            return Err("Invalid workflow size".into());
        }
        let ids: BTreeSet<_> = steps.iter().map(|s| s.id.as_str()).collect();
        if ids.len() != steps.len() {
            return Err("Duplicate step ID".into());
        }
        for s in &steps {
            registry.get(&s.tool)?;
            if s.id.is_empty()
                || s.id.len() > 128
                || !s
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || s.status != Status::Pending
                || s.output.is_some()
                || s.attempts != 0
                || s.approval_digest.is_some()
                || !s.input.is_object()
                || serde_json::to_vec(&s.input)
                    .map_err(|e| e.to_string())?
                    .len()
                    > 65536
                || s.dependencies
                    .iter()
                    .any(|d| d == &s.id || !ids.contains(d.as_str()))
            {
                return Err("Invalid step or dependency".into());
            }
        }
        let mut visited = BTreeSet::new();
        loop {
            let before = visited.len();
            for s in &steps {
                if s.dependencies.iter().all(|d| visited.contains(d)) {
                    visited.insert(s.id.clone());
                }
            }
            if visited.len() == steps.len() {
                break;
            }
            if before == visited.len() {
                return Err("Dependency cycle".into());
            }
        }
        Ok(Self {
            version: 1,
            id: format!("control-{:032x}", rand::random::<u128>()),
            command,
            mode,
            revision: 0,
            paused: false,
            cancelled: false,
            steps,
            activity: vec!["Workflow created".into()],
        })
    }
    /// All steps must have validated receipts, never an optimistic progress flag.
    pub fn finished(&self) -> bool {
        self.steps.iter().all(|s| s.status == Status::Done)
    }
    /// Whether Resume can advance this graph, without replaying a dispatched write.
    /// Failed dependencies do not make their pending children runnable.
    pub fn can_continue(&self, registry: &Registry) -> bool {
        !self.cancelled && self.steps.iter().any(|s| {
            s.dependencies.iter().all(|id| self.steps.iter().any(|d| &d.id == id && d.status == Status::Done))
                && registry.get(&s.tool).is_ok_and(|cap| {
                    s.status == Status::Pending
                        || (matches!(s.status, Status::Failed | Status::WaitingInput)
                            && s.attempts < 3
                            && (cap.replay_safe || (s.status == Status::WaitingInput && s.attempts == 0)))
                })
        })
    }
    fn step_mut(&mut self, id: &str) -> Result<&mut Step, String> {
        self.steps
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or("Unknown step".into())
    }
    /// Human-only approval seam. Bind it to exactly the input displayed for review.
    pub fn approve(&mut self, id: &str, expected: &str) -> Result<(), String> {
        let reviewed = self.review_input(id)?;
        let s = self.step_mut(id)?;
        if s.status != Status::WaitingApproval || digest(&reviewed) != expected {
            return Err("Approval is stale".into());
        }
        s.approval_digest = Some(expected.into());
        s.status = Status::Pending;
        self.revision += 1;
        Ok(())
    }
    /// Resolve only completed, declared dependencies for the human review surface.
    pub fn review_input(&self, id: &str) -> Result<Value, String> {
        let s = self
            .steps
            .iter()
            .find(|s| s.id == id)
            .ok_or("Unknown step")?;
        let outputs = self
            .steps
            .iter()
            .filter(|s| s.status == Status::Done)
            .filter_map(|s| s.output.clone().map(|v| (s.id.clone(), v)))
            .collect();
        resolve(&s.input, &s.dependencies, &outputs, 0)
    }
    /// Change mode without turning pending sensitive actions into implicit approvals.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.revision += 1;
    }
    /// Call only after the executor has really stopped. Unknown writes cannot resume.
    pub fn pause(&mut self) -> Result<(), String> {
        if self.steps.iter().any(|s| s.status == Status::Running) {
            return Err("Cancel the active executor and wait for settlement first".into());
        }
        self.paused = true;
        self.revision += 1;
        Ok(())
    }
    /// Resume from the same persistent graph, without clearing unknown outcomes.
    pub fn resume(&mut self) {
        self.paused = false;
        self.revision += 1;
    }
    /// User cancellation never erases committed outputs or claims rollback.
    pub fn cancel(&mut self) -> Result<(), String> {
        if self.steps.iter().any(|s| s.status == Status::Running) {
            return Err("Stop the active executor first".into());
        }
        self.cancelled = true;
        for s in &mut self.steps {
            if s.status != Status::Done {
                s.status = Status::Cancelled;
            }
        }
        self.revision += 1;
        Ok(())
    }
    /// Patch after manual takeover. Completed downstream work is invalidated explicitly.
    pub fn patch_input(
        &mut self,
        id: &str,
        input: Value,
        expected_revision: u64,
    ) -> Result<(), String> {
        if self.cancelled
            || self.revision != expected_revision
            || self.steps.iter().any(|s| s.status == Status::Running)
            || !input.is_object()
            || serde_json::to_vec(&input).map_err(|e| e.to_string())?.len() > 65536
        {
            return Err("Workflow changed or executor still active".into());
        }
        self.step_mut(id)?;
        let mut dirty = BTreeSet::from([id.to_owned()]);
        loop {
            let n = dirty.len();
            for s in &self.steps {
                if s.dependencies.iter().any(|d| dirty.contains(d)) {
                    dirty.insert(s.id.clone());
                }
            }
            if n == dirty.len() {
                break;
            }
        }
        if self
            .steps
            .iter()
            .any(|s| dirty.contains(&s.id) && s.status == Status::Unknown)
        {
            return Err("Reconcile unknown outcomes before restarting affected steps".into());
        }
        for s in &mut self.steps {
            if dirty.contains(&s.id) {
                s.status = Status::Pending;
                s.output = None;
                s.error = None;
                s.approval_digest = None;
            }
        }
        self.step_mut(id)?.input = input;
        self.revision += 1;
        self.activity.push(format!("Manual update: {id}"));
        Ok(())
    }
    /// Select a ready step using runtime-granted scopes and current approval policy.
    fn claim(
        &mut self,
        registry: &Registry,
        scopes: &BTreeSet<String>,
    ) -> Result<Option<Ticket>, String> {
        if self.paused || self.cancelled || self.steps.iter().any(|s| s.status == Status::Running) {
            return Ok(None);
        }
        let done: BTreeSet<_> = self
            .steps
            .iter()
            .filter(|s| s.status == Status::Done)
            .map(|s| s.id.clone())
            .collect();
        let outputs: BTreeMap<_, _> = self
            .steps
            .iter()
            .filter_map(|s| s.output.clone().map(|v| (s.id.clone(), v)))
            .collect();
        let mode = self.mode;
        for s in &mut self.steps {
            if s.status != Status::Pending || !s.dependencies.iter().all(|d| done.contains(d)) {
                continue;
            }
            let cap = registry.get(&s.tool)?;
            if !scopes.contains(&cap.scope) {
                s.status = Status::WaitingInput;
                s.error = Some(format!("Missing permission: {}", cap.scope));
                self.revision += 1;
                return Ok(None);
            }
            let hash = digest(&s.input);
            let resolved = resolve(&s.input, &s.dependencies, &outputs, 0)?;
            if (cap.approval_required
                || mode == Mode::Manual
                || (mode == Mode::Supervised && !cap.read_only))
                && s.approval_digest.as_deref() != Some(digest(&resolved).as_str())
            {
                s.status = Status::WaitingApproval;
                self.revision += 1;
                return Ok(None);
            }
            s.status = Status::Running;
            s.attempts += 1;
            s.error = None;
            self.revision += 1;
            self.activity.push(format!("Started {} ({})", s.id, s.tool));
            return Ok(Some(Ticket {
                workflow: self.id.clone(),
                step: s.id.clone(),
                revision: self.revision,
                attempt: s.attempts,
                digest: hash,
                tool: s.tool.clone(),
                input: resolved,
            }));
        }
        Ok(None)
    }
    /// Settle once. Worker success must include an executor-validated output.
    pub fn settle(
        &mut self,
        ticket: &Ticket,
        outcome: Result<Value, Failure>,
    ) -> Result<(), String> {
        if self.id != ticket.workflow {
            return Err("Stale worker receipt".into());
        }
        let s = self.step_mut(&ticket.step)?;
        if s.status != Status::Running
            || s.attempts != ticket.attempt
            || digest(&s.input) != ticket.digest
        {
            return Err("Worker input changed".into());
        }
        match outcome {
            Ok(value) => {
                s.output = Some(value);
                s.status = Status::Done;
            }
            Err(e) => {
                s.status = if e.unknown {
                    Status::Unknown
                } else if e.waiting_input {
                    Status::WaitingInput
                } else {
                    Status::Failed
                };
                s.error = Some(e.message);
            }
        }
        self.revision += 1;
        self.activity.push(format!("Settled {}", ticket.step));
        Ok(())
    }
    /// Only explicit safe retry; writes with an unknown outcome are never replayed.
    pub fn retry(&mut self, id: &str, registry: &Registry) -> Result<(), String> {
        let s = self.step_mut(id)?;
        let cap = registry.get(&s.tool)?;
        let never_dispatched = s.attempts == 0 && s.status == Status::WaitingInput;
        if (!cap.replay_safe && !never_dispatched)
            || s.attempts >= 3
            || !(matches!(s.status, Status::Failed | Status::WaitingInput)
                || (s.status == Status::Unknown && cap.read_only && cap.replay_safe))
        {
            return Err("This outcome cannot be safely replayed".into());
        }
        s.status = Status::Pending;
        s.error = None;
        s.approval_digest = None;
        self.revision += 1;
        Ok(())
    }
}
fn resolve(
    value: &Value,
    dependencies: &[String],
    outputs: &BTreeMap<String, Value>,
    depth: u32,
) -> Result<Value, String> {
    if depth > 32 {
        return Err("Input nesting limit".into());
    }
    if let Some(object) = value.as_object() {
        if let Some(id) = object.get("$step").and_then(Value::as_str) {
            if object.len() != 2 || !dependencies.iter().any(|d| d == id) {
                return Err("Invalid output reference".into());
            }
            let pointer = object
                .get("pointer")
                .and_then(Value::as_str)
                .ok_or("Missing output pointer")?;
            return outputs
                .get(id)
                .and_then(|v| v.pointer(pointer))
                .cloned()
                .ok_or("Missing dependency output".into());
        }
        return object
            .iter()
            .map(|(k, v)| Ok((k.clone(), resolve(v, dependencies, outputs, depth + 1)?)))
            .collect::<Result<serde_json::Map<String, Value>, String>>()
            .map(Value::Object);
    }
    if let Some(array) = value.as_array() {
        return array
            .iter()
            .map(|v| resolve(v, dependencies, outputs, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array);
    }
    Ok(value.clone())
}
/// Typed failure classifications; raw provider messages should not enter the ledger.
#[derive(Debug)]
#[allow(missing_docs)]
pub struct Failure {
    pub message: String,
    pub unknown: bool,
    pub waiting_input: bool,
}
/// One process owns the store; atomic snapshots preserve the previous checkpoint.
pub struct Store {
    root: PathBuf,
    _owner: File,
}
impl Store {
    /// Windows share-mode lock is released by the OS even after a crash.
    pub fn open(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let owner = options
            .open(root.join("owner.lock"))
            .map_err(|_| "Another control runtime owns this store".to_owned())?;
        Ok(Self {
            root: root.to_owned(),
            _owner: owner,
        })
    }
    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if !id.starts_with("control-")
            || id.len() != 40
            || !id[8..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("Invalid workflow ID".into());
        }
        Ok(self.root.join(format!("{id}.json")))
    }
    /// Atomic write, followed by directory-independent in-process ownership.
    pub fn save(&self, workflow: &Workflow) -> Result<(), String> {
        let dest = self.path(&workflow.id)?;
        let temp = dest.with_extension("tmp");
        let bytes = serde_json::to_vec_pretty(workflow).map_err(|e| e.to_string())?;
        let mut file = File::create(&temp).map_err(|e| e.to_string())?;
        use std::io::Write;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(temp, dest).map_err(|e| e.to_string())
    }
    /// A crashed in-flight write becomes unknown, not successful or auto-retried.
    pub fn load(&self, id: &str) -> Result<Workflow, String> {
        let path = self.path(id)?;
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
            return Err("Workflow too large".into());
        }
        let mut w: Workflow = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if w.version != 1 || w.id != id {
            return Err("Unsupported workflow".into());
        }
        let mut interrupted = false;
        for s in &mut w.steps {
            if s.status == Status::Running {
                s.status = Status::Unknown;
                s.error =
                    Some("Execution interrupted; verify the outcome before continuing".into());
                interrupted = true;
            }
        }
        if interrupted {
            w.revision += 1;
            self.save(&w)?;
        }
        Ok(w)
    }
    /// Persist running state before handing work to any executor.
    pub fn claim(
        &self,
        w: &mut Workflow,
        registry: &Registry,
        scopes: &BTreeSet<String>,
    ) -> Result<Option<Ticket>, String> {
        let mut next = w.clone();
        let ticket = next.claim(registry, scopes)?;
        self.save(&next)?;
        *w = next;
        Ok(ticket)
    }
    /// Discover saved workflows; invalid files are not surfaced as successful tasks.
    pub fn list(&self) -> Vec<String> {
        fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                (p.extension()?.to_str()? == "json")
                    .then(|| p.file_stem()?.to_str().map(str::to_owned))
                    .flatten()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn registry() -> Registry {
        let mut r = Registry::default();
        for (name, read, sensitive) in [
            ("read", true, false),
            ("write", false, false),
            ("publish", false, true),
        ] {
            r.register(Capability {
                name: name.into(),
                description: name.into(),
                module: "test".into(),
                input_schema: json!({"type":"object"}),
                output_schema: json!({"type":"object"}),
                scope: "local".into(),
                read_only: read,
                replay_safe: read,
                approval_required: sensitive,
                timeout_seconds: 30,
            })
            .unwrap();
        }
        r
    }
    fn step(id: &str, tool: &str, deps: Vec<&str>) -> Step {
        Step {
            id: id.into(),
            tool: tool.into(),
            dependencies: deps.into_iter().map(str::to_owned).collect(),
            input: json!({}),
            status: Status::Pending,
            output: None,
            attempts: 0,
            error: None,
            approval_digest: None,
        }
    }
    fn scopes() -> BTreeSet<String> {
        BTreeSet::from(["local".into()])
    }
    #[test]
    fn continue_requires_runnable_work_and_never_replays_dispatched_writes() {
        let r = registry();
        let mut w = Workflow::new("request".into(), Mode::FullAuto,
            vec![step("plan", "write", vec![]), step("child", "read", vec!["plan"])], &r).unwrap();
        assert!(w.can_continue(&r));
        w.steps[0].status = Status::Failed;
        w.steps[0].attempts = 1;
        assert!(!w.can_continue(&r)); // Also covers old saved failed planners.
        w.steps[0].status = Status::Unknown;
        assert!(!w.can_continue(&r));
        w.steps[0].status = Status::WaitingInput;
        w.steps[0].attempts = 0;
        assert!(w.can_continue(&r)); // Permission was missing before dispatch.
        w.steps[0].status = Status::Done;
        assert!(w.can_continue(&r));
        w.cancelled = true;
        assert!(!w.can_continue(&r));
    }
    #[test]
    fn continue_preserves_safe_retries_but_respects_attempt_limit_and_approval() {
        let r = registry();
        let mut w = Workflow::new("request".into(), Mode::FullAuto, vec![step("read", "read", vec![])], &r).unwrap();
        w.steps[0].status = Status::Failed;
        w.steps[0].attempts = 1;
        assert!(w.can_continue(&r));
        w.steps[0].attempts = 3;
        assert!(!w.can_continue(&r));
        w.steps[0].status = Status::WaitingApproval;
        assert!(!w.can_continue(&r));
        w.steps[0].status = Status::Done;
        assert!(!w.can_continue(&r));
    }
    #[test]
    fn executor_contracts_reject_unknown_fields_wrong_types_and_unbounded_arrays() {
        let schema = json!({"type":"object","additionalProperties":false,"required":["paths"],"properties":{"paths":{"type":"array","minItems":1,"maxItems":2,"items":{"type":"string","minLength":1}}}});
        assert!(validate_json(&json!({"paths":["real.mp4"]}), &schema, 0).is_ok());
        for input in [
            json!({}),
            json!({"paths":[]}),
            json!({"paths":[3]}),
            json!({"paths":["a","b","c"]}),
            json!({"paths":["a"],"shell":"bad"}),
        ] {
            assert!(validate_json(&input, &schema, 0).is_err());
        }
    }
    #[test]
    fn numeric_duration_goals_are_enforced_without_confusing_product_quantities() {
        assert_eq!(
            requested_seconds("Video giới thiệu 3 phút, sản phẩm 1 kg").unwrap(),
            Some(180.0)
        );
        assert_eq!(requested_seconds("Video 1phút 20giây").unwrap(), Some(80.0));
        assert_eq!(requested_seconds("Video 1,5 minutes").unwrap(), Some(90.0));
        assert_eq!(
            requested_seconds("Giới thiệu sản phẩm 20 chiếc").unwrap(),
            None
        );
        assert!(requested_seconds("Video 1 giờ").is_err());
    }
    #[test]
    fn rejected_identifiers_and_unknown_outputs_cannot_restart_into_a_write() {
        let r = registry();
        assert!(
            Workflow::new(
                "x".into(),
                Mode::FullAuto,
                vec![step("../outside", "read", vec![])],
                &r
            )
            .is_err()
        );
        let mut w = Workflow::new(
            "x".into(),
            Mode::FullAuto,
            vec![step("a", "write", vec![])],
            &r,
        )
        .unwrap();
        let t = w.claim(&r, &scopes()).unwrap().unwrap();
        w.settle(
            &t,
            Err(Failure {
                message: "timeout".into(),
                unknown: true,
                waiting_input: false,
            }),
        )
        .unwrap();
        assert!(w.patch_input("a", json!({"change":1}), w.revision).is_err());
    }
    #[test]
    fn rejects_cycles_unknown_tools_and_model_supplied_success() {
        let r = registry();
        assert!(
            Workflow::new(
                "x".into(),
                Mode::FullAuto,
                vec![step("a", "read", vec!["b"]), step("b", "read", vec!["a"])],
                &r
            )
            .is_err()
        );
        assert!(
            Workflow::new(
                "x".into(),
                Mode::FullAuto,
                vec![step("a", "shell", vec![])],
                &r
            )
            .is_err()
        );
        let mut s = step("a", "read", vec![]);
        s.status = Status::Done;
        assert!(Workflow::new("x".into(), Mode::FullAuto, vec![s], &r).is_err());
    }
    #[test]
    fn sensitive_action_requires_exact_human_approval_even_full_auto() {
        let r = registry();
        let mut w = Workflow::new(
            "x".into(),
            Mode::FullAuto,
            vec![step("a", "publish", vec![])],
            &r,
        )
        .unwrap();
        assert!(w.claim(&r, &scopes()).unwrap().is_none());
        assert_eq!(w.steps[0].status, Status::WaitingApproval);
        assert!(w.approve("a", "wrong").is_err());
        w.approve("a", &digest(&json!({}))).unwrap();
        assert!(w.claim(&r, &scopes()).unwrap().is_some());
    }
    #[test]
    fn manual_changes_invalidate_downstream_and_stale_receipts() {
        let r = registry();
        let mut w = Workflow::new(
            "x".into(),
            Mode::FullAuto,
            vec![step("a", "read", vec![]), step("b", "write", vec!["a"])],
            &r,
        )
        .unwrap();
        let ticket = w.claim(&r, &scopes()).unwrap().unwrap();
        assert!(w.pause().is_err());
        w.settle(&ticket, Ok(json!({"valid":true}))).unwrap();
        assert!(w.settle(&ticket, Ok(json!({}))).is_err());
        w.pause().unwrap();
        w.patch_input("a", json!({"manual":"new"}), w.revision)
            .unwrap();
        assert_eq!(w.steps[0].status, Status::Pending);
        assert!(w.steps[0].output.is_none());
        assert_eq!(w.steps[1].status, Status::Pending);
    }
    #[test]
    fn restart_marks_inflight_unknown_and_never_replays_write() {
        let root = std::env::temp_dir().join(format!(
            "concat-control-test-{:032x}",
            rand::random::<u128>()
        ));
        let r = registry();
        let id;
        {
            let store = Store::open(&root).unwrap();
            let mut w = Workflow::new(
                "x".into(),
                Mode::FullAuto,
                vec![step("a", "write", vec![])],
                &r,
            )
            .unwrap();
            store.claim(&mut w, &r, &scopes()).unwrap().unwrap();
            id = w.id;
        }
        {
            let store = Store::open(&root).unwrap();
            let mut w = store.load(&id).unwrap();
            assert_eq!(w.steps[0].status, Status::Unknown);
            assert!(w.retry("a", &r).is_err());
            assert!(!w.finished());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn approval_never_grants_a_missing_scope() {
        let r = registry();
        let mut w = Workflow::new(
            "x".into(),
            Mode::FullAuto,
            vec![step("a", "read", vec![])],
            &r,
        )
        .unwrap();
        assert!(w.claim(&r, &BTreeSet::new()).unwrap().is_none());
        assert_eq!(w.steps[0].status, Status::WaitingInput);
        assert!(w.approve("a", &digest(&json!({}))).is_err());
    }
    #[test]
    fn mode_change_keeps_active_receipt_valid_and_dependencies_resolve() {
        let r = registry();
        let mut b = step("b", "write", vec!["a"]);
        b.input = json!({"file":{"$step":"a","pointer":"/path"}});
        let mut w = Workflow::new(
            "x".into(),
            Mode::FullAuto,
            vec![step("a", "read", vec![]), b],
            &r,
        )
        .unwrap();
        let ticket = w.claim(&r, &scopes()).unwrap().unwrap();
        w.set_mode(Mode::Supervised);
        w.settle(&ticket, Ok(json!({"path":"actual.mp4"}))).unwrap();
        assert!(w.claim(&r, &scopes()).unwrap().is_none());
        assert!(w.approve("b", &digest(&w.steps[1].input)).is_err());
        w.approve("b", &digest(&w.review_input("b").unwrap()))
            .unwrap();
        let next = w.claim(&r, &scopes()).unwrap().unwrap();
        assert_eq!(next.input["file"], "actual.mp4");
    }
}
