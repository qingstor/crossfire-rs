use criterion::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

mod common;
use common::*;

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

criterion_group!(benches, bench_tokio,);
criterion_main!(benches);
