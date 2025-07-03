use criterion::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

mod common;
use common::*;

fn _flume_bounded_sync(bound: usize, tx_count: usize, rx_count: usize, msg_count: usize) {
    let (tx, rx) = flume::bounded(bound);
    let send_counter = Arc::new(AtomicUsize::new(0));
    let mut th_s = Vec::new();
    for _tx_i in 0..tx_count {
        let _send_counter = send_counter.clone();
        let _tx = tx.clone();
        th_s.push(thread::spawn(move || loop {
            let i = _send_counter.fetch_add(1, Ordering::SeqCst);
            if i < msg_count {
                if let Err(e) = _tx.send(i) {
                    panic!("send error: {:?}", e);
                }
            } else {
                break;
            }
        }));
    }
    drop(tx);
    let recv_counter = Arc::new(AtomicUsize::new(0));
    for _ in 0..(rx_count - 1) {
        let _rx = rx.clone();
        let _rx_counter = recv_counter.clone();
        th_s.push(thread::spawn(move || loop {
            match _rx.recv() {
                Ok(_) => {
                    let _ = _rx_counter.fetch_add(1, Ordering::SeqCst);
                }
                Err(_) => {
                    break;
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
    for th in th_s {
        let _ = th.join();
    }
    assert!(send_counter.load(Ordering::Acquire) >= msg_count);
    assert!(recv_counter.load(Ordering::Acquire) >= msg_count);
}

async fn _flume_unbounded_async(tx_count: usize, rx_count: usize, msg_count: usize) {
    let (tx, rx) = flume::unbounded();
    let counter = Arc::new(AtomicUsize::new(0));
    let mut th_s = Vec::new();
    for _tx_i in 0..tx_count {
        let _counter = counter.clone();
        let _tx = tx.clone();
        th_s.push(tokio::spawn(async move {
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
        th_s.push(tokio::spawn(async move {
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
    for th in th_s {
        let _ = th.await;
    }
    assert!(counter.load(Ordering::Acquire) >= msg_count);
    assert!(recv_counter.load(Ordering::Acquire) >= msg_count);
}

async fn _flume_bounded_async(bound: usize, tx_count: usize, rx_count: usize, msg_count: usize) {
    let (tx, rx) = flume::bounded(bound);
    let counter = Arc::new(AtomicUsize::new(0));
    let mut th_s = Vec::new();
    for _tx_i in 0..tx_count {
        let _counter = counter.clone();
        let _tx = tx.clone();
        th_s.push(tokio::spawn(async move {
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
        th_s.push(tokio::spawn(async move {
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
    for th in th_s {
        let _ = th.await;
    }
    assert!(counter.load(Ordering::Acquire) >= msg_count);
    assert!(recv_counter.load(Ordering::Acquire) >= msg_count);
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

fn bench_flume_unbounded_async(c: &mut Criterion) {
    let mut group = c.benchmark_group("flume_unbounded_async");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(20));
    for input in [(1, 1), (2, 1), (4, 1), (8, 1), (16, 1)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc unbounded", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _flume_unbounded_async(i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
    for input in [(2, 2), (4, 4), (8, 8), (16, 16)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpmc unbounded", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _flume_unbounded_async(i.tx_count, i.rx_count, ONE_MILLION))
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

criterion_group!(
    benches,
    bench_flume_bounded_sync,
    bench_flume_bounded_async,
    bench_flume_unbounded_async,
);
criterion_main!(benches);
