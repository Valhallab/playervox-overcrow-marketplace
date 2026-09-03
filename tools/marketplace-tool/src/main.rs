mod package;

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
        _ => {
            eprintln!("usage: marketplace-tool package <source-dir> <destination.ocpkg>");
            eprintln!("       marketplace-tool inspect <package.ocpkg>");
            ExitCode::FAILURE
        }
    }
}
