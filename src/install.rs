use crate::config::{self, Backend};
use crate::phonon;
use std::path::Path;
use std::process::Command;

/// One `Environment=` line, quoted so spaces survive, `%` is not expanded as a specifier,
/// and a value cannot smuggle extra directives in through a newline.
fn env_line(key: &str, val: &str) -> String {
    if val.chars().any(|c| c.is_control()) {
        eprintln!("vtd: refusing {key}: value contains control characters");
        std::process::exit(1);
    }
    let escaped = val.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%");
    format!("Environment=\"{key}={escaped}\"\n")
}

pub struct Options {
    pub backend: Backend,
    /// whisper.cpp model name (whisper backend only)
    pub model: String,
    /// enable and (re)start the services after writing the units
    pub start: bool,
}

const KNOWN_MODELS: &[&str] = &[
    "tiny", "tiny.en", "base", "base.en", "small", "small.en", "medium", "medium.en", "large-v1",
    "large-v2", "large-v3", "large-v3-turbo",
];

const FORWARDED_ENV_VARS: &[&str] = &[
    "VTD_KEYBOARD_DEVICE",
    "VTD_MIC_TARGET",
    "VTD_WHISPER_BIN",
    "VTD_WHISPER_LD_LIBRARY_PATH",
    "VTD_TRIGGER_KEY",
    "VTD_KEY_DELAY",
    "VTD_KEY_HOLD",
];

pub fn install(opts: &Options) {
    match opts.backend {
        Backend::Phonon => install_phonon(opts),
        Backend::Whisper => install_whisper(opts),
    }
}

fn install_phonon(opts: &Options) {
    let podman = match phonon::setup() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("vtd: {e}");
            std::process::exit(1);
        }
    };

    let exe = current_exe();
    let mut env_lines = String::from("Environment=VTD_BACKEND=phonon\n");
    // A whisper.cpp build the user already has stays usable as the fallback.
    for key in FORWARDED_ENV_VARS.iter().chain(["VTD_WHISPER_MODEL"].iter()) {
        if let Ok(val) = std::env::var(key) {
            env_lines.push_str(&env_line(key, &val));
        }
    }

    write_unit(phonon::SERVER_UNIT, &phonon::server_unit(&podman));
    write_unit("vtd.service", &vtd_unit(&exe, &env_lines, true));

    if opts.start {
        let mut ok = run_systemctl(&["daemon-reload"]);
        ok &= run_systemctl(&["enable", phonon::SERVER_UNIT, "vtd.service"]);
        println!("vtd: starting the Phonon server (loads the model, about 15 s)...");
        ok &= run_systemctl(&["restart", phonon::SERVER_UNIT]);
        ok &= run_systemctl(&["restart", "vtd.service"]);
        if !ok {
            eprintln!("vtd: installed, but starting the services failed; see `journalctl --user -u vtd-phonon -u vtd -e`");
            std::process::exit(1);
        }
        println!("vtd: installed. Hold Right Alt, speak, release.");
        println!("vtd: check with `vtd status`, or `journalctl --user -u vtd-phonon -u vtd -f`.");
    } else {
        println!("vtd: units written; not started (--no-start). Enable with `systemctl --user enable --now vtd-phonon vtd`.");
    }
}

fn current_exe() -> std::path::PathBuf {
    match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("vtd: could not determine my own executable path: {e}");
            std::process::exit(1);
        }
    }
}

fn write_unit(name: &str, content: &str) {
    let dir = config::systemd_user_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("vtd: failed to create {dir:?}: {e}");
        std::process::exit(1);
    }
    let path = dir.join(name);
    if let Err(e) = std::fs::write(&path, content) {
        eprintln!("vtd: failed to write {path:?}: {e}");
        std::process::exit(1);
    }
    println!("vtd: wrote {path:?}");
}

fn vtd_unit(exe: &Path, env_lines: &str, with_phonon: bool) -> String {
    let (after, wants) = if with_phonon {
        (" vtd-phonon.service", "Wants=vtd-phonon.service\n")
    } else {
        ("", "")
    };
    format!(
        "[Unit]\n\
         Description=vtd push-to-talk voice dictation daemon\n\
         After=graphical-session.target pipewire.service{after}\n\
         {wants}\
         \n\
         [Service]\n\
         ExecStart={}\n\
         {env_lines}\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        phonon::unit_word(&exe.display().to_string()),
    )
}

fn install_whisper(opts: &Options) {
    let model = opts.model.as_str();
    if !KNOWN_MODELS.contains(&model) {
        eprintln!("vtd: unknown model {model:?}. Known models: {}", KNOWN_MODELS.join(", "));
        std::process::exit(1);
    }

    let models_dir = config::models_dir();
    if let Err(e) = std::fs::create_dir_all(&models_dir) {
        eprintln!("vtd: failed to create {models_dir:?}: {e}");
        std::process::exit(1);
    }

    let model_path = models_dir.join(format!("ggml-{model}.bin"));
    if model_path.exists() {
        println!("vtd: model already present at {model_path:?}, skipping download");
    } else {
        let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model}.bin");
        println!("vtd: downloading {model} model from {url}");
        let tmp_path = models_dir.join(format!("ggml-{model}.bin.part"));
        let status = Command::new("curl")
            .arg("-L").arg("--fail").arg("--progress-bar")
            .arg("-o").arg(&tmp_path)
            .arg(&url)
            .status();
        match status {
            Ok(s) if s.success() => {
                if let Err(e) = std::fs::rename(&tmp_path, &model_path) {
                    eprintln!("vtd: downloaded model but failed to move into place: {e}");
                    std::process::exit(1);
                }
            }
            Ok(s) => {
                eprintln!("vtd: model download failed (curl exited with {s:?})");
                let _ = std::fs::remove_file(&tmp_path);
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("vtd: failed to run curl: {e}");
                std::process::exit(1);
            }
        }
    }

    let whisper_bin = std::env::var("VTD_WHISPER_BIN")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| config::data_dir().join("whisper.cpp/build/bin/whisper-cli"));
    if !whisper_bin.exists() {
        println!(
            "vtd: whisper-cli not found at {whisper_bin:?}.\n\
             Build it first: run scripts/build-whisper.sh from the vtd source checkout,\n\
             or set VTD_WHISPER_BIN to point at an existing whisper.cpp build."
        );
    }

    let exe = current_exe();
    let mut env_lines = format!(
        "Environment=VTD_BACKEND=whisper\n{}",
        env_line("VTD_WHISPER_MODEL", &model_path.display().to_string())
    );
    for key in FORWARDED_ENV_VARS {
        if let Ok(val) = std::env::var(key) {
            env_lines.push_str(&env_line(key, &val));
        }
    }

    // Switching back from the Phonon backend: drop its server.
    let server_unit = config::systemd_user_dir().join(phonon::SERVER_UNIT);
    if server_unit.exists() && opts.start {
        run_systemctl(&["disable", "--now", phonon::SERVER_UNIT]);
        let _ = std::fs::remove_file(&server_unit);
    }

    write_unit("vtd.service", &vtd_unit(&exe, &env_lines, false));
    if opts.start {
        let ok = run_systemctl(&["daemon-reload"]) & run_systemctl(&["enable", "vtd.service"]) & run_systemctl(&["restart", "vtd.service"]);
        if !ok {
            eprintln!("vtd: installed, but starting the service failed; see `journalctl --user -u vtd -e`");
            std::process::exit(1);
        }
        println!("vtd: installed and started as a systemd --user service.");
        println!("vtd: check status with `systemctl --user status vtd.service` or `vtd status`.");
    }
}

pub fn uninstall(purge: bool) {
    for unit in ["vtd.service", phonon::SERVER_UNIT] {
        run_systemctl(&["disable", "--now", unit]);
        let unit_path = config::systemd_user_dir().join(unit);
        if unit_path.exists() {
            if let Err(e) = std::fs::remove_file(&unit_path) {
                eprintln!("vtd: failed to remove {unit_path:?}: {e}");
            } else {
                println!("vtd: removed {unit_path:?}");
            }
        }
    }
    run_systemctl(&["daemon-reload"]);
    if purge {
        phonon::purge();
        println!("vtd: service uninstalled and Phonon image and model removed.");
    } else {
        println!(
            "vtd: service uninstalled. The Phonon image/model and any whisper.cpp files in {:?} were left in place; \
             `vtd uninstall --purge` removes the Phonon image and model.",
            config::data_dir()
        );
    }
}

pub fn status() {
    phonon::status();
    println!();
    let whisper_bin = std::env::var("VTD_WHISPER_BIN")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| config::data_dir().join("whisper.cpp/build/bin/whisper-cli"));
    println!("whisper-cli: {} ({})", whisper_bin.display(), if whisper_bin.exists() { "found" } else { "MISSING" });

    let models_dir = config::models_dir();
    match std::fs::read_dir(&models_dir) {
        Ok(entries) => {
            let names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            println!("models in {:?}: {}", models_dir, if names.is_empty() { "(none)".to_string() } else { names.join(", ") });
        }
        Err(_) => println!("models dir {models_dir:?}: (missing)"),
    }

    println!();
    let _ = Command::new("systemctl").arg("--user").arg("status").arg(phonon::SERVER_UNIT).arg("vtd.service").arg("--no-pager").status();
}

fn run_systemctl(args: &[&str]) -> bool {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!("vtd: systemctl --user {} exited with {:?}", args.join(" "), s);
            false
        }
        Err(e) => {
            eprintln!("vtd: failed to run systemctl: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_line_quotes_and_escapes_values() {
        assert_eq!(env_line("VTD_MIC_TARGET", "53"), "Environment=\"VTD_MIC_TARGET=53\"\n");
        assert_eq!(env_line("K", "a b"), "Environment=\"K=a b\"\n");
        assert_eq!(env_line("K", "50%"), "Environment=\"K=50%%\"\n");
        assert_eq!(env_line("K", "say \"hi\""), "Environment=\"K=say \\\"hi\\\"\"\n");
    }

    #[test]
    fn vtd_unit_orders_after_the_phonon_server() {
        let unit = vtd_unit(Path::new("/opt/my vtd/vtd"), "Environment=\"VTD_BACKEND=phonon\"\n", true);
        assert!(unit.contains("After=graphical-session.target pipewire.service vtd-phonon.service"));
        assert!(unit.contains("Wants=vtd-phonon.service"));
        assert!(unit.contains("ExecStart=\"/opt/my vtd/vtd\""));
        assert!(!vtd_unit(Path::new("/x"), "", false).contains("vtd-phonon"));
    }
}
