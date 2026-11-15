use std::process::ExitCode;

use caligo_core::commands::{daemon, manager, media};

const USAGE: &str = "\
Caligo for Windows

Usage
  caligo.exe manage [--terminal] [--root <absolute-private-root>]
  caligo.exe daemon --pipe-prefix <name> --journal <path> --baseline <id> --account <uin>
  caligo.exe manager --root <absolute-private-root> [--check] [--set-config <absolute-json>]
                     [--select] [--management-bind <loopback-address>]
  caligo.exe media --config <absolute-json> <operation> --input <absolute-file>
  caligo.exe cli <command>

Run caligo.exe cli --help for probe and loader commands.
See README.md for startup and configuration.
";

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args
        .split_first()
        .map(|(command, rest)| (command.as_str(), rest))
    {
        None | Some(("-h" | "--help", [])) => print!("{USAGE}"),
        Some(("manage", rest)) => manager::guided_entry(rest),
        Some(("daemon", rest)) => daemon::entry(rest, "caligo.exe daemon"),
        Some(("manager", rest)) => manager::entry(rest),
        Some(("media", rest)) => media::entry(rest, "caligo.exe media"),
        Some(("cli", rest)) => {
            if rest.is_empty() || (rest.len() == 1 && matches!(rest[0].as_str(), "-h" | "--help")) {
                print!("{}", caligo_cli::usage("caligo.exe cli"));
            } else {
                return caligo_cli::run(rest, "caligo.exe cli");
            }
        }
        Some((command, _)) => {
            eprintln!("Unknown command: {command}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
