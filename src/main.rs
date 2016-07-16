#![allow(dead_code)]
#![feature(custom_derive, plugin)]
#![plugin(serde_macros)]

#[macro_use] extern crate log;
extern crate ansi_term;
extern crate bincode;
extern crate sha2;
extern crate time;
extern crate pbr;

use pbr::{ProgressBar, Units};

use time::{Duration, PreciseTime};

use sha2::sha2::Sha256;
use sha2::Digest;

use bincode::serde::*;
use bincode::SizeLimit;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, IpAddr, SocketAddr};
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
    let mut power = 4;
    let mut block_size = 2;

    while block_size < 1000000 && total_size / block_size > 1000 {
        power += 4;
        block_size = base * power;
    }

    block_size
}

#[derive(Serialize, Deserialize, Debug)]
struct FileMetadata {
    /// SHA256 Hash of the files content
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
    for block in block_sources.iter_mut() { block.sort_by(|a, b| {
        let comparison = a.0.cmp(&b.0);
        if comparison == Ordering::Equal {
            // In case a == b we compare their ping and use the better one
            let a_ping = ping(SocketAddr::new(*a.1, 9999));
            let b_ping = ping(SocketAddr::new(*b.1, 9999));
            a_ping.cmp(&b_ping)
        } else { comparison }
    })};
    block_sources.into_iter().map(|block| {
        block.into_iter().map(|source| source.1.clone()).collect()
    }).collect()
}

use std::cmp::Ordering;
fn sort_by_block_availability(sources: Vec<Vec<IpAddr>>) -> Vec<usize> {
    let mut block_availability: Vec<_> = sources.into_iter().enumerate().map(|(id, block)| (id, block.len())).collect();
    // Put the available ones at the top and sort them by their availability (lowest first to speed up distribution)
    block_availability.sort_by(|a, b|
        // If none of them is available they are equal
        if a.1 == 0 && b.1 == 0 { Ordering::Equal }
        // If a is not available then b is better
        else if a.1 == 0 { Ordering::Greater }
        // If b is not available then a is better
        else if b.1 == 0 { Ordering::Less }
        // If both are available then their availability will be compared
        else { a.1.cmp(&b.1) }
    );
    // Strip the availability value
    let block_availability = block_availability.into_iter().map(|(id, _)| id).collect();
    block_availability
}

fn request(uuid: &Vec<u8>) {
    let mut uuid = uuid.clone();

    info!("Requesting {}", to_hex_string(&uuid));

    let sock = UDPSocket::new().create_handle();
    let sock_addr = sock.socket.local_addr().unwrap();
    let (tcp_tx, tcp_rx) = std::sync::mpsc::channel();
    let (udp_tx, udp_rx) = std::sync::mpsc::channel();

    // TCP receive thread
    let hash_copy = uuid.clone();
    let tcp_ready = std::sync::Arc::new(std::sync::Mutex::new(false));
    let tcp_ready_thread = tcp_ready.clone();
    spawn(move || {
        let tcp_sock = TcpListener::bind(sock_addr).unwrap();
        *tcp_ready_thread.lock().unwrap() = true;
        let mut stream = match tcp_sock.incoming().next() {
            Some(sock) => sock.unwrap(),
            None => {
                tcp_tx.send(None).unwrap();
                return
            }
        };
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).unwrap();
        let metadata: FileMetadata = deserialize(&buf).unwrap();
        if metadata.hash != hash_copy { exit!(2, "Hash mismatch! (remote vs local)"); }
        tcp_tx.send(Some(metadata)).unwrap();
    });

    // UDP receive thread
    let thread_sock = sock.try_clone().unwrap();
    spawn(move || {
        loop {
            udp_tx.send(thread_sock.receive()).unwrap();
        }
    });

    uuid.push(1); // Request file details in addition to block lists
    loop {
        if *tcp_ready.lock().unwrap() == true { break; }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
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
            //TODO: Compare hash in metadata w/ local UUID
            let blocks = convert_block_sources(metadata.size, block_sources);
            for block in sort_by_block_availability(blocks.clone()).iter() {
                let ref current_sources = blocks[*block];
                if current_sources.len() > 0 {
                    println!("Currently loading block {} from sources {:?}", block, current_sources);
                }
            }
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
            hash: generate_uuid(&"some random file contents\n".to_string()),
            size: 100000
        },
        blocks: vec![(0, 2), (1, 1), (2, 0)]
    });

    spawn(move || {
        let sock = UDPSocket::new().create_listener();
        debug!("Announce thread started.");
        loop {
            let (mut data, src) = sock.receive();
            let file_details_requested = data.pop();

            debug!("Received request for file {:?}", to_hex_string(&data));

            let matching_files = files.iter_mut().filter(|f| f.metadata.hash == data);
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

fn ping_server() -> JoinHandle<()> {
    spawn(|| {
        let tcp_sock = TcpListener::bind("0.0.0.0:9999").unwrap();
        for stream in tcp_sock.incoming() {
            let mut stream = stream.unwrap();
            let mut buf = Vec::new();
            stream.read(&mut [0]).unwrap();
            stream.write_all(&mut buf).unwrap();
        }
    })
}

fn ping(mut target: SocketAddr) -> Option<Duration> {
    target.set_port(9999);
    match TcpStream::connect(target) {
        Ok(mut stream) => {
            stream.set_read_timeout(Some(std::time::Duration::from_millis(5000))).unwrap();
            let start = PreciseTime::now();
            match stream.write(&[1]) {
                Ok(_) => {
                    match stream.read(&mut [0]) {
                        Ok(_) => Some(start.to(PreciseTime::now())),
                        Err(_) => None
                    }
                },
                Err(_) => None
            }
        },
        Err(_) => None
    }
}

fn read_file() {
    const STEP: usize = 1000000;

    let f = std::fs::File::open("./test").unwrap();
    let size = f.metadata().unwrap().len();
    let block_size = calculate_block_size(size as usize);
    let mut pb = ProgressBar::new(size); pb.set_units(Units::Bytes);
    let reader = std::io::BufReader::with_capacity(block_size, f);

    println!("File size: {}, Block size: {}", size, block_size);

    let mut hash = Sha256::new();
    let mut buf = Vec::new();
    for (id, byte) in reader.bytes().enumerate() {
        match byte {
            Ok(byte) => {
                if id % STEP == 0 {
                    pb.add(STEP as u64);
                    hash.input(&buf);
                    buf.clear();
                }
                buf.push(byte);
            },
            Err(e) => { exit!(1, "Error occurred whilst reading file ({:?})", e); }
        }
    }
    pb.add(STEP as u64);
    hash.input(&buf);
    let mut buf = vec![0; hash.output_bytes()];
    hash.result(&mut buf);

    println!("{:?}", to_hex_string(&buf));
}

fn main() {
    Logger::init();
    info!("DDP node v{}-{}", VERSION, GIT_HASH);

    read_file();
    exit!(3);

    ping_server();
    let handle = announce();

    // Request some random file
    std::thread::sleep(std::time::Duration::from_millis(200));
    request(&generate_uuid(&"some random file contents\n".to_string()));

    handle.join().unwrap();
}
