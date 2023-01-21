#![allow(clippy::module_name_repetitions)]

#[cfg(feature = "repr_c")]
#[derive(Debug)]
#[repr(C)]
pub struct Mutex<T> {
    pub lock: nix::sys::pthread::Mutex,
    data: std::cell::UnsafeCell<T>,
}

impl<T> Mutex<T> {
    /// Creates a new mutex.
    ///
    /// # Panics
    ///
    /// When [`nix::sys::pthread::Mutex::new`] errors.
    pub fn new(data: T, attr: Option<nix::sys::pthread::MutexAttr>) -> Self {
        #[cfg(feature = "log")]
        log::trace!("Mutex::new");

        Self {
            lock: nix::sys::pthread::Mutex::new(attr).unwrap(),
            data: std::cell::UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> nix::Result<MutexGuard<T>> {
        #[cfg(feature = "log")]
        log::trace!("Mutex::lock");

        self.lock.lock()?;

        Ok(MutexGuard(self))
    }

    /// Returns a pointer to the underlying data without locking.
    ///
    /// # Safety
    ///
    /// Does not lock the data.
    pub unsafe fn get(&self) -> *mut T {
        #[cfg(feature = "log")]
        log::trace!("Mutex::get");

        self.data.get()
    }
}

pub struct MutexGuard<'a, T>(&'a Mutex<T>);
unsafe impl<T> Sync for Mutex<T> {}
impl<'a, T> std::ops::Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        #[cfg(feature = "log")]
        log::trace!("Mutex::deref");

        unsafe { &*self.0.data.get() }
    }
}
impl<'a, T> std::ops::DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[cfg(feature = "log")]
        log::trace!("Mutex::deref_mut");

        unsafe { &mut *self.0.data.get() }
    }
}
impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        #[cfg(feature = "log")]
        log::trace!("Mutex::drop");

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
