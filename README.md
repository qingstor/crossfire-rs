# Crossfire

[![Build Status](https://github.com/frostyplanet/crossfire-rs/workflows/Rust/badge.svg)](
https://github.com/frostyplanet/crossfire-rs/actions)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](
https://github.com/qignstor/crossfire-rs#license)
[![Cargo](https://img.shields.io/crates/v/crossfire.svg)](
https://crates.io/crates/crossfire)
[![Documentation](https://docs.rs/crossfire/badge.svg)](
https://docs.rs/crossfire)
[![Rust 1.36+](https://img.shields.io/badge/rust-1.36+-lightgray.svg)](
https://www.rust-lang.org)

High-performance spsc/mpsc/mpmc channels.

It supports async context, or communicates between async-blocking context.

Implemented with lockless in mind, low level is based on crossbeam-channel.

## Stablity and versions

Crossfire v1.0 has been released and used in production since 2022.12. Heavily tested on X86_64 and ARM.

v2.0 has refactored the codebase and API at 2025.6. By removing generic types of ChanelShare object in sender and receiver,
it's easier to remember and code.

v2.0.x branch will remain in maintenance mode. Further optimization might be in v2.x_beta
version until long run tests prove to be stable.

## Performance

We focus on optimization of async logic, outperforming other async capability channels
(flume, tokio::mpsc, etc) in most cases.

Due to context switching between sleep and wake, there is a certain
overhead on async context over crossbeam-channel which in blocking context.

Benchmark is written in criterion framework. You can run benchmark by:

```
cargo bench
```

More benchmark data is on [wiki](https://github.com/frostyplanet/crossfire-rs/wiki). Here are some of the results:

<img src="https://github.com/frostyplanet/crossfire-rs/wiki/images/benchmark-2025-06-27/mpmc_bound_size_100_async.png" alt="mpmc bounded size 100 async context" width="100%">

<img src="https://github.com/frostyplanet/crossfire-rs/wiki/images/benchmark-2025-06-27/mpsc_unbounded_async.png" alt="mpsc unbounded async context" width="100%">


## APIs

### modules and functions

There are 3 modules: [spsc], [mpsc], [mpmc], providing functions to allocate different types of channels.

For SP or SC, it's more memory efficient than MP or MC implementations, and sometimes slightly faster.

The return types in these 3 modules are different:

* [mpmc::bounded_async()]:  (tx async, rx async)

* [mpmc::bounded_tx_async_rx_blocking()]

* [mpmc::bounded_tx_blocking_rx_async()]

* [mpmc::unbounded_async()]


> **NOTE** :  For bounded channel, 0 size case is not supported yet. (Temporary rewrite as 1 size).

### Types

<table align="center" cellpadding="30">
<tr> <th rowspan="2"> Context </th><th colspan="2" align="center"> Sender (Producer) </th> <th colspan="2" align="center"> Receiver (Consumer) </th> </tr>
<tr> <td> Single </td> <td> Multiple </td><td> Single </td><td> Multiple </td></tr>
<tr><td rowspan="2"> <b>Blocking</b> </td>
<td colspan="2" align="center"> <a href="trait.BlockingTxTrait.html">BlockingRxTrait</a> </td>
<td colspan="2" align="center"> <a href="trait.BlockingRxTrait.html">BlockingRxTrait</a> </td></tr>
<tr>
<td align="center"> <a href="struct.Tx.html">Tx</a> </td>
<td align="center"> <a href="struct.MTx.html">MTx</a> </td>
<td align="center"> <a href="struct.Rx.html">Rx</a> </td>
<td align="center"> <a href="struct.MRx">MRx</a> </td> </tr>

<tr><td rowspan="2"><b>Async</b></td>
<td colspan="2" align="center"><a href="trait.AsyncTxTrait.html">AsyncTxTrait</a></td>
<td colspan="2" align="center"><a href="trait.AsyncRxTrait.html">AsyncRxTrait</a></td></tr>
<tr>
<td> <a href="struct.AsyncTx.html">AsyncTx</a> </td>
<td> <a href="struct.MAsyncTx.html">MAsyncTx</a> </td>
<td> <a href="struct.AsyncRx.html">AsyncRx</a> </td>
<td> <a href="struct.MAsyncRx.html">MAsyncRx</a> </td></tr>

</table>


> **NOTE**: For SP / SC version [AsyncTx] and [AsyncRx], although not designed to be non-clonable,
 send() recv() use immutable &self for convenient reason. Be careful do not use the SP/SC concurrently when put in Arc.

### Error types

Error types are re-exported from crossbeam-channel:  [TrySendError], [SendError], [TryRecvError], [RecvError]

### Async compatibility

Mainly tested on tokio-1.x.

In async context, future-select! can be used.  Cancelling is supported. You can combine
send() or recv() future with tokio::time::timeout.

While using MAsyncTx or MAsyncRx, there's memory overhead to pass along small size wakers
for pending async producer or consumer. Because we aim to be lockless,
when the sending/receiving futures are cancelled (like tokio::time::timeout()),
might trigger immediate cleanup if non-conflict conditions are met.
Otherwise will rely on lazy cleanup. (waker will be consumed by actural message send and recv).

Never the less, for close notification without sending anything,
I suggest that use `tokio::sync::oneshot` instead.

## Usage

Cargo.toml:
```toml
[dependencies]
crossfire = "2.0"
```
example:

```rust

extern crate crossfire;
use crossfire::*;

#[tokio::main]
async main() {
    let (tx, rx) = mpsc::bounded_async::<i32>(100);
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
}

```
