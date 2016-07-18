use std::thread::spawn;
use std::sync::{Arc, Mutex};
use std::net::{TcpStream, TcpListener};
use std::io::{Read, Write};

use bincode::serde::*;
use bincode::SizeLimit;

use file::File;
use networking::{UDPSocket, BASE_PORT};
use helpers::to_hex_string;

pub fn announce(files: Arc<Mutex<Vec<File>>>) {
    {
        let files = files.clone();
        spawn(move || {
            let sock = UDPSocket::new().create_listener();
            debug!("Announce thread started.");
            loop {
                let (mut data, src) = sock.receive();
                let file_details_requested = data.pop();

                let mut files = files.lock().unwrap();

                debug!("Received request for file {:?}", to_hex_string(&data));

                let matching_files = files.iter_mut().filter(|f| f.metadata.hash.0 == data);
                if matching_files.size_hint().1 > Some(1) { exit!(1, "Got more than one matching file stored with the same UUID!"); }

                for file in matching_files {
                    if file_details_requested == Some(1) {
                        // Attempt to send metadata and fail silently (fail = somebody else sent it earlier)
                        match TcpStream::connect(src) {
                            Ok(mut stream) => {
                                let metadata = serialize(&file.metadata, SizeLimit::Infinite).unwrap();
                                stream.write(&metadata).unwrap();
                            },
                            Err(_) => {}
                        }
                    } else if file_details_requested == Some(0) {
                        // Send available blocks
                        // Sort by connected clients
                        file.blocks.sort_by(|a, b| a.1.cmp(&b.1));
                        // Remove the client list and serialize
                        let block_list = file.blocks.iter().map(|i| i.0).collect::<Vec<_>>();
                        // Do not send the list if its empty
                        if block_list.len() > 0 {
                            // Send the block list
                            let block_list = serialize(&block_list, SizeLimit::Infinite).unwrap();
                            UDPSocket::new().create_handle().send(&block_list, src);
                        }
                    }
                }
            }
        });
    }

    spawn(move || {
        let socket = TcpListener::bind(("0.0.0.0", BASE_PORT)).unwrap();
        for stream in socket.incoming() {
            let files = files.clone();
            spawn(move || {
                let mut stream = stream.unwrap();
                let mut buffer = Vec::new();
                stream.read_to_end(&mut buffer).unwrap();

                let (hash, block): (Vec<u8>, usize) = deserialize(&buffer).unwrap();
                let files = files.lock().unwrap();
                match files.iter().find(|file| file.metadata.hash.0 == hash) {
                    Some(file) => {
                        stream.write_all(&file.get_block(block)).unwrap();
                    },
                    None => { warn!("Block request for non-existent file"); }
                }
            });
        }
    });
}
