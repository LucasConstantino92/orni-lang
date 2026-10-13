use std::{env, fs, process};

use orni::run;

fn main() {
    let mut args = env::args();
    let executable = args.next().unwrap_or_else(|| "orni".to_string());

    let Some(path) = args.next() else {
        eprintln!("Usage: {executable} <file.orni>");
        process::exit(2);
    };

    if args.next().is_some() {
        eprintln!("Error: provide exactly one .orni file");
        process::exit(2);
    }

    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        eprintln!("Could not read '{path}': {error}");
        process::exit(1);
    });

    match run(&source) {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}
