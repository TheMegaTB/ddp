use std::thread::{spawn,JoinHandle};
use std::sync::{Arc, Mutex};
use std::net::TcpStream;
use std::io::Write;

use bincode::serde::*;
use bincode::SizeLimit;

use file::File;
use networking::UDPSocket;
use helpers::to_hex_string;

pub fn announce(files: Arc<Mutex<Vec<File>>>) -> JoinHandle<()> {
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
