use std::cmp::Ordering;
use std::collections::HashMap;
use std::net::{TcpListener, IpAddr, SocketAddr, TcpStream, Shutdown};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{spawn, sleep};
use std::time::Duration;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::fs::File as F;
use std::io::{Seek, SeekFrom};

use bincode::serde::*;
use bincode::SizeLimit;

use sha2::sha2::Sha256;
use sha2::Digest;

use ext_time::{Duration as ext_Duration, PreciseTime};

use helpers::{to_hex_string, calculate_block_size};

use networking::{UDPSocket, ping, BASE_PORT};

use file::{FileMetadata, File, FileHandle};


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
            let a_ping = ping(SocketAddr::new(*a.1, BASE_PORT + 1));
            let b_ping = ping(SocketAddr::new(*b.1, BASE_PORT + 1));
            a_ping.cmp(&b_ping)
        } else { comparison }
    })};
    block_sources.into_iter().map(|block| {
        block.into_iter().map(|source| source.1.clone()).collect()
    }).collect()
}

pub fn sort_by_block_availability(sources: Vec<Vec<IpAddr>>) -> Vec<usize> {
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

impl File {
    pub fn from_metadata(uuid: &Vec<u8>, path: PathBuf) -> Option<File> {
        let mut uuid = uuid.clone();

        info!("Requesting metadata for {}", to_hex_string(&uuid));

        let sock = UDPSocket::new().create_handle();
        let sock_addr = sock.socket.local_addr().unwrap();
        let (tcp_tx, tcp_rx) = mpsc::channel();

        // TCP receive thread
        let hash_copy = uuid.clone();
        let tcp_ready = Arc::new(Mutex::new(false));
        let tcp_ready_thread = tcp_ready.clone();
        spawn(move || {
            let tcp_sock = TcpListener::bind(sock_addr).unwrap();
            *tcp_ready_thread.lock().unwrap() = true;
            let mut stream = match tcp_sock.accept() {
                Ok((sock, _)) => sock,
                Err(_) => {
                    tcp_tx.send(None).unwrap();
                    return
                }
            };
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).unwrap();
            let metadata: FileMetadata = deserialize(&buf).unwrap();
            if metadata.hash.0 != hash_copy { exit!(2, "Hash mismatch! (remote vs local)"); }
            tcp_tx.send(Some(metadata)).unwrap();
        });

        uuid.push(1); // Request file details in addition to block lists
        loop {
            if *tcp_ready.lock().unwrap() == true { break; }
            sleep(Duration::from_millis(10));
        }
        sock.send_to_multicast(&uuid); // Send request

        let start = PreciseTime::now();
        let mut metadata = None;
        let mut received_metadata = false;
        while start.to(PreciseTime::now()) < ext_Duration::seconds(1) {
            if !received_metadata {
                match tcp_rx.try_recv() {
                    Ok(m) => {
                        metadata = m;
                        received_metadata = true;
                    },
                    Err(_) => {}
                }
            } else {
                return Some(File {
                    metadata: metadata.unwrap(),
                    blocks: Vec::new(),
                    local_path: path
                })
            }
        }

        None
    }
}

impl FileHandle {

    fn update_sources(&mut self) {
        let file_size = self.file.lock().unwrap().metadata.size;
        let mut uuid = self.file.lock().unwrap().metadata.hash.0.clone();
        uuid.push(0); // Do not request file details but only the available blocks

        let (udp_tx, udp_rx) = mpsc::channel();
        let sock = UDPSocket::new().create_handle();
        sock.send_to_multicast(&uuid);
        spawn(move || {
            loop {
                // TODO: Set datagram size dynamically
                udp_tx.send(sock.receive()).unwrap();
            }
        });

        let start = PreciseTime::now();
        let mut block_sources: HashMap<IpAddr, Vec<usize>> = HashMap::new();
        while start.to(PreciseTime::now()) < ext_Duration::seconds(1) {
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

        self.sources = convert_block_sources(file_size, block_sources);
    }

    fn allocate(&mut self) {
        let file = self.file.lock().unwrap();
        let size = file.metadata.size;
        let path = file.local_path.clone();
        drop(file);

        let mut f = F::create(path).unwrap();
        f.seek(SeekFrom::Start(size as u64)).unwrap();
        f.write(&[0]).unwrap();
        f.sync_all().unwrap();
    }

    pub fn download(&mut self) {
        // self.allocate();
        self.update_sources();
        let mut metadata = self.file.lock().unwrap().metadata.clone();
        let block_size = calculate_block_size(metadata.size);
        let path = self.file.lock().unwrap().local_path.clone();
        let mut f = F::create(path).unwrap();
        // TODO: Update sources after every block download
        for block_id in sort_by_block_availability(self.sources.clone()).iter() {
            let ref current_sources = self.sources[*block_id];
            if current_sources.len() > 0 {
                for source in current_sources {
                    match TcpStream::connect((*source, BASE_PORT)) {
                        Ok(mut stream) => {
                            let payload = serialize(&(metadata.hash.0.clone(), block_id), SizeLimit::Infinite).unwrap();
                            stream.write_all(&payload).unwrap();
                            stream.shutdown(Shutdown::Write).unwrap();

                            let mut block = Vec::with_capacity(block_size);
                            stream.read_to_end(&mut block).unwrap();
                            if block.len() > 0 {
                                let mut block_hash = Sha256::new();
                                block_hash.input(&block);
                                let mut buf = vec![0; block_hash.output_bytes()];
                                block_hash.result(&mut buf);
                                if buf != metadata.hash.1[*block_id] { exit!(1, "HASH MISMATCH"); }
                                f.seek(SeekFrom::Start((block_id * block_size) as u64 )).unwrap();
                                f.write_all(&mut block).unwrap();
                                break;
                            } else { warn!("Received invalid block data (zero_len)"); }
                        },
                        Err(_) => {}
                    }
                }
            }
        }

        f.seek(SeekFrom::Start((metadata.hash.1.len() * block_size) as u64)).unwrap();
        f.write_all(&mut metadata.trailing_bytes).unwrap();
    }
}
