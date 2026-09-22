<p align="center">
  <img src="logo.svg" alt="vtd mascot" width="160">
</p>

# vtd

A tiny, no-UI, push-to-talk voice dictation daemon for Linux. Hold a key, talk,
release — your speech is transcribed locally (via [whisper.cpp](https://github.com/ggml-org/whisper.cpp))
and typed directly into whatever text field is focused, anywhere on your desktop.

No Electron app, no tray icon, no cloud API calls. Just a background process
that watches one key and a keyboard-injection call.

## Runs GPU-accelerated on AMD Strix Halo (Ryzen AI Max / Radeon 8060S, gfx1151)

This project was built and is actively used on an **AMD Ryzen AI Max ("Strix
Halo") APU with Radeon 8060S graphics (gfx1151)**, transcribing with
whisper.cpp's `large-v3-turbo` model fully offloaded to the integrated GPU via
ROCm/HIP — under a second per utterance, no CPU fallback needed. If you're
looking for a dictation tool that actually uses your Strix Halo GPU instead of
falling back to CPU, this repo includes the exact ROCm compatibility patch and
build script that got HIP acceleration working (see
[`patches/`](patches/) and [Known issues](#known-issues) below).

It should also work on any other AMD GPU ROCm supports, or CPU-only anywhere
`whisper.cpp` runs.

## Works well on Wayland

Unlike tools built on `wtype` (which relies on the Wayland virtual-keyboard
protocol and simply doesn't work on compositors that don't implement it, like
mutter), `vtd` injects keystrokes through the kernel's `/dev/uinput` via
`ydotool` — compositor-agnostic by construction. It's actively developed and
tested on **Ubuntu with GNOME on Wayland**, where `wtype`-based tools fail
outright, and the same approach should work unmodified on KDE, sway, or X11.

## Why

Tools like [Handy](https://github.com/cjpais/handy) are great, but on GNOME/
Wayland their input-injection layer (`wtype`, which relies on the Wayland
virtual-keyboard protocol) simply doesn't work — mutter doesn't implement that
protocol. `vtd` instead injects keystrokes through the kernel's `/dev/uinput`
via [`ydotool`](https://github.com/ReimuNotMoe/ydotool), which bypasses the
compositor entirely and works the same way on GNOME, KDE, sway, or anything
else.

It's also intentionally minimal: one Rust binary, zero external crates, no
UI. It reads raw keyboard events, shells out to `pw-record`, `whisper-cli`,
and `ydotool`, and that's the whole program.

## How it works

1. A background thread reads raw `evdev` events from your keyboard device,
   watching for a specific key (default: Right Alt).
2. On key-down, `pw-record` starts capturing 16kHz mono audio from your
   default microphone (or a configured PipeWire source).
3. While the key stays down, short repeated "pulses" from the keyboard are
   treated as "still held" (many keyboards/firmware don't report a clean
   continuous hold, they re-fire make/break events every few hundred ms).
   Once no pulse arrives for ~500ms, the key is considered released.
4. The recording is stopped and handed to `whisper-cli` for local
   transcription.
5. The resulting text is typed into whatever's focused via `ydotool type`.

No audio or text ever leaves your machine.

## Requirements

- Linux with `/dev/uinput` access (works on GNOME, KDE, sway, X11, Wayland —
  anything, since injection is kernel-level, not compositor-level)
- [`ydotool`](https://github.com/ReimuNotMoe/ydotool) installed
- PipeWire (`pw-record`) for audio capture
- `curl`, `git`, `cmake`, a C++ toolchain (to build whisper.cpp)
- Rust/cargo (to build `vtd` itself)
- Read access to your keyboard's `/dev/input/eventN` node, and read/write
  access to `/dev/uinput` (see [Permissions](#permissions) below)

## Quickstart

```sh
# 1. Build whisper.cpp (auto-detects ROCm/HIP, falls back to CPU)
./scripts/build-whisper.sh

# 2. Build vtd
cargo build --release

# 3. Download a model and install as a systemd --user service
./target/release/vtd install --model large-v3-turbo
```

That's it — hold Right Alt, speak, release, and the transcription gets typed
wherever your cursor is focused.

Check on it any time with:

```sh
vtd status
```

Uninstall the service (models and the whisper.cpp checkout are left in place):

```sh
vtd uninstall
```

## Choosing a model

`vtd install --model <name>` accepts any of whisper.cpp's standard ggml
models: `tiny`, `tiny.en`, `base`, `base.en`, `small`, `small.en`, `medium`,
`medium.en`, `large-v1`, `large-v2`, `large-v3`, `large-v3-turbo`. Smaller
models are faster but less accurate; `large-v3-turbo` is the default and, on
GPU, still transcribes a several-second utterance in under a second.

## Permissions

`vtd` needs to read your keyboard device and write to `/dev/uinput`. The
simplest fix is adding yourself to the relevant groups and re-logging in:

```sh
sudo usermod -aG input $USER
```

`/dev/uinput` access varies by distro; if `ydotool` reports it can't open the
device, either add a udev rule granting your user/group access, or grant a
one-off ACL to test with: `sudo setfacl -m u:$USER:rw /dev/uinput`.

## Configuration

All configuration is environment variables, forwarded into the systemd unit
at `vtd install` time if set beforehand:

| Variable | Default | Purpose |
|---|---|---|
| `VTD_KEYBOARD_DEVICE` | autodetected | `/dev/input/eventN` for your keyboard |
| `VTD_TRIGGER_KEY` | `100` (`KEY_RIGHTALT`) | Linux key code to hold |
| `VTD_MIC_TARGET` | PipeWire default | PipeWire source target id/name |
| `VTD_WHISPER_BIN` | `~/.local/share/vtd/whisper.cpp/build/bin/whisper-cli` | path to whisper-cli |
| `VTD_WHISPER_MODEL` | `~/.local/share/vtd/models/ggml-large-v3-turbo.bin` | path to a ggml model |
| `VTD_WHISPER_LD_LIBRARY_PATH` | unset | extra `LD_LIBRARY_PATH` for whisper-cli (non-standard ROCm installs) |

Keyboard device autodetection walks `/proc/bus/input/devices` looking for a
device with a `kbd` event handler, preferring one with "keyboard" in its
name. It isn't foolproof on every laptop/keyboard combination — if `vtd`
picks the wrong device (or your Right Alt doesn't fire, e.g. no physical key
present), override `VTD_KEYBOARD_DEVICE` / `VTD_TRIGGER_KEY` directly. You can
find your keyboard's event number and a candidate trigger key's code by
watching `cat /proc/bus/input/devices` and reading raw events from the
candidate `/dev/input/eventN`.

## Known issues

**Old system HIP headers can shadow a newer ROCm install.** On some systems
with more than one HIP/ROCm installation (e.g. a distro-packaged `hip-dev` in
`/usr/include` alongside a newer install elsewhere), the compiler's HIP
driver mode adds your real ROCm include path via low-priority `-idirafter`,
while `/usr/include` is always searched first — so a stale `hip_version.h`
wins and whisper.cpp's HIP compatibility shims pick the wrong preprocessor
branch, breaking the build with errors about `hipblasDatatype_t` or
`hipStreamWaitEvent`. `scripts/build-whisper.sh` works around this with an
explicit `-isystem $ROCM_PATH/include`, which takes priority over
`-idirafter`. `patches/rocm-7.14-hipblas-compat.patch` additionally restores
a default `flags` argument to `hipStreamWaitEvent` that ROCm's HIP runtime
dropped relative to what whisper.cpp's CUDA-compat macro assumes.

## License

Public domain, [The Unlicense](LICENSE).
