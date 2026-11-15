use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    caligo_cli::run(&args, "caligo-cli")
}
