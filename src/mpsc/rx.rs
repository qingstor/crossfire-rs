use crossbeam::channel::{Receiver, TryRecvError, RecvError};
use std::task::{Context, Poll};
use std::sync::Arc;
use std::pin::Pin;
use std::future::Future;
use crate::channel::*;

pub struct RxBlocking<T, S: MPSCShared> {
    recv: Receiver<T>,
    shared: Arc<S>,
}

impl <T, S: MPSCShared> Drop for RxBlocking<T, S> {

    fn drop(&mut self) {
        self.shared.close_rx();
    }
}

impl <T, S: MPSCShared> RxBlocking <T, S> {

    #[inline]
    pub fn new(recv: Receiver<T>, shared: Arc<S>) -> Self {
        Self{
            recv: recv,
            shared: shared,
        }
    }

    #[inline]
    pub fn recv(&self) -> Result<T, RecvError> {
        match self.recv.recv() {
            Err(e)=>return Err(e),
            Ok(i)=>{
                self.shared.on_recv();
                return Ok(i);
            },
        }
    }

    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.recv.try_recv() {
            Err(e)=>return Err(e),
            Ok(i)=>{
                self.shared.on_recv();
                return Ok(i);
            },
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.recv.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.recv.is_empty()
    }

    #[inline(always)]
    pub fn raw(&self) -> &Receiver<T> {
        &self.recv
    }

    #[inline(always)]
    pub fn on_recv(&self) {
        self.shared.on_recv();
    }
}

pub struct RxFuture<T, S: MPSCShared> {
    recv: Receiver<T>,
    shared: Arc<S>,
}

impl <T, S: MPSCShared> Drop for RxFuture<T, S> {

    fn drop(&mut self) {
        self.shared.close_rx();
    }
}

impl <T, S: MPSCShared> RxFuture <T, S> {

    #[inline]
    pub fn new(recv: Receiver<T>, shared: Arc<S>) -> Self {
        Self{
            recv: recv,
            shared: shared,
        }
    }

    #[inline]
    pub fn recv_blocking(&self) -> Result<T, RecvError> {
        match self.recv.recv() {
            Err(e)=>return Err(e),
            Ok(i)=>{
                self.shared.on_recv();
                return Ok(i);
            },
        }
    }

    #[inline(always)]
    pub fn make_recv_future<'a>(&'a self) -> ReceiveFuture<'a, T, S> {
        return ReceiveFuture{rx: &self, waker: None}
    }

    #[inline]
    pub async fn recv(&self) -> Result<T, RecvError> {
        match self.try_recv() {
            Err(e)=>{
                if e.is_empty() {
                    return self.make_recv_future().await;
                }
                return Err(RecvError{});
            },
            Ok(item)=>return Ok(item),
        }
    }

    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.recv.try_recv() {
            Err(e)=>return Err(e),
            Ok(i)=>{
                self.shared.on_recv();
                return Ok(i);
            },
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.recv.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.recv.is_empty()
    }

    #[inline]
    pub fn get_waker_length(&self) -> (usize, usize) {
        return self.shared.get_waker_length();
    }

    #[inline]
    pub fn poll_item(&self, ctx: &mut Context, waker: &mut Option<LockedWaker>) -> Result<T, TryRecvError> {
        match self.recv.try_recv() {
            Err(e)=>{
                if !e.is_empty() {
                     if let Some(old_waker) = waker.take() {
                        old_waker.abandon();
                    }
                    return Err(e)
                }
                if let Some(old_waker) = waker.as_ref() {
                    if old_waker.is_waked() {
                        waker.take(); // should reg again
                    } else {
                        return Err(e)
                    }
                }
            },
            Ok(item)=>{
                self.shared.on_recv();
                if let Some(old_waker) = waker.take() {
                    if old_waker.is_waked() {
                        old_waker.abandon();
                    }
                }
                return Ok(item)
            }
        }
        self.shared.cancel_recv_reg();
        if let Some(_waker) = self.shared.reg_recv() {
            match self.recv.try_recv() {
                Err(e)=>{
                    if e.is_empty() {
                        _waker.commit(ctx);
                        waker.replace(_waker);
                        return Err(e);
                    }
                    self.shared.cancel_recv_reg();
                    return Err(e);
                },
                Ok(item)=>{
                    _waker.cancel();
                    self.shared.on_recv();
                    self.shared.cancel_recv_reg();
                    // should unlock before on receive
                    return Ok(item)
                }
            }
        } else {
            // all rx closed, but still may be some buffer in channel
            match self.recv.try_recv() {
                Err(_e)=>{
                    return Err(TryRecvError::Disconnected{})
                },
                Ok(item)=>{
                    self.shared.on_recv();
                    return Ok(item);
                }
            }
        }
    }

}

#[async_trait]
impl <'a, T: Send + Sync + 'static, S: MPSCShared> AsyncRx<T> for RxFuture<T, S>
{

    #[inline(always)]
    fn poll_item(&self, ctx: &mut Context, waker: &mut Option<LockedWaker>) -> Result<T, TryRecvError> {
        self.poll_item(ctx, waker)
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        self.try_recv()
    }

    #[inline(always)]
    async fn recv(&self) -> Result<T, RecvError> {
        self.recv().await
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.recv.is_empty()
    }
}

pub struct ReceiveFuture<'a, T, S: MPSCShared> {
    rx: &'a RxFuture<T, S>,
    waker: Option<LockedWaker>,
}

impl<T, S: MPSCShared> ReceiveFuture<'_, T, S> {

    // Take waker to save outside, so that waker will not be dropped (canceled).
    // You may need to try_recv or make another ReceiveFuture once woke up.
    #[inline]
    pub fn take_waker(&mut self) -> Option<LockedWaker> {
        self.waker.take()
    }
}

impl <T, S: MPSCShared> Future for ReceiveFuture<'_, T, S> {
    type Output = Result<T, RecvError>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut _self = self.get_mut();
        match _self.rx.poll_item(ctx, &mut _self.waker) {
            Err(e)=>{
                if !e.is_empty() {
                    return Poll::Ready(Err(RecvError{}));
                }
                return Poll::Pending
            },
            Ok(item)=>{
                return Poll::Ready(Ok(item));
            }
        }
    }
}

impl<T, S: MPSCShared> Drop for ReceiveFuture<'_, T, S> {

    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.abandon();
        }
    }
}

