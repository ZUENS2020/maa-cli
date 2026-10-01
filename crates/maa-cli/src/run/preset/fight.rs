use anyhow::{Context, bail};
use maa_value::prelude::*;

use crate::config::task::ClientType;

#[derive(clap::Args)]
pub struct FightParams {
    /// Stage to fight, e.g. 1-7, leave empty to fight current/last stage
    stage: Option<String>,
    #[clap(short, long)]
    /// Number of medicine (Sanity Potion) used to fight, default to 0
    medicine: Option<i32>,
    #[clap(long)]
    /// Number of expiring medicine (Sanity Potion) used to fight, default to 0
    expiring_medicine: Option<i32>,
    #[clap(long)]
    /// Number of stone (Originite Prime) used to fight, default to 0
    stone: Option<i32>,
    #[clap(long)]
    /// Exit after fighting given times, default to infinite
    times: Option<i32>,
    #[clap(short = 'D', long, action = clap::ArgAction::Append)]
    /// Exit after collecting given number of drops, default to no limit
    ///
    /// Example: `-D30012=100` to exit after get 100 Orirock Cube,
    /// 30012 is the item ID of Orirock Cube, you can find it at `item_index.json`.
    /// You can specify multiple drops, by repeating this option,
    /// e.g. `-D30012=100 -D30011=100` to exit after get 100 Orirock or 100 Orirock Cube.
    drops: Vec<String>,
    #[clap(long)]
    /// Repeat times of single proxy combat (-1 ~ 6), default to 1
    ///
    /// - -1: disable switching series,
    /// - 0: automatically switch to the maximum number of series currently available, if the
    ///   current sanity is less than 6 times, select the minimum number of times available,
    /// - 1 ~ 6: uee the specified number of times (default to 1).
    series: Option<i32>,
    #[clap(long)]
    /// Whether report drops to the Penguin Statistics
    report_to_penguin: bool,
    #[clap(long)]
    /// Penguin Statistics ID to report drops, leave empty to report anonymously
    penguin_id: Option<String>,
    #[clap(long)]
    /// Whether report drops to the yituliu
    report_to_yituliu: bool,
    #[clap(long)]
    /// Yituliu ID to report drops, leave empty to report anonymously
    yituliu_id: Option<String>,
    #[clap(long)]
    /// Client type used to restart the game client if game crashed
    client_type: Option<ClientType>,
    #[clap(long)]
    /// Whether to use Originites like Dr. Grandet
    ///
    /// In DrGrandet mode, Wait in the using Originites confirmation screen until
    /// the 1 point of sanity has been restored and then immediately use the Originite.
    dr_grandet: bool,
    /// Annihilation only: what to do when no PRTS proxy card is available
    ///
    /// `current` (default) keeps the historical flow, `skip` / `fail` /
    /// `normal_deploy` require a newer MaaCore. Older cores ignore this key.
    #[arg(long, value_parser = ["current", "skip", "fail", "normal_deploy"])]
    on_no_card: Option<String>,
    /// Annihilation only: what to do when the map has no 400-kill full record
    #[arg(long, value_parser = ["current", "skip", "fail"])]
    on_no_record: Option<String>,
    /// Annihilation only: maximum PRTS proxy cards to consume this run
    #[arg(long)]
    max_cards: Option<i32>,
}

impl super::ToTaskType for FightParams {
    fn to_task_type(&self) -> super::TaskType {
        super::TaskType::Fight
    }
}

impl super::IntoParameters for FightParams {
    fn into_parameters_no_context(self) -> anyhow::Result<MAAValue> {
        let mut params = object!(
            "stage" => self.stage.unwrap_or_default(),
            "DrGrandet" => self.dr_grandet
        );

        // Fight conditions - optional parameters
        insert!(params,
            "medicine" =>? self.medicine,
            "expiring_medicine" =>? self.expiring_medicine,
            "stone" =>? self.stone,
            "times" =>? self.times,
            "series" =>? self.series,
            "on_no_card" =>? self.on_no_card,
            "on_no_record" =>? self.on_no_record,
            "max_cards" =>? self.max_cards
        );

        // Drops handling
        let drops = self.drops;
        if !drops.is_empty() {
            let mut drop_map = maa_value::map::StringMap::new();

            for drop in drops {
                let mut parts = drop.split('=');
                let item_id = parts.next();
                let count = parts.next();

                match (item_id, count) {
                    (Some(item_id), Some(count)) => {
                        let count: i32 = count
                            .parse()
                            .with_context(|| format!(" Failed to parse drop count: {count}"))?;

                        drop_map.insert(item_id.to_owned(), count.into());
                    }
                    _ => {
                        bail!("Invalid drop format: {}", drop)
                    }
                }
            }

            insert!(params, "drops" => MAAValue::Object(drop_map));
        }

        // Penguin Statistics reporting
        if self.report_to_penguin {
            insert!(params,
                "report_to_penguin" => true,
                "penguin_id" =>? self.penguin_id
            );
        }

        // Yituliu reporting
        if self.report_to_yituliu {
            insert!(params,
                "report_to_yituliu" => true,
                "yituliu_id" =>? self.yituliu_id
            );
        }

        // Client type
        if let Some(client_type) = self.client_type {
            insert!(params,
                "client_type" => client_type.to_str(),
                "server" =>? client_type.server_report()
            );
        }

        Ok(params)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::command::{Command, parse_from};

    #[test]
    fn parse_fight_params() {
        fn parse<I, T>(args: I) -> anyhow::Result<MAAValue>
        where
            I: IntoIterator<Item = T>,
            T: Into<std::ffi::OsString> + Clone,
        {
            let command = parse_from(args).command;
            match command {
                Command::Fight { params, .. } => {
                    use super::super::{IntoParameters, TaskType, ToTaskType};
                    assert_eq!(params.to_task_type(), TaskType::Fight);
                    params.into_parameters_no_context()
                }
                _ => panic!("Not a Fight command"),
            }
        }

        let default_params = object!(
            "stage" => "",
            "DrGrandet" => false,
        );

        assert_eq!(parse(["maa", "fight"]).unwrap(), default_params.clone());

        assert_eq!(
            parse([
                "maa",
                "fight",
                "1-7",
                "-m1",
                "-D30012=100",
                "--report-to-penguin",
                "--penguin-id=123456789",
                "--report-to-yituliu",
                "--yituliu-id=123456789",
                "--client-type=YoStarJP",
            ])
            .unwrap(),
            default_params.join(object!(
                "stage" => "1-7",
                "medicine" => 1,
                "drops" => object!("30012" => 100),
                "report_to_penguin" => true,
                "penguin_id" => "123456789",
                "report_to_yituliu" => true,
                "yituliu_id" => "123456789",
                "client_type" => "YoStarJP",
                "server" => "JP",
            ))
        );

        assert_eq!(
            parse([
                "maa",
                "fight",
                "1-7",
                "-m1",
                "-D30011=100",
                "-D30012=100",
                "--client-type=YoStarJP",
            ])
            .unwrap(),
            default_params.join(object!(
                "stage" => "1-7",
                "medicine" => 1,
                "drops" => object!(
                    "30011" => 100,
                    "30012" => 100,
                ),
                "client_type" => "YoStarJP",
                "server" => "JP",
            ))
        );

        assert_eq!(
            parse([
                "maa",
                "fight",
                "1-7",
                "--series=6",
                "--expiring-medicine=100",
                "--stone=10",
                "--dr-grandet",
            ])
            .unwrap(),
            object!(
                "stage" => "1-7",
                "expiring_medicine" => 100,
                "stone" => 10,
                "series" => 6,
                "DrGrandet" => true,
            )
        );

        assert!(parse(["maa", "fight", "1-7", "-D30012=100", "-D30011"]).is_err());

        assert_eq!(
            parse([
                "maa",
                "fight",
                "Annihilation",
                "--on-no-card=skip",
                "--on-no-record=fail",
                "--max-cards=2",
            ])
            .unwrap(),
            object!(
                "stage" => "Annihilation",
                "DrGrandet" => false,
                "on_no_card" => "skip",
                "on_no_record" => "fail",
                "max_cards" => 2,
            )
        );

        assert!(
            crate::command::Cli::try_parse_from([
                "maa",
                "fight",
                "Annihilation",
                "--on-no-card=explode"
            ])
            .is_err()
        );
    }
}
