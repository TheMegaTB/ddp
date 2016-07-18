#![allow(dead_code)]
#![feature(custom_derive, plugin)]
#![plugin(serde_macros)]

#[macro_use] extern crate log;
extern crate ansi_term;
extern crate bincode;
extern crate sha2;
extern crate time as ext_time;
extern crate pbr;

use std::sync::{Arc, Mutex};
use std::path::PathBuf;

#[macro_use]
mod helpers;

mod git_hash;
use git_hash::GIT_HASH;

mod logger;
use logger::Logger;

mod networking;
use networking::start_ping_server;

mod file;
use file::File;

mod announce;
use announce::announce;

mod request;
use request::request;

/// Constant containing version string provided by cargo
pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");

fn main() {
    Logger::init();
    info!("DDP node v{}-{}", VERSION, GIT_HASH);

    start_ping_server();

    let files = Arc::new(Mutex::new(Vec::new()));

    {
        let mut files = files.lock().unwrap();
        files.push(
            File::prepare(PathBuf::from("./test"))
        );
    }

    let handle = announce(files.clone());

    // Request some random file
    {
        let uuid = files.lock().unwrap()[0].metadata.hash.0.clone();
        std::thread::sleep(std::time::Duration::from_millis(200));
        request(&uuid);
    }

    handle.join().unwrap();
}
