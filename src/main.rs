mod config;
mod daemon;
mod install;
mod keyboard;
mod phonon;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None | Some("run") => daemon::run(config::Config::from_env()),
        Some("install") => install::install(&parse_install_flags(&args[1..])),
        Some("uninstall") => install::uninstall(args[1..].iter().any(|a| a == "--purge")),
        Some("status") => install::status(),
        Some("-h") | Some("--help") | Some("help") => print_help(),
        Some(other) => {
            eprintln!("vtd: unknown command {other:?}\n");
            print_help();
            std::process::exit(1);
        }
    }
}

fn parse_install_flags(args: &[String]) -> install::Options {
    let mut opts = install::Options {
        backend: config::backend_from_env(),
        model: "large-v3-turbo".to_string(),
        start: true,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--backend" | "--model" => {
                let Some(v) = args.get(i + 1) else {
                    eprintln!("vtd: {} requires a value", args[i]);
                    std::process::exit(1);
                };
                if args[i] == "--model" {
                    opts.model = v.clone();
                } else {
                    opts.backend = match v.as_str() {
                        "phonon" => config::Backend::Phonon,
                        "whisper" => config::Backend::Whisper,
                        other => {
                            eprintln!("vtd: unknown backend {other:?} (expected \"phonon\" or \"whisper\")");
                            std::process::exit(1);
                        }
                    };
                }
                i += 1;
            }
            "--no-start" => opts.start = false,
            other => {
                eprintln!("vtd: unknown install option {other:?}\n");
                print_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }
    opts
}

fn print_help() {
    println!(
        "vtd - push-to-talk local voice dictation daemon\n\
         \n\
         Usage:\n\
         \x20 vtd [run]              Run the daemon in the foreground (default)\n\
         \x20 vtd install [--backend phonon|whisper] [--model NAME] [--no-start]\n\
         \x20                        Set up a backend and install vtd as a systemd --user service.\n\
         \x20                        phonon (default): build the sandboxed Phonon-2 image with podman,\n\
         \x20                        download the pinned model, install the server unit.\n\
         \x20                        whisper: download a whisper.cpp model (--model NAME, English and\n\
         \x20                        multilingual; needs a whisper.cpp build, see scripts/build-whisper.sh).\n\
         \x20 vtd uninstall [--purge]\n\
         \x20                        Stop and remove the services; --purge also removes the Phonon\n\
         \x20                        image and downloaded model\n\
         \x20 vtd status             Show backend, model and service status\n\
         \n\
         Configuration (environment variables):\n\
         \x20 VTD_BACKEND            phonon (default) or whisper\n\
         \x20 VTD_KEYBOARD_DEVICE    /dev/input/eventN (default: autodetected)\n\
         \x20 VTD_TRIGGER_KEY        Linux key code to hold (default: 100 = KEY_RIGHTALT)\n\
         \x20 VTD_MIC_TARGET         PipeWire source target id/name (default: PipeWire default)\n\
         \x20 VTD_PHONON_SOCKET      Unix socket of the Phonon server (default: $XDG_RUNTIME_DIR/vtd/phonon.sock)\n\
         \x20 VTD_WHISPER_BIN        Path to whisper-cli (default: ~/.local/share/vtd/whisper.cpp/build/bin/whisper-cli);\n\
         \x20                        with the phonon backend it is the fallback if the server is down\n\
         \x20 VTD_WHISPER_MODEL      Path to a ggml model file (default: ~/.local/share/vtd/models/ggml-large-v3-turbo.bin)\n\
         \x20 VTD_WHISPER_LD_LIBRARY_PATH\n\
         \x20                        Extra LD_LIBRARY_PATH for whisper-cli (e.g. a non-standard ROCm install)\n"
    );
}
