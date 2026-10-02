//! Phonon-2 backend: a sandboxed, offline speech server run under podman.
//!
//! `vtd install` builds a hash-pinned image, downloads and verifies the pinned
//! model, pre-unpacks it, and installs a systemd --user unit that serves it on
//! an owner-only Unix socket. The server container has no network, a read-only
//! root filesystem, no capabilities, and read-only model mounts.

use crate::config;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Bump when the Containerfile, locks, or fetch script change so installs rebuild.
pub const IMAGE: &str = "localhost/vtd-phonon:1";
pub const SERVER_UNIT: &str = "vtd-phonon.service";
const CONTAINER: &str = "vtd-phonon";
/// Socket directory mount; `%t` is systemd's runtime dir (`$XDG_RUNTIME_DIR`).
const SOCKET_MOUNT: &str = "%t/vtd:/sock:rw,z";

const BUILD_FILES: &[(&str, &str)] = &[
    ("Containerfile", include_str!("../phonon/Containerfile")),
    ("requirements-torch.lock", include_str!("../phonon/requirements-torch.lock")),
    ("requirements.lock", include_str!("../phonon/requirements.lock")),
    ("fetch_model.py", include_str!("../phonon/fetch_model.py")),
];

pub fn phonon_dir() -> PathBuf {
    config::data_dir().join("phonon")
}

fn hf_dir() -> PathBuf {
    phonon_dir().join("hf")
}

fn cache_dir() -> PathBuf {
    phonon_dir().join("cache")
}

/// Absolute path of `podman` on PATH (systemd units need it spelled out).
pub fn find_podman() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    // Only absolute PATH entries: a relative one would yield a unit with a bogus ExecStart.
    std::env::split_paths(&path).filter(|d| d.is_absolute()).map(|d| d.join("podman")).find(|p| p.is_file())
}

/// Flags shared by every container vtd starts: no capabilities, read-only root,
/// the invoking user's uid (so mounted files stay owned by the user), scratch
/// space only in tmpfs.
fn sandbox_args() -> Vec<String> {
    [
        "--read-only", "--cap-drop", "all", "--security-opt", "no-new-privileges",
        "--userns=keep-id", "--pids-limit", "512", "--memory", "4g",
        "--tmpfs", "/tmp", "--tmpfs", "/home/vtd:size=64m",
        "-e", "HOME=/home/vtd", "-e", "HF_HOME=/hf", "-e", "HF_HUB_DISABLE_TELEMETRY=1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn mount(host: &Path, container: &str, mode: &str) -> Vec<String> {
    // `z` relabels for SELinux hosts and is ignored elsewhere.
    vec!["-v".to_string(), format!("{}:{container}:{mode},z", host.display())]
}

fn run(podman: &Path, args: &[String]) -> Result<(), String> {
    match Command::new(podman).args(args).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("`podman {}` exited with {s:?}", args.first().map(String::as_str).unwrap_or(""))),
        Err(e) => Err(format!("failed to run podman: {e}")),
    }
}

/// Build the image, download + verify the model, and warm the unpacked cache.
/// Returns the podman path for the unit files.
pub fn setup() -> Result<PathBuf, String> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(format!(
            "the Phonon backend is only built and tested for x86-64 Linux (this is {}). \
             Use `vtd install --backend whisper` instead.",
            std::env::consts::ARCH
        ));
    }
    let podman = find_podman().ok_or(
        "podman not found on PATH. Install rootless podman (e.g. `sudo apt install podman`), \
         or use `vtd install --backend whisper`.",
    )?;

    let build = phonon_dir().join("build");
    std::fs::create_dir_all(&build).map_err(|e| format!("failed to create {build:?}: {e}"))?;
    for (name, content) in BUILD_FILES {
        std::fs::write(build.join(name), content).map_err(|e| format!("failed to write {name}: {e}"))?;
    }
    for dir in [hf_dir(), cache_dir()] {
        std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create {dir:?}: {e}"))?;
    }

    println!("vtd: building the sandboxed Phonon image {IMAGE} (pinned base image and hash-locked packages, ~1.5 GB; a few minutes the first time)");
    run(
        &podman,
        &[
            "build".into(), "-t".into(), IMAGE.into(),
            "-f".into(), build.join("Containerfile").display().to_string(),
            build.display().to_string(),
        ],
    )?;

    println!("vtd: downloading the Phonon-2 model (164 MB, pinned revision, sha256-verified)");
    let mut fetch = vec!["run".to_string(), "--rm".to_string()];
    fetch.extend(sandbox_args());
    fetch.extend(mount(&hf_dir(), "/hf", "rw"));
    fetch.extend([IMAGE.into(), "python".into(), "/opt/vtd/fetch_model.py".into()]);
    run(&podman, &fetch)?;

    println!("vtd: unpacking and warming the model offline (about 30 s, once)");
    let mut warm = vec!["run".to_string(), "--rm".to_string(), "--network".to_string(), "none".to_string()];
    warm.extend(sandbox_args());
    warm.extend(["-e".into(), "HF_HUB_OFFLINE=1".into()]);
    warm.extend(mount(&hf_dir(), "/hf", "ro"));
    warm.extend(mount(&cache_dir(), "/home/vtd/.cache/fermion", "rw"));
    warm.extend([
        IMAGE.into(), "sh".into(), "-c".into(),
        "python -c 'import wave,struct;w=wave.open(\"/tmp/s.wav\",\"wb\");w.setnchannels(1);w.setsampwidth(2);\
         w.setframerate(16000);w.writeframes(struct.pack(\"<h\",0)*16000);w.close()' \
         && fermion transcribe phonon-2 /tmp/s.wav"
            .into(),
    ]);
    run(&podman, &warm)?;
    Ok(podman)
}

/// Quote one ExecStart= word for a systemd unit (also escapes specifiers and `$`).
pub fn unit_word(s: &str) -> String {
    let escaped = s.replace('%', "%%").replace('$', "$$").replace('\\', "\\\\").replace('"', "\\\"");
    if escaped.is_empty() || escaped.contains(|c: char| c.is_whitespace() || c == '\'') {
        format!("\"{escaped}\"")
    } else {
        escaped
    }
}

/// The systemd --user unit for the server container.
pub fn server_unit(podman: &Path) -> String {
    let p = unit_word(&podman.display().to_string());
    let mut run_args = vec!["run".to_string(), "--name".to_string(), CONTAINER.to_string(), "--rm".to_string(), "--network".to_string(), "none".to_string()];
    run_args.extend(sandbox_args());
    run_args.extend(["-e".into(), "HF_HUB_OFFLINE=1".into()]);
    run_args.extend(mount(&hf_dir(), "/hf", "ro"));
    run_args.extend(mount(&cache_dir(), "/home/vtd/.cache/fermion", "ro"));
    run_args.extend(["-v".into(), SOCKET_MOUNT.into()]);
    run_args.extend([IMAGE.into(), "fermion".into(), "serve".into(), "phonon-2".into(), "--unix-socket".into(), "/sock/phonon.sock".into()]);
    // %t (the runtime dir) must stay a specifier for the one fixed socket mount; every other
    // word, including user-controlled paths, is quoted and escaped.
    let run_line = run_args
        .iter()
        .map(|a| if a == SOCKET_MOUNT { a.clone() } else { unit_word(a) })
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        "[Unit]\n\
         Description=Phonon-2 speech server for vtd (sandboxed podman: no network, read-only, caps dropped)\n\
         \n\
         [Service]\n\
         ExecStartPre=-{p} rm -f {CONTAINER}\n\
         ExecStartPre=/bin/sh -c 'mkdir -p -m 700 %t/vtd && rm -f %t/vtd/phonon.sock'\n\
         ExecStart={p} {run_line}\n\
         ExecStartPost=/bin/sh -c 'for i in $$(seq 1 160); do curl -sf --unix-socket %t/vtd/phonon.sock http://localhost/health >/dev/null && exit 0; sleep 0.5; done; exit 1'\n\
         ExecStop={p} stop -t 5 {CONTAINER}\n\
         TimeoutStartSec=120\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

/// Remove the image and all downloaded model data.
pub fn purge() {
    if let Some(podman) = find_podman() {
        let _ = Command::new(&podman).args(["rm", "-f", CONTAINER]).output();
        match Command::new(&podman).args(["rmi", IMAGE]).status() {
            Ok(s) if s.success() => println!("vtd: removed image {IMAGE}"),
            _ => eprintln!("vtd: could not remove image {IMAGE} (already gone?)"),
        }
    }
    let dir = phonon_dir();
    if dir.exists() {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => println!("vtd: removed {dir:?}"),
            Err(e) => eprintln!("vtd: failed to remove {dir:?}: {e}"),
        }
    }
}

pub fn status() {
    let image = find_podman()
        .map(|p| Command::new(p).args(["image", "exists", IMAGE]).status().map(|s| s.success()).unwrap_or(false))
        .unwrap_or(false);
    println!("phonon image {IMAGE}: {}", if image { "present" } else { "MISSING (run `vtd install`)" });
    let unpacked = cache_dir().join("speech").exists();
    let fetched = hf_dir().join("hub").exists();
    println!("phonon model: {}", if unpacked && fetched { "downloaded and unpacked" } else { "MISSING (run `vtd install`)" });
    let sock = config::default_phonon_socket();
    println!("phonon socket {sock:?}: {}", if sock.exists() { "present" } else { "absent (server not running)" });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_word_quotes_and_escapes() {
        assert_eq!(unit_word("/usr/bin/podman"), "/usr/bin/podman");
        assert_eq!(unit_word("/home/a b/vtd"), "\"/home/a b/vtd\"");
        assert_eq!(unit_word("100%"), "100%%");
        assert_eq!(unit_word("$HOME"), "$$HOME");
        assert_eq!(unit_word("a\"b"), "a\\\"b");
        assert_eq!(unit_word(""), "\"\"");
    }

    #[test]
    fn server_unit_keeps_only_the_socket_specifier() {
        let unit = server_unit(Path::new("/usr/bin/podman"));
        let exec = unit.lines().find(|l| l.starts_with("ExecStart=")).unwrap();
        // The fixed runtime-dir mount is the only unescaped `%t` on the podman command line.
        assert_eq!(exec.matches("%t").count(), 1);
        assert!(exec.contains(" %t/vtd:/sock:rw,z "));
        for flag in ["--network none", "--read-only", "--cap-drop all", "no-new-privileges", ":ro,z"] {
            assert!(exec.contains(flag), "missing {flag}");
        }
        assert!(unit.contains("$$(seq"), "$ must be doubled in ExecStartPost");
    }

    #[test]
    fn find_podman_ignores_relative_path_entries() {
        let saved = std::env::var_os("PATH");
        // SAFETY: tests in this module do not run concurrently with other env readers of PATH.
        unsafe { std::env::set_var("PATH", "bin:.") };
        assert!(find_podman().is_none());
        match saved {
            Some(p) => unsafe { std::env::set_var("PATH", p) },
            None => unsafe { std::env::remove_var("PATH") },
        }
    }
}
