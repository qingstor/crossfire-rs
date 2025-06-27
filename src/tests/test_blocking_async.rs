use super::common::*;
use crate::*;
use log::*;
use rstest::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;
use std::time::*;

#[rstest]
#[case(spsc::bounded_tx_blocking_rx_async::<usize>(10))]
#[case(mpsc::bounded_tx_blocking_rx_async::<usize>(10))]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(10))]
#[tokio::test]
async fn test_basic_1_tx_blocking_1_rx_async<T: BlockingTxTrait<usize>, R: AsyncRxTrait<usize>>(
    setup_log: (), #[case] channel: (T, R),
) {
    let _ = setup_log; // Disable unused var warning
    let (tx, rx) = channel;
    let rx_res = rx.try_recv();
    assert!(rx_res.is_err());
    assert!(rx_res.unwrap_err().is_empty());
    for i in 0usize..10 {
        let tx_res = tx.send(i);
        assert!(tx_res.is_ok());
    }
    let tx_res = tx.try_send(11);
    assert!(tx_res.is_err());
    assert!(tx_res.unwrap_err().is_full());

    let th = thread::spawn(move || {
        assert!(tx.send(10).is_ok());
        std::thread::sleep(Duration::from_secs(1));
        assert!(tx.send(11).is_ok());
    });
    for i in 0usize..12 {
        match rx.recv().await {
            Ok(j) => {
                debug!("recv {}", i);
                assert_eq!(i, j);
            }
            Err(e) => {
                panic!("error {}", e);
            }
        }
    }
    let res = rx.recv().await;
    assert!(res.is_err());
    debug!("rx close");
    let _ = th.join();
}

#[rstest]
#[case(spsc::bounded_tx_blocking_rx_async::<usize>(1))]
#[case(mpsc::bounded_tx_blocking_rx_async::<usize>(1))]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(1))]
#[case(spsc::bounded_tx_blocking_rx_async::<usize>(100))]
#[case(mpsc::bounded_tx_blocking_rx_async::<usize>(100))]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(100))]
#[case(spsc::unbounded_async::<usize>())]
#[case(mpsc::unbounded_async::<usize>())]
#[case(mpmc::unbounded_async::<usize>())]
#[tokio::test]
async fn test_presure_1_tx_blocking_1_rx_async<
    T: BlockingTxTrait<usize>,
    R: AsyncRxTrait<usize>,
>(
    setup_log: (), #[case] channel: (T, R),
) {
    let _ = setup_log; // Disable unused var warning
    let (tx, rx) = channel;
    let round: usize = 100000;
    let th = thread::spawn(move || {
        for i in 0..round {
            tx.send(i).expect("send ok");
        }
    });
    for i in 0..round {
        match rx.recv().await {
            Ok(msg) => {
                //debug!("recv {}", msg);
                assert_eq!(msg, i);
            }
            Err(_e) => {
                panic!("channel closed");
            }
        }
    }
    assert!(rx.recv().await.is_err());
    let _ = th.join();
}

#[rstest]
#[case(mpsc::bounded_tx_blocking_rx_async::<usize>(1), 10)]
#[case(mpsc::bounded_tx_blocking_rx_async::<usize>(1), 100)]
#[case(mpsc::bounded_tx_blocking_rx_async::<usize>(1), 1000)]
#[case(mpsc::bounded_tx_blocking_rx_async::<usize>(100), 10)]
#[case(mpsc::bounded_tx_blocking_rx_async::<usize>(100), 100)]
#[case(mpsc::bounded_tx_blocking_rx_async::<usize>(100), 1000)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(1), 10)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(1), 100)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(1), 1000)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(100), 10)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(100), 100)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(100), 1000)]
#[case(mpsc::unbounded_async::<usize>(), 10)]
#[case(mpsc::unbounded_async::<usize>(), 100)]
#[case(mpsc::unbounded_async::<usize>(), 1000)]
#[case(mpmc::unbounded_async::<usize>(), 10)]
#[case(mpmc::unbounded_async::<usize>(), 100)]
#[case(mpmc::unbounded_async::<usize>(), 1000)]
#[tokio::test]
async fn test_presure_tx_multi_blocking_1_rx_async<R: AsyncRxTrait<usize>>(
    setup_log: (), #[case] channel: (MTx<usize>, R), #[case] tx_count: usize,
) {
    let _ = setup_log; // Disable unused var warning
    let (tx, rx) = channel;
    let counter = Arc::new(AtomicUsize::new(0));
    let round = 1000000;
    let mut tx_ths = Vec::new();
    let send_msg = Arc::new(AtomicUsize::new(0));
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
                        //debug!("tx {} {}", _tx_i, i);
                    }
                }
            }
            info!("tx {} exit", _tx_i);
        }));
    }
    drop(tx);
    let _counter = counter.clone();
    'A: loop {
        match rx.recv().await {
            Ok(_i) => {
                _counter.as_ref().fetch_add(1, Ordering::SeqCst);
                //debug!("rx {} {}\r", _rx_i, _i);
            }
            Err(_) => break 'A,
        }
    }
    assert_eq!(counter.as_ref().load(Ordering::Acquire), round);
    for th in tx_ths {
        let _ = th.join();
    }
}

#[rstest]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(1), 10, 10)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(1), 100, 20)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(1), 1000, 200)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(10), 10, 10)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(10), 100, 20)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(10), 1000, 200)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(100), 10, 200)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(100), 100, 200)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(100), 300, 500)]
#[case(mpmc::bounded_tx_blocking_rx_async::<usize>(100), 30, 1000)]
#[case(mpmc::unbounded_async::<usize>(), 10, 10)]
#[case(mpmc::unbounded_async::<usize>(), 100, 20)]
#[case(mpmc::unbounded_async::<usize>(), 1000, 200)]
#[case(mpmc::unbounded_async::<usize>(), 10, 200)]
#[case(mpmc::unbounded_async::<usize>(), 100, 200)]
#[case(mpmc::unbounded_async::<usize>(), 300, 500)]
#[case(mpmc::unbounded_async::<usize>(), 30, 1000)]
#[tokio::test]
async fn test_presure_tx_multi_blocking_multi_rx_async(
    setup_log: (), #[case] channel: (MTx<usize>, MAsyncRx<usize>), #[case] tx_count: usize,
    #[case] rx_count: usize,
) {
    let _ = setup_log; // Disable unused var warning
    let (tx, rx) = channel;

    let counter = Arc::new(AtomicUsize::new(0));
    let round = 1000000;
    let mut tx_ths = Vec::new();
    let send_msg = Arc::new(AtomicUsize::new(0));
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
                        //debug!("tx {} {}", _tx_i, i);
                    }
                }
            }
            debug!("tx {} exit", _tx_i);
        }));
    }
    drop(tx);
    let (noti_tx, mut noti_rx) = tokio::sync::mpsc::channel::<usize>(rx_count);
    for _rx_i in 0..rx_count {
        let _rx = rx.clone();
        let mut _noti_tx = noti_tx.clone();
        let _counter = counter.clone();
        tokio::spawn(async move {
            'A: loop {
                match _rx.recv().await {
                    Ok(_i) => {
                        _counter.as_ref().fetch_add(1, Ordering::SeqCst);
                        //debug!("rx {} {}\r", _rx_i, _i);
                    }
                    Err(_) => break 'A,
                }
            }
            debug!("rx {} exiting", _rx_i);
            let _ = _noti_tx.send(_rx_i).await;
            debug!("rx {} exit", _rx_i);
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
    assert_eq!(counter.as_ref().load(Ordering::Acquire), round);
    for th in tx_ths {
        let _ = th.join();
    }
}
