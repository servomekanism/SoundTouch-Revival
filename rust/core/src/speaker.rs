use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::backup::recovery_backup;
use crate::model::{check_slot, hardware_matches, parse_agent, parse_hardware, parse_info};
use crate::transport::{BOSE_PORT, HttpTransport, Method, STR_PORTS, Transport};
use crate::{
    Agent, ConnectionAttempt, Error, HardwarePreset, Host, Preset, Presets, Report, Result, SLOTS,
    SpeakerInfo,
};

const VERSION: &str = "/api/agent/version";
const BOSE_PRESETS: &str = "/presets";
const STR_PRESETS: &str = "/api/presets";

#[derive(Clone)]
pub struct Speaker {
    host: Host,
    agent: Agent,
    transport: Arc<dyn Transport>,
}

impl std::fmt::Debug for Speaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Speaker")
            .field("host", &self.host)
            .field("agent", &self.agent)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub stored: Presets,
    pub hardware: BTreeMap<u8, HardwarePreset>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyMode {
    Edit,
    Restore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
    pub slot: u8,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ApplyOutcome {
    pub snapshot: Snapshot,
    pub recovery: Option<PathBuf>,
}

impl Snapshot {
    pub fn hardware_only(&self, slot: u8) -> bool {
        !self.stored.contains_key(&slot) && self.hardware.get(&slot).is_some_and(|key| key.has_item)
    }

    pub fn mismatches(&self) -> Vec<u8> {
        SLOTS.filter(|slot| self.mismatched(*slot)).collect()
    }

    fn mismatched(&self, slot: u8) -> bool {
        match self.stored.get(&slot) {
            Some(preset) => !hardware_matches(preset, self.hardware.get(&slot)),
            None => self.hardware_only(slot),
        }
    }

    pub fn changes(&self, desired: &Presets, mode: ApplyMode) -> Result<Vec<Change>> {
        for (&slot, preset) in desired {
            check_slot(slot)?;
            if preset.slot != slot {
                return Err(Error::Invalid(format!(
                    "preset slot does not match entry {slot}"
                )));
            }
        }
        let mut result = Vec::new();
        for slot in SLOTS {
            let old = self.stored.get(&slot);
            let new = desired.get(&slot);
            if self.hardware_only(slot) && (new.is_some() || mode == ApplyMode::Restore) {
                return Err(Error::Invalid(format!(
                    "slot {slot} has a Bose-only preset that STR cannot restore"
                )));
            }
            if old != new || (mode == ApplyMode::Restore && new.is_some() && self.mismatched(slot))
            {
                result.push(Change {
                    slot,
                    before: old.map(|p| p.name.clone()),
                    after: new.map(|p| p.name.clone()),
                });
            }
        }
        Ok(result)
    }
}

impl Speaker {
    pub fn connect(host: Host) -> Result<Self> {
        Self::connect_with_transport(host, Arc::new(HttpTransport::default()))
    }

    /// Load the initial preset snapshot under a single deadline.
    pub fn load(
        host: Host,
        attempt: &ConnectionAttempt,
    ) -> Result<(Self, Option<SpeakerInfo>, Snapshot)> {
        attempt.remaining()?;
        let transport = Arc::new(HttpTransport::for_connection(attempt.clone()));
        let result = (|| {
            let mut speaker = Self::connect_with_transport(host, transport)?;
            let info = speaker.info().ok();
            let snapshot = speaker.snapshot()?;
            // The deadline is for the initial load only.
            speaker.transport = Arc::new(HttpTransport::default());
            Ok((speaker, info, snapshot))
        })();
        attempt.remaining()?;
        result
    }

    fn connect_with_transport(host: Host, transport: Arc<dyn Transport>) -> Result<Self> {
        let (agent, errors) = probe_agent(transport.as_ref(), host);
        match agent {
            Some(agent) => Ok(Self {
                host,
                agent,
                transport,
            }),
            None => match bose_info(transport.as_ref(), host) {
                Ok(_) => Err(Error::AgentUnavailable(port_errors(&errors))),
                Err(error) => Err(Error::SpeakerUnavailable(format!(
                    "Bose API: {error}. STR: {}",
                    port_errors(&errors)
                ))),
            },
        }
    }

    pub fn host(&self) -> Host {
        self.host
    }

    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    pub fn info(&self) -> Result<SpeakerInfo> {
        bose_info(self.transport.as_ref(), self.host)
    }

    fn request(&self, method: Method, path: &str, body: &[u8]) -> Result<Vec<u8>> {
        self.transport
            .request(self.host, self.agent.port, method, path, body)
    }

    pub fn stored(&self) -> Result<Presets> {
        let mut map = Presets::new();
        // Individual GETs avoid triggering the bulk GET's library refresh sweep.
        for slot in SLOTS {
            if let Some(preset) = self.read_slot(slot)? {
                map.insert(slot, preset);
            }
        }
        Ok(map)
    }

    fn read_slot(&self, slot: u8) -> Result<Option<Preset>> {
        let body = match self.request(Method::Get, &key_path(slot), &[]) {
            Ok(body) => body,
            Err(Error::Http { status: 404, .. }) => return Ok(None),
            Err(err) => return Err(err),
        };
        let preset: Preset = serde_json::from_slice(&body).map_err(|_| {
            Error::Response(format!("unexpected STR preset response for slot {slot}"))
        })?;
        if preset.slot != slot {
            return Err(Error::Response(format!(
                "unexpected STR preset response for slot {slot}"
            )));
        }
        Ok(Some(preset))
    }

    fn hardware(&self) -> Result<BTreeMap<u8, HardwarePreset>> {
        let body = self
            .transport
            .request(self.host, BOSE_PORT, Method::Get, BOSE_PRESETS, &[])?;
        parse_hardware(&body)
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        Ok(Snapshot {
            stored: self.stored()?,
            hardware: self.hardware()?,
        })
    }

    pub fn set(&self, baseline: &Snapshot, station: &Preset) -> Result<()> {
        let slot = station.slot;
        check_slot(slot)?;
        self.guard_slot(baseline, slot)?;
        self.send_put(&key_path(slot), station)?;
        let saved = self.read_slot(slot)?;
        if !saved
            .as_ref()
            .is_some_and(|saved| station.matches_saved_radio(saved))
        {
            return Err(Error::Readback(slot.to_string()));
        }
        if !hardware_matches(station, self.hardware()?.get(&slot)) {
            return Err(Error::Hardware(slot.to_string()));
        }
        Ok(())
    }

    pub fn apply(
        &self,
        baseline: &Snapshot,
        desired: &Presets,
        mode: ApplyMode,
        recovery_dir: &Path,
    ) -> Result<ApplyOutcome> {
        let plan = baseline.changes(desired, mode)?;
        let current = self.snapshot()?;
        if &current != baseline {
            return Err(Error::Invalid(
                "presets changed after they were loaded. Reload them before applying changes"
                    .into(),
            ));
        }
        if plan.is_empty() {
            return Ok(ApplyOutcome {
                snapshot: current,
                recovery: None,
            });
        }
        let recovery = recovery_backup(recovery_dir, &baseline.stored)?;
        match self.apply_plan(baseline, desired, &plan, mode) {
            Ok(snapshot) => Ok(ApplyOutcome {
                snapshot,
                recovery: Some(recovery),
            }),
            Err(source) => Err(Error::Apply {
                source: Box::new(source),
                backup: recovery,
            }),
        }
    }

    fn apply_plan(
        &self,
        baseline: &Snapshot,
        desired: &Presets,
        plan: &[Change],
        mode: ApplyMode,
    ) -> Result<Snapshot> {
        let mut clearing: BTreeSet<_> = plan
            .iter()
            .filter(|change| change.after.is_none())
            .map(|change| change.slot)
            .collect();
        let writes: Vec<_> = plan
            .iter()
            .filter_map(|change| desired.get(&change.slot))
            .collect();
        if !writes.is_empty() {
            loop {
                match self.send_put(STR_PRESETS, &writes) {
                    Ok(()) => break,
                    Err(Error::Http {
                        status: 409,
                        code: Some(ref code),
                        slot: Some(slot),
                        ..
                    }) if code == "already-on-slot" && clearing.remove(&slot) => {
                        self.clear_unchanged(baseline, slot)?;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        for slot in clearing {
            self.clear_unchanged(baseline, slot)?;
        }

        let snapshot = self.snapshot()?;
        // For edits, allow server-populated metadata for a replacement radio
        // station. Restore verifies the full backup, including opaque fields.
        let differs = |slot: u8| {
            let wanted = desired.get(&slot);
            let saved = snapshot.stored.get(&slot);
            let same_radio_edit = mode == ApplyMode::Edit
                && plan.iter().any(|change| change.slot == slot)
                && wanted
                    .zip(saved)
                    .is_some_and(|(p, q)| p.is_radio() && p.matches_saved_radio(q));
            wanted != saved && !same_radio_edit
        };
        let fail: Vec<_> = SLOTS.filter(|slot| differs(*slot)).collect();
        if !fail.is_empty() {
            return Err(Error::Readback(slot_list(&fail)));
        }
        let fail: Vec<_> = SLOTS
            .filter(|slot| {
                (mode == ApplyMode::Restore || plan.iter().any(|change| change.slot == *slot))
                    && snapshot.mismatched(*slot)
            })
            .collect();
        if !fail.is_empty() {
            return Err(Error::Hardware(slot_list(&fail)));
        }

        Ok(snapshot)
    }

    fn guard_slot(&self, baseline: &Snapshot, slot: u8) -> Result<()> {
        if self.read_slot(slot)?.as_ref() != baseline.stored.get(&slot)
            || self.hardware()?.get(&slot) != baseline.hardware.get(&slot)
        {
            return Err(Error::Invalid(format!(
                "slot {slot} changed since it was loaded. Reload before overwriting"
            )));
        }
        Ok(())
    }

    fn clear_unchanged(&self, baseline: &Snapshot, slot: u8) -> Result<()> {
        self.guard_slot(baseline, slot)?;
        self.request(Method::Delete, &key_path(slot), &[])?;
        Ok(())
    }

    fn send_put(&self, path: &str, value: &impl Serialize) -> Result<()> {
        let payload = serde_json::to_vec(value)
            .map_err(|_| Error::Invalid("could not encode the preset write".into()))?;
        self.request(Method::Put, path, &payload)?;
        Ok(())
    }
}

fn slot_list(slots: &[u8]) -> String {
    slots
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn key_path(slot: u8) -> String {
    format!("{STR_PRESETS}/{slot}")
}

fn bose_info(transport: &dyn Transport, host: Host) -> Result<SpeakerInfo> {
    let body = transport.request(host, BOSE_PORT, Method::Get, "/info", &[])?;
    parse_info(&body)
}

fn probe_agent(transport: &dyn Transport, host: Host) -> (Option<Agent>, BTreeMap<String, String>) {
    let mut errors = BTreeMap::new();
    for port in STR_PORTS {
        match transport
            .request(host, port, Method::Get, VERSION, &[])
            .and_then(|body| parse_agent(&body, port))
        {
            Ok(agent) => return (Some(agent), errors),
            Err(error) => {
                errors.insert(format!("{port}{VERSION}"), error.to_string());
            }
        }
    }
    (None, errors)
}

fn port_errors(errors: &BTreeMap<String, String>) -> String {
    errors
        .iter()
        .map(|(port, error)| format!("{port}: {error}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn diagnose(host: Host) -> Report {
    let transport = HttpTransport::default();
    let mut report = Report::default();
    match bose_info(&transport, host) {
        Ok(info) => report.info = Some(info),
        Err(error) => {
            report.errors.insert("8090/info".into(), error.to_string());
        }
    }
    let presets = transport
        .request(host, BOSE_PORT, Method::Get, BOSE_PRESETS, &[])
        .and_then(|body| parse_hardware(&body));
    match presets {
        Ok(presets) => report.presets = Some(presets.into_values().collect()),
        Err(error) => {
            report
                .errors
                .insert("8090/presets".into(), error.to_string());
        }
    }
    let (agent, errors) = probe_agent(&transport, host);
    report.str_agent = agent;
    report.errors.extend(errors);
    report
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use serde_json::Value;

    use super::*;
    use crate::{Codec, load_backup};

    type Call = (Method, u16, String, Option<Value>, Result<Vec<u8>>);

    #[derive(Default)]
    struct Mock(Mutex<VecDeque<Call>>);

    impl Transport for Mock {
        fn request(
            &self,
            _: Host,
            port: u16,
            method: Method,
            path: &str,
            payload: &[u8],
        ) -> Result<Vec<u8>> {
            let (verb, at, expected, body, result) = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected {method:?} {port}{path}"));
            assert_eq!((method, port, path), (verb, at, expected.as_str()));
            let payload = if payload.is_empty() {
                None
            } else {
                Some(serde_json::from_slice(payload).unwrap())
            };
            assert_eq!(payload, body);
            result
        }
    }

    fn missing(slot: u8) -> Error {
        Error::Http {
            operation: key_path(slot),
            status: 404,
            detail: String::new(),
            code: None,
            slot: None,
        }
    }

    impl Mock {
        fn enqueue(
            &self,
            method: Method,
            port: u16,
            path: &str,
            body: Option<Value>,
            result: Result<Vec<u8>>,
        ) {
            self.0
                .lock()
                .unwrap()
                .push_back((method, port, path.into(), body, result));
        }

        fn get_slot(&self, slot: u8, stored: &Presets) {
            let body = stored
                .get(&slot)
                .map(|preset| serde_json::to_vec(preset).unwrap())
                .ok_or_else(|| missing(slot));
            self.enqueue(Method::Get, 17008, &key_path(slot), None, body);
        }

        fn hardware(&self, slots: &[u8]) {
            let mut xml = String::from("<presets>");
            for slot in slots {
                xml.push_str(&format!("<preset id='{slot}'><ContentItem source='UPNP' location='http://127.0.0.1:8888/stream/{slot}'/></preset>"));
            }
            xml.push_str("</presets>");
            self.enqueue(Method::Get, 8090, BOSE_PRESETS, None, Ok(xml.into_bytes()));
        }

        fn snapshot(&self, stored: &Presets, hardware: &[u8]) {
            for slot in SLOTS {
                self.get_slot(slot, stored);
            }
            self.hardware(hardware);
        }

        fn bulk(&self, payload: &[&Preset], reply: Result<Vec<u8>>) {
            self.enqueue(
                Method::Put,
                17008,
                STR_PRESETS,
                Some(serde_json::to_value(payload).unwrap()),
                reply,
            );
        }

        fn speaker(self: &Arc<Self>) -> Speaker {
            Speaker {
                host: "192.168.1.50".parse().unwrap(),
                agent: parse_agent(br#"{"version":"v1.8.2","build":"test"}"#, 17008).unwrap(),
                transport: self.clone(),
            }
        }

        fn finished(&self) {
            assert!(self.0.lock().unwrap().is_empty());
        }
    }

    fn station(slot: u8, name: &str) -> Preset {
        Preset::radio(
            slot,
            name,
            &format!("https://example.org/{name}"),
            Codec::Mp3,
            None,
        )
        .unwrap()
    }

    #[test]
    fn initial_load_rejects_a_cancelled_or_expired_attempt() {
        use std::time::{Duration, Instant};

        let host = "192.168.1.50".parse().unwrap();
        let cancelled = ConnectionAttempt::new(Instant::now() + Duration::from_secs(10));
        cancelled.clone().cancel();
        assert!(matches!(
            Speaker::load(host, &cancelled),
            Err(Error::ConnectionCancelled)
        ));

        let expired = ConnectionAttempt::new(Instant::now());
        assert!(matches!(
            Speaker::load(host, &expired),
            Err(Error::ConnectionTimeout)
        ));
    }

    #[test]
    fn checks_agent_schema_on_both_ports_and_read_only_snapshot_uses_slot_gets() {
        let mock = Arc::new(Mock::default());
        let speaker = mock.speaker();
        mock.enqueue(Method::Get, 8888, VERSION, None, Ok(b"{}".to_vec()));
        mock.enqueue(
            Method::Get,
            17008,
            VERSION,
            None,
            Ok(br#"{"version":"v1.8.2","build":"test"}"#.to_vec()),
        );
        let (agent, errors) = probe_agent(mock.as_ref(), speaker.host());
        assert_eq!(agent.unwrap().port, 17008);
        assert_eq!(errors.len(), 1);
        let stored = Presets::from([(1, station(1, "one"))]);
        mock.snapshot(&stored, &[1, 2]);
        let snapshot = speaker.snapshot().unwrap();
        assert_eq!(snapshot.stored, stored);
        assert!(snapshot.hardware_only(2));
        assert_eq!(snapshot.mismatches(), [2]);
        assert!(snapshot.changes(&stored, ApplyMode::Restore).is_err());
        mock.finished();
    }

    #[test]
    fn bulk_edit_swaps_two_existing_presets_in_one_write_and_verifies_both() {
        let mock = Arc::new(Mock::default());
        let speaker = mock.speaker();
        let stored = Presets::from([(1, station(1, "one")), (2, station(2, "two"))]);
        let desired = Presets::from([(1, station(1, "two")), (2, station(2, "one"))]);
        mock.snapshot(&stored, &[1, 2]);
        mock.snapshot(&stored, &[1, 2]);
        mock.bulk(&[&desired[&1], &desired[&2]], Ok(Vec::new()));
        mock.snapshot(&desired, &[1, 2]);
        let before = speaker.snapshot().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let result = speaker
            .apply(&before, &desired, ApplyMode::Edit, folder.path())
            .unwrap();
        assert_eq!(result.snapshot.stored, desired);
        assert_eq!(load_backup(&result.recovery.unwrap()).unwrap(), stored);
        mock.finished();
    }

    #[test]
    fn bulk_edit_refuses_concurrent_changes_before_backup_or_write() {
        let mock = Arc::new(Mock::default());
        let speaker = mock.speaker();
        let stored = Presets::from([(1, station(1, "one"))]);
        let external = Presets::from([(1, station(1, "someone-else"))]);
        let desired = Presets::from([(1, station(1, "two"))]);
        mock.snapshot(&stored, &[1]);
        mock.snapshot(&external, &[1]);
        let before = speaker.snapshot().unwrap();
        let folder = tempfile::tempdir().unwrap();
        assert!(matches!(
            speaker.apply(&before, &desired, ApplyMode::Edit, folder.path()),
            Err(Error::Invalid(_))
        ));
        assert_eq!(folder.path().read_dir().unwrap().count(), 0);
        mock.finished();
    }

    #[test]
    fn restore_clears_only_a_planned_blocking_slot_and_reports_failed_registration() {
        let mock = Arc::new(Mock::default());
        let speaker = mock.speaker();
        let stored = Presets::from([(1, station(1, "one"))]);
        let desired = Presets::from([(2, station(2, "one"))]);
        mock.snapshot(&stored, &[1]);
        mock.snapshot(&stored, &[1]);
        mock.bulk(
            &[&desired[&2]],
            Err(Error::Http {
                operation: STR_PRESETS.into(),
                status: 409,
                detail: String::new(),
                code: Some("already-on-slot".into()),
                slot: Some(1),
            }),
        );
        mock.get_slot(1, &stored);
        mock.hardware(&[1]);
        mock.enqueue(Method::Delete, 17008, &key_path(1), None, Ok(Vec::new()));
        mock.bulk(&[&desired[&2]], Ok(Vec::new()));
        mock.snapshot(&desired, &[]);
        let before = speaker.snapshot().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let Error::Apply { source, backup } = speaker
            .apply(&before, &desired, ApplyMode::Restore, folder.path())
            .unwrap_err()
        else {
            panic!("expected an incomplete restore with recovery backup");
        };
        assert!(matches!(*source, Error::Hardware(ref slot) if slot == "2"));
        assert_eq!(load_backup(&backup).unwrap(), stored);
        mock.finished();
    }
}
