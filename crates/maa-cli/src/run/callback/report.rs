//! Structured JSON run report built from MaaCore callbacks.
//!
//! The schema is intentionally open: unknown extra-info `what` values and leftover
//! callback fields are stored under each task's [`TaskReport::details`] object so
//! future core callbacks (Status, annihilation reason codes, ...) land without a
//! schema change.

use std::{collections::BTreeMap, io::Write, path::Path, sync::Mutex};

use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use maa_types::{MessageKind, TaskType, primitive::AsstTaskId};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::atomic_fs;

static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);
static LAST_GAME_STATUS: Mutex<Option<Value>> = Mutex::new(None);

fn with_recorder_mut<T>(f: impl FnOnce(&mut Recorder) -> T) -> Option<T> {
    RECORDER.lock().unwrap().as_mut().map(f)
}

pub(crate) fn init(recorder: Recorder) {
    *RECORDER.lock().unwrap() = Some(recorder);
    *LAST_GAME_STATUS.lock().unwrap() = None;
}

pub(crate) fn last_game_status() -> Option<Value> {
    LAST_GAME_STATUS.lock().unwrap().clone()
}

pub(crate) fn ingest(kind: MessageKind, message: &Map<String, Value>) {
    with_recorder_mut(|recorder| recorder.ingest(kind, message));
}

pub(crate) fn mark_interrupted() {
    with_recorder_mut(|recorder| recorder.interrupted = true);
}

pub(crate) fn mark_startup_error(message: impl Into<String>) {
    let message = message.into();
    with_recorder_mut(|recorder| recorder.mark_startup_error(message));
}

pub(crate) fn take() -> Option<Recorder> {
    RECORDER.lock().unwrap().take()
}

pub(crate) fn with_core_version(version: impl Into<String>) {
    let version = version.into();
    with_recorder_mut(|recorder| recorder.set_core_version(version));
}

pub(crate) fn add_task(
    index: usize,
    id: AsstTaskId,
    name: impl Into<String>,
    task_type: TaskType,
    enabled: bool,
    configured_stage: Option<String>,
    configured_times: Option<i64>,
) {
    let name = name.into();
    with_recorder_mut(|recorder| {
        recorder.add_task(
            index,
            id,
            name,
            task_type,
            enabled,
            configured_stage,
            configured_times,
        )
    });
}

pub(crate) fn add_unavailable(
    index: usize,
    name: impl Into<String>,
    task_type: TaskType,
    message: impl Into<String>,
) {
    let name = name.into();
    let message = message.into();
    with_recorder_mut(|recorder| recorder.add_unavailable(index, name, task_type, message));
}

pub(crate) fn add_skipped(index: usize, name: impl Into<String>, task_type: TaskType) {
    let name = name.into();
    with_recorder_mut(|recorder| recorder.add_skipped(index, name, task_type));
}

/// Distinct process exit codes used with `--strict-exit`.
///
/// Without `--strict-exit`, any failure still maps to `1` (backward compatible).
pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_GENERIC_FAILURE: i32 = 1;
pub const EXIT_PARTIAL_FAILURE: i32 = 2;
pub const EXIT_STARTUP_FAILURE: i32 = 3;
pub const EXIT_INTERRUPTED: i32 = 130;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Succeeded,
    PartialFailure,
    StartupFailure,
    Interrupted,
    DryRun,
}

impl RunStatus {
    pub fn exit_code(self, strict: bool) -> i32 {
        match self {
            Self::Succeeded | Self::DryRun => EXIT_SUCCESS,
            Self::PartialFailure => {
                if strict {
                    EXIT_PARTIAL_FAILURE
                } else {
                    EXIT_GENERIC_FAILURE
                }
            }
            Self::StartupFailure => {
                if strict {
                    EXIT_STARTUP_FAILURE
                } else {
                    EXIT_GENERIC_FAILURE
                }
            }
            Self::Interrupted => {
                if strict {
                    EXIT_INTERRUPTED
                } else {
                    EXIT_GENERIC_FAILURE
                }
            }
        }
    }

    fn as_outcome(self) -> Outcome {
        match self {
            Self::Succeeded => Outcome::Succeeded,
            Self::PartialFailure => Outcome::Partial,
            Self::StartupFailure => Outcome::Failed,
            Self::Interrupted => Outcome::Interrupted,
            Self::DryRun => Outcome::DryRun,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Partial,
    Failed,
    Interrupted,
    DryRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Succeeded,
    Failed,
    Skipped,
    Stopped,
    NotRun,
}

#[derive(Clone, Debug, Serialize)]
pub struct FailReason {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Versions {
    pub cli: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunReport {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub outcome: Outcome,
    pub versions: Versions,
    pub connection: ConnectionReport,
    pub tasks: Vec<TaskReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_status: Option<Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConnectionReport {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub what: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TaskReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<AsstTaskId>,
    pub name: String,
    #[serde(rename = "type")]
    pub task_type: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_reason: Option<FailReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<FailReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fight: Option<FightReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annihilation: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recruit: Option<RecruitReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub infrast: Option<InfrastReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub award: Option<AwardReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_status: Option<Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct FightReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_id: Option<String>,
    pub times: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sanity_used: Option<i64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub drops: BTreeMap<String, i64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub drops_named: BTreeMap<String, i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medicine: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiring_medicine: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stone: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sanity_start: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sanity: Option<Value>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RecruitReport {
    pub started: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed: Option<i64>,
    pub tags: Vec<RecruitTags>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecruitTags {
    pub level: u64,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct InfrastReport {
    pub rooms: Vec<InfrastRoom>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InfrastRoom {
    pub facility: String,
    pub index: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub operators: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct AwardReport {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checked: Vec<String>,
    pub claimed: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChainReason {
    Unstarted,
    Unfinished,
    Completed,
    Stopped,
    Error,
}

struct TaskRecord {
    id: Option<AsstTaskId>,
    index: usize,
    name: String,
    task_type: String,
    enabled: bool,
    variant_skipped: bool,
    skip_code: Option<String>,
    skip_message: Option<String>,
    chain: ChainReason,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    fail_reason: Option<FailReason>,
    last_error: Option<FailReason>,
    configured_stage: Option<String>,
    fight: FightReport,
    annihilation: BTreeMap<String, Value>,
    recruit: RecruitReport,
    infrast: InfrastReport,
    award: AwardReport,
    game_status: Option<Value>,
    details: BTreeMap<String, Value>,
    last_series: (i64, i64),
}

impl TaskRecord {
    fn new(index: usize, name: String, task_type: String, enabled: bool) -> Self {
        Self {
            id: None,
            index,
            name,
            task_type,
            enabled,
            variant_skipped: false,
            skip_code: None,
            skip_message: None,
            chain: ChainReason::Unstarted,
            started_at: None,
            finished_at: None,
            fail_reason: None,
            last_error: None,
            configured_stage: None,
            fight: FightReport::default(),
            annihilation: BTreeMap::new(),
            recruit: RecruitReport::default(),
            infrast: InfrastReport::default(),
            award: AwardReport::default(),
            game_status: None,
            details: BTreeMap::new(),
            last_series: (0, 0),
        }
    }
}

pub(crate) struct Recorder {
    started_at: DateTime<Utc>,
    versions: Versions,
    dry_run: bool,
    interrupted: bool,
    connection_what: Option<String>,
    connection_why: Option<String>,
    connection_ok: bool,
    connection_failed: bool,
    init_failed: bool,
    details: BTreeMap<String, Value>,
    game_status: Option<Value>,
    tasks: Vec<TaskRecord>,
    current: Option<AsstTaskId>,
    next_synthetic_id: AsstTaskId,
}

impl Recorder {
    pub(crate) fn new(versions: Versions, dry_run: bool) -> Self {
        Self {
            started_at: Utc::now(),
            versions,
            dry_run,
            interrupted: false,
            connection_what: None,
            connection_why: None,
            connection_ok: false,
            connection_failed: false,
            init_failed: false,
            details: BTreeMap::new(),
            game_status: None,
            tasks: Vec::new(),
            current: None,
            next_synthetic_id: -1,
        }
    }

    pub(crate) fn set_core_version(&mut self, version: impl Into<String>) {
        self.versions.core = Some(version.into());
    }

    pub(crate) fn mark_startup_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.init_failed = true;
        self.details
            .insert("startup_error".to_owned(), Value::String(message));
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors the append_task fields recorded for reports"
    )]
    pub(crate) fn add_task(
        &mut self,
        index: usize,
        id: AsstTaskId,
        name: impl Into<String>,
        task_type: TaskType,
        enabled: bool,
        configured_stage: Option<String>,
        configured_times: Option<i64>,
    ) {
        let mut record =
            TaskRecord::new(index, name.into(), task_type.to_str().to_owned(), enabled);
        record.id = Some(id);
        if let Some(stage) = configured_stage {
            if task_type == TaskType::Fight {
                record.fight.stage = Some(stage.clone());
            }
            record.configured_stage = Some(stage);
        }
        if self.dry_run
            && let Some(times) = configured_times
        {
            record.fight.times = times;
        }
        self.tasks.push(record);
    }

    pub(crate) fn add_unavailable(
        &mut self,
        index: usize,
        name: impl Into<String>,
        task_type: TaskType,
        message: impl Into<String>,
    ) {
        let mut record = TaskRecord::new(index, name.into(), task_type.to_str().to_owned(), true);
        record.skip_code = Some("UNSUPPORTED_TASK_TYPE".to_owned());
        record.skip_message = Some(message.into());
        record.chain = ChainReason::Completed;
        self.tasks.push(record);
    }

    pub(crate) fn add_skipped(
        &mut self,
        index: usize,
        name: impl Into<String>,
        task_type: TaskType,
    ) {
        let mut record = TaskRecord::new(index, name.into(), task_type.to_str().to_owned(), true);
        record.variant_skipped = true;
        record.skip_code = Some("VARIANT_INACTIVE".to_owned());
        record.skip_message = Some("No task variant condition matched".to_owned());
        self.tasks.push(record);
    }

    fn find_task_index(&self, id: Option<AsstTaskId>, taskchain: Option<&str>) -> Option<usize> {
        if let Some(id) = id
            && let Some(index) = self.tasks.iter().position(|task| task.id == Some(id))
        {
            return Some(index);
        }
        if let Some(chain) = taskchain
            && let Some(index) = self
                .tasks
                .iter()
                .rposition(|task| task.task_type == chain || task.name == chain)
        {
            return Some(index);
        }
        self.current
            .and_then(|cid| self.tasks.iter().position(|task| task.id == Some(cid)))
    }

    fn ensure_task(&mut self, id: AsstTaskId, taskchain: Option<&str>) -> &mut TaskRecord {
        if let Some(index) = self.find_task_index(Some(id), taskchain) {
            return &mut self.tasks[index];
        }
        let name = taskchain.unwrap_or("Unknown").to_owned();
        let mut record = TaskRecord::new(self.tasks.len(), name.clone(), name, true);
        record.id = Some(id);
        self.tasks.push(record);
        self.tasks
            .last_mut()
            .unwrap_or_else(|| unreachable!("just pushed"))
    }

    fn ensure_current(&mut self, message: &Map<String, Value>) -> Option<&mut TaskRecord> {
        let id = as_task_id(message);
        let taskchain = message.get("taskchain").and_then(Value::as_str);
        let index = self.find_task_index(id, taskchain)?;
        Some(&mut self.tasks[index])
    }

    pub(crate) fn ingest(&mut self, kind: MessageKind, message: &Map<String, Value>) {
        use MessageKind::*;
        match kind {
            InitFailed => {
                self.init_failed = true;
                let what = message
                    .get("what")
                    .and_then(Value::as_str)
                    .unwrap_or("InitFailed");
                let why = message.get("why").and_then(Value::as_str).unwrap_or("");
                self.details
                    .insert("init_failed".to_owned(), Value::Object(message.clone()));
                self.connection_what = Some(what.to_owned());
                if !why.is_empty() {
                    self.connection_why = Some(why.to_owned());
                }
            }
            ConnectionInfo => self.ingest_connection(message),
            TaskChainStart | TaskChainCompleted | TaskChainError | TaskChainStopped
            | TaskChainExtraInfo => self.ingest_taskchain(kind, message),
            SubTaskError => self.ingest_subtask_error(message),
            SubTaskStart | SubTaskCompleted => self.ingest_process_task(kind, message),
            SubTaskExtraInfo => self.ingest_extra(message),
            Unknown(_) => {
                self.details
                    .entry("unknown_messages".to_owned())
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Some(Value::Array(arr)) = self.details.get_mut("unknown_messages") {
                    arr.push(Value::Object(message.clone()));
                }
            }
            _ => {}
        }
    }

    fn ingest_connection(&mut self, message: &Map<String, Value>) {
        let Some(what) = message.get("what").and_then(Value::as_str) else {
            return;
        };
        let why = message
            .get("why")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        match what {
            "Connected" | "Reconnected" => {
                self.connection_ok = true;
                // Keep the first Connected/failure event; later FPS/uuid noise must not replace it.
                if !matches!(
                    self.connection_what.as_deref(),
                    Some("ConnectFailed" | "Disconnect")
                ) {
                    self.connection_what = Some(what.to_owned());
                    if why.is_some() {
                        self.connection_why = why;
                    }
                }
            }
            "UuidGot" => {
                self.connection_ok = true;
                if self.connection_what.is_none() {
                    self.connection_what = Some(what.to_owned());
                    if why.is_some() {
                        self.connection_why = why;
                    }
                }
            }
            "ConnectFailed"
            | "Disconnect"
            | "UnsupportedResolution"
            | "ResolutionError"
            | "TouchModeNotAvailable" => {
                self.connection_failed = true;
                self.connection_what = Some(what.to_owned());
                self.connection_why = why;
            }
            _ => {}
        }
    }

    fn ingest_taskchain(&mut self, kind: MessageKind, message: &Map<String, Value>) {
        let taskchain = message.get("taskchain").and_then(Value::as_str);
        let id = as_task_id(message);
        // Late taskid-0 TaskChainStart must attach by name only when that task never started.
        if kind == MessageKind::TaskChainStart && id.is_none() {
            let existing = self.find_task_index(None, taskchain);
            let reopen = existing
                .is_some_and(|index| !matches!(self.tasks[index].chain, ChainReason::Unstarted));
            if existing.is_none() || reopen {
                merge_value(
                    &mut self.details,
                    "orphaned_callbacks",
                    Value::Object(message.clone()),
                );
                return;
            }
        }
        let task = match (kind, id) {
            (MessageKind::TaskChainStart, Some(id)) => self.ensure_task(id, taskchain),
            _ => {
                if let Some(index) = self.find_task_index(id, taskchain) {
                    &mut self.tasks[index]
                } else if kind == MessageKind::TaskChainStart && id.is_some() {
                    let synth = {
                        let id = self.next_synthetic_id;
                        self.next_synthetic_id -= 1;
                        id
                    };
                    self.ensure_task(synth, taskchain)
                } else {
                    merge_value(
                        &mut self.details,
                        "orphaned_callbacks",
                        Value::Object(message.clone()),
                    );
                    return;
                }
            }
        };
        match kind {
            MessageKind::TaskChainStart => {
                task.started_at = Some(Utc::now());
                task.chain = ChainReason::Unfinished;
                self.current = task.id;
            }
            MessageKind::TaskChainCompleted => {
                task.finished_at = Some(Utc::now());
                task.chain = ChainReason::Completed;
                self.current = None;
            }
            MessageKind::TaskChainStopped => {
                task.finished_at = Some(Utc::now());
                task.chain = ChainReason::Stopped;
                self.current = None;
            }
            MessageKind::TaskChainError => {
                task.finished_at = Some(Utc::now());
                task.chain = ChainReason::Error;
                if let Some(details) = message.get("details") {
                    if let Some(code) = details.get("error").and_then(Value::as_str) {
                        task.fail_reason = Some(FailReason {
                            code: normalize_code(code),
                            message: code.to_owned(),
                        });
                    }
                    merge_value(&mut task.details, "taskchain_error", details.clone());
                }
                if task.fail_reason.is_none() {
                    task.fail_reason = task.last_error.clone().or_else(|| {
                        let chain = taskchain.unwrap_or("task");
                        Some(FailReason {
                            code: "TASK_CHAIN_ERROR".to_owned(),
                            message: format!("{chain} error"),
                        })
                    });
                }
                self.current = None;
            }
            MessageKind::TaskChainExtraInfo => {
                if let Some(details) = message.get("details") {
                    merge_value(&mut task.details, "taskchain_extra", details.clone());
                    capture_reason_fields(task, details);
                }
                if let Some(what) = message.get("what").and_then(Value::as_str) {
                    capture_named_reason(task, what, message.get("details"));
                }
            }
            _ => {}
        }
    }

    fn ingest_subtask_error(&mut self, message: &Map<String, Value>) {
        if is_benign_subtask_error(message) {
            return;
        }
        let subtask = message.get("subtask").and_then(Value::as_str).unwrap_or("");
        let why = message.get("why").and_then(Value::as_str);
        let (code, default_msg) = match subtask {
            "StartGameTask" => ("FAILED_TO_START_GAME", "Failed to start game"),
            "RecognizeDrops" => ("RECOGNIZE_DROPS_FAILED", "Failed to recognize drops"),
            "ReportToPenguinStats" => (
                "REPORT_TO_PENGUIN_FAILED",
                "Failed to report to Penguin Stats",
            ),
            "CheckStageValid" => ("STAGE_NOT_FOUND", "Stage is not valid"),
            _ => ("SUBTASK_ERROR", "Subtask error"),
        };
        let message_text = why
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| default_msg.to_owned());
        let reason = FailReason {
            code: why
                .filter(|value| looks_like_code(value))
                .map(normalize_code)
                .unwrap_or_else(|| code.to_owned()),
            message: message_text,
        };
        if let Some(task) = self.ensure_current(message) {
            task.last_error = Some(reason);
            merge_value(
                &mut task.details,
                "last_subtask_error",
                Value::Object(message.clone()),
            );
        } else {
            merge_value(
                &mut self.details,
                "orphaned_callbacks",
                Value::Object(message.clone()),
            );
        }
    }

    fn ingest_process_task(&mut self, kind: MessageKind, message: &Map<String, Value>) {
        let Some(details) = message.get("details").and_then(Value::as_object) else {
            return;
        };
        let Some(task_name) = details.get("task").and_then(Value::as_str) else {
            return;
        };
        let Some(task) = self.ensure_current(message) else {
            return;
        };

        match task_name {
            "StartButton2" | "AnnihilationConfirm" if kind == MessageKind::SubTaskStart => {
                let (series, sanity) = task.last_series;
                if series > 0 {
                    task.fight.times += series;
                } else {
                    task.fight.times += 1;
                }
                if sanity > 0 {
                    task.fight.sanity_used = Some(task.fight.sanity_used.unwrap_or(0) + sanity);
                }
            }
            "StoneConfirm" => {
                if let Some(exec) = details.get("exec_times").and_then(Value::as_i64) {
                    task.fight.stone = Some(exec);
                }
            }
            "RecruitConfirm" if kind == MessageKind::SubTaskStart => {
                task.recruit.started += 1;
                if let Some(last) = task.recruit.tags.last_mut() {
                    last.state = Some("recruited".to_owned());
                }
            }
            "RecruitRefreshConfirm" if kind == MessageKind::SubTaskStart => {
                task.recruit.refreshed = Some(task.recruit.refreshed.unwrap_or(0) + 1);
                if let Some(last) = task.recruit.tags.last_mut() {
                    last.state = Some("refreshed".to_owned());
                }
            }
            _ => {}
        }

        if (task.task_type == "Award"
            || message.get("taskchain").and_then(Value::as_str) == Some("Award"))
            && kind == MessageKind::SubTaskStart
        {
            if let Some(claim) = award_claimed_from_task(task_name)
                && !task.award.claimed.iter().any(|item| item == claim)
            {
                task.award.claimed.push(claim.to_owned());
            }
            if let Some(checked) = award_checked_from_task(task_name)
                && !task.award.checked.iter().any(|item| item == checked)
            {
                task.award.checked.push(checked.to_owned());
            }
        }

        if looks_like_code(task_name) {
            capture_named_reason(task, task_name, Some(&Value::Object(details.clone())));
        }
    }

    fn ingest_extra(&mut self, message: &Map<String, Value>) {
        let Some(what) = message.get("what").and_then(Value::as_str) else {
            return;
        };
        let details = message.get("details").cloned().unwrap_or(Value::Null);
        let Some(task) = self.ensure_current(message) else {
            merge_value(
                &mut self.details,
                "orphaned_callbacks",
                Value::Object(message.clone()),
            );
            return;
        };

        capture_named_reason(task, what, Some(&details));
        match what {
            "GameStatus" => {
                let snapshot = normalize_timestamps(details.clone());
                task.game_status = Some(snapshot.clone());
                self.game_status = Some(snapshot);
            }
            "TaskResult" => {
                ingest_task_result(task, &details);
                merge_value(&mut task.details, "TaskResult", details.clone());
            }
            "FightTimes" => {
                task.details
                    .insert("FightTimes".to_owned(), details.clone());
                if let Some(obj) = details.as_object() {
                    let series = obj.get("series").and_then(Value::as_i64).unwrap_or(0);
                    let sanity = obj.get("sanity_cost").and_then(Value::as_i64).unwrap_or(0);
                    task.last_series = (series, sanity);
                    if let Some(times) = obj
                        .get("times_finished")
                        .or_else(|| obj.get("times"))
                        .and_then(Value::as_i64)
                    {
                        merge_value(&mut task.details, "fight_times_total", Value::from(times));
                    }
                }
            }
            "StageDrops" => {
                if let Some(obj) = details.as_object() {
                    ingest_stage_drops(task, obj);
                }
            }
            "SanityBeforeStage" => {
                let snapshot = normalize_timestamps(details.clone());
                if task.fight.sanity_start.is_none() {
                    task.fight.sanity_start = Some(snapshot.clone());
                }
                task.fight.sanity = Some(snapshot);
            }
            "UseMedicine" => {
                if let Some(obj) = details.as_object() {
                    let count = obj.get("count").and_then(Value::as_i64).unwrap_or(0);
                    let expiring = obj
                        .get("is_expiring")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    task.fight.medicine = Some(task.fight.medicine.unwrap_or(0) + count);
                    if expiring {
                        task.fight.expiring_medicine =
                            Some(task.fight.expiring_medicine.unwrap_or(0) + count);
                    }
                }
            }
            "EnterFacility" | "ProductOfFacility" | "CustomInfrastRoomOperators" => {
                ingest_infrast_room(&mut task.infrast, what, &details);
            }
            "RecruitResult" => {
                if let Some(obj) = details.as_object() {
                    let level = obj.get("level").and_then(Value::as_u64).unwrap_or(0);
                    let tags = obj
                        .get("tags")
                        .and_then(Value::as_array)
                        .map(|arr| {
                            arr.iter()
                                .filter_map(Value::as_str)
                                .map(ToOwned::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    task.recruit.tags.push(RecruitTags {
                        level,
                        tags,
                        state: None,
                    });
                }
            }
            "RecruitNoPermit" => {
                task.skip_code
                    .get_or_insert_with(|| "NO_RECRUIT_PERMIT".to_owned());
                task.skip_message
                    .get_or_insert_with(|| "No recruitment permit".to_owned());
                merge_value(&mut task.details, what, details);
            }
            _ => {
                if is_annihilation_what(what) {
                    if let Some(obj) = details.as_object() {
                        for (key, value) in obj {
                            task.annihilation.insert(key.clone(), value.clone());
                        }
                    } else if !details.is_null() {
                        task.annihilation.insert(what.to_owned(), details.clone());
                    }
                }
                merge_value(&mut task.details, what, details);
            }
        }
    }

    pub(crate) fn finish(mut self, finished_at: DateTime<Utc>) -> (RunReport, RunStatus) {
        self.tasks.sort_by_key(|task| task.index);
        for task in &mut self.tasks {
            finalize_annihilation(task);
        }
        let tasks: Vec<TaskReport> = self
            .tasks
            .iter()
            .map(|task| task_to_report(task, self.dry_run, self.interrupted))
            .collect();

        let status = classify(
            self.dry_run,
            self.interrupted,
            self.init_failed,
            self.connection_failed && !self.connection_ok,
            &tasks,
        );

        let connection_status = if self.dry_run {
            "not_attempted"
        } else if self.connection_ok {
            "connected"
        } else if self.connection_failed || self.init_failed {
            "failed"
        } else if self.interrupted {
            "interrupted"
        } else {
            "unknown"
        };

        let game_status = self.game_status.clone();
        *LAST_GAME_STATUS.lock().unwrap() = game_status.clone();

        let report = RunReport {
            started_at: self.started_at,
            finished_at,
            outcome: status.as_outcome(),
            versions: self.versions,
            connection: ConnectionReport {
                status: connection_status.to_owned(),
                what: self.connection_what,
                why: self.connection_why,
            },
            tasks,
            game_status,
            details: self.details,
        };
        (report, status)
    }
}

fn task_to_report(task: &TaskRecord, dry_run: bool, interrupted: bool) -> TaskReport {
    let duration_s = match (task.started_at, task.finished_at) {
        (Some(start), Some(end)) => Some((end - start).num_milliseconds() as f64 / 1000.0),
        _ => None,
    };

    let (status, fail_reason, skip_reason) = classify_task(task, dry_run, interrupted);

    let fight = if task.task_type == "Fight" || has_fight_data(&task.fight) {
        Some(task.fight.clone())
    } else {
        None
    };

    let mut annihilation = if task.annihilation.is_empty() {
        None
    } else {
        Some(Value::Object(
            task.annihilation
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ))
    };
    if annihilation.is_none()
        && fight
            .as_ref()
            .and_then(|f| f.stage.as_deref())
            .is_some_and(|stage| stage.contains("Annihilation"))
    {
        let mut obj = Map::new();
        if let Some(ref fight) = fight {
            obj.insert("times".to_owned(), Value::from(fight.times));
            if let Some(stage) = fight.stage.clone() {
                obj.insert("stage".to_owned(), Value::String(stage));
            }
        }
        annihilation = Some(Value::Object(obj));
    }

    let recruit =
        if task.task_type == "Recruit" || task.recruit.started > 0 || !task.recruit.tags.is_empty()
        {
            Some(task.recruit.clone())
        } else {
            None
        };

    let infrast = if task.task_type == "Infrast" || !task.infrast.rooms.is_empty() {
        Some(task.infrast.clone())
    } else {
        None
    };

    let award = if task.task_type == "Award"
        || !task.award.claimed.is_empty()
        || !task.award.checked.is_empty()
    {
        Some(task.award.clone())
    } else {
        None
    };

    TaskReport {
        id: task.id.filter(|id| *id > 0),
        name: task.name.clone(),
        task_type: task.task_type.clone(),
        status,
        started_at: task.started_at,
        finished_at: task.finished_at,
        duration_s,
        fail_reason,
        skip_reason,
        fight,
        annihilation,
        recruit,
        infrast,
        award,
        game_status: task.game_status.clone(),
        details: task.details.clone(),
    }
}

fn classify_task(
    task: &TaskRecord,
    dry_run: bool,
    interrupted: bool,
) -> (TaskStatus, Option<FailReason>, Option<FailReason>) {
    if task.variant_skipped {
        return (
            TaskStatus::Skipped,
            None,
            Some(FailReason {
                code: task
                    .skip_code
                    .clone()
                    .unwrap_or_else(|| "VARIANT_INACTIVE".to_owned()),
                message: task
                    .skip_message
                    .clone()
                    .unwrap_or_else(|| "No task variant condition matched".to_owned()),
            }),
        );
    }
    if !task.enabled {
        return (
            TaskStatus::Skipped,
            None,
            Some(FailReason {
                code: "DISABLED".to_owned(),
                message: "Task parameter enable is false".to_owned(),
            }),
        );
    }
    if dry_run && task.chain == ChainReason::Unstarted {
        return (TaskStatus::NotRun, None, None);
    }

    match task.chain {
        ChainReason::Completed => {
            if let Some(code) = task.skip_code.as_deref() {
                return (
                    TaskStatus::Skipped,
                    None,
                    Some(FailReason {
                        code: code.to_owned(),
                        message: task.skip_message.clone().unwrap_or_else(|| code.to_owned()),
                    }),
                );
            }
            (TaskStatus::Succeeded, None, None)
        }
        ChainReason::Error => (
            TaskStatus::Failed,
            task.fail_reason.clone().or_else(|| {
                Some(FailReason {
                    code: "TASK_CHAIN_ERROR".to_owned(),
                    message: "Task chain error".to_owned(),
                })
            }),
            None,
        ),
        ChainReason::Stopped => (
            TaskStatus::Stopped,
            Some(FailReason {
                code: "STOPPED".to_owned(),
                message: "Task chain stopped".to_owned(),
            }),
            None,
        ),
        ChainReason::Unfinished => {
            if interrupted {
                (
                    TaskStatus::Stopped,
                    Some(FailReason {
                        code: "INTERRUPTED".to_owned(),
                        message: "Run interrupted".to_owned(),
                    }),
                    None,
                )
            } else {
                (
                    TaskStatus::Failed,
                    Some(FailReason {
                        code: "UNFINISHED".to_owned(),
                        message: "Task did not finish".to_owned(),
                    }),
                    None,
                )
            }
        }
        ChainReason::Unstarted => (TaskStatus::NotRun, None, None),
    }
}

fn classify(
    dry_run: bool,
    interrupted: bool,
    init_failed: bool,
    connection_failed: bool,
    tasks: &[TaskReport],
) -> RunStatus {
    if dry_run {
        return RunStatus::DryRun;
    }
    if interrupted {
        return RunStatus::Interrupted;
    }
    if init_failed || connection_failed {
        return RunStatus::StartupFailure;
    }
    let any_failed = tasks
        .iter()
        .any(|task| matches!(task.status, TaskStatus::Failed | TaskStatus::Stopped));
    if any_failed {
        RunStatus::PartialFailure
    } else {
        RunStatus::Succeeded
    }
}

fn has_fight_data(fight: &FightReport) -> bool {
    fight.stage.is_some()
        || fight.times > 0
        || fight.sanity_used.is_some()
        || !fight.drops.is_empty()
        || fight.medicine.is_some()
        || fight.stone.is_some()
}

fn ingest_infrast_room(report: &mut InfrastReport, what: &str, details: &Value) {
    let Some(obj) = details.as_object() else {
        return;
    };
    let facility = obj
        .get("facility")
        .and_then(Value::as_str)
        .unwrap_or("Unknown")
        .to_owned();
    let index = obj.get("index").and_then(Value::as_i64).unwrap_or(0);
    let room = if let Some(existing) = report
        .rooms
        .iter_mut()
        .find(|room| room.facility == facility && room.index == index)
    {
        existing
    } else {
        report.rooms.push(InfrastRoom {
            facility,
            index,
            product: None,
            operators: Vec::new(),
            candidates: Vec::new(),
        });
        report
            .rooms
            .last_mut()
            .unwrap_or_else(|| unreachable!("just pushed"))
    };
    match what {
        "ProductOfFacility" => {
            room.product = obj
                .get("product")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        "CustomInfrastRoomOperators" => {
            room.operators = obj
                .get("names")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            room.candidates = obj
                .get("candidates")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default();
        }
        _ => {}
    }
}

fn award_claimed_from_task(task: &str) -> Option<&'static str> {
    let lower = task.to_ascii_lowercase();
    if lower.contains("without") {
        None
    } else if lower.contains("mail") && (lower.contains("receiveall") || lower.contains("receive"))
    {
        Some("mail")
    } else if lower.contains("receiveaward") || lower.contains("clickreceive") {
        Some("daily")
    } else if lower.contains("orundum") && lower.contains("receive") {
        Some("orundum")
    } else if lower.contains("mining") && lower.contains("receive") {
        Some("mining")
    } else if (lower.contains("specialaccess") || lower.contains("monthly"))
        && lower.contains("receive")
    {
        Some("specialaccess")
    } else if lower.contains("signin") && (lower.contains("earn") || lower.contains("receive")) {
        Some("signinevent")
    } else if lower.contains("recruit") && lower.contains("receive") {
        Some("recruit")
    } else {
        None
    }
}

fn award_checked_from_task(task: &str) -> Option<&'static str> {
    let lower = task.to_ascii_lowercase();
    if lower.contains("mail") {
        Some("mail")
    } else if lower.contains("weekly") {
        Some("weekly")
    } else if lower.contains("orundum") || lower.contains("lucky") {
        Some("orundum")
    } else if lower.contains("mining") {
        Some("mining")
    } else if lower.contains("specialaccess") || lower.contains("monthly") {
        Some("specialaccess")
    } else if lower.contains("signin") || lower.contains("sign_in") {
        Some("signinevent")
    } else if lower.contains("gacha") || (lower.contains("recruit") && !lower.contains("refresh")) {
        Some("recruit")
    } else if lower.contains("award") || lower.contains("daily") {
        Some("daily")
    } else {
        None
    }
}

const SKIP_CODES: &[&str] = &[
    "NO_PRTS_CARD",
    "NO_FULL_RECORD",
    "WEEKLY_CAP_REACHED",
    "MAX_CARDS_REACHED",
    "NO_AUTO_DEPLOY",
    "SANITY_NOT_ENOUGH",
    "NOTHING_TO_DO",
    "NO_RECRUIT_PERMIT",
    "VARIANT_INACTIVE",
    "DISABLED",
];

fn capture_named_reason(task: &mut TaskRecord, what: &str, details: Option<&Value>) {
    let code = normalize_code(what);
    if SKIP_CODES.contains(&code.as_str()) {
        task.skip_code = Some(code.clone());
        task.skip_message = Some(reason_message(what, details));
    }
    if let Some(details) = details {
        capture_reason_fields(task, details);
    }
}

fn capture_reason_fields(task: &mut TaskRecord, details: &Value) {
    let Some(obj) = details.as_object() else {
        return;
    };
    for key in ["code", "reason", "why", "error"] {
        if let Some(value) = obj.get(key).and_then(Value::as_str) {
            let code = normalize_code(value);
            if SKIP_CODES.contains(&code.as_str()) || looks_like_code(value) {
                if SKIP_CODES.contains(&code.as_str()) {
                    task.skip_code.get_or_insert(code.clone());
                    task.skip_message
                        .get_or_insert_with(|| reason_message(value, Some(details)));
                } else if task.last_error.is_none() {
                    task.last_error = Some(FailReason {
                        code,
                        message: value.to_owned(),
                    });
                }
            }
        }
    }
}

fn reason_message(what: &str, details: Option<&Value>) -> String {
    details
        .and_then(|value| {
            value
                .get("message")
                .or_else(|| value.get("why"))
                .or_else(|| value.get("reason"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| what.to_owned())
}

fn is_annihilation_what(what: &str) -> bool {
    let upper = what.to_ascii_uppercase();
    upper.contains("ANNIHILATION") || SKIP_CODES.contains(&upper.as_str())
}

fn looks_like_code(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn normalize_code(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn as_task_id(message: &Map<String, Value>) -> Option<AsstTaskId> {
    message
        .get("taskid")
        .and_then(Value::as_i64)
        .filter(|id| *id > 0)
        .map(|id| id as AsstTaskId)
}

fn is_benign_subtask_error(message: &Map<String, Value>) -> bool {
    let task = message
        .get("details")
        .and_then(|value| value.get("task"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if task.contains("LastBattleStageName") {
        return true;
    }
    let why = message.get("why").and_then(Value::as_str).unwrap_or("");
    let max_times = message
        .get("details")
        .and_then(|value| value.get("max_times"))
        .and_then(Value::as_i64)
        .unwrap_or(-1);
    why == "ExceededLimit" && max_times == 0
}

fn ingest_task_result(task: &mut TaskRecord, details: &Value) {
    let Some(obj) = details.as_object() else {
        return;
    };
    let status = obj.get("status").and_then(Value::as_str).unwrap_or("");
    let reason = obj
        .get("reason")
        .and_then(Value::as_str)
        .map(normalize_code);
    let partial = obj.get("partial").and_then(Value::as_bool).unwrap_or(false);
    match status {
        "skipped" => {
            if let Some(code) = reason {
                task.skip_code = Some(code.clone());
                task.skip_message = Some(reason_message(&code, Some(details)));
            }
        }
        "failed" if !partial => {
            if let Some(code) = reason {
                task.last_error = Some(FailReason {
                    message: reason_message(&code, Some(details)),
                    code,
                });
            }
        }
        _ => {}
    }
    if let Some(cards) = obj.get("cards_used") {
        task.annihilation
            .insert("cards_used".to_owned(), cards.clone());
    }
}

fn ingest_stage_drops(task: &mut TaskRecord, obj: &Map<String, Value>) {
    if let Some(stage) = obj.get("stage").and_then(Value::as_object) {
        let code = stage
            .get("stageCode")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let stage_id = stage
            .get("stageId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let stage_name = stage
            .get("stageName")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        if let Some(id) = stage_id {
            task.fight.stage_id = Some(id.to_owned());
        }
        if let Some(name) = stage_name {
            task.annihilation
                .entry("map_name".to_owned())
                .or_insert_with(|| Value::String(name.to_owned()));
        }
        if let Some(code) = code {
            if looks_like_stage_code(code) {
                if task.configured_stage.is_none() {
                    task.fight.stage = Some(code.to_owned());
                }
            } else {
                task.annihilation
                    .entry("region".to_owned())
                    .or_insert_with(|| Value::String(code.to_owned()));
                if stage_name.is_none() {
                    task.annihilation
                        .entry("map_name".to_owned())
                        .or_insert_with(|| Value::String(code.to_owned()));
                }
                if task.fight.stage.is_none() {
                    task.fight.stage = task
                        .configured_stage
                        .clone()
                        .or_else(|| Some("Annihilation".to_owned()));
                }
            }
        }
    }

    if let Some(drops) = obj.get("drops").and_then(Value::as_array) {
        for drop in drops {
            let Some(drop) = drop.as_object() else {
                continue;
            };
            let count = drop.get("quantity").and_then(Value::as_i64).unwrap_or(0);
            if let Some(id) = drop.get("itemId").and_then(Value::as_str) {
                *task.fight.drops.entry(id.to_owned()).or_insert(0) += count;
            }
            if let Some(name) = drop.get("itemName").and_then(Value::as_str) {
                *task.fight.drops_named.entry(name.to_owned()).or_insert(0) += count;
            }
        }
    }

    if let Some(stats) = obj.get("stats") {
        task.details
            .insert("drop_stats".to_owned(), normalize_drop_stats(stats));
    }

    if let Some(progress) = obj
        .get("annihilation_weekly_process")
        .and_then(Value::as_array)
        && progress.len() >= 2
    {
        let current = progress[0].clone();
        let cap = progress[1].clone();
        if !task.annihilation.contains_key("progress_before") {
            task.annihilation
                .insert("progress_before".to_owned(), current.clone());
        }
        task.annihilation
            .insert("progress_after".to_owned(), current);
        task.annihilation.insert("weekly_cap".to_owned(), cap);
        task.annihilation
            .insert("cards_used".to_owned(), Value::from(task.fight.times));
        if task.fight.stage.is_none() {
            task.fight.stage = task
                .configured_stage
                .clone()
                .or_else(|| Some("Annihilation".to_owned()));
        }
    }

    store_unknown_fields(&mut task.details, "StageDrops", obj, &[
        "drops",
        "stage",
        "stats",
        "stars",
        "annihilation_weekly_process",
    ]);
}

fn normalize_drop_stats(stats: &Value) -> Value {
    match stats {
        Value::Array(items) => {
            if items.iter().any(Value::is_array) {
                items
                    .last()
                    .cloned()
                    .unwrap_or_else(|| Value::Array(Vec::new()))
            } else {
                stats.clone()
            }
        }
        Value::Object(obj) => Value::Array(
            obj.iter()
                .map(|(item_id, quantity)| {
                    let mut entry = Map::new();
                    entry.insert("itemId".to_owned(), Value::String(item_id.clone()));
                    entry.insert("quantity".to_owned(), quantity.clone());
                    Value::Object(entry)
                })
                .collect(),
        ),
        other => Value::Array(vec![other.clone()]),
    }
}

fn looks_like_stage_code(code: &str) -> bool {
    code.eq_ignore_ascii_case("Annihilation")
        || code.contains("@Annihilation")
        || code.contains('-')
        || code.contains('_')
}

fn finalize_annihilation(task: &mut TaskRecord) {
    if task.task_type != "Fight" && task.annihilation.is_empty() {
        return;
    }
    let is_annihilation = task
        .configured_stage
        .as_deref()
        .is_some_and(|stage| stage.contains("Annihilation"))
        || task
            .fight
            .stage
            .as_deref()
            .is_some_and(|stage| stage.contains("Annihilation"))
        || task.annihilation.contains_key("weekly_cap")
        || task.annihilation.contains_key("prts_cards");
    if !is_annihilation {
        return;
    }

    task.annihilation
        .entry("cards_used".to_owned())
        .or_insert_with(|| Value::from(task.fight.times));
    if let Some(stage) = task.fight.stage.clone() {
        task.annihilation
            .entry("stage".to_owned())
            .or_insert(Value::String(stage));
    }

    if task.skip_code.is_some() {
        return;
    }

    let progress = task
        .annihilation
        .get("progress_after")
        .and_then(Value::as_i64);
    let cap = task.annihilation.get("weekly_cap").and_then(Value::as_i64);
    let cap_reached = matches!((progress, cap), (Some(p), Some(c)) if c > 0 && p >= c);
    let sanity = current_sanity(task.fight.sanity.as_ref())
        .or_else(|| current_sanity(task.fight.sanity_start.as_ref()));

    if task.fight.times == 0 && cap_reached {
        task.skip_code = Some("WEEKLY_CAP_REACHED".to_owned());
        task.skip_message = Some("Weekly annihilation cap already reached".to_owned());
        task.annihilation.insert(
            "reason".to_owned(),
            Value::String("WEEKLY_CAP_REACHED".to_owned()),
        );
    } else if !cap_reached && sanity.is_some_and(|value| value < 25) {
        task.annihilation.insert(
            "reason".to_owned(),
            Value::String("SANITY_NOT_ENOUGH".to_owned()),
        );
        if task.fight.times == 0 {
            task.skip_code = Some("SANITY_NOT_ENOUGH".to_owned());
            task.skip_message = Some("Sanity below annihilation cost (25)".to_owned());
        }
    }
}

fn current_sanity(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value
        .get("current_sanity")
        .or_else(|| value.get("current"))
        .and_then(Value::as_i64)
}

fn normalize_timestamps(mut value: Value) -> Value {
    if let Some(obj) = value.as_object_mut()
        && let Some(Value::String(time)) = obj.get_mut("report_time")
    {
        *time = to_rfc3339_z(time);
    }
    value
}

fn to_rfc3339_z(raw: &str) -> String {
    const FORMATS: &[&str] = &[
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
    ];
    for format in FORMATS {
        if let Ok(naive) = NaiveDateTime::parse_from_str(raw, format) {
            return naive.and_utc().to_rfc3339_opts(SecondsFormat::Secs, true);
        }
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true);
    }
    raw.to_owned()
}

fn merge_value(map: &mut BTreeMap<String, Value>, key: &str, value: Value) {
    if value.is_null() {
        return;
    }
    match map.get_mut(key) {
        Some(existing) => {
            let previous = std::mem::take(existing);
            *existing = match previous {
                Value::Array(mut arr) => {
                    arr.push(value);
                    Value::Array(arr)
                }
                other => Value::Array(vec![other, value]),
            };
        }
        None => {
            map.insert(key.to_owned(), value);
        }
    }
}

fn store_unknown_fields(
    map: &mut BTreeMap<String, Value>,
    what: &str,
    obj: &Map<String, Value>,
    known: &[&str],
) {
    let extra: Map<String, Value> = obj
        .iter()
        .filter(|(key, _)| !known.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if !extra.is_empty() {
        merge_value(map, what, Value::Object(extra));
    }
}

pub(crate) fn write_report(path: &Path, report: &RunReport) -> std::io::Result<()> {
    let mut json = serde_json::to_vec_pretty(report)?;
    if !json.ends_with(b"\n") {
        json.push(b'\n');
    }
    atomic_fs::write(path, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))?;
    }
    Ok(())
}

pub(crate) fn print_report(report: &RunReport) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{json}")
}

pub(crate) fn resource_version() -> Option<String> {
    let mut candidates = vec![
        maa_dirs::hot_update_resource().join("version.json"),
        maa_dirs::maa_resource()
            .join("resource")
            .join("version.json"),
        maa_dirs::maa_resource().join("version.json"),
    ];
    if let Some(dir) = maa_dirs::find_resource() {
        candidates.push(dir.join("version.json"));
        candidates.push(dir.join("resource").join("version.json"));
    }
    for path in candidates {
        if let Some(version) = read_version_file(&path) {
            return Some(version);
        }
    }
    None
}

fn read_version_file(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .get("version")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("last_updated")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| value.as_str().map(ToOwned::to_owned))
        .map(|raw| to_rfc3339_z(&raw))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use maa_types::MessageKind;

    use super::*;

    fn obj(json: &str) -> Map<String, Value> {
        serde_json::from_str::<Value>(json)
            .unwrap()
            .as_object()
            .cloned()
            .unwrap()
    }

    fn versions() -> Versions {
        Versions {
            cli: "0.7.5".to_owned(),
            core: Some("6.18.0".to_owned()),
            resource: Some("test".to_owned()),
        }
    }

    fn finished(recorder: Recorder) -> (RunReport, RunStatus) {
        recorder.finish(
            DateTime::parse_from_rfc3339("2026-04-01T01:00:00Z")
                .unwrap()
                .to_utc(),
        )
    }

    #[test]
    fn fight_aggregates_drops_and_sanity() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(0, 1, "Annihilation", TaskType::Fight, true, None, None);
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Fight","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{
                    "taskchain":"Fight","taskid":1,"what":"FightTimes",
                    "details":{"series":2,"sanity_cost":36,"times":2,"extra_core_field":true}
                }"#),
        );
        recorder.ingest(
            MessageKind::SubTaskStart,
            &obj(r#"{"taskchain":"Fight","taskid":1,"subtask":"ProcessTask",
                    "details":{"task":"StartButton2","exec_times":1}}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{
                    "taskchain":"Fight","taskid":1,"what":"StageDrops",
                    "details":{
                        "stage":{"stageCode":"1-7","stageId":"main_01-07"},
                        "drops":[
                            {"itemId":"30012","itemName":"固源岩","quantity":3},
                            {"itemId":"30011","itemName":"源岩","quantity":1}
                        ]
                    }
                }"#),
        );
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"Fight","taskid":1}"#),
        );

        let (report, status) = finished(recorder);
        assert_eq!(status, RunStatus::Succeeded);
        assert_eq!(report.tasks.len(), 1);
        let task = &report.tasks[0];
        assert_eq!(task.status, TaskStatus::Succeeded);
        let fight = task.fight.as_ref().unwrap();
        assert_eq!(fight.stage.as_deref(), Some("1-7"));
        assert_eq!(fight.times, 2);
        assert_eq!(fight.sanity_used, Some(36));
        assert_eq!(fight.drops.get("30012"), Some(&3));
        assert_eq!(fight.drops_named.get("固源岩"), Some(&3));
        assert_eq!(
            task.details
                .get("FightTimes")
                .and_then(|v| v.get("extra_core_field")),
            Some(&Value::Bool(true))
        );
    }

    #[test]
    fn annihilation_skip_reason_from_future_callback() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(
            0,
            1,
            "Annihilation",
            TaskType::Fight,
            true,
            Some("Annihilation".to_owned()),
            None,
        );
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Fight","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{
                    "taskchain":"Fight","taskid":1,"what":"AnnihilationStatus",
                    "details":{
                        "code":"WEEKLY_CAP_REACHED",
                        "progress_before":400,
                        "progress_after":400,
                        "weekly_cap":1800,
                        "prts_cards":0
                    }
                }"#),
        );
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"Fight","taskid":1}"#),
        );

        let (report, status) = finished(recorder);
        assert_eq!(status, RunStatus::Succeeded);
        let task = &report.tasks[0];
        assert_eq!(task.status, TaskStatus::Skipped);
        assert_eq!(
            task.skip_reason.as_ref().map(|r| r.code.as_str()),
            Some("WEEKLY_CAP_REACHED")
        );
        assert_eq!(
            task.annihilation
                .as_ref()
                .and_then(|v| v.get("progress_after"))
                .and_then(Value::as_i64),
            Some(400)
        );
        assert_eq!(
            task.annihilation
                .as_ref()
                .and_then(|v| v.get("prts_cards"))
                .and_then(Value::as_i64),
            Some(0)
        );
    }

    #[test]
    fn variant_skip_distinct_from_success() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_skipped(0, "Mall", TaskType::Mall);
        recorder.add_task(1, 1, "Award", TaskType::Award, true, None, None);
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Award","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskStart,
            &obj(r#"{"taskchain":"Award","taskid":1,"subtask":"ProcessTask",
                    "details":{"task":"AwardBegin"}}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskStart,
            &obj(r#"{"taskchain":"Award","taskid":1,"subtask":"ProcessTask",
                    "details":{"task":"Mail_ReceiveAll"}}"#),
        );
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"Award","taskid":1}"#),
        );

        let (report, _) = finished(recorder);
        assert_eq!(report.tasks[0].name, "Mall");
        assert_eq!(report.tasks[0].status, TaskStatus::Skipped);
        assert_eq!(
            report.tasks[0]
                .skip_reason
                .as_ref()
                .map(|r| r.code.as_str()),
            Some("VARIANT_INACTIVE")
        );
        assert_eq!(report.tasks[1].status, TaskStatus::Succeeded);
        assert_eq!(report.tasks[1].award.as_ref().unwrap().claimed, vec![
            "mail".to_owned()
        ]);
        assert_eq!(report.tasks[1].award.as_ref().unwrap().checked, vec![
            "daily".to_owned(),
            "mail".to_owned()
        ]);
    }

    #[test]
    fn recruit_and_infrast_details() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(0, 1, "Recruit", TaskType::Recruit, true, None, None);
        recorder.add_task(1, 2, "Infrast", TaskType::Infrast, true, None, None);

        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Recruit","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{"taskchain":"Recruit","taskid":1,"what":"RecruitResult",
                    "details":{"level":4,"tags":["减速","术师干员"]}}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskStart,
            &obj(
                r#"{"taskchain":"Recruit","taskid":1,"subtask":"ProcessTask",
                    "details":{"task":"RecruitConfirm"}}"#,
            ),
        );
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"Recruit","taskid":1}"#),
        );

        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Infrast","taskid":2}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{"taskchain":"Infrast","taskid":2,"what":"EnterFacility",
                    "details":{"facility":"Mfg","index":1}}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(
                r#"{"taskchain":"Infrast","taskid":2,"what":"ProductOfFacility",
                    "details":{"facility":"Mfg","index":1,"product":"Money"}}"#,
            ),
        );
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"Infrast","taskid":2}"#),
        );

        let (report, _) = finished(recorder);
        let recruit = report.tasks[0].recruit.as_ref().unwrap();
        assert_eq!(recruit.started, 1);
        assert_eq!(recruit.tags[0].tags, ["减速", "术师干员"]);
        assert_eq!(recruit.tags[0].state.as_deref(), Some("recruited"));

        let rooms = &report.tasks[1].infrast.as_ref().unwrap().rooms;
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].facility, "Mfg");
        assert_eq!(rooms[0].index, 1);
        assert_eq!(rooms[0].product.as_deref(), Some("Money"));
    }

    #[test]
    fn failed_task_and_not_run_and_startup() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(0, 1, "StartUp", TaskType::StartUp, true, None, None);
        recorder.add_task(1, 2, "Fight", TaskType::Fight, true, None, None);
        recorder.ingest(
            MessageKind::ConnectionInfo,
            &obj(r#"{"what":"ConnectFailed","why":"adb failed"}"#),
        );
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"StartUp","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskError,
            &obj(r#"{"taskchain":"StartUp","taskid":1,"subtask":"StartGameTask"}"#),
        );
        recorder.ingest(
            MessageKind::TaskChainError,
            &obj(r#"{"taskchain":"StartUp","taskid":1}"#),
        );

        let (report, status) = finished(recorder);
        assert_eq!(status, RunStatus::StartupFailure);
        assert_eq!(report.tasks[0].status, TaskStatus::Failed);
        assert_eq!(
            report.tasks[0]
                .fail_reason
                .as_ref()
                .map(|r| r.code.as_str()),
            Some("FAILED_TO_START_GAME")
        );
        assert_eq!(report.tasks[1].status, TaskStatus::NotRun);
        assert_eq!(status.exit_code(false), 1);
        assert_eq!(status.exit_code(true), 3);
    }

    #[test]
    fn partial_failure_exit_codes() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(0, 1, "Fight", TaskType::Fight, true, None, None);
        recorder.ingest(MessageKind::ConnectionInfo, &obj(r#"{"what":"Connected"}"#));
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Fight","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::TaskChainError,
            &obj(r#"{"taskchain":"Fight","taskid":1,"details":{"error":"OpenCVException"}}"#),
        );
        let (report, status) = finished(recorder);
        assert_eq!(status, RunStatus::PartialFailure);
        assert_eq!(report.outcome, Outcome::Partial);
        assert_eq!(
            report.tasks[0]
                .fail_reason
                .as_ref()
                .map(|r| r.code.as_str()),
            Some("OPENCVEXCEPTION")
        );
        assert_eq!(status.exit_code(false), 1);
        assert_eq!(status.exit_code(true), 2);
    }

    #[test]
    fn disabled_task_is_skipped() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(0, 1, "Mall", TaskType::Mall, false, None, None);
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Mall","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"Mall","taskid":1}"#),
        );
        let (report, _) = finished(recorder);
        assert_eq!(report.tasks[0].status, TaskStatus::Skipped);
        assert_eq!(
            report.tasks[0]
                .skip_reason
                .as_ref()
                .map(|r| r.code.as_str()),
            Some("DISABLED")
        );
    }

    #[test]
    fn writes_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.json");
        let recorder = Recorder::new(versions(), true);
        let (report, status) = finished(recorder);
        assert_eq!(status, RunStatus::DryRun);
        write_report(&path, &report).unwrap();
        let parsed: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed["versions"]["cli"], "0.7.5");
        assert_eq!(parsed["outcome"], "dry_run");
        assert_eq!(parsed["connection"]["status"], "not_attempted");
    }

    #[test]
    fn unknown_callback_fields_are_preserved() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(0, 1, "Status", TaskType::Custom, true, None, None);
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Custom","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{
                    "taskchain":"Custom","taskid":1,"what":"StatusSnapshot",
                    "details":{"sanity":{"current":80,"max":135},"prts_cards":3}
                }"#),
        );
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"Custom","taskid":1}"#),
        );
        let (report, _) = finished(recorder);
        assert_eq!(report.tasks[0].details["StatusSnapshot"]["prts_cards"], 3);
        assert_eq!(
            report.tasks[0].details["StatusSnapshot"]["sanity"]["current"],
            80
        );
    }

    #[test]
    fn stock_annihilation_from_stage_drops() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(0, 1, "StartUp", TaskType::StartUp, true, None, None);
        recorder.add_task(
            1,
            2,
            "Annihilation",
            TaskType::Fight,
            true,
            Some("Annihilation".to_owned()),
            None,
        );
        recorder.ingest(MessageKind::ConnectionInfo, &obj(r#"{"what":"Connected"}"#));
        recorder.ingest(
            MessageKind::ConnectionInfo,
            &obj(r#"{"what":"UuidGot","why":"abc"}"#),
        );
        recorder.ingest(
            MessageKind::ConnectionInfo,
            &obj(r#"{"what":"EmulatorFPS","why":"Normal"}"#),
        );
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"StartUp","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"StartUp","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Fight","taskid":2}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{
                "taskchain":"Fight","taskid":2,"what":"SanityBeforeStage",
                "details":{"current_sanity":20,"max_sanity":135,"report_time":"2026-04-01 00:00:00"}
            }"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{
                "taskchain":"Fight","taskid":2,"what":"StageDrops",
                "details":{
                    "stage":{"stageCode":"拉特兰","stageId":"","stageName":"默祷圣祠"},
                    "stats":{"4001":10},
                    "annihilation_weekly_process":[740,1800]
                }
            }"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{
                "taskchain":"Fight","taskid":2,"what":"StageDrops",
                "details":{
                    "stage":{"stageCode":"拉特兰","stageId":"","stageName":"默祷圣祠"},
                    "stats":[{"itemId":"4001","quantity":20}],
                    "annihilation_weekly_process":[740,1800]
                }
            }"#),
        );
        recorder.ingest(
            MessageKind::SubTaskError,
            &obj(r#"{
                "taskchain":"StartUp","taskid":0,"subtask":"ProcessTask","why":"ExceededLimit",
                "details":{"task":"ReturnButton","max_times":0}
            }"#),
        );
        recorder.ingest(
            MessageKind::SubTaskError,
            &obj(r#"{
                "taskchain":"Fight","taskid":2,"subtask":"ProcessTask","why":"ExceededLimit",
                "details":{"task":"LastBattleStageName","max_times":1}
            }"#),
        );
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"Fight","taskid":2}"#),
        );

        let (report, _) = finished(recorder);
        assert_eq!(report.connection.what.as_deref(), Some("Connected"));
        assert_eq!(report.tasks.len(), 2);
        assert_eq!(report.tasks[0].name, "StartUp");
        assert_eq!(report.tasks[0].status, TaskStatus::Succeeded);
        let task = &report.tasks[1];
        assert_eq!(
            task.fight.as_ref().unwrap().stage.as_deref(),
            Some("Annihilation")
        );
        assert!(task.fight.as_ref().unwrap().stage_id.is_none());
        assert_eq!(
            task.annihilation
                .as_ref()
                .and_then(|v| v.get("map_name"))
                .and_then(Value::as_str),
            Some("默祷圣祠")
        );
        assert_eq!(
            task.annihilation
                .as_ref()
                .and_then(|v| v.get("region"))
                .and_then(Value::as_str),
            Some("拉特兰")
        );
        assert_eq!(
            task.annihilation
                .as_ref()
                .and_then(|v| v.get("weekly_cap"))
                .and_then(Value::as_i64),
            Some(1800)
        );
        assert_eq!(
            task.annihilation
                .as_ref()
                .and_then(|v| v.get("progress_after"))
                .and_then(Value::as_i64),
            Some(740)
        );
        assert_eq!(
            task.annihilation
                .as_ref()
                .and_then(|v| v.get("cards_used"))
                .and_then(Value::as_i64),
            Some(0)
        );
        assert_eq!(
            task.skip_reason.as_ref().map(|r| r.code.as_str()),
            Some("SANITY_NOT_ENOUGH")
        );
        assert_eq!(task.status, TaskStatus::Skipped);
        assert!(!task.details.contains_key("last_subtask_error"));
        assert!(task.details["drop_stats"].is_array());
        assert!(!task.details["drop_stats"][0].is_array());
        assert_eq!(task.details["drop_stats"][0]["quantity"], 20);
        assert_eq!(
            task.fight
                .as_ref()
                .and_then(|f| f.sanity_start.as_ref())
                .and_then(|v| v.get("report_time"))
                .and_then(Value::as_str),
            Some("2026-04-01T00:00:00Z")
        );
    }

    #[test]
    fn award_checked_not_claimed_without_receive() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(0, 1, "Award", TaskType::Award, true, None, None);
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Award","taskid":1}"#),
        );
        for task in [
            "DailyTask",
            "WeeklyTask",
            "MailWithoutAward",
            "RecruitingActivitiesBegin",
            "OrundumActivitiesBegin",
            "MiningActivitiesBegin",
            "SpecialAccessActivitiesBegin",
        ] {
            recorder.ingest(
                MessageKind::SubTaskStart,
                &obj(&format!(
                    r#"{{"taskchain":"Award","taskid":1,"subtask":"ProcessTask","details":{{"task":"{task}"}}}}"#
                )),
            );
        }
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"Award","taskid":1}"#),
        );
        let (report, _) = finished(recorder);
        let award = report.tasks[0].award.as_ref().unwrap();
        assert!(award.claimed.is_empty());
        assert!(award.checked.contains(&"daily".to_owned()));
        assert!(award.checked.contains(&"mail".to_owned()));
        assert!(award.checked.contains(&"recruit".to_owned()));
    }

    #[test]
    fn game_status_snapshot_and_partial_task_result() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(0, 1, "Status", TaskType::Status, true, None, None);
        recorder.ingest(
            MessageKind::TaskChainStart,
            &obj(r#"{"taskchain":"Status","taskid":1}"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{
                "taskchain":"Status","taskid":1,"what":"GameStatus",
                "details":{"sanity":{"current":80,"max":135},"annihilation":{"weekly_progress":370,"weekly_cap":1800},"errors":["drones"]}
            }"#),
        );
        recorder.ingest(
            MessageKind::SubTaskExtraInfo,
            &obj(r#"{
                "taskchain":"Status","taskid":1,"what":"TaskResult",
                "details":{"status":"failed","reason":"RECOGNITION_FAILED","partial":true}
            }"#),
        );
        recorder.ingest(
            MessageKind::TaskChainCompleted,
            &obj(r#"{"taskchain":"Status","taskid":1}"#),
        );
        let (report, _) = finished(recorder);
        assert_eq!(report.tasks[0].status, TaskStatus::Succeeded);
        assert_eq!(
            report.game_status.as_ref().unwrap()["sanity"]["current"],
            80
        );
        assert_eq!(
            report.tasks[0].game_status.as_ref().unwrap()["errors"][0],
            "drones"
        );
        assert_eq!(last_game_status().unwrap()["sanity"]["current"], 80);
    }

    #[test]
    fn dry_run_uses_configured_fight_times() {
        let mut recorder = Recorder::new(versions(), true);
        recorder.add_task(
            0,
            1,
            "Fight",
            TaskType::Fight,
            true,
            Some("1-7".to_owned()),
            Some(5),
        );
        let (report, status) = finished(recorder);
        assert_eq!(status, RunStatus::DryRun);
        assert_eq!(report.tasks[0].fight.as_ref().unwrap().times, 5);
        assert_eq!(
            report.tasks[0].fight.as_ref().unwrap().stage.as_deref(),
            Some("1-7")
        );
    }

    #[cfg(unix)]
    #[test]
    fn report_file_is_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.json");
        let (report, _) = finished(Recorder::new(versions(), true));
        write_report(&path, &report).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }
}
