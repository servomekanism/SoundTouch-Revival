# Independent playback verification

## Preliminary playback check on 2026-11-21

The speaker played ERT's Kosmos HTTPS MP3 stream through STR in an ad hoc test. Its Bose API reported `LOCAL_INTERNET_RADIO` and `PLAY_STATE`, and STR reported 256 kbps with no stream error. The owner heard the station. The stop request returned `stopped` and the Bose API then reported `STOP_STATE`. The test left both preset stores empty. [ERT Kosmos](https://www.ertecho.gr/radio/kosmos/)

## Physical button and cold-start checks

The owner approved writing the six original stations in order. The manager applied them in one bulk write, matched the read-back to the submitted mappings, and verified the six Bose key registrations. The owner tested all six physical buttons and reported that all tests passed. The pre-power-cycle mappings were saved privately to `backups/str-presets-configured.json`.

| Button | Expected station | Verified source | Initial playback | Linux and phone off | After speaker power cycle |
| --- | --- | --- | --- | --- | --- |
| 1 | Real FM | [Real FM 97.8](https://realfm.live24.gr/realfm), HTTPS MP3 | Pass | Pass | Pass |
| 2 | ERA Deytero | [ERT Deftero](https://www.ertecho.gr/radio/deftero/), HTTPS MP3 | Pass | Pass | Pass |
| 3 | The Rebel | [WRBF 104.9, Georgia](https://listen.streamon.fm/wrbf), HTTPS AAC | Pass | Pass | Pass |
| 4 | ERA 3 Trito | [ERT Trito](https://www.ertecho.gr/radio/trito/), HTTPS MP3 | Pass | Pass | Pass |
| 5 | ERA Open | [ERT Open, Attica feed](https://www.ertopen.com/radio), HTTP MP3 | Pass | Pass | Pass |
| 6 | ERA Kosmos | [ERT Kosmos](https://www.ertecho.gr/radio/kosmos/), HTTPS MP3 | Pass | Pass | Pass |

- [x] Read back the STR assignments and compare them with the intended stream URLs.
- [x] Confirm the intended station plays from each physical button.
- [x] Repeat with the Linux configuration computer and phone unavailable. The home router and Internet connection remain on.
- [x] Cold-start the speaker while the configuration devices are still unavailable and repeat all six buttons.
- [x] Confirm the speaker uses Wi-Fi. After the offline tests, STR reported an active Wi-Fi profile and no Ethernet uplink. All six assignments use public stream endpoints. No router packet capture was taken.
- [x] Bring Linux back online and read back the presets. All six Bose registrations passed, and every STR mapping matched the pre-power-cycle backup exactly.

**Acceptance passed on 2026-11-21.** The six presets worked after a cold start with the Linux configuration computer and phone unavailable. STR v1.8.2 was reachable over Wi-Fi afterward, and the Bose firmware remained `27.0.6.46330.5043500`.
