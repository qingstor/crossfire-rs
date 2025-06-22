use crate::channel::*;
use crate::tx::*;
use async_trait::async_trait;
use crossbeam::channel::Sender;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// Sender that works in blocking context, the same with [`Tx<T>`], except that it can be
/// `clone()`.
///
/// You can use `into()` to convert it to `Tx<T>`.
pub struct MTx<T>(pub(crate) Tx<T>);

impl<T> MTx<T> {
    #[inline]
    pub(crate) fn new(send: Sender<T>, shared: Arc<ChannelShared>) -> Self {
        Self(Tx::new(send, shared))
    }
}

impl<T: Unpin> Clone for MTx<T> {
    #[inline]
    fn clone(&self) -> Self {
        let inner = &self.0;
        inner.shared.add_tx();
        Self(Tx::new(inner.sender.clone(), inner.shared.clone()))
    }
}

impl<T> Deref for MTx<T> {
    type Target = Tx<T>;

    /// inherit all the functions of [Tx]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for MTx<T> {
    /// inherit all the functions of [Tx]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Sender that works in blocking context, the same with [`AsyncTx<T>`], except that it can be
/// `clone()`.
///
/// You can use `into()` to convert it to `AsyncTx<T>`.
pub struct MAsyncTx<T>(pub(crate) AsyncTx<T>);

impl<T: Unpin> Clone for MAsyncTx<T> {
    #[inline]
    fn clone(&self) -> Self {
        let inner = &self.0;
        inner.shared.add_tx();
        Self(AsyncTx::new(inner.sender.clone(), inner.shared.clone()))
    }
}

impl<T> MAsyncTx<T> {
    #[inline]
    pub(crate) fn new(send: Sender<T>, shared: Arc<ChannelShared>) -> Self {
        Self(AsyncTx::new(send, shared))
    }
}

impl<T> Deref for MAsyncTx<T> {
    type Target = AsyncTx<T>;

    /// inherit all the functions of [AsyncTx]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for MAsyncTx<T> {
    /// inherit all the functions of [AsyncTx]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Send + 'static> BlockingTxTrait<T> for MTx<T> {
    #[inline(always)]
    fn send(&self, item: T) -> Result<(), SendError<T>> {
        self.0.send(item)
    }

    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        self.0.try_send(item)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.0.len()
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[async_trait]
impl<T: Unpin + Send + 'static> AsyncTxTrait<T> for MAsyncTx<T> {
    #[inline(always)]
    async fn send(&self, item: T) -> Result<(), SendError<T>> {
        self.0.send(item).await
    }

    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        self.0.try_send(item)
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline(always)]
    fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn make_send_future<'a>(&'a self, item: T) -> SendFuture<'a, T> {
        self.0.make_send_future(item)
    }

    /// Just for debugging purpose, to monitor queue size
    #[inline(always)]
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize) {
        self.0.get_waker_size()
    }
}
