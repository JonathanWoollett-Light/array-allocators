use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut, Drop};

#[cfg(feature = "log")]
use log::trace;

#[derive(Debug)]
#[repr(C)]
pub struct ArrayAllocator<const N: usize, T> {
    allocator: Allocator<T>,
    data: [Block<T>; N],
}
impl<const N: usize, T> ArrayAllocator<N, T> {
    pub fn allocator(&self) -> &Allocator<T> {
        &self.allocator
    }

    pub fn data(&self) -> &[Block<T>; N] {
        &self.data
    }

    #[must_use]
    pub fn new(attr: Option<nix::sys::pthread::MutexAttr>) -> Self {
        #[cfg(feature = "log")]
        trace!("ArrayAllocator::new");

        let mut this: Self = unsafe { std::mem::zeroed() };
        unsafe {
            Allocator::init(&mut this.allocator, attr, N);
        }
        this
    }
}

impl<const N: usize, T> Deref for ArrayAllocator<N, T> {
    type Target = Allocator<T>;

    fn deref(&self) -> &Self::Target {
        &self.allocator
    }
}
impl<const N: usize, T> DerefMut for ArrayAllocator<N, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.allocator
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Allocator<T>(crate::mutex::Mutex<InnerAllocator<T>>);

impl<T> Allocator<T> {
    /// Initializes `Self` at `ptr`.
    ///
    /// # Safety
    ///
    /// `ptr` must be valid.
    ///
    /// # Panics
    ///
    /// When failing to initialize the inner mutex.

    pub unsafe fn init(ptr: *mut Self, attr: Option<nix::sys::pthread::MutexAttr>, size: usize) {
        #[cfg(feature = "log")]
        trace!("Allocator::init");

        (*ptr).0.lock = nix::sys::pthread::Mutex::new(attr).unwrap();

        #[cfg(feature = "log")]
        trace!("Allocator::init 2");

        <InnerAllocator<T>>::init((*ptr).0.get(), size);
    }

    /// Allocates a given `x`.
    ///
    /// # Panics
    ///
    /// When locking the mutex fails.
    pub fn allocate(&self, x: T) -> Option<Wrapper<T>> {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate");
        let mut inner_allocator = self.0.lock().unwrap();
        if let Some(head) = inner_allocator.head {
            let index = head;
            unsafe {
                inner_allocator.head = inner_allocator.data().as_ref()[index].empty;
                inner_allocator.data().as_mut()[index] = Block {
                    full: ManuallyDrop::new(x),
                };
            }
            Some(Wrapper {
                allocator: self,
                index,
            })
        } else {
            None
        }
    }

    /// Returns wrappers for all non-free spaces.
    ///
    /// The intended usage is for one process `std::mem::forget`s all its wrappers then another
    /// picks them up with this iterator.
    ///
    /// # Safety
    ///
    /// Since this returns wrappers for all non-free spaces, dropping these may invalidate other
    /// presently held wrappers.
    ///
    /// `drop(x.iter().collect::<Vec<_>>())` would free all memory, invalidating any wrappers
    /// presently held.
    ///
    /// # Panics
    ///
    /// When locking the mutex fails.
    pub unsafe fn iter(&self) -> WrapperIterator<T> {
        #[cfg(feature = "log")]
        trace!("Allocator::iter");
        let head = self.0.lock().unwrap().head;
        WrapperIterator {
            allocator: self,
            free: head,
            used: 0,
        }
    }
}

#[derive(Debug)]
pub struct WrapperIterator<'a, T> {
    allocator: &'a Allocator<T>,
    free: Option<usize>,
    used: usize,
}
impl<'a, T> WrapperIterator<'a, T> {
    #[must_use]
    pub fn allocator(&self) -> &'a Allocator<T> {
        self.allocator
    }

    #[must_use]
    pub fn free(&self) -> &Option<usize> {
        &self.free
    }

    #[must_use]
    pub fn used(&self) -> &usize {
        &self.used
    }
}
impl<'a, T> Iterator for WrapperIterator<'a, T> {
    type Item = Wrapper<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        let inner_guard = self.allocator.0.lock().unwrap();
        let inner = &*inner_guard;
        loop {
            let free = self.free.unwrap_or(inner.size);
            println!("free: {free}");
            println!("self.used: {}", self.used);
            if self.used < free {
                let temp = self.used;
                self.used += 1;
                break Some(Wrapper {
                    allocator: self.allocator,
                    index: temp,
                });
            }
            println!("free: {free}");
            println!("self.used: {}", self.used);
            println!("inner.size: {}", inner.size);
            if self.used == inner.size {
                break None;
            }

            debug_assert_eq!(self.used, free);
            debug_assert!(
                unsafe { inner.data().as_ref()[free].empty.unwrap_or(inner.size) } > free
            );
            // println!("inner.data().as_ref()[free].empty: {:?}",
            // unsafe{inner.data().as_ref()[free].empty}); println!("inner.data().
            // as_ref()[free+1].empty: {:?}", unsafe{inner.data().as_ref()[free+1].empty});
            // println!("inner.data().as_ref()[free+2].empty: {:?}",
            // unsafe{inner.data().as_ref()[free+2].empty});
            self.free = unsafe { inner.data().as_ref()[free].empty };
            self.used = free + 1;
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
#[repr(C)]
pub struct InnerAllocator<T> {
    head: Option<usize>,
    size: usize,
    _marker: PhantomData<T>,
}

use std::ptr::NonNull;

#[allow(clippy::needless_range_loop)]
impl<T> InnerAllocator<T> {
    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    ///
    /// # Panics
    ///
    /// When `&self == std::ptr::null()`.
    #[must_use]
    pub unsafe fn data(&self) -> NonNull<[Block<T>]> {
        #[cfg(feature = "log")]
        trace!("InnerAllocator::data");

        std::ptr::NonNull::slice_from_raw_parts(
            NonNull::new((self as *const Self as *mut Self).add(1).cast()).unwrap(),
            self.size,
        )
    }

    unsafe fn init(ptr: *mut Self, size: usize) {
        #[cfg(feature = "log")]
        trace!("InnerAllocator::init");

        if size > 0 {
            #[cfg(feature = "log")]
            trace!("InnerAllocator::init non-empty");

            (*ptr).head = Some(0);
            (*ptr).size = size;

            #[cfg(feature = "log")]
            trace!("InnerAllocator::init head written");

            let data_ref = (*ptr).data().as_mut();
            for i in 0..(size - 1) {
                // println!("inner data: {:#?}",(*ptr).data().as_ref());
                data_ref[i] = Block { empty: Some(i + 1) };
            }
            data_ref[size - 1] = Block { empty: None };
        } else {
            #[cfg(feature = "log")]
            trace!("InnerAllocator::init empty");

            (*ptr).head = None;
            (*ptr).size = size;
        }
    }
}

#[repr(C)]
pub union Block<T> {
    empty: Option<usize>,
    full: ManuallyDrop<T>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for Block<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(feature = "log")]
        trace!("Block::fmt");

        f.debug_struct("Block")
            .field("empty", unsafe { &self.empty })
            .field("full", unsafe { &self.full })
            .finish()
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Wrapper<'a, T> {
    allocator: &'a Allocator<T>,
    index: usize,
}

impl<'a, T> Wrapper<'a, T> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator<T> {
        #[cfg(feature = "log")]
        trace!("Wrapper::allocator");

        self.allocator
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn allocator_mut(&mut self) -> &mut &'a Allocator<T> {
        #[cfg(feature = "log")]
        trace!("Wrapper::allocator_mut");

        &mut self.allocator
    }

    #[must_use]
    pub fn index(&self) -> usize {
        #[cfg(feature = "log")]
        trace!("Wrapper::index");

        self.index
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn index_mut(&mut self) -> &mut usize {
        #[cfg(feature = "log")]
        trace!("Wrapper::index_mut");

        &mut self.index
    }
}

impl<'a, T> Drop for Wrapper<'a, T> {
    fn drop(&mut self) {
        #[cfg(feature = "log")]
        trace!("Wrapper::drop");

        let mut inner_allocator_guard = self.allocator.0.lock().unwrap();
        // To avoid a massive number of mutex deref calls we deref here.
        let inner_allocator = &mut *inner_allocator_guard;
        let data = unsafe { inner_allocator.data().as_mut() };

        if let Some(head) = inner_allocator.head {
            debug_assert_ne!(head, self.index);
            if head > self.index {
                unsafe {
                    ManuallyDrop::drop(&mut data[self.index].full);
                }
                data[self.index] = Block { empty: Some(head) };
                inner_allocator.head = Some(self.index);
            } else {
                debug_assert!(head < self.index);
                let mut current = head;

                loop {
                    match unsafe { data[current].empty } {
                        None => {
                            unsafe {
                                ManuallyDrop::drop(&mut data[self.index].full);
                            }
                            data[self.index] = Block { empty: None };
                            data[current].empty = Some(self.index);
                            break;
                        }
                        Some(next) if next > self.index => {
                            unsafe {
                                ManuallyDrop::drop(&mut data[self.index].full);
                            }
                            data[self.index] = Block { empty: Some(next) };
                            data[current].empty = Some(self.index);
                            break;
                        }
                        Some(next) => {
                            debug_assert!(next < self.index);
                            current = next;
                        }
                    }
                }
            }
        } else {
            unsafe {
                ManuallyDrop::drop(&mut data[self.index].full);
            }
            inner_allocator.head = Some(self.index);
            data[self.index] = Block { empty: None };
        }
    }
}

impl<'a, T> Deref for Wrapper<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        #[cfg(feature = "log")]
        trace!("Wrapper::deref");

        // We circumvent acquiring a guard as we don't need to lock to safely dereference allocated
        // memory.

        let inner_allocator = unsafe { &*self.allocator.0.get() };

        unsafe { &inner_allocator.data().as_ref()[self.index].full }
    }
}
impl<'a, T> DerefMut for Wrapper<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[cfg(feature = "log")]
        trace!("Wrapper::deref_mut");

        // We circumvent acquiring a guard as we don't need to lock to safely dereference allocated
        // memory.

        let inner_allocator = unsafe { &mut *self.allocator.0.get() };

        unsafe { &mut inner_allocator.data().as_mut()[self.index].full }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::pedantic)]

    use std::mem::forget;
    use std::time::{Duration, Instant};

    use rand::Rng;

    use super::*;

    fn forget_wrapper<T: Copy + std::fmt::Debug>(x: Option<Wrapper<T>>) -> Option<T> {
        let y = x.as_ref().map(|x| **x);
        forget(x);
        y
    }

    #[test]
    fn slab_1() {
        const SIZE: usize = 10;
        let memory = ArrayAllocator::<SIZE, u8>::new(None);
        const X: u8 = 1;

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(0),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(slice[0].empty, Some(1));
            assert_eq!(slice[1].empty, Some(2));
            assert_eq!(slice[2].empty, Some(3));
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(5));
            assert_eq!(slice[5].empty, Some(6));
            assert_eq!(slice[6].empty, Some(7));
            assert_eq!(slice[7].empty, Some(8));
            assert_eq!(slice[8].empty, Some(9));
            assert_eq!(slice[9].empty, None);
        }

        let a = memory.allocate(X).unwrap();

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(1),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(slice[1].empty, Some(2));
            assert_eq!(slice[2].empty, Some(3));
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(5));
            assert_eq!(slice[5].empty, Some(6));
            assert_eq!(slice[6].empty, Some(7));
            assert_eq!(slice[7].empty, Some(8));
            assert_eq!(slice[8].empty, Some(9));
            assert_eq!(slice[9].empty, None);
        }

        let b = memory.allocate(X).unwrap();

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(2),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(*slice[1].full, X);
            assert_eq!(slice[2].empty, Some(3));
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(5));
            assert_eq!(slice[5].empty, Some(6));
            assert_eq!(slice[6].empty, Some(7));
            assert_eq!(slice[7].empty, Some(8));
            assert_eq!(slice[8].empty, Some(9));
            assert_eq!(slice[9].empty, None);
        }

        let c = memory.allocate(X).unwrap();

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(3),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(*slice[1].full, X);
            assert_eq!(*slice[2].full, X);
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(5));
            assert_eq!(slice[5].empty, Some(6));
            assert_eq!(slice[6].empty, Some(7));
            assert_eq!(slice[7].empty, Some(8));
            assert_eq!(slice[8].empty, Some(9));
            assert_eq!(slice[9].empty, None);
        }

        let d = memory.allocate(X).unwrap();

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(4),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(*slice[1].full, X);
            assert_eq!(*slice[2].full, X);
            assert_eq!(*slice[3].full, X);
            assert_eq!(slice[4].empty, Some(5));
            assert_eq!(slice[5].empty, Some(6));
            assert_eq!(slice[6].empty, Some(7));
            assert_eq!(slice[7].empty, Some(8));
            assert_eq!(slice[8].empty, Some(9));
            assert_eq!(slice[9].empty, None);
        }

        let e = memory.allocate(X).unwrap();

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(5),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(*slice[1].full, X);
            assert_eq!(*slice[2].full, X);
            assert_eq!(*slice[3].full, X);
            assert_eq!(*slice[4].full, X);
            assert_eq!(slice[5].empty, Some(6));
            assert_eq!(slice[6].empty, Some(7));
            assert_eq!(slice[7].empty, Some(8));
            assert_eq!(slice[8].empty, Some(9));
            assert_eq!(slice[9].empty, None);
        }

        let f = memory.allocate(X).unwrap();

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(6),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(*slice[1].full, X);
            assert_eq!(*slice[2].full, X);
            assert_eq!(*slice[3].full, X);
            assert_eq!(*slice[4].full, X);
            assert_eq!(*slice[5].full, X);
            assert_eq!(slice[6].empty, Some(7));
            assert_eq!(slice[7].empty, Some(8));
            assert_eq!(slice[8].empty, Some(9));
            assert_eq!(slice[9].empty, None);
        }

        let g = memory.allocate(X).unwrap();

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(7),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(*slice[1].full, X);
            assert_eq!(*slice[2].full, X);
            assert_eq!(*slice[3].full, X);
            assert_eq!(*slice[4].full, X);
            assert_eq!(*slice[5].full, X);
            assert_eq!(*slice[6].full, X);
            assert_eq!(slice[7].empty, Some(8));
            assert_eq!(slice[8].empty, Some(9));
            assert_eq!(slice[9].empty, None);
        }

        let h = memory.allocate(X).unwrap();

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(8),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(*slice[1].full, X);
            assert_eq!(*slice[2].full, X);
            assert_eq!(*slice[3].full, X);
            assert_eq!(*slice[4].full, X);
            assert_eq!(*slice[5].full, X);
            assert_eq!(*slice[6].full, X);
            assert_eq!(*slice[7].full, X);
            assert_eq!(slice[8].empty, Some(9));
            assert_eq!(slice[9].empty, None);
        }

        let i = memory.allocate(X).unwrap();

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(9),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(*slice[1].full, X);
            assert_eq!(*slice[2].full, X);
            assert_eq!(*slice[3].full, X);
            assert_eq!(*slice[4].full, X);
            assert_eq!(*slice[5].full, X);
            assert_eq!(*slice[6].full, X);
            assert_eq!(*slice[7].full, X);
            assert_eq!(*slice[8].full, X);
            assert_eq!(slice[9].empty, None);
        }

        let j = memory.allocate(X).unwrap();

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: None,
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(*slice[1].full, X);
            assert_eq!(*slice[2].full, X);
            assert_eq!(*slice[3].full, X);
            assert_eq!(*slice[4].full, X);
            assert_eq!(*slice[5].full, X);
            assert_eq!(*slice[6].full, X);
            assert_eq!(*slice[7].full, X);
            assert_eq!(*slice[8].full, X);
            assert_eq!(*slice[9].full, X);
        }

        drop(b);

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(1),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(slice[1].empty, None);
            assert_eq!(*slice[2].full, X);
            assert_eq!(*slice[3].full, X);
            assert_eq!(*slice[4].full, X);
            assert_eq!(*slice[5].full, X);
            assert_eq!(*slice[6].full, X);
            assert_eq!(*slice[7].full, X);
            assert_eq!(*slice[8].full, X);
            assert_eq!(*slice[9].full, X);
        }

        drop(d);

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(1),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(slice[1].empty, Some(3));
            assert_eq!(*slice[2].full, X);
            assert_eq!(slice[3].empty, None);
            assert_eq!(*slice[4].full, X);
            assert_eq!(*slice[5].full, X);
            assert_eq!(*slice[6].full, X);
            assert_eq!(*slice[7].full, X);
            assert_eq!(*slice[8].full, X);
            assert_eq!(*slice[9].full, X);
        }

        drop(e);

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(1),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(slice[1].empty, Some(3));
            assert_eq!(*slice[2].full, X);
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, None);
            assert_eq!(*slice[5].full, X);
            assert_eq!(*slice[6].full, X);
            assert_eq!(*slice[7].full, X);
            assert_eq!(*slice[8].full, X);
            assert_eq!(*slice[9].full, X);
        }

        drop(i);

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(1),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(*slice[0].full, X);
            assert_eq!(slice[1].empty, Some(3));
            assert_eq!(*slice[2].full, X);
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(8));
            assert_eq!(*slice[5].full, X);
            assert_eq!(*slice[6].full, X);
            assert_eq!(*slice[7].full, X);
            assert_eq!(slice[8].empty, None);
            assert_eq!(*slice[9].full, X);
        }

        drop(a);

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(0),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(slice[0].empty, Some(1));
            assert_eq!(slice[1].empty, Some(3));
            assert_eq!(*slice[2].full, X);
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(8));
            assert_eq!(*slice[5].full, X);
            assert_eq!(*slice[6].full, X);
            assert_eq!(*slice[7].full, X);
            assert_eq!(slice[8].empty, None);
            assert_eq!(*slice[9].full, X);
        }

        drop(c);

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(0),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(slice[0].empty, Some(1));
            assert_eq!(slice[1].empty, Some(2));
            assert_eq!(slice[2].empty, Some(3));
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(8));
            assert_eq!(*slice[5].full, X);
            assert_eq!(*slice[6].full, X);
            assert_eq!(*slice[7].full, X);
            assert_eq!(slice[8].empty, None);
            assert_eq!(*slice[9].full, X);
        }

        drop(f);

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(0),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(slice[0].empty, Some(1));
            assert_eq!(slice[1].empty, Some(2));
            assert_eq!(slice[2].empty, Some(3));
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(5));
            assert_eq!(slice[5].empty, Some(8));
            assert_eq!(*slice[6].full, X);
            assert_eq!(*slice[7].full, X);
            assert_eq!(slice[8].empty, None);
            assert_eq!(*slice[9].full, X);
        }

        drop(g);

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(0),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(slice[0].empty, Some(1));
            assert_eq!(slice[1].empty, Some(2));
            assert_eq!(slice[2].empty, Some(3));
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(5));
            assert_eq!(slice[5].empty, Some(6));
            assert_eq!(slice[6].empty, Some(8));
            assert_eq!(*slice[7].full, X);
            assert_eq!(slice[8].empty, None);
            assert_eq!(*slice[9].full, X);
        }

        drop(h);

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(0),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(slice[0].empty, Some(1));
            assert_eq!(slice[1].empty, Some(2));
            assert_eq!(slice[2].empty, Some(3));
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(5));
            assert_eq!(slice[5].empty, Some(6));
            assert_eq!(slice[6].empty, Some(7));
            assert_eq!(slice[7].empty, Some(8));
            assert_eq!(slice[8].empty, None);
            assert_eq!(*slice[9].full, X);
        }

        drop(j);

        unsafe {
            let guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(0),
                    size: SIZE,
                    _marker: PhantomData
                }
            );
            let slice = guard.data().as_ref();
            assert_eq!(slice[0].empty, Some(1));
            assert_eq!(slice[1].empty, Some(2));
            assert_eq!(slice[2].empty, Some(3));
            assert_eq!(slice[3].empty, Some(4));
            assert_eq!(slice[4].empty, Some(5));
            assert_eq!(slice[5].empty, Some(6));
            assert_eq!(slice[6].empty, Some(7));
            assert_eq!(slice[7].empty, Some(8));
            assert_eq!(slice[8].empty, Some(9));
            assert_eq!(slice[9].empty, None);
        }
    }

    #[test]
    fn slab_2() {
        const SIZE: usize = 100;
        const MAX: usize = 1_000_000;

        let memory = ArrayAllocator::<SIZE, u64>::new(None);

        let mut rng = rand::thread_rng();
        // Vector to store allocated items.
        let mut items = Vec::with_capacity(SIZE);

        let now = Instant::now();

        let mut allocated = 0;
        let mut allocate_overall = Duration::ZERO;
        let mut dropped = 0;
        let mut drop_overall = Duration::ZERO;
        for _ in 0..MAX {
            if rng.gen() {
                let x = memory.allocate(u64::default());
                let now = Instant::now();
                items.push(x);
                allocate_overall += now.elapsed();
                allocated += 1;
            } else {
                let x = items.pop();
                let now = Instant::now();
                drop(x);
                drop_overall += now.elapsed();
                dropped += 1;
            }
            // if i % 10_000 == 0 {
            //     println!("{:.2}%",100f32 * (i as f32) / (MAX  as f32));
            // }
        }
        println!("elapsed: {:?}", now.elapsed());
        println!("elapsed: {:?}", allocate_overall.div_f64(allocated as f64));
        println!("elapsed: {:?}", drop_overall.div_f64(dropped as f64));
    }

    #[test]
    fn wrapper_iterator_debug() {
        let memory = ArrayAllocator::<0, ()>::new(None);

        let expected = "WrapperIterator { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { \
                        .. }), data: UnsafeCell { .. } }), free: None, used: 0 }";
        assert_eq!(format!("{:?}", unsafe { memory.iter() }), expected);
    }

    #[test]
    fn wrapper_iterator() {
        let memory = ArrayAllocator::<10, u8>::new(None);
        const X: u8 = 1;

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(0));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        let a = memory.allocate(X).unwrap();

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(1));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        let b = memory.allocate(X).unwrap();

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(2));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        let c = memory.allocate(X).unwrap();

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(3));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        let d = memory.allocate(X).unwrap();

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(4));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        let e = memory.allocate(X).unwrap();

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(5));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        let f = memory.allocate(X).unwrap();

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(6));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        let g = memory.allocate(X).unwrap();

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(7));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        let h = memory.allocate(X).unwrap();

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(8));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        let i = memory.allocate(X).unwrap();

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(9));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        let j = memory.allocate(X).unwrap();

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), None);
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        drop(b);

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(1));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        drop(d);

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(1));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        drop(e);

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(1));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        drop(i);

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(1));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        drop(a);

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(0));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        drop(c);

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(0));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        drop(f);

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(0));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        drop(g);

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(0));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        drop(h);

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(0));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), Some(X));
            assert_eq!(forget_wrapper(iter.next()), None);
        }

        drop(j);

        {
            let mut iter = unsafe { memory.iter() };
            assert_eq!(*iter.free(), Some(0));
            assert_eq!(*iter.used(), 0);
            assert_eq!(forget_wrapper(iter.next()), None);
        }
    }

    #[test]
    fn inner_allocator_debug() {
        assert_eq!(
            format!("{:?}", ArrayAllocator::<0, ()>::new(None)),
            "ArrayAllocator { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. }), data: \
             UnsafeCell { .. } }), data: [] }"
        );
    }

    #[test]
    fn inner_allocator_default() {
        let _ = ArrayAllocator::<0, ()>::new(None);
    }

    #[test]
    fn block_debug() {
        assert_eq!(
            format!("{:?}", Block::<u8> { empty: None }),
            "Block { empty: None, full: ManuallyDrop { value: 0 } }"
        );
    }

    #[test]
    fn wrapper_debug() {
        let allocator = ArrayAllocator::<1, ()>::new(None);
        let wrapper = allocator.allocate(()).unwrap();

        let expected = "Wrapper { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. }), \
                        data: UnsafeCell { .. } }), index: 0 }";

        assert_eq!(format!("{wrapper:?}"), expected);
    }

    #[test]
    fn wrapper_allocator() {
        let allocator = ArrayAllocator::<1, ()>::new(None);
        let wrapper = allocator.allocate(()).unwrap();
        let _ = wrapper.allocator();
    }

    #[test]
    fn wrapper_index() {
        let allocator = ArrayAllocator::<1, ()>::new(None);
        let wrapper = allocator.allocate(()).unwrap();
        assert_eq!(wrapper.index(), 0);
    }

    #[test]
    fn wrapper_deref() {
        let allocator = ArrayAllocator::<1, u8>::new(None);
        let wrapper = allocator.allocate(0).unwrap();
        assert_eq!(*wrapper, 0);
    }

    #[test]
    fn wrapper_deref_mut() {
        let allocator = ArrayAllocator::<1, u8>::new(None);
        let mut wrapper = allocator.allocate(0).unwrap();
        assert_eq!(*wrapper, 0);
        *wrapper = 1;
        assert_eq!(*wrapper, 1);
    }

    #[test]
    fn allocator_debug() {
        let expected = "ArrayAllocator { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. \
                        }), data: UnsafeCell { .. } }), data: [] }";

        assert_eq!(
            format!("{:?}", ArrayAllocator::<0, ()>::new(None)),
            expected
        );
    }

    // `None` head
    #[test]
    fn drop_0() {
        let memory = ArrayAllocator::<1, ()>::new(None);
        memory.allocate(()).unwrap();
    }
    // `head > self.index`
    #[test]
    fn drop_1() {
        let memory = ArrayAllocator::<2, ()>::new(None);
        memory.allocate(()).unwrap();
    }
    // `head < self.index`
    #[test]
    fn drop_2() {
        let memory = ArrayAllocator::<3, ()>::new(None);
        let a = memory.allocate(()).unwrap();
        let b = memory.allocate(()).unwrap();
        drop(a);
        drop(b);
        drop(memory);
    }
    // `head < self.index` and `Some(next) = unsafe { inner_allocator.data[current].empty }`
    #[test]
    fn drop_3() {
        let memory = ArrayAllocator::<4, ()>::new(None);
        let a = memory.allocate(()).unwrap();
        let b = memory.allocate(()).unwrap();
        let c = memory.allocate(()).unwrap();
        drop(a);
        drop(b);
        drop(c);
        drop(memory);
    }
}
