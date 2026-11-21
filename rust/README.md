# SoundTouch Radio in Rust

The workspace builds `soundtouch-radio` for terminal use and `soundtouch-radio-desktop` for the Iced editor. Both use the same local STR client and JSON backup format as the Python app. Your SoundTouch streams independently once you save its presets.

## Build and run

Use a current stable Rust toolchain. The workspace was developed with Rust 1.95. The target platforms are Windows x64, Linux x64, and Apple Silicon macOS. The desktop app uses software rendering. On Linux it needs an X11 or Wayland session, and the backup dialogs need an XDG desktop portal with a file chooser backend. The CLI also works over SSH.

From the repository root, build both applications with the checked-in lockfile.

```sh
cargo build --manifest-path rust/Cargo.toml --workspace --release --locked
```

The binaries are in `rust/target/release/`, with `.exe` extensions on Windows. You can also build and start the desktop app with one command. Use `--release` for responsive software rendering, especially in a VM.

```sh
cargo run --manifest-path rust/Cargo.toml -p soundtouch-radio-desktop --release --locked
```

## Edit the buttons

1. Enter the speaker's IPv4 address or choose a result from **Search LAN**.
2. Click **Connect & load presets**. The app checks the STR agent, reads the six saved assignments, and compares them with the Bose button registrations. The whole attempt has a 10-second limit. Click **Cancel connection** to stop waiting and retry immediately. Invalid LAN addresses get an immediate error.
3. Open any numbered row to edit its name, direct stream URL, or codec. Use **Clear slot** to empty a button or **Undo** to reset that row. The app prompts before reloading or switching speakers while you have unsaved edits.
4. Click **Apply changes**, review the affected buttons, then confirm. You can swap two stations by editing both rows before applying. The app saves a recovery copy in the local application data directory before writing and checks both the saved assignments and their button registrations afterward.
5. Press each changed button on the speaker to confirm playback. The app checks URL syntax and passes the batch to STR for validation. The batch API checks the shape of the stream URL and skips the network probes that STR runs on individual saves. [STR v1.8.2 preset handlers](https://github.com/JRpersonal/streborn/blob/v1.8.2/internal/webui/presets_http.go)

**Backup & restore** saves the speaker's current STR mappings or opens a Python-compatible backup for review. If a separate Bose-only preset occupies a button, the desktop labels and protects it. Backups can contain station URLs and metadata. Store them locally and review them before sharing.

## Use the CLI

Replace `192.168.1.50` with your speaker's address. In these examples, `soundtouch-radio` is on your `PATH`. To run from source, replace `soundtouch-radio` with `cargo run --manifest-path rust/Cargo.toml -p soundtouch-radio --locked --`.

```sh
soundtouch-radio discover
soundtouch-radio info --host 192.168.1.50 --json
soundtouch-radio presets list --host 192.168.1.50
soundtouch-radio presets backup --host 192.168.1.50 --file presets.json
soundtouch-radio presets set --host 192.168.1.50 --slot 6 --name "Rebetiko Sealabs" --codec MP3 --url "https://rebetiko.sealabs.net/radio"
soundtouch-radio presets verify --host 192.168.1.50
soundtouch-radio presets restore --host 192.168.1.50 --file presets.json
```

Discovery uses mDNS. Enter the address directly if multicast is blocked by a VM, firewall, or network configuration. The probe and verify commands send GET requests. `set` and `restore` ask for confirmation unless you supply `--yes`. Read the current presets before retrying a write that timed out.

## Test

```sh
cargo test --manifest-path rust/Cargo.toml --workspace --locked
```

Tests use simulated Bose and STR responses. The GitHub Actions workflow defines native build and test jobs for the three target platforms.
