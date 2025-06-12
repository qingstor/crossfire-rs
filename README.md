# Crossfire

[![Build Status](https://github.com/qingstor/crossfire-rs/workflows/Rust/badge.svg)](
https://github.com/qingstor/crossfire-rs/actions)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](
https://github.com/qignstor/crossfire-rs#license)
[![Cargo](https://img.shields.io/crates/v/crossfire.svg)](
https://crates.io/crates/crossfire)
[![Documentation](https://docs.rs/crossfire/badge.svg)](
https://docs.rs/crossfire)
[![Rust 1.36+](https://img.shields.io/badge/rust-1.36+-lightgray.svg)](
https://www.rust-lang.org)


This crate provide channels used between async-async or async-blocking code, in all direction.
Implmented with lockless in mind, low level is based on crossbeam-channel

## Performance

Faster than channel in std or mpsc in tokio, slightly slower than crossbeam itself (since async overhead to wake up sender or receiver).

Run the benchmark tests to see for yourself:

	cargo test performance --release -- --nocapture --test-threads=1


## APIs


## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
crossfire = "0.1"
```

```rust

extern crate crossfire;
extern crate tokio;

use crossfire::mpsc;

// async-async

let (tx, rx) = mpsc::bounded_future_both::<i32>(100);
tokio::spawn(async move {
    for i in 0i32..10000 {
        let _ = tx.send(i).await;
        println!("sent {}", i);
    }
});

loop {
    if let Ok(_i) = rx.recv().await {
        println!("recv {}", _i);
    } else {
        println!("rx closed");
        break;
    }
}

```

mpmc & mpsc package is almost the same, while mpsc has some optimization becauses it assumes only one consumer.

Error types are re-exported from crossbeam-channel.


## Compatibility

Supports stable Rust. Mainly tested on tokio-1.x (Not tested on async-std or other runtime).
future::selects and timeout work fine, but it takes advantage of runtime behavior not documented by Rust official.

Refer to https://github.com/rust-lang/rust/issues/73002


## Memory overhead

While using mp tx or mp rx, there's memory overhead to pass along wakers for pending async producer or consumer.
Since waker is small, the overhead can be ignored if your channel is busy.
Canceled wakers will be eventually cleanup by later send/receive event.
If the channel is used for close notification (which never trigger) in combine with futures::select,
currently there's hard coded threshold to clean up those canceled wakers.

## Stability

This channel implementation serves in various components of our storage engine, you are
welcome to rely on it.
