use std::{env, io, io::Read, process};

use orni::{
    OrniDiagnostic, Project, check, diagnose_file, diagnose_file_with_source, diagnose_module,
    diagnose_module_buffers, diagnose_project_buffers, run_file,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq)]
enum Command<'a> {
    Run(&'a str),
    Check(&'a str),
    CheckModule(&'a str),
    CheckStdin,
    CheckOverlay(&'a str, &'a str),
    CheckBuffers(&'a str),
    CheckModuleBuffers(&'a str),
    Help,
    Version,
}

fn usage(executable: &str) -> String {
    format!(
        "Orni {VERSION}\n\nUsage:\n  {executable} run [file.orni | project-directory]\n  {executable} check [file.orni | project-directory]\n  {executable} check-module <file.orni>\n  {executable} --version\n  {executable} --help\n\nWithout a file, run and check use the nearest orni.toml entry."
    )
}

fn parse_command(arguments: &[String]) -> Option<Command<'_>> {
    match arguments {
        [command, entry] if command == "--check-module-buffers" => {
            Some(Command::CheckModuleBuffers(entry))
        }
        [command, entry] if command == "--check-buffers" => Some(Command::CheckBuffers(entry)),
        [command, entry, file] if command == "--check-overlay" => {
            Some(Command::CheckOverlay(entry, file))
        }
        [command] if command == "run" => Some(Command::Run(".")),
        [command] if command == "check" => Some(Command::Check(".")),
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

fn portable_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn entry_path(path: &str) -> Result<std::path::PathBuf, orni::OrniError> {
    let path = std::path::PathBuf::from(portable_path(path));
    if path.is_dir() {
        Ok(Project::discover(&path)?
            .map_or_else(|| path.join("main.orni"), |project| project.entry))
    } else {
        Ok(path)
    }
}

fn read_source(limit: u64) -> String {
    let mut source = String::new();
    io::stdin()
        .take(limit + 1)
        .read_to_string(&mut source)
        .unwrap_or_else(|error| {
            eprintln!("Could not read standard input: {error}");
            process::exit(1);
        });
    source
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
        Command::Run(path) => entry_path(path).and_then(run_file),
        Command::Check(path) => entry_path(path)
            .and_then(diagnose_file)
            .map(|diagnostics| format_diagnostics(&diagnostics)),
        Command::CheckModule(path) => {
            diagnose_module(portable_path(path)).map(|diagnostics| format_diagnostics(&diagnostics))
        }
        Command::CheckStdin => {
            let source = read_source(1_048_576);
            check(&source).map(|()| String::new())
        }
        Command::CheckOverlay(entry, file) => diagnose_file_with_source(
            portable_path(entry),
            portable_path(file),
            &read_source(1_048_576),
        )
        .map(|diagnostics| format_diagnostics(&diagnostics)),
        Command::CheckBuffers(entry) => {
            diagnose_project_buffers(portable_path(entry), &read_source(8_388_608))
                .map(|diagnostics| format_diagnostics(&diagnostics))
        }
        Command::CheckModuleBuffers(entry) => {
            diagnose_module_buffers(portable_path(entry), &read_source(8_388_608))
                .map(|diagnostics| format_diagnostics(&diagnostics))
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
    use super::{Command, parse_command, portable_path};

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

    #[test]
    fn normalizes_windows_paths_for_the_wasi_runtime() {
        assert_eq!(
            portable_path(r"examples\experimental\framework_project\main.orni"),
            "examples/experimental/framework_project/main.orni"
        );
    }
}
