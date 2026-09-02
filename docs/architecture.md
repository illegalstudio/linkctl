# Architecture

```text
src/
  main.rs            entry point: parse CLI, run, map errors to exit codes
  cli.rs             clap definitions (global flags + subcommands)
  commands/          one file per command family; all rendering via Output
    mod.rs           Context (config, --force, --device), dispatch, guard
    status.rs  info.rs  devices.rs
    motion.rs        center/left/right/up/down/pan/tilt/move/zoom
    format.rs        formats / resolution
    image.rs         focus/wb/brightness/contrast/saturation/sharpness/hue
    tracking.rs      vendor AI tracking (experimental)
    preset.rs        preset save/load/list/delete
  output.rs          normal / --quiet / --json / --verbose
  error.rs           Error enum + stable exit codes
  config.rs          XDG config.toml (default_step, preview_*, presets)
  presets.rs         Preset type, format-preserving TOML edits
  preview.rs         spawn ffplay / mpv on the streaming node
  units.rs           degrees<->arcsec, zoom<->raw, Range, relative moves
  camera/
    mod.rs           Camera: open control node, typed get/set helpers
    v4l2.rs          capability/control ioctl wrappers (unsafe lives here)
    format.rs        ENUM_FMT/FRAMESIZES/FRAMEINTERVALS, G/S_FMT, G/S_PARM
    controls.rs      Control enum with V4L2_CID_* ids and names
    discovery.rs     sysfs + QUERYCAP enumeration, node roles, selection
    activity.rs      /proc fd scan (see activity-detection.md)
    model.rs         VID/PID -> Model
    insta360/
      xu.rs          UVCIOC_CTRL_QUERY wrapper with write policy checks
      link2.rs       Link 2 XU GUIDs/constants, tracking, descriptor parser
tests/
  hardware.rs        opt-in, #[ignore]d tests against a real camera
```

## Flow of a typical command

```text
linkctl left 10
  ├─ cli::Cli::parse
  ├─ Config::load                (XDG, defaults if absent, error if malformed)
  ├─ discovery::select           (sysfs scan, VID/PID match, QUERYCAP classify)
  ├─ Camera::open                (O_RDWR|O_NONBLOCK on /dev/video0)
  ├─ validate arguments          (device-reported ranges; may read controls)
  ├─ Context::ensure_active      (/proc scan; refuse unless --force)
  ├─ Camera::pan_relative        (G_CTRL, clamp+snap, S_CTRL)
  └─ Output::emit                ("Pan: -10°" / JSON / nothing)
```

Read-only commands skip the guard. `preview` skips it too because activating
the camera is its purpose.

## Design decisions

**Direct ioctls instead of the `v4l` crate.** Only a dozen ioctls are
needed (capabilities, controls, format enumeration/get/set, stream
parameters, `UVCIOC_CTRL_QUERY`). Wrapping them directly keeps the
dependency tree tiny, keeps every `unsafe` block in three files
(`camera/v4l2.rs`, `camera/format.rs`, `camera/insta360/xu.rs`) with
compile-time layout assertions, and avoids abstracting over buffers and
streaming, which a control tool does not use.

**Device identity by `st_rdev`, not path.** Discovery maps `--device` paths
to sysfs through `/sys/dev/char/MAJ:MIN`, and activity detection compares
device numbers, so symlinks and unusual `/dev` layouts work.

**Model-specific code is isolated in `camera/insta360/`.** Everything else is
generic UVC/V4L2. Adding a model means adding a PID to `model.rs` and, if it
has different extension units, a sibling of `link2.rs`.

**Validation before the guard.** Commands validate their arguments against
the device's real ranges before checking activity, so `linkctl pan 500`
reports "out of range" even when the camera is parked, without writing
anything.

**Presets edit the config file in place.** `toml_edit` preserves comments
and unrelated settings; writes are atomic (temp file + rename).

**No daemon, no cache.** Each invocation discovers the camera from sysfs
(a handful of small file reads plus one `QUERYCAP` per Insta360 node).
Measured end-to-end latency for `linkctl right` on the development machine
is a few milliseconds.

## Dependencies

| crate       | why                                                           |
|-------------|---------------------------------------------------------------|
| clap        | derive-based CLI with good help/usage errors                  |
| libc        | `ioctl`, `S_IFMT`, `major`/`minor`, errno constants           |
| serde       | JSON output structs, config deserialisation                   |
| serde_json  | `--json` output                                                |
| toml        | strict config parsing (`deny_unknown_fields`)                 |
| toml_edit   | format-preserving preset save/delete                          |
| thiserror   | error enum boilerplate                                         |

No `nix`, `v4l`, `rusb`/libusb, `udev` or async runtime.
