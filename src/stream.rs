use crate::channel::AsyncRx;
use crate::locked_waker::LockedWaker;
use futures::stream;
use std::pin::Pin;
use std::task::*;

pub struct Stream<T, R>
where
    T: Unpin + Send + Sync + 'static,
    R: AsyncRx<T>,
{
    rx: R,
    waker: Option<LockedWaker>,
    phan: std::marker::PhantomData<T>,
    ended: bool,
}

impl<T, R> Stream<T, R>
where
    T: Unpin + Send + Sync + 'static,
    R: AsyncRx<T>,
{
    pub fn new(rx: R) -> Self {
        Self { rx, waker: None, phan: Default::default(), ended: false }
    }
}

impl<T, R> stream::Stream for Stream<T, R>
where
    T: Unpin + Send + Sync + 'static,
    R: AsyncRx<T> + Unpin,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut _self = self.get_mut();
        match _self.rx.poll_item(ctx, &mut _self.waker) {
            Err(e) => {
                if e.is_empty() {
                    return Poll::Pending;
                }
                _self.ended = true;
                return Poll::Ready(None);
            }
            Ok(item) => Poll::Ready(Some(item)),
        }
    }
}

impl<T, R> stream::FusedStream for Stream<T, R>
where
    T: Unpin + Send + Sync + 'static,
    R: AsyncRx<T> + Unpin,
{
    fn is_terminated(&self) -> bool {
        self.ended
    }
}

impl<T, R> Drop for Stream<T, R>
where
    T: Unpin + Send + Sync + 'static,
    R: AsyncRx<T>,
{
    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            self.rx.clear_recv_wakers(waker.get_seq());
        }
    }
}

#[cfg(test)]
mod tests {

    use futures::stream::{FusedStream, StreamExt};

    #[test]
    fn test_into_stream() {
        println!();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let total_message = 100;
            let (tx, rx) = crate::mpmc::bounded_future_both::<i32>(2);
            tokio::spawn(async move {
                println!("sender thread send {} message start", total_message);
                for i in 0i32..total_message {
                    let _ = tx.send(i).await;
                    // println!("send {}", i);
                }
                println!("sender thread send {} message end", total_message);
            });
            let mut s = rx.into_stream();

            for _i in 0..total_message {
                assert_eq!(s.next().await, Some(_i));
            }
            assert_eq!(s.next().await, None);
            assert!(s.is_terminated())
        });
    }
}
