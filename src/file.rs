use pbr::{ProgressBar, Units};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::io::BufReader;
use std::fs::File as F;
use std::io::{Seek, SeekFrom};
use std::net::{TcpStream, Shutdown, IpAddr};

use sha2::sha2::Sha256;
use sha2::Digest;

use bincode::serde::*;
use bincode::SizeLimit;

use helpers::calculate_block_size;
use networking::BASE_PORT;
use request::sort_by_block_availability;

#[derive(Serialize, Deserialize, Debug)]
pub struct FileMetadata {
    /// SHA256 Hash of the files content and the blocks
    pub hash: (
        Vec<u8>,
        Vec<Vec<u8>>
    ),
    /// Total size of the file in bytes
    pub size: usize
}

pub struct File {
    pub metadata: FileMetadata,
    /// Block ID and people downloading it currently
    pub blocks: Vec<(usize, usize)>,
    pub local_path: PathBuf
}

impl File {
    pub fn prepare(path: PathBuf) -> File {
        let f = F::open(path.clone()).unwrap();
        let size = f.metadata().unwrap().len();
        let block_size = calculate_block_size(size as usize);
        let mut pb = ProgressBar::new(size); pb.set_units(Units::Bytes);
        let reader = BufReader::with_capacity(block_size, f);

        println!("File size: {}, Block size: {}", size, block_size);

        let mut block_hashes = Vec::new();

        let mut hash = Sha256::new();
        let mut block_hash = Sha256::new();
        let mut block = Vec::new();
        for (id, byte) in reader.bytes().enumerate() {
            match byte {
                Ok(byte) => {
                    if id % block_size == 0 && block.len() > 0 {
                        pb.add(block_size as u64);

                        // Create block hash
                        block_hash.input(&block);
                        let mut buf = vec![0; block_hash.output_bytes()];
                        block_hash.result(&mut buf);
                        block_hashes.push(buf.clone());
                        block_hash.reset();

                        // Add to main hash and clear block
                        hash.input(&block);
                        block.clear();
                    }
                    block.push(byte);
                },
                Err(e) => { exit!(1, "Error occurred whilst reading file ({:?})", e); }
            }
        }
        println!("TODO: Remaining bytes: {}", block.len());
        pb.add(block_size as u64);
        hash.input(&block);
        let mut hash_res = vec![0; hash.output_bytes()];
        hash.result(&mut hash_res);

        File {
            blocks: (0..block_hashes.len()).map(|i| (i, 0)).collect(),
            local_path: path.canonicalize().unwrap(),
            metadata: FileMetadata {
                hash: (
                    hash_res,
                    block_hashes
                ),
                size: size as usize
            }
        }
    }

    pub fn get_block(&self, block_id: usize) -> Vec<u8> {
        let f = F::open(self.local_path.as_path()).unwrap();
        let block_size = calculate_block_size(f.metadata().unwrap().len() as usize);
        let mut reader = BufReader::with_capacity(block_size, f);
        reader.seek(SeekFrom::Start((block_size * block_id) as u64)).unwrap();
        let mut buf = vec![0; block_size];
        reader.read_exact(&mut buf).unwrap();
        buf
    }

    pub fn download(&mut self, sources: Vec<Vec<IpAddr>>) {
        for block in sort_by_block_availability(sources.clone()).iter() {
            let ref current_sources = sources[*block];
            if current_sources.len() > 0 {
                println!("Currently loading block {} from sources {:?}", block, current_sources);
                for source in current_sources {
                    match TcpStream::connect((*source, BASE_PORT)) {
                        Ok(mut stream) => {
                            let payload = serialize(&(self.metadata.hash.0.clone(), block), SizeLimit::Infinite).unwrap();
                            stream.write_all(&payload).unwrap();
                            stream.shutdown(Shutdown::Write).unwrap();

                            let mut block = Vec::with_capacity(calculate_block_size(self.metadata.size));
                            stream.read_to_end(&mut block).unwrap();
                            if block.len() > 0 {
                                // TODO: Write block to file
                                break;
                            } else { warn!("Received invalid block data (zero_len)"); }
                        },
                        Err(_) => {}
                    }
                }
            }
        }
    }
}
