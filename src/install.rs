use crate::config;
use std::process::Command;

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
];

pub fn install(model: &str) {
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

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("vtd: could not determine my own executable path: {e}");
            std::process::exit(1);
        }
    };

    let mut env_lines = format!("Environment=VTD_WHISPER_MODEL={}\n", model_path.display());
    for key in FORWARDED_ENV_VARS {
        if let Ok(val) = std::env::var(key) {
            env_lines.push_str(&format!("Environment={key}={val}\n"));
        }
    }

    let unit = format!(
        "[Unit]\n\
         Description=vtd push-to-talk voice dictation daemon\n\
         After=graphical-session.target pipewire.service\n\
         \n\
         [Service]\n\
         ExecStart={}\n\
         {}\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe.display(),
        env_lines,
    );

    let systemd_dir = config::systemd_user_dir();
    if let Err(e) = std::fs::create_dir_all(&systemd_dir) {
        eprintln!("vtd: failed to create {systemd_dir:?}: {e}");
        std::process::exit(1);
    }
    let unit_path = systemd_dir.join("vtd.service");
    if let Err(e) = std::fs::write(&unit_path, unit) {
        eprintln!("vtd: failed to write {unit_path:?}: {e}");
        std::process::exit(1);
    }
    println!("vtd: wrote {unit_path:?}");

    run_systemctl(&["daemon-reload"]);
    run_systemctl(&["enable", "--now", "vtd.service"]);

    println!("vtd: installed and started as a systemd --user service.");
    println!("vtd: check status with `systemctl --user status vtd.service` or `vtd status`.");
}

pub fn uninstall() {
    run_systemctl(&["disable", "--now", "vtd.service"]);
    let unit_path = config::systemd_user_dir().join("vtd.service");
    if unit_path.exists() {
        if let Err(e) = std::fs::remove_file(&unit_path) {
            eprintln!("vtd: failed to remove {unit_path:?}: {e}");
        } else {
            println!("vtd: removed {unit_path:?}");
        }
    }
    run_systemctl(&["daemon-reload"]);
    println!("vtd: service uninstalled. Models and the whisper.cpp checkout in {:?} were left in place.", config::data_dir());
}

pub fn status() {
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
    let _ = Command::new("systemctl").arg("--user").arg("status").arg("vtd.service").arg("--no-pager").status();
}

fn run_systemctl(args: &[&str]) {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("vtd: systemctl --user {} exited with {:?}", args.join(" "), s),
        Err(e) => eprintln!("vtd: failed to run systemctl: {e}"),
    }
}
