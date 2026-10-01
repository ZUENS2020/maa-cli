//! Structured JSON run report built from MaaCore callbacks.
//!
//! The schema is intentionally open: unknown extra-info `what` values and leftover
//! callback fields are stored under each task's [`TaskReport::details`] object so
//! future core callbacks (Status, annihilation reason codes, ...) land without a
//! schema change.

use std::{collections::BTreeMap, io::Write, path::Path, sync::Mutex};

use chrono::{DateTime, Utc};
use maa_types::{MessageKind, TaskType, primitive::AsstTaskId};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::atomic_fs;

static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);

fn with_recorder_mut<T>(f: impl FnOnce(&mut Recorder) -> T) -> Option<T> {
    RECORDER.lock().unwrap().as_mut().map(f)
}

pub(crate) fn init(recorder: Recorder) {
    *RECORDER.lock().unwrap() = Some(recorder);
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
) {
    let name = name.into();
    with_recorder_mut(|recorder| recorder.add_task(index, id, name, task_type, enabled));
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
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct FightReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
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
    fight: FightReport,
    annihilation: BTreeMap<String, Value>,
    recruit: RecruitReport,
    infrast: InfrastReport,
    award: AwardReport,
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
            fight: FightReport::default(),
            annihilation: BTreeMap::new(),
            recruit: RecruitReport::default(),
            infrast: InfrastReport::default(),
            award: AwardReport::default(),
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

    pub(crate) fn add_task(
        &mut self,
        index: usize,
        id: AsstTaskId,
        name: impl Into<String>,
        task_type: TaskType,
        enabled: bool,
    ) {
        let mut record =
            TaskRecord::new(index, name.into(), task_type.to_str().to_owned(), enabled);
        record.id = Some(id);
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

    fn task_by_id_mut(&mut self, id: AsstTaskId) -> Option<&mut TaskRecord> {
        self.tasks.iter_mut().find(|task| task.id == Some(id))
    }

    fn current_mut(&mut self) -> Option<&mut TaskRecord> {
        let id = self.current?;
        self.task_by_id_mut(id)
    }

    fn ensure_task(&mut self, id: AsstTaskId, taskchain: Option<&str>) -> &mut TaskRecord {
        if let Some(index) = self.tasks.iter().position(|task| task.id == Some(id)) {
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
        if let Some(id) = as_task_id(message) {
            let taskchain = message.get("taskchain").and_then(Value::as_str);
            Some(self.ensure_task(id, taskchain))
        } else {
            self.current_mut()
        }
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
        self.connection_what = Some(what.to_owned());
        if let Some(why) = message.get("why").and_then(Value::as_str) {
            self.connection_why = Some(why.to_owned());
        }
        match what {
            "Connected" | "UuidGot" | "Reconnected" => self.connection_ok = true,
            "ConnectFailed" | "Disconnect" => self.connection_failed = true,
            "UnsupportedResolution" | "ResolutionError" | "TouchModeNotAvailable" => {
                self.connection_failed = true;
            }
            _ => {}
        }
    }

    fn ingest_taskchain(&mut self, kind: MessageKind, message: &Map<String, Value>) {
        let taskchain = message.get("taskchain").and_then(Value::as_str);
        let id = as_task_id(message).unwrap_or_else(|| {
            let id = self.next_synthetic_id;
            self.next_synthetic_id -= 1;
            id
        });
        let task = self.ensure_task(id, taskchain);
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
            && let Some(claim) = award_claim_from_task(task_name)
            && !task.award.claimed.iter().any(|item| item == claim)
        {
            task.award.claimed.push(claim.to_owned());
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
            return;
        };

        capture_named_reason(task, what, Some(&details));
        match what {
            "FightTimes" => {
                if let Some(obj) = details.as_object() {
                    let series = obj.get("series").and_then(Value::as_i64).unwrap_or(0);
                    let sanity = obj.get("sanity_cost").and_then(Value::as_i64).unwrap_or(0);
                    task.last_series = (series, sanity);
                    if let Some(times) = obj.get("times").and_then(Value::as_i64) {
                        // Core reports cumulative times for the current series selection.
                        merge_value(&mut task.details, "fight_times_total", Value::from(times));
                    }
                    store_unknown_fields(&mut task.details, "FightTimes", obj, &[
                        "series",
                        "sanity_cost",
                        "times",
                    ]);
                }
            }
            "StageDrops" => {
                if let Some(obj) = details.as_object() {
                    if let Some(stage) = obj
                        .get("stage")
                        .and_then(|v| v.get("stageCode"))
                        .and_then(Value::as_str)
                        && task.fight.stage.is_none()
                    {
                        task.fight.stage = Some(stage.to_owned());
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
                                *task.fight.drops_named.entry(name.to_owned()).or_insert(0) +=
                                    count;
                            }
                        }
                    }
                    if let Some(stats) = obj.get("stats").and_then(Value::as_array) {
                        merge_value(&mut task.details, "drop_stats", Value::Array(stats.clone()));
                    }
                    store_unknown_fields(&mut task.details, "StageDrops", obj, &[
                        "drops", "stage", "stats", "stars",
                    ]);
                }
            }
            "SanityBeforeStage" => {
                task.fight.sanity = Some(details.clone());
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

    let award = if task.task_type == "Award" || !task.award.claimed.is_empty() {
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

fn award_claim_from_task(task: &str) -> Option<&'static str> {
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
        .map(|id| id as AsstTaskId)
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
    atomic_fs::write(path, json)
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
        recorder.add_task(0, 1, "Annihilation", TaskType::Fight, true);
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
        recorder.add_task(0, 1, "Annihilation", TaskType::Fight, true);
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
        recorder.add_task(1, 1, "Award", TaskType::Award, true);
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
            "daily".to_owned(),
            "mail".to_owned()
        ]);
    }

    #[test]
    fn recruit_and_infrast_details() {
        let mut recorder = Recorder::new(versions(), false);
        recorder.add_task(0, 1, "Recruit", TaskType::Recruit, true);
        recorder.add_task(1, 2, "Infrast", TaskType::Infrast, true);

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
        recorder.add_task(0, 1, "StartUp", TaskType::StartUp, true);
        recorder.add_task(1, 2, "Fight", TaskType::Fight, true);
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
        recorder.add_task(0, 1, "Fight", TaskType::Fight, true);
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
        recorder.add_task(0, 1, "Mall", TaskType::Mall, false);
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
        recorder.add_task(0, 1, "Status", TaskType::Custom, true);
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
}
