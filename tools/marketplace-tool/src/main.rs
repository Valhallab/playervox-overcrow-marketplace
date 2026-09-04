mod admission;
mod package;
mod snapshot;

use std::{env, path::Path, process::ExitCode};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("package") => {
            let arguments = args.collect::<Vec<_>>();
            let [source, destination] = arguments.as_slice() else {
                eprintln!("error: invalid package arguments");
                return ExitCode::FAILURE;
            };
            match package::write_package(Path::new(source), Path::new(destination)) {
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
            let arguments = args.collect::<Vec<_>>();
            let [archive] = arguments.as_slice() else {
                eprintln!("error: invalid inspection arguments");
                return ExitCode::FAILURE;
            };
            match package::inspect(Path::new(archive)) {
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
        Some("ingest") => {
            let arguments = args.collect::<Vec<_>>();
            if arguments.len() != 12
                || arguments[0] != "--receipt"
                || arguments[2] != "--artifacts"
                || arguments[4] != "--store"
                || arguments[6] != "--trust-sha"
                || arguments[8] != "--review-sha"
                || arguments[10] != "--review-tree"
            {
                eprintln!("error: invalid admission ingestion arguments");
                return ExitCode::FAILURE;
            }
            let expected = admission::ExpectedAdmission {
                trust_sha: &arguments[7],
                review_sha: &arguments[9],
                review_tree: &arguments[11],
            };
            match admission::ingest(
                Path::new(&arguments[1]),
                Path::new(&arguments[3]),
                Path::new(&arguments[5]),
                &expected,
            ) {
                Ok(stored) => {
                    println!("{} {}", stored.review_tree, stored.artifact_count);
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("verify-admission") => {
            let arguments = args.collect::<Vec<_>>();
            if arguments.len() != 4 || arguments[0] != "--store" || arguments[2] != "--review-tree"
            {
                eprintln!("error: invalid admission verification arguments");
                return ExitCode::FAILURE;
            }
            match admission::verify(Path::new(&arguments[1]), &arguments[3]) {
                Ok(stored) => {
                    println!("{} {}", stored.review_tree, stored.artifact_count);
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
            eprintln!(
                "       marketplace-tool ingest --receipt <path> --artifacts <directory> --store <directory> --trust-sha <sha> --review-sha <sha> --review-tree <tree>"
            );
            eprintln!(
                "       marketplace-tool verify-admission --store <directory> --review-tree <tree>"
            );
            eprintln!("       marketplace-tool snapshot-plan --repository <path> --revision <sha>");
            ExitCode::FAILURE
        }
    }
}
