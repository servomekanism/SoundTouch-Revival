use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Preset, Presets, Result, SLOTS};

const FORMAT: u32 = 1;
const MAX_BACKUP: u64 = 6 * 1024 * 1024 + 4096;

#[derive(Serialize)]
struct Export<'a> {
    format: u32,
    presets: Vec<&'a Preset>,
}

#[derive(Deserialize)]
struct Import {
    format: u32,
    presets: Vec<Preset>,
}

pub fn snapshot_json(presets: &Presets) -> Result<String> {
    let export = Export {
        format: FORMAT,
        presets: presets.values().collect(),
    };
    let mut json = serde_json::to_string_pretty(&export)
        .map_err(|_| Error::Invalid("could not encode the preset backup".into()))?;
    json.push('\n');
    if json.len() as u64 > MAX_BACKUP {
        return Err(Error::Invalid("backup exceeds the size limit".into()));
    }
    Ok(json)
}

fn io_error(operation: &str, path: &Path, error: std::io::Error) -> Error {
    Error::File(format!("{operation} {}", path.display()), error)
}

pub fn load_backup(path: &Path) -> Result<Presets> {
    let file = File::open(path).map_err(|e| io_error("open", path, e))?;
    let mut bytes = Vec::new();
    file.take(MAX_BACKUP + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error("read", path, e))?;
    if bytes.len() as u64 > MAX_BACKUP {
        return Err(Error::Invalid("backup exceeds the size limit".into()));
    }
    let backup: Import = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Invalid("backup is not valid preset JSON".into()))?;
    if backup.format != FORMAT {
        return Err(Error::Invalid("unsupported backup format".into()));
    }
    let mut presets = Presets::new();
    for preset in backup.presets {
        if !SLOTS.contains(&preset.slot) || presets.insert(preset.slot, preset).is_some() {
            return Err(Error::Invalid(
                "backup has an invalid or duplicate physical slot".into(),
            ));
        }
    }
    Ok(presets)
}

pub fn save_backup(path: &Path, presets: &Presets) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file = write_private(parent, ".soundtouch-", presets)?;
    file.persist_noclobber(path)
        .map_err(|err| io_error("save", path, err.error))?;
    Ok(())
}

pub(crate) fn recovery_backup(folder: &Path, presets: &Presets) -> Result<PathBuf> {
    let file = write_private(folder, "presets.before-restore-", presets)?;
    let (_, path) = file
        .keep()
        .map_err(|err| io_error("keep recovery backup in", folder, err.error))?;
    Ok(path)
}

fn write_private(
    folder: &Path,
    prefix: &str,
    presets: &Presets,
) -> Result<tempfile::NamedTempFile> {
    let json = snapshot_json(presets)?;
    let mut file = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(".json")
        .tempfile_in(folder)
        .map_err(|err| io_error("create backup in", folder, err))?;
    file.write_all(json.as_bytes())
        .and_then(|()| file.as_file().sync_all())
        .map_err(|err| io_error("write backup in", folder, err))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_compatible_backup_round_trip_keeps_unknown_metadata() {
        let folder = tempfile::tempdir().unwrap();
        let input = folder.path().join("input.json");
        let output = folder.path().join("output.json");
        std::fs::write(&input, r#"{"format":1,"presets":[{"slot":2,"name":"Example","type":"spotify","stream_url":"","uri":"spotify:playlist:example","account":"test-account","future":42}]}"#).unwrap();
        let expected = load_backup(&input).unwrap();
        save_backup(&output, &expected).unwrap();
        assert_eq!(load_backup(&output).unwrap(), expected);
        assert!(snapshot_json(&expected).unwrap().contains("\"future\": 42"));
        assert!(save_backup(&output, &Presets::new()).is_err());
        assert_eq!(load_backup(&output).unwrap(), expected);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                output.metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
