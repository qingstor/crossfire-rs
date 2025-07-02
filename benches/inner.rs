use criterion::*;
use crossbeam::queue::ArrayQueue;
use crossfire::collections::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Weak,
};
use std::thread;
use std::time::Duration;

const ONE_MILLION: usize = 1000000;

struct Foo {
    _inner: usize,
}

fn _bench_array_queue(count: usize) {
    let queue = Arc::new(ArrayQueue::<Weak<Foo>>::new(1));
    let mut th_s = Vec::new();
    let counter = Arc::new(AtomicUsize::new(0));
    for _ in 0..count {
        let _queue = queue.clone();
        let _counter = counter.clone();
        th_s.push(thread::spawn(move || loop {
            let i = _counter.fetch_add(1, Ordering::SeqCst);
            if i < ONE_MILLION {
                if let Some(weak) = _queue.pop() {
                    let _ = weak.upgrade();
                }
            } else {
                break;
            }
        }));
    }
    th_s.push(thread::spawn(move || {
        for _ in 0..ONE_MILLION {
            let foo = Arc::new(Foo { _inner: 1 });
            if let Err(w) = queue.push(Arc::downgrade(&foo)) {
                let _ = queue.pop();
                let _ = queue.push(w);
            }
        }
    }));
    for th in th_s {
        let _ = th.join();
    }
}

fn _bench_spmc_cell(count: usize) {
    let cell = Arc::new(SpmcCell::<Foo>::new());
    let mut th_s = Vec::new();
    let counter = Arc::new(AtomicUsize::new(0));
    for _ in 0..count {
        let _cell = cell.clone();
        let _counter = counter.clone();
        th_s.push(thread::spawn(move || loop {
            let i = _counter.fetch_add(1, Ordering::SeqCst);
            if i < ONE_MILLION {
                let _ = _cell.pop();
            } else {
                break;
            }
        }));
    }
    th_s.push(thread::spawn(move || {
        for _ in 0..ONE_MILLION {
            let foo = Arc::new(Foo { _inner: 1 });
            cell.put(Arc::downgrade(&foo));
        }
    }));
    for th in th_s {
        let _ = th.join();
    }
}

fn _bench_empty(c: &mut Criterion) {
    let mut group = c.benchmark_group("empty");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Elements(ONE_MILLION as u64));
    group.bench_function("spmc", |b| {
        b.iter(|| {
            let cell = SpmcCell::<Foo>::new();
            for _ in 0..ONE_MILLION {
                let _ = cell.pop();
            }
        })
    });
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Elements(ONE_MILLION as u64));
    group.bench_function("array_queue", |b| {
        b.iter(|| {
            let queue = ArrayQueue::<Foo>::new(1);
            for _ in 0..ONE_MILLION {
                let _ = queue.pop();
            }
        })
    });
}

fn _bench_sequence(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequence");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Elements(ONE_MILLION as u64));
    group.bench_function("spmc", |b| {
        b.iter(|| {
            let cell = SpmcCell::<Foo>::new();
            for _ in 0..ONE_MILLION {
                let foo = Arc::new(Foo { _inner: 1 });
                cell.put(Arc::downgrade(&foo));
                let _ = cell.pop();
            }
        })
    });
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Elements(ONE_MILLION as u64));
    group.bench_function("array_queue", |b| {
        b.iter(|| {
            let queue = ArrayQueue::<Weak<Foo>>::new(1);
            for _ in 0..ONE_MILLION {
                let foo = Arc::new(Foo { _inner: 1 });
                let _ = queue.push(Arc::downgrade(&foo));
                if let Some(w) = queue.pop() {
                    let _ = w.upgrade();
                }
            }
        })
    });
}

fn _bench_threads(c: &mut Criterion) {
    let mut group = c.benchmark_group("threads");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(10));

    for input in [1, 2, 4, 8, 16] {
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("spmc_cell", input), &input, |b, i| {
            b.iter(|| _bench_spmc_cell(*i))
        });
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("array_queue", input), &input, |b, i| {
            b.iter(|| _bench_array_queue(*i))
        });
    }
}

criterion_group!(benches, _bench_empty, _bench_sequence, _bench_threads,);
criterion_main!(benches);
