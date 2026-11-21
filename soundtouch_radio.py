#!/usr/bin/env python3
"""Manage the six physical radio presets on a SoundTouch speaker running STR."""

import argparse
import base64
import http.client
import json
import os
import re
import sys
import tempfile
from pathlib import Path
from typing import Any
from urllib.parse import unquote, urlsplit

from soundtouch_probe import (
    Agent,
    ProbeError,
    clean,
    get,
    lan_ip,
    main as probe_main,
    parse_agent,
    parse_presets,
    xml_root,
)

SLOTS = range(1, 7)
BACKUP_VERSION = 1
MAX_BACKUP = 6 * 1024 * 1024 + 4096
MAX_RESPONSE = 1024 * 1024
JsonObject = dict[str, Any]
PresetMap = dict[int, JsonObject]


class RadioError(Exception):
    """A preset manager error suitable for the terminal."""


class HttpError(RadioError):
    def __init__(self, method: str, path: str, status: int, body: bytes) -> None:
        self.status = status
        self.code = None
        self.slot = None
        try:
            reply = json.loads(body)
        except (ValueError, RecursionError):
            reply = None
        message = f"{method} {path} returned HTTP {status}"
        if isinstance(reply, dict):
            code = reply.get("code")
            if isinstance(code, str) and re.fullmatch(r"[a-z0-9-]{1,64}", code):
                self.code = code
                message += f" ({code})"
            if type(reply.get("slot")) is int and 1 <= reply["slot"] <= 12:
                self.slot = reply["slot"]
                message += f", slot {self.slot}"
        super().__init__(message)


class Speaker:
    def __init__(self, host: str, agent: Agent) -> None:
        self.host = host
        self.agent = agent

    @classmethod
    def connect(cls, host: str) -> "Speaker":
        errors = []
        for port in (8888, 17008):
            try:
                return cls(host, parse_agent(get(host, port, "/api/agent/version"), port))
            except ProbeError as exc:
                errors.append(f"{port}: {exc}")
        raise RadioError(f"STR version endpoint did not identify an agent ({', '.join(errors)})")

    def request(self, method: str, path: str, payload: object = None) -> Any:
        # Bulk writes wait for STR to pace the updates to the six hardware keys.
        timeout = 4 if method == "GET" else 45
        connection = http.client.HTTPConnection(self.host, self.agent.port, timeout=timeout)
        try:
            body = json.dumps(payload, allow_nan=False).encode("utf-8") if payload is not None else None
            headers = {"User-Agent": "soundtouch-radio/0.1"}
            if body is not None:
                headers["Content-Type"] = "application/json"
            connection.request(method, path, body, headers)
            with connection.getresponse() as response:
                if response.status == 204 and method == "DELETE":
                    return None
                data = response.read(MAX_RESPONSE + 1)
                if response.status != 200:
                    raise HttpError(method, path, response.status, data[:MAX_RESPONSE])
                if len(data) > MAX_RESPONSE:
                    raise RadioError(f"{path} response exceeds the size limit")
                return json.loads(data)
        except (OSError, http.client.HTTPException) as exc:
            error = "timed out" if isinstance(exc, TimeoutError) else "connection failed"
            suffix = ". Read back the presets before retrying" if method != "GET" else ""
            raise RadioError(f"{method} {path} {error}{suffix}") from None
        except (ValueError, RecursionError):
            raise RadioError(f"{method} {path} returned unreadable data") from None
        finally:
            connection.close()

    def stored(self) -> PresetMap:
        current: PresetMap = {}
        # Per-slot GET avoids kicking the agent's library refresh sweep.
        for slot in SLOTS:
            try:
                item = self.request("GET", f"/api/presets/{slot}")
            except HttpError as exc:
                if exc.status == 404:
                    continue
                raise
            if not isinstance(item, dict) or type(item.get("slot")) is not int or item["slot"] != slot:
                raise RadioError(f"unexpected STR preset response for slot {slot}")
            current[slot] = item
        return current

    def physical(self) -> dict[int, tuple[str | None, str | None]]:
        body = get(self.host, 8090, "/presets")
        parse_presets(body)
        physical = {}
        for node in xml_root(body, "presets"):
            item = node.find("ContentItem")
            if item is not None:
                physical[int(node.get("id", ""))] = (item.get("source"), item.get("location"))
        return physical

    def clear(self, slot: int) -> None:
        self.request("DELETE", f"/api/presets/{slot}")


def stream_url(value: str) -> str:
    try:
        url = urlsplit(value)
        if (
            url.scheme in ("http", "https") and url.hostname and url.port != 0
            and url.username is None and url.password is None and not url.fragment
            and "\\" not in value
            and all(char.isprintable() and not char.isspace() for char in value)
        ):
            return value
    except ValueError:
        pass
    raise argparse.ArgumentTypeError("use a direct HTTP or HTTPS stream URL without credentials or a fragment")


def preset_name(value: str) -> str:
    name = value.strip()
    if 1 <= len(name) <= 64 and name.isprintable():
        return name
    raise argparse.ArgumentTypeError("use a name of 1–64 printable characters")


def hardware_matches(slot: int, preset: JsonObject, physical: tuple[str | None, str | None] | None) -> bool:
    if not physical:
        return False
    source, location = physical
    kind = preset.get("type")
    if kind == "native":
        item = preset.get("native")
        return isinstance(item, dict) and source == item.get("source") and location == item.get("location")

    if source == "LOCAL_INTERNET_RADIO" and location and "/station?data=" in location:
        data = unquote(location.split("/station?data=", 1)[1])
        try:
            station = json.loads(base64.b64decode(data + "=" * (-len(data) % 4), altchars=b"-_", validate=True))
        except (ValueError, RecursionError):
            return False
        location = station.get("streamUrl") if isinstance(station, dict) else None
    elif source != "UPNP":
        return False

    path = f"/spotify/stream-{slot}.ogg" if kind == "spotify" else f"/stream/{slot}"
    return location == f"http://127.0.0.1:8888{path}"


def verify_hardware(speaker: Speaker, stored: PresetMap, slots: range | tuple[int, ...] = SLOTS) -> list[int]:
    physical = speaker.physical()
    mismatches = []
    for slot in slots:
        if slot in stored:
            if not hardware_matches(slot, stored[slot], physical.get(slot)):
                mismatches.append(slot)
        elif slot in physical:
            mismatches.append(slot)
    return mismatches


def backup_text(presets: PresetMap) -> str:
    return json.dumps({"format": BACKUP_VERSION, "presets": list(presets.values())}, indent=2) + "\n"


def save_backup(path: Path, presets: PresetMap) -> None:
    data = backup_text(presets)
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w", encoding="utf-8") as file:
        file.write(data)


def load_backup(path: Path) -> PresetMap:
    with path.open("rb") as file:
        raw = file.read(MAX_BACKUP + 1)
    if len(raw) > MAX_BACKUP:
        raise RadioError("backup exceeds the size limit")
    try:
        backup = json.loads(raw)
    except (ValueError, RecursionError):
        raise RadioError("backup is not valid JSON") from None
    if not isinstance(backup, dict) or type(backup.get("format")) is not int or backup["format"] != BACKUP_VERSION:
        raise RadioError("unsupported backup format")
    entries = backup.get("presets")
    if not isinstance(entries, list) or len(entries) > len(SLOTS):
        raise RadioError("invalid backup preset list")
    slots: PresetMap = {}
    for entry in entries:
        slot = entry.get("slot") if isinstance(entry, dict) else None
        if type(slot) is not int or slot not in SLOTS or slot in slots:
            raise RadioError("backup has an invalid or duplicate physical slot")
        if not isinstance(entry.get("name"), str) or not isinstance(entry.get("type"), str):
            raise RadioError(f"backup is missing name or type for slot {slot}")
        slots[slot] = entry
    return slots


def confirm(message: str, assume_yes: bool) -> None:
    if assume_yes:
        return
    try:
        answer = input(f"{message} [y/N] ")
    except EOFError:
        answer = ""
    if answer.strip().casefold() not in ("y", "yes"):
        raise RadioError("cancelled")


def set_preset(speaker: Speaker, args: argparse.Namespace) -> None:
    payload = {"slot": args.slot, "type": "radio", "stream_url": args.url, "name": args.name}
    if args.codec:
        payload["codec"] = args.codec
    old = speaker.stored().get(args.slot)
    physical = speaker.physical().get(args.slot)
    old_name = clean(old.get("name")) if old and isinstance(old.get("name"), str) else None
    if old_name:
        message = f"Replace slot {args.slot} ({old_name})"
    elif old or physical:
        message = f"Replace the existing entry on physical slot {args.slot}"
    else:
        message = f"Set physical slot {args.slot}"
    confirm(f"{message} with {args.name}?", args.yes)
    if speaker.stored().get(args.slot) != old or speaker.physical().get(args.slot) != physical:
        raise RadioError("the slot changed during confirmation. Read it again before overwriting")
    speaker.request("PUT", f"/api/presets/{args.slot}", payload)
    current = speaker.stored()
    if any(current.get(args.slot, {}).get(key) != val for key, val in payload.items()):
        raise RadioError("STR read-back of the saved station differs from the requested mapping")
    if verify_hardware(speaker, current, (args.slot,)):
        raise RadioError(f"STR stored slot {args.slot}, but the Bose key did not verify")
    print(f"Verified slot {args.slot}: {args.name}")


def restore(speaker: Speaker, desired: PresetMap, path: Path, assume_yes: bool) -> None:
    current = speaker.stored()
    physical = speaker.physical()
    to_clear = (current.keys() | physical.keys()) - desired.keys()
    writes = [item for slot, item in desired.items() if item != current.get(slot) or
              not hardware_matches(slot, item, physical.get(slot))]
    if not to_clear and not writes:
        print("The six keys already match the backup.")
        return
    # A hardware-only entry cannot be recreated through the verified STR
    # restore endpoints. Preserve it unless the user changes it individually.
    unmanaged = (physical.keys() - current.keys()) & (to_clear | {item["slot"] for item in writes})
    if unmanaged:
        slots = ", ".join(map(str, sorted(unmanaged)))
        raise RadioError(f"restore would overwrite hardware-only slots {slots}. Use explicit per-slot set to replace them")
    confirm(f"Restore the six physical keys from {path}?", assume_yes)
    if speaker.stored() != current or speaker.physical() != physical:
        raise RadioError("the presets changed during confirmation. Take a fresh backup and retry")

    # STR bulk PUT is atomic for saved slots, but clearing omitted keys is a
    # separate API operation. Keep a private recovery copy for partial changes.
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", dir=path.parent, prefix=f"{path.stem}.before-restore-",
        suffix=".json", delete=False,
    ) as recovery:
        recovery.write(backup_text(current))
    print(f"Previous STR presets saved to {recovery.name}", flush=True)

    while writes:
        try:
            speaker.request("PUT", "/api/presets", writes)
            break
        except HttpError as exc:
            # A station can have moved to a slot that this restore will empty.
            # The upstream duplicate check names that exact blocking slot.
            if exc.status != 409 or exc.code != "already-on-slot" or exc.slot not in to_clear:
                raise
            if speaker.stored().get(exc.slot) != current.get(exc.slot):
                raise RadioError(f"slot {exc.slot} changed during restore")
            speaker.clear(exc.slot)
            to_clear.remove(exc.slot)
    for slot in sorted(to_clear):
        if speaker.stored().get(slot) != current.get(slot):
            raise RadioError(f"slot {slot} changed during restore")
        speaker.clear(slot)

    restored = speaker.stored()
    if restored != desired:
        raise RadioError("restore read-back differs from the backup. Check the stored mappings")
    mismatches = verify_hardware(speaker, restored)
    if mismatches:
        raise RadioError(f"Bose key registration differs in slots {', '.join(map(str, mismatches))}")
    print("Verified all six restored keys.")


def parser_for_cli() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="soundtouch-radio", description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    discover = commands.add_parser("discover", help="browse SoundTouch mDNS advertisements")
    discover.add_argument("--json", action="store_true")
    info = commands.add_parser("info", help="inspect Bose hardware, firmware and STR status")
    info.add_argument("--host", required=True, type=lan_ip)
    info.add_argument("--json", action="store_true")
    presets = commands.add_parser("presets", help="manage the six physical keys")
    operations = presets.add_subparsers(dest="operation", required=True)
    for operation in ("list", "backup", "set", "verify", "restore"):
        command = operations.add_parser(operation)
        command.add_argument("--host", required=True, type=lan_ip)
        if operation == "list":
            command.add_argument("--json", action="store_true")
        if operation in ("backup", "restore"):
            command.add_argument("--file", type=Path, default=Path("presets.json"), required=operation == "restore")
        if operation in ("set", "restore"):
            command.add_argument("--yes", action="store_true", help="explicitly authorize the write without prompting")
        if operation == "set":
            command.add_argument("--slot", type=int, choices=SLOTS, required=True)
            command.add_argument("--name", type=preset_name, required=True)
            command.add_argument("--url", type=stream_url, required=True)
            command.add_argument("--codec", choices=("MP3", "AAC", "AAC+"))
    return parser


def main(argv: list[str] | None = None) -> int:
    args = parser_for_cli().parse_args(argv)
    if args.command in ("info", "discover"):
        probe_args = ["--discover"] if args.command == "discover" else ["--host", args.host]
        return probe_main(probe_args + (["--json"] if args.json else []))
    try:
        desired = load_backup(args.file) if args.operation == "restore" else None
        speaker = Speaker.connect(args.host)
        if args.operation == "set":
            set_preset(speaker, args)
        elif desired is not None:
            restore(speaker, desired, args.file, args.yes)
        else:
            stored = speaker.stored()
            if args.operation == "backup":
                save_backup(args.file, stored)
                print(f"Backed up {len(stored)} STR presets to {args.file}")
            elif args.operation == "verify":
                mismatches = verify_hardware(speaker, stored)
                if mismatches:
                    raise RadioError(f"Bose key registration differs in slots {', '.join(map(str, mismatches))}")
                print(f"Verified all six Bose keys ({len(stored)} assigned).")
            elif args.json:
                print(backup_text(stored), end="")
            else:
                for slot in SLOTS:
                    preset = stored.get(slot)
                    name = clean(str(preset.get("name") or "(unnamed)")) if preset else "(empty)"
                    print(f"{slot}. {name}")
    except FileExistsError:
        print("Error: backup path already exists. Choose another file.", file=sys.stderr)
        return 1
    except (OSError, ProbeError, RadioError) as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
