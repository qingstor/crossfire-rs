use crossbeam::channel::{Sender, TrySendError, SendError};
use std::task::{Context, Poll};
use std::sync::Arc;
use std::pin::Pin;
use std::future::Future;
use crate::channel::*;

pub struct TxBlocking<T, S: MPSCShared> {
    sender: Sender<T>,
    shared: Arc<S>,
}

impl <T, S: MPSCShared> Clone for TxBlocking<T, S> {

    #[inline]
    fn clone(&self) -> Self {
        self.shared.add_tx();
        Self {
            sender: self.sender.clone(),
            shared: self.shared.clone(),
        }
    }
}

impl <T, S: MPSCShared> Drop for TxBlocking<T, S> {

    fn drop(&mut self) {
        self.shared.close_tx();
    }
}

impl<T, S: MPSCShared> TxBlocking<T, S> {

    #[inline]
    pub(crate) fn new(sender: Sender<T>, shared: Arc<S>) -> Self {
        Self{
            sender: sender,
            shared: shared,
        }
    }

    #[inline]
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        match self.sender.send(item) {
            Err(e)=>return Err(e),
            Ok(_)=>{
                self.shared.on_send();
                return Ok(());
            },
        }
    }

    #[inline]
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        match self.sender.try_send(item) {
            Err(e)=>return Err(e),
            Ok(_)=>{
                self.shared.on_send();
                return Ok(());
            },
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.sender.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.sender.is_empty()
    }

}

pub struct TxFuture<T, S: MPSCShared> {
    sender: Sender<T>,
    shared: Arc<S>,
}

impl <T, S: MPSCShared> Clone for TxFuture<T, S> {

    #[inline]
    fn clone(&self) -> Self {
        self.shared.add_tx();
        Self {
            sender: self.sender.clone(),
            shared: self.shared.clone(),
        }
    }
}

impl <T, S: MPSCShared> Drop for TxFuture<T, S> {

    fn drop(&mut self) {
        self.shared.close_tx();
    }
}

impl <T: Unpin, S: MPSCShared> TxFuture<T, S> {

    #[inline]
    pub(crate) fn new(sender: Sender<T>, shared: Arc<S>) -> Self {
        Self{
            sender: sender,
            shared: shared,
        }
    }

    #[inline]
    pub async fn send(&self, item: T) -> Result<(), SendError<T>> {
        match self.try_send(item) {
            Ok(())=>return Ok(()),
            Err(TrySendError::Full(t))=>{
                return SendFuture{tx: &self, item: Some(t), waker: None}.await;
            },
            Err(TrySendError::Disconnected(t))=>return Err(SendError(t)),
        }
    }

    /// Send a message while blocking the current thread. (Used outside async context,
    /// if you know what you're doing)
    #[inline]
    pub fn send_blocking(&self, item: T) -> Result<(), SendError<T>> {
        self.sender.send(item)
    }

    /// Generate a future object that send a message
    #[inline(always)]
    pub fn make_send_future<'a>(&'a self, item: T) -> SendFuture<'a, T, S> {
        return SendFuture{tx: &self, item: Some(item), waker: None}
    }

    #[inline]
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        match self.sender.try_send(item) {
            Err(e)=>return Err(e),
            Ok(_)=>{
                self.shared.on_send();
                return Ok(());
            },
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

    /// Returns count of tx / rx wakers stored in channel for debug purpose
    #[inline]
    pub fn get_waker_length(&self) -> (usize, usize) {
        return self.shared.get_waker_length();
    }

    #[inline]
    fn clear_send_wakers(&self, waker: LockedWaker) {
        self.shared.clear_send_wakers(waker);
    }

    #[inline(always)]
    fn poll_send<'a>(&'a self, ctx: &'a mut Context, mut item: T, waker: &'a mut Option<LockedWaker>) -> Result<(), TrySendError<T>> {
        match self.sender.try_send(item) {
            Err(TrySendError::Disconnected(t))=>{
                if let Some(old_waker) = waker.take() {
                    old_waker.abandon();
                }
                return Err(TrySendError::Disconnected(t));
            },
            Err(TrySendError::Full(t))=>{
                if let Some(old_waker) = waker.as_ref() {
                    if old_waker.is_waked() {
                        let _ = waker.take(); //reg again
                    } else {
                        return Err(TrySendError::Full(t));
                    }
                }
                item = t;
            },
            Ok(())=>{
                self.shared.on_send();
                if let Some(old_waker) = waker.take() {
                    if !old_waker.is_waked() {
                        old_waker.abandon();
                    }
                }
                return Ok(());
            },
        }
        if let Some(_waker) = self.shared.reg_send() {
            match self.sender.try_send(item) {
                Ok(())=>{
                    self.shared.on_send();
                    if !_waker.is_waked() {
                        _waker.cancel(); // First release out spin lock, then poll a recver_waker
                    }
                    return Ok(());
                },
                Err(TrySendError::Full(t))=>{
                    _waker.commit(ctx);
                    waker.replace(_waker);
                    return Err(TrySendError::Full(t));
                },
                Err(TrySendError::Disconnected(t))=>{
                    _waker.cancel();
                    return Err(TrySendError::Disconnected(t));
                }
            }
        } else {
            // all rx closed
            return Err(TrySendError::Disconnected(item));
        }
    }
}

pub struct SendFuture<'a, T: Unpin, S: MPSCShared> {
    tx: &'a TxFuture<T, S>,
    item: Option<T>,
    waker: Option<LockedWaker>,
}

impl <'a, T: Unpin, S: MPSCShared> Drop for SendFuture<'a, T, S> {

    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            if waker.abandon() {
                // We are waked, but abandoning, should notify another sender
                if !self.tx.sender.is_full() {
                    self.tx.shared.on_recv();
                }
            } else {
                self.tx.clear_send_wakers(waker);
            }
        }
    }
}

impl <'a, T: Unpin, S: MPSCShared> Future for SendFuture<'a, T, S> {
    type Output = Result<(), SendError<T>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut _self = self.get_mut();
        let item = _self.item.take().unwrap();
        let tx = _self.tx;
        let r = tx.poll_send(ctx, item, &mut _self.waker);
        match r {
            Ok(())=>{
                return Poll::Ready(Ok(()));
            },
            Err(TrySendError::Disconnected(t))=>{
                return Poll::Ready(Err(SendError(t)));
            },
            Err(TrySendError::Full(t))=>{
                _self.item.replace(t);
                return Poll::Pending;
            },
        }
    }
}
