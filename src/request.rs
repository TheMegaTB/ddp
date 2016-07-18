use std::cmp::Ordering;
use std::collections::HashMap;
use std::net::{TcpListener, IpAddr, SocketAddr};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{spawn, sleep};
use std::time::Duration;
use std::io::Read;

use ext_time::{Duration as ext_Duration, PreciseTime};

use bincode::serde::*;

use helpers::calculate_block_size;
use helpers::to_hex_string;

use networking::UDPSocket;
use networking::ping;

use file::{File, FileMetadata};


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

pub fn request_sources(uuid: &Vec<u8>, size: usize) -> Vec<Vec<IpAddr>> {
    let mut uuid = uuid.clone();
    uuid.push(0);

    let (udp_tx, udp_rx) = mpsc::channel();
    let sock = UDPSocket::new().create_handle();
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
                println!("Received something");
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

    convert_block_sources(size, block_sources)
}

pub fn request_metadata(uuid: &Vec<u8>) -> Option<FileMetadata> {
    let mut uuid = uuid.clone();

    info!("Requesting {}", to_hex_string(&uuid));

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
        if metadata.hash.0 != hash_copy { exit!(2, "Hash mismatch! (remote vs local)"); }
        tcp_tx.send(Some(metadata)).unwrap();
    });

    // UDP receive thread
    // let thread_sock = sock.try_clone().unwrap();
    // spawn(move || {
    //     loop {
    //         // TODO: Set datagram size dynamically
    //         udp_tx.send(thread_sock.receive()).unwrap();
    //     }
    // });

    uuid.push(1); // Request file details in addition to block lists
    loop {
        if *tcp_ready.lock().unwrap() == true { break; }
        sleep(Duration::from_millis(10));
    }
    sock.send_to_multicast(&uuid); // Send request

    let start = PreciseTime::now();
    let mut block_sources: HashMap<IpAddr, Vec<usize>> = HashMap::new();
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
        }
        // match udp_rx.try_recv() {
        //     Ok(d) => {
        //         let mut data: Vec<usize> = deserialize(&d.0).unwrap();
        //         let ip = d.1.ip();
        //         if match block_sources.get_mut(&ip) {
        //             Some(v) => { v.append(&mut data); false},
        //             None => true
        //         } {
        //             block_sources.insert(ip, data);
        //         }
        //     },
        //     Err(_) => {}
        // }
    }

    metadata

    // match metadata {
    //     Some(metadata) => {
    //         metadata
    //         //TODO: Compare hash in metadata w/ local UUID
    //         // let blocks = convert_block_sources(metadata.size, block_sources);
    //         // for block in sort_by_block_availability(blocks.clone()).iter() {
    //         //     let ref current_sources = blocks[*block];
    //         //     if current_sources.len() > 0 {
    //         //         println!("Currently loading block {} from sources {:?}", block, current_sources);
    //         //     }
    //         // }
    //     },
    //     None => {
    //         exit!(1, "File is not available (no_peers)");
    //     }
    // }

}
