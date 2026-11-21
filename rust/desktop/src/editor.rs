use std::collections::BTreeMap;

use soundtouch_radio_core::{Codec, Preset, Presets, SLOTS, Snapshot};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Edit {
    #[default]
    Keep,
    Replace,
    Clear,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Fields {
    pub name: String,
    pub url: String,
    pub codec: Codec,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Row {
    pub fields: Fields,
    pub edit: Edit,
    pub bose_only: bool,
}

#[derive(Default)]
pub struct Drafts {
    original: [Row; 6],
    current: [Row; 6],
}

impl Drafts {
    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        let original = std::array::from_fn(|index| {
            let slot = index as u8 + 1;
            if let Some(preset) = snapshot.stored.get(&slot) {
                return Row {
                    fields: Fields {
                        name: preset.name.clone(),
                        url: preset.stream_url().unwrap_or_default().into(),
                        codec: preset
                            .codec_label()
                            .and_then(|label| label.parse().ok())
                            .unwrap_or_default(),
                    },
                    ..Row::default()
                };
            }
            if snapshot.hardware_only(slot) {
                return Row {
                    fields: Fields {
                        name: snapshot.hardware[&slot]
                            .name
                            .clone()
                            .unwrap_or_else(|| "Bose preset".into()),
                        ..Fields::default()
                    },
                    bose_only: true,
                    ..Row::default()
                };
            }
            Row::default()
        });
        Self {
            current: original.clone(),
            original,
        }
    }

    pub fn at(&self, slot: u8) -> &Row {
        &self.current[usize::from(slot - 1)]
    }

    pub fn dirty(&self) -> bool {
        self.current.iter().any(|row| row.edit != Edit::Keep)
    }

    pub fn discard(&mut self) {
        self.current = self.original.clone();
    }

    pub fn undo(&mut self, slot: u8) {
        let index = usize::from(slot - 1);
        self.current[index] = self.original[index].clone();
    }

    fn change(&mut self, slot: u8, edit: impl FnOnce(&mut Fields)) {
        let index = usize::from(slot - 1);
        let row = &mut self.current[index];
        edit(&mut row.fields);
        row.edit = if row.fields == self.original[index].fields {
            Edit::Keep
        } else {
            Edit::Replace
        };
    }

    pub fn name(&mut self, slot: u8, value: String) {
        self.change(slot, |fields| fields.name = value);
    }

    pub fn url(&mut self, slot: u8, value: String) {
        self.change(slot, |fields| fields.url = value);
    }

    pub fn codec(&mut self, slot: u8, value: Codec) {
        self.change(slot, |fields| fields.codec = value);
    }

    pub fn clear(&mut self, slot: u8) {
        let index = usize::from(slot - 1);
        self.current[index] = Row {
            edit: if self.original[index].fields == Fields::default() {
                Edit::Keep
            } else {
                Edit::Clear
            },
            ..Row::default()
        };
    }

    pub fn validate(
        &self,
        snapshot: &Snapshot,
    ) -> std::result::Result<Presets, BTreeMap<u8, String>> {
        let mut desired = snapshot.stored.clone();
        let mut errors = BTreeMap::new();
        for slot in SLOTS {
            let row = self.at(slot);
            match row.edit {
                Edit::Keep => {}
                Edit::Clear => {
                    desired.remove(&slot);
                }
                Edit::Replace => match Preset::radio(
                    slot,
                    &row.fields.name,
                    row.fields.url.trim(),
                    row.fields.codec,
                    snapshot.stored.get(&slot),
                ) {
                    Ok(preset) => {
                        desired.insert(slot, preset);
                    }
                    Err(error) => {
                        errors.insert(slot, error.to_string());
                    }
                },
            }
        }
        let mut urls = BTreeMap::new();
        for (&slot, preset) in &desired {
            if let Some(url) = preset.stream_url().filter(|url| !url.is_empty())
                && let Some(other) = urls.insert(url, slot)
                && (self.at(slot).edit != Edit::Keep || self.at(other).edit != Edit::Keep)
            {
                errors.insert(
                    slot,
                    format!("This station is also assigned to button {other}."),
                );
                errors.insert(
                    other,
                    format!("This station is also assigned to button {slot}."),
                );
            }
        }
        if errors.is_empty() {
            Ok(desired)
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initial() -> Snapshot {
        let mut snapshot = Snapshot::default();
        for (slot, name, url) in [
            (1, "One", "https://example.org/one"),
            (2, "Two", "https://example.org/two"),
        ] {
            snapshot.stored.insert(
                slot,
                Preset::radio(slot, name, url, Codec::Mp3, None).unwrap(),
            );
        }
        snapshot
    }

    #[test]
    fn loads_current_values_and_reports_duplicate_then_accepts_swap() {
        let snapshot = initial();
        let mut drafts = Drafts::from_snapshot(&snapshot);
        assert_eq!(drafts.at(1).fields.name, "One");
        assert!(!drafts.dirty());
        drafts.url(1, "https://example.org/two".into());
        assert_eq!(drafts.validate(&snapshot).unwrap_err().len(), 2);
        drafts.url(2, "https://example.org/one".into());
        assert!(drafts.validate(&snapshot).is_ok());
        drafts.discard();
        assert!(!drafts.dirty());
    }

    #[test]
    fn clear_and_restore_draft_are_distinct_from_invalid_fields() {
        let snapshot = initial();
        let mut drafts = Drafts::from_snapshot(&snapshot);
        drafts.url(1, "https://website invalid".into());
        assert!(drafts.validate(&snapshot).unwrap_err().contains_key(&1));
        drafts.clear(1);
        assert!(!drafts.validate(&snapshot).unwrap().contains_key(&1));
        drafts.undo(1);
        assert_eq!(drafts.validate(&snapshot).unwrap(), snapshot.stored);
    }
}
