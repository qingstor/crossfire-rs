use std::fmt;
use tokio::runtime::Runtime;

#[allow(dead_code)]
pub const ONE_MILLION: usize = 1000000;
#[allow(dead_code)]
pub const TEN_THOUSAND: usize = 10000;

#[allow(dead_code)]
pub struct Concurrency {
    pub tx_count: usize,
    pub rx_count: usize,
}

impl fmt::Display for Concurrency {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}x{}", self.tx_count, self.rx_count)
    }
}

#[allow(dead_code)]
pub fn get_runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
}
