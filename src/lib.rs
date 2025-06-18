//! # Crossfire
//!
//! This crate provide channels used between async-async or async-blocking code, in all direction.
//! Implmented with lockless in mind, low level is based on crossbeam-channel
//!
//! ## Performance
//!
//! Faster than channel in std or mpsc in tokio, slightly slower than crossbeam itself (since async overhead to wakeup sender or receiver).
//!
//! ## APIs
//!
//!
//! ## Usage
//!
//! Add this to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! crossfire = "1"
//! tokio = "1"
//! ```
//!
//! ```rust
//!
//! extern crate crossfire;
//! extern crate tokio;
//! use crossfire::mpsc;
//!
//! // async-async
//!
//! let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
//!
//! let (tx, rx) = mpsc::bounded_future_both::<i32>(100);
//!
//! rt.block_on(async move {
//!    tokio::spawn(async move {
//!        for i in 0i32..10000 {
//!            let _ = tx.send(i).await;
//!            println!("sent {}", i);
//!        }
//!    });
//!
//!    loop {
//!        if let Ok(_i) = rx.recv().await {
//!            println!("recv {}", _i);
//!        } else {
//!            println!("rx closed");
//!            break;
//!        }
//!    }
//! });
//!
//! ```
//!
//! mpmc & mpsc package is almost the same, while mpsc has some optimization becauses it assumes only one consumer.
//!
//! Error types are re-exported from crossbeam-channel.
//!
//! ## Compatibility
//!
//! Supports stable Rust. Mainly tested on tokio-0.2 (Not tried on async-std or other runtime).
//! future::selects and timeout work fine, but it takes advantage of runtime behavior not documented by Rust official.
//!
//! Refer to https://github.com/rust-lang/rust/issues/73002
//!
//! ## Memory overhead
//! While using mp tx or mp rx, there's memory overhead to pass along wakers for pending async producer or consumer.
//! Since waker is small, the overhead can be ignored if your channel is busy.
//! Canceled wakers will be eventually cleanup by later send/receive event.
//! If the channel is used for close notification (which never trigger) in combine with futures::select,
//! currently there's hard coded threshold to clean up those canceled wakers.

extern crate crossbeam;
#[macro_use]
extern crate async_trait;
extern crate futures;

#[macro_use]
pub mod channel;
pub mod mpmc;
pub mod mpsc;

pub use crossbeam::channel::{RecvError, SendError, TryRecvError, TrySendError};
