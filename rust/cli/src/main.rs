use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, value_parser};
use serde_json::to_string_pretty;
use soundtouch_radio_core::{
    ApplyMode, Codec, Error, Host, Preset, Result, SLOTS, Speaker, clean_text, diagnose, discover,
    load_backup, save_backup, snapshot_json,
};

#[derive(Parser)]
#[command(
    name = "soundtouch-radio",
    version,
    about = "Manage physical SoundTouch presets over the LAN"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Browse the local network for SoundTouch advertisements.
    Discover {
        #[arg(long)]
        json: bool,
    },
    /// Inspect Bose hardware, firmware and STR status.
    Info {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        json: bool,
    },
    /// Manage the six physical keys.
    Presets {
        #[command(subcommand)]
        operation: PresetCommand,
    },
}

#[derive(Args)]
struct Target {
    /// Numeric private or link-local IPv4 address.
    #[arg(long)]
    host: Host,
}

#[derive(Subcommand)]
enum PresetCommand {
    /// List the six STR preset assignments.
    List {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        json: bool,
    },
    /// Save the current assignments to a new JSON file.
    Backup {
        #[command(flatten)]
        target: Target,
        #[arg(long, default_value = "presets.json")]
        file: PathBuf,
    },
    /// Set one key to a direct HTTP or HTTPS radio stream.
    Set {
        #[command(flatten)]
        target: Target,
        #[arg(long, value_parser = value_parser!(u8).range(1..=6))]
        slot: u8,
        #[arg(long)]
        name: String,
        #[arg(long)]
        url: String,
        #[arg(long, default_value = "Auto")]
        codec: Codec,
        /// Apply without an interactive prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Verify the six Bose key registrations.
    Verify {
        #[command(flatten)]
        target: Target,
    },
    /// Restore STR assignments from a Python-compatible JSON backup.
    Restore {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        file: PathBuf,
        /// Apply without an interactive prompt.
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<bool> {
    match command {
        Command::Discover { json } => {
            let result = discover();
            if json {
                print_json(&result)?;
            } else {
                for (ip, services) in &result.devices {
                    println!(
                        "{ip}  {}",
                        services.iter().cloned().collect::<Vec<_>>().join(", ")
                    );
                }
                if result.devices.is_empty() {
                    println!(
                        "No SoundTouch or STR mDNS advertisements found. Try connecting by IP."
                    );
                }
                for (service, error) in &result.errors {
                    eprintln!("{service}: {error}");
                }
            }
            Ok(!result.devices.is_empty())
        }
        Command::Info { target, json } => {
            let report = diagnose(target.host);
            if json {
                print_json(&report)?;
            } else {
                print_report(&report);
            }
            Ok(report.info.is_some())
        }
        Command::Presets { operation } => {
            presets(operation)?;
            Ok(true)
        }
    }
}

fn presets(command: PresetCommand) -> Result<()> {
    match command {
        PresetCommand::List { target, json } => {
            let presets = Speaker::connect(target.host)?.stored()?;
            if json {
                print!("{}", snapshot_json(&presets)?);
            } else {
                for slot in SLOTS {
                    let name = presets
                        .get(&slot)
                        .map(|p| clean_text(&p.name))
                        .unwrap_or_else(|| "(empty)".into());
                    println!("{slot}. {name}");
                }
            }
        }
        PresetCommand::Backup { target, file } => {
            let presets = Speaker::connect(target.host)?.stored()?;
            save_backup(&file, &presets)?;
            println!(
                "Backed up {} STR presets to {}",
                presets.len(),
                file.display()
            );
        }
        PresetCommand::Set {
            target,
            slot,
            name,
            url,
            codec,
            yes,
        } => {
            let wanted = Preset::radio(slot, &name, &url, codec, None)?;
            let speaker = Speaker::connect(target.host)?;
            let baseline = speaker.snapshot()?;
            let prompt = if let Some(previous) = baseline.stored.get(&slot) {
                format!(
                    "Replace slot {slot} ({}) with {}?",
                    clean_text(&previous.name),
                    wanted.name
                )
            } else if baseline.hardware_only(slot) {
                format!(
                    "Replace the Bose-only preset on physical slot {slot} with {}?",
                    wanted.name
                )
            } else {
                format!("Set physical slot {slot} to {}?", wanted.name)
            };
            confirm(&prompt, yes)?;
            speaker.set(&baseline, &wanted)?;
            println!("Verified slot {slot}: {}", wanted.name);
        }
        PresetCommand::Verify { target } => {
            let snapshot = Speaker::connect(target.host)?.snapshot()?;
            let mismatches = snapshot.mismatches();
            if !mismatches.is_empty() {
                let slots = mismatches
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(Error::Response(format!(
                    "Bose key registration differs in slots {slots}"
                )));
            }
            println!(
                "Verified all six Bose keys ({} assigned).",
                snapshot.stored.len()
            );
        }
        PresetCommand::Restore { target, file, yes } => {
            let desired = load_backup(&file)?;
            let speaker = Speaker::connect(target.host)?;
            let baseline = speaker.snapshot()?;
            if baseline.changes(&desired, ApplyMode::Restore)?.is_empty() {
                println!("The six keys already match the backup.");
                return Ok(());
            }
            confirm(
                &format!("Restore the six physical keys from {}?", file.display()),
                yes,
            )?;
            let folder = file
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let result = speaker.apply(&baseline, &desired, ApplyMode::Restore, folder)?;
            if let Some(backup) = result.recovery {
                println!("Previous STR presets saved to {}", backup.display());
            }
            println!("Verified all six restored keys.");
        }
    }
    Ok(())
}

fn confirm(prompt: &str, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    print!("{prompt} [y/N] ");
    io::stdout()
        .flush()
        .map_err(|e| Error::File("show confirmation".into(), e))?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| Error::File("read confirmation".into(), e))?;
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(Error::Invalid("cancelled".into()))
    }
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    let json =
        to_string_pretty(value).map_err(|_| Error::Response("could not encode report".into()))?;
    println!("{json}");
    Ok(())
}

fn print_report(report: &soundtouch_radio_core::Report) {
    if let Some(info) = &report.info {
        println!("Bose API reachable. Model: {}", info.model);
        println!(
            "Bose firmware (SCM component): {}",
            info.firmware.as_deref().unwrap_or("unknown")
        );
        println!(
            "Module type: {}",
            info.module_type.as_deref().unwrap_or("unknown")
        );
        println!("Variant: {}", info.variant.as_deref().unwrap_or("unknown"));
        println!(
            "Variant mode: {}",
            info.variant_mode.as_deref().unwrap_or("unknown")
        );
        for component in &info.components {
            println!(
                "  {}: {}",
                component.category.as_deref().unwrap_or("Unknown component"),
                component.software_version.as_deref().unwrap_or("unknown")
            );
        }
    } else {
        println!("Bose /info could not identify a speaker.");
    }
    if let Some(presets) = &report.presets {
        println!("Physical presets reported by Bose");
        for slot in SLOTS {
            if let Some(preset) = presets.iter().find(|key| key.slot == slot) {
                println!(
                    "  {slot}. {} [{}]",
                    preset.name.as_deref().unwrap_or("(unnamed)"),
                    preset.source.as_deref().unwrap_or("unknown source")
                );
            } else {
                println!("  {slot}. (empty)");
            }
        }
    }
    if let Some(agent) = &report.str_agent {
        println!(
            "STR agent detected on port {}, version {}, build {}",
            agent.port,
            agent.version,
            agent.build.as_deref().unwrap_or("unknown")
        );
    } else {
        println!("STR agent not detected. Installation status remains unknown.");
    }
    for (endpoint, error) in &report.errors {
        println!("  {endpoint}: {error}");
    }
}
