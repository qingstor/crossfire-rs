use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use crossbeam::queue::ArrayQueue;
use std::task::*;
use super::tx::*;
use super::rx::*;
use crate::channel::*;

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
    recv_waker: ArrayQueue<LockedWakerRef>,
}

impl MPSCShared for UnboundedSharedFuture {
    #[inline]
    fn cancel_recv_reg(&self) {
        let _ = self.recv_waker.pop();
    }

    fn new() -> Self {
        Self{
            recv_waker: ArrayQueue::new(1),
            tx_count: AtomicUsize::new(1),
        }
    }

    #[inline]
    fn on_recv(&self) {
    }

    #[inline]
    fn on_send(&self) {
        on_send_s!(self)
    }

    fn reg_recv(&self, ctx: &mut Context) -> Option<LockedWaker> {
        reg_recv_s!(self, ctx)
    }

    #[inline]
    fn reg_send(&self, _ctx: &mut Context) -> Option<LockedWaker> {
        None
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
    }

    #[inline]
    fn get_tx_count(&self) -> usize {
        self.tx_count.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {

    extern crate tokio;
    use super::*;
    use std::time::Instant;
    use std::thread;
    use std::sync::atomic::{AtomicI32, Ordering};

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

        println!("{} message, single sender thread single receiver thread use crossbeam::channel, {} /s",
                 total_message, (total_message as f64) / end.duration_since(start).as_secs_f64());
    }

    #[test]
    fn bench_tokio_mpsc_performance() {
        println!();
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
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

            println!("{} message, single sender thread single receiver thread use mpmc {} /s",
                     total_message, (total_message as f64) / end.duration_since(start).as_secs_f64());
        });
    }

    fn _tx_blocking_rx_future_multi(real_threads: usize, tx_count: usize) {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(real_threads).enable_all().build().unwrap();
        let (tx, rx) = unbounded_future::<i32>();
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
}
