use crate::channel::*;
use crossbeam_utils::Backoff;
use std::fmt;
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Sender that works in blocking context
pub struct Tx<T> {
    pub(crate) shared: Arc<ChannelShared<T>>,
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

impl<T: Send + 'static> Tx<T> {
    #[inline(always)]
    fn _try_send(shared: &ChannelShared<T>, item: T) -> Result<(), T> {
        let _item = MaybeUninit::new(item);
        match shared.try_send(&_item) {
            Err(()) => {
                return Err(unsafe { _item.assume_init_read() });
            }
            Ok(_) => {
                shared.on_send();
                return Ok(());
            }
        }
    }

    #[inline(always)]
    pub(crate) fn _send_blocking(
        shared: &ChannelShared<T>, item: T, deadline: Option<Instant>,
    ) -> Result<(), SendTimeoutError<T>> {
        if shared.get_rx_count() == 0 {
            return Err(SendTimeoutError::Disconnected(item));
        }
        if let Some(bound_size) = shared.bound_size {
            if bound_size == 0 {
                todo!();
            } else {
                let waker = LockedWaker::new_blocking();
                debug_assert!(waker.is_waked());
                let _item = MaybeUninit::new(item);
                loop {
                    let backoff = Backoff::new();
                    loop {
                        if shared.try_send(&_item).is_ok() {
                            shared.on_send();
                            return Ok(());
                        }
                        if backoff.is_completed() {
                            break;
                        }
                        backoff.snooze();
                    }
                    if shared.get_rx_count() == 0 {
                        return Err(SendTimeoutError::Disconnected(unsafe {
                            _item.assume_init_read()
                        }));
                    }
                    if waker.is_waked() {
                        shared.reg_send_blocking(&waker);
                        if !shared.is_full() {
                            continue;
                        }
                    }
                    if !wait_timeout(deadline) {
                        return Err(SendTimeoutError::Timeout(unsafe { _item.assume_init_read() }));
                    }
                }
            }
        } else {
            // unbounded
            match Self::_try_send(shared, item) {
                Ok(_) => return Ok(()),
                Err(_) => unreachable!(),
            }
        }
    }

    /// Send message. Will block when channel is full.
    ///
    /// Returns `Ok(())` on successful.
    ///
    /// Returns Err([SendError]) when all Rx is dropped.
    ///
    #[inline]
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        Self::_send_blocking(&self.shared, item, None).map_err(|err| match err {
            SendTimeoutError::Disconnected(msg) => SendError(msg),
            SendTimeoutError::Timeout(_) => unreachable!(),
        })
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
        if self.shared.bound_size == Some(0) {
            todo!();
        }
        if let Err(t) = Self::_try_send(&self.shared, item) {
            if self.shared.get_rx_count() == 0 {
                return Err(TrySendError::Disconnected(t));
            }
            return Err(TrySendError::Full(t));
        } else {
            Ok(())
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
        match Instant::now().checked_add(timeout) {
            Some(deadline) => Self::_send_blocking(&self.shared, item, Some(deadline)),
            None => self.try_send(item).map_err(|e| match e {
                TrySendError::Disconnected(t) => SendTimeoutError::Disconnected(t),
                TrySendError::Full(t) => SendTimeoutError::Timeout(t),
            }),
        }
    }
}

impl<T> Tx<T> {
    #[inline]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self { shared }
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline]
    pub fn len(&self) -> usize {
        self.shared.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.shared.is_empty()
    }
}

/// Sender that works in blocking context. MP version of [`Tx<T>`] implements [Clone].
///
/// You can use `into()` to convert it to `Tx<T>`.
pub struct MTx<T>(pub(crate) Tx<T>);

impl<T> MTx<T> {
    #[inline]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self(Tx::new(shared))
    }
}

impl<T: Unpin> Clone for MTx<T> {
    #[inline]
    fn clone(&self) -> Self {
        let inner = &self.0;
        inner.shared.add_tx();
        Self(Tx::new(inner.shared.clone()))
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
