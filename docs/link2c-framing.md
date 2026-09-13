# Link 2C digital framing protocol

Status: recovered by static analysis of the official Linux SDK and implemented
in `src/camera/insta360/link2c.rs` via the `frame` command. Readback and
GET_LEN/GET_INFO have been validated on a physical camera. The camera owner
subsequently confirmed that framing writes work in manual testing. That is
user-reported hardware validation, not an automated visual-motion test.
No SDK executable was run.

## Source and reproducibility

Official repository: https://github.com/Insta360Develop/Link-SDK

Analyzed revision: `2dd2c78186b0335a530042ae49accbef249d6a95`

Binary: `UVCCamera_Linux/bin/libUVCCamera.so` (x86-64 ELF)

SHA-256: `1caf4198fc10f8f16ace1f93263fa8627f7cfb60902688549faac48863798061`

Public declarations: `UVCCamera_Linux/include/uvc_common.h` (`HostPTZInfo`)
and `uvc_camera.h` (`GetHostPTZ` / `SetHostPTZ`). Example:
`UVCCamera_Linux/test/main.cc`, case 1108.

Inspect without executing the SDK:

```sh
nm -DC libUVCCamera.so | grep HostPTZ
objdump -dC -Mintel libUVCCamera.so > sdk.asm
```

## Write crop position and zoom

- Transport: `UVCIOC_CTRL_QUERY` on the video node, not libusb.
- Extension GUID: `E307E649-4618-A3FF-82FC-2D8B5F216773`.
- Unit ID on the attached Link 2C: **10**. Resolve/validate by GUID.
- Selector: **0x13**.
- Request: `UVC_SET_CUR` (**0x01**).
- Logical payload: **8 bytes**.

| Byte offset | Encoding | Meaning |
| --- | --- | --- |
| 0 | uint16 little-endian | Zoom ratio; example accepts 100..400 (1x..4x) |
| 2 | uint16 little-endian | Horizontal crop center, normalized X times 65535 |
| 4 | uint16 little-endian | Vertical crop center, normalized Y times 65535 |
| 6 | uint8 | Movement step; SDK example uses 20 |
| 7 | uint8 | Zoom step; SDK example uses 20 |

Encoding example, not a hardware test:

```python
import struct
payload = struct.pack('<HHHBB', 200, 32767, 32767, 20, 20)
assert payload.hex(' ') == 'c8 00 ff 7f ff 7f 14 14'
```

This corresponds to 2x zoom and approximately centered X/Y using the SDK's
truncating normalized-coordinate conversion. There is no separate left/right
opcode: change X or Y while preserving the other position and zoom values.
Screen-direction polarity, edge clamping, step timing, and interaction with
Auto Framing remain to be checked visually on hardware.

The SDK first sends `GET_LEN` (0x85, two-byte result) on the same unit/selector.
It allocates a zero-filled buffer of that returned length, copies the first
`min(length, 8)` payload bytes, and sends SET_CUR. If GET_LEN fails it falls
back to 8. A linkctl implementation should reject unexpected lengths rather
than blindly copying this truncation/zero-padding behavior.

There is no checksum, extra packet header, AI-mode write, or driver detach
in this setter's call chain.

Do not confuse this with **unit 9 / selector 0x13**, the head/half/whole-body
composition selector documented for other Link functionality.

### Static evidence

ELF virtual addresses in the analyzed binary:

- `0x70d82`: `LinuxUVCCameraExtendController::SetHostPTZ` builds 8 bytes.
- `0x70de3` through `0x70e5d`: copies three uint16 values and two bytes.
- `0x70e82`: selector argument is 0x13.
- `0x70e8a`: calls `SendCommand2`.
- `0x68b28`: `SendCommand2` resolves `GetExtensionUnit2Id`, obtains GET_LEN,
  then sends the payload through `SetData`.
- `0x823fe`: descriptor parser matches `kExtUnit2Guid` at `0x8d8f0`.
  Its raw GUID bytes are `49 e6 07 e3 18 46 ff a3 82 fc 2d 8b 5f 21 67 73`.
- `0x80e44`: `LinuxUVCDeviceImp::SetData` selects request 0x01.
- `0x80aa1`: `CtrlQuery` uses ioctl `0xc0107521` (UVCIOC_CTRL_QUERY on x86-64).

## Read crop position

The SDK's `GetHostPTZ` does **not** read unit 10 / selector 0x13. It calls
`GetVideoMode` and extracts coordinates from the mode-state payload:

- Extension GUID: `FAF1672D-B71B-4793-8C91-7B1C9B7F95F8`.
- Unit ID on the attached Link 2C: **9**. Resolve/validate by GUID.
- Selector: **0x02**.
- Request: `UVC_GET_CUR` (**0x81**), after GET_LEN.
- X: uint16 little-endian at byte offsets **38..39** (0x26).
- Y: uint16 little-endian at byte offsets **40..41** (0x28).

The SDK explicitly selects this decoding for PIDs 0x4c03 and 0x4c05.
The public `VideoMode` enum also identifies 0x07 as AutoFraming. The linkctl
implementation accepts mode 0x00, 0x01, and 0x07; it rejects special modes,
unknown modes, and the 0xFF idle/transition sentinel. It only
reaches these fields when the mode-state payload has at least 52 bytes;
46-byte special-mode payloads take a separate path. The attached camera
previously reported a 56-byte mode payload through linkctl info.

`GetHostPTZ` fills only zoomX and zoomY; it does not populate ratio or the
step fields. Do not assume a zero-initialized structure returned by it is
safe to pass directly to SetHostPTZ. Read/preserve zoom separately.

Static evidence: `GetHostPTZ` at `0x5f748`; `GetVideoMode` at `0x6a2e2`;
PID 0x4c03 comparison at `0x6a4d3`; X/Y extraction at `0x6a4ea` / `0x6a503`.

## Read-only hardware verification

On the connected Link 2C (2e1a:4c03, /dev/video0):

- Unit 10 / 0x13 GET_LEN returned `08 00` (8 bytes).
- Unit 10 / 0x13 GET_INFO returned `03` (GET and SET supported).
- Mode 0x07, 56-byte mode payload: frame readback was X=32767, Y=32767,
  zoom=100 (approximately centered at 1x).

The CLI rounds normalized input to the nearest uint16 coordinate rather
than truncating as the SDK example does: 0.5 encodes to 32768. Unspecified
coordinates retain their exact raw readback values.

## Implementation boundary

The raw transport and encoding are identified, and framing writes have been
confirmed working by the camera owner. Edge behavior and Auto Framing interactions
still need broader validation. Keep Link 2 gimbal controls separate from Link 2C digital crop
controls, preserve the activity guard, validate GUIDs and lengths, and use
readback plus a visible stream to test the result. Never write to unit 9's
mode payload merely to move the crop.
