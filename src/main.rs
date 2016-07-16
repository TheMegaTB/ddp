#![allow(dead_code)]
#![feature(custom_derive, plugin)]
#![plugin(serde_macros)]

#[macro_use] extern crate log;
extern crate ansi_term;
extern crate bincode;
extern crate sha2;
extern crate time;

use time::{Duration, PreciseTime};

use sha2::sha2::Sha256;
use sha2::Digest;
use bincode::serde::*;
use bincode::SizeLimit;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, IpAddr};
use std::collections::HashMap;

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

#[derive(Serialize, Deserialize, Debug)]
struct FileMetadata {
    /// SHA256 Hash of the filename
    id: Vec<u8>,
    /// SHA512 Hash of the files content
    hash: Vec<u8>,
    /// Total size of the file in bytes
    size: usize
}

struct File {
    metadata: FileMetadata,
    /// Block ID, hash and people downloading it currently
    blocks: Vec<(usize, Vec<u8>, usize)>
}

fn request(filename: String) {
    info!("Requesting {} {}", filename, to_hex_string(&generate_uuid(&filename)));

    let sock = UDPSocket::new().create_handle();
    let sock_addr = sock.socket.local_addr().unwrap();
    let (tcp_tx, tcp_rx) = std::sync::mpsc::channel();
    let (udp_tx, udp_rx) = std::sync::mpsc::channel();

    // TCP receive thread
    spawn(move || {
        let tcp_sock = TcpListener::bind(sock_addr).unwrap();
        let mut stream = match tcp_sock.incoming().next() {
            Some(sock) => sock.unwrap(),
            None => {
                tcp_tx.send(None).unwrap();
                return;
            }
        };
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).unwrap();
        let metadata: FileMetadata = deserialize(&buf).unwrap();
        tcp_tx.send(Some(metadata)).unwrap();
    });

    // UDP receive thread
    let thread_sock = sock.try_clone().unwrap();
    spawn(move || {
        loop {
            udp_tx.send(thread_sock.receive()).unwrap();
        }
    });

    let mut uuid = generate_uuid(&filename);
    uuid.push(1); // Request file details in addition to block lists
    std::thread::sleep(std::time::Duration::from_millis(500)); //TODO: Replace this with waiting for the TCP socket
    sock.send_to_multicast(&uuid); // Send request

    let start = PreciseTime::now();
    let mut block_availability: HashMap<IpAddr, Vec<u8>> = HashMap::new();
    let mut metadata = None;
    let mut received_metadata = false;
    while start.to(PreciseTime::now()) < Duration::seconds(5) {
        if !received_metadata {
            match tcp_rx.try_recv() {
                Ok(m) => {
                    metadata = m;
                    received_metadata = true;
                },
                Err(_) => {}
            }
        }
        match udp_rx.try_recv() {
            Ok(mut d) => {
                if match block_availability.get_mut(&d.1.ip()) {
                    Some(v) => { v.append(&mut d.0); false},
                    None => true
                } {
                    block_availability.insert(d.1.ip(), d.0);
                }
            },
            Err(_) => {}
        }
    }
    println!("{:?}", metadata);
    println!("{:?}", block_availability);
}

use std::thread::{spawn,JoinHandle};
fn announce() -> JoinHandle<()> {
    let mut files: Vec<File> = Vec::new();

    // Example file
    files.push(File {
        metadata: FileMetadata {
            id: generate_uuid(&"firefox.pkg".to_string()),
            hash: vec![0; 64],
            size: 55899986
        },
        blocks: Vec::new()
    });

    spawn(move || {
        let sock = UDPSocket::new().create_listener();
        debug!("Announce thread started.");
        loop {
            let (mut data, src) = sock.receive();
            let file_details_requested = data.pop();

            debug!("Received request for file {:?}", to_hex_string(&data));

            let matching_files = files.iter().filter(|f| f.metadata.id == data);
            if matching_files.size_hint().1 > Some(1) { exit!(1, "Got more than one matching file stored with the same UUID!"); }

            for file in matching_files {
                UDPSocket::new().create_handle().send(&[1, 2, 3], src);
                UDPSocket::new().create_handle().send(&[4, 5, 6], src);
                if file_details_requested == Some(1) {
                    // Attempt to send metadata and fail silently (fail = somebody else sent it earlier)
                    match TcpStream::connect(src) {
                        Ok(mut stream) => {
                            let metadata = serialize(&file.metadata, SizeLimit::Infinite).unwrap();
                            stream.write(&metadata).unwrap();
                        },
                        Err(_) => {}
                    }
                }
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
