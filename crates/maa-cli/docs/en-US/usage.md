# Usage

maa-cli's main functionality is to automate Arknights game tasks by calling MaaCore. Additionally, for convenience, maa-cli also provides functions to manage MaaCore.

## Manage MaaCore

maa-cli can install and update MaaCore and resources by running the following commands:

```bash
maa install # Install MaaCore and resources
maa update # Update MaaCore and resources
```

## Update maa-cli Itself

maa-cli can update itself by running the following command:

```bash
maa self update
```

**Note**: Users who installed maa-cli via a package manager should use the package manager to update maa-cli. This command will not work for those users.

## Initialize Configuration

Once MaaCore is installed, you can typically run tasks directly without additional configuration. However, the default configuration may not be suitable for all users, so you can initialize the configuration with:

```bash
maa init
```

With this command, you can interactively configure [MaaCore-related settings][config-core].

## Run Tasks

After installing and configuring MaaCore, you can run tasks. maa-cli supports two types of tasks: predefined tasks and custom tasks.

### Predefined Tasks

For common tasks, maa-cli provides several predefined options:

- `maa startup [client]`: Start the game and enter the main interface. `[client]` is the client type; leave empty to not start any game client.
- `maa closedown [client]`: Close the game client. `[client]` is the client type, defaulting to `Official`.
- `maa fight [stage]`: Run a combat task. `[stage]` is the stage name like `1-7`; leave empty to select the last or current stage. Annihilation accepts `--on-no-card` / `--on-no-record` / `--max-cards` (newer MaaCore; older cores ignore these keys).
- `maa copilot <maa_uri>...`: Auto-run copilot tasks. `<maa_uri>` is the task URI, multiple URIs will execute in sequence. `maa_uri` can be `maa://1234` or a local file path like `./1234.json`.
- `maa sscopilot <maa_uri>`: Auto-run Stationary Security Service tasks. `<maa_uri>` is the task URI.
- `maa roguelike <theme>`: Auto-run Integrated Strategy. `<theme>` is the theme, with options including `Phantom`, `Mizuki`, `Sami`, `Sarkaz`, and `JieGarden`.
- `maa reclamation <theme>`: Auto-run Reclamation Algorithm. `<theme>` is the theme, currently only `Tales` is available.
- `maa status [--fields sanity,annihilation,...] [--startup] [--json]`: Read-only snapshot of sanity, currency, annihilation progress, and similar fields. Does not fight, claim rewards, or shift infrastructure. Requires a MaaCore that implements the Status task; older cores mark the task unavailable instead of crashing.

`--json` prints only the `GameStatus` snapshot:

```bash
maa status --fields sanity,annihilation --startup --json
```

```json
{
  "sanity": { "current": 80, "max": 135 },
  "annihilation": { "weekly_progress": 370, "weekly_cap": 1800 }
}
```

These tasks accept various parameters. You can view the specific parameters with `maa <task> --help`.

For example, if you want to open the game, use 3 sanity potions to farm BB-7, and then close the game, you can run:

```bash
maa startup Official && maa fight BB-7 -m 3 && maa closedown
```

### Custom Tasks

Due to MAA's support for numerous tasks, maa-cli cannot provide predefined options for everything. Additionally, you may need to run multiple tasks as in the example above. To address this, maa-cli offers custom task functionality. Custom tasks can combine different tasks, providing finer control over parameters and execution order. They also support conditional execution based on specific criteria, automating your daily routines. Custom tasks are defined via configuration files—see the [Custom Task Documentation][custom-task] for details on location and format. After creating a configuration file, run your custom task with `maa run <task>`, where `<task>` is the filename without extension.

### Task Summary

Both predefined and custom tasks output summary information upon completion, including each subtask's runtime (start time, end time, duration). For certain tasks, result summaries include:

- `fight` task: Stage name, number of runs, sanity potions used, and drop statistics
- `infrast`: Operators assigned to each facility, including product types for factories and trading posts
- `recruit`: Tags, star ratings, and status for each recruitment, plus total recruitment count
- `roguelike`: Exploration count, investment count

If you don't want task summaries, disable them with the `--no-summary` parameter.

### Structured Run Report

Unattended scripts can pass `--report <path>` to write a JSON report when the run finishes (including on task failure or interruption, as long as the process can still write the file). Default logs and the human-readable summary stay unchanged.

```bash
maa run daily --batch --report /tmp/maa-report.json
maa run daily --batch --output json
maa run daily --batch --report /tmp/maa-report.json --output json --strict-exit
```

`--output json` prints the same report to stdout and omits the human-readable summary. The JSON Schema is [`run-report.schema.json`][run-report-schema].

The report includes start/finish timestamps, cli/core/resource versions, and per-task `name`, `type`, `status` (`succeeded` / `failed` / `skipped` / `stopped` / `not_run`), duration, and a failure/skip reason. Per-type details are aggregated from callbacks MaaCore already emits:

- `fight`: stage, times fought, sanity at start and end, drops keyed by item id
- `annihilation`: weekly `progress_before` / `progress_after` / `weekly_cap` / `cards_used`. Stock cores fill this from `StageDrops.annihilation_weekly_process`; sanity under 25 with the cap not reached is recorded as `SANITY_NOT_ENOUGH`. `WEEKLY_CAP_REACHED` and similar codes come from a newer core
- `recruit`: recruits started and their tags
- `infrast`: rooms handled
- `award.checked`: reward entries that were actually inspected; `award.claimed`: rewards that were actually received (for example `ReceiveAward` / `Mail_ReceiveAll`)

`skipped` is distinct from `succeeded`: for example a `[[tasks.variants]]` condition that did not match, or a core skip code such as `WEEKLY_CAP_REACHED` / `NO_PRTS_CARD` / `NO_FULL_RECORD` / `SANITY_NOT_ENOUGH`. Tasks that never started are `not_run`. Unknown or future callback fields are stored on each task's `details` object. `--report` writes the file with mode `0644`.

`--dry-run` also warns about unknown parameter keys for known task types (for example a typo like `mial` instead of `mail`). Free-form `Custom` tasks are not checked.

### Exit Codes

Default behavior is unchanged: `0` on success, `1` on any failure. With `--strict-exit` the process uses distinct codes:

| Exit code | Meaning |
| --- | --- |
| 0 | All tasks succeeded or were skipped (including dry-run) |
| 2 | One or more tasks failed or stopped after a successful connection |
| 3 | Startup / connection / initialization failure |
| 130 | Interrupted by a termination signal |

[run-report-schema]: ../../schemas/run-report.schema.json

### Task Logging

maa-cli outputs logs with the following levels (low to high): `Error`, `Warn`, `Info`, `Debug`, and `Trace`. The default level is `Warn`. Set the log level via the `MAA_LOG` environment variable (e.g., `MAA_LOG=debug`) or use `-v` to increase and `-q` to decrease the level.

By default, logs go to standard error (stderr). The `--log-file` option can redirect logs to a file at `$(maa dir log)/YYYY/MM/DD/HH:MM:SS.log`, where `$(maa dir log)` is the log directory obtainable via `maa dir log`. You can also specify a custom log path with `--log-file=path/to/log`.

All logs normally include a timestamp and level prefix. The `MAA_LOG_PREFIX` environment variable controls this behavior: `Always` always includes prefixes, `Auto` includes prefixes in log files but not in stderr output, and `Never` omits prefixes even in log files.

### Other Subcommands

Besides the above commands, maa-cli provides additional subcommands:

- `maa list`: List all available tasks
- `maa dir <dir>`: Get a specific directory path, such as `maa dir config` for the configuration directory
- `maa version`: Get version information for `maa-cli` and `MaaCore`
- `maa convert <input> [output]`: Convert between `JSON`, `YAML`, or `TOML` format files
- `maa complete <shell>`: Generate auto-completion scripts
- `maa activity [client]`: Get current in-game activity information, with `[client]` defaulting to `Official`
- `maa cleanup`: Clean `maa-cli` and `MaaCore` caches
- `maa import <file> [-t <type>]`: Import a configuration file, with `-t` specifying the type (e.g., `cli`, `profile`, `infrast`)

For more command information, use `maa help`. For specific command details, use `maa help <command>`.

[config-core]: config.md#maacore-related-configurations
[custom-task]: config.md#custom-tasks
