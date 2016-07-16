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
