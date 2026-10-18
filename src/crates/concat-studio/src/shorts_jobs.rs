// SPDX-License-Identifier: AGPL-3.0-or-later
//! Durable local automation metadata, not a renderer or scheduler.
//! Behavior adapted from HotClip's watch/session-checkpoint/automation-history
//! concepts (AGPL-3.0); native implementation keeps explicit bounded retries.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_STORE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 128 * 1024;

/// Safe, fixed-class errors; never contains source contents or credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueError {
    Io,
    InvalidStore,
    Input,
    Capacity,
    Missing,
    Transition,
    Busy,
    AttemptsExhausted,
    SourceChanged,
}
impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "shorts queue: {self:?}")
    }
}
impl std::error::Error for QueueError {}

/// Persisted lifecycle; active stages only advance forwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Stage {
    Queued,
    Analyzing,
    Transcribing,
    DetectingHighlights,
    PreparingEdit,
    Rendering,
    QualityChecking,
    Completed,
    Failed,
    Cancelled,
    NeedsReview,
    Interrupted,
}
impl Stage {
    fn rank(self) -> Option<u8> {
        match self {
            Self::Queued => Some(0),
            Self::Analyzing => Some(1),
            Self::Transcribing => Some(2),
            Self::DetectingHighlights => Some(3),
            Self::PreparingEdit => Some(4),
            Self::Rendering => Some(5),
            Self::QualityChecking => Some(6),
            _ => None,
        }
    }
    /// True only while one worker owns the job.
    pub fn active(self) -> bool {
        self.rank().is_some_and(|rank| rank > 0)
    }
    /// Stable wire label, usable as receipt key and UI status.
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "QUEUED",
            Self::Analyzing => "ANALYZING",
            Self::Transcribing => "TRANSCRIBING",
            Self::DetectingHighlights => "DETECTING_HIGHLIGHTS",
            Self::PreparingEdit => "PREPARING_EDIT",
            Self::Rendering => "RENDERING",
            Self::QualityChecking => "QUALITY_CHECKING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::NeedsReview => "NEEDS_REVIEW",
            Self::Interrupted => "INTERRUPTED",
        }
    }
}

/// File identity includes the complete deterministic configuration digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFingerprint {
    pub path: PathBuf,
    pub size: u64,
    pub modified_ns: u128,
    pub config_digest: String,
}
impl SourceFingerprint {
    /// Inspect a non-symlink local file; no media contents are read.
    pub fn capture(path: &Path, config: &Value) -> Result<Self, QueueError> {
        validate_metadata(config)?;
        if fs::symlink_metadata(path)
            .map_err(|_| QueueError::Io)?
            .file_type()
            .is_symlink()
        {
            return Err(QueueError::Input);
        }
        let path = fs::canonicalize(path).map_err(|_| QueueError::Io)?;
        let info = fs::metadata(&path).map_err(|_| QueueError::Io)?;
        if !info.is_file() || info.len() == 0 {
            return Err(QueueError::Input);
        }
        Ok(Self {
            path,
            size: info.len(),
            modified_ns: info
                .modified()
                .map_err(|_| QueueError::Io)?
                .duration_since(UNIX_EPOCH)
                .map_err(|_| QueueError::Input)?
                .as_nanos(),
            config_digest: digest_config(config)?,
        })
    }
    /// Recheck source identity immediately before work/resume.
    pub fn unchanged(&self) -> Result<bool, QueueError> {
        let Ok(info) = fs::symlink_metadata(&self.path) else {
            return Ok(false);
        };
        if !info.is_file() || info.file_type().is_symlink() {
            return Ok(false);
        }
        let modified = info
            .modified()
            .map_err(|_| QueueError::Io)?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| QueueError::Input)?
            .as_nanos();
        Ok(info.len() == self.size && modified == self.modified_ns)
    }
}

/// Durable metadata only. Credentials must remain in the existing provider store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub source: SourceFingerprint,
    pub config: Value,
    pub stage: Stage,
    pub attempts: u32,
    pub receipts: BTreeMap<String, Value>,
    pub outputs: Vec<PathBuf>,
    pub error_code: Option<String>,
    pub created_ms: u64,
    pub updated_ms: u64,
}
/// Idempotent enqueue receipt.
pub struct EnqueueOutcome {
    pub job: Job,
    pub inserted: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct Store {
    version: u32,
    jobs: Vec<Job>,
}

/// Clones and separate opens of the same path serialize through one process lock.
#[derive(Clone)]
pub struct Queue {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
    capacity: usize,
    max_attempts: u32,
}
type Locks = Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>;
static LOCKS: OnceLock<Locks> = OnceLock::new();

impl Queue {
    /// Open metadata without starting work or changing active jobs.
    pub fn open(path: PathBuf, capacity: usize, max_attempts: u32) -> Result<Self, QueueError> {
        if !(1..=1000).contains(&capacity) || !(1..=10).contains(&max_attempts) {
            return Err(QueueError::Input);
        }
        let parent = path.parent().ok_or(QueueError::Input)?;
        fs::create_dir_all(parent).map_err(|_| QueueError::Io)?;
        let path = fs::canonicalize(parent)
            .map_err(|_| QueueError::Io)?
            .join(path.file_name().ok_or(QueueError::Input)?);
        let mut locks = LOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| QueueError::Io)?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        let lock = locks.get(&path).and_then(Weak::upgrade).unwrap_or_else(|| {
            let lock = Arc::new(Mutex::new(()));
            locks.insert(path.clone(), Arc::downgrade(&lock));
            lock
        });
        drop(locks);
        let queue = Self {
            path,
            lock,
            capacity,
            max_attempts,
        };
        queue.list()?;
        Ok(queue)
    }
    /// Return actual persisted records, oldest first.
    pub fn list(&self) -> Result<Vec<Job>, QueueError> {
        let _guard = self.lock.lock().map_err(|_| QueueError::Io)?;
        let _file_guard = self.filesystem_lock()?;
        Ok(self.load()?.jobs)
    }
    /// Idempotent for every existing lifecycle state, including failed/completed.
    pub fn enqueue(
        &self,
        source: SourceFingerprint,
        config: Value,
    ) -> Result<EnqueueOutcome, QueueError> {
        validate_metadata(&config)?;
        if source.config_digest != digest_config(&config)? || !source.unchanged()? {
            return Err(QueueError::SourceChanged);
        }
        self.mutate(|store| {
            if let Some(job) = store.jobs.iter().find(|job| job.source == source) {
                return Ok(EnqueueOutcome {
                    job: job.clone(),
                    inserted: false,
                });
            }
            if store.jobs.len() >= self.capacity {
                return Err(QueueError::Capacity);
            }
            let now = now_ms();
            let job = Job {
                id: format!("shorts-{:032x}", rand::random::<u128>()),
                source,
                config,
                stage: Stage::Queued,
                attempts: 0,
                receipts: BTreeMap::new(),
                outputs: vec![],
                error_code: None,
                created_ms: now,
                updated_ms: now,
            };
            store.jobs.push(job.clone());
            Ok(EnqueueOutcome {
                job,
                inserted: true,
            })
        })
    }
    /// Persist a one-worker claim before any expensive side effect.
    pub fn claim(&self, id: &str) -> Result<Job, QueueError> {
        self.mutate(|store| {
            if store.jobs.iter().any(|job| job.stage.active()) {
                return Err(QueueError::Busy);
            }
            let job = find(store, id)?;
            if job.stage != Stage::Queued {
                return Err(QueueError::Transition);
            }
            if job.attempts >= self.max_attempts {
                return Err(QueueError::AttemptsExhausted);
            }
            if !job.source.unchanged()? {
                return Err(QueueError::SourceChanged);
            }
            job.attempts += 1;
            job.stage = Stage::Analyzing;
            job.error_code = None;
            job.updated_ms = now_ms();
            Ok(job.clone())
        })
    }
    /// Record successful work before advancing to a later native stage.
    pub fn advance(&self, id: &str, stage: Stage, receipt: Value) -> Result<Job, QueueError> {
        validate_metadata(&receipt)?;
        self.update(id, |job| {
            if !job.stage.active() || !stage.active() || stage.rank() < job.stage.rank() {
                return Err(QueueError::Transition);
            }
            job.receipts.insert(job.stage.label().into(), receipt);
            job.stage = stage;
            Ok(())
        })
    }
    /// Complete only after QA stage and actual nonempty output existence checks.
    pub fn finish(
        &self,
        id: &str,
        outputs: Vec<PathBuf>,
        receipt: Value,
    ) -> Result<Job, QueueError> {
        validate_metadata(&receipt)?;
        if outputs.is_empty() || outputs.len() > 100 {
            return Err(QueueError::Input);
        }
        let outputs = outputs
            .into_iter()
            .map(|path| {
                let info = fs::symlink_metadata(&path).map_err(|_| QueueError::Input)?;
                if !info.is_file() || info.file_type().is_symlink() || info.len() == 0 {
                    return Err(QueueError::Input);
                }
                fs::canonicalize(path).map_err(|_| QueueError::Io)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.update(id, |job| {
            if job.stage != Stage::QualityChecking {
                return Err(QueueError::Transition);
            }
            job.stage = Stage::Completed;
            job.outputs = outputs;
            job.receipts.insert("QUALITY_CHECKING".into(), receipt);
            job.error_code = None;
            Ok(())
        })
    }
    /// Failed processing with a safe machine code, never a provider response body.
    pub fn fail(&self, id: &str, code: &str) -> Result<Job, QueueError> {
        self.terminal(id, Stage::Failed, code)
    }
    /// Keep questionable results visible without declaring completion.
    pub fn needs_review(&self, id: &str, code: &str) -> Result<Job, QueueError> {
        self.terminal(id, Stage::NeedsReview, code)
    }
    fn terminal(&self, id: &str, stage: Stage, code: &str) -> Result<Job, QueueError> {
        if !safe_code(code) {
            return Err(QueueError::Input);
        }
        self.update(id, |job| {
            if !job.stage.active() {
                return Err(QueueError::Transition);
            }
            job.stage = stage;
            job.error_code = Some(code.into());
            Ok(())
        })
    }
    /// Request local cancellation; caller must also signal its owned worker.
    pub fn cancel(&self, id: &str) -> Result<Job, QueueError> {
        self.update(id, |job| {
            if !(job.stage == Stage::Queued
                || job.stage.active()
                || job.stage == Stage::NeedsReview)
            {
                return Err(QueueError::Transition);
            }
            job.stage = Stage::Cancelled;
            job.error_code = None;
            Ok(())
        })
    }
    /// Explicit finite retry. No automatic quota-bearing submission occurs here.
    pub fn retry(&self, id: &str) -> Result<Job, QueueError> {
        self.update(id, |job| {
            if !matches!(
                job.stage,
                Stage::Failed | Stage::Cancelled | Stage::Interrupted | Stage::NeedsReview
            ) {
                return Err(QueueError::Transition);
            }
            if job.attempts >= self.max_attempts {
                return Err(QueueError::AttemptsExhausted);
            }
            if !job.source.unchanged()? {
                return Err(QueueError::SourceChanged);
            }
            job.stage = Stage::Queued;
            job.error_code = None;
            Ok(())
        })
    }
    /// Explicit startup-only recovery; never call while a worker is running.
    pub fn recover_interrupted(&self) -> Result<usize, QueueError> {
        self.mutate(|store| {
            let mut count = 0;
            for job in &mut store.jobs {
                if job.stage.active() || job.stage == Stage::Queued {
                    job.stage = Stage::Interrupted;
                    job.error_code = Some("PROCESS_INTERRUPTED".into());
                    job.updated_ms = now_ms();
                    count += 1;
                }
            }
            Ok(count)
        })
    }
    fn update(
        &self,
        id: &str,
        change: impl FnOnce(&mut Job) -> Result<(), QueueError>,
    ) -> Result<Job, QueueError> {
        self.mutate(|store| {
            let job = find(store, id)?;
            change(job)?;
            job.updated_ms = now_ms();
            Ok(job.clone())
        })
    }
    fn mutate<T>(
        &self,
        change: impl FnOnce(&mut Store) -> Result<T, QueueError>,
    ) -> Result<T, QueueError> {
        let _guard = self.lock.lock().map_err(|_| QueueError::Io)?;
        let _file_guard = self.filesystem_lock()?;
        let mut store = self.load()?;
        let result = change(&mut store)?;
        self.save(&store)?;
        Ok(result)
    }
    // Always acquire the process mutex before the OS lock. Never call this
    // recursively: load/save assume their caller owns both guards. The lock
    // handle is released by the OS on process exit, including a crash.
    fn filesystem_lock(&self) -> Result<fs::File, QueueError> {
        let mut name = self
            .path
            .file_name()
            .ok_or(QueueError::Input)?
            .to_os_string();
        name.push(".lock");
        let path = self.path.with_file_name(name);
        if let Ok(info) = fs::symlink_metadata(&path) {
            if !info.is_file() || info.file_type().is_symlink() {
                return Err(QueueError::Io);
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| QueueError::Io)?;
        file.lock().map_err(|_| QueueError::Io)?;
        Ok(file)
    }
    fn load(&self) -> Result<Store, QueueError> {
        let info = match fs::symlink_metadata(&self.path) {
            Ok(info) => info,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Store {
                    version: 1,
                    jobs: vec![],
                });
            }
            Err(_) => return Err(QueueError::Io),
        };
        if !info.is_file() || info.file_type().is_symlink() || info.len() > MAX_STORE_BYTES {
            return Err(QueueError::InvalidStore);
        }
        let store: Store =
            serde_json::from_slice(&fs::read(&self.path).map_err(|_| QueueError::Io)?)
                .map_err(|_| QueueError::InvalidStore)?;
        if store.version != 1 || store.jobs.len() > 1000 {
            return Err(QueueError::InvalidStore);
        }
        let mut ids = BTreeSet::new();
        for job in &store.jobs {
            if !job.id.starts_with("shorts-")
                || job.id.len() != 39
                || !ids.insert(job.id.clone())
                || job.attempts > 10
                || job.source.config_digest != digest_config(&job.config)?
                || job.outputs.len() > 100
                || job
                    .error_code
                    .as_deref()
                    .is_some_and(|code| !safe_code(code))
                || job.receipts.len() > 12
            {
                return Err(QueueError::InvalidStore);
            }
            validate_metadata(&job.config)?;
            for receipt in job.receipts.values() {
                validate_metadata(receipt)?;
            }
        }
        Ok(store)
    }
    fn save(&self, store: &Store) -> Result<(), QueueError> {
        let bytes = serde_json::to_vec(store).map_err(|_| QueueError::Input)?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            return Err(QueueError::Capacity);
        }
        let temp = self
            .path
            .with_file_name(format!(".shorts-jobs-{:032x}.tmp", rand::random::<u128>()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)
                .map_err(|_| QueueError::Io)?;
            file.write_all(&bytes).map_err(|_| QueueError::Io)?;
            file.sync_all().map_err(|_| QueueError::Io)?;
            drop(file);
            fs::rename(&temp, &self.path).map_err(|_| QueueError::Io)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}
fn find<'a>(store: &'a mut Store, id: &str) -> Result<&'a mut Job, QueueError> {
    store
        .jobs
        .iter_mut()
        .find(|job| job.id == id)
        .ok_or(QueueError::Missing)
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
fn safe_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}
fn validate_metadata(value: &Value) -> Result<(), QueueError> {
    fn walk(value: &Value, depth: usize) -> bool {
        if depth > 32 {
            return false;
        }
        match value {
            Value::Object(map) => map.iter().all(|(key, value)| {
                let key = key.to_ascii_lowercase().replace('-', "_");
                !matches!(
                    key.as_str(),
                    "token"
                        | "access_token"
                        | "refresh_token"
                        | "api_key"
                        | "apikey"
                        | "authorization"
                        | "password"
                        | "cookie"
                        | "secret"
                ) && walk(value, depth + 1)
            }),
            Value::Array(items) => items.iter().all(|v| walk(v, depth + 1)),
            _ => true,
        }
    }
    if serde_json::to_vec(value)
        .map_err(|_| QueueError::Input)?
        .len()
        > MAX_METADATA_BYTES
        || !walk(value, 0)
    {
        Err(QueueError::Input)
    } else {
        Ok(())
    }
}
fn digest_config(value: &Value) -> Result<String, QueueError> {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let sorted: BTreeMap<_, _> =
                    map.iter().map(|(k, v)| (k.clone(), canonical(v))).collect();
                serde_json::to_value(sorted).unwrap_or(Value::Null)
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&canonical(value)).map_err(|_| QueueError::Input)?)
    ))
}

/// Poll-driven stable-file sampler; no timer, process, or source deletion.
pub struct StableFolderSampler {
    rounds: usize,
    capacity: usize,
    tracks: HashMap<PathBuf, (SourceFingerprint, usize)>,
    emitted: Vec<SourceFingerprint>,
}
impl StableFolderSampler {
    /// Capacity bounds both tracked files and dedup memory.
    pub fn new(stable_rounds: usize, capacity: usize) -> Self {
        Self {
            rounds: stable_rounds.clamp(1, 20),
            capacity: capacity.clamp(1, 1000),
            tracks: HashMap::new(),
            emitted: vec![],
        }
    }
    /// Poll one explicit folder; reject symlink folders and excessive file counts.
    pub fn scan(
        &mut self,
        folder: &Path,
        config: &Value,
    ) -> Result<Vec<SourceFingerprint>, QueueError> {
        let info = fs::symlink_metadata(folder).map_err(|_| QueueError::Io)?;
        if !info.is_dir() || info.file_type().is_symlink() {
            return Err(QueueError::Input);
        }
        let mut sources = vec![];
        let mut count = 0;
        for entry in fs::read_dir(folder).map_err(|_| QueueError::Io)? {
            count += 1;
            if count > self.capacity {
                return Err(QueueError::Capacity);
            }
            let entry = entry.map_err(|_| QueueError::Io)?;
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with('.')
                || name.contains(".part")
                || name.contains(".download")
                || name.contains(".tmp")
                || name.contains(".crdownload")
            {
                continue;
            }
            let p = entry.path();
            if !p
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    ["mp4", "mkv", "mov", "flv", "ts", "webm", "m4v", "avi"]
                        .contains(&ext.to_ascii_lowercase().as_str())
                })
            {
                continue;
            }
            if entry.file_type().map_err(|_| QueueError::Io)?.is_symlink() {
                continue;
            }
            if let Ok(source) = SourceFingerprint::capture(&p, config) {
                sources.push(source);
            }
        }
        Ok(self.observe(sources))
    }
    /// Inject file snapshots for deterministic tests or alternate intake adapters.
    pub fn observe(&mut self, sources: Vec<SourceFingerprint>) -> Vec<SourceFingerprint> {
        let present: BTreeSet<_> = sources.iter().map(|s| s.path.clone()).collect();
        self.tracks.retain(|path, _| present.contains(path));
        let mut ready = vec![];
        for source in sources.into_iter().take(self.capacity) {
            if self.emitted.contains(&source) {
                continue;
            }
            if let Some((before, rounds)) = self.tracks.get_mut(&source.path) {
                if before == &source {
                    *rounds += 1;
                    if *rounds >= self.rounds && self.emitted.len() < self.capacity {
                        self.emitted.push(source.clone());
                        ready.push(source);
                    }
                } else {
                    *before = source;
                    *rounds = 0;
                }
            } else if self.tracks.len() < self.capacity {
                self.tracks.insert(source.path.clone(), (source, 0));
            }
        }
        ready
    }
    /// Re-arm a failed enqueue only; queued/running/completed work stays deduplicated.
    pub fn release(&mut self, source: &SourceFingerprint) {
        self.emitted.retain(|item| item != source);
        self.tracks.remove(&source.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "concat-shorts-jobs-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn source(&self) -> PathBuf {
            let p = self.0.join("source.mp4");
            std::fs::write(&p, b"synthetic fixture").unwrap();
            p
        }
        fn queue(&self) -> Queue {
            Queue::open(self.0.join("queue.json"), 10, 2).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn enqueue_deduplicates_across_instances_config_and_completed_receipts() {
        let f = Fixture::new();
        let p = f.source();
        let cfg = json!({"duration":30});
        let q = f.queue();
        let source = SourceFingerprint::capture(&p, &cfg).unwrap();
        let a = q.enqueue(source.clone(), cfg.clone()).unwrap();
        assert!(a.inserted);
        assert!(
            !f.queue()
                .enqueue(source.clone(), cfg.clone())
                .unwrap()
                .inserted
        );
        q.claim(&a.job.id).unwrap();
        q.advance(&a.job.id, Stage::QualityChecking, json!({"qa":"pass"}))
            .unwrap();
        q.finish(&a.job.id, vec![p.clone()], json!({"verified":true}))
            .unwrap();
        assert!(!q.enqueue(source, cfg).unwrap().inserted);
        let different = json!({"duration":60});
        assert!(
            q.enqueue(
                SourceFingerprint::capture(&p, &different).unwrap(),
                different
            )
            .unwrap()
            .inserted
        );
    }
    #[test]
    fn interrupted_recovery_and_explicit_retries_are_finite() {
        let f = Fixture::new();
        let q = f.queue();
        let cfg = json!({});
        let a = q
            .enqueue(SourceFingerprint::capture(&f.source(), &cfg).unwrap(), cfg)
            .unwrap()
            .job;
        q.claim(&a.id).unwrap();
        assert_eq!(q.recover_interrupted().unwrap(), 1);
        assert_eq!(q.list().unwrap()[0].stage, Stage::Interrupted);
        q.retry(&a.id).unwrap();
        q.claim(&a.id).unwrap();
        q.fail(&a.id, "ENCODER_FAILED").unwrap();
        assert!(matches!(q.retry(&a.id), Err(QueueError::AttemptsExhausted)));
    }
    #[test]
    fn capacity_transitions_cancellation_and_safe_errors_are_enforced() {
        let f = Fixture::new();
        let q = Queue::open(f.0.join("queue.json"), 1, 2).unwrap();
        let p = f.source();
        let cfg = json!({});
        let a = q
            .enqueue(SourceFingerprint::capture(&p, &cfg).unwrap(), cfg)
            .unwrap()
            .job;
        assert!(q.finish(&a.id, vec![p.clone()], json!({})).is_err());
        let cfg = json!({"new":true});
        assert!(matches!(
            q.enqueue(SourceFingerprint::capture(&p, &cfg).unwrap(), cfg),
            Err(QueueError::Capacity)
        ));
        q.cancel(&a.id).unwrap();
        assert!(q.claim(&a.id).is_err());
        q.retry(&a.id).unwrap();
        q.claim(&a.id).unwrap();
        assert!(q.fail(&a.id, "https://secret.invalid/token").is_err());
        q.advance(&a.id, Stage::Rendering, json!({})).unwrap();
        assert!(q.advance(&a.id, Stage::Analyzing, json!({})).is_err());
        q.needs_review(&a.id, "QA_WARNING").unwrap();
        assert_eq!(
            q.list().unwrap()[0].error_code.as_deref(),
            Some("QA_WARNING")
        );
    }
    #[test]
    fn malformed_store_is_not_silently_replaced_and_bad_output_never_completes() {
        let f = Fixture::new();
        std::fs::write(f.0.join("queue.json"), b"{invalid").unwrap();
        assert!(Queue::open(f.0.join("queue.json"), 10, 2).is_err());
        assert_eq!(std::fs::read(f.0.join("queue.json")).unwrap(), b"{invalid");
        let q = Queue::open(f.0.join("good.json"), 10, 2).unwrap();
        let cfg = json!({});
        let a = q
            .enqueue(SourceFingerprint::capture(&f.source(), &cfg).unwrap(), cfg)
            .unwrap()
            .job;
        q.claim(&a.id).unwrap();
        q.advance(&a.id, Stage::QualityChecking, json!({})).unwrap();
        assert!(
            q.finish(&a.id, vec![f.0.join("missing.mp4")], json!({}))
                .is_err()
        );
        assert_eq!(q.list().unwrap()[0].stage, Stage::QualityChecking);
    }
    #[test]
    fn stable_sampler_requires_unchanged_rounds_and_never_reemits_inflight_fingerprint() {
        let f = Fixture::new();
        let cfg = json!({});
        let p = f.source();
        let source = SourceFingerprint::capture(&p, &cfg).unwrap();
        let mut sampler = StableFolderSampler::new(2, 8);
        assert!(sampler.observe(vec![source.clone()]).is_empty());
        assert!(sampler.observe(vec![source.clone()]).is_empty());
        assert_eq!(sampler.observe(vec![source.clone()]), vec![source.clone()]);
        for _ in 0..10 {
            assert!(sampler.observe(vec![source.clone()]).is_empty());
        }
        sampler.release(&source);
        assert!(sampler.observe(vec![source]).is_empty());
    }
    #[test]
    fn concurrent_opens_cannot_duplicate_enqueue_or_claim() {
        let f = Fixture::new();
        let cfg = json!({});
        let source = SourceFingerprint::capture(&f.source(), &cfg).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(5));
        let handles = (0..4)
            .map(|_| {
                let q = f.queue();
                let source = source.clone();
                let cfg = cfg.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    q.enqueue(source, cfg).unwrap().inserted
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|inserted| *inserted)
                .count(),
            1
        );
        let q = f.queue();
        let id = q.list().unwrap()[0].id.clone();
        let other = q.clone();
        q.claim(&id).unwrap();
        assert!(matches!(other.claim(&id), Err(QueueError::Busy)));
        assert_eq!(q.list().unwrap().len(), 1);
        assert!(fs::read_dir(&f.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".shorts-jobs-")
        }));
    }
    #[test]
    fn changed_sources_and_credential_metadata_are_rejected() {
        let f = Fixture::new();
        let q = f.queue();
        let p = f.source();
        let cfg = json!({});
        let a = q
            .enqueue(SourceFingerprint::capture(&p, &cfg).unwrap(), cfg)
            .unwrap()
            .job;
        fs::write(&p, b"different length synthetic media").unwrap();
        assert!(!a.source.unchanged().unwrap());
        assert!(matches!(q.claim(&a.id), Err(QueueError::SourceChanged)));
        assert!(SourceFingerprint::capture(&p, &json!({"nested":{"api_key":"fixture"}})).is_err());
        assert_eq!(q.list().unwrap()[0].stage, Stage::Queued);
    }
    #[test]
    fn scan_ignores_unfinished_files_and_bounds_folder_enumeration() {
        let f = Fixture::new();
        let cfg = json!({});
        f.source();
        fs::write(f.0.join(".hidden.mp4"), b"fixture").unwrap();
        fs::write(f.0.join("writing.part.mp4"), b"fixture").unwrap();
        let mut sampler = StableFolderSampler::new(1, 8);
        assert!(sampler.scan(&f.0, &cfg).unwrap().is_empty());
        let ready = sampler.scan(&f.0, &cfg).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].path.file_name().unwrap(), "source.mp4");
        let mut bounded = StableFolderSampler::new(1, 1);
        assert!(matches!(
            bounded.scan(&f.0, &cfg),
            Err(QueueError::Capacity)
        ));
    }
    #[test]
    #[ignore = "child helper explicitly invoked by cross-process queue test"]
    fn child_queue_process() {
        use std::io::Read;
        let Some(path) = std::env::var_os("CONCAT_TEST_SHORTS_QUEUE") else {
            return;
        };
        let path = PathBuf::from(path);
        let source = PathBuf::from(
            std::env::var_os("CONCAT_TEST_SHORTS_SOURCE").expect("owned fixture source"),
        );
        let mut start = [0u8; 1];
        std::io::stdin().read_exact(&mut start).unwrap();
        let q = Queue::open(path, 20, 2).unwrap();
        let cfg = serde_json::json!({});
        for _ in 0..20 {
            q.enqueue(
                SourceFingerprint::capture(&source, &cfg).unwrap(),
                cfg.clone(),
            )
            .unwrap();
        }
    }
    #[test]
    fn file_lock_serializes_independent_process_writers() {
        use std::process::{Command, Stdio};
        let f = Fixture::new();
        let source = f.source();
        let mut children = (0..4)
            .map(|_| {
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--ignored",
                        "--exact",
                        "shorts_jobs::tests::child_queue_process",
                    ])
                    .env("CONCAT_TEST_SHORTS_QUEUE", f.0.join("queue.json"))
                    .env("CONCAT_TEST_SHORTS_SOURCE", &source)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for child in &mut children {
            child.stdin.take().unwrap().write_all(b"x").unwrap();
        }
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        assert_eq!(f.queue().list().unwrap().len(), 1);
    }
}
