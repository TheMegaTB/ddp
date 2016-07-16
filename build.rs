use std::process::Command;
use std::io::prelude::*;
use std::fs::*;

fn main() {
    // Generate the git hash
    let mut hash = if cfg!( any(unix) ) {
        Command::new("/usr/bin/git").arg("rev-parse").arg("--short").arg("HEAD").output().unwrap_or_else(|e| {
            panic!("failed to execute process: {}", e)
        }).stdout
    } else { panic!("You shall not pas...ehm..compile on a non-unix OS!") };
    hash.pop();

    // Write the constant to a file that is compiled into the project
    let mut f = File::create("src/git_hash.rs").unwrap();
    f.write_all("//! A dynamically generated file containing the current hash of the repository\n".to_string().as_bytes()).unwrap();
    f.write_all("/// The current hash of the project\n".to_string().as_bytes()).unwrap();
    f.write_all("pub const GIT_HASH: &'static str = \"".to_string().as_bytes()).unwrap();
    f.write_all(hash.as_slice()).unwrap();
    f.write_all("\";".to_string().as_bytes()).unwrap();
}
