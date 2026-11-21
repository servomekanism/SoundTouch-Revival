# SoundTouch 20 compatibility and recovery assessment

**STR v1.8.2 was installed over Wi-Fi after explicit approval.** The [installation record](installation.md#observed-result-on-2026-11-21) contains the post-restart checks. The results below record the pre-install baseline and recovery assessment.

## Live diagnostic on 2026-11-21

| Check | Result |
| --- | --- |
| Bose model | SoundTouch 20 |
| Hardware | `sm2`, `spotty`, `normal`, with `SCM` and `PackagedProduct` components |
| Retail generation | Series III according to the pinned STR firmware matcher's ST20 fixture. The Bose API returns the generic SoundTouch 20 label. The case label has yet to be read. [STR firmware matcher test](https://github.com/JRpersonal/streborn/blob/v1.8.2/desktop-app/bosefw/bosefw_test.go#L80) |
| Bose firmware | `27.0.6.46330.5043500`, build stamp `epdbuild.trunk.hepdswbld04.2022-08-04T11:20:29`. Already at STR's target. |
| STR detection | No matching agent response. Port 8888 refused the connection and port 17008 timed out. A stopped or unreachable agent remains possible. |
| Network access | Direct HTTP from the NATed Linux VM worked after sandbox network approval. Desktop discovery and the installer's maintenance connection remain untested from the VM. |

The Bose API returned six TuneIn assignments, in slot order.

1. Real FM
2. ERA Deytero
3. The Rebel
4. ERA 3 Trito
5. ERA Open
6. ERA Kosmos

Capture the raw Bose preset mapping privately before running the installer. These titles document the assignments, and recovery of the original preset mappings or their cloud-era playback remains uncertain. [Bose end-of-life notice](https://www.bose.com/soundtouch-end-of-life-row)

## Device-specific install and recovery decision

1. Use the upstream desktop network installer pinned to v1.8.2. Install only the STR agent on the already-matching Bose build. Back up the original preset data and verify the release artifact against the published SHA-256 list before running it. Use a trusted LAN and stable power. If discovery fails in the NATed VM, use the desktop app from a computer directly on the home LAN. [Pinned release](https://github.com/JRpersonal/streborn/releases/tag/v1.8.2) · [STR network installation](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/ARCHITECTURE.md#5-first-install-network-install-the-normal-path)
2. If the unit still boots but Wi-Fi access is lost, use Ethernet to regain LAN access and reassess its state. For a working boot and reachable maintenance path, the upstream app's **Speaker Settings → Remove STR** is the first uninstall option. If normal network access fails but the speaker still mounts USB at boot, STR documents its prepared USB stick as a fallback. The uninstall workflow can remove STR's boot hook and files. Bose preset restoration is a separate question. [STR removal workflow](https://github.com/JRpersonal/streborn/blob/v1.8.2/desktop-app/frontend/src/views/settings/actions.js#L312) · [STR USB fallback](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/ARCHITECTURE.md#5b-first-install-usb-stick-fallback-and-recovery)
3. If Bose firmware repair is needed, Bose documents three Series III update paths that use a local file: USB-A stick, USB Micro-B service cable, and the speaker's Setup Wi-Fi. Use the applicable route only if its updater is reachable. The matching Bose SM2 image was downloaded to the VM's `/tmp/soundtouch-st20-sm2-27.0.6.stu` and checked on 2026-11-21. It matched the catalogue values of 99,445,800 bytes and CRC32 `536e6d6f`. The locally computed SHA-256 is `ecd3f316ad53292b9841c94e2f81cfa15ef81dc12ad79376514ea1d130ee61e2`. The same-version image is available for recovery assessment. Whether it clears STR changes or recovers a failed boot on this unit remains unverified. [Bose Series III update instructions](https://support.bose.ca/en-CA/article/soundtouch-20-iii-updating-the-software-or-firmware-of-your-product) · [Bose port documentation](https://support.bose.ca/en-CA/article/st20iii-speakerwireless-understanding-the-rear-inputs---ka08c000000hgkvaae) · [Bose SM2 firmware image](https://downloads.bose.com/ced/soundtouch/downloads_stockholm/stu/s/sm2/Update.stu)

If the speaker loses both network access and its USB boot or update path, there is no verified recovery procedure for this unit.

## STR review on 2026-11-21

- The approved release is **v1.8.2**, built from commit `52b3b45615f6c2025cb06d9ef42bb7122b433658`. The installer advertised **v1.8.3** during setup. Its release notes cover multiroom, playback, and UI changes. The install remains pinned to the approved release. [Release v1.8.2](https://github.com/JRpersonal/streborn/releases/tag/v1.8.2) · [Release v1.8.3](https://github.com/JRpersonal/streborn/releases/tag/v1.8.3)
- Upstream lists both ST20 module variants as working. The older `scm` unit was contributor-confirmed with final stability confirmation pending. The `sm2` variant was user-confirmed. Both can report `SoundTouch 20`, codename `spotty`, and the same final firmware build. `moduleType` identifies the module. `Lightswitch` and `SMSC` components support an older-module identification when present. The component named `SCM` carries the Bose firmware version on both variants. Upstream's internal Series-II Wi-Fi family label describes the module and does not identify the retail generation. [STR model support](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/MODELS.md) · [STR model fingerprints](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/MODEL-VARIANTS.md)
- STR targets Bose firmware **27.0.6.46330.5043500**. Upstream reports instability on one `sm2` ST20 running 27.0.3, with the cause unresolved. The firmware guide documents a staged path for some older Series I units and calls out unverified steps. Establish the exact starting build and matching route before considering an update. [STR firmware guide](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/FIRMWARE-UPDATE.md) · [STR model fingerprints](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/MODEL-VARIANTS.md)
- STR meets the proposed architecture in upstream design. Its boot hook and preset store live on the speaker's persistent storage. The speaker agent handles button events and fetches/proxies radio itself. The Linux client can be closed after configuration. Playback, Wi-Fi, and persistence across a cold start still need verification on this unit and with the chosen streams. [STR architecture](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/ARCHITECTURE.md) · [STR control API](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/CONTROL-API.md)
- The issue tracker lists an open BCO Wi-Fi setup issue and a preset sweep interacting with AirPlay. An ST20 owner also reported intermittent radio playback and preset failures in a thread that primarily discusses Spotify. These reports inform the Wi-Fi, playback, and existing-function regression checks. [Issue tracker](https://github.com/JRpersonal/streborn/issues) · [BCO Wi-Fi #1573](https://github.com/JRpersonal/streborn/issues/1573) · [AirPlay #1035](https://github.com/JRpersonal/streborn/issues/1035) · [ST20 report #1768](https://github.com/JRpersonal/streborn/issues/1768)

## Installation and recovery paths to assess

| Path | Evidence and limits |
| --- | --- |
| STR desktop network installer | Upstream's primary first-install path. It uses the stock maintenance interface, temporarily uses SSH, writes a persistent agent and boot hook, and reboots the speaker. Confirm eligibility on the exact module and firmware first. [Architecture](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/ARCHITECTURE.md#5-first-install-network-install-the-normal-path) |
| STR USB fallback | Upstream documents a prepared USB stick for fallback and recovery where the speaker still mounts the stick during boot. A broken boot or unavailable maintenance path can prevent recovery. [USB architecture](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/ARCHITECTURE.md#5b-first-install-usb-stick-fallback-and-recovery) |
| STR desktop uninstall | The upstream routine removes the agent and its boot changes, with a separate factory-reset path for clearing Bose settings. Uninstall depends on reaching a working maintenance path. Confirm what happens to existing presets and Wi-Fi separately before using it. [Architecture](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/ARCHITECTURE.md) · [Uninstall source](https://github.com/JRpersonal/streborn/blob/v1.8.2/desktop-app/uninstall_str.go) |
| Original Bose firmware | Bose documents manual updates. Both Bose-hosted 27.0.6 ST20 files returned HTTP 200 to HEAD requests at the expected sizes. The matching SM2 file was subsequently downloaded and checked as described above. The older-module file is for the `scm` hardware. [STR Bose file catalogue](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/FIRMWARE-UPDATE.md#9-boses-files-for-2706-per-model) |

A power interruption during a firmware or boot-hook write can leave the speaker unable to start normally. An incorrect variant image or unsupported version jump can fail the update or leave the unit unusable. A failed install can make the speaker unreachable over Wi-Fi. Assess whether Ethernet and USB are physically available and which recovery path still works if normal boot or Wi-Fi fails. There is no confirmed unbrick procedure for this unit. [STR firmware guide](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/FIRMWARE-UPDATE.md) · [STR model support](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/MODELS.md)

Bose and STR expose unauthenticated LAN services. STR's threat model also describes a LAN-accessible way to enable passwordless root SSH, and an older BCO firmware interface can disclose the stored Wi-Fi passphrase to LAN callers. Keep the speaker's management interfaces on a trusted network without WAN port forwards. [STR threat model](https://github.com/JRpersonal/streborn/blob/v1.8.2/docs/THREAT-MODEL.md)

The owner confirmed physical access to the speaker. If recovery becomes necessary, an Ethernet connection and a USB drive or Micro-B service cable are the documented local options. If the case label disagrees with the inferred retail series, resolve the discrepancy before any firmware recovery. Any further update, reboot, or preset write requires separate explicit approval. [Bose Series III label guide](https://support.bose.ca/article/st20iii-speakerwireless-serial-number-location---ka08c000001budwaaq)
