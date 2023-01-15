#![warn(clippy::pedantic)]
#![allow(
    clippy::module_name_repetitions,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]

use std::cell::UnsafeCell;
use std::ops::Drop;

mod linked_list;
pub use linked_list::*;

mod slab;
pub use slab::*;

#[derive(Debug)]
#[repr(C)]
struct Mutex<T> {
    lock: nix::sys::pthread::Mutex,
    data: std::cell::UnsafeCell<T>,
}
impl<T> Mutex<T> {
    /// Creates a new mutex.
    ///
    /// # Panics
    ///
    /// When [`nix::sys::pthread::Mutex::new`] errors.
    pub fn new(data: T, attr: Option<nix::sys::pthread::MutexAttr>) -> Self {
        Self {
            lock: nix::sys::pthread::Mutex::new(attr).unwrap(),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> MutexGuard<T> {
        MutexGuard(self)
    }

    /// Returns a pointer to the underlying data without locking.
    ///
    /// # Safety
    ///
    /// Does not lock the data.
    pub unsafe fn get(&self) -> *mut T {
        self.data.get()
    }
}
struct MutexGuard<'a, T>(&'a Mutex<T>);
unsafe impl<T> Sync for Mutex<T> {}
impl<'a, T> std::ops::Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.0.data.get() }
    }
}
impl<'a, T> std::ops::DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.0.data.get() }
    }
}
impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.0.lock.unlock().unwrap();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::pedantic)]

    use super::*;

    #[test]
    fn mutex_debug() {
        assert_eq!(
            format!("{:?}", Mutex::new((), None)),
            "Mutex { lock: Mutex(UnsafeCell { .. }), data: UnsafeCell { .. } }"
        );
    }

    #[test]
    fn mutex_get() {
        unsafe {
            Mutex::new((), None).get();
        }
    }
}
