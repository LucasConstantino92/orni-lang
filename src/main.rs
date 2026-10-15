use std::{
    env, fs, io,
    io::Read,
    path::{Path, PathBuf},
    process,
};

use orni::{
    OrniDiagnostic, Project, check, diagnose_file, diagnose_file_with_source, diagnose_module,
    diagnose_module_buffers, diagnose_project_buffers, run_file,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq)]
enum Command<'a> {
    Run(&'a str),
    Serve(&'a str),
    Dev,
    Test(&'a str),
    New(&'a str, bool),
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
        "Orni {VERSION}\n\nUsage:\n  {executable} new <project-name> [--empty]\n  {executable} dev\n  {executable} run [file.orni | project-directory]\n  {executable} serve [file.orni | project-directory]\n  {executable} test [file.orni | project-directory]\n  {executable} check [file.orni | project-directory]\n  {executable} check-module <file.orni>\n  {executable} --version\n  {executable} --help\n\nWithout a path, run, serve, check, and test use the nearest orni.toml entry."
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
        [command] if command == "serve" => Some(Command::Serve(".")),
        [command] if command == "dev" => Some(Command::Dev),
        [command] if command == "test" => Some(Command::Test(".")),
        [command, name] if command == "new" => Some(Command::New(name, false)),
        [command, name, flag] if command == "new" && flag == "--empty" => {
            Some(Command::New(name, true))
        }
        [command] if command == "check" => Some(Command::Check(".")),
        [flag] if flag == "--help" || flag == "-h" => Some(Command::Help),
        [flag] if flag == "--version" || flag == "-V" => Some(Command::Version),
        [flag] if flag == "--check-stdin" => Some(Command::CheckStdin),
        [command, path] if command == "run" => Some(Command::Run(path)),
        [command, path] if command == "serve" => Some(Command::Serve(path)),
        [command, path] if command == "test" => Some(Command::Test(path)),
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
    if let Command::New(name, empty) = command {
        match create_project(name, empty) {
            Ok(message) => println!("{message}"),
            Err(message) => {
                eprintln!("{message}");
                process::exit(1);
            }
        }
        return;
    }

    let result = match command {
        Command::Run(path) => entry_path(path).and_then(run_file),
        Command::Serve(path) => {
            #[cfg(target_arch = "wasm32")]
            {
                let _ = path;
                eprintln!("'orni serve' requires the native Rust target");
                process::exit(2);
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                entry_path(path).and_then(|path| orni::serve_file(path).map(|()| String::new()))
            }
        }
        Command::Dev => {
            #[cfg(target_arch = "wasm32")]
            {
                eprintln!("'orni dev' requires the native Rust target");
                process::exit(2);
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                entry_path(".").and_then(|path| orni::serve_file(path).map(|()| String::new()))
            }
        }
        Command::Test(path) => run_tests(path),
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
        Command::Help | Command::Version | Command::New(_, _) => unreachable!(),
    };

    match result {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}

fn run_tests(path: &str) -> Result<String, orni::OrniError> {
    let path = PathBuf::from(portable_path(path));
    let tests_root = if path.is_file() {
        return run_file(&path).map(|_| format!("PASS {}\n\n1 test passed.\n", path.display()));
    } else {
        Project::discover(&path)?
            .map_or_else(|| path.join("tests"), |project| project.root.join("tests"))
    };
    let mut files = Vec::new();
    collect_orni_files(&tests_root, &mut files)?;
    if files.is_empty() {
        return Ok(format!(
            "No test files found in {}.\n",
            tests_root.display()
        ));
    }
    files.sort();
    let mut output = String::new();
    let mut passed = 0_usize;
    for file in files {
        match run_file(&file) {
            Ok(_) => {
                passed += 1;
                output.push_str(&format!("PASS {}\n", file.display()));
            }
            Err(error) => {
                output.push_str(&format!("FAIL {}\n{error}\n", file.display()));
                return Err(orni::OrniError::runtime(1, 1, output));
            }
        }
    }
    output.push_str(&format!(
        "\n{passed} test{} passed.\n",
        if passed == 1 { "" } else { "s" }
    ));
    Ok(output)
}

fn collect_orni_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), orni::OrniError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(orni::OrniError::runtime(1, 1, error.to_string())),
    };
    for entry in entries {
        let entry = entry.map_err(|error| orni::OrniError::runtime(1, 1, error.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_orni_files(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "orni")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn create_project(name: &str, empty: bool) -> Result<String, String> {
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return Err("project names use letters, numbers, underscores, or hyphens".to_owned());
    }
    let root = Path::new(name);
    if root.exists() {
        return Err(format!(
            "project directory '{}' already exists",
            root.display()
        ));
    }
    fs::create_dir_all(root.join("src/services")).map_err(|error| error.to_string())?;
    fs::create_dir_all(root.join("tests")).map_err(|error| error.to_string())?;
    fs::write(
        root.join("orni.toml"),
        format!("[project]\nname = \"{name}\"\nentry = \"src/main.orni\"\nsource = \"src\"\n"),
    )
    .map_err(|error| error.to_string())?;
    let source = if empty { EMPTY_TEMPLATE } else { CRUD_TEMPLATE };
    fs::write(root.join("src/main.orni"), source).map_err(|error| error.to_string())?;
    fs::write(root.join("src/services/health.orni"), HEALTH_TEMPLATE)
        .map_err(|error| error.to_string())?;
    fs::write(root.join("tests/health.orni"), TEST_TEMPLATE).map_err(|error| error.to_string())?;
    fs::write(
        root.join("README.md"),
        "# Orni project\n\nRun `orni dev` from this directory. Run `orni test` to execute files in `tests`. Test reusable modules such as `src/services/health.orni`; do not import `src/main.orni`, because it is the application entry point.\n",
    )
    .map_err(|error| error.to_string())?;
    Ok(format!(
        "Created {name}.\n\nNext:\n  cd {name}\n  orni dev\n  orni test"
    ))
}

const EMPTY_TEMPLATE: &str = r#"import services.health.{health};

/// Runs once before the development server starts.
func main() -> unit {
    app.name("Orni API");
    app.port(3000);
    print "Starting Orni API";
}

route get "/health" => health;
"#;

const CRUD_TEMPLATE: &str = r#"import services.health.{health};

/// Runs once before the development server starts.
func main() -> unit {
    app.name("Orni API");
    app.port(3000);
    print "Starting Orni API";
}

/// Describes a stored user.
model User {
    id: uuid;
    name: string<80>;
    email: string<254>;
}

/// Defines the fields a client supplies when creating a user.
model CreateUser {
    name: string<80>;
    email: string<254>;
}

/// Defines the fields a client supplies when replacing a user.
model UpdateUser {
    name: string<80>;
    email: string<254>;
}

/// Defines the fields a client can supply when partially updating a user.
model PatchUser {
    name: string<80>;
    email: string<254>;
}

store users: User;

func list_users() -> response {
    return users.list();
}

func create_user(input: CreateUser) -> response {
    return users.create(input);
}

func get_user(id: uuid) -> response {
    return users.find(id);
}

func update_user(id: uuid, input: UpdateUser) -> response {
    return users.replace(id, input);
}

func patch_user(id: uuid, input: PatchUser) -> response {
    return users.patch(id, input);
}

func delete_user(id: uuid) -> response {
    return users.delete(id);
}

route post "/users" => create_user;
route get "/health" => health;
route get "/users" => list_users;
route get "/users/:id" => get_user;
route put "/users/:id" => update_user;
route patch "/users/:id" => patch_user;
route delete "/users/:id" => delete_user;
"#;

const HEALTH_TEMPLATE: &str = r#"/// Confirms that the local API is running.
func health() -> string {
    return "OK";
}
"#;

const TEST_TEMPLATE: &str = r#"import services.health.{health};

/// A basic example of an Orni test.
assert health() == "OK";
"#;

#[cfg(test)]
mod tests {
    use super::{Command, HEALTH_TEMPLATE, parse_command, portable_path};

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
    fn accepts_the_native_serve_command() {
        assert_eq!(
            parse_command(&arguments(&["serve"])),
            Some(Command::Serve("."))
        );
        assert_eq!(
            parse_command(&arguments(&["serve", "examples/experimental/http_server"])),
            Some(Command::Serve("examples/experimental/http_server"))
        );
    }

    #[test]
    fn accepts_project_creation_and_development_commands() {
        assert_eq!(
            parse_command(&arguments(&["new", "my_api"])),
            Some(Command::New("my_api", false))
        );
        assert_eq!(
            parse_command(&arguments(&["new", "my_api", "--empty"])),
            Some(Command::New("my_api", true))
        );
        assert_eq!(parse_command(&arguments(&["dev"])), Some(Command::Dev));
        assert_eq!(
            parse_command(&arguments(&["test"])),
            Some(Command::Test("."))
        );
        assert_eq!(
            parse_command(&arguments(&["test", "my_api"])),
            Some(Command::Test("my_api"))
        );
        orni::check(HEALTH_TEMPLATE).expect("the reusable health module should compile");
    }

    #[test]
    fn normalizes_windows_paths_for_the_wasi_runtime() {
        assert_eq!(
            portable_path(r"examples\experimental\framework_project\main.orni"),
            "examples/experimental/framework_project/main.orni"
        );
    }
}
