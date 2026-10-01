use anyhow::{Result, bail};
use maa_types::TaskType;
use maa_value::prelude::*;

use crate::config::{
    asst::AsstConfig,
    task::{Task, TaskConfig},
};

const STATUS_FIELDS: &[&str] = &[
    "sanity",
    "currency",
    "orundum",
    "originite",
    "lmd",
    "annihilation",
    "depot",
    "drones",
];

/// Parameters for `maa status`.
#[derive(clap::Args)]
pub struct StatusParams {
    /// Fields to collect, comma-separated
    ///
    /// Any of `sanity`, `currency` (aliases: `orundum` / `originite` / `lmd`),
    /// `annihilation`, `depot`, `drones`. Omit to let the core default to sanity.
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub fields: Vec<String>,
    /// Run StartUp first so the game reaches the home screen
    ///
    /// Not needed when the client is already on the main screen.
    #[arg(long)]
    pub startup: bool,
}

impl super::IntoTaskConfig for StatusParams {
    fn into_task_config(self, _config: &AsstConfig) -> Result<TaskConfig> {
        for field in &self.fields {
            if !STATUS_FIELDS.contains(&field.as_str()) {
                bail!(
                    "unknown status field `{field}`, expected one of {}",
                    STATUS_FIELDS.join(", ")
                );
            }
        }

        let mut tasks = Vec::new();
        if self.startup {
            tasks.push(Task::new(TaskType::StartUp, MAAValue::default()));
        }

        let mut params = MAAValue::default();
        if !self.fields.is_empty() {
            let fields = self
                .fields
                .into_iter()
                .map(MAAValue::from)
                .collect::<Vec<_>>();
            insert!(params, "fields" => MAAValue::Array(fields));
        }
        tasks.push(Task::new(TaskType::Status, params));

        TaskConfig::new_with_tasks(tasks)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::{
        command::{Command, parse_from},
        run::preset::IntoTaskConfig,
    };

    #[test]
    fn parse_status_params() {
        let command = parse_from([
            "maa",
            "status",
            "--fields",
            "sanity,annihilation",
            "--startup",
        ]);
        match command.command {
            Command::Status {
                params,
                json,
                common: _,
            } => {
                assert_eq!(params.fields, ["sanity", "annihilation"]);
                assert!(params.startup);
                assert!(!json);
                let config = params.into_task_config(&AsstConfig::default()).unwrap();
                assert_eq!(config.tasks.len(), 2);
                assert_eq!(config.tasks[0].task_type, TaskType::StartUp);
                assert_eq!(config.tasks[1].task_type, TaskType::Status);
            }
            _ => panic!("expected Status command"),
        }
    }

    #[test]
    fn rejects_unknown_field() {
        let params = StatusParams {
            fields: vec!["box".to_owned()],
            startup: false,
        };
        assert!(params.into_task_config(&AsstConfig::default()).is_err());
    }
}
