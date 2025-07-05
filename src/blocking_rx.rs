use crate::channel::*;
use crossbeam_utils::Backoff;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Receiver that works in blocking context
pub struct Rx<T> {
    pub(crate) shared: Arc<ChannelShared<T>>,
}

impl<T> fmt::Debug for Rx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Rx")
    }
}

impl<T> Drop for Rx<T> {
    fn drop(&mut self) {
        self.shared.close_rx();
    }
}

impl<T> Rx<T> {
    #[inline(always)]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self { shared }
    }

    #[inline(always)]
    pub(crate) fn _recv_blocking(
        shared: &ChannelShared<T>, deadline: Option<Instant>,
    ) -> Result<T, RecvTimeoutError> {
        if shared.bound_size == Some(0) {
            todo!();
        } else {
            let waker = LockedWaker::new_blocking();
            debug_assert!(waker.is_waked());
            loop {
                let backoff = Backoff::new();
                loop {
                    if let Some(item) = shared.try_recv() {
                        shared.on_recv();
                        return Ok(item);
                    }
                    if backoff.is_completed() {
                        break;
                    }
                    backoff.snooze();
                }
                if shared.get_tx_count() == 0 {
                    return Err(RecvTimeoutError::Disconnected);
                }
                if waker.is_waked() {
                    shared.reg_recv_blocking(&waker);
                    if !shared.is_empty() {
                        continue;
                    }
                }
                if !wait_timeout(deadline) {
                    return Err(RecvTimeoutError::Timeout);
                }
            }
        }
    }

    /// Receive message, will block when channel is empty.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([RecvError]) when all Tx dropped.
    #[inline]
    pub fn recv<'a>(&'a self) -> Result<T, RecvError> {
        Self::_recv_blocking(&self.shared, None).map_err(|err| match err {
            RecvTimeoutError::Disconnected => RecvError,
            RecvTimeoutError::Timeout => unreachable!(),
        })
    }

    /// Try to receive message, non-blocking.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([TryRecvError::Empty]) when channel is empty.
    ///
    /// returns Err([TryRecvError::Disconnected]) when all Tx dropped.
    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        if self.shared.bound_size == Some(0) {
            todo!();
        } else {
            if let Some(item) = self.shared.try_recv() {
                self.shared.on_recv();
                return Ok(item);
            } else {
                if self.shared.get_tx_count() == 0 {
                    return Err(TryRecvError::Disconnected);
                }
                return Err(TryRecvError::Empty);
            }
        }
    }

    /// Waits for a message to be received from the channel, but only for a limited time.
    /// Will block when channel is empty.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([RecvTimeoutError::Timeout]) when a message could not be received because the channel is empty and the operation timed out.
    ///
    /// returns Err([RecvTimeoutError::Disconnected]) when all Tx dropped.
    #[inline]
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        match Instant::now().checked_add(timeout) {
            Some(deadline) => Self::_recv_blocking(&self.shared, Some(deadline)),
            None => self.try_recv().map_err(|e| match e {
                TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
                TryRecvError::Empty => RecvTimeoutError::Timeout,
            }),
        }
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.shared.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.shared.is_empty()
    }
}

/// Receiver that works in blocking context. MC version of [`Rx<T>`] implements [Clone].
///
/// You can use `into()` to convert it to `Rx<T>`.
pub struct MRx<T>(pub(crate) Rx<T>);

impl<T> MRx<T> {
    #[inline(always)]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self(Rx::new(shared))
    }
}

impl<T> Clone for MRx<T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        let inner = &self.0;
        inner.shared.add_rx();
        Self(Rx { shared: inner.shared.clone() })
    }
}

impl<T> Deref for MRx<T> {
    type Target = Rx<T>;

    /// inherit all the functions of [Rx]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for MRx<T> {
    /// inherit all the functions of [Rx]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<MRx<T>> for Rx<T> {
    fn from(rx: MRx<T>) -> Self {
        rx.0
    }
}

/// For writing generic code with MRx & Rx
pub trait BlockingRxTrait<T: Send + 'static>: Send + 'static {
    /// Receive message, will block when channel is empty.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// Returns Err([RecvError]) when all Tx dropped.
    fn recv<'a>(&'a self) -> Result<T, RecvError>;

    /// Try to receive message, non-blocking.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// Returns Err([TryRecvError::Empty]) when channel is empty.
    ///
    /// Returns Err([TryRecvError::Disconnected]) when all Tx dropped.
    fn try_recv(&self) -> Result<T, TryRecvError>;

    /// Waits for a message to be received from the channel, but only for a limited time.
    /// Will block when channel is empty.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([RecvTimeoutError::Timeout]) when a message could not be received because the channel is empty and the operation timed out.
    ///
    /// returns Err([RecvTimeoutError::Disconnected]) when all Tx dropped.
    fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError>;

    /// Probe possible messages in the channel (not accurate)
    fn len(&self) -> usize;

    /// Whether there's message in the channel (not accurate)
    fn is_empty(&self) -> bool;
}

impl<T: Send + 'static> BlockingRxTrait<T> for Rx<T> {
    #[inline(always)]
    fn recv<'a>(&'a self) -> Result<T, RecvError> {
        Rx::recv(self)
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        Rx::try_recv(self)
    }

    #[inline(always)]
    fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        Rx::recv_timeout(self, timeout)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        Rx::len(self)
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        Rx::is_empty(self)
    }
}

impl<T: Send + 'static> BlockingRxTrait<T> for MRx<T> {
    #[inline(always)]
    fn recv<'a>(&'a self) -> Result<T, RecvError> {
        self.0.recv()
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv()
    }

    #[inline(always)]
    fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        self.0.recv_timeout(timeout)
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
}
