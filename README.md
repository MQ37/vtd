<p align="center">
  <img src="logo.svg" alt="vtd mascot" width="160">
</p>

# vtd

A tiny, no-UI, push-to-talk voice dictation daemon for Linux. Hold a key, talk,
release — your speech is transcribed locally and typed directly into whatever
text field is focused, anywhere on your desktop.

No Electron app, no tray icon, no cloud API calls. Just a background process
that watches one key, a sandboxed local speech server, and a keyboard-injection
call.

- **Fast:** the default [Phonon-2](https://huggingface.co/FermionResearch/Phonon-2)
  engine runs on the CPU and transcribes a typical utterance in roughly 50–150 ms
  (measured on a 16-core Ryzen AI Max+ 395) — no GPU needed.
- **Private:** audio and text never leave your machine. The speech server runs in a
  rootless podman container with **no network**.
- **Safe to install:** everything the Phonon backend downloads is pinned and hash-verified;
  see [Security model](#security-model).
- **Wayland-proof:** types through `/dev/uinput` (`ydotool`), so it works on GNOME/mutter,
  KDE, sway and X11 alike.

## Quickstart

Requirements: x86-64 Linux with glibc, **rootless [podman](https://podman.io)**, `curl`,
[`ydotool`](https://github.com/ReimuNotMoe/ydotool), PipeWire (`pw-record`), Rust/cargo,
and read access to your keyboard's `/dev/input/eventN` plus `/dev/uinput`
(see [Permissions](#permissions)).

```sh
cargo build --release
./target/release/vtd install
```

That's it. `vtd install` (about 2–3 minutes, ~2 GB of disk, one-time):

1. builds the `vtd-phonon` container image from a digest-pinned base image and a
   sha256-locked set of Python packages;
2. downloads the Phonon-2 model (164 MB) at a pinned revision and verifies every file's sha256;
3. unpacks and warms the model offline;
4. installs and starts two `systemd --user` services: `vtd-phonon` (the speech server)
   and `vtd` (the key watcher).

Hold Right Alt, speak, release — the text is typed wherever your cursor is.

```sh
vtd status                  # image, model, socket and service state
vtd uninstall               # stop and remove the services
vtd uninstall --purge       # ...and delete the Phonon image and downloaded model
```

Logs: `journalctl --user -u vtd-phonon -u vtd -f`.

## Choosing a backend

| | **Phonon-2** (default) | **whisper.cpp** |
|---|---|---|
| Languages | English only | ~100 languages |
| Speed (typical utterance) | ~50–150 ms, CPU only | ~1 s per utterance (process start + model load), GPU optional |
| Accuracy (English, Open ASR Leaderboard avg. WER, per the model card) | 5.21 % | 6.58 % (`large-v3-turbo`) |
| Footprint | ~1.2 GB RAM while running, ~2 GB disk | model file 0.1–3 GB |
| Install | `vtd install` | `scripts/build-whisper.sh` + `vtd install --backend whisper` |

Use whisper.cpp if you dictate in a language other than English:

```sh
./scripts/build-whisper.sh                       # auto-detects ROCm/HIP, falls back to CPU
./target/release/vtd install --backend whisper --model large-v3-turbo
```

(Whisper models are fetched from the `ggerganov/whisper.cpp` Hugging Face repo without a
checksum; the pinning described below applies to the Phonon backend.) `--model` accepts any standard ggml model: `tiny`, `tiny.en`, `base`, `base.en`,
`small`, `small.en`, `medium`, `medium.en`, `large-v1`, `large-v2`, `large-v3`,
`large-v3-turbo`.

If you installed Phonon and also have a whisper.cpp build (`VTD_WHISPER_BIN`), vtd falls
back to it when the Phonon server is down.

## Security model

Dictation software hears everything you say, so `vtd install` is deliberately paranoid
about what it pulls in and what that code can do.

**What gets downloaded, and how it is pinned**

| What | From | Pinned by |
|---|---|---|
| Base image `python:3.12-slim-trixie` | Docker Hub | multi-arch index digest in [`phonon/Containerfile`](phonon/Containerfile) |
| 40 Python packages (torch CPU, transformers, [`fermion-research`](https://pypi.org/project/fermion-research/) 0.2.7, …) | PyPI and the PyTorch CPU index | exact versions + sha256 of every acceptable wheel in [`phonon/requirements.lock`](phonon/requirements.lock) and [`phonon/requirements-torch.lock`](phonon/requirements-torch.lock); installed with `--require-hashes --only-binary=:all: --no-deps`, so no build scripts run and a substituted file fails the install |
| Phonon-2 model | Hugging Face `FermionResearch/Phonon-2` | commit revision and per-file sha256 in [`phonon/fetch_model.py`](phonon/fetch_model.py) |

**What the speech server can do at runtime**

The `vtd-phonon.service` container runs with `--network none`, `--read-only`, `--cap-drop all`,
`--security-opt no-new-privileges`, your own non-root uid, a 4 GB memory and 512-process limit,
read-only model mounts, and nothing else mounted except one owner-only (`0700`) socket
directory in `$XDG_RUNTIME_DIR`. It cannot reach the network or read your files. The model
is downloaded only during `vtd install`, in a separate short-lived container.

**One thing you should know:** Phonon's inference kernels are closed-source native libraries
(`libphonon2_*.so`, shipped inside the `fermion-research` wheel). We reviewed them statically:
they import only libc/libm/pthread functions (no sockets, file I/O, exec, or `dlopen`) and contain
no inline `syscall` instructions. Combined with the sandbox above, they have no way to
exfiltrate anything — but they are not auditable source. If that is not acceptable to you, use the
whisper.cpp backend.

**Updating the pins** (maintainers): bump versions in `phonon/requirements.in`, run
`scripts/update-phonon-lock.py` inside a container (instructions in the script), review the diff of
the lock files, bump `IMAGE` in `src/phonon.rs`, and re-run `vtd install`.

## How it works

1. A background thread reads raw `evdev` events from your keyboard device,
   watching for a specific key (default: Right Alt).
2. On key-down, `pw-record` starts capturing 16 kHz mono audio from your
   default microphone (or a configured PipeWire source).
3. While the key stays down, short repeated "pulses" from the keyboard are
   treated as "still held" (many keyboards/firmware don't report a clean
   continuous hold, they re-fire make/break events every few hundred ms).
   Once no pulse arrives for ~500 ms, the key is considered released.
4. The recording is POSTed (via `curl`) to the Phonon server on its Unix socket
   (`/v1/audio/transcriptions`, OpenAI-compatible). With the whisper backend it is passed to
   `whisper-cli` instead.
5. The resulting text is typed into whatever's focused via `ydotool type`.

vtd itself is one Rust binary with zero crates; it shells out to `pw-record`, `curl`, `ydotool`
and `systemctl`.

## Why not `wtype`-based tools?

Tools like [Handy](https://github.com/cjpais/handy) are great, but on GNOME/Wayland their
input-injection layer (`wtype`, which relies on the Wayland virtual-keyboard protocol) simply
doesn't work — mutter doesn't implement that protocol. `vtd` instead injects keystrokes through the
kernel's `/dev/uinput` via `ydotool`, which bypasses the compositor entirely.

## Permissions

`vtd` needs to read your keyboard device and write to `/dev/uinput`. The
simplest fix is adding yourself to the relevant groups and re-logging in:

```sh
sudo usermod -aG input $USER
```

`/dev/uinput` access varies by distro; if `ydotool` reports it can't open the
device, either add a udev rule granting your user/group access, or grant a
one-off ACL to test with: `sudo setfacl -m u:$USER:rw /dev/uinput`.

Rootless podman needs `/etc/subuid` and `/etc/subgid` entries for your user (the default on
most distros).

## Configuration

All configuration is environment variables. `vtd install` forwards them into the systemd unit
if they are set when you run it (except `VTD_PHONON_SOCKET`, which only applies to a foreground
`vtd run`; the installed server always listens on the default socket):

| Variable | Default | Purpose |
|---|---|---|
| `VTD_BACKEND` | `phonon` | `phonon` or `whisper` |
| `VTD_KEYBOARD_DEVICE` | autodetected | `/dev/input/eventN` for your keyboard |
| `VTD_TRIGGER_KEY` | `100` (`KEY_RIGHTALT`) | Linux key code to hold |
| `VTD_MIC_TARGET` | PipeWire default | PipeWire source target id/name |
| `VTD_PHONON_SOCKET` | `$XDG_RUNTIME_DIR/vtd/phonon.sock` | Unix socket of the Phonon server |
| `VTD_WHISPER_BIN` | `~/.local/share/vtd/whisper.cpp/build/bin/whisper-cli` | whisper-cli (backend, or fallback) |
| `VTD_WHISPER_MODEL` | `~/.local/share/vtd/models/ggml-large-v3-turbo.bin` | ggml model for whisper-cli |
| `VTD_WHISPER_LD_LIBRARY_PATH` | unset | extra `LD_LIBRARY_PATH` for whisper-cli (non-standard ROCm installs) |

Keyboard device autodetection walks `/proc/bus/input/devices` looking for a
device with a `kbd` event handler, preferring one with "keyboard" in its
name. It isn't foolproof on every laptop/keyboard combination — if `vtd`
picks the wrong device (or your Right Alt doesn't fire, e.g. no physical key
present), override `VTD_KEYBOARD_DEVICE` / `VTD_TRIGGER_KEY` directly. You can
find your keyboard's event number and a candidate trigger key's code by
watching `cat /proc/bus/input/devices` and reading raw events from the
candidate `/dev/input/eventN`.

## Whisper backend on AMD Strix Halo (ROCm/HIP)

This project was built on an **AMD Ryzen AI Max ("Strix Halo") APU with Radeon 8060S graphics
(gfx1151)**, where whisper.cpp's `large-v3-turbo` runs fully offloaded to the integrated GPU via
ROCm/HIP — about a second per utterance. This repo includes the ROCm compatibility patch and build
script that got HIP acceleration working (see [`patches/`](patches/) and
[Known issues](#known-issues)). It should also work on any other AMD GPU ROCm supports, or
CPU-only anywhere `whisper.cpp` runs. Phonon-2 itself has no GPU backend for AMD; it runs on the
CPU, which on this class of machine is faster than the GPU whisper path for dictation-sized clips.

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

**The Phonon server takes ~10–15 s to load after login or restart.** During that window dictation
falls back to whisper.cpp if you have it installed, otherwise vtd logs
`no transcription (is vtd-phonon.service running?)`.

## Licenses and credits

vtd itself is public domain, [The Unlicense](LICENSE).

The Phonon backend downloads third-party software at install time (nothing is bundled in this repo):

- **Phonon-2 weights** — © Fermion Research, [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/);
  a quantized derivative of NVIDIA's [parakeet-tdt-0.6b-v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3)
  (CC-BY-4.0). The upstream `NOTICE` lists the changes.
- **`fermion-research`** (the engine/CLI) — Apache-2.0 Python code plus compiled inference kernels whose source is not published (see above).
- PyTorch, Transformers, NumPy and the other pinned dependencies under their own licenses.

vtd is not affiliated with Fermion Research or NVIDIA.
