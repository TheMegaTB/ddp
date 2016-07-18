use sha2::sha2::Sha256;
use sha2::Digest;

pub fn to_hex_string(bytes: &Vec<u8>) -> String {
    bytes.chunks(8).map(|c| {
        c.iter().map(|b| format!("{:02X}", b)).collect::<Vec<String>>().join("")
    }).collect::<Vec<String>>().join("-")
}

pub fn generate_uuid(input: &String) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.input_str(&input);
    let mut buf = vec![0; hash.output_bytes()];
    hash.result(&mut buf);
    buf
}

pub fn calculate_block_size(total_size: usize) -> usize {
    let mut block_size = 2;

    while block_size < 1000000 && total_size / block_size > 1000 {
        block_size += 1;
    }

    block_size - 1
}

/// Panic with a given error code and print an optional message
/// # Examples
///
/// ```should_panic
/// # #[macro_use] extern crate structures;
/// # #[macro_use] extern crate log;
/// # fn main() {
/// // An error code is required
/// exit!(1);
/// # }
/// ```
///
/// ```should_panic
/// # #[macro_use] extern crate structures;
/// # #[macro_use] extern crate log;
/// # fn main() {
/// // Additionally you can provide an error message
/// exit!(1, "Some random generic error.");
/// # }
/// ```
///
/// ```should_panic
/// # #[macro_use] extern crate structures;
/// # #[macro_use] extern crate log;
/// # fn main() {
/// // It's even possible to use format arguments
/// exit!(1, "Some random generic error. And some nice arguments are possible as well: {}", 5);
/// # }
/// ```
#[macro_export]
macro_rules! exit {
    () => {exit!(1)};
    ($code:expr) => {
        // TODO Save all that important work
        ::std::process::exit($code);
    };
    ($code:expr, $res:expr) => {
        error!("{}", $res);
        exit!($code);
    };
    ($code:expr, $res:expr, $($arg:tt)*) => {
        exit!($code, format!($res, $($arg)*));
    };
}
