use rstest::*;
use tokio::runtime::*;

#[fixture]
pub fn setup_log() {
    captains_log::recipe::raw_file_logger("/tmp", "test_crossfire", log::Level::Debug)
        .test()
        .build()
        .expect("log setup");
}

#[allow(dead_code)]
pub fn get_runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
}
