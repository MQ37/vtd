//! Optional heavy backend: whisper.cpp with ggml `large-v3`, run on the GPU in a sandbox.
//!
//! `vtd install --heavy` builds a pinned whisper.cpp (Vulkan) image and downloads the pinned,
//! sha256-verified model. `vtd transcribe FILE --heavy` then starts one container per file: no
//! network, read-only root, no capabilities, read-only model and audio mounts, and only
//! `/dev/dri` (the GPU) passed through. It is slow-and-accurate by design, with automatic
//! language detection, so it handles Czech and English mixed in one recording.

use crate::{config, phonon};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Bump when the Containerfile or model pin change so installs rebuild.
pub const IMAGE: &str = "localhost/vtd-heavy:1";

const CONTAINERFILE: &str = include_str!("../heavy/Containerfile");

const MODEL_FILE: &str = "ggml-large-v3.bin";
/// Revision of the `ggerganov/whisper.cpp` Hugging Face repo and the model's sha256 at it.
const MODEL_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
const MODEL_SHA256: &str = "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2";

fn heavy_dir() -> PathBuf {
    config::data_dir().join("heavy")
}

fn model_path() -> PathBuf {
    heavy_dir().join(MODEL_FILE)
}

/// `vtd install --heavy`: build the image and fetch the model. Touches no services.
pub fn install() {
    if let Err(e) = setup() {
        eprintln!("vtd: {e}");
        std::process::exit(1);
    }
    println!("vtd: heavy backend ready. Use `vtd transcribe FILE --heavy`.");
}

fn setup() -> Result<(), String> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(format!("the heavy backend is only built and tested for x86-64 Linux (this is {})", std::env::consts::ARCH));
    }
    let podman = phonon::find_podman().ok_or("podman not found on PATH. Install rootless podman (e.g. `sudo apt install podman`).")?;

    let build = heavy_dir().join("build");
    std::fs::create_dir_all(&build).map_err(|e| format!("failed to create {build:?}: {e}"))?;
    std::fs::write(build.join("Containerfile"), CONTAINERFILE).map_err(|e| format!("failed to write Containerfile: {e}"))?;

    println!("vtd: building the sandboxed whisper.cpp (Vulkan) image {IMAGE} (pinned source and base image; a few minutes the first time)");
    phonon::run(
        &podman,
        &[
            "build".into(), "-t".into(), IMAGE.into(),
            "-f".into(), build.join("Containerfile").display().to_string(),
            build.display().to_string(),
        ],
    )?;

    let model = model_path();
    if model.exists() {
        println!("vtd: model already present at {model:?}");
        return Ok(());
    }
    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/{MODEL_REVISION}/{MODEL_FILE}");
    println!("vtd: downloading {MODEL_FILE} (3.1 GB, pinned revision, sha256-verified)");
    let part = heavy_dir().join(format!("{MODEL_FILE}.part"));
    let fetched = Command::new("curl").args(["-L", "--fail", "--progress-bar", "-o"]).arg(&part).arg(&url).status();
    if !matches!(fetched, Ok(s) if s.success()) {
        let _ = std::fs::remove_file(&part);
        return Err("model download failed".to_string());
    }
    if let Err(e) = verify_sha256(&part, MODEL_SHA256) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &model).map_err(|e| format!("failed to move the model into place: {e}"))
}

fn verify_sha256(file: &Path, expected: &str) -> Result<(), String> {
    let out = Command::new("sha256sum").arg(file).output().map_err(|e| format!("failed to run sha256sum: {e}"))?;
    let got = String::from_utf8_lossy(&out.stdout);
    let got = got.split_whitespace().next().unwrap_or("");
    if got == expected {
        Ok(())
    } else {
        Err(format!("sha256 mismatch for {file:?}: expected {expected}, got {got:?}; refusing to use it"))
    }
}

/// `podman run` arguments that transcribe `wav` with the heavy model.
fn run_args(model: &Path, wav: &Path, gpu: bool) -> Vec<String> {
    let wav_dir = wav.parent().unwrap_or(Path::new("/"));
    let wav_name = wav.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let model_dir = model.parent().unwrap_or(Path::new("/"));
    let model_name = model.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut a: Vec<String> = [
        "run", "--rm", "--network", "none", "--read-only", "--cap-drop", "all",
        "--security-opt", "no-new-privileges", "--userns=keep-id", "--pids-limit", "512",
        "--memory", "16g", "--tmpfs", "/tmp", "-e", "HOME=/tmp",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if gpu {
        // The render group owns /dev/dri; keep-groups lets the container user inherit it.
        a.extend(["--device".into(), "/dev/dri".into(), "--group-add".into(), "keep-groups".into()]);
    }
    a.extend(phonon::mount(model_dir, "/model", "ro"));
    a.extend(phonon::mount(wav_dir, "/in", "ro"));
    // Not `-nt`: without timestamps whisper.cpp loses its place in long recordings and skips audio.
    a.extend([
        IMAGE.into(), "-m".into(), format!("/model/{model_name}"), "-f".into(), format!("/in/{wav_name}"),
        "-l".into(), "auto".into(), "-bs".into(), "5".into(), "-np".into(),
    ]);
    a
}

/// Transcribe a 16 kHz mono WAV with the heavy model; one segment per line.
pub fn transcribe(wav: &Path) -> Result<String, String> {
    let podman = phonon::find_podman().ok_or("podman not found on PATH")?;
    let model = model_path();
    if !model.exists() {
        return Err("the heavy backend is not installed (run `vtd install --heavy`)".to_string());
    }
    let gpu = Path::new("/dev/dri").exists();
    if !gpu {
        eprintln!("vtd: no /dev/dri; the heavy model will run on the CPU and be very slow");
    }
    let output = Command::new(podman)
        .args(run_args(&model, wav, gpu))
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("failed to run podman: {e}"))?;
    if !output.status.success() {
        return Err(format!("heavy transcription failed ({:?}); is the image built? (`vtd install --heavy`)", output.status));
    }
    Ok(strip_timestamps(&String::from_utf8_lossy(&output.stdout)))
}

/// whisper-cli prints `[00:00:00.000 --> 00:00:15.000]   text`; keep only the text.
fn strip_timestamps(out: &str) -> String {
    out.lines()
        .filter_map(|l| {
            let l = l.trim();
            if !l.starts_with('[') {
                return None;
            }
            let (stamp, text) = l.split_once(']')?;
            if !stamp.contains("-->") {
                return None;
            }
            Some(text.trim())
        })
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Remove the image and model (for `vtd uninstall --purge`).
pub fn purge() {
    if let Some(podman) = phonon::find_podman() {
        match Command::new(&podman).args(["rmi", IMAGE]).stderr(Stdio::null()).status() {
            Ok(s) if s.success() => println!("vtd: removed image {IMAGE}"),
            _ => {}
        }
    }
    let dir = heavy_dir();
    if dir.exists() {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => println!("vtd: removed {dir:?}"),
            Err(e) => eprintln!("vtd: failed to remove {dir:?}: {e}"),
        }
    }
}

pub fn status() {
    let image = phonon::find_podman()
        .map(|p| Command::new(p).args(["image", "exists", IMAGE]).status().map(|s| s.success()).unwrap_or(false))
        .unwrap_or(false);
    println!("heavy image {IMAGE}: {}", if image { "present" } else { "not installed (optional: `vtd install --heavy`)" });
    println!("heavy model {MODEL_FILE}: {}", if model_path().exists() { "present" } else { "not installed" });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_timestamps_and_keeps_segments() {
        let out = "\n[00:00:00.000 --> 00:00:15.000]   Ok, tohle je test.\n[00:00:15.000 --> 00:00:20.000]   Now English.\nnoise\n";
        assert_eq!(strip_timestamps(out), "Ok, tohle je test.\nNow English.");
        assert_eq!(strip_timestamps("[not a stamp] hi"), "");
    }

    #[test]
    fn run_args_are_sandboxed_and_use_the_gpu() {
        let a = run_args(Path::new("/m/ggml-large-v3.bin"), Path::new("/run/u/x.wav"), true).join(" ");
        for flag in ["--network none", "--read-only", "--cap-drop all", "no-new-privileges", "--device /dev/dri",
                     "/m:/model:ro,z", "/run/u:/in:ro,z", "-l auto", "-bs 5", "/in/x.wav"] {
            assert!(a.contains(flag), "missing {flag}: {a}");
        }
        assert!(!a.contains(" -nt"));
        assert!(!run_args(Path::new("/m/a.bin"), Path::new("/x/y.wav"), false).join(" ").contains("/dev/dri"));
    }
}
