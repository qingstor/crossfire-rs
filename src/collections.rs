use std::ptr;
use std::sync::{
    atomic::{AtomicPtr, Ordering},
    Arc, Weak,
};

pub struct SpmcCell<T> {
    ptr: AtomicPtr<T>,
}

unsafe impl<T> Send for SpmcCell<T> {}
unsafe impl<T> Sync for SpmcCell<T> {}

impl<T> SpmcCell<T> {
    #[inline(always)]
    pub fn new() -> Self {
        Self { ptr: AtomicPtr::new(ptr::null_mut()) }
    }

    #[inline(always)]
    pub fn pop(&self) -> Option<Arc<T>> {
        let ptr = self.ptr.swap(ptr::null_mut(), Ordering::SeqCst);
        if ptr != ptr::null_mut() {
            return unsafe { Weak::from_raw(ptr) }.upgrade();
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn clear(&self) {
        let ptr = self.ptr.swap(ptr::null_mut(), Ordering::SeqCst);
        if ptr != ptr::null_mut() {
            // Convert into Weak and drop
            let _ = unsafe { Weak::from_raw(ptr) };
        }
    }

    #[inline(always)]
    pub fn put(&self, item: &Arc<T>) {
        let weak = Arc::downgrade(item).into_raw();
        let old_ptr = self.ptr.swap(weak as *mut T, Ordering::SeqCst);
        if old_ptr != ptr::null_mut() {
            let _ = unsafe { Weak::from_raw(old_ptr) };
        }
    }
}
