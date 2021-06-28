use std::sync::{Arc, atomic::{AtomicUsize, AtomicU64, AtomicBool, Ordering}};
use crossbeam::queue::{ArrayQueue, SegQueue};
use super::tx::*;
use super::rx::*;
use crate::channel::*;

/// Initiate a bounded channel that sender and receiver are async
pub fn bounded_future_both<T: Unpin>(size: usize) -> (TxFuture<T, SharedFutureBoth>, RxFuture<T, SharedFutureBoth>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let shared = Arc::new(SharedFutureBoth::new());

    let tx_f = TxFuture::new(tx, shared.clone());
    let rx_f = RxFuture::new(rx, shared);
    (tx_f, rx_f)
}

/// Initiate a bounded channel that sender is async, receiver is blocking
pub fn bounded_tx_future_rx_blocking<T: Unpin>(size: usize) -> (TxFuture<T, SharedSenderFRecvB>, RxBlocking<T, SharedSenderFRecvB>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let shared = Arc::new(SharedSenderFRecvB::new());

    let tx_f = TxFuture::new(tx, shared.clone());
    let rx_b = RxBlocking::new(rx, shared);
    (tx_f, rx_b)
}

/// Initiate a bounded channel that sender is blocking, receiver is sync
pub fn bounded_tx_blocking_rx_future<T>(size: usize) -> (TxBlocking<T, SharedSenderBRecvF>, RxFuture<T, SharedSenderBRecvF>) {
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
    send_waker_tx_seq: AtomicU64,
    send_waker_rx_seq: AtomicU64,

    recv_waker: ArrayQueue<LockedWakerRef>,
    checking_sender: AtomicBool,
}

impl MPSCShared for SharedFutureBoth {
    #[inline]
    fn cancel_recv_reg(&self) {
        let _ = self.recv_waker.pop();
    }

    fn new() -> Self {
        Self{
            sender_waker: SegQueue::new(),
            recv_waker: ArrayQueue::new(1),
            tx_count: AtomicUsize::new(1),
            rx_count: AtomicUsize::new(1),
            checking_sender: AtomicBool::new(false),
            send_waker_tx_seq: AtomicU64::new(0),
            send_waker_rx_seq: AtomicU64::new(0),
        }
    }

    #[inline]
    fn on_recv(&self) {
        on_recv_m!(self)
    }

    #[inline]
    fn on_send(&self) {
        on_send_s!(self)
    }

    #[inline]
    fn reg_recv(&self) -> Option<LockedWaker> {
        reg_recv_s!(self)
    }

    #[inline]
    fn reg_send(&self) -> Option<LockedWaker> {
        reg_send_m!(self)
    }

    #[inline(always)]
    fn add_tx(&self) {
        let _ = self.tx_count.fetch_add(1, Ordering::SeqCst);
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
        (self.sender_waker.len(), 0)
    }

    #[inline]
    fn clear_send_wakers(&self, waker: LockedWaker) {
        clear_sender_wakers_common!(self, waker.get_seq())
    }
}

pub struct SharedSenderBRecvF {
    tx_count: AtomicUsize,
    rx_count: AtomicUsize,
    recv_waker: ArrayQueue<LockedWakerRef>,
}

impl MPSCShared for SharedSenderBRecvF {

    #[inline]
    fn cancel_recv_reg(&self) {
        let _ = self.recv_waker.pop();
    }

    fn new() -> Self {
        Self{
            recv_waker: ArrayQueue::new(1),
            tx_count: AtomicUsize::new(1),
            rx_count: AtomicUsize::new(1),
        }
    }

    #[inline]
    fn on_recv(&self) {
    }

    #[inline]
    fn on_send(&self) {
        on_send_s!(self)
    }

    #[inline]
    fn reg_recv(&self) -> Option<LockedWaker> {
        reg_recv_s!(self)
    }

    #[inline]
    fn reg_send(&self) -> Option<LockedWaker> {
        None
    }

    #[inline]
    fn add_tx(&self) {
        self.tx_count.fetch_add(1, Ordering::SeqCst);
    }

    #[inline]
    fn close_tx(&self) {
        close_tx_common!(self)
    }

    #[inline]
    fn close_rx(&self) {
        let _ = self.rx_count.fetch_sub(1, Ordering::SeqCst);
    }

    #[inline]
    fn get_tx_count(&self) -> usize {
        self.tx_count.load(Ordering::SeqCst)
    }

    fn get_waker_length(&self) -> (usize, usize) {
        (0, self.recv_waker.len())
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

impl MPSCShared for SharedSenderFRecvB {

    #[inline]
    fn cancel_recv_reg(&self) {
    }

    fn new() -> Self {
        Self{
            sender_waker: SegQueue::new(),
            rx_count: AtomicUsize::new(1),
            tx_count: AtomicUsize::new(1),
            checking_sender: AtomicBool::new(false),
            send_waker_tx_seq: AtomicU64::new(0),
            send_waker_rx_seq: AtomicU64::new(0),
        }
    }

    #[inline]
    fn on_recv(&self) {
        on_recv_m!(self)
    }

    #[inline]
    fn on_send(&self) {
    }

    #[inline]
    fn reg_recv(&self) -> Option<LockedWaker> {
        return None
    }

    #[inline]
    fn reg_send(&self) -> Option<LockedWaker> {
        reg_send_m!(self)
    }

    #[inline]
    fn add_tx(&self) {
        let _ = self.tx_count.fetch_add(1, Ordering::SeqCst);
    }

    #[inline]
    fn close_tx(&self) {
        let _ = self.tx_count.fetch_sub(1, Ordering::SeqCst);
    }

    #[inline]
    fn close_rx(&self) {
        if self.rx_count.fetch_sub(1, Ordering::SeqCst) > 1 {
            return;
        }
        // wake all tx, since no one will wake blocked fauture after that
        loop {
            match self.sender_waker.pop() {
                Ok(waker)=>{
                    waker.wake();
                },
                Err(_)=>return,
            }
        }
    }

    fn get_waker_length(&self) -> (usize, usize) {
        (self.sender_waker.len(), 0)
    }

    #[inline]
    fn get_tx_count(&self) -> usize {
        1
    }

    #[inline]
    fn clear_send_wakers(&self, waker: LockedWaker) {
        clear_sender_wakers_common!(self, waker.get_seq())
    }
}


#[cfg(test)]
mod tests {

    extern crate tokio;
    use super::*;
    use std::time::{Instant, Duration};
    use std::thread;
    use tokio::time::delay_for;
    use std::sync::atomic::{AtomicI32, Ordering};


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

        println!("{} message, single sender thread single receiver thread use crossbeam::channel, {} /s",
                 total_message, (total_message as f64) / end.duration_since(start).as_secs_f64());
    }


    #[test]
    fn bench_future_both_performance() {
        println!();
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(2).build().unwrap();
        rt.block_on(async move {
            let total_message = 1000000;
            let (tx, rx) = bounded_future_both::<i32>(100);
            let start = Instant::now();
            tokio::spawn(async move {
                println!("sender thread send {} message start", total_message);
                for i in 0i32..total_message {
                    let _ = tx.send(i).await;
                    // println!("send {}", i);
                }
                println!("sender thread send {} message end", total_message);
            });

            for _ in 0..total_message {
                if let Ok(_i) = rx.recv().await {
                    //println!("recv {}", _i);
                }
            }
            let end = Instant::now();

            println!("{} message, single sender thread single receiver thread use mpsc {} /s",
                     total_message, (total_message as f64) / end.duration_since(start).as_secs_f64());
        });
    }

    #[test]
    fn bench_tx_blocking_rx_future_performance() {
        println!();
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(1).build().unwrap();
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

            println!("{} message, single sender thread single receiver thread use mpsc {} /s",
                     total_message, (total_message as f64) / end.duration_since(start).as_secs_f64());
        });
    }

    #[test]
    fn bench_tx_future_rx_blocking_performance() {
        println!();
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(1).build().unwrap();
        let total_message = 1000000;
        let (tx_f, rx) = bounded_tx_future_rx_blocking::<i32>(100);
        let start = Instant::now();
        let th = thread::spawn(move || {
            for _i in 0..total_message {
                let _r = rx.recv();
//                assert_eq!(r.unwrap(), i);
            }
            let end = Instant::now();
            println!("{} message, single sender thread single receiver thread use mpsc {} /s",
                     total_message, (total_message as f64) / end.duration_since(start).as_secs_f64());
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
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(2).build().unwrap();
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
    fn test_future_both_1_thread_single() {
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(1).build().unwrap();
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
                for i in 0i32..12  {
                    match rx.recv().await {
                        Ok(j)=>{
                            println!("recv {}", i);
                            assert_eq!(i, j);
                        },
                        Err(e)=>{
                            panic!("error {}", e);
                        },
                    }
                }
                let res = rx.recv().await;
                assert!(res.is_err());
                println!("rx close");
                let _ = noti_tx.send(true);
            });
            assert!(tx.send(10).await.is_ok());
            delay_for(Duration::from_secs(1)).await;
            assert!(tx.send(11).await.is_ok());
            drop(tx);
            let _ = noti_rx.await;
        });
    }

    #[test]
    fn test_tx_blocking_rx_future_background() {
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(1).build().unwrap();

        let (tx, rx) = bounded_tx_blocking_rx_future::<i32>(1);
        thread::spawn(move || {
            rt.spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(i)=>println!("recv {}", i),
                        Err(_e)=>{ println!("channel closed"); return; },
                    }
                }
            });
            rt.block_on(async move {
                loop {
                    tokio::time::delay_for(Duration::from_secs(1)).await;
                }
            })
        });
        for i in 0i32..10 {
            let tx_res = tx.send(i);
            match tx_res {
                Ok(_)=>{
                    println!("send {}", i);
                },
                Err(_)=>{
                    println!("Channel close on recv side when sending {}", i);
                    break;
                },
            }
        }
    }

    #[test]
    fn test_tx_blocking_rx_future_1_thread_single() {
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(1).build().unwrap();
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
                for i in 0i32..12  {
                    match rx.recv().await {
                        Ok(j)=>{
                            println!("recv {}", i);
                            assert_eq!(i, j);
                        },
                        Err(e)=>{
                            panic!("error {}", e);
                        },
                    }
                }
                let res = rx.recv().await;
                assert!(res.is_err());
                println!("rx close");
                let _ = noti_tx.send(true);
            });
            assert!(tx.send(10).is_ok());
            delay_for(Duration::from_secs(1)).await;
            assert!(tx.send(11).is_ok());
            drop(tx);
            let _ = noti_rx.await;
        });
    }


    #[test]
    fn test_tx_future_rx_blocking_1_thread_single() {
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(1).build().unwrap();
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
                    assert!(tx.send(10+i).await.is_ok());
                    delay_for(Duration::from_secs(1)).await;
                }
                println!("tx close");
                let _ = noti_tx.send(true);
            });
            for i in 0i32..15  {
                match rx.recv() {
                    Ok(j)=>{
                        println!("recv {}", i);
                        assert_eq!(i, j);
                    },
                    Err(e)=>{
                        panic!("error {}", e);
                    },
                }
            }
            let res = rx.recv();
            assert!(res.is_err());
            drop(rx);
            let _ = noti_rx.await;
        });
    }

    #[test]
    fn test_future_both_1_thread_multi_4tx() {
        _future_both_thread_multi(1, 4);
    }

    #[test]
    fn test_future_both_2_thread_multi_4tx() {
        _future_both_thread_multi(2, 4);
    }

    #[test]
    fn test_future_both_8_thread_multi_4tx() {
        _future_both_thread_multi(8, 4);
    }

    fn _future_both_thread_multi(real_threads: usize, tx_count: usize) {
        let rx_count = 1usize;
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(real_threads).build().unwrap();
        rt.block_on(async move {
            let (tx, rx) = bounded_future_both::<i32>(10);
            let (noti_tx, mut noti_rx) = tokio::sync::mpsc::channel::<usize>(tx_count + rx_count);

            let counter = Arc::new(AtomicI32::new(0));
            let round = 100000;
            for _tx_i in 0..tx_count {
                let _tx = tx.clone();
                let mut _noti_tx = noti_tx.clone();
                let _round = round;
                tokio::spawn(async move {
                    for i in 0i32.._round  {
                        match _tx.send(i).await {
                            Err(e)=>panic!("{}", e),
                            _=>{},
                        }
                    }
                    let _ = _noti_tx.send(_tx_i).await;
                    println!("tx {} exit", _tx_i);
                });
            }
            let mut _noti_tx = noti_tx.clone();
            let _counter = counter.clone();
            drop(tx);

            'A: loop {
                match rx.recv().await {
                    Ok(_) =>{
                        _counter.as_ref().fetch_add(1i32, Ordering::SeqCst);
                        //print!("{}\n",  i);
                    },
                    Err(_)=>break 'A,
                }
            }
            println!("recv done");
            drop(noti_tx);
            for _ in 0..tx_count {
                match noti_rx.recv().await {
                    Some(_)=>{},
                    None=>break,
                }
            }
            assert_eq!(counter.as_ref().load(Ordering::Relaxed), round * (tx_count as i32));
        });
    }

    fn _tx_blocking_rx_future_multi(real_threads: usize, tx_count: usize) {
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(real_threads).build().unwrap();
        let (tx, rx) = bounded_tx_blocking_rx_future::<i32>(10);
        let counter = Arc::new(AtomicI32::new(0));
        let round = 100000;
        let mut tx_ths = Vec::new();
        for _tx_i in 0..tx_count {
            let _tx = tx.clone();
            let _round = round;
            tx_ths.push(thread::spawn(move || {
                for i in 0i32.._round  {
                    match _tx.send(i) {
                        Err(e)=>panic!("{}", e),
                        _=>{},
                    }
                }
                println!("tx {} exit", _tx_i);
            }));
        }
        drop(tx);
        rt.block_on(async move {

            'A: loop {
                match rx.recv().await {
                    Ok(_) =>{
                        counter.as_ref().fetch_add(1i32, Ordering::SeqCst);
                        //print!("{} {}\r", _rx_i, i);
                    },
                    Err(_)=>break 'A,
                }
            }
            assert_eq!(counter.as_ref().load(Ordering::Relaxed), round * (tx_count as i32));
        });
        for th in tx_ths {
            let _ = th.join();
        }
    }

    #[test]
    fn test_tx_blocking_rx_future_1_thread_multi_4tx() {
        _tx_blocking_rx_future_multi(1, 4);
    }

    #[test]
    fn test_tx_blocking_rx_future_2_thread_multi_4tx() {
        _tx_blocking_rx_future_multi(2, 4);
    }

    #[test]
    fn test_tx_blocking_rx_future_8_thread_multi_4tx() {
        _tx_blocking_rx_future_multi(8, 4);
    }

    fn _tx_future_rx_blocking_multi(real_threads: usize, tx_count: usize) {
        let mut rt = tokio::runtime::Builder::new().threaded_scheduler().enable_all().core_threads(real_threads).build().unwrap();
        let (tx, rx) = bounded_tx_future_rx_blocking::<i32>(10);
        let counter = Arc::new(AtomicI32::new(0));
        let round = 100000;
        let mut rx_ths = Vec::new();
        let _counter = counter.clone();
        rx_ths.push(thread::spawn(move || {
            'A: loop {
                match rx.recv() {
                    Ok(_) =>{
                        _counter.as_ref().fetch_add(1i32, Ordering::SeqCst);
                        //print!("{} {}\r", _rx_i, i);
                    },
                    Err(_)=>break 'A,
                }
            }
            println!("rx exit");
        }));
        rt.block_on(async move {

            let (noti_tx, mut noti_rx) = tokio::sync::mpsc::channel::<usize>(tx_count);
            for _tx_i in 0..tx_count {
                let _tx = tx.clone();
                let mut _noti_tx = noti_tx.clone();
                tokio::spawn(async move {
                    for i in 0i32..round  {
                        match _tx.send(i).await {
                            Err(e)=>panic!("{}", e),
                            _=>{},
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
                    Some(_)=>{},
                    None=>break,
                }
            }
        });
        for th in rx_ths {
            let _ = th.join();
        }
        assert_eq!(counter.as_ref().load(Ordering::Relaxed), round * (tx_count as i32));
    }

    #[test]
    fn test_tx_future_rx_blocking_1_thread_multi_4tx() {
        _tx_future_rx_blocking_multi(1, 4);
    }

    #[test]
    fn test_tx_future_rx_blocking_2_thread_multi_4tx() {
        _tx_future_rx_blocking_multi(2, 4);
    }

    #[test]
    fn test_tx_future_rx_blocking_8_thread_multi_4tx() {
        _tx_future_rx_blocking_multi(4, 4);
    }
}
