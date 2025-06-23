// Command-line argument parsing logic

use std::path::{Path, PathBuf};

pub fn parse_args() -> (PathBuf, PathBuf) {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let arg_path = Path::new(&args[1]);
        if arg_path.is_file() {
            (
                arg_path.to_path_buf(),
                arg_path
                    .parent()
                    .map_or_else(|| Path::new(".").to_path_buf(), Path::to_path_buf),
            )
        } else if arg_path.is_dir() {
            (arg_path.join("linkfield.redb"), arg_path.to_path_buf())
        } else {
            (
                Path::new("test.redb").to_path_buf(),
                Path::new(".").to_path_buf(),
            )
        }
    } else {
        (
            Path::new("test.redb").to_path_buf(),
            Path::new(".").to_path_buf(),
        )
    }
}
