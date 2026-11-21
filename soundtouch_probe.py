#!/usr/bin/env python3
"""Discover and diagnose a Bose SoundTouch speaker using read-only LAN queries."""

import argparse
import http.client
import ipaddress
import json
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from dataclasses import asdict, dataclass

TIMEOUT = 4
MAX_BODY = 256 * 1024
LAN_NETWORKS = tuple(
    ipaddress.IPv4Network(block)
    for block in ("10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16")
)
MDNS_SERVICES = (
    "_soundtouch._tcp",
    "_bose-soundtouch._tcp",
    "_streborn._tcp",
    "_soundtouchstick._tcp",
)
STR_VERSION = re.compile(r"v?[0-9]+\.[0-9]+\.[0-9]+(?:[-+][a-zA-Z0-9.+-]+)?")


class ProbeError(Exception):
    """A diagnostic failure safe to show without printing a raw response."""


@dataclass
class Component:
    category: str | None
    software_version: str | None


@dataclass
class SpeakerInfo:
    model: str
    module_type: str | None
    variant: str | None
    variant_mode: str | None
    firmware: str | None
    components: list[Component]


@dataclass
class Preset:
    slot: int
    source: str | None
    name: str | None


@dataclass
class Agent:
    port: int
    version: str
    build: str | None


@dataclass
class Report:
    info: SpeakerInfo | None
    presets: list[Preset] | None
    str_agent: Agent | None
    errors: dict[str, str]


def lan_ip(value: str) -> str:
    """Accept numeric private or link-local IPv4 addresses."""
    try:
        address = ipaddress.IPv4Address(value)
    except ipaddress.AddressValueError:
        raise argparse.ArgumentTypeError("use a numeric LAN IPv4 address") from None
    if not any(address in network for network in LAN_NETWORKS):
        raise argparse.ArgumentTypeError("use a private or link-local IPv4 address")
    return str(address)


def get(host: str, port: int, path: str) -> bytes:
    # A direct connection ignores proxy settings and never follows redirects.
    connection = http.client.HTTPConnection(host, port, timeout=TIMEOUT)
    try:
        connection.request("GET", path)
        with connection.getresponse() as response:
            if 300 <= response.status < 400:
                raise ProbeError(f"HTTP {response.status} redirect refused")
            if response.status != 200:
                raise ProbeError(f"HTTP {response.status}")
            body = response.read(MAX_BODY + 1)
            if len(body) > MAX_BODY:
                raise ProbeError("response exceeds size limit")
            return body
    except TimeoutError:
        raise ProbeError("connection or response timed out") from None
    except ConnectionRefusedError:
        raise ProbeError("connection refused") from None
    except http.client.HTTPException:
        raise ProbeError("invalid or incomplete HTTP response") from None
    except OSError:
        raise ProbeError("network connection failed") from None
    finally:
        connection.close()


def clean(value: str | None) -> str | None:
    if value is None:
        return None
    safe = "".join(char if char.isprintable() else " " for char in value)
    return safe.strip()[:180] or None


def xml_root(body: bytes, expected: str) -> ET.Element:
    try:
        xml = body.decode("utf-8-sig")
    except UnicodeDecodeError:
        # Some Bose firmware declares UTF-8 but writes names as Latin-1.
        xml = body.removeprefix(b"\xef\xbb\xbf").decode("latin-1")
    if "<!DOCTYPE" in xml:
        raise ProbeError("XML document type declaration refused")
    try:
        root = ET.fromstring(xml)
    except ET.ParseError:
        raise ProbeError("malformed or unsupported XML") from None
    for element in root.iter():
        element.tag = element.tag.rsplit("}", 1)[-1]
    if root.tag != expected:
        raise ProbeError(f"expected <{expected}> XML")
    return root


def parse_info(body: bytes) -> SpeakerInfo:
    root = xml_root(body, "info")
    model = clean(root.findtext("type"))
    if not model:
        raise ProbeError("missing device type in /info")

    components = [
        Component(clean(node.findtext("componentCategory")), clean(node.findtext("softwareVersion")))
        for node in root.findall("components/component")
    ]
    # The SCM component label is also used on sm2 speakers.
    firmware = next(
        (
            component.software_version
            for component in components
            if (component.category or "").casefold() == "scm" and component.software_version
        ),
        None,
    )
    return SpeakerInfo(
        model,
        clean(root.findtext("moduleType")),
        clean(root.findtext("variant")),
        clean(root.findtext("variantMode")),
        firmware,
        components,
    )


def parse_presets(body: bytes) -> list[Preset]:
    root = xml_root(body, "presets")
    presets: dict[int, Preset] = {}
    for node in root:
        if node.tag != "preset":
            raise ProbeError("unexpected element in physical preset list")
        slot_id = node.get("id", "").strip().lstrip("0")
        if slot_id not in ("1", "2", "3", "4", "5", "6"):
            raise ProbeError("unexpected physical preset slot")
        slot = int(slot_id)
        if slot in presets:
            raise ProbeError("duplicate physical preset slot")
        item = node.find("ContentItem")
        presets[slot] = (
            Preset(slot, clean(item.get("source")), clean(item.findtext("itemName")))
            if item is not None else Preset(slot, None, None)
        )
    return sorted(presets.values(), key=lambda preset: preset.slot)


def parse_agent(body: bytes, port: int) -> Agent:
    try:
        reply = json.loads(body)
    except (ValueError, RecursionError):
        raise ProbeError("malformed or unsupported STR JSON") from None
    if isinstance(reply, dict):
        version = reply.get("version")
        build = reply.get("build")
        if (
            isinstance(version, str)
            and len(version) <= 80
            and STR_VERSION.fullmatch(version)
            and isinstance(build, str)
        ):
            return Agent(port, version, clean(build))
    raise ProbeError("response does not match the STR version schema")


def diagnose(host: str) -> Report:
    report = Report(info=None, presets=None, str_agent=None, errors={})
    try:
        report.info = parse_info(get(host, 8090, "/info"))
    except ProbeError as error:
        report.errors["8090/info"] = str(error)
    try:
        report.presets = parse_presets(get(host, 8090, "/presets"))
    except ProbeError as error:
        report.errors["8090/presets"] = str(error)
    for port in (8888, 17008):
        try:
            report.str_agent = parse_agent(get(host, port, "/api/agent/version"), port)
            break
        except ProbeError as error:
            report.errors[f"{port}/api/agent/version"] = str(error)
    return report


def discover() -> tuple[dict[str, list[str]], dict[str, str]]:
    devices: dict[str, list[str]] = {}
    errors: dict[str, str] = {}
    for service in MDNS_SERVICES:
        try:
            result = subprocess.run(
                ["avahi-browse", "-rptk", "-d", "local", service],
                capture_output=True, encoding="utf-8", errors="replace", timeout=TIMEOUT,
            )
        except FileNotFoundError:
            return devices, {"avahi": "avahi-browse is missing (install avahi-utils)"}
        except subprocess.TimeoutExpired:
            errors[service] = "mDNS browse timed out"
            continue
        except OSError:
            return devices, {"avahi": "could not run avahi-browse"}
        if result.returncode:
            errors[service] = "mDNS browse failed (check whether avahi-daemon is running)"
            continue
        for line in result.stdout.splitlines():
            # Avahi parsable mode escapes semicolons in instance names.
            fields = line.split(";", 9)
            if len(fields) < 9 or fields[0] != "=" or fields[4:6] != [service, "local"]:
                continue
            try:
                address = lan_ip(fields[7])
            except argparse.ArgumentTypeError:
                continue
            services = devices.setdefault(address, [])
            if service not in services:
                services.append(service)
    return devices, errors


def print_report(report: Report) -> None:
    info = report.info
    if info:
        print(f"Bose API reachable. Model: {info.model}")
        print(f"Bose firmware (SCM component): {info.firmware or 'unknown'}")
        print(f"Module type: {info.module_type or 'unknown'}")
        print(f"Variant: {info.variant or 'unknown'}")
        print(f"Variant mode: {info.variant_mode or 'unknown'}")
        for component in info.components:
            print(f"  {component.category or 'Unknown component'}: {component.software_version or 'unknown'}")
    else:
        print("Bose /info could not identify a speaker.")

    if report.presets is not None:
        print("Physical presets reported by Bose")
        presets = {preset.slot: preset for preset in report.presets}
        for slot in range(1, 7):
            preset = presets.get(slot)
            description = (
                f"{preset.name or '(unnamed)'} [{preset.source or 'unknown source'}]"
                if preset else "(empty)"
            )
            print(f"  {slot}. {description}")

    agent = report.str_agent
    if agent:
        print(f"STR agent detected on port {agent.port}, version {agent.version}, build {agent.build or 'unknown'}")
    else:
        print("STR agent not detected. Installation status remains unknown.")
    for endpoint, reason in report.errors.items():
        print(f"  {endpoint}: {reason}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--host", type=lan_ip, metavar="IP", help="private or link-local IPv4 address")
    target.add_argument("--discover", action="store_true", help="list local mDNS advertisements using Avahi")
    parser.add_argument("--json", action="store_true", help="print JSON")
    args = parser.parse_args(argv)

    if args.discover:
        devices, errors = discover()
        if args.json:
            print(json.dumps({"devices": devices, "errors": errors}, indent=2))
        else:
            for address, services in devices.items():
                print(f"{address}  {', '.join(services)}")
            if not devices:
                print("No SoundTouch or STR mDNS advertisements found.")
            for service, error in errors.items():
                print(f"{service}: {error}", file=sys.stderr)
        return int(not devices)

    report = diagnose(args.host)
    if args.json:
        print(json.dumps(asdict(report), indent=2))
    else:
        print_report(report)
    return int(report.info is None)


if __name__ == "__main__":
    sys.exit(main())
