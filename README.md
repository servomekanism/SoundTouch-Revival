# SoundTouch Radio

Configure the six physical internet radio presets on a Bose SoundTouch 20. The speaker runs STR and stores the presets on its own flash. The app connects only while you configure the buttons. [STR architecture](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/ARCHITECTURE.md)

## Implementations

- The Rust workspace builds a CLI and an Iced desktop editor for Windows x64, Linux x64, and Apple Silicon macOS. Both read and write the same backup format as Python. [Rust build and usage guide](rust/README.md)
- The [Python CLI](python/) is available for Linux. The `python-v0.1.0` tag records the original root-level layout.

## Desktop app

You’ll need Rust on your computer and SoundTouch Reborn installed on the speaker. Run the optimized desktop build from the repository root. [SoundTouch Reborn setup](https://github.com/JRpersonal/streborn)

```bash
cargo run --manifest-path rust/Cargo.toml -p soundtouch-radio-desktop --release --locked
```

Enter your speaker IP and click **Connect & load presets**. Typing the address is local, and **Search LAN** starts discovery. Open any of the six rows to edit it, then review and confirm with **Apply changes**. Use **Backup & restore** to save a copy or load a previous setup. [Desktop instructions](rust/README.md#edit-the-buttons)

## Manage presets

Use Python 3.10 or newer. From this directory run `python3 python/soundtouch_radio.py`. To install the `soundtouch-radio` command, install the project in a Python virtual environment with `python -m pip install ./python`. The CLI has no runtime Python dependencies.

Replace `192.168.1.50` with your speaker's IPv4 address.

```bash
python3 python/soundtouch_radio.py discover
python3 python/soundtouch_radio.py info --host 192.168.1.50
python3 python/soundtouch_radio.py presets list --host 192.168.1.50
python3 python/soundtouch_radio.py presets backup --host 192.168.1.50 --file presets.json
python3 python/soundtouch_radio.py presets set --host 192.168.1.50 --slot 6 \
    --name "ERA Kosmos" --codec MP3 --url "https://radiostreaming.ert.gr/ert-kosmos"
python3 python/soundtouch_radio.py presets verify --host 192.168.1.50
python3 python/soundtouch_radio.py presets restore --host 192.168.1.50 --file presets.json
```

`set` and `restore` prompt for confirmation. Use `--yes` only when you intend to apply the write. `set` reads back the station and checks the Bose hardware registration. `verify` compares all six registrations with the STR store. A passing registration check still needs an audible button test. `list --json` emits the full STR mappings, which can include private URLs and account metadata.

Backups store the STR data for slots 1–6 and leave slots 7–12 outside the operation. The file is private and the backup command refuses to overwrite an existing file. `restore` checks the full six-key state, saves a private copy beside the input file, and verifies the result. STR bulk saves and slot clears are separate operations. If the connection drops, read back the current state before retrying, using the saved copy if needed. Restore preserves Bose-only entries it cannot recreate through STR. The original Bose XML backup is a different format.

Use the station's direct HTTP or HTTPS stream URL. The CLI validates syntax and the pinned STR release rejects URLs it identifies as web pages. Set `--codec AAC` or `--codec AAC+` where the station uses that codec. STR can proxy MP3 and AAC and can convert HLS with MPEG-TS segments carrying supported audio. The live test here used HTTPS MP3. Compatibility for other streams depends on the station and should be confirmed by playback. Use public station sources to keep playback independent of other local devices. [STR control API](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/CONTROL-API.md) · [STR stream proxy design](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/ARCHITECTURE.md)

## Six-station example

This setup assigns Trito Programma, Deftero Programma, ERA Kosmos, WRBF 104.9 The Rebel, Trito Maestro, and Rebetiko Sealabs to buttons 1–6 respectively.

Replace `192.168.1.50` with your speaker's IP. Run the commands one at a time in the order shown. Confirm each prompt and wait for `Verified slot` before continuing. If STR reports that a station is already assigned, free its old button before assigning it to a new one.

```bash
python3 python/soundtouch_radio.py presets set --host 192.168.1.50 \
    --slot 6 --name "Rebetiko Sealabs" --codec MP3 --url "https://rebetiko.sealabs.net/radio"

python3 python/soundtouch_radio.py presets set --host 192.168.1.50 \
    --slot 3 --name "ERA Kosmos" --codec MP3 --url "https://radiostreaming.ert.gr/ert-kosmos"

python3 python/soundtouch_radio.py presets set --host 192.168.1.50 \
    --slot 4 --name "Rebel" --codec AAC --url "https://ais-sa1.streamon.fm/7047_48k.aac"

python3 python/soundtouch_radio.py presets set --host 192.168.1.50 \
    --slot 1 --name "Trito Programma" --codec MP3 --url "https://radiostreaming.ert.gr/ert-trito"

python3 python/soundtouch_radio.py presets set --host 192.168.1.50 \
    --slot 2 --name "Deftero Programma" --codec MP3 --url "https://radiostreaming.ert.gr/ert-deftero"

python3 python/soundtouch_radio.py presets set --host 192.168.1.50 \
    --slot 5 --name "Trito Maestro" --codec MP3 --url "https://radiostreaming.ert.gr/ert-trito-maestro"
```

## Run the diagnostic

Run these commands from this directory on a Linux computer connected to the speaker's home network. Use Python 3.10 or newer. Direct probing uses only the Python standard library.

```bash
python3 python/soundtouch_probe.py --discover
```

Discovery uses `avahi-browse` and a running `avahi-daemon` on the Linux computer. On Debian or Ubuntu, `sudo apt install avahi-utils` installs the browse utility. If discovery finds no advertisements or mDNS is blocked, use the IPv4 address from your router's connected-device list. [Bose IP address guide](https://support.bose.com/s/article/st20-speakerwireless-determining-the-ip-address---ka08c000001pxt9aam?language=en_US)

Replace the example address with the speaker's private or link-local IPv4 address.

```bash
python3 python/soundtouch_probe.py --host 192.168.1.50
python3 python/soundtouch_probe.py --host 192.168.1.50 --json > soundtouch-diagnostic.json
```

The report shows the model, module type, variant, firmware and component versions when supplied, the Bose API's physical preset list, and the STR agent version if detected. The JSON omits the target address, friendly name, serial numbers, MAC addresses, account fields, and preset URLs. Review preset titles before sharing the file. It is a diagnostic summary and contains too little information to restore presets.

A direct probe exits with status 0 when Bose `/info` identifies the device, and 1 when it cannot. Check the endpoint errors for partial results, including an unreadable preset list. Discovery exits with status 1 if it finds no candidates. Advertisements are candidates, and an unresponsive STR API leaves installation status unknown.

## Network operations

| Operation | Verified source |
| --- | --- |
| HTTP GET `:8090/info` and `:8090/presets` | [Bose SoundTouch Web API v1.1, sections 12.13 and 12.14](https://assets.bosecreative.com/m/496577402d128874/original/SoundTouch-Web-API.pdf#page=21) |
| HTTP GET `:8888/api/agent/version`, falling back to `:17008/api/agent/version` if needed | [STR local control API](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/CONTROL-API.md) |
| mDNS browse for `_soundtouch._tcp`, `_bose-soundtouch._tcp`, `_streborn._tcp`, and `_soundtouchstick._tcp` in `.local` | [STR discovery source](https://github.com/JRpersonal/streborn/blob/v1.8.2/discovery/discovery.go) |

The probe uses direct HTTP sockets with a four-second socket timeout and caps each body at 256 KiB. It rejects redirects and XML document type declarations. It tries UTF-8 first and falls back to Latin-1 for older firmware that emits names in that encoding. STR detection requires JSON with its documented version and build fields. The interface uses plain HTTP, so the match is local API evidence. [Upstream XML handling](https://github.com/JRpersonal/streborn/blob/v1.8.2/internal/boxapi/boxapi.go)

## Test

```bash
(cd python && python3 -B -m unittest discover -s tests -v)
cargo test --manifest-path rust/Cargo.toml --workspace --locked
```

The tests use mocked HTTP and mDNS responses.

## References and credits

- Thanks to **JRpersonal and the SoundTouch Reborn contributors** for building and documenting the speaker-side agent that makes standalone presets possible. The client targets STR v1.8.2. [SoundTouch Reborn](https://github.com/JRpersonal/streborn) · [v1.8.2 release](https://github.com/JRpersonal/streborn/releases/tag/v1.8.2) · [Control API](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/CONTROL-API.md)
- Thanks to the **Rebetiko Sealabs archive** for making its recordings available through its online radio station. [Rebetiko Sealabs radio](https://rebetiko.sealabs.net/radiofono)
