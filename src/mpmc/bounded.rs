use super::rx::*;
use super::tx::*;
use crate::channel::*;
use crossbeam::queue::SegQueue;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::task::*;

/// Initiate a bounded channel that sender and receiver are async
pub fn bounded_future_both<T: Unpin>(
    size: usize,
) -> (TxFuture<T, SharedFutureBoth>, RxFuture<T, SharedFutureBoth>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let shared = Arc::new(SharedFutureBoth::new());

    let tx_f = TxFuture::new(tx, shared.clone());
    let rx_f = RxFuture::new(rx, shared);
    (tx_f, rx_f)
}

/// Initiate a bounded channel that sender is async, receiver is blocking
pub fn bounded_tx_future_rx_blocking<T: Unpin>(
    size: usize,
) -> (TxFuture<T, SharedSenderFRecvB>, RxBlocking<T, SharedSenderFRecvB>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let shared = Arc::new(SharedSenderFRecvB::new());

    let tx_f = TxFuture::new(tx, shared.clone());
    let rx_b = RxBlocking::new(rx, shared);
    (tx_f, rx_b)
}

/// Initiate a bounded channel that sender is blocking, receiver is sync
pub fn bounded_tx_blocking_rx_future<T>(
    size: usize,
) -> (TxBlocking<T, SharedSenderBRecvF>, RxFuture<T, SharedSenderBRecvF>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let shared = Arc::new(SharedSenderBRecvF::new());

    let tx_b = TxBlocking::new(tx, shared.clone());
    let rx_f = RxFuture::new(rx, shared);
    (tx_b, rx_f)
}

pub struct SharedFutureBoth {
    tx_count: AtomicUsize,
    rx_count: AtomicUsize,
    sender_waker: SegQueue<LockedWakerRef>,
    recv_waker: SegQueue<LockedWakerRef>,
    send_waker_tx_seq: AtomicU64,
    send_waker_rx_seq: AtomicU64,
    recv_waker_tx_seq: AtomicU64,
    recv_waker_rx_seq: AtomicU64,
    checking_sender: AtomicBool,
    checking_recv: AtomicBool,
}

impl MPMCShared for SharedFutureBoth {
    fn new() -> Self {
        Self {
            sender_waker: SegQueue::new(),
            recv_waker: SegQueue::new(),
            tx_count: AtomicUsize::new(1),
            rx_count: AtomicUsize::new(1),
            checking_sender: AtomicBool::new(false),
            checking_recv: AtomicBool::new(false),
            send_waker_tx_seq: AtomicU64::new(0),
            send_waker_rx_seq: AtomicU64::new(0),
            recv_waker_tx_seq: AtomicU64::new(0),
            recv_waker_rx_seq: AtomicU64::new(0),
        }
    }

    #[inline]
    fn on_recv(&self) {
        on_recv_m!(self)
    }

    #[inline]
    fn on_send(&self) {
        on_send_m!(self)
    }

    #[inline]
    fn reg_recv(&self, ctx: &mut Context) -> Option<LockedWaker> {
        reg_recv_m!(self, ctx)
    }

    #[inline]
    fn reg_send(&self, ctx: &mut Context) -> Option<LockedWaker> {
        reg_send_m!(self, ctx)
    }

    #[inline(always)]
    fn add_tx(&self) {
        let _ = self.tx_count.fetch_add(1, Ordering::SeqCst);
    }

    #[inline(always)]
    fn add_rx(&self) {
        let _ = self.rx_count.fetch_add(1, Ordering::SeqCst);
    }

    #[inline]
    fn close_tx(&self) {
        close_tx_common!(self)
    }

    #[inline]
    fn close_rx(&self) {
        close_rx_common!(self)
    }

    #[inline]
    fn get_tx_count(&self) -> usize {
        self.tx_count.load(Ordering::SeqCst)
    }

    fn get_waker_length(&self) -> (usize, usize) {
        (self.sender_waker.len(), self.recv_waker.len())
    }

    #[inline]
    fn clear_send_wakers(&self, waker: LockedWaker) {
        clear_sender_wakers_common!(self, waker.get_seq())
    }

    #[inline]
    fn clear_recv_wakers(&self, waker: LockedWaker) {
        clear_recv_wakers_common!(self, waker.get_seq())
    }
}

pub struct SharedSenderBRecvF {
    tx_count: AtomicUsize,
    rx_count: AtomicUsize,
    recv_waker: SegQueue<LockedWakerRef>,
    recv_waker_tx_seq: AtomicU64,
    recv_waker_rx_seq: AtomicU64,
    checking_recv: AtomicBool,
}

impl MPMCShared for SharedSenderBRecvF {
    fn new() -> Self {
        Self {
            recv_waker: SegQueue::new(),
            tx_count: AtomicUsize::new(1),
            rx_count: AtomicUsize::new(1),
            recv_waker_tx_seq: AtomicU64::new(0),
            recv_waker_rx_seq: AtomicU64::new(0),
            checking_recv: AtomicBool::new(false),
        }
    }

    #[inline]
    fn on_recv(&self) {}

    #[inline]
    fn on_send(&self) {
        on_send_m!(self)
    }

    #[inline]
    fn reg_recv(&self, ctx: &mut Context) -> Option<LockedWaker> {
        reg_recv_m!(self, ctx)
    }

    #[inline]
    fn reg_send(&self, _ctx: &mut Context) -> Option<LockedWaker> {
        None
    }

    #[inline]
    fn add_tx(&self) {
        self.tx_count.fetch_add(1, Ordering::SeqCst);
    }

    #[inline]
    fn add_rx(&self) {
        self.rx_count.fetch_add(1, Ordering::SeqCst);
    }

    #[inline]
    fn close_tx(&self) {
        close_tx_common!(self)
    }

    #[inline]
    fn close_rx(&self) {
        self.rx_count.fetch_sub(1, Ordering::SeqCst);
    }

    #[inline]
    fn get_tx_count(&self) -> usize {
        self.tx_count.load(Ordering::SeqCst)
    }

    fn get_waker_length(&self) -> (usize, usize) {
        (0, self.recv_waker.len())
    }

    #[inline]
    fn clear_recv_wakers(&self, waker: LockedWaker) {
        clear_recv_wakers_common!(self, waker.get_seq())
    }
}

pub struct SharedSenderFRecvB {
    tx_count: AtomicUsize,
    rx_count: AtomicUsize,
    sender_waker: SegQueue<LockedWakerRef>,
    send_waker_tx_seq: AtomicU64,
    send_waker_rx_seq: AtomicU64,
    checking_sender: AtomicBool,
}

impl MPMCShared for SharedSenderFRecvB {
    fn new() -> Self {
        Self {
            sender_waker: SegQueue::new(),
            rx_count: AtomicUsize::new(1),
            tx_count: AtomicUsize::new(1),
            send_waker_tx_seq: AtomicU64::new(0),
            send_waker_rx_seq: AtomicU64::new(0),
            checking_sender: AtomicBool::new(false),
        }
    }

    #[inline]
    fn on_recv(&self) {
        on_recv_m!(self)
    }

    #[inline]
    fn on_send(&self) {}

    #[inline]
    fn reg_recv(&self, _ctx: &mut Context) -> Option<LockedWaker> {
        None
    }

    #[inline]
    fn reg_send(&self, ctx: &mut Context) -> Option<LockedWaker> {
        reg_send_m!(self, ctx)
    }

    #[inline]
    fn add_tx(&self) {
        self.tx_count.fetch_add(1, Ordering::SeqCst);
    }

    #[inline]
    fn add_rx(&self) {
        self.rx_count.fetch_add(1, Ordering::SeqCst);
    }

    #[inline]
    fn close_tx(&self) {
        self.tx_count.fetch_sub(1, Ordering::SeqCst);
    }

    #[inline]
    fn close_rx(&self) {
        close_rx_common!(self)
    }

    #[inline]
    fn get_tx_count(&self) -> usize {
        return self.tx_count.load(Ordering::Acquire);
    }

    #[inline]
    fn clear_send_wakers(&self, waker: LockedWaker) {
        clear_sender_wakers_common!(self, waker.get_seq())
    }

    fn get_waker_length(&self) -> (usize, usize) {
        (self.sender_waker.len(), 0)
    }
}

#[cfg(test)]
mod tests {

    extern crate tokio;
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};
    use tokio::time::timeout;

    #[test]
    fn bench_std_sync_channel_performance() {
        println!();
        let total_message = 1000000;
        let (tx, rx) = std::sync::mpsc::sync_channel(100);
        let start = Instant::now();
        thread::spawn(move || {
            let _tx = tx.clone();
            for i in 0..total_message {
                let _ = _tx.send(i);
            }
        });

        for _ in 0..total_message {
            rx.recv().unwrap();
        }
        let end = Instant::now();

        println!("{} message, single sender thread single receiver thread use std::sync::sync_channel, cost time:{} s",
                 total_message, (total_message as f64) / end.duration_since(start).as_secs_f64());
    }

    #[test]
    fn bench_crossbeam_channel_performance() {
        println!();
        let total_message = 1000000;
        let (tx, rx) = crossbeam::channel::bounded(100);
        let start = Instant::now();
        thread::spawn(move || {
            let _tx = tx.clone();
            for i in 0..total_message {
                let _ = _tx.send(i);
            }
        });

        for _ in 0..total_message {
            rx.recv().unwrap();
        }
        let end = Instant::now();

        println!(
            "{} message, single sender thread single receiver thread use crossbeam::channel, {} /s",
            total_message,
            (total_message as f64) / end.duration_since(start).as_secs_f64()
        );
    }

    #[test]
    fn bench_future_both_performance() {
        println!();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let total_message = 1000000;
            let (tx, rx) = bounded_future_both::<i32>(100);
            let start = Instant::now();
            tokio::spawn(async move {
                println!("sender thread send {} message start", total_message);
                for i in 0i32..total_message {
                    let _ = tx.send(i).await;
                    //println!("sent {}", i);
                }
                println!("sender thread send {} message end", total_message);
            });

            for _ in 0..total_message {
                if let Ok(_i) = rx.recv().await {
                    //println!("recv {}", _i);
                }
            }
            let end = Instant::now();

            println!(
                "{} message, single sender thread single receiver thread use mpmc {} /s",
                total_message,
                (total_message as f64) / end.duration_since(start).as_secs_f64()
            );
        });
    }

    #[test]
    fn bench_tx_blocking_rx_future_performance() {
        println!();
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let total_message = 1000000;
        let (tx, rx_f) = bounded_tx_blocking_rx_future::<i32>(100);
        let start = Instant::now();
        thread::spawn(move || {
            for i in 0..total_message {
                let _ = tx.send(i);
            }
        });
        rt.block_on(async move {
            for _ in 0..total_message {
                let _ = rx_f.recv().await;
            }
            let end = Instant::now();

            println!(
                "{} message, single sender thread single receiver thread use mpmc {} /s",
                total_message,
                (total_message as f64) / end.duration_since(start).as_secs_f64()
            );
        });
    }

    #[test]
    fn bench_tx_future_rx_blocking_performance() {
        println!();
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let total_message = 1000000;
        let (tx_f, rx) = bounded_tx_future_rx_blocking::<i32>(100);
        let start = Instant::now();
        let th = thread::spawn(move || {
            for _i in 0..total_message {
                let _r = rx.recv();
                //                assert_eq!(r.unwrap(), i);
            }
            let end = Instant::now();
            println!(
                "{} message, single sender thread single receiver thread use mpmc {} /s",
                total_message,
                (total_message as f64) / end.duration_since(start).as_secs_f64()
            );
        });
        rt.block_on(async move {
            for i in 0i32..total_message {
                let _ = tx_f.send(i).await;
            }
        });
        let _ = th.join();
    }

    #[test]
    fn bench_tokio_mpsc_performance() {
        println!();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let total_message = 1000000;
            let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(100);
            let start = Instant::now();
            tokio::spawn(async move {
                println!("sender thread send {} message start", total_message);
                let mut _tx = tx.clone();
                for i in 0i32..total_message {
                    let _ = _tx.send(i).await;
                }
                println!("sender thread send {} message end", total_message);
            });

            println!("receiver thread recv {} message start", total_message);
            for _ in 0..total_message {
                rx.recv().await;
            }
            println!("receiver thread recv {} message end", total_message);
            let end = Instant::now();

            println!("{} message, single sender thread single receiver thread use tokio::sync::channel, {} /s",
                     total_message, (total_message as f64) / end.duration_since(start).as_secs_f64());
        });
    }

    #[test]
    fn test_mpmc_sender_close() {
        let (tx, rx) = bounded_tx_blocking_rx_future::<i32>(10);
        let total_msg_count = 5;
        for i in 0..total_msg_count {
            let _ = tx.send(i);
        }
        drop(tx);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let mut recv_msg_count = 0;
            loop {
                match rx.recv().await {
                    Ok(_) => {
                        recv_msg_count += 1;
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
            assert_eq!(recv_msg_count, total_msg_count);
        });
    }

    #[test]
    fn test_future_both_1_thread_single() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let (tx, rx) = bounded_future_both::<i32>(10);
            let rx_res = rx.try_recv();
            assert!(rx_res.is_err());
            assert!(rx_res.unwrap_err().is_empty());
            for i in 0i32..10 {
                let tx_res = tx.try_send(i);
                assert!(tx_res.is_ok());
            }
            let tx_res = tx.try_send(11);
            assert!(tx_res.is_err());
            assert!(tx_res.unwrap_err().is_full());

            let (noti_tx, noti_rx) = tokio::sync::oneshot::channel::<bool>();
            tokio::spawn(async move {
                for i in 0i32..12 {
                    match rx.recv().await {
                        Ok(j) => {
                            println!("recv {}", i);
                            assert_eq!(i, j);
                        }
                        Err(e) => {
                            panic!("error {}", e);
                        }
                    }
                }
                let res = rx.recv().await;
                assert!(res.is_err());
                println!("rx close");
                let _ = noti_tx.send(true);
            });
            assert!(tx.send(10).await.is_ok());
            tokio::time::sleep(Duration::from_secs(1)).await;
            assert!(tx.send(11).await.is_ok());
            drop(tx);
            let _ = noti_rx.await;
        });
    }

    #[test]
    fn test_tx_blocking_rx_future_1_thread_single() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .unwrap();
        rt.block_on(async move {
            let (tx, rx) = bounded_tx_blocking_rx_future::<i32>(10);
            let rx_res = rx.try_recv();
            assert!(rx_res.is_err());
            assert!(rx_res.unwrap_err().is_empty());
            for i in 0i32..10 {
                let tx_res = tx.send(i);
                assert!(tx_res.is_ok());
            }
            let tx_res = tx.try_send(11);
            assert!(tx_res.is_err());
            assert!(tx_res.unwrap_err().is_full());

            let (noti_tx, noti_rx) = tokio::sync::oneshot::channel::<bool>();
            tokio::spawn(async move {
                for i in 0i32..12 {
                    match rx.recv().await {
                        Ok(j) => {
                            println!("recv {}", i);
                            assert_eq!(i, j);
                        }
                        Err(e) => {
                            panic!("error {}", e);
                        }
                    }
                }
                let res = rx.recv().await;
                assert!(res.is_err());
                println!("rx close");
                let _ = noti_tx.send(true);
            });
            assert!(tx.send(10).is_ok());
            tokio::time::sleep(Duration::from_secs(1)).await;
            assert!(tx.send(11).is_ok());
            drop(tx);
            let _ = noti_rx.await;
        });
    }

    #[test]
    fn test_tx_future_rx_blocking_1_thread_single() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .unwrap();
        rt.block_on(async move {
            let (tx, rx) = bounded_tx_future_rx_blocking::<i32>(10);
            let rx_res = rx.try_recv();
            assert!(rx_res.is_err());
            assert!(rx_res.unwrap_err().is_empty());
            for i in 0i32..10 {
                let tx_res = tx.send(i).await;
                assert!(tx_res.is_ok());
            }
            let tx_res = tx.try_send(11);
            assert!(tx_res.is_err());
            assert!(tx_res.unwrap_err().is_full());

            let (noti_tx, noti_rx) = tokio::sync::oneshot::channel::<bool>();
            tokio::spawn(async move {
                for i in 0i32..5 {
                    assert!(tx.send(10 + i).await.is_ok());
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                println!("tx close");
                let _ = noti_tx.send(true);
            });
            for i in 0i32..15 {
                match rx.recv() {
                    Ok(j) => {
                        println!("recv {}", i);
                        assert_eq!(i, j);
                    }
                    Err(e) => {
                        panic!("error {}", e);
                    }
                }
            }
            let res = rx.recv();
            assert!(res.is_err());
            drop(rx);
            let _ = noti_rx.await;
        });
    }

    #[test]
    fn test_future_both_1_thread_multi_4tx_2rx() {
        _future_both_thread_multi(1, 4, 2);
    }

    #[test]
    fn test_future_both_2_thread_multi_4tx_2rx() {
        _future_both_thread_multi(2, 4, 2);
    }

    #[test]
    fn test_future_both_8_thread_multi_4tx_4rx() {
        _future_both_thread_multi(8, 4, 4);
    }

    fn _future_both_thread_multi(real_threads: usize, tx_count: usize, rx_count: usize) {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(real_threads)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let (tx, rx) = bounded_future_both::<i32>(10);
            let (noti_tx, mut noti_rx) = tokio::sync::mpsc::channel::<usize>(tx_count + rx_count);

            let counter = Arc::new(AtomicI32::new(0));
            let round = 100000;
            println!("");
            for _tx_i in 0..tx_count {
                let _tx = tx.clone();
                let mut _noti_tx = noti_tx.clone();
                let _round = round;
                tokio::spawn(async move {
                    for i in 0i32.._round {
                        match _tx.send(i).await {
                            Err(e) => panic!("{}", e),
                            _ => {}
                        }
                    }
                    let _ = _noti_tx.send(_tx_i).await;
                    println!("tx {} exit", _tx_i);
                });
            }
            for _rx_i in 0..rx_count {
                let _rx = rx.clone();
                let mut _noti_tx = noti_tx.clone();
                let _counter = counter.clone();
                tokio::spawn(async move {
                    'A: loop {
                        match _rx.recv().await {
                            Ok(_i) => {
                                _counter.as_ref().fetch_add(1i32, Ordering::SeqCst);
                                //print!("recv {} {}\r", _rx_i, i);
                            }
                            Err(_) => break 'A,
                        }
                    }
                    let _ = _noti_tx.send(_rx_i).await;
                    println!("rx {} exit", _rx_i);
                });
            }
            drop(tx);
            drop(rx);
            drop(noti_tx);
            for _ in 0..(rx_count + tx_count) {
                match noti_rx.recv().await {
                    Some(_) => {}
                    None => break,
                }
            }
            assert_eq!(counter.as_ref().load(Ordering::Relaxed), round * (tx_count as i32));
        });
    }

    fn _tx_blocking_rx_future_multi(real_threads: usize, tx_count: usize, rx_count: usize) {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(real_threads)
            .enable_all()
            .build()
            .unwrap();
        let (tx, rx) = bounded_tx_blocking_rx_future::<i32>(10);
        let counter = Arc::new(AtomicI32::new(0));
        let round = 100000;
        let mut tx_ths = Vec::new();
        let send_msg = Arc::new(AtomicI32::new(0));
        for _tx_i in 0..tx_count {
            let _tx = tx.clone();
            let _round = round;
            let _send_msg = send_msg.clone();
            tx_ths.push(thread::spawn(move || {
                loop {
                    let i = _send_msg.fetch_add(1, Ordering::SeqCst);
                    if i >= round {
                        break;
                    }
                    match _tx.send(i) {
                        Err(e) => panic!("{}", e),
                        _ => {
                            //println!("tx {} {}", _tx_i, i);
                        }
                    }
                }
                println!("tx {} exit", _tx_i);
            }));
        }
        drop(tx);
        rt.block_on(async move {
            let (noti_tx, mut noti_rx) = tokio::sync::mpsc::channel::<usize>(rx_count);
            for _rx_i in 0..rx_count {
                let _rx = rx.clone();
                let mut _noti_tx = noti_tx.clone();
                let _counter = counter.clone();
                tokio::spawn(async move {
                    'A: loop {
                        match _rx.recv().await {
                            Ok(_i) => {
                                _counter.as_ref().fetch_add(1i32, Ordering::SeqCst);
                                //println!("rx {} {}\r", _rx_i, _i);
                            }
                            Err(_) => break 'A,
                        }
                    }
                    println!("rx {} exiting", _rx_i);
                    let _ = _noti_tx.send(_rx_i).await;
                    println!("rx {} exit", _rx_i);
                });
            }
            drop(rx);
            drop(noti_tx);
            for _ in 0..(rx_count) {
                match noti_rx.recv().await {
                    Some(_) => {}
                    None => break,
                }
            }
            assert_eq!(counter.as_ref().load(Ordering::Relaxed), round as i32);
        });
        for th in tx_ths {
            let _ = th.join();
        }
    }

    #[test]
    fn test_tx_blocking_rx_future_1_thread_multi_4tx_2rx() {
        _tx_blocking_rx_future_multi(1, 4, 2);
    }

    #[test]
    fn test_tx_blocking_rx_future_2_thread_multi_4tx_3rx() {
        _tx_blocking_rx_future_multi(2, 4, 3);
    }

    #[test]
    fn test_tx_blocking_rx_future_8_thread_multi_4tx_4rx() {
        _tx_blocking_rx_future_multi(8, 4, 4);
    }

    fn _tx_future_rx_blocking_multi(real_threads: usize, tx_count: usize, rx_count: usize) {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(real_threads)
            .enable_all()
            .build()
            .unwrap();
        let (tx, rx) = bounded_tx_future_rx_blocking::<i32>(10);
        let counter = Arc::new(AtomicI32::new(0));
        let round = 100000;
        let mut rx_ths = Vec::new();
        for _rx_i in 0..rx_count {
            let _rx = rx.clone();
            let _round = round;
            let _counter = counter.clone();
            rx_ths.push(thread::spawn(move || {
                'A: loop {
                    match _rx.recv() {
                        Ok(_) => {
                            _counter.as_ref().fetch_add(1i32, Ordering::SeqCst);
                            //print!("{} {}\r", _rx_i, i);
                        }
                        Err(_) => break 'A,
                    }
                }
                println!("rx {} exit", _rx_i);
            }));
        }
        drop(rx);
        rt.block_on(async move {
            let (noti_tx, mut noti_rx) = tokio::sync::mpsc::channel::<usize>(tx_count);
            for _tx_i in 0..tx_count {
                let _tx = tx.clone();
                let mut _noti_tx = noti_tx.clone();
                tokio::spawn(async move {
                    for i in 0i32..round {
                        match _tx.send(i).await {
                            Err(e) => panic!("{}", e),
                            _ => {}
                        }
                    }
                    let _ = _noti_tx.send(_tx_i).await;
                    println!("tx {} exit", _tx_i);
                });
            }
            drop(tx);
            drop(noti_tx);
            for _ in 0..(tx_count) {
                match noti_rx.recv().await {
                    Some(_) => {}
                    None => break,
                }
            }
        });
        for th in rx_ths {
            let _ = th.join();
        }
        assert_eq!(counter.as_ref().load(Ordering::Relaxed), round * (tx_count as i32));
    }

    #[test]
    fn test_tx_future_rx_blocking_1_thread_multi_4tx_2rx() {
        _tx_future_rx_blocking_multi(1, 4, 2);
    }

    #[test]
    fn test_tx_future_rx_blocking_2_thread_multi_4tx_3rx() {
        _tx_future_rx_blocking_multi(2, 4, 3);
    }

    #[test]
    fn test_tx_future_rx_blocking_8_thread_multi_4tx_4rx() {
        _tx_future_rx_blocking_multi(8, 4, 4);
    }

    #[test]
    fn test_timeout_future_both() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let (tx, rx) = bounded_future_both::<i32>(100);
            assert!(timeout(Duration::from_secs(1), rx.recv()).await.is_err());
            let (tx_done, rx_done) = crossbeam::channel::bounded::<i32>(1);
            tokio::spawn(async move {
                match rx.recv().await {
                    Ok(item) => {
                        let _ = tx_done.send(item);
                    }
                    Err(_e) => {
                        println!("recv error");
                    }
                }
            });
            let _ = tx.send(1).await;
            assert_eq!(rx_done.recv().unwrap(), 1);
        });
    }
}
