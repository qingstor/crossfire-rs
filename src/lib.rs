//! # Crossfire
//!
//! High-performance spsc/mpsc/mpmc channels.
//!
//! It supports async context, and bridge the gap between async and blocking context.
//!
//! Implemented with lockless in mind, low level is based on crossbeam-channel.
//! For the concept, please refer to [wiki](https://github.com/frostyplanet/crossfire-rs/wiki).
//!
//! ## Stability and versions
//!
//! Crossfire v1.0 has been released and used in production since 2022.12. Heavily tested on X86_64 and ARM.
//!
//! V2.0 has refactored the codebase and API at 2025.6. By removing generic types of ChannelShared object in sender and receiver,
//! it's easier to remember and code.
//!
//! V2.0.x branch will remain in maintenance mode. Future optimization might be in v2.x_beta
//! version until long run tests prove to be stable.
//!
//! ## Performance
//!
//! We focus on optimization of async logic, outperforming other async capability channel
//! implementations
//! (flume, tokio::mpsc, etc).
//!
//! Due to context switching between sleep and wake, there is a certain
//! overhead over crossbeam-channel.
//!
//! Benchmark is written in criterion framework. You can run benchmark by:
//!
//! ``` shell
//! cargo bench
//! ```
//!
//! Some benchmark data is posted on [wiki](https://github.com/frostyplanet/crossfire-rs/wiki).
//!
//! ## APIs
//!
//! ### modules and functions
//!
//! There are 3 modules: [spsc], [mpsc], [mpmc], providing functions to allocate different types of channels.
//!
//! For SP or SC, it's more memory efficient than MP or MC implementations, and sometimes slightly faster.
//!
//! The return types in these 3 modules are different:
//!
//! * [mpmc::bounded_async()]:  (tx async, rx async)
//!
//! * [mpmc::bounded_tx_async_rx_blocking()]
//!
//! * [mpmc::bounded_tx_blocking_rx_async()]
//!
//! * [mpmc::unbounded_async()]
//!
//!
//! > **NOTE** :  For bounded channel, 0 size case is not supported yet. (Temporary rewrite as 1 size).
//!
//!
//! ### Types
//!
//! <table align="center" cellpadding="20">
//! <tr> <th rowspan="2"> Context</th><th colspan="2" align="center">Sender (Producer)</th> <th colspan="2" align="center">Receiver (Consumer)</th> </tr>
//! <tr> <td>Single</td> <td>Multiple</td><td>Single</td><td>Multiple</td></tr>
//! <tr><td rowspan="2"><b>Blocking</b></td><td colspan="2" align="center"><a href="trait.BlockingTxTrait.html">BlockingTxTrait</a></td>
//! <td colspan="2" align="center"><a href="trait.BlockingRxTrait.html">BlockingRxTrait</a></td></tr>
//! <tr>
//! <td align="center"><a href="struct.Tx.html">Tx</a></td>
//! <td align="center"><a href="struct.MTx.html">MTx</a></td>
//! <td align="center"><a href="struct.Rx.html">Rx</a></td>
//! <td align="center"><a href="struct.MRx">MRx</a></td> </tr>
//!
//! <tr><td rowspan="2"><b>Async</b></td>
//! <td colspan="2" align="center"><a href="trait.AsyncTxTrait.html">AsyncTxTrait</a></td>
//! <td colspan="2" align="center"><a href="trait.AsyncRxTrait.html">AsyncRxTrait</a></td></tr>
//! <tr><td><a href="struct.AsyncTx.html">AsyncTx</a></td>
//! <td><a href="struct.MAsyncTx.html">MAsyncTx</a></td><td><a href="struct.AsyncRx.html">AsyncRx</a></td>
//! <td><a href="struct.MAsyncRx.html">MAsyncRx</a></td></tr>
//!
//! </table>
//!
//! > **NOTE**: For SP / SC version [AsyncTx] and [AsyncRx], although not designed to be not cloneable,
//! send() recv() use immutable &self for convenient reason. Be careful do not use the SP / SC concurrently when put in Arc.
//!
//! ### Error types
//!
//! Error types are re-exported from crossbeam-channel:  [TrySendError], [SendError], [TryRecvError], [RecvError]
//!
//! ### Async compatibility
//!
//! Mainly tested on tokio-1.x.
//!
//! In async context, future-select! can be used.  Cancelling is supported. You can combine
//! send() or recv() future with tokio::time::timeout.
//!
//! While using MAsyncTx or MAsyncRx, there's memory overhead to pass along small size wakers
//! for pending async producer or consumer. Because we aim to be lockless,
//! when the sending/receiving futures are cancelled (like tokio::time::timeout()),
//! might trigger immediate cleanup if non-conflict conditions are met.
//! Otherwise will rely on lazy cleanup. (waker will be consumed by actual message send and recv).
//!
//! Never the less, for close notification without sending anything,
//! I suggest that use `tokio::sync::oneshot` instead.
//!
//! ## Usage
//!
//! Cargo.toml:
//! ```toml
//! [dependencies]
//! crossfire = "2.0"
//! ```
//! example:
//!
//! ```rust
//!
//! extern crate crossfire;
//! use crossfire::*;
//! #[macro_use]
//! extern crate tokio;
//!
//! #[tokio::main]
//! async fn main() {
//!     let (tx, rx) = mpsc::bounded_async::<i32>(100);
//!     tokio::spawn(async move {
//!        for i in 0i32..10000 {
//!            let _ = tx.send(i).await;
//!            println!("sent {}", i);
//!        }
//!     });
//!     loop {
//!         if let Ok(_i) = rx.recv().await {
//!             println!("recv {}", _i);
//!         } else {
//!             println!("rx closed");
//!             break;
//!         }
//!     }
//! }
//!
//! ```

extern crate crossbeam;
extern crate futures;
#[macro_use]
extern crate enum_dispatch;

mod channel;
mod locked_waker;
mod waker_registry;
pub use locked_waker::LockedWaker;

/// collections that can be re-used
pub mod collections;

/// Multi producers, single consumer
pub mod mpmc;
/// Multi producers, multi consumers
pub mod mpsc;
/// Single producer, single consumer
pub mod spsc;

mod blocking_tx;
pub use blocking_tx::*;
mod blocking_rx;
pub use blocking_rx::*;
mod async_tx;
pub use async_tx::*;
mod async_rx;
pub use async_rx::*;

pub mod stream;

#[cfg(test)]
mod tests;
