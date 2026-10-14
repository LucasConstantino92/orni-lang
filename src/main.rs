use std::{env, io, io::Read, process};

use orni::{OrniDiagnostic, check, diagnose_file, diagnose_module, run_file};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq)]
enum Command<'a> {
    Run(&'a str),
    Check(&'a str),
    CheckModule(&'a str),
    CheckStdin,
    Help,
    Version,
}

fn usage(executable: &str) -> String {
    format!(
        "Orni {VERSION}\n\nUsage:\n  {executable} run <file.orni>\n  {executable} check <file.orni>\n  {executable} check-module <file.orni>\n  {executable} --version\n  {executable} --help"
    )
}

fn parse_command(arguments: &[String]) -> Option<Command<'_>> {
    match arguments {
        [flag] if flag == "--help" || flag == "-h" => Some(Command::Help),
        [flag] if flag == "--version" || flag == "-V" => Some(Command::Version),
        [flag] if flag == "--check-stdin" => Some(Command::CheckStdin),
        [command, path] if command == "run" => Some(Command::Run(path)),
        [command, path] if command == "check" || command == "--check" => Some(Command::Check(path)),
        [command, path] if command == "check-module" || command == "--check-module" => {
            Some(Command::CheckModule(path))
        }
        [path] if path.ends_with(".orni") => Some(Command::Run(path)),
        _ => None,
    }
}

fn format_diagnostics(diagnostics: &[OrniDiagnostic]) -> String {
    let mut output = diagnostics
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    output
}

fn main() {
    let mut args = env::args();
    let executable = args.next().unwrap_or_else(|| "orni".to_string());
    let arguments = args.collect::<Vec<_>>();
    let Some(command) = parse_command(&arguments) else {
        eprintln!("{}", usage(&executable));
        process::exit(2);
    };

    if command == Command::Help {
        println!("{}", usage(&executable));
        return;
    }
    if command == Command::Version {
        println!("orni {VERSION}");
        return;
    }

    let result = match command {
        Command::Run(path) => run_file(path),
        Command::Check(path) => {
            diagnose_file(path).map(|diagnostics| format_diagnostics(&diagnostics))
        }
        Command::CheckModule(path) => {
            diagnose_module(path).map(|diagnostics| format_diagnostics(&diagnostics))
        }
        Command::CheckStdin => {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .unwrap_or_else(|error| {
                    eprintln!("Could not read standard input: {error}");
                    process::exit(1);
                });
            check(&source).map(|()| String::new())
        }
        Command::Help | Command::Version => unreachable!(),
    };

    match result {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_command};

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn accepts_friendly_run_and_check_commands() {
        let run = arguments(&["run", "examples/hello.orni"]);
        let check = arguments(&["check", "examples/hello.orni"]);

        assert_eq!(
            parse_command(&run),
            Some(Command::Run("examples/hello.orni"))
        );
        assert_eq!(
            parse_command(&check),
            Some(Command::Check("examples/hello.orni"))
        );
    }

    #[test]
    fn preserves_the_original_single_file_command() {
        let command = arguments(&["examples/hello.orni"]);
        assert_eq!(
            parse_command(&command),
            Some(Command::Run("examples/hello.orni"))
        );
    }
}
