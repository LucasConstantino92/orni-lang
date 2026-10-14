use std::{env, io, io::Read, process};

use orni::{OrniDiagnostic, check, diagnose_file, diagnose_module, run_file};

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

    let arguments: Vec<_> = args.collect();
    if arguments.is_empty() {
        eprintln!("Usage: {executable} [--check | --check-module] <file.orni> | --check-stdin");
        process::exit(2);
    }

    let result = match arguments.as_slice() {
        [flag] if flag == "--check-stdin" => {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .unwrap_or_else(|error| {
                    eprintln!("Could not read standard input: {error}");
                    process::exit(1);
                });
            check(&source).map(|()| String::new())
        }
        [flag, path] if flag == "--check" => {
            diagnose_file(path).map(|diagnostics| format_diagnostics(&diagnostics))
        }
        [flag, path] if flag == "--check-module" => {
            diagnose_module(path).map(|diagnostics| format_diagnostics(&diagnostics))
        }
        [path] => run_file(path),
        _ => {
            eprintln!("Usage: {executable} [--check | --check-module] <file.orni> | --check-stdin");
            process::exit(2);
        }
    };

    match result {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}
