# STR network installation

The assessed device is a SoundTouch 20 with module `sm2`, variant `spotty`, and Bose firmware `27.0.6.46330.5043500`. The approved install uses STR v1.8.2 on that firmware. The desktop app verifies reachability, installs the speaker agent, and restarts the speaker. [Device assessment](compatibility.md) · [STR architecture](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/ARCHITECTURE.md#5-first-install-network-install-the-normal-path)

## Verify and launch the Linux installer

These commands use the Linux x86-64 desktop release and require GTK 3, WebKitGTK 4.1 and libsoup 3. They assume the current directory is a new local directory for the downloaded files. [Release v1.8.2](https://github.com/JRpersonal/streborn/releases/tag/v1.8.2)

```bash
curl -fLO https://github.com/JRpersonal/streborn/releases/download/v1.8.2/SHA256SUMS
curl -fLO https://github.com/JRpersonal/streborn/releases/download/v1.8.2/STR-Linux-x64.tar.gz
awk '$2 == "STR-Linux-x64.tar.gz"' SHA256SUMS | sha256sum -c -
```

After the checksum succeeds, unpack the archive and launch the desktop app.

```bash
tar -xzf STR-Linux-x64.tar.gz
./STR-Linux-x64
```

The archive's `install.sh` adds a local desktop launcher. Launching the release binary directly runs the same speaker installer.

## Install on the existing Wi-Fi connection

1. Save the current preset assignments and the [Bose diagnostic](../README.md) before changing the speaker. The pre-install `/presets` XML for this speaker is in the private `backups/` directory.
2. In **Listen to music**, use **Connect by IP** if discovery misses the speaker. Then go to **Set up speaker**, select the entry by its known IP address, and check that the model is SoundTouch 20.
3. Leave the Wi-Fi fields blank to use the profile already on the speaker. Keep stable power throughout the next step. [Installer Wi-Fi handling](https://github.com/JRpersonal/streborn/blob/v1.8.2/desktop-app/frontend/src/views/setup/install.js#L948)
4. With approval, select **Install STR now**. Wait for the installer to finish copying the agent and restarting the speaker.
5. Run the read-only diagnostic against the speaker's current IP. Check the STR build, Bose firmware, and physical preset list. Physical playback and cold-start tests are separate acceptance checks.

See the [recovery instructions](recovery.md) if the installer reports a failure or the speaker stops responding.

## Observed result on 2026-11-21

- The verified desktop archive SHA-256 was `2cf2ff46038a2c1ad7f80a1da04caf60c1a0d320b67f20294b520e4e30de8146`. The app found the speaker by IP and completed the approved network installation over Wi-Fi.
- After the installer restarted the speaker, Bose `/info` reported the same `27.0.6.46330.5043500` build. STR answered on port `17008` with version `v1.8.2` and build `2026-11-19-1650`. The agent subsequently reported healthy Bose control and no USB stick.
- Bose `/presets` and STR `/api/presets` both read back empty. The original six TuneIn mappings are preserved locally in `backups/bose-presets-before-str.xml`. Original playback and any migration from this backup remain unverified.
- The approved ad hoc Kosmos test reached `PLAY_STATE` with a 256 kbps stream and the owner heard it. STR reported no stream error and the speaker reported `STOP_STATE` after the test. Both preset lists remained empty during this test. After configuring the original six stations, the owner verified all six physical buttons with Linux and the phone unavailable, including after a speaker cold start. All mappings matched their backup after the test. [Verification checklist](verification.md)
