# linkctl

Native Linux command-line control for Insta360 Link webcams.

No daemon. No official app. Just one binary.

```bash
linkctl status
linkctl preview        # open a live view (this is what activates the camera)
linkctl right 10
linkctl zoom 1.5
linkctl center
linkctl preset save desk
```

`linkctl` talks to the camera through the Linux kernel's V4L2 and UVC
interfaces directly. It does not shell out to `v4l2-ctl`, does not need
Python, and never detaches the `uvcvideo` driver.

## Supported cameras

| model | USB id | status |
|-------|--------|--------|
| Insta360 Link 2 | `2e1a:4c04` | validated on real hardware |
| Insta360 Link (original) | `2e1a:4c01` | recognised, **not tested** |

## Features

* Pan, tilt, zoom in human units (degrees, zoom factor) with device-reported
  ranges, absolute and relative.
* Focus, white balance, brightness, contrast, saturation, sharpness, hue.
* Framing presets stored in a TOML config file.
* Resolution, pixel format (MJPEG/H.264) and frame rate: list what the
  camera offers and set the default the preview and simple tools will use.
* `status`, `info`, `devices` with `--json` output for scripting.
* A safety guard that refuses to move a camera nobody is using.
* Experimental AI tracking on/off through the vendor extension unit.
* Fast enough for keybindings: a movement command completes in milliseconds.

## Installation

Every release ships static Linux binaries (no glibc dependency) for
`x86_64` and `aarch64`, plus packages, on the
[releases page](https://github.com/illegalstudio/linkctl/releases):

| asset | use |
|-------|-----|
| `linkctl_<ver>_linux_amd64.tar.gz`, `..._arm64.tar.gz` | tarball with the binary, LICENSE and README |
| `linkctl_<ver>_linux_<arch>.pkg.tar.zst` | Arch Linux: `sudo pacman -U <file>` |
| `linkctl_<ver>_linux_<arch>.deb` | Debian/Ubuntu: `sudo apt install ./<file>` |
| `linkctl_<ver>_linux_<arch>.rpm` | Fedora/openSUSE: `sudo dnf install ./<file>` |
| `linkctl_<ver>_checksums.txt` | SHA-256 of every asset |
| `PKGBUILD` | the `linkctl-bin` AUR package for that release |

### mise

```bash
mise use -g github:illegalstudio/linkctl@latest
```

The `ubi` backend works too: `mise use -g ubi:illegalstudio/linkctl`.

### Arch Linux

```bash
# AUR (binary package, updated by the release workflow)
paru -S linkctl-bin        # or: yay -S linkctl-bin

# or the package attached to the release
sudo pacman -U linkctl_<ver>_linux_amd64.pkg.tar.zst

# or build from source with the PKGBUILD in packaging/aur/linkctl
```

### From source

Requires stable Rust (1.85 or newer) and Linux.

```bash
cargo install --path .        # or: make install  (installs to ~/.local/bin)
```

For `linkctl preview`, install FFmpeg (`ffplay`) or mpv.

## Quick start

```bash
linkctl devices          # 1  Insta360 Link 2  /dev/video0  2e1a:4c04
linkctl status           # State: inactive
linkctl preview          # opens ffplay; the camera raises from its parked position
# in another terminal, while the preview is open:
linkctl right 10
linkctl up 5
linkctl zoom 2
linkctl center
```

## Camera activity behaviour

**`linkctl` never wakes an inactive camera implicitly.**

The Link 2 parks itself, pointing down, whenever no application is streaming
from it. Sending it a pan/tilt command would make it physically stand up and
move. So every state-changing command first checks whether some other process
has the camera open, and refuses otherwise:

```text
$ linkctl right
Camera is inactive.

Start a preview with:
  linkctl preview
```

Exit code is 5 and nothing is sent to the camera. This makes accidental
keybinding presses harmless.

Open the camera in an application or run:

```bash
linkctl preview
```

Use `--force` only when you intentionally want to bypass this safeguard:

```bash
linkctl --force center
```

Read-only commands (`status`, `info`, `devices`, `pan`/`tilt`/`zoom` with
no argument, `preset save`, `preset list`, `tracking status`) always work.
See [docs/activity-detection.md](docs/activity-detection.md) for how
activity is detected and where it can be wrong.

## Preview

```bash
linkctl preview
linkctl preview --player mpv
linkctl preview --resolution 1920x1080
```

`preview` launches an external player on the camera's streaming node with
low-latency options and waits for it to exit. It uses the camera's current
pixel format and, unless `--resolution` or `preview_resolution` says
otherwise, its current size (see `linkctl resolution`).
It is the intended way to activate the camera. If the player is missing:

```text
ffplay was not found.

Install FFmpeg or use another application to open the camera.
```

Only `ffplay` has been validated; `mpv` support is provided but untested.

## Commands

```text
linkctl status                 state, pan, tilt, zoom, focus, wb, tracking
linkctl info [--controls]      USB, nodes, driver, ranges, extension units
linkctl devices                list Insta360 cameras
linkctl formats                pixel formats, resolutions and frame rates offered
linkctl resolution [WxH[@FPS]] [--format mjpeg|h264]   show or set the current format

linkctl center                 pan 0°, tilt 0° (zoom/focus/wb untouched)
linkctl left|right|up|down [DEGREES]   relative move (default step: 5°)
linkctl pan [DEGREES]          absolute pan, negative = left; no arg reads
linkctl tilt [DEGREES]         absolute tilt, negative = down; no arg reads
linkctl move --pan 30 --tilt -10

linkctl zoom [FACTOR]          1 .. 4 on the Link 2
linkctl focus auto|VALUE
linkctl wb auto|KELVIN
linkctl brightness|contrast|saturation|sharpness|hue [VALUE]

linkctl tracking [status|on|off|toggle]   experimental

linkctl preset save|load|delete NAME
linkctl preset list

linkctl preview [--player ffplay|mpv] [--resolution WxH]
```

Global flags: `--device PATH`, `--force`, `--json`, `--quiet`, `--verbose`.

### Output modes

```bash
$ linkctl pan 30
Pan: 30°

$ linkctl --quiet pan 30          # nothing on success, errors on stderr

$ linkctl status --json
{
  "model": "Insta360 Link 2",
  "device": "/dev/video0",
  "state": "active",
  "pan": -15.0,
  "tilt": 4.0,
  "zoom": 1.0,
  "focus": { "auto": true, "value": 94 },
  "white_balance": { "auto": false, "temperature": 3650 },
  "tracking": false
}
```

In `--json` mode errors are also JSON, on stderr:
`{"error":"camera_inactive","message":"Camera is inactive.","exit_code":5,...}`.

### Exit codes

| code | meaning |
|------|---------|
| 0 | success |
| 2 | invalid command-line arguments |
| 3 | camera not found / device path does not exist / not an Insta360 camera |
| 4 | multiple cameras found, use `--device` |
| 5 | camera inactive (use `linkctl preview` or `--force`) |
| 6 | permission denied opening the device |
| 7 | control not supported by this camera |
| 8 | device I/O error (ioctl failure, device disappeared) |
| 9 | configuration file error |
| 10 | preview player missing or failed |
| 11 | value out of range / invalid |
| 12 | vendor extension-unit error |
| 13 | camera busy (format change attempted while another app streams) |

These are stable.

## Resolution and format

```bash
$ linkctl formats
MJPG (Motion-JPEG)
  1920x1080  30 25 24 fps
* 1280x720   30 25 24 fps
  3840x2160  30 25 24 fps
H264 (H.264)
  ...

$ linkctl resolution
Resolution: 1280x720 @ 30 fps MJPG

$ linkctl resolution 1920x1080@25
$ linkctl resolution --format h264 3840x2160
```

In V4L2 the resolution is negotiated by whichever application opens the
stream, so this is not a setting stored in the camera. What `resolution`
sets is the driver's *current* format, which `linkctl preview`, ffplay, mpv
and most simple tools pick up; browsers and PipeWire negotiate their own.
Requests are validated against `formats`, so a size or rate the camera does
not offer is rejected with the list of valid ones. Changing the format does
not move the gimbal, and while another application is streaming the driver
refuses it (exit code 13).

## Presets

Presets store pan, tilt and zoom in human units:

```bash
linkctl preset save desk       # reads the current framing (works while inactive)
linkctl preset load desk       # respects the activity guard
linkctl preset list
linkctl preset delete desk
```

They live in the config file:

```toml
[presets.desk]
pan = -15.0
tilt = 4.0
zoom = 1.1
```

`preset save` edits the file in place and preserves comments and other
settings. Note that while the camera is parked, the values it reports are
the last commanded position, not the physical parked position.

## Configuration

`$XDG_CONFIG_HOME/linkctl/config.toml`, falling back to
`~/.config/linkctl/config.toml`. Everything is optional:

```toml
default_step = 5.0            # degrees for left/right/up/down
preview_player = "ffplay"     # or "mpv"
preview_resolution = "1280x720"   # optional; default: the camera's current format

[presets.desk]
pan = -15.0
tilt = 4.0
zoom = 1.1
```

A malformed file is an error (exit 9), never silently ignored.

## Hyprland / Omarchy example

`linkctl` is desktop-agnostic; it simply behaves well when bound to keys.

```ini
bind = SUPER, LEFT,  exec, linkctl --quiet left
bind = SUPER, RIGHT, exec, linkctl --quiet right
bind = SUPER, UP,    exec, linkctl --quiet up
bind = SUPER, DOWN,  exec, linkctl --quiet down
bind = SUPER, C,     exec, linkctl --quiet center
bind = SUPER, P,     exec, linkctl preview
```

Because inactive cameras refuse movement, pressing these keys while the
webcam is not in use does nothing.

For Waybar or similar, `linkctl status --json` gives you `state`, `pan`,
`tilt` and `zoom` in one call.

## Multiple cameras

With exactly one supported camera connected everything works without
options. With several:

```text
Multiple Insta360 Link cameras found.

1. Insta360 Link 2 — /dev/video0
2. Insta360 Link 2 — /dev/video4

Use --device to select one.
```

`--device` accepts any node of the camera, including the metadata node.

## How it works

* **Discovery**: `/sys/class/video4linux/video*` is enumerated, each node's
  USB parent is read from sysfs, and only nodes with an Insta360 VID/PID are
  opened and classified with `VIDIOC_QUERYCAP`. The video-capture node is the
  control node; metadata nodes are recorded but never used for control.
* **Controls**: standard V4L2 controls via `VIDIOC_QUERYCTRL`,
  `VIDIOC_G_CTRL`, `VIDIOC_S_CTRL`. Pan/tilt are arc seconds
  (1° = 3600), zoom is `100` = 1.0x; ranges always come from the device.
* **Vendor features**: `UVCIOC_CTRL_QUERY` through the kernel driver, with
  the unit id confirmed against the GUID in the USB descriptors.
* **Activity**: a native `/proc/<pid>/fd` scan comparing device numbers.

See [docs/architecture.md](docs/architecture.md).

## Linux permissions

`linkctl` needs read/write access to `/dev/videoN`. On most desktops the
logged-in user gets it automatically through systemd-logind (`uaccess`).
Otherwise add yourself to the `video` group, or install the optional udev
rule in [`contrib/99-insta360-link.rules`](contrib/99-insta360-link.rules).

If access is missing:

```text
Permission denied opening /dev/video0.

Check your device permissions or group membership.
```

`linkctl` never changes udev rules, groups or ACLs itself.

## Safety

* Standard V4L2 controls are the normal path and are validated against the
  device's own ranges.
* Vendor writes are limited to one documented, reversible, 1-byte control
  and go through read → validate (`GET_LEN`, `GET_INFO`, GUID) → write →
  read-back.
* The kernel driver is never detached; libusb is not used at all.
* No firmware, EEPROM, bootloader or unknown-selector writes, ever.

Details in [docs/safety.md](docs/safety.md).

## Experimental vendor features

`linkctl tracking on|off|toggle|status` uses extension unit 11, selector
0x02 (1 byte), which two independent projects report as the Link 2 AI
tracking switch. Reading it is validated on the development camera; the
write path is implemented with full checks but is marked experimental until
it has been exercised on more hardware. The firmware only honours vendor
writes while video is streaming, so the activity guard applies and a
read-back mismatch is reported.

DeskView, Whiteboard, Overhead, privacy, gestures and HDR are documented in
[docs/research.md](docs/research.md) but intentionally not implemented yet.

## Known limitations

* Activity detection cannot see processes of other users (see
  `docs/activity-detection.md`); use `--force` in that situation.
* Pan/tilt values read while the camera is parked reflect the last command,
  not the physical position.
* No serial number is exposed by the Link 2 over USB, so multi-camera
  selection is by device path only.
* Original Link support is limited to recognition.
* `mpv` preview is untested.

## Reverse-engineering references

`linkctl` contains no code from these projects, but owes them the protocol
knowledge:

* [fmontes/insta360-link-cli](https://github.com/fmontes/insta360-link-cli) (MIT) — Link 2 extension unit GUIDs and the XU11 tracking selector.
* [csmarshall/link-ctl](https://github.com/csmarshall/link-ctl) (MIT) — extensive Link 2 Linux notes, AI mode payload, hang reports.
* [fugisawa/insta360-link-ctl](https://github.com/fugisawa/insta360-link-ctl) (MIT) — `UVCIOC_CTRL_QUERY` approach, GUID-based unit resolution, "writes revert unless streaming".
* [insta360-link-rs](https://tangled.org/mara.x0f.nl/insta360-link-rs) — Rust/V4L2 reference.
* [jfwoods/insta360link-controller](https://github.com/jfwoods/insta360link-controller) — CLI UX ideas.

Standard controls follow the Linux kernel V4L2 documentation and the UVC 1.5
specification.

## Contributing

```bash
make check          # cargo fmt --check, clippy -D warnings, cargo test
make build          # release binary in bin/linkctl
make dist           # static musl binary + tar.gz/deb/rpm/pkg.tar.zst in dist/
```

`make dist` needs the musl target (`rustup target add x86_64-unknown-linux-musl`)
and, for the deb/rpm/Arch packages, [nfpm](https://nfpm.goreleaser.com/) on
`PATH`.

### Releasing

```bash
make release
```

proposes the next semver tag, sets `version` in `Cargo.toml`, commits, tags
and pushes. The tag triggers `.github/workflows/release.yml`, which builds
and tests both architectures, packages them, creates the GitHub release
with checksums, and publishes `linkctl-bin` to the AUR when the
`AUR_SSH_PRIVATE_KEY` repository secret is configured.

Hardware tests are opt-in and never run in CI:

```bash
cargo test --features hardware-tests -- --ignored readonly   # safe while parked
linkctl preview &                                            # then, with the camera active:
cargo test --features hardware-tests -- --ignored movement
```

Please do not submit vendor-control changes without a hardware validation
note and a reference to the reverse-engineering source.

## License

MIT. See [LICENSE](LICENSE).
