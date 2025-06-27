use super::common::*;
use crate::*;
use log::*;
use rstest::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

#[rstest]
#[case(spsc::bounded_tx_async_rx_blocking::<usize>(100))]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(100))]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(100))]
#[tokio::test]
async fn test_basic_1_tx_async_1_rx_blocking<T: AsyncTxTrait<usize>, R: BlockingRxTrait<usize>>(
    setup_log: (), #[case] channel: (T, R),
) {
    let _ = setup_log; // Disable unused var warning
    let (tx, rx) = channel;

    let rx_res = rx.try_recv();
    assert!(rx_res.is_err());
    assert!(rx_res.unwrap_err().is_empty());
    let batch_1: usize = 100;
    let batch_2: usize = 200;
    let rt = get_runtime();
    rt.spawn(async move {
        for i in 0..batch_1 {
            let tx_res = tx.send(i).await;
            assert!(tx_res.is_ok());
        }
        for i in batch_1..(batch_1 + batch_2) {
            assert!(tx.send(10 + i).await.is_ok());
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });
    for _ in 0..(batch_1 + batch_2) {
        match rx.recv() {
            Ok(i) => {
                debug!("recv {}", i);
            }
            Err(e) => {
                panic!("error {}", e);
            }
        }
    }
    let res = rx.recv();
    assert!(res.is_err());
    rt.shutdown_background(); // Prevent panic on runtime drop
}

#[rstest]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(10), 8)]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(10), 100)]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(10), 1000)]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(10), 8)]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(10), 100)]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(10), 1000)]
fn test_basic_multi_tx_async_1_rx_blocking<R: BlockingRxTrait<usize>>(
    setup_log: (), #[case] channel: (MAsyncTx<usize>, R), #[case] tx_count: usize,
) {
    let _ = setup_log; // Disable unused var warning
    let (tx, rx) = channel;

    let rx_res = rx.try_recv();
    assert!(rx_res.is_err());
    assert!(rx_res.unwrap_err().is_empty());
    let batch_1: usize = 100;
    let batch_2: usize = 200;
    let rt = get_runtime();
    let (noti_tx, mut noti_rx) = tokio::sync::mpsc::channel::<usize>(tx_count);
    for tx_i in 0..tx_count {
        let _tx = tx.clone();
        let _noti_tx = noti_tx.clone();
        rt.spawn(async move {
            for i in 0..batch_1 {
                let tx_res = _tx.send(i).await;
                assert!(tx_res.is_ok());
            }
            for i in batch_1..(batch_1 + batch_2) {
                assert!(_tx.send(10 + i).await.is_ok());
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            _noti_tx.send(tx_i).await.expect("noti send ok");
        });
    }
    drop(tx);
    for _ in 0..((batch_1 + batch_2) * tx_count) {
        match rx.recv() {
            Ok(i) => {
                debug!("recv {}", i);
            }
            Err(e) => {
                panic!("error {}", e);
            }
        }
    }
    let res = rx.recv();
    assert!(res.is_err());
    // Wait for tokio spawn exit
    rt.block_on(async move {
        for _ in 0..tx_count {
            let _ = noti_rx.recv().await;
        }
    });
}

#[rstest]
#[case(spsc::bounded_tx_async_rx_blocking::<usize>(1))]
#[case(spsc::bounded_tx_async_rx_blocking::<usize>(10))]
#[case(spsc::bounded_tx_async_rx_blocking::<usize>(100))]
#[case(spsc::bounded_tx_async_rx_blocking::<usize>(1000))]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(1))]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(10))]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(100))]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(1000))]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(1))]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(10))]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(100))]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(1000))]
fn test_pressure_1_tx_async_1_rx_blocking<T: AsyncTxTrait<usize>, R: BlockingRxTrait<usize>>(
    setup_log: (), #[case] channel: (T, R),
) {
    let _ = setup_log; // Disable unused var warning
    let (tx, rx) = channel;

    let counter = Arc::new(AtomicUsize::new(0));
    let round: usize = 1000000;
    let _round = round;
    let _counter = counter.clone();
    let th = thread::spawn(move || {
        'A: loop {
            match rx.recv() {
                Ok(_) => {
                    _counter.as_ref().fetch_add(1, Ordering::SeqCst);
                    //debug!("{} {}\r", _rx_i, i);
                }
                Err(_) => break 'A,
            }
        }
        debug!("rx exit");
    });
    let rt = get_runtime();
    rt.block_on(async move {
        for i in 0..round {
            match tx.send(i).await {
                Err(e) => panic!("{}", e),
                _ => {}
            }
        }
        debug!("tx exit");
    });
    let _ = th.join();
    assert_eq!(counter.as_ref().load(Ordering::Acquire), round);
}

#[rstest]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(10), 8)]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(10), 10)]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(10), 100)]
#[case(mpsc::bounded_tx_async_rx_blocking::<usize>(100), 200)]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(10), 8)]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(10), 100)]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(10), 10)]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(100), 200)]
fn test_pressure_multi_tx_async_1_rx_blocking<R: BlockingRxTrait<usize>>(
    setup_log: (), #[case] channel: (MAsyncTx<usize>, R), #[case] tx_count: usize,
) {
    let _ = setup_log; // Disable unused var warning
    let (tx, rx) = channel;

    let counter = Arc::new(AtomicUsize::new(0));
    let round: usize = 100000;
    let _round = round;
    let _counter = counter.clone();
    let th = thread::spawn(move || {
        'A: loop {
            match rx.recv() {
                Ok(_) => {
                    _counter.as_ref().fetch_add(1, Ordering::SeqCst);
                    //debug!("{} {}\r", _rx_i, i);
                }
                Err(_) => break 'A,
            }
        }
        debug!("rx exit");
    });
    let rt = get_runtime();
    rt.block_on(async move {
        let (noti_tx, mut noti_rx) = tokio::sync::mpsc::channel::<usize>(tx_count);
        for _tx_i in 0..tx_count {
            let _tx = tx.clone();
            let mut _noti_tx = noti_tx.clone();
            tokio::spawn(async move {
                for i in 0..round {
                    match _tx.send(i).await {
                        Err(e) => panic!("{}", e),
                        _ => {}
                    }
                }
                let _ = _noti_tx.send(_tx_i).await;
                debug!("tx {} exit", _tx_i);
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
    let _ = th.join();
    assert_eq!(counter.as_ref().load(Ordering::Acquire), round * (tx_count));
}

#[rstest]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(10), 8, 8)]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(10), 100, 100)]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(10), 10, 1000)]
#[case(mpmc::bounded_tx_async_rx_blocking::<usize>(100), 500, 500)]
fn test_pressure_multi_tx_async_multi_rx_blocking(
    setup_log: (), #[case] channel: (MAsyncTx<usize>, MRx<usize>), #[case] tx_count: usize,
    #[case] rx_count: usize,
) {
    let _ = setup_log; // Disable unused var warning
    let (tx, rx) = channel;

    let counter = Arc::new(AtomicUsize::new(0));
    let round: usize = 100000;
    let mut rx_th_s = Vec::new();
    for _rx_i in 0..rx_count {
        let _rx = rx.clone();
        let _round = round;
        let _counter = counter.clone();
        rx_th_s.push(thread::spawn(move || {
            'A: loop {
                match _rx.recv() {
                    Ok(_) => {
                        _counter.as_ref().fetch_add(1, Ordering::SeqCst);
                        //debug!("{} {}\r", _rx_i, i);
                    }
                    Err(_) => break 'A,
                }
            }
            debug!("rx {} exit", _rx_i);
        }));
    }
    drop(rx);
    let rt = get_runtime();
    rt.block_on(async move {
        let (noti_tx, mut noti_rx) = tokio::sync::mpsc::channel::<usize>(tx_count);
        for _tx_i in 0..tx_count {
            let _tx = tx.clone();
            let mut _noti_tx = noti_tx.clone();
            tokio::spawn(async move {
                for i in 0..round {
                    match _tx.send(i).await {
                        Err(e) => panic!("{}", e),
                        _ => {}
                    }
                }
                let _ = _noti_tx.send(_tx_i).await;
                debug!("tx {} exit", _tx_i);
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
    for th in rx_th_s {
        let _ = th.join();
    }
    assert_eq!(counter.as_ref().load(Ordering::Acquire), round * (tx_count));
}
