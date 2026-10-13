use std::{env, fs, io, io::Read, process};

use orni::{check, run};

fn main() {
    let mut args = env::args();
    let executable = args.next().unwrap_or_else(|| "orni".to_string());

    let arguments: Vec<_> = args.collect();
    if arguments.is_empty() {
        eprintln!("Usage: {executable} [--check] <file.orni> | --check-stdin");
        process::exit(2);
    }

    let (source, check_only) = match arguments.as_slice() {
        [flag] if flag == "--check-stdin" => {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .unwrap_or_else(|error| {
                    eprintln!("Could not read standard input: {error}");
                    process::exit(1);
                });
            (source, true)
        }
        [flag, path] if flag == "--check" => (read_source(path), true),
        [path] => (read_source(path), false),
        _ => {
            eprintln!("Usage: {executable} [--check] <file.orni> | --check-stdin");
            process::exit(2);
        }
    };

    let result = if check_only {
        check(&source).map(|()| String::new())
    } else {
        run(&source)
    };

    match result {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}

/// Reads an Orni source file or exits with a useful command-line error.
fn read_source(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| {
        eprintln!("Could not read '{path}': {error}");
        process::exit(1);
    })
}
