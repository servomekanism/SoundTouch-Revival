//! Local network access and preset transactions shared by both Rust clients.

mod backup;
mod discovery;
mod model;
mod speaker;
mod transport;

pub use backup::{load_backup, save_backup, snapshot_json};
pub use discovery::{Discovery, discover};
pub use model::{
    Agent, Codec, HardwarePreset, Host, Preset, Presets, Report, SLOTS, SpeakerInfo, clean_text,
    validate_name, validate_url,
};
pub use speaker::{ApplyMode, ApplyOutcome, Change, Snapshot, Speaker, diagnose};

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{operation} failed: {reason}")]
    Network { operation: String, reason: String },
    #[error("{operation} returned HTTP {status}{detail}")]
    Http {
        operation: String,
        status: u16,
        detail: String,
        code: Option<String>,
        slot: Option<u8>,
    },
    #[error("{0}")]
    Response(String),
    #[error("speaker is reachable, but STR could not be identified ({0})")]
    AgentUnavailable(String),
    #[error("could not identify the speaker ({0})")]
    SpeakerUnavailable(String),
    #[error("preset read-back differs in slots {0}")]
    Readback(String),
    #[error("STR saved the changes, but Bose key registration differs in slots {0}")]
    Hardware(String),
    #[error("{0}: {1}")]
    File(String, #[source] std::io::Error),
    #[error("{source}. Check the current presets before retrying. Previous mappings: {backup}")]
    Apply { source: Box<Error>, backup: PathBuf },
}

pub type Result<T> = std::result::Result<T, Error>;
