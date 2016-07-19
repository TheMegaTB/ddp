use pbr::{ProgressBar, Units};
use std::io::Read;
use std::path::PathBuf;
use std::io::BufReader;
use std::fs::File as F;
use std::io::{Seek, SeekFrom};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use sha2::sha2::Sha256;
use sha2::Digest;

use helpers::calculate_block_size;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileMetadata {
    /// SHA256 Hash of the files content and the blocks
    pub hash: (
        Vec<u8>,
        Vec<Vec<u8>>
    ),
    /// Total size of the file in bytes
    pub size: usize,
    /// Trailing bytes
    pub trailing_bytes: Vec<u8>
}

pub struct File {
    pub metadata: FileMetadata,
    /// Block ID and people downloading it currently
    pub blocks: Vec<(usize, usize)>,
    pub local_path: PathBuf
}

pub struct FileHandle {
    pub file: Arc<Mutex<File>>,
    pub sources: Vec<Vec<IpAddr>>
}

impl File {
    pub fn to_handle(self) -> FileHandle {
        FileHandle {
            file: Arc::new(Mutex::new(self)),
            sources: Vec::new()
        }
    }

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
                trailing_bytes: block,
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
}
