use criterion::*;
use crossfire::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use std::{fmt, thread};
use tokio::runtime::Runtime;

const ONE_MILLION: usize = 1000000;
const TEN_THOUSAND: usize = 10000;

struct Concurrency {
    tx_count: usize,
    rx_count: usize,
}

impl fmt::Display for Concurrency {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}x{}", self.tx_count, self.rx_count)
    }
}

fn _crossbeam_bounded_sync(bound: usize, tx_count: usize, rx_count: usize, msg_count: usize) {
    let (tx, rx) = crossbeam::channel::bounded::<usize>(bound);
    let send_counter = Arc::new(AtomicUsize::new(0));
    let recv_counter = Arc::new(AtomicUsize::new(0));
    let mut ths = Vec::new();
    for _ in 0..tx_count {
        let _send_counter = send_counter.clone();
        let _tx = tx.clone();
        ths.push(thread::spawn(move || {
            loop {
                let i = _send_counter.fetch_add(1, Ordering::SeqCst);
                if i < msg_count {
                    _tx.send(i).expect("send");
                } else {
                    break;
                }
            }
        }));
    }
    drop(tx);
    for _ in 0..(rx_count - 1) {
        let _rx = rx.clone();
        let _recv_counter = recv_counter.clone();
        ths.push(thread::spawn(move || {
            loop {
                match _rx.recv() {
                    Ok(_) => {
                        let _ = _recv_counter.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }));
    }
    loop {
        match rx.recv() {
            Ok(_) => {
                let _ = recv_counter.fetch_add(1, Ordering::SeqCst);
            }
            Err(_) => {
                break;
            }
        }
    }
    for th in ths {
        let _ = th.join();
    }
    assert!(send_counter.load(Ordering::Acquire) >= msg_count);
    assert!(recv_counter.load(Ordering::Acquire) >= msg_count);
}

async fn _tokio_bounded_mpsc(bound: usize, tx_count: usize, msg_count: usize) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<usize>(bound);

    let counter = Arc::new(AtomicUsize::new(0));
    for _tx_i in 0..tx_count {
        let _tx = tx.clone();
        let _counter = counter.clone();
        tokio::spawn(async move {
            loop {
                let i = _counter.fetch_add(1, Ordering::SeqCst);
                if i < msg_count {
                    let _ = _tx.send(i).await;
                } else {
                    break;
                }
            }
        });
    }
    drop(tx);
    for _ in 0..msg_count {
        if let Some(_msg) = rx.recv().await {
            //    println!("recv {}", _msg);
        } else {
            panic!("recv error");
        }
    }
}

fn _flume_bounded_sync(bound: usize, tx_count: usize, rx_count: usize, msg_count: usize) {
    let (tx, rx) = flume::bounded(bound);
    let send_counter = Arc::new(AtomicUsize::new(0));
    let mut ths = Vec::new();
    for _tx_i in 0..tx_count {
        let _send_counter = send_counter.clone();
        let _tx = tx.clone();
        ths.push(thread::spawn(move || {
            loop {
                let i = _send_counter.fetch_add(1, Ordering::SeqCst);
                if i < msg_count {
                    if let Err(e) = _tx.send(i) {
                        panic!("send error: {:?}", e);
                    }
                } else {
                    break;
                }
            }
        }));
    }
    drop(tx);
    let recv_counter = Arc::new(AtomicUsize::new(0));
    for _ in 0..(rx_count - 1) {
        let _rx = rx.clone();
        let _rx_counter = recv_counter.clone();
        ths.push(thread::spawn(move || {
            loop {
                match _rx.recv() {
                    Ok(_) => {
                        let _ = _rx_counter.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }));
    }
    loop {
        match rx.recv() {
            Ok(_) => {
                let _ = recv_counter.fetch_add(1, Ordering::SeqCst);
            }
            Err(_) => {
                break;
            }
        }
    }
    for th in ths {
        let _ = th.join();
    }
    assert!(send_counter.load(Ordering::Acquire) >= msg_count);
    assert!(recv_counter.load(Ordering::Acquire) >= msg_count);
}

async fn _flume_unbounded_async(bound: usize, tx_count: usize, rx_count: usize, msg_count: usize) {
    let (tx, rx) = flume::bounded(bound);
    let counter = Arc::new(AtomicUsize::new(0));
    let mut ths = Vec::new();
    for _tx_i in 0..tx_count {
        let _counter = counter.clone();
        let _tx = tx.clone();
        ths.push(tokio::spawn(async move {
            loop {
                let i = _counter.fetch_add(1, Ordering::SeqCst);
                if i < msg_count {
                    if let Err(e) = _tx.send(i) {
                        panic!("send error: {:?}", e);
                    }
                } else {
                    break;
                }
            }
        }));
    }
    drop(tx);
    let recv_counter = Arc::new(AtomicUsize::new(0));
    for _ in 0..(rx_count - 1) {
        let _rx = rx.clone();
        let _recv_counter = recv_counter.clone();
        ths.push(tokio::spawn(async move {
            loop {
                match _rx.recv_async().await {
                    Ok(_) => {
                        let _ = _recv_counter.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }));
    }
    loop {
        match rx.recv_async().await {
            Ok(_) => {
                let _ = recv_counter.fetch_add(1, Ordering::SeqCst);
            }
            Err(_) => {
                break;
            }
        }
    }
    for th in ths {
        let _ = th.await;
    }
    assert!(counter.load(Ordering::Acquire) >= msg_count);
    assert!(recv_counter.load(Ordering::Acquire) >= msg_count);
}

async fn _flume_bounded_async(bound: usize, tx_count: usize, rx_count: usize, msg_count: usize) {
    let (tx, rx) = flume::bounded(bound);
    let counter = Arc::new(AtomicUsize::new(0));
    let mut ths = Vec::new();
    for _tx_i in 0..tx_count {
        let _counter = counter.clone();
        let _tx = tx.clone();
        ths.push(tokio::spawn(async move {
            loop {
                let i = _counter.fetch_add(1, Ordering::SeqCst);
                if i < msg_count {
                    if let Err(e) = _tx.send_async(i).await {
                        panic!("send error: {:?}", e);
                    }
                } else {
                    break;
                }
            }
        }));
    }
    drop(tx);
    let recv_counter = Arc::new(AtomicUsize::new(0));
    for _ in 0..(rx_count - 1) {
        let _rx = rx.clone();
        let _recv_counter = recv_counter.clone();
        ths.push(tokio::spawn(async move {
            loop {
                match _rx.recv_async().await {
                    Ok(_) => {
                        let _ = _recv_counter.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }));
    }
    loop {
        match rx.recv_async().await {
            Ok(_) => {
                let _ = recv_counter.fetch_add(1, Ordering::SeqCst);
            }
            Err(_) => {
                break;
            }
        }
    }
    for th in ths {
        let _ = th.await;
    }
    assert!(counter.load(Ordering::Acquire) >= msg_count);
    assert!(recv_counter.load(Ordering::Acquire) >= msg_count);
}

fn _crossfire_btx_clone(tx: MTx<usize>, count: usize) -> Vec<MTx<usize>> {
    let mut v = Vec::with_capacity(count);
    for _ in 0..count {
        v.push(tx.clone());
    }
    v
}

fn _crossfire_atx_clone(tx: MAsyncTx<usize>, count: usize) -> Vec<MAsyncTx<usize>> {
    let mut v = Vec::with_capacity(count);
    for _ in 0..count {
        v.push(tx.clone());
    }
    v
}

fn _crossfire_arx_clone(rx: MAsyncRx<usize>, count: usize) -> Vec<MAsyncRx<usize>> {
    let mut v = Vec::with_capacity(count);
    for _ in 0..count {
        v.push(rx.clone());
    }
    v
}

async fn _crossfire_blocking_async<T: BlockingTxTrait<usize>, R: AsyncRxTrait<usize>>(
    txs: Vec<T>, mut rxs: Vec<R>, msg_count: usize,
) {
    let counter = Arc::new(AtomicUsize::new(0));
    let mut sender_ths = Vec::new();
    for tx in txs {
        let _counter = counter.clone();
        sender_ths.push(thread::spawn(move || {
            loop {
                let i = _counter.fetch_add(1, Ordering::SeqCst);
                if i < msg_count {
                    if let Err(e) = tx.send(i) {
                        panic!("send error: {:?}", e);
                    }
                } else {
                    break;
                }
            }
        }));
    }
    let recv_counter = Arc::new(AtomicUsize::new(0));
    let rx_count = rxs.len();
    let mut recv_ths = Vec::new();
    for _ in 0..(rx_count - 1) {
        let _rx = rxs.pop().unwrap();
        let _recv_counter = recv_counter.clone();
        recv_ths.push(tokio::spawn(async move {
            loop {
                match _rx.recv().await {
                    Ok(_) => {
                        let _ = _recv_counter.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }));
    }
    let rx = rxs.pop().unwrap();
    loop {
        match rx.recv().await {
            Ok(_) => {
                let _ = recv_counter.fetch_add(1, Ordering::SeqCst);
            }
            Err(_) => {
                break;
            }
        }
    }
    for th in recv_ths {
        let _ = th.await;
    }
    for th in sender_ths {
        let _ = th.join();
    }
    assert!(counter.load(Ordering::Acquire) >= msg_count);
    assert!(recv_counter.load(Ordering::Acquire) >= msg_count);
}

async fn _crossfire_bounded_async<T: AsyncTxTrait<usize>, R: AsyncRxTrait<usize>>(
    txs: Vec<T>, mut rxs: Vec<R>, msg_count: usize,
) {
    let counter = Arc::new(AtomicUsize::new(0));
    let mut ths = Vec::new();
    for tx in txs {
        let _counter = counter.clone();
        ths.push(tokio::spawn(async move {
            loop {
                let i = _counter.fetch_add(1, Ordering::SeqCst);
                if i < msg_count {
                    if let Err(e) = tx.send(i).await {
                        panic!("send error: {:?}", e);
                    }
                } else {
                    break;
                }
            }
        }));
    }
    let recv_counter = Arc::new(AtomicUsize::new(0));
    let rx_count = rxs.len();
    for _ in 0..(rx_count - 1) {
        let _rx = rxs.pop().unwrap();
        let _recv_counter = recv_counter.clone();
        ths.push(tokio::spawn(async move {
            loop {
                match _rx.recv().await {
                    Ok(_) => {
                        let _ = _recv_counter.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }));
    }
    let rx = rxs.pop().unwrap();
    loop {
        match rx.recv().await {
            Ok(_) => {
                let _ = recv_counter.fetch_add(1, Ordering::SeqCst);
            }
            Err(_) => {
                break;
            }
        }
    }
    for th in ths {
        let _ = th.await;
    }
    assert!(counter.load(Ordering::Acquire) >= msg_count);
    assert!(recv_counter.load(Ordering::Acquire) >= msg_count);
}

fn get_runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
}

fn bench_tokio(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokio_mpsc");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(10));
    for input in [1, 2, 4, 8, 16] {
        let param = Concurrency { tx_count: input, rx_count: 1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc", input), &param, |b, i| {
            b.to_async(get_runtime()).iter(|| _tokio_bounded_mpsc(100, i.tx_count, ONE_MILLION))
        });
    }
}

fn bench_flume_bounded_sync(c: &mut Criterion) {
    let mut group = c.benchmark_group("flume_bounded_sync");
    group.significance_level(0.1).sample_size(50);
    group.throughput(Throughput::Elements(ONE_MILLION as u64));
    group.measurement_time(Duration::from_secs(15));
    for input in [1, 2, 4, 8, 16] {
        let param = Concurrency { tx_count: input, rx_count: 1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc", input), &param, |b, i| {
            b.iter(|| _flume_bounded_sync(100, i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
    for input in [(2, 2), (4, 4), (8, 8), (16, 16)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpmc", param.to_string()), &param, |b, i| {
            b.iter(|| _flume_bounded_sync(100, i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
    group.finish();
}

fn bench_crossbeam_bounded_sync(c: &mut Criterion) {
    let mut group = c.benchmark_group("crossbeam_bounded_sync");
    group.significance_level(0.1).sample_size(50);
    for input in [1, 2, 4, 8, 16] {
        let param = Concurrency { tx_count: input, rx_count: 1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc", input), &param, |b, i| {
            b.iter(|| _crossbeam_bounded_sync(100, i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
    for input in [(2, 2), (4, 4), (8, 8), (16, 16)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpmc", param.to_string()), &param, |b, i| {
            b.iter(|| _crossbeam_bounded_sync(100, i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
    group.finish();
}

fn bench_flume_unbounded_async(c: &mut Criterion) {
    let mut group = c.benchmark_group("flume_unbounded_async");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(20));
    for input in [(1, 1), (2, 1), (4, 1), (8, 1), (16, 1)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc unbounded", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _flume_unbounded_async(100, i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
    for input in [(2, 2), (4, 4), (8, 8), (16, 16)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpmc unbounded", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _flume_unbounded_async(100, i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
}

fn bench_flume_bounded_async(c: &mut Criterion) {
    let mut group = c.benchmark_group("flume_bounded_async");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(20));
    for input in [(1, 1), (2, 1), (4, 1), (8, 1), (16, 1)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(TEN_THOUSAND as u64));
        group.bench_with_input(BenchmarkId::new("mpsc bound 1", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _flume_bounded_async(1, i.tx_count, i.rx_count, TEN_THOUSAND))
        });
    }

    for input in [(1, 1), (2, 1), (4, 1), (8, 1), (16, 1)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc bound 100", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _flume_bounded_async(100, i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
    for input in [(2, 2), (4, 4), (8, 8), (16, 16)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpmc bound 100", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _flume_bounded_async(100, i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
}

fn bench_crossfire_bounded_async_1_1(c: &mut Criterion) {
    let mut group = c.benchmark_group("crossfire_bounded_async_1_1");
    group.significance_level(0.1).sample_size(100);
    group.measurement_time(Duration::from_secs(10));
    for (size, msg_count) in [(1, TEN_THOUSAND), (100, ONE_MILLION)] {
        group.throughput(Throughput::Elements(msg_count as u64));
        group.bench_function(format!("spsc 1x1 size {}", size).to_string(), |b| {
            b.to_async(get_runtime()).iter(async || {
                let (tx, rx) = crossfire::spsc::bounded_async(size);
                _crossfire_bounded_async(vec![tx], vec![rx], msg_count).await;
            })
        });
        group.throughput(Throughput::Elements(msg_count as u64));
        group.bench_function(format!("mpsc 1x1 size {}", size).to_string(), |b| {
            b.to_async(get_runtime()).iter(async || {
                let (tx, rx) = crossfire::mpsc::bounded_async(size);
                _crossfire_bounded_async(vec![tx], vec![rx], msg_count).await;
            })
        });
        group.throughput(Throughput::Elements(msg_count as u64));
        group.bench_function(format!("mpmc 1x1 size {}", size).to_string(), |b| {
            b.to_async(get_runtime()).iter(async || {
                let (tx, rx) = crossfire::mpmc::bounded_async(size);
                _crossfire_bounded_async(vec![tx], vec![rx], msg_count).await;
            })
        });
    }
    group.finish();
}

fn bench_crossfire_bounded_100_async_mpsc(c: &mut Criterion) {
    let mut group = c.benchmark_group("crossfire_bounded_100_async_n_1");
    group.significance_level(0.1).sample_size(100);
    group.measurement_time(Duration::from_secs(20));
    for tx_count in [1, 2, 4, 8, 16] {
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc", tx_count), &tx_count, |b, i| {
            b.to_async(get_runtime()).iter(async || {
                let (tx, rx) = crossfire::mpsc::bounded_async(100);
                _crossfire_bounded_async(_crossfire_atx_clone(tx, *i), vec![rx], ONE_MILLION).await;
            })
        });
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpmc", tx_count), &tx_count, |b, i| {
            b.to_async(get_runtime()).iter(async || {
                let (tx, rx) = crossfire::mpmc::bounded_async(100);
                _crossfire_bounded_async(_crossfire_atx_clone(tx, *i), vec![rx], ONE_MILLION).await;
            })
        });
    }
    group.finish();
}

fn bench_crossfire_bounded_100_async_mpmc(c: &mut Criterion) {
    let mut group = c.benchmark_group("crossfire_bounded_100_async_n_n");
    group.significance_level(0.1).sample_size(100);
    group.measurement_time(Duration::from_secs(20));
    group.throughput(Throughput::Elements(ONE_MILLION as u64));
    for input in [(2, 2), (4, 4), (8, 8), (16, 16)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpmc", &param), &param, |b, i| {
            b.to_async(get_runtime()).iter(async || {
                let (tx, rx) = crossfire::mpmc::bounded_async(100);
                _crossfire_bounded_async(
                    _crossfire_atx_clone(tx, i.tx_count),
                    _crossfire_arx_clone(rx, i.rx_count),
                    ONE_MILLION,
                )
                .await;
            })
        });
    }
    group.finish();
}

fn bench_crossfire_unbounded_async_1_1(c: &mut Criterion) {
    let mut group = c.benchmark_group("crossfire_unbounded_async_1_1");
    group.significance_level(0.1).sample_size(100);
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Elements(ONE_MILLION as u64));
    group.bench_function("spsc 1x1", |b| {
        b.to_async(get_runtime()).iter(async || {
            let (tx, rx) = crossfire::spsc::unbounded_async();
            _crossfire_blocking_async(vec![tx], vec![rx], ONE_MILLION).await;
        })
    });
    group.bench_function("mpsc 1x1", |b| {
        b.to_async(get_runtime()).iter(async || {
            let (tx, rx) = crossfire::mpsc::unbounded_async();
            _crossfire_blocking_async(vec![tx], vec![rx], ONE_MILLION).await;
        })
    });

    group.bench_function("mpmc 1x1", |b| {
        b.to_async(get_runtime()).iter(async || {
            let (tx, rx) = crossfire::mpmc::unbounded_async();
            _crossfire_blocking_async(vec![tx], vec![rx], ONE_MILLION).await;
        })
    });
    group.finish();
}

fn bench_crossfire_unbounded_async_mpsc(c: &mut Criterion) {
    let mut group = c.benchmark_group("crossfire_unbounded_async_n_1");
    group.significance_level(0.1).sample_size(100);
    group.measurement_time(Duration::from_secs(20));
    group.throughput(Throughput::Elements(ONE_MILLION as u64));
    for input in [1, 2, 4, 8, 16] {
        let param = Concurrency { tx_count: input, rx_count: 1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc", &param), &param, |b, i| {
            b.to_async(get_runtime()).iter(async || {
                let (tx, rx) = crossfire::mpsc::unbounded_async();
                _crossfire_blocking_async(
                    _crossfire_btx_clone(tx, i.tx_count),
                    vec![rx],
                    ONE_MILLION,
                )
                .await;
            })
        });
    }
    for input in [1, 2, 4, 8, 16] {
        let param = Concurrency { tx_count: input, rx_count: 1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpmc", &param), &param, |b, i| {
            b.to_async(get_runtime()).iter(async || {
                let (tx, rx) = crossfire::mpmc::unbounded_async();
                _crossfire_blocking_async(
                    _crossfire_btx_clone(tx, i.tx_count),
                    vec![rx],
                    ONE_MILLION,
                )
                .await;
            })
        });
    }
    group.finish();
}

fn bench_crossfire_unbounded_async_mpmc(c: &mut Criterion) {
    let mut group = c.benchmark_group("crossfire_unbounded_async_n_n");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(20));
    for input in [(2, 2), (4, 4), (8, 8), (16, 16)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_function(format!("{}", param).to_string(), |b| {
            b.to_async(get_runtime()).iter(async || {
                let (tx, rx) = crossfire::mpmc::unbounded_async();
                _crossfire_blocking_async(
                    _crossfire_btx_clone(tx, input.0),
                    _crossfire_arx_clone(rx, input.1),
                    ONE_MILLION,
                )
                .await;
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_tokio,
    bench_crossbeam_bounded_sync,
    bench_flume_bounded_sync,
    bench_flume_bounded_async,
    bench_flume_unbounded_async,
    bench_crossfire_bounded_async_1_1,
    bench_crossfire_bounded_100_async_mpsc,
    bench_crossfire_bounded_100_async_mpmc,
    bench_crossfire_unbounded_async_1_1,
    bench_crossfire_unbounded_async_mpsc,
    bench_crossfire_unbounded_async_mpmc,
);
criterion_main!(benches);
