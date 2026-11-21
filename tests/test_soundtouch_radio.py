import argparse
import base64
import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, call, patch

import soundtouch_radio as radio
from soundtouch_probe import Agent, ProbeError

HOST = "192.168.1.50"
URL = "https://radiostreaming.ert.gr/ert-kosmos"
AGENT = Agent(17008, "v1.8.2", "2026-11-19-1650")


def station(slot: int, url: str = URL) -> dict:
    return {"slot": slot, "name": "Kosmos", "stream_url": url, "type": "radio"}


def upnp(slot: int) -> tuple[str, str]:
    return "UPNP", f"http://127.0.0.1:8888/stream/{slot}"


class ApiTests(unittest.TestCase):
    @patch.object(radio, "get")
    def test_fallback_requires_matching_str_response(self, get: MagicMock) -> None:
        get.side_effect = [ProbeError("connection refused"), b'{"version":"v1.8.2","build":"2026-11-19-1650"}']
        self.assertEqual(radio.Speaker.connect(HOST).agent, AGENT)
        self.assertEqual(get.call_args_list, [
            call(HOST, 8888, "/api/agent/version"),
            call(HOST, 17008, "/api/agent/version"),
        ])

    @patch.object(radio.http.client, "HTTPConnection")
    def test_read_only_store_uses_individual_slots(self, connection: MagicMock) -> None:
        response = connection.return_value.getresponse.return_value.__enter__.return_value
        response.status = 404
        response.read.return_value = b"preset not set"
        self.assertEqual(radio.Speaker(HOST, AGENT).stored(), {})
        self.assertEqual(
            [(entry.args[0], entry.args[1]) for entry in connection.return_value.request.call_args_list],
            [("GET", f"/api/presets/{slot}") for slot in radio.SLOTS],
        )
        connection.assert_called_with(HOST, 17008, timeout=4)

    @patch.object(radio.http.client, "HTTPConnection")
    def test_write_payload_and_conflict(self, connection: MagicMock) -> None:
        response = connection.return_value.getresponse.return_value.__enter__.return_value
        response.status = 409
        response.read.return_value = b'{"code":"already-on-slot","slot":3,"name":"private"}'
        with self.assertRaisesRegex(radio.HttpError, "already-on-slot.*slot 3"):
            radio.Speaker(HOST, AGENT).request("PUT", "/api/presets/1", station(1))
        method, path, body, headers = connection.return_value.request.call_args.args
        self.assertEqual((method, path, json.loads(body)), ("PUT", "/api/presets/1", station(1)))
        self.assertEqual(headers["Content-Type"], "application/json")
        connection.assert_called_once_with(HOST, 17008, timeout=45)

    @patch.object(radio.http.client, "HTTPConnection")
    def test_write_timeout_requests_readback(self, connection: MagicMock) -> None:
        connection.return_value.getresponse.side_effect = TimeoutError
        with self.assertRaisesRegex(radio.RadioError, "Read back"):
            radio.Speaker(HOST, AGENT).clear(2)


class MappingTests(unittest.TestCase):
    def test_validate_url(self) -> None:
        for url in (URL, "http://example.org/live.aac", "https://example.org:8000/live.m3u8?a=1"):
            self.assertEqual(radio.stream_url(url), url)
        for url in ("ftp://host/a", "https://host/a\n", "https://u:p@host/stream", "http://host:99999",
                    "https://host/#section", "https://", "https://host/invalid path"):
            with self.subTest(url=url), self.assertRaises(argparse.ArgumentTypeError):
                radio.stream_url(url)

    def test_hardware_recognizes_both_str_registration_forms(self) -> None:
        payload = base64.urlsafe_b64encode(json.dumps({"streamUrl": upnp(1)[1]}).encode()).decode().strip("=")
        self.assertTrue(radio.hardware_matches(1, station(1), upnp(1)))
        self.assertTrue(radio.hardware_matches(1, station(1), ("LOCAL_INTERNET_RADIO", "/station?data=" + payload)))
        self.assertFalse(radio.hardware_matches(1, station(1), upnp(2)))
        self.assertFalse(radio.hardware_matches(1, station(1), ("LOCAL_INTERNET_RADIO", "/station?data=invalid")))

    def test_backup_roundtrip_preserves_nonradio_metadata(self) -> None:
        data = {2: {"slot": 2, "name": "Playlist", "type": "spotify", "stream_url": "",
                    "uri": "spotify:playlist:example", "account": "test-account"}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "presets.json"
            radio.save_backup(path, data)
            self.assertEqual(radio.load_backup(path), data)
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError):
                radio.save_backup(path, {})

    def test_backup_rejects_duplicate_slots_before_connecting(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "presets.json"
            path.write_text(json.dumps({"format": 1, "presets": [station(1), station(1)]}))
            with self.assertRaisesRegex(radio.RadioError, "duplicate"):
                radio.load_backup(path)


class MutationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.speaker = MagicMock(spec=radio.Speaker)
        self.args = argparse.Namespace(slot=1, url=URL, name="Kosmos", codec=None, yes=False)

    @patch("builtins.input", return_value="no")
    def test_cancel_skips_all_writes(self, read: MagicMock) -> None:
        self.speaker.stored.return_value = {1: station(1)}
        self.speaker.physical.return_value = {1: upnp(1)}
        with self.assertRaisesRegex(radio.RadioError, "cancelled"):
            radio.set_preset(self.speaker, self.args)
        self.speaker.request.assert_not_called()
        self.assertIn("Replace slot 1 (Kosmos)", read.call_args.args[0])

    @patch("builtins.input", return_value="yes")
    def test_changed_assignment_before_write_is_preserved(self, read: MagicMock) -> None:
        self.speaker.stored.side_effect = [{}, {1: station(1)}]
        with self.assertRaisesRegex(radio.RadioError, "changed during confirmation"):
            radio.set_preset(self.speaker, self.args)
        self.speaker.request.assert_not_called()

    @patch("builtins.input", return_value="yes")
    def test_set_checks_both_store_and_physical_registration(self, read: MagicMock) -> None:
        self.speaker.stored.side_effect = [{}, {}, {1: station(1)}]
        self.speaker.physical.side_effect = [{}, {}, {1: upnp(2)}]
        with self.assertRaisesRegex(radio.RadioError, "Bose key did not verify"):
            radio.set_preset(self.speaker, self.args)
        self.speaker.request.assert_called_once_with("PUT", "/api/presets/1", station(1))

    def test_reordered_restore_uses_one_bulk_write(self) -> None:
        before = {1: station(1, "https://example.org/a"), 2: station(2)}
        desired = {1: station(1), 2: station(2, "https://example.org/a")}
        self.speaker.stored.side_effect = [before, before, desired]
        self.speaker.physical.return_value = {1: upnp(1), 2: upnp(2)}
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            path = Path(directory) / "presets.json"
            radio.restore(self.speaker, desired, path, True)
            snapshot, = path.parent.glob("presets.before-restore-*.json")
            self.assertEqual(radio.load_backup(snapshot), before)
        self.speaker.request.assert_called_once_with("PUT", "/api/presets", list(desired.values()))
        self.speaker.clear.assert_not_called()

    def test_restore_clears_a_conflicting_slot_being_emptied(self) -> None:
        before, desired = {2: station(2)}, {1: station(1)}
        self.speaker.stored.side_effect = [before, before, before, desired]
        self.speaker.physical.side_effect = [{2: upnp(2)}, {2: upnp(2)}, {1: upnp(1)}]
        self.speaker.request.side_effect = [
            radio.HttpError("PUT", "/api/presets", 409, b'{"code":"already-on-slot","slot":2}'),
            {},
        ]
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            radio.restore(self.speaker, desired, Path(directory) / "presets.json", True)
        self.assertEqual(self.speaker.request.call_count, 2)
        self.speaker.clear.assert_called_once_with(2)

    def test_restore_preserves_hardware_only_entries(self) -> None:
        self.speaker.stored.return_value = {}
        self.speaker.physical.return_value = {1: ("TUNEIN", "legacy")}
        with self.assertRaisesRegex(radio.RadioError, "hardware-only slots 1"):
            radio.restore(self.speaker, {}, Path("presets.json"), True)
        self.speaker.request.assert_not_called()
        self.speaker.clear.assert_not_called()
