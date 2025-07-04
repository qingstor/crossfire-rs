use crate::channel::*;
use crossbeam::channel::Sender;
pub use crossbeam::channel::{SendError, SendTimeoutError, TrySendError};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::Duration;

/// Sender that works in blocking context
pub struct Tx<T> {
    pub(crate) sender: Sender<T>,
    pub(crate) shared: Arc<ChannelShared>,
}

impl<T> fmt::Debug for Tx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Tx")
    }
}

impl<T> Drop for Tx<T> {
    fn drop(&mut self) {
        self.shared.close_tx();
    }
}

impl<T> Tx<T> {
    #[inline]
    pub(crate) fn new(sender: Sender<T>, shared: Arc<ChannelShared>) -> Self {
        Self { sender, shared }
    }

    /// Send message. Will block when channel is full.
    ///
    /// Returns `Ok(())` on successful.
    ///
    /// Returns Err([SendError]) when all Rx is dropped.
    ///
    #[inline]
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        match self.sender.send(item) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.shared.on_send();
                return Ok(());
            }
        }
    }

    /// Try to send message, non-blocking
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([TrySendError::Full]) on channel full for bounded channel.
    ///
    /// Returns Err([TrySendError::Disconnected]) when all Rx dropped.
    #[inline]
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        match self.sender.try_send(item) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.shared.on_send();
                return Ok(());
            }
        }
    }

    /// Waits for a message to be sent into the channel, but only for a limited time.
    /// Will block when channel is empty.
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([SendTimeoutError::Timeout]) when the message could not be sent because the channel is full and the operation timed out.
    ///
    /// Returns Err([SendTimeoutError::Disconnected]) when all Rx dropped.
    #[inline]
    pub fn send_timeout(&self, item: T, timeout: Duration) -> Result<(), SendTimeoutError<T>> {
        match self.sender.send_timeout(item, timeout) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.shared.on_recv();
                return Ok(());
            }
        }
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline]
    pub fn len(&self) -> usize {
        self.sender.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.sender.is_empty()
    }
}

/// Sender that works in blocking context. MP version of [`Tx<T>`] implements [Clone].
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

/// For writing generic code with MTx & Tx
pub trait BlockingTxTrait<T: Send + 'static>: Send + 'static {
    /// Send message. Will block when channel is full.
    ///
    /// Returns `Ok(())` on successful.
    ///
    /// Returns Err([SendError]) when all Rx is dropped.
    fn send(&self, _item: T) -> Result<(), SendError<T>>;

    /// Try to send message, non-blocking
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([TrySendError::Full]) on channel full for bounded channel.
    ///
    /// Returns Err([TrySendError::Disconnected]) when all Rx dropped.
    fn try_send(&self, _item: T) -> Result<(), TrySendError<T>>;

    /// Waits for a message to be sent into the channel, but only for a limited time.
    /// Will block when channel is empty.
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([SendTimeoutError::Timeout]) when the message could not be sent because the channel is full and the operation timed out.
    ///
    /// Returns Err([SendTimeoutError::Disconnected]) when all Rx dropped.
    fn send_timeout(&self, item: T, timeout: Duration) -> Result<(), SendTimeoutError<T>>;

    /// Probe possible messages in the channel (not accurate)
    fn len(&self) -> usize;

    /// Whether there's message in the channel (not accurate)
    fn is_empty(&self) -> bool;
}

impl<T: Send + 'static> BlockingTxTrait<T> for Tx<T> {
    #[inline(always)]
    fn send(&self, item: T) -> Result<(), SendError<T>> {
        Tx::send(self, item)
    }

    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        Tx::try_send(self, item)
    }

    #[inline(always)]
    fn send_timeout(&self, item: T, timeout: Duration) -> Result<(), SendTimeoutError<T>> {
        Tx::send_timeout(&self, item, timeout)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        Tx::len(self)
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        Tx::is_empty(self)
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
    fn send_timeout(&self, item: T, timeout: Duration) -> Result<(), SendTimeoutError<T>> {
        self.0.send_timeout(item, timeout)
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
