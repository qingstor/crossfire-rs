use crate::channel::*;
use crate::stream::Stream;
use crossbeam::channel::{Receiver, RecvError, TryRecvError};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// Receiver that dose not support async
pub struct RxBlocking<T, S: ChannelShared> {
    recv: Receiver<T>,
    shared: Arc<S>,
}

impl<T, S: ChannelShared> Clone for RxBlocking<T, S> {
    #[inline]
    fn clone(&self) -> Self {
        self.shared.add_rx();
        Self { recv: self.recv.clone(), shared: self.shared.clone() }
    }
}

impl<T, S: ChannelShared> Drop for RxBlocking<T, S> {
    fn drop(&mut self) {
        self.shared.close_rx();
    }
}

impl<T, S: ChannelShared> RxBlocking<T, S> {
    #[inline]
    pub(crate) fn new(recv: Receiver<T>, shared: Arc<S>) -> Self {
        Self { recv, shared }
    }

    #[inline]
    pub fn recv<'a>(&'a self) -> Result<T, RecvError> {
        match self.recv.recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.recv.try_recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline]
    pub fn len(&self) -> usize {
        self.recv.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.recv.is_empty()
    }

    /// Return a crossbeam Receiver, you should make sure to call on_recv() after receiving a
    /// message
    /// (If you know what you're doing)
    #[inline(always)]
    pub fn raw(&self) -> &Receiver<T> {
        &self.recv
    }

    /// (If you know what you're doing)
    #[inline(always)]
    pub fn on_recv(&self) {
        self.shared.on_recv();
    }
}

pub struct RxFuture<T, S: ChannelShared> {
    recv: Receiver<T>,
    shared: Arc<S>,
}

impl<T, S: ChannelShared> Clone for RxFuture<T, S> {
    #[inline]
    fn clone(&self) -> Self {
        self.shared.add_rx();
        Self { recv: self.recv.clone(), shared: self.shared.clone() }
    }
}

impl<T, S: ChannelShared> Drop for RxFuture<T, S> {
    fn drop(&mut self) {
        self.shared.close_rx();
    }
}

impl<T, S: ChannelShared> RxFuture<T, S> {
    #[inline]
    pub(crate) fn new(recv: Receiver<T>, shared: Arc<S>) -> Self {
        Self { recv, shared }
    }

    #[inline]
    pub async fn recv(&self) -> Result<T, RecvError> {
        match self.try_recv() {
            Err(e) => {
                if e.is_empty() {
                    return ReceiveFuture { rx: &self, waker: None }.await;
                }
                return Err(RecvError {});
            }
            Ok(item) => return Ok(item),
        }
    }

    /// Receive a message while blocking the current thread. (If you know what you're
    /// doing)
    #[inline]
    pub fn recv_blocking(&self) -> Result<T, RecvError> {
        match self.recv.recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.recv.try_recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    /// Generate a future object that receive a message
    #[inline]
    pub fn make_recv_future<'a>(&'a self) -> ReceiveFuture<'a, T, S> {
        return ReceiveFuture { rx: &self, waker: None };
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline]
    pub fn len(&self) -> usize {
        self.recv.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.recv.is_empty()
    }

    /// Returns count of tx / rx wakers stored in channel for debug purpose
    #[inline]
    pub fn get_waker_length(&self) -> (usize, usize) {
        return self.shared.get_waker_length();
    }

    /// This is only useful when you're writing your own future
    #[inline(always)]
    fn poll_item(
        &self, ctx: &mut Context, waker: &mut Option<LockedWaker>,
    ) -> Result<T, TryRecvError> {
        match self.recv.try_recv() {
            Err(e) => {
                if !e.is_empty() {
                    if let Some(old_waker) = waker.take() {
                        old_waker.abandon();
                    }
                    return Err(e);
                }
                if let Some(old_waker) = waker.as_ref() {
                    if old_waker.is_waked() {
                        let _ = waker.take(); // should reg again
                    } else {
                        return Err(e);
                    }
                }
            }
            Ok(item) => {
                self.shared.on_recv();
                if let Some(old_waker) = waker.take() {
                    old_waker.abandon();
                }
                return Ok(item);
            }
        }
        if let Some(_waker) = self.shared.reg_recv(ctx) {
            match self.recv.try_recv() {
                Err(e) => {
                    if e.is_empty() {
                        _waker.commit();
                        waker.replace(_waker);
                    } else {
                        _waker.cancel();
                    }
                    return Err(e);
                }
                Ok(item) => {
                    if !_waker.is_waked() {
                        _waker.cancel();
                    }
                    self.shared.on_recv();
                    return Ok(item);
                }
            }
        } else {
            // all rx closed, but still may be some buffer in channel
            match self.recv.try_recv() {
                Err(_e) => return Err(TryRecvError::Disconnected {}),
                Ok(item) => {
                    self.shared.on_recv();
                    return Ok(item);
                }
            }
        }
    }

    pub fn into_stream(self) -> Stream<T, Self>
    where
        T: Sync + Send + Unpin + 'static,
    {
        Stream::new(self)
    }
}

#[async_trait]
impl<'a, T: Send + Sync + 'static, S: ChannelShared> AsyncRx<T> for RxFuture<T, S> {
    #[inline(always)]
    fn poll_item(
        &self, ctx: &mut Context, waker: &mut Option<LockedWaker>,
    ) -> Result<T, TryRecvError> {
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

    #[inline(always)]
    fn on_send(&self) {
        self.shared.on_send();
    }

    #[inline(always)]
    fn clear_recv_wakers(&self, seq: u64) {
        self.shared.clear_recv_wakers(seq)
    }
}

pub struct ReceiveFuture<'a, T, S: ChannelShared> {
    rx: &'a RxFuture<T, S>,
    waker: Option<LockedWaker>,
}

impl<T, S: ChannelShared> Drop for ReceiveFuture<'_, T, S> {
    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            if waker.abandon() {
                // We are waked, but abandoning, should notify another receiver
                if !self.rx.recv.is_empty() {
                    self.rx.shared.on_send();
                }
            } else {
                self.rx.shared.clear_recv_wakers(waker.get_seq());
            }
        }
    }
}

impl<T, S: ChannelShared> Future for ReceiveFuture<'_, T, S> {
    type Output = Result<T, RecvError>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut _self = self.get_mut();
        match _self.rx.poll_item(ctx, &mut _self.waker) {
            Err(e) => {
                if !e.is_empty() {
                    return Poll::Ready(Err(RecvError {}));
                } else {
                    return Poll::Pending;
                }
            }
            Ok(item) => {
                return Poll::Ready(Ok(item));
            }
        }
    }
}
