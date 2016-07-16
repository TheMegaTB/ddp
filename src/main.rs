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
    // TODO: Hash every block and validate them when downloading
}

struct File {
    metadata: FileMetadata,
    /// Block ID and people downloading it currently
    blocks: Vec<(usize, usize)>
}

fn convert_block_sources(filesize: usize, sources: HashMap<IpAddr, Vec<usize>>) -> Vec<Vec<IpAddr>> {
    let block_count = filesize / calculate_block_size(filesize);
    // Restructure block_sources to be a vector of blocks
    // Each block is a vector of sources where a source is a touple of the 'rank' transmitted by that source and the IP of it
    let mut block_sources: Vec<Vec<_>> = (0..block_count).map(|_| Vec::new()).collect();
    for (source, blocks) in sources.iter() {
        for (rank, block) in blocks.iter().enumerate() {
            block_sources[*block].push((rank, source));
        }
    }
    // Sort the block sources by rank and strip it afterwards
    for block in block_sources.iter_mut() { block.sort_by(|a, b| a.0.cmp(&b.0)) };
    block_sources.into_iter().map(|block| {
        block.into_iter().map(|source| source.1.clone()).collect()
    }).collect()
}

fn sort_by_block_availability(filesize: usize, sources: HashMap<IpAddr, Vec<usize>>) -> Vec<usize> {
    let block_availability = convert_block_sources(filesize, sources).into_iter().map(|block| block.len()).collect();
    println!("{:?}", block_availability);
    block_availability
}

fn request(filename: String) {
    info!("Requesting {}", to_hex_string(&generate_uuid(&filename)));

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
    let mut block_sources: HashMap<IpAddr, Vec<usize>> = HashMap::new();
    let mut metadata = None;
    let mut received_metadata = false;
    while start.to(PreciseTime::now()) < Duration::seconds(1) {
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
            Ok(d) => {
                let mut data: Vec<usize> = deserialize(&d.0).unwrap();
                let ip = d.1.ip();
                if match block_sources.get_mut(&ip) {
                    Some(v) => { v.append(&mut data); false},
                    None => true
                } {
                    block_sources.insert(ip, data);
                }
            },
            Err(_) => {}
        }
    }

    match metadata {
        Some(metadata) => {
            sort_by_block_availability(metadata.size, block_sources);
        },
        None => {}
    }

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
        blocks: vec![(0, 2), (1, 1), (2, 0), (3, 0)]
    });

    spawn(move || {
        let sock = UDPSocket::new().create_listener();
        debug!("Announce thread started.");
        loop {
            let (mut data, src) = sock.receive();
            let file_details_requested = data.pop();

            debug!("Received request for file {:?}", to_hex_string(&data));

            let matching_files = files.iter_mut().filter(|f| f.metadata.id == data);
            if matching_files.size_hint().1 > Some(1) { exit!(1, "Got more than one matching file stored with the same UUID!"); }

            for file in matching_files {
                // Sort by connected clients
                file.blocks.sort_by(|a, b| a.1.cmp(&b.1));
                // Remove the client list and serialize
                let block_list = file.blocks.iter().map(|i| i.0).collect::<Vec<_>>();
                let block_list = serialize(&block_list, SizeLimit::Infinite).unwrap();

                // Send the block list
                UDPSocket::new().create_handle().send(&block_list, src);

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
