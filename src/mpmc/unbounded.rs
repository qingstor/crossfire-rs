use super::rx::*;
use super::tx::*;
use crate::channel::*;
use crossbeam::queue::SegQueue;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::task::*;

pub type RxUnbounded<T> = RxFuture<T, UnboundedSharedFuture>;
pub type TxUnbounded<T> = TxBlocking<T, UnboundedSharedFuture>;

/// Initiate a unbounded channel.
/// Sender will never block, so we use the same TxBlocking for threads
pub fn unbounded_future<T: Unpin>() -> (TxUnbounded<T>, RxUnbounded<T>) {
    let (tx, rx) = crossbeam::channel::unbounded();
    let shared = Arc::new(UnboundedSharedFuture::new());

    let tx_f = TxBlocking::new(tx, shared.clone());
    let rx_f = RxFuture::new(rx, shared);
    (tx_f, rx_f)
}

pub struct UnboundedSharedFuture {
    tx_count: AtomicUsize,
    rx_count: AtomicUsize,
    recv_waker: SegQueue<LockedWakerRef>,
    recv_waker_tx_seq: AtomicU64,
    recv_waker_rx_seq: AtomicU64,
    checking_recv: AtomicBool,
}

impl MPMCShared for UnboundedSharedFuture {
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
        let _ = self.rx_count.fetch_sub(1, Ordering::SeqCst);
        return;
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

#[cfg(test)]
mod tests {

    extern crate tokio;
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::thread;
    use std::time::Instant;
    use tokio::time::Duration;

    #[test]
    fn bench_std_channel_performance() {
        println!();
        let total_message = 1000000;
        let (tx, rx) = std::sync::mpsc::channel();
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
        let (tx, rx) = crossbeam::channel::unbounded();
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
    fn bench_tokio_mpsc_performance() {
        println!();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let total_message = 1000000;
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<i32>();
            let start = Instant::now();
            tokio::spawn(async move {
                println!("sender thread send {} message start", total_message);
                let mut _tx = tx.clone();
                for i in 0i32..total_message {
                    let _ = _tx.send(i);
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
    fn bench_tx_blocking_rx_future_performance() {
        println!();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let total_message = 1000000;
        let (tx, rx_f) = unbounded_future::<i32>();
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

    fn _tx_blocking_rx_future_multi(real_threads: usize, tx_count: usize) {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(real_threads)
            .enable_all()
            .build()
            .unwrap();
        let (tx, rx) = unbounded_future::<i32>();
        let counter = Arc::new(AtomicI32::new(0));
        let round = 100000;
        let mut tx_ths = Vec::new();
        for _tx_i in 0..tx_count {
            let _tx = tx.clone();
            let _round = round;
            tx_ths.push(thread::spawn(move || {
                for i in 0i32.._round {
                    match _tx.send(i) {
                        Err(e) => panic!("{}", e),
                        _ => {}
                    }
                }
                println!("tx {} exit", _tx_i);
            }));
        }
        drop(tx);
        rt.block_on(async move {
            'A: loop {
                match rx.recv().await {
                    Ok(_) => {
                        counter.as_ref().fetch_add(1i32, Ordering::SeqCst);
                        //print!("{} {}\r", _rx_i, i);
                    }
                    Err(_) => break 'A,
                }
            }
            assert_eq!(counter.as_ref().load(Ordering::Acquire), round * (tx_count as i32));
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

    #[test]
    fn test_tx_unbounded_idle_select() {
        use futures::{pin_mut, select, FutureExt};
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let (_tx, rx_f) = unbounded_future::<i32>();

        async fn loop_fn() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        rt.block_on(async move {
            let mut c = rx_f.make_recv_future().fuse();
            for _ in 0..1000 {
                {
                    let f = loop_fn().fuse();
                    pin_mut!(f);
                    select! {
                        _ = f => {
                            let (_tx_wakers, _rx_wakers) = rx_f.get_waker_length();
                            //println!("waker tx{} rx {}", _tx_wakers, _rx_wakers);
                        },
                        _ = c => {
                            unreachable!()
                        },
                    }
                }
            }
            let (tx_wakers, rx_wakers) = rx_f.get_waker_length();
            assert_eq!(tx_wakers, 0);
            println!("waker rx {}", rx_wakers);
        });
    }
}
