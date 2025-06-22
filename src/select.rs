use crate::channel::*;
use crossbeam::channel::RecvError;
use std::cell::UnsafeCell;
use std::future::Future;
use std::mem::transmute;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

/// A selector for receiving the same type of message from different channel
///
/// Usage:
///
/// ```rust
///
///  use crossfire::mpmc;
///  use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
///  use tokio::time::{sleep, Duration};
///  use crossfire::channel::SelectSame;
///
///  let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
///  rt.block_on(async move {
///      let (tx1, rx1) = mpmc::bounded_future_both::<i32>(3);
///      let (tx2, rx2) = mpmc::bounded_future_both::<i32>(2);
///
///      let recv_count = Arc::new(AtomicUsize::new(0));
///      let mut ths = Vec::new();
///      for _j in 0..2 {
///          let _rx1 = rx1.clone();
///          let _rx2 = rx2.clone();
///          let count = recv_count.clone();
///          ths.push(tokio::task::spawn(async move {
///              let sel = SelectSame::new();
///              let _op1 = sel.add_recv(_rx1);
///              let op2 = sel.add_recv(_rx2);
///              loop {
///                  match sel.select().await {
///                      Ok(_i)=>{
///                          count.fetch_add(1, Ordering::SeqCst);
///                      },
///                      Err(_e)=>break,
///                  }
///                  if let Some(_i) = sel.try_recv(op2) {
///                      count.fetch_add(1, Ordering::SeqCst);
///                  }
///              }
///              println!("rx done");
///          }));
///      }
///      tokio::spawn(async move {
///          for i in 0..1000i32 {
///              let _ = tx1.send(i).await;
///              sleep(Duration::from_millis(1)).await;
///          }
///          for i in 1500..2500i32 {
///              let _ = tx1.send(i).await;
///          }
///      });
///      tokio::spawn(async move {
///          for i in 1000..1010i32 {
///              let _ = tx2.send(i).await;
///          }
///      });
///      for th in ths {
///          let _ = th.await;
///      }
///      assert_eq!(recv_count.load(Ordering::Acquire), 2000 + 10);
///  });
///
/// ```

pub struct SelectSame<T>
where
    T: Sync + Send + Unpin + 'static,
{
    size: AtomicUsize,
    left_handles: AtomicUsize,
    handles: UnsafeCell<Vec<Option<Box<dyn AsyncRx<T>>>>>,
    wakers: UnsafeCell<Vec<Option<LockedWaker>>>,
    last_index: AtomicUsize,
    has_waker: AtomicUsize,
    phan: std::marker::PhantomData<T>,
}

unsafe impl<T> Send for SelectSame<T> where T: Sync + Send + Unpin + 'static {}

unsafe impl<T> Sync for SelectSame<T> where T: Sync + Send + Unpin + 'static {}

macro_rules! try_recv_poll {
    ($self: expr, $index: expr, $rx: expr, $ctx: expr, $wakers: expr) => {{
        let has_waker = $wakers[$index].is_some();
        let r = $rx.poll_item($ctx, &mut $wakers[$index]);
        match r {
            Ok(r) => {
                $self.last_index.store($index, Ordering::Release);
                if $wakers[$index].is_none() && has_waker {
                    $self.has_waker.fetch_sub(1, Ordering::SeqCst);
                }
                return Poll::Ready(Ok(r));
            }
            Err(e) => {
                if e.is_empty() {
                    if $wakers[$index].is_some() && !has_waker {
                        $self.has_waker.fetch_add(1, Ordering::SeqCst);
                    }
                } else {
                    $self.close_handler($index);
                }
            }
        }
    }};
}

impl<T> SelectSame<T>
where
    T: Sync + Send + Unpin + 'static,
{
    pub fn new() -> Self {
        Self {
            size: AtomicUsize::new(0),
            handles: UnsafeCell::new(Vec::new()),
            wakers: UnsafeCell::new(Vec::new()),
            has_waker: AtomicUsize::new(0),
            last_index: AtomicUsize::new(0),
            left_handles: AtomicUsize::new(0),
            phan: Default::default(),
        }
    }

    #[inline(always)]
    fn get_handles(&self) -> &mut Vec<Option<Box<dyn AsyncRx<T>>>> {
        unsafe { transmute(self.handles.get()) }
    }

    #[inline(always)]
    fn get_wakers(&self) -> &mut Vec<Option<LockedWaker>> {
        unsafe { transmute(self.wakers.get()) }
    }

    pub fn add_recv<Rx: AsyncRx<T> + 'static>(&self, rx: Rx) -> usize {
        let op = self.size.fetch_add(1, Ordering::SeqCst);
        self.get_handles().push(Some(Box::new(rx)));
        self.get_wakers().push(None);
        self.left_handles.fetch_add(1, Ordering::SeqCst);
        op
    }

    fn close_handler(&self, index: usize) {
        self.get_handles()[index] = None;
        self.left_handles.fetch_sub(1, Ordering::SeqCst);
        let _ = self.last_index.compare_exchange_weak(index, 0, Ordering::SeqCst, Ordering::SeqCst);
        let wakers = self.get_wakers();
        if wakers[index].is_some() {
            wakers[index] = None;
            self.has_waker.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[inline]
    pub fn try_recv(&self, index: usize) -> Option<T> {
        if let Some(rx) = &self.get_handles()[index] {
            debug_assert!(self.get_wakers()[index].is_none());
            match rx.try_recv() {
                Ok(r) => {
                    self.last_index.store(index, Ordering::SeqCst);
                    return Some(r);
                }
                Err(e) => {
                    if !e.is_empty() {
                        self.close_handler(index);
                    }
                }
            }
        }
        return None;
    }

    #[inline(always)]
    fn cleanup_wakers(&self) {
        let wakers = self.get_wakers();
        let handles = self.get_handles();
        for i in 0..wakers.len() {
            if let Some(w) = wakers[i].take() {
                if w.abandon() {
                    // We are waked, but abandoning, should notify another receiver
                    if let Some(h) = &handles[i] {
                        if !h.is_empty() {
                            h.on_send();
                        }
                    }
                } else {
                    if let Some(h) = &handles[i] {
                        h.clear_recv_wakers(w);
                    }
                }
            }
        }
        self.has_waker.store(0, Ordering::Release);
    }

    pub async fn select(&self) -> Result<T, RecvError> {
        let r = SelectSameFuture { selector: self }.await;
        self.cleanup_wakers();
        return r;
    }
}

struct SelectSameFuture<'a, T>
where
    T: Sync + Send + Unpin + 'static,
{
    selector: &'a SelectSame<T>,
}

impl<'a, T> Future for SelectSameFuture<'a, T>
where
    T: Sync + Send + Unpin + 'static,
{
    type Output = Result<T, RecvError>;

    #[inline]
    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let _self = &self.selector;
        let handles = _self.get_handles();
        let wakers = _self.get_wakers();
        if _self.left_handles.load(Ordering::Acquire) == 0 {
            return Poll::Ready(Err(RecvError {}));
        }
        let mut polled_last_index: Option<usize> = None;
        let mut polled_waked: Option<usize> = None;
        // Only Try waker is waked if waker exists
        if _self.has_waker.load(Ordering::Acquire) > 0 {
            {
                for i in 0..handles.len() {
                    let _rx = &handles[i];
                    if let Some(waker) = &wakers[i] {
                        if waker.is_waked() {
                            if let Some(rx) = _rx {
                                try_recv_poll!(_self, i, rx, ctx, wakers);
                                polled_waked = Some(i);
                            }
                        }
                    }
                }
            }
            if _self.left_handles.load(Ordering::Acquire) == 0 {
                return Poll::Ready(Err(RecvError {}));
            }
        }
        // Try last one first if no wakers
        if _self.has_waker.load(Ordering::Acquire) == 0 {
            let last_index = _self.last_index.load(Ordering::Acquire);
            if let Some(rx) = &handles[last_index] {
                if !rx.is_empty() {
                    try_recv_poll!(_self, last_index, rx, ctx, wakers);
                    polled_last_index = Some(last_index);
                }
            }
        }
        if _self.left_handles.load(Ordering::Acquire) == 0 {
            return Poll::Ready(Err(RecvError {}));
        }
        // Check any thing left without wakers
        {
            for i in 0..handles.len() {
                let _rx = &handles[i];
                if Some(i) == polled_last_index || Some(i) == polled_waked {
                    continue;
                }
                if let Some(rx) = _rx {
                    try_recv_poll!(_self, i, rx, ctx, wakers);
                }
            }
        }
        if _self.left_handles.load(Ordering::Acquire) == 0 {
            return Poll::Ready(Err(RecvError {}));
        }
        return Poll::Pending;
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::mpmc;
    use crate::mpsc;
    use std::sync::Arc;
    use tokio::time::Duration;

    #[test]
    fn test_select_same_mpsc() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let (tx1, rx1) = mpsc::bounded_future_both::<i32>(3);
            let (tx2, rx2) = mpsc::bounded_future_both::<i32>(2);
            let sel = SelectSame::new();
            let _op1 = sel.add_recv(rx1);
            let op2 = sel.add_recv(rx2);
            let _ = tx1.send(1).await;
            let _ = tx1.send(2).await;
            assert_eq!(sel.select().await, Ok(1));
            assert_eq!(sel.select().await, Ok(2));
            assert_eq!(sel.try_recv(op2), None);
            tokio::spawn(async move {
                for i in 0..1000i32 {
                    let _ = tx1.send(i).await;
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                println!("tx 1 sleep 1");
                tokio::time::sleep(Duration::from_secs(1)).await;
                for i in 0..1000i32 {
                    let _ = tx1.send(i).await;
                }
            });
            tokio::spawn(async move {
                for i in 1000..1010i32 {
                    println!("tx 2 sleep 1");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let _ = tx2.send(i).await;
                }
            });
            loop {
                match sel.select().await {
                    Ok(_i) => { // println!("recv {}", i);
                    }
                    Err(_e) => break,
                }
                if let Some(_i) = sel.try_recv(op2) {
                    //println!("recv {}", _i)
                }
            }
        });
    }

    #[test]
    fn test_select_same_mpmc() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let (tx1, rx1) = mpmc::bounded_future_both::<i32>(3);
            let (tx2, rx2) = mpmc::bounded_future_both::<i32>(2);

            let recv_count = Arc::new(AtomicUsize::new(0));
            let mut ths = Vec::new();
            for _j in 0..2 {
                let _rx1 = rx1.clone();
                let _rx2 = rx2.clone();
                let count = recv_count.clone();
                ths.push(tokio::task::spawn(async move {
                    let sel = SelectSame::new();
                    let _op1 = sel.add_recv(_rx1);
                    let op2 = sel.add_recv(_rx2);
                    loop {
                        match sel.select().await {
                            Ok(_i) => {
                                //println!("{} recv {}", _j, _i);
                                count.fetch_add(1, Ordering::SeqCst);
                            }
                            Err(_e) => break,
                        }
                        if let Some(_i) = sel.try_recv(op2) {
                            //println!("{} try recv {}", _j, _i);
                            count.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    println!("rx done");
                }));
            }
            tokio::spawn(async move {
                for i in 0..1000i32 {
                    let _ = tx1.send(i).await;
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                println!("tx 1 sleep 1");
                tokio::time::sleep(Duration::from_secs(1)).await;
                for i in 1500..2500i32 {
                    let _ = tx1.send(i).await;
                }
            });
            tokio::spawn(async move {
                for i in 1000..1010i32 {
                    println!("tx 2 sleep 1");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let _ = tx2.send(i).await;
                    //println!("tx2 sent {}", i);
                }
            });
            for th in ths {
                let _ = th.await;
            }
            assert_eq!(recv_count.load(Ordering::Acquire), 2000 + 10);
        });
    }
}
