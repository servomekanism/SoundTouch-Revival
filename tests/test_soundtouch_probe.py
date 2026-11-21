import argparse
import io
import json
import subprocess
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest.mock import MagicMock, call, patch

import soundtouch_probe as probe


HOST = "192.168.1.50"
INFO = b"""<info deviceID="private-device-id">
  <name>private-room-name</name>
  <type>SoundTouch 20</type>
  <moduleType>sm2</moduleType>
  <variant>spotty</variant>
  <variantMode>normal</variantMode>
  <margeAccountUUID>private-account-id</margeAccountUUID>
  <networkInfo><macAddress>private-mac</macAddress><ipAddress>192.168.1.50</ipAddress></networkInfo>
  <components>
    <component><componentCategory>PackagedProduct</componentCategory>
      <softwareVersion>other-version</softwareVersion></component>
    <component><componentCategory>SCM</componentCategory>
      <softwareVersion>27.0.6.46330.5043500 epdbuild.trunk.hepdswbld04.2022-08-04T11:20:29</softwareVersion>
      <serialNumber>private-serial</serialNumber></component>
  </components>
</info>"""
PRESETS = b"""<presets>
  <preset id="6" createdOn="123"><ContentItem source="UPNP" location="private-stream-url"
      sourceAccount="private-preset-account"><itemName>Radio &amp; Jazz</itemName></ContentItem></preset>
  <preset id="1"><ContentItem source="TUNEIN"><itemName>News</itemName></ContentItem></preset>
</presets>"""
AGENT = b'{"version":"v1.8.2","build":"2026-11-19","friendlyName":"private-agent-name"}'


class HTTPTests(unittest.TestCase):
    def setUp(self) -> None:
        patcher = patch.object(probe.http.client, "HTTPConnection", autospec=True)
        self.connection_type = patcher.start()
        self.addCleanup(patcher.stop)
        self.connection = self.connection_type.return_value
        self.response = MagicMock()
        self.connection.getresponse.return_value.__enter__.return_value = self.response
        self.response.status = 200

    def test_get_only_direct_connection_with_timeout(self) -> None:
        self.response.read.return_value = INFO
        self.assertEqual(probe.get(HOST, 8090, "/info"), INFO)
        self.connection_type.assert_called_once_with(HOST, 8090, timeout=probe.TIMEOUT)
        self.connection.request.assert_called_once_with("GET", "/info")
        self.response.read.assert_called_once_with(probe.MAX_BODY + 1)
        self.connection.close.assert_called_once()

    def test_redirect_and_http_error_do_not_read_or_follow_response(self) -> None:
        for status, message in ((302, "redirect refused"), (404, "HTTP 404")):
            with self.subTest(status=status):
                self.connection.request.reset_mock()
                self.response.status = status
                with self.assertRaisesRegex(probe.ProbeError, message):
                    probe.get(HOST, 8090, "/presets")
                self.connection.request.assert_called_once_with("GET", "/presets")
                self.response.read.assert_not_called()
        self.assertEqual(self.connection_type.call_count, 2)

    def test_timeout_refusal_and_invalid_http_are_reported(self) -> None:
        for error, message in (
            (TimeoutError("private-host"), "timed out"),
            (ConnectionRefusedError("private-host"), "connection refused"),
            (probe.http.client.IncompleteRead(b"private-body"), "incomplete HTTP"),
        ):
            with self.subTest(error=type(error).__name__):
                self.connection.getresponse.side_effect = error
                with self.assertRaisesRegex(probe.ProbeError, message) as caught:
                    probe.get(HOST, 8090, "/info")
                self.assertNotIn("private-", str(caught.exception))
        self.assertEqual(self.connection.close.call_count, 3)

    def test_oversized_body(self) -> None:
        self.response.read.return_value = b"x" * (probe.MAX_BODY + 1)
        with self.assertRaisesRegex(probe.ProbeError, "size limit"):
            probe.get(HOST, 8090, "/info")


class ParserTests(unittest.TestCase):
    def test_info_has_module_and_full_scm_version(self) -> None:
        info = probe.parse_info(INFO.replace(b"<info ", b'<info xmlns="urn:bose" '))
        self.assertEqual(info.model, "SoundTouch 20")
        self.assertEqual(info.module_type, "sm2")
        self.assertEqual(info.variant, "spotty")
        self.assertEqual(info.variant_mode, "normal")
        self.assertEqual(info.firmware, "27.0.6.46330.5043500 epdbuild.trunk.hepdswbld04.2022-08-04T11:20:29")
        self.assertEqual(info.components[0].category, "PackagedProduct")

    def test_model_alone_leaves_module_and_firmware_unknown(self) -> None:
        info = probe.parse_info(b"<info><type>SoundTouch 20</type></info>")
        self.assertIsNone(info.module_type)
        self.assertIsNone(info.firmware)

    def test_invalid_info_xml(self) -> None:
        for body in (
            b"<info>",
            b"<info/>",
            b"<html/>",
            b"<info><type>SoundTouch 20\x00</type></info>",
            b'<!DOCTYPE info [<!ENTITY x "data">]><info><type>&x;</type></info>',
        ):
            with self.subTest(body=body), self.assertRaises(probe.ProbeError):
                probe.parse_info(body)

    def test_preset_names_follow_bose_schema_and_order(self) -> None:
        self.assertEqual(
            probe.parse_presets(PRESETS),
            [probe.Preset(1, "TUNEIN", "News"), probe.Preset(6, "UPNP", "Radio & Jazz")],
        )
        self.assertEqual(probe.parse_presets(b"<presets/>"), [])

    def test_invalid_preset_list_is_not_reported_as_empty(self) -> None:
        entry = b'<preset id="1"><ContentItem/></preset>'
        for body in (
            b"<errors/>",
            b"<presets><errors/></presets>",
            b'<presets><preset id="7"><ContentItem/></preset></presets>',
            b"<presets>" + entry * 2 + b"</presets>",
        ):
            with self.subTest(body=body), self.assertRaises(probe.ProbeError):
                probe.parse_presets(body)

    def test_known_firmware_encoding_and_slot_quirks(self) -> None:
        info = INFO.replace(b"private-room-name", b"K\xfcche")
        self.assertEqual(probe.parse_info(info).model, "SoundTouch 20")
        presets = (
            b'<?xml version="1.0" encoding="UTF-8"?><presets><preset id="2"/>'
            b'<preset id="04"><ContentItem><itemName>K\xfcche</itemName></ContentItem></preset></presets>'
        )
        self.assertEqual(probe.parse_presets(presets), [
            probe.Preset(2, None, None),
            probe.Preset(4, None, "Küche"),
        ])

    def test_agent_requires_documented_version_response(self) -> None:
        for body in (
            b"<html/>", b"[]", b'{"version":null}', b'{"version":"1.8.2"}',
            b'{"version":"other","build":"1"}', b'{"version":"1.8.2","build":null}',
        ):
            with self.subTest(body=body), self.assertRaises(probe.ProbeError):
                probe.parse_agent(body, 8888)
        self.assertEqual(probe.parse_agent(AGENT, 8888), probe.Agent(8888, "v1.8.2", "2026-11-19"))

    def test_lan_ipv4_only(self) -> None:
        for address in ("10.0.0.10", "172.31.2.3", HOST, "169.254.1.1"):
            self.assertEqual(probe.lan_ip(address), address)
        for address in ("speaker.local", "127.0.0.1", "8.8.8.8", "224.0.0.1", "::1", HOST + ":8090"):
            with self.subTest(address=address), self.assertRaises(argparse.ArgumentTypeError):
                probe.lan_ip(address)


class CLITests(unittest.TestCase):
    @patch.object(probe, "get")
    def test_json_privacy_and_fallback_to_bco_port(self, get: MagicMock) -> None:
        get.side_effect = [INFO, PRESETS, b"{}", AGENT]
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(probe.main(["--host", HOST, "--json"]), 0)
        get.assert_has_calls([
            call(HOST, 8090, "/info"), call(HOST, 8090, "/presets"),
            call(HOST, 8888, "/api/agent/version"), call(HOST, 17008, "/api/agent/version"),
        ])
        self.assertEqual(get.call_count, 4)
        self.assertNotIn("private-", output.getvalue())
        self.assertNotIn(HOST, output.getvalue())
        report = json.loads(output.getvalue())
        self.assertEqual(report["str_agent"]["port"], 17008)
        self.assertIn("8888/api/agent/version", report["errors"])

    @patch.object(probe, "get")
    def test_unreadable_info_keeps_independent_results(self, get: MagicMock) -> None:
        get.side_effect = [probe.ProbeError("timed out"), PRESETS, AGENT]
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(probe.main(["--host", HOST]), 1)
        self.assertEqual(get.call_count, 3)
        self.assertIn("STR agent detected on port 8888", output.getvalue())
        self.assertIn("6. Radio & Jazz [UPNP]", output.getvalue())
        self.assertIn("8090/info: timed out", output.getvalue())

    @patch.object(probe, "get")
    def test_unsupported_preset_and_str_keep_unknown_status(self, get: MagicMock) -> None:
        get.side_effect = [INFO, probe.ProbeError("HTTP 404"), b"{}", b"{}"]
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(probe.main(["--host", HOST]), 0)
        self.assertIn("Installation status remains unknown.", output.getvalue())
        self.assertIn("8090/presets: HTTP 404", output.getvalue())
        self.assertNotIn("(empty)", output.getvalue())

    @patch.object(probe.subprocess, "run")
    def test_discovery_lists_addresses_and_services_without_metadata(self, run: MagicMock) -> None:
        def mdns(command: list[str], **kwargs: object) -> subprocess.CompletedProcess:
            service = command[-1]
            output = ""
            if service in ("_soundtouch._tcp", "_streborn._tcp"):
                resolved = (
                    f"=;wlan0;IPv4;private-room-name;{service};local;"
                    "private-hostname;192.168.1.50;8090;private-txt\n"
                )
                output = resolved * 2 + resolved.replace(HOST, "8.8.8.8")
            return subprocess.CompletedProcess(command, 0, stdout=output, stderr="")

        run.side_effect = mdns
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(probe.main(["--discover", "--json"]), 0)
        self.assertEqual(json.loads(output.getvalue()), {
            "devices": {HOST: ["_soundtouch._tcp", "_streborn._tcp"]},
            "errors": {},
        })
        self.assertEqual(run.call_count, len(probe.MDNS_SERVICES))
        self.assertEqual(run.call_args.kwargs["timeout"], probe.TIMEOUT)

    @patch.object(probe.subprocess, "run", side_effect=FileNotFoundError)
    def test_discovery_missing_avahi(self, run: MagicMock) -> None:
        error = io.StringIO()
        with redirect_stdout(io.StringIO()), redirect_stderr(error):
            self.assertEqual(probe.main(["--discover"]), 1)
        self.assertIn("install avahi-utils", error.getvalue())
        run.assert_called_once()


if __name__ == "__main__":
    unittest.main()
