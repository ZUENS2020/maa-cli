use std::str::FromStr;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde::Deserialize;

pub mod archive;
mod meta;
mod package;
mod selection;

/// Release channel for maa-cli.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// Stable release
    Stable,
    /// Beta pre-release
    Beta,
    /// Alpha pre-release (nightly)
    Alpha,
}

impl FromStr for Channel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "stable" => Ok(Channel::Stable),
            "beta" => Ok(Channel::Beta),
            "alpha" => Ok(Channel::Alpha),
            _ => bail!("Unknown channel: {s}"),
        }
    }
}

impl Channel {
    /// Get the channel name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
            Channel::Alpha => "alpha",
        }
    }

    /// Get the version file name for this channel.
    pub fn version_file(&self) -> String {
        let channel = self.as_str();
        format!("version/{channel}.json")
    }

    /// Get the list of version files to update for this channel.
    ///
    /// - Alpha: updates alpha.json
    /// - Beta: updates alpha.json and beta.json
    /// - Stable: updates all three (alpha.json, beta.json, stable.json)
    pub fn version_files(&self) -> &[&'static str] {
        match self {
            Channel::Alpha => &["version/alpha.json"],
            Channel::Beta => &["version/alpha.json", "version/beta.json"],
            Channel::Stable => &[
                "version/alpha.json",
                "version/beta.json",
                "version/stable.json",
            ],
        }
    }
}

#[derive(Subcommand)]
pub enum ReleaseCommands {
    /// Parse version and determine release metadata
    Meta(ReleaseVersionOptions),
    /// Select a stable version for a release PR or pre-release base
    SelectVersion(SelectVersionOptions),
    /// Update version.json files with release information
    Package,
    /// Update the version branch from the packaged release manifests
    Index,
    /// Check that the prepared release does not supersede a newer publication
    CheckPublication,
}

#[derive(Args)]
pub struct ReleaseVersionOptions {
    /// Channel to build or publish
    #[arg(long)]
    channel: Channel,
    /// Publish an optimized release build
    #[arg(long)]
    publish: bool,
    /// Version selection for pre-releases; stable uses the Cargo package version
    #[arg(long, default_value = "auto")]
    version: selection::VersionSelection,
    /// Full SHA of the commit selected by the triggering workflow
    #[arg(long)]
    commit: String,
}

#[derive(Args)]
pub struct SelectVersionOptions {
    /// auto, patch, minor, major, or an explicit stable X.Y.Z
    #[arg(long, default_value = "auto")]
    version: selection::VersionSelection,
}

pub fn run(command: ReleaseCommands) -> Result<()> {
    match command {
        ReleaseCommands::Meta(options) => meta::run(options),
        ReleaseCommands::SelectVersion(options) => selection::run(options),
        ReleaseCommands::Package => package::run(),
        ReleaseCommands::Index => package::update_index(),
        ReleaseCommands::CheckPublication => package::check_publication(),
    }
}
