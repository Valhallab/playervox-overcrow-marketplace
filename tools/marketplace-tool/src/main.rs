mod package;
mod snapshot;

use std::{env, path::Path, process::ExitCode};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("package") => {
            let source = args.next().expect("source directory");
            let destination = args.next().expect("destination .ocpkg");
            match package::write_package(Path::new(&source), Path::new(&destination)) {
                Ok(written) => {
                    println!(
                        "{} {}",
                        package::sha256_hex(&written.digest),
                        written.path.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("inspect") => {
            let archive = args.next().expect("package path");
            match package::inspect(Path::new(&archive)) {
                Ok(manifest) => {
                    println!("{} {}", manifest.id, manifest.version);
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("snapshot-plan") => {
            let arguments = args.collect::<Vec<_>>();
            if arguments.len() != 4
                || arguments[0] != "--repository"
                || arguments[2] != "--revision"
            {
                eprintln!(
                    "usage: marketplace-tool snapshot-plan --repository <path> --revision <sha>"
                );
                return ExitCode::FAILURE;
            }
            match snapshot::write_plan(Path::new(&arguments[1]), &arguments[3]) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: marketplace-tool package <source-dir> <destination.ocpkg>");
            eprintln!("       marketplace-tool inspect <package.ocpkg>");
            eprintln!("       marketplace-tool snapshot-plan --repository <path> --revision <sha>");
            ExitCode::FAILURE
        }
    }
}
