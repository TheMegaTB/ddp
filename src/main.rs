#![allow(dead_code)]

#[macro_use] extern crate log;
extern crate ansi_term;

extern crate sha2;
use sha2::sha2::Sha256;
use sha2::Digest;

mod logger;
use logger::Logger;

mod git_hash;
use git_hash::GIT_HASH;

#[macro_use]
pub mod helpers;
pub use helpers::*;

mod networking;
use networking::*;

/// Constant containing version string provided by cargo
pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");

pub const BLOCK_SIZE: usize = 2048;

fn to_hex_string(bytes: &Vec<u8>) -> String {
    bytes.chunks(8).map(|c| {
        c.iter().map(|b| format!("{:02X}", b)).collect::<Vec<String>>().join("")
    }).collect::<Vec<String>>().join("-")
}

fn generate_uuid(input: &String) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.input_str(&input);
    let mut buf = vec![0; hash.output_bytes()];
    hash.result(&mut buf);
    buf
}

fn calculate_block_size(total_size: usize) -> usize {
    let base: usize = 2;
    let mut power = 2;
    let mut block_size = 2;

    while block_size < 1000000 && total_size / block_size > 1000 {
        power += 1;
        block_size = base.pow(power);
    }

    block_size
}

use std::collections::HashMap;
struct File {
    /// SHA256 Hash of the filename
    id: Vec<u8>,
    /// SHA512 Hash of the files content
    hash: Vec<u8>,
    /// Block ID, hash and people downloading it currently
    blocks: HashMap<usize, (String, usize)>,
    /// Total size of the file in bytes
    size: usize
}

fn request(filename: String) {
    info!("Requesting {} {}", filename, to_hex_string(&generate_uuid(&filename)));
    let sock = UDPSocket::new().create_handle();
    sock.send_to_multicast(&generate_uuid(&filename));
}

use std::thread::{spawn,JoinHandle};
fn announce() -> JoinHandle<()> {
    let mut files: Vec<File> = Vec::new();
    files.push(File {
        id: generate_uuid(&"firefox.pkg".to_string()),
        hash: vec![0; 64],
        blocks: HashMap::new(),
        size: 55899986
    });

    spawn(move || {
        let sock = UDPSocket::new().create_listener();
        debug!("Announce thread started.");
        loop {
            let (data, src) = sock.receive();

            debug!("Received request for file {:?}", to_hex_string(&data));

            let matching_files = files.iter().filter(|f| f.id == data);
            if matching_files.size_hint().1 > Some(1) { exit!(1, "Got more than one matching file stored with the same UUID!"); }

            for file in matching_files {
                trace!("Got matching file! Sending . . .");
            }
        }
    })
}

fn main() {
    Logger::init();
    info!("DDP node v{}-{}", VERSION, GIT_HASH);

    info!("Block size: {}", calculate_block_size(350000000));

    let handle = announce();
    std::thread::sleep(std::time::Duration::from_millis(200));
    request("firefox.pkg".to_string());
    handle.join().unwrap();
}
