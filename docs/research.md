# Research notes

Summary of what existing projects established about Insta360 Link cameras
on Linux, what was verified on the development Insta360 Link 2, and which
findings `linkctl` relies on. Nothing here is an official Insta360 API.

## Ground truth from the development camera

Insta360 Link 2, USB `2e1a:4c04`, Linux 7.1.9, `uvcvideo`.

| item | observation |
|------|-------------|
| nodes | `/dev/video0` (video capture + controls), `/dev/video1` (metadata only), `/dev/media0` |
| pan_absolute | `-522000..522000`, step `3600`, SET works and physically moves the gimbal |
| tilt_absolute | `-324000..360000`, step `3600`, SET works |
| zoom_absolute | `100..400` (1.0x..4.0x) |
| focus | `focus_absolute 0..100` (flagged *inactive* while `focus_automatic_continuous`=1) |
| white balance | `white_balance_automatic`, `white_balance_temperature 2000..10000`, tested at 3650 K |
| image | brightness/contrast/saturation/sharpness `0..100`, hue `-15..15`, power_line_frequency menu |
| pan_speed / tilt_speed | present but `disabled` (uvcvideo marks them non-compliant) |
| formats | MJPEG and H.264 at 720p..4K, 24/25/30 fps; no raw YUV |
| extension units | 9 `FAF1672D-…`, 10 `E307E649-…`, 11 `A8BD5DF2-…`, each advertising 11 controls |
| XU 11 / 0x02 | `GET_LEN` = 1, `GET_CUR` = `0x00` while tracking is off |
| XU 9 / 0x02 | `GET_LEN` = **60** (not 61 as documented by csmarshall); byte 0 = `0x00` (normal) |
| activity | nothing holds the nodes open while idle; PipeWire does not keep them open |
| permissions | `/dev/video0` is `root:video 0660` plus a logind ACL for the seat user |

Standard V4L2 pan/tilt SET works on this camera. Reports to the contrary in
other projects are noted below but were not reproduced.

## Reference projects

### fmontes/insta360-link-cli — MIT

macOS, Link 2. The most directly relevant Link 2 source.

* Extension unit GUIDs read from the USB descriptor:
  * 9 `FAF1672D-B71B-4793-8C91-7B1C9B7F95F8` (device info/status, author's guess)
  * 10 `E307E649-4618-A3FF-82FC-2D8B5F216773` (image/AE, guess)
  * 11 `A8BD5DF2-1A98-474E-8DD0-D92672D194FA` (AI features)
* **AI tracking: unit 11, selector 0x02, 1 byte, `01` on / `00` off.**
  Found by differential register scanning (dump all selectors with the
  feature off, toggle in the official app, dump again, diff, discard registers
  that drift on their own). Verified by writing.
* Pan/tilt/zoom use standard UVC controls; units 1/3600°, step 3600 = 1°;
  descriptor claims ±180° but real travel is ±145° pan, −90..+100° tilt.
* Writes from scratch, no GET_LEN/GET_INFO. No hang reports.

### csmarshall/link-ctl — MIT

Python, macOS + Linux, all Link models. `docs/LINK2_LINUX.md` is the most
detailed Linux write-up.

* Uses `UVCIOC_CTRL_QUERY` for units 9/10/11 and, optionally, libusb with
  kernel-driver detach for camera-terminal requests. Detach is opt-in
  because of hangs.
* **Unit 9 / 0x02 "AI mode", 61 bytes on Link 2** (52 on the original Link).
  Bytes 0–1: normal `00 00`, tracking `01 00`, whiteboard `04 01`,
  overhead `05 03`, deskview `06 10`. Byte 0 reads back `0xFF` while idle
  or in transition. The write protocol is fragile: the camera must be
  streaming, the state must be polled until it leaves `0xFF`, "normal" is
  written first, then the target, re-asserted every few seconds, and a
  hybrid read-modify-write (zero bytes 0..51, keep bytes 52..60) is
  required because both a full RMW and a full zero-fill misbehave.
* Unit 9 / 0x1B "function enable" bitmask, 2 bytes LE: bit 2 HDR, bit 3
  mirror, bit 4 gesture zoom, bit 11 privacy (Link 2).
* Unit 10 / 0x0F privacy, 2 bytes (`0x0002` on); readback unreliable.
* Unit 9 selectors 0x09 exposure comp, 0x13 framing (1 head / 2 half /
  3 whole), 0x19 ISO, 0x1A pan/tilt readback (int32 LE tilt,pan; stale on
  Link 2), 0x1D shutter, 0x1E AE mode.
* Claims V4L2 `pan_absolute`/`tilt_absolute` SET fails on Link 2 and uses
  libusb instead. **Not reproduced on the development camera.**
* Hang reports: repeated detach/rebind loops and rapid AI-mode writes leave
  the firmware stuck (device enumerated, no `/dev/video*`); `libusb_reset_device`
  can leave AI mode stuck; recovery = unbind/bind `uvcvideo` as root or replug.

### fugisawa/insta360-link-ctl — MIT

Python (stdlib only), Linux, original Link (`2e1a:4c01`, fw 1.4.3.8).

* Uses `UVCIOC_CTRL_QUERY` via `fcntl.ioctl` on the video node; never
  detaches the driver. Resolves unit ids by parsing extension-unit GUIDs
  from the sysfs `descriptors` file — the approach `linkctl` adopted.
* Same GUID→unit mapping as above (9 info, 10 image, 11 ai).
* **Unit 11 / 0x02 AI tracking, 1 byte, `01`/`00` — confirmed** (gimbal
  locks onto the subject).
* Unit 9 / 0x02 mode payload is 52 bytes on the original Link; RMW of bytes
  2+.
* Unit 9 / 0x03 device info (170 bytes: serial, UUID, firmware string),
  0x0E gimbal reset (1 byte, write-only), 0x13 framing, 0x14 detection box
  (240 bytes, read-only, 4 floats), 0x1C/0x1D stream telemetry.
* Important caveat: **vendor writes are silently reverted by the firmware
  within about a second unless video is streaming.** This is why `linkctl`
  applies the activity guard to `tracking on/off` and reads the value back.
* V4L2 pan/tilt/zoom work; `pantilt_relative` returns EIO and is disabled
  by uvcvideo.

### insta360-link-rs (tangled.org/mara.x0f.nl) — MIT per Cargo.toml, no LICENSE file

Rust, Linux, CLI + egui GUI, Link and Link 2.

* Uses the `v4l` crate for controls and `nix::ioctl_readwrite!` for
  `UVCIOC_CTRL_QUERY`; no libusb.
* Discovers devices by opening every `/dev/video*` and matching the card
  name; picks the unit id by probing 9/10/11/4/3/6 rather than by GUID.
* Mode writes are built from a zeroed buffer (not RMW), no read-back.
  Link 2 support is an untested port of a Pascal application.
* Useful ideas taken: EIO-tolerant control enumeration, GET_LEN-driven
  payload sizing. Not taken: scanning selectors 1..20 on every open.

### jfwoods/insta360link-controller — no license

C daemon + `linkctl` client for macOS, original Link only. Used for UX ideas
only (no code reused): `status`, `center`, `zoom`, and a jog mode with
W/S/A/D, +/- step, C center, Q quit. Its unit-9 selectors 0x16 (relative
pan/tilt) and 0x1A (gimbal center) are original-Link specific.

## Decisions taken

1. **PTZ and image controls use standard V4L2.** Verified on hardware;
   no vendor path is needed or implemented for them.
2. **Tracking uses unit 11 / selector 0x02 (1 byte).** Two independent
   projects confirm it (one on Link 2), the payload is fully understood,
   it is readable, reversible and does not require the multi-step protocol
   of the unit-9 mode payload. The unit id is confirmed against the GUID in
   the USB descriptors before every write.
3. **The unit-9 AI mode payload is read-only** in `linkctl` (for `info`).
   Its length differs between firmwares (60 vs 61) and the documented write
   protocol is fragile enough that a mis-step is a plausible cause of the
   reported hangs. DeskView/Whiteboard/Overhead therefore remain future work.
4. **No libusb, no driver detach.** Every reported hang involves one of
   those.
5. **Unit ids are validated by GUID, never assumed.**

## Not yet implemented (needs hardware validation first)

* DeskView, Whiteboard, Overhead (unit 9 / 0x02 write protocol).
* Privacy (unit 9 / 0x1B bit 11 + unit 10 / 0x0F).
* Framing (unit 9 / 0x13), HDR / mirror / gesture bits (unit 9 / 0x1B).
* Device info / firmware version (unit 9 / 0x03) — read-only, low risk,
  but the payload layout is only documented for the original Link.

## Licensing

fmontes, csmarshall and fugisawa are MIT; insta360-link-rs declares MIT in
`Cargo.toml` without a license file; jfwoods has no license. `linkctl`
contains no code copied from any of them: the V4L2/UVC structures come from
the Linux UAPI headers and the UVC 1.5 specification, and the protocol
facts above are used as documented knowledge with attribution.
