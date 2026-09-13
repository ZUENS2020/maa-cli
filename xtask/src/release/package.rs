use std::{cmp::Ordering, collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};
use maa_version::{
    VersionManifest,
    cli::{Asset, Details},
};
use semver::Version;

use super::{Channel, archive, archive::ArchiveFormat};
use crate::env;

pub fn run() -> Result<()> {
    let channel: Channel = env::var("CHANNEL")?.parse()?;
    let version_str = env::var("VERSION")?;
    let tag = env::var("TAG")?;
    let commit = env::var("COMMIT")?;

    let version = Version::parse(&version_str)
        .with_context(|| format!("Failed to parse version: {}", version_str))?;
    let bundle = Path::new("release-bundle");
    fs::create_dir_all(bundle.join("version")).context("Failed to create release bundle")?;

    // Determine which version files to update
    let version_files = channel.version_files();

    // Read existing manifests to preserve asset data structure
    let mut manifests: Vec<VersionManifest<Details>> = version_files
        .iter()
        .map(|file| {
            let manifest = read_or_create_manifest(file)?;
            Ok(manifest)
        })
        .collect::<Result<Vec<_>>>()?;

    // Update target-independent version info
    for manifest in &mut manifests {
        manifest.version = version.clone();
        manifest.details.tag = tag.clone();
        manifest.details.commit = commit.clone();
    }

    // Process each artifact directory
    let entries = fs::read_dir(".")
        .context("Failed to read current directory")?
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|s| s.starts_with("maa_cli-"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    for entry in entries {
        let dir_name = entry.file_name();
        let dir_str = dir_name.to_str().context("Invalid directory name")?;
        let target = &dir_str[8..]; // Remove "maa_cli-" prefix

        println!("Processing target: {target}");

        // Extract tar file
        let tar_file = format!(
            "{dir_str}/{}.tar",
            target.strip_suffix("-winget").unwrap_or(target)
        );
        archive::extract_tar(&tar_file, dir_str)?;

        // Copy licenses.md
        fs::copy("licenses.md", format!("{dir_str}/licenses.md"))
            .context("Failed to copy licenses.md")?;

        // Create archive based on platform and get checksum
        let (archive_name, checksum_hash) = create_archive(bundle, target, &version_str, dir_str)?;
        let size = fs::metadata(bundle.join(&archive_name))
            .context("Failed to get file metadata")?
            .len();

        println!("  Archive: {archive_name}");
        println!("  Size: {size} bytes");
        println!("  SHA256: {checksum_hash}");

        // No need to update manifests for winget
        if target.ends_with("winget") {
            continue;
        }

        // Update version files with target-specific info
        let asset = Asset {
            name: archive_name,
            size,
            sha256sum: checksum_hash,
        };

        for manifest in &mut manifests {
            manifest
                .details
                .assets
                .insert(target.to_string(), asset.clone());
        }
    }

    // The published index is read-only input; all outputs belong to the bundle.
    for (file, manifest) in version_files.iter().zip(&manifests) {
        write_manifest(bundle.join(file), manifest)?;
        write_shell_format(bundle.join(file), manifest)?;
    }

    println!("Release bundle created successfully");
    Ok(())
}

/// Apply a published bundle to a fresh checkout of the version branch.
pub fn update_index() -> Result<()> {
    let channel = env::var("CHANNEL")?.parse()?;
    let version = Version::parse(&env::var("VERSION")?)?;
    let tag = env::var("TAG")?;
    let commit = env::var("COMMIT")?;

    apply_index(
        Path::new("release-bundle"),
        Path::new("."),
        channel,
        &version,
        &tag,
        &commit,
    )
}

/// Reject stale publication plans before changing the release or its assets.
pub fn check_publication() -> Result<()> {
    let channel: Channel = env::var("CHANNEL")?.parse()?;
    let version = Version::parse(&env::var("VERSION")?)?;
    let tag = env::var("TAG")?;
    let commit = env::var("COMMIT")?;
    let file = channel.version_file();
    let current: VersionManifest<Details> =
        serde_json::from_slice(&fs::read(&file).with_context(|| format!("Failed to read {file}"))?)
            .with_context(|| format!("Failed to parse {file}"))?;
    validate_publication(&current, &version, &tag, &commit)
}

fn matches_identity(
    manifest: &VersionManifest<Details>,
    version: &Version,
    tag: &str,
    commit: &str,
) -> bool {
    manifest.version == *version && manifest.details.tag == tag && manifest.details.commit == commit
}

fn validate_publication(
    current: &VersionManifest<Details>,
    version: &Version,
    tag: &str,
    commit: &str,
) -> Result<()> {
    match current.version.cmp_precedence(version) {
        Ordering::Greater => anyhow::bail!(
            "Cannot publish {version}: channel already contains newer version {}",
            current.version
        ),
        Ordering::Equal => ensure!(
            matches_identity(current, version, tag, commit),
            "Conflicting publication identity at version {version}"
        ),
        Ordering::Less => {}
    }
    Ok(())
}

fn apply_index(
    bundle: &Path,
    checkout: &Path,
    channel: Channel,
    version: &Version,
    tag: &str,
    commit: &str,
) -> Result<()> {
    let mut updates = Vec::new();
    // Validate every source and destination before changing any file. A later
    // channel conflict must not leave earlier channels partially updated.
    for file in channel.version_files() {
        let source = bundle.join(file);
        let incoming: VersionManifest<Details> = serde_json::from_slice(
            &fs::read(&source).with_context(|| format!("Failed to read {}", source.display()))?,
        )
        .with_context(|| format!("Failed to parse {}", source.display()))?;
        ensure!(
            matches_identity(&incoming, version, tag, commit),
            "Release identity mismatch in {}",
            source.display()
        );

        let destination = checkout.join(file);
        match fs::read(&destination) {
            Ok(content) => {
                let current: VersionManifest<Details> = serde_json::from_slice(&content)
                    .with_context(|| format!("Failed to parse {}", destination.display()))?;
                // Build metadata has no chronological ordering. Equal alpha
                // counters with different commit metadata are conflicts too.
                match current.version.cmp_precedence(&incoming.version) {
                    Ordering::Greater => {
                        println!(
                            "Preserving {} at newer version {}",
                            destination.display(),
                            current.version
                        );
                        continue;
                    }
                    Ordering::Equal => ensure!(
                        serde_json::to_value(&current)? == serde_json::to_value(&incoming)?,
                        "Conflicting release at version {version} in {}",
                        destination.display()
                    ),
                    Ordering::Less => {}
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to read {}", destination.display()));
            }
        }
        updates.push((destination, incoming));
    }

    for (destination, manifest) in updates {
        write_manifest(&destination, &manifest)?;
        write_shell_format(&destination, &manifest)?;
    }
    Ok(())
}

fn read_or_create_manifest(file: &str) -> Result<VersionManifest<Details>> {
    if fs::metadata(file).is_ok() {
        let content =
            fs::read_to_string(file).with_context(|| format!("Failed to read {}", file))?;

        serde_json::from_str(&content).with_context(|| format!("Failed to parse {}", file))
    } else {
        // Create a new manifest with empty data
        Ok(VersionManifest {
            version: Version::new(0, 0, 0),
            details: Details {
                tag: String::new(),
                commit: String::new(),
                assets: BTreeMap::new(),
            },
        })
    }
}

fn write_manifest(file: impl AsRef<Path>, manifest: &VersionManifest<Details>) -> Result<()> {
    let file = file.as_ref();
    let content = serde_json::to_string_pretty(manifest).context("Failed to serialize manifest")?;

    fs::write(file, content).with_context(|| format!("Failed to write {}", file.display()))
}

fn write_shell_format(file: impl AsRef<Path>, manifest: &VersionManifest<Details>) -> Result<()> {
    // Write a shell-friendly .txt format alongside the JSON
    let txt_path = file.as_ref().with_extension("txt");

    use std::io::Write;

    let mut txt_file = std::fs::File::create(&txt_path)
        .with_context(|| format!("Failed to create {}", txt_path.display()))?;

    writeln!(txt_file, "VERSION={}", manifest.version)?;
    writeln!(txt_file, "TAG={}", manifest.details.tag)?;
    writeln!(txt_file, "COMMIT={}", manifest.details.commit)?;
    writeln!(txt_file)?;

    // Write assets in a shell-friendly format
    for (target, asset) in &manifest.details.assets {
        let target_upper = target.to_uppercase().replace('-', "_");
        writeln!(txt_file, "# {target}")?;
        writeln!(txt_file, "{target_upper}_NAME={}", asset.name)?;
        writeln!(txt_file, "{target_upper}_SIZE={}", asset.size)?;
        writeln!(txt_file, "{target_upper}_SHA256={}", asset.sha256sum)?;
        writeln!(txt_file)?;
    }

    txt_file
        .sync_all()
        .with_context(|| format!("Failed to sync {}", txt_path.display()))?;

    Ok(())
}

fn create_archive(
    output: &Path,
    target: &str,
    version: &str,
    dir: &str,
) -> Result<(String, String)> {
    // Determine archive format and binary name based on target
    // Use tar.gz for Unix-like systems (Linux, macOS) and zip for Windows
    let (format, bin_name) = if target.contains("-windows-msvc-winget") {
        (ArchiveFormat::Zip, "maa-cli.exe")
    } else if target.contains("-windows-msvc") {
        (ArchiveFormat::Zip, "maa.exe")
    } else if target.contains("-linux-") || target.ends_with("-apple-darwin") {
        (ArchiveFormat::TarGz, "maa")
    } else {
        anyhow::bail!("Unknown target: {target}")
    };

    let ext = format.extension();
    let archive_name = format!("maa_cli-v{version}-{target}.{ext}");

    let binary = format!("{dir}/{bin_name}");
    let licenses = format!("{dir}/licenses.md");

    let checksum_hash = format.create(output.join(&archive_name), &[
        (binary.as_str(), bin_name),
        (licenses.as_str(), "licenses.md"),
    ])?;

    Ok((archive_name, checksum_hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str) -> Result<VersionManifest<Details>> {
        Ok(VersionManifest {
            version: Version::parse(version)?,
            details: Details {
                tag: format!("v{version}"),
                commit: "release-commit".into(),
                assets: BTreeMap::from([("aarch64-apple-darwin".into(), Asset {
                    name: format!("maa_cli-v{version}-aarch64-apple-darwin.tar.gz"),
                    size: 123,
                    sha256sum: "a".repeat(64),
                })]),
            },
        })
    }

    fn setup(channel: Channel, incoming: &VersionManifest<Details>) -> Result<tempfile::TempDir> {
        let directory = tempfile::tempdir()?;
        fs::create_dir_all(directory.path().join("release-bundle/version"))?;
        fs::create_dir(directory.path().join("version"))?;
        for file in channel.version_files() {
            write_manifest(directory.path().join("release-bundle").join(file), incoming)?;
        }
        Ok(directory)
    }

    fn apply(root: &Path, channel: Channel, incoming: &VersionManifest<Details>) -> Result<()> {
        apply_index(
            &root.join("release-bundle"),
            root,
            channel,
            &incoming.version,
            &incoming.details.tag,
            &incoming.details.commit,
        )
    }

    #[test]
    fn index_initial_apply_and_replay_repair_text() -> Result<()> {
        let incoming = manifest("0.8.0")?;
        let directory = setup(Channel::Stable, &incoming)?;
        let root = directory.path();
        fs::write(root.join("version/unrelated.txt"), "preserve me")?;

        apply(root, Channel::Stable, &incoming)?;
        let first = fs::read(root.join("version/stable.json"))?;
        fs::write(
            root.join("version/stable.txt"),
            "interrupted previous write",
        )?;
        apply(root, Channel::Stable, &incoming)?;

        assert_eq!(fs::read(root.join("version/stable.json"))?, first);
        for file in Channel::Stable.version_files() {
            let actual: serde_json::Value = serde_json::from_slice(&fs::read(root.join(file))?)?;
            assert_eq!(actual, serde_json::to_value(&incoming)?);
            let text = fs::read_to_string(root.join(file).with_extension("txt"))?;
            assert!(text.starts_with("VERSION=0.8.0\nTAG=v0.8.0\nCOMMIT=release-commit\n"));
            assert!(text.contains("AARCH64_APPLE_DARWIN_SHA256="));
        }
        assert_eq!(
            fs::read_to_string(root.join("version/unrelated.txt"))?,
            "preserve me"
        );
        Ok(())
    }

    #[test]
    fn index_rejects_identity_and_asset_conflicts_before_any_write() -> Result<()> {
        for field in ["commit", "tag", "checksum"] {
            let incoming = manifest("0.8.0")?;
            let directory = setup(Channel::Stable, &incoming)?;
            let root = directory.path();
            let mut conflicting = manifest("0.8.0")?;
            match field {
                "commit" => conflicting.details.commit = "different-commit".into(),
                "tag" => conflicting.details.tag = "different-tag".into(),
                _ => {
                    for asset in conflicting.details.assets.values_mut() {
                        asset.sha256sum = "b".repeat(64);
                    }
                }
            }
            write_manifest(root.join("version/stable.json"), &conflicting)?;
            let before = fs::read(root.join("version/stable.json"))?;

            assert!(apply(root, Channel::Stable, &incoming).is_err(), "{field}");
            assert!(!root.join("version/alpha.json").exists());
            assert!(!root.join("version/beta.json").exists());
            assert_eq!(fs::read(root.join("version/stable.json"))?, before);
        }
        Ok(())
    }

    #[test]
    fn index_preserves_newer_channels_while_repairing_stable() -> Result<()> {
        let incoming = manifest("0.8.0")?;
        let directory = setup(Channel::Stable, &incoming)?;
        let root = directory.path();
        for (file, version) in [
            ("version/alpha.json", "0.9.0-beta.1.alpha.1+sha.new"),
            ("version/beta.json", "0.9.0-beta.1"),
        ] {
            let newer = manifest(version)?;
            write_manifest(root.join(file), &newer)?;
            write_shell_format(root.join(file), &newer)?;
        }
        let alpha_before = fs::read(root.join("version/alpha.json"))?;
        let beta_before = fs::read(root.join("version/beta.txt"))?;
        apply(root, Channel::Stable, &incoming)?;
        assert_eq!(fs::read(root.join("version/alpha.json"))?, alpha_before);
        assert_eq!(fs::read(root.join("version/beta.txt"))?, beta_before);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(
                root.join("version/stable.json")
            )?)?,
            serde_json::to_value(&incoming)?
        );
        Ok(())
    }

    #[test]
    fn index_rejects_equal_alpha_counters_with_different_build_metadata() -> Result<()> {
        let incoming = manifest("0.8.0-alpha.1+sha.zzz")?;
        let directory = setup(Channel::Alpha, &incoming)?;
        let destination = directory.path().join("version/alpha.json");
        write_manifest(&destination, &manifest("0.8.0-alpha.1+sha.aaa")?)?;
        let before = fs::read(&destination)?;
        assert!(apply(directory.path(), Channel::Alpha, &incoming).is_err());
        assert_eq!(fs::read(destination)?, before);
        Ok(())
    }

    #[test]
    fn index_validates_all_bundle_inputs_before_writing() -> Result<()> {
        for invalid in ["not json", &serde_json::to_string(&manifest("0.9.0")?)?] {
            let incoming = manifest("0.8.0")?;
            let directory = setup(Channel::Stable, &incoming)?;
            let root = directory.path();
            fs::write(root.join("release-bundle/version/stable.json"), invalid)?;
            assert!(apply(root, Channel::Stable, &incoming).is_err());
            assert_eq!(fs::read_dir(root.join("version"))?.count(), 0);
        }
        Ok(())
    }

    #[test]
    fn publication_rejects_stale_or_conflicting_plans_but_allows_replay() -> Result<()> {
        let current = manifest("0.8.0-alpha.2+sha.current")?;
        let tag = &current.details.tag;
        let commit = &current.details.commit;
        assert!(validate_publication(&current, &current.version, tag, commit).is_ok());
        assert!(
            validate_publication(
                &current,
                &Version::parse("0.8.0-alpha.3+sha.new")?,
                tag,
                "new-commit"
            )
            .is_ok()
        );
        for version in ["0.8.0-alpha.1+sha.old", "0.8.0-alpha.2+sha.zzz"] {
            assert!(
                validate_publication(&current, &Version::parse(version)?, tag, commit).is_err()
            );
        }
        assert!(validate_publication(&current, &current.version, tag, "different").is_err());
        assert!(validate_publication(&current, &current.version, "different", commit).is_err());
        Ok(())
    }
}
