mod config;
mod daemon;
mod install;
mod keyboard;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None | Some("run") => daemon::run(config::Config::from_env()),
        Some("install") => install::install(&parse_model_flag(&args[1..])),
        Some("uninstall") => install::uninstall(),
        Some("status") => install::status(),
        Some("-h") | Some("--help") | Some("help") => print_help(),
        Some(other) => {
            eprintln!("vtd: unknown command {other:?}\n");
            print_help();
            std::process::exit(1);
        }
    }
}

fn parse_model_flag(args: &[String]) -> String {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--model" {
            if let Some(v) = args.get(i + 1) {
                return v.clone();
            }
            eprintln!("vtd: --model requires a value");
            std::process::exit(1);
        }
        i += 1;
    }
    "large-v3-turbo".to_string()
}

fn print_help() {
    println!(
        "vtd - push-to-talk local voice dictation daemon\n\
         \n\
         Usage:\n\
         \x20 vtd [run]              Run the daemon in the foreground (default)\n\
         \x20 vtd install [--model NAME]\n\
         \x20                        Download a whisper.cpp model and install/start\n\
         \x20                        vtd as a systemd --user service\n\
         \x20 vtd uninstall          Stop and remove the systemd --user service\n\
         \x20 vtd status             Show model/binary/service status\n\
         \n\
         Configuration (environment variables):\n\
         \x20 VTD_KEYBOARD_DEVICE    /dev/input/eventN (default: autodetected)\n\
         \x20 VTD_TRIGGER_KEY        Linux key code to hold (default: 100 = KEY_RIGHTALT)\n\
         \x20 VTD_MIC_TARGET         PipeWire source target id/name (default: PipeWire default)\n\
         \x20 VTD_WHISPER_BIN        Path to whisper-cli (default: ~/.local/share/vtd/whisper.cpp/build/bin/whisper-cli)\n\
         \x20 VTD_WHISPER_MODEL      Path to a ggml model file (default: ~/.local/share/vtd/models/ggml-large-v3-turbo.bin)\n\
         \x20 VTD_WHISPER_LD_LIBRARY_PATH\n\
         \x20                        Extra LD_LIBRARY_PATH for whisper-cli (e.g. a non-standard ROCm install)\n"
    );
}
