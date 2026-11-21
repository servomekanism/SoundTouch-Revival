# SoundTouch Radio in Rust

The workspace contains the initial layout for the Rust port.

- `core/` will own speaker discovery, the Bose and STR APIs, preset operations, and JSON backup compatibility.
- `cli/` will expose the `soundtouch-radio` command for terminal and SSH use.
- `desktop/` will contain the Iced desktop app for Windows x64, Linux x64, and Apple Silicon macOS.

Run `cargo check --workspace --locked` from this directory to check the workspace. The command-line and desktop entry points are placeholders. Use the [Python CLI](../README.md#manage-presets) to manage a speaker today.
