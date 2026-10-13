use std::{env, fs, process};

use orni::{check, run};

fn main() {
    let mut args = env::args();
    let executable = args.next().unwrap_or_else(|| "orni".to_string());

    let Some(first_argument) = args.next() else {
        eprintln!("Usage: {executable} [--check] <file.orni>");
        process::exit(2);
    };

    let (check_only, path) = if first_argument == "--check" {
        let Some(path) = args.next() else {
            eprintln!("Usage: {executable} --check <file.orni>");
            process::exit(2);
        };
        (true, path)
    } else {
        (false, first_argument)
    };

    if args.next().is_some() {
        eprintln!("Error: provide exactly one .orni file");
        process::exit(2);
    }

    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        eprintln!("Could not read '{path}': {error}");
        process::exit(1);
    });

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
