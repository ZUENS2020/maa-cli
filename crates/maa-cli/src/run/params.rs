//! Known MaaCore task parameter keys, used by `--dry-run` to warn about typos.

use maa_types::TaskType;
use maa_value::prelude::*;

/// Return unknown parameter names for a known task type.
///
/// Returns `None` when the task type is free-form (Custom / SingleStep /
/// VideoRecognition) or has no curated key list, so callers skip the warning.
///
/// The lists track stock MaaCore 6.18 plus fork extras (`Status`, annihilation
/// `on_no_*` / `max_cards`). Unknown keys are a dry-run warning only.
pub fn unknown_param_keys(task_type: TaskType, params: &MAAValue) -> Option<Vec<String>> {
    let known = known_keys(task_type)?;
    let map = params.as_map()?;
    let unknown = map
        .keys()
        .filter(|key| !known.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    Some(unknown)
}

pub fn warn_unknown_params(task_type: TaskType, name: &str, params: &MAAValue) {
    let Some(unknown) = unknown_param_keys(task_type, params) else {
        return;
    };
    for key in unknown {
        log::warn!("Unknown parameter `{key}` for {task_type} task [{name}]");
    }
}

fn known_keys(task_type: TaskType) -> Option<&'static [&'static str]> {
    Some(match task_type {
        TaskType::StartUp => &[
            "enable",
            "client_type",
            "start_game_enabled",
            "account_name",
        ],
        TaskType::CloseDown => &["enable", "client_type"],
        TaskType::Fight => &[
            "enable",
            "stage",
            "medicine",
            "expiring_medicine",
            "stone",
            "times",
            "series",
            "drops",
            "report_to_penguin",
            "penguin_id",
            "report_to_yituliu",
            "yituliu_id",
            "server",
            "client_type",
            "DrGrandet",
            "medicine_expire_days",
            "on_no_card",
            "on_no_record",
            "max_cards",
        ],
        TaskType::Recruit => &[
            "enable",
            "refresh",
            "select",
            "confirm",
            "first_tags",
            "extra_tags_mode",
            "times",
            "set_time",
            "expedite",
            "expedite_times",
            "skip_robot",
            "force_refresh",
            "level3_recruitment_permit_reserve",
            "recruitment_time",
            "report_to_penguin",
            "penguin_id",
            "report_to_yituliu",
            "yituliu_id",
            "server",
        ],
        TaskType::Infrast => &[
            "enable",
            "mode",
            "facility",
            "drones",
            "threshold",
            "replenish",
            "dorm_notstationed_enabled",
            "dorm_trust_enabled",
            "reception_message_board",
            "reception_clue_exchange",
            "reception_send_clue",
            "filename",
            "plan_index",
            "continue_training",
            "fiammetta_recovery_enabled",
            "fiammetta_targets",
            "use_abyssal_hunter",
            "use_perception_information",
            "use_pinus_sylvestris",
            "use_worldly_plight",
        ],
        TaskType::Mall => &[
            "enable",
            "visit_friends",
            "shopping",
            "buy_first",
            "blacklist",
            "force_shopping_if_credit_full",
            "only_buy_discount",
            "reserve_max_credit",
            "credit_fight",
            "formation_index",
            "select_formation",
        ],
        TaskType::Award => &[
            "enable",
            "award",
            "mail",
            "recruit",
            "orundum",
            "mining",
            "specialaccess",
            "signinevent",
        ],
        TaskType::Roguelike => &[
            "enable",
            "theme",
            "mode",
            "squad",
            "roles",
            "core_char",
            "use_support",
            "use_nonfriend_support",
            "starts_count",
            "difficulty",
            "stop_at_final_boss",
            "stop_at_max_level",
            "investment_enabled",
            "investments_count",
            "stop_when_investment_full",
            "investment_with_more_score",
            "start_with_elite_two",
            "only_start_with_elite_two",
            "refresh_trader_with_dice",
            "first_floor_foldartal",
            "start_foldartal_list",
            "collectible_mode_start_list",
            "use_foldartal",
            "check_collapsal_paradigms",
            "double_check_collapsal_paradigms",
            "expected_collapsal_paradigms",
            "monthly_squad_auto_iterate",
            "monthly_squad_check_comms",
            "deep_exploration_auto_iterate",
            "collectible_mode_shopping",
            "collectible_mode_squad",
            "start_with_seed",
        ],
        TaskType::Copilot => &[
            "enable",
            "filename",
            "copilot_list",
            "loop_times",
            "use_sanity_potion",
            "formation",
            "formation_index",
            "user_additional",
            "add_trust",
            "ignore_requirements",
            "support_unit_usage",
            "support_unit_name",
        ],
        TaskType::SSSCopilot => &["enable", "filename", "loop_times"],
        TaskType::ParadoxCopilot => &["enable", "filename", "list"],
        TaskType::Depot => &["enable"],
        TaskType::OperBox => &["enable"],
        TaskType::Reclamation => &[
            "enable",
            "theme",
            "mode",
            "tools_to_craft",
            "increment_mode",
            "num_craft_batches",
            "clear_store",
        ],
        TaskType::Status => &[
            "enable",
            "fields",
            "sanity",
            "currency",
            "orundum",
            "originite",
            "lmd",
            "annihilation",
            "depot",
            "drones",
        ],
        TaskType::Custom | TaskType::SingleStep | TaskType::VideoRecognition => return None,
    })
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn detects_typo_for_known_type() {
        let params = object!("stage" => "1-7", "mial" => true, "medicine" => 1);
        let unknown = unknown_param_keys(TaskType::Fight, &params).unwrap();
        assert_eq!(unknown, vec!["mial".to_owned()]);
    }

    #[test]
    fn known_keys_are_silent() {
        let params = object!("stage" => "Annihilation", "client_type" => "Official");
        assert!(
            unknown_param_keys(TaskType::Fight, &params)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn custom_tasks_are_not_checked() {
        let params = object!("whatever" => true);
        assert!(unknown_param_keys(TaskType::Custom, &params).is_none());
    }

    #[test]
    fn award_typo() {
        let params = object!("award" => true, "maill" => true);
        let unknown = unknown_param_keys(TaskType::Award, &params).unwrap();
        assert_eq!(unknown, vec!["maill".to_owned()]);
    }

    #[test]
    fn core_618_and_status_keys_are_known() {
        let fight = object!(
            "stage" => "Annihilation",
            "on_no_card" => "skip",
            "on_no_record" => "fail",
            "max_cards" => 2,
            "medicine_expire_days" => 3
        );
        assert!(
            unknown_param_keys(TaskType::Fight, &fight)
                .unwrap()
                .is_empty()
        );

        let status = object!("sanity" => true, "annihilation" => true, "fields" => "sanity");
        assert!(
            unknown_param_keys(TaskType::Status, &status)
                .unwrap()
                .is_empty()
        );

        let award = object!("award" => true, "signinevent" => true);
        assert!(
            unknown_param_keys(TaskType::Award, &award)
                .unwrap()
                .is_empty()
        );

        let infrast = object!(
            "continue_training" => true,
            "fiammetta_recovery_enabled" => true,
            "fiammetta_targets" => "Castle-3",
            "use_abyssal_hunter" => true,
            "use_perception_information" => true,
            "use_pinus_sylvestris" => true,
            "use_worldly_plight" => true
        );
        assert!(
            unknown_param_keys(TaskType::Infrast, &infrast)
                .unwrap()
                .is_empty()
        );

        let recruit = object!(
            "force_refresh" => true,
            "level3_recruitment_permit_reserve" => 1
        );
        assert!(
            unknown_param_keys(TaskType::Recruit, &recruit)
                .unwrap()
                .is_empty()
        );

        let mall = object!("select_formation" => 1);
        assert!(
            unknown_param_keys(TaskType::Mall, &mall)
                .unwrap()
                .is_empty()
        );

        let reclamation = object!("clear_store" => true);
        assert!(
            unknown_param_keys(TaskType::Reclamation, &reclamation)
                .unwrap()
                .is_empty()
        );

        let paradox = object!("list" => true);
        assert!(
            unknown_param_keys(TaskType::ParadoxCopilot, &paradox)
                .unwrap()
                .is_empty()
        );
    }
}
