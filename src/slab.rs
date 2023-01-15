use std::mem::ManuallyDrop;
use std::ops::Drop;

use super::Mutex;

#[derive(Debug)]
#[repr(C)]
pub struct Allocator<const N: usize, T>(Mutex<InnerAllocator<N, T>>);

impl<const N: usize, T> Allocator<N, T> {
    #[must_use]
    pub fn new(attr: Option<nix::sys::pthread::MutexAttr>) -> Self {
        Self(Mutex::new(InnerAllocator::default(), attr))
    }

    pub fn allocate(&self, x: T) -> Option<Wrapper<N, T>> {
        let mut inner_allocator = self.0.lock();
        if let Some(head) = inner_allocator.head {
            let index = head;
            inner_allocator.head = unsafe { inner_allocator.data[index].empty };
            inner_allocator.data[index] = Block {
                full: ManuallyDrop::new(x),
            };
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
    pub unsafe fn iter(&self) -> WrapperIterator<N, T> {
        let head = self.0.lock().head;
        WrapperIterator {
            allocator: self,
            free: head,
            used: 0,
        }
    }
}

#[derive(Debug)]
pub struct WrapperIterator<'a, const N: usize, T> {
    allocator: &'a Allocator<N, T>,
    free: Option<usize>,
    used: usize,
}
impl<'a, const N: usize, T> Iterator for WrapperIterator<'a, N, T> {
    type Item = Wrapper<'a, N, T>;

    fn next(&mut self) -> Option<Self::Item> {
        let inner = self.allocator.0.lock();
        loop {
            let free = self.free.unwrap_or(N);
            if self.used < free {
                let temp = self.used;
                self.used += 1;
                break Some(Wrapper {
                    allocator: self.allocator,
                    index: temp,
                });
            }
            if self.used == N {
                break None;
            }
            debug_assert_eq!(self.used, free);
            self.free = unsafe { inner.data[free].empty };
            self.used = free + 1;
        }
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct InnerAllocator<const N: usize, T> {
    head: Option<usize>,
    data: [Block<T>; N],
}

#[allow(clippy::needless_range_loop)]
impl<const N: usize, T> Default for InnerAllocator<N, T> {
    fn default() -> Self {
        if N > 0 {
            let mut data: [Block<T>; N] = unsafe { std::mem::zeroed() };
            for i in 0..(N - 1) {
                data[i] = Block { empty: Some(i + 1) };
            }
            data[N - 1] = Block { empty: None };

            Self {
                head: Some(0),
                data,
            }
        } else {
            Self {
                head: None,
                data: unsafe { std::mem::zeroed() },
            }
        }
    }
}

#[repr(C)]
union Block<T> {
    empty: Option<usize>,
    full: ManuallyDrop<T>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for Block<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Block")
            .field("empty", unsafe { &self.empty })
            .field("full", unsafe { &self.full })
            .finish()
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Wrapper<'a, const N: usize, T> {
    allocator: &'a Allocator<N, T>,
    index: usize,
}

impl<'a, const N: usize, T> Wrapper<'a, N, T> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator<N, T> {
        self.allocator
    }

    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }
}

impl<'a, const N: usize, T> Drop for Wrapper<'a, N, T> {
    fn drop(&mut self) {
        let mut inner_allocator = self.allocator.0.lock();

        if let Some(head) = inner_allocator.head {
            debug_assert_ne!(head, self.index);
            if head > self.index {
                unsafe {
                    ManuallyDrop::drop(&mut inner_allocator.data[self.index].full);
                }
                inner_allocator.data[self.index] = Block { empty: Some(head) };
                inner_allocator.head = Some(self.index);
            } else {
                debug_assert!(head < self.index);
                let mut current = head;

                loop {
                    match unsafe { inner_allocator.data[current].empty } {
                        None => {
                            unsafe {
                                ManuallyDrop::drop(&mut inner_allocator.data[self.index].full);
                            }
                            inner_allocator.data[self.index] = Block { empty: None };
                            inner_allocator.data[current].empty = Some(self.index);
                            break;
                        }
                        Some(next) if next > self.index => {
                            unsafe {
                                ManuallyDrop::drop(&mut inner_allocator.data[self.index].full);
                            }
                            inner_allocator.data[self.index] = Block { empty: Some(next) };
                            inner_allocator.data[current].empty = Some(self.index);
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
                ManuallyDrop::drop(&mut inner_allocator.data[self.index].full);
            }
            inner_allocator.head = Some(self.index);
            inner_allocator.data[self.index] = Block { empty: None };
        }
    }
}

impl<'a, const N: usize, T> std::ops::Deref for Wrapper<'a, N, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let allocator = unsafe { &*self.allocator.0.get() };
        unsafe { &*std::ptr::addr_of!(allocator.data[self.index]).cast() }
    }
}
impl<'a, const N: usize, T> std::ops::DerefMut for Wrapper<'a, N, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let allocator = unsafe { &mut *self.allocator.0.get() };

        unsafe { &mut *std::ptr::addr_of_mut!(allocator.data[self.index]).cast() }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::pedantic)]

    use std::time::{Duration, Instant};

    use rand::Rng;

    use super::*;

    #[test]
    fn slab_1() {
        let memory = Allocator::<10, u8>::new(None);
        const X: u8 = 1;

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(0));
            unsafe {
                assert_eq!(inner.data[0].empty, Some(1));
                assert_eq!(inner.data[1].empty, Some(2));
                assert_eq!(inner.data[2].empty, Some(3));
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(5));
                assert_eq!(inner.data[5].empty, Some(6));
                assert_eq!(inner.data[6].empty, Some(7));
                assert_eq!(inner.data[7].empty, Some(8));
                assert_eq!(inner.data[8].empty, Some(9));
                assert_eq!(inner.data[9].empty, None);
            }
        }

        let a = memory.allocate(X).unwrap();

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(1));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(inner.data[1].empty, Some(2));
                assert_eq!(inner.data[2].empty, Some(3));
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(5));
                assert_eq!(inner.data[5].empty, Some(6));
                assert_eq!(inner.data[6].empty, Some(7));
                assert_eq!(inner.data[7].empty, Some(8));
                assert_eq!(inner.data[8].empty, Some(9));
                assert_eq!(inner.data[9].empty, None);
            }
        }

        let b = memory.allocate(X).unwrap();

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(2));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(*inner.data[1].full, X);
                assert_eq!(inner.data[2].empty, Some(3));
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(5));
                assert_eq!(inner.data[5].empty, Some(6));
                assert_eq!(inner.data[6].empty, Some(7));
                assert_eq!(inner.data[7].empty, Some(8));
                assert_eq!(inner.data[8].empty, Some(9));
                assert_eq!(inner.data[9].empty, None);
            }
        }

        let c = memory.allocate(X).unwrap();

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(3));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(*inner.data[1].full, X);
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(5));
                assert_eq!(inner.data[5].empty, Some(6));
                assert_eq!(inner.data[6].empty, Some(7));
                assert_eq!(inner.data[7].empty, Some(8));
                assert_eq!(inner.data[8].empty, Some(9));
                assert_eq!(inner.data[9].empty, None);
            }
        }

        let d = memory.allocate(X).unwrap();

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(4));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(*inner.data[1].full, X);
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(*inner.data[3].full, X);
                assert_eq!(inner.data[4].empty, Some(5));
                assert_eq!(inner.data[5].empty, Some(6));
                assert_eq!(inner.data[6].empty, Some(7));
                assert_eq!(inner.data[7].empty, Some(8));
                assert_eq!(inner.data[8].empty, Some(9));
                assert_eq!(inner.data[9].empty, None);
            }
        }

        let e = memory.allocate(X).unwrap();

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(5));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(*inner.data[1].full, X);
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(*inner.data[3].full, X);
                assert_eq!(*inner.data[4].full, X);
                assert_eq!(inner.data[5].empty, Some(6));
                assert_eq!(inner.data[6].empty, Some(7));
                assert_eq!(inner.data[7].empty, Some(8));
                assert_eq!(inner.data[8].empty, Some(9));
                assert_eq!(inner.data[9].empty, None);
            }
        }

        let f = memory.allocate(X).unwrap();

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(6));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(*inner.data[1].full, X);
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(*inner.data[3].full, X);
                assert_eq!(*inner.data[4].full, X);
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(inner.data[6].empty, Some(7));
                assert_eq!(inner.data[7].empty, Some(8));
                assert_eq!(inner.data[8].empty, Some(9));
                assert_eq!(inner.data[9].empty, None);
            }
        }

        let g = memory.allocate(X).unwrap();

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(7));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(*inner.data[1].full, X);
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(*inner.data[3].full, X);
                assert_eq!(*inner.data[4].full, X);
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(inner.data[7].empty, Some(8));
                assert_eq!(inner.data[8].empty, Some(9));
                assert_eq!(inner.data[9].empty, None);
            }
        }

        let h = memory.allocate(X).unwrap();

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(8));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(*inner.data[1].full, X);
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(*inner.data[3].full, X);
                assert_eq!(*inner.data[4].full, X);
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(inner.data[8].empty, Some(9));
                assert_eq!(inner.data[9].empty, None);
            }
        }

        let i = memory.allocate(X).unwrap();

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(9));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(*inner.data[1].full, X);
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(*inner.data[3].full, X);
                assert_eq!(*inner.data[4].full, X);
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(*inner.data[8].full, X);
                assert_eq!(inner.data[9].empty, None);
            }
        }

        let j = memory.allocate(X).unwrap();

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, None);
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(*inner.data[1].full, X);
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(*inner.data[3].full, X);
                assert_eq!(*inner.data[4].full, X);
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(*inner.data[8].full, X);
                assert_eq!(*inner.data[9].full, X);
            }
        }

        drop(b);

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(1));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(inner.data[1].empty, None);
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(*inner.data[3].full, X);
                assert_eq!(*inner.data[4].full, X);
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(*inner.data[8].full, X);
                assert_eq!(*inner.data[9].full, X);
            }
        }

        drop(d);

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(1));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(inner.data[1].empty, Some(3));
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(inner.data[3].empty, None);
                assert_eq!(*inner.data[4].full, X);
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(*inner.data[8].full, X);
                assert_eq!(*inner.data[9].full, X);
            }
        }

        drop(e);

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(1));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(inner.data[1].empty, Some(3));
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, None);
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(*inner.data[8].full, X);
                assert_eq!(*inner.data[9].full, X);
            }
        }

        drop(i);

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(1));
            unsafe {
                assert_eq!(*inner.data[0].full, X);
                assert_eq!(inner.data[1].empty, Some(3));
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(8));
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(inner.data[8].empty, None);
                assert_eq!(*inner.data[9].full, X);
            }
        }

        drop(a);

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(0));
            unsafe {
                assert_eq!(inner.data[0].empty, Some(1));
                assert_eq!(inner.data[1].empty, Some(3));
                assert_eq!(*inner.data[2].full, X);
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(8));
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(inner.data[8].empty, None);
                assert_eq!(*inner.data[9].full, X);
            }
        }

        drop(c);

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(0));
            unsafe {
                assert_eq!(inner.data[0].empty, Some(1));
                assert_eq!(inner.data[1].empty, Some(2));
                assert_eq!(inner.data[2].empty, Some(3));
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(8));
                assert_eq!(*inner.data[5].full, X);
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(inner.data[8].empty, None);
                assert_eq!(*inner.data[9].full, X);
            }
        }

        drop(f);

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(0));
            unsafe {
                assert_eq!(inner.data[0].empty, Some(1));
                assert_eq!(inner.data[1].empty, Some(2));
                assert_eq!(inner.data[2].empty, Some(3));
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(5));
                assert_eq!(inner.data[5].empty, Some(8));
                assert_eq!(*inner.data[6].full, X);
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(inner.data[8].empty, None);
                assert_eq!(*inner.data[9].full, X);
            }
        }

        drop(g);

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(0));
            unsafe {
                assert_eq!(inner.data[0].empty, Some(1));
                assert_eq!(inner.data[1].empty, Some(2));
                assert_eq!(inner.data[2].empty, Some(3));
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(5));
                assert_eq!(inner.data[5].empty, Some(6));
                assert_eq!(inner.data[6].empty, Some(8));
                assert_eq!(*inner.data[7].full, X);
                assert_eq!(inner.data[8].empty, None);
                assert_eq!(*inner.data[9].full, X);
            }
        }

        drop(h);

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(0));
            unsafe {
                assert_eq!(inner.data[0].empty, Some(1));
                assert_eq!(inner.data[1].empty, Some(2));
                assert_eq!(inner.data[2].empty, Some(3));
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(5));
                assert_eq!(inner.data[5].empty, Some(6));
                assert_eq!(inner.data[6].empty, Some(7));
                assert_eq!(inner.data[7].empty, Some(8));
                assert_eq!(inner.data[8].empty, None);
                assert_eq!(*inner.data[9].full, X);
            }
        }

        drop(j);

        {
            let inner = memory.0.lock();
            assert_eq!(inner.head, Some(0));
            unsafe {
                assert_eq!(inner.data[0].empty, Some(1));
                assert_eq!(inner.data[1].empty, Some(2));
                assert_eq!(inner.data[2].empty, Some(3));
                assert_eq!(inner.data[3].empty, Some(4));
                assert_eq!(inner.data[4].empty, Some(5));
                assert_eq!(inner.data[5].empty, Some(6));
                assert_eq!(inner.data[6].empty, Some(7));
                assert_eq!(inner.data[7].empty, Some(8));
                assert_eq!(inner.data[8].empty, Some(9));
                assert_eq!(inner.data[9].empty, None);
            }
        }
    }

    #[test]
    fn slab_2() {
        const SIZE: usize = 100;
        const MAX: usize = 1_000_000;

        let memory = Allocator::<SIZE, u64>::new(None);

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
        let memory = Allocator::<0, ()>::new(None);
        assert_eq!(
            format!("{:?}", unsafe { memory.iter() }),
            "WrapperIterator { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. }), data: \
             UnsafeCell { .. } }), free: None, used: 0 }"
        );
    }

    #[test]
    fn wrapper_iterator() {
        let memory = Allocator::<10, u8>::new(None);
        const X: u8 = 1;

        let wrappers_1 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert!(wrappers_1.is_empty());

        let a = memory.allocate(X).unwrap();

        let wrappers_2 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_2.len(), 1);
        assert_eq!(*wrappers_2[0], X);

        let b = memory.allocate(X).unwrap();

        let wrappers_3 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_3.len(), 2);
        assert_eq!(*wrappers_3[0], X);
        assert_eq!(*wrappers_3[1], X);

        let c = memory.allocate(X).unwrap();

        let wrappers_4 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_4.len(), 3);
        assert_eq!(*wrappers_4[0], X);
        assert_eq!(*wrappers_4[1], X);
        assert_eq!(*wrappers_4[2], X);

        let d = memory.allocate(X).unwrap();

        let wrappers_5 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_5.len(), 4);
        assert_eq!(*wrappers_5[0], X);
        assert_eq!(*wrappers_5[1], X);
        assert_eq!(*wrappers_5[2], X);
        assert_eq!(*wrappers_5[3], X);

        let e = memory.allocate(X).unwrap();

        let wrappers_6 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_6.len(), 5);
        assert_eq!(*wrappers_6[0], X);
        assert_eq!(*wrappers_6[1], X);
        assert_eq!(*wrappers_6[2], X);
        assert_eq!(*wrappers_6[3], X);
        assert_eq!(*wrappers_6[4], X);

        let f = memory.allocate(X).unwrap();

        let wrappers_7 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_7.len(), 6);
        assert_eq!(*wrappers_7[0], X);
        assert_eq!(*wrappers_7[1], X);
        assert_eq!(*wrappers_7[2], X);
        assert_eq!(*wrappers_7[3], X);
        assert_eq!(*wrappers_7[4], X);
        assert_eq!(*wrappers_7[5], X);

        let g = memory.allocate(X).unwrap();

        let wrappers_8 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_8.len(), 7);
        assert_eq!(*wrappers_8[0], X);
        assert_eq!(*wrappers_8[1], X);
        assert_eq!(*wrappers_8[2], X);
        assert_eq!(*wrappers_8[3], X);
        assert_eq!(*wrappers_8[4], X);
        assert_eq!(*wrappers_8[5], X);
        assert_eq!(*wrappers_8[6], X);

        let h = memory.allocate(X).unwrap();

        let wrappers_9 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_9.len(), 8);
        assert_eq!(*wrappers_9[0], X);
        assert_eq!(*wrappers_9[1], X);
        assert_eq!(*wrappers_9[2], X);
        assert_eq!(*wrappers_9[3], X);
        assert_eq!(*wrappers_9[4], X);
        assert_eq!(*wrappers_9[5], X);
        assert_eq!(*wrappers_9[6], X);
        assert_eq!(*wrappers_9[7], X);

        let i = memory.allocate(X).unwrap();

        let wrappers_10 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_10.len(), 9);
        assert_eq!(*wrappers_10[0], X);
        assert_eq!(*wrappers_10[1], X);
        assert_eq!(*wrappers_10[2], X);
        assert_eq!(*wrappers_10[3], X);
        assert_eq!(*wrappers_10[4], X);
        assert_eq!(*wrappers_10[5], X);
        assert_eq!(*wrappers_10[6], X);
        assert_eq!(*wrappers_10[7], X);
        assert_eq!(*wrappers_10[8], X);

        let j = memory.allocate(X).unwrap();

        let wrappers_11 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_11.len(), 10);
        assert_eq!(*wrappers_11[0], X);
        assert_eq!(*wrappers_11[1], X);
        assert_eq!(*wrappers_11[2], X);
        assert_eq!(*wrappers_11[3], X);
        assert_eq!(*wrappers_11[4], X);
        assert_eq!(*wrappers_11[5], X);
        assert_eq!(*wrappers_11[6], X);
        assert_eq!(*wrappers_11[7], X);
        assert_eq!(*wrappers_11[8], X);
        assert_eq!(*wrappers_11[9], X);

        drop(b);

        let wrappers_12 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_12.len(), 9);
        assert_eq!(*wrappers_12[0], X);
        assert_eq!(*wrappers_12[1], X);
        assert_eq!(*wrappers_12[2], X);
        assert_eq!(*wrappers_12[3], X);
        assert_eq!(*wrappers_12[4], X);
        assert_eq!(*wrappers_12[5], X);
        assert_eq!(*wrappers_12[6], X);
        assert_eq!(*wrappers_12[7], X);
        assert_eq!(*wrappers_12[8], X);

        drop(d);

        let wrappers_13 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_13.len(), 8);
        assert_eq!(*wrappers_13[0], X);
        assert_eq!(*wrappers_13[1], X);
        assert_eq!(*wrappers_13[2], X);
        assert_eq!(*wrappers_13[3], X);
        assert_eq!(*wrappers_13[4], X);
        assert_eq!(*wrappers_13[5], X);
        assert_eq!(*wrappers_13[6], X);
        assert_eq!(*wrappers_13[7], X);

        drop(e);

        let wrappers_14 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_14.len(), 7);
        assert_eq!(*wrappers_14[0], X);
        assert_eq!(*wrappers_14[1], X);
        assert_eq!(*wrappers_14[2], X);
        assert_eq!(*wrappers_14[3], X);
        assert_eq!(*wrappers_14[4], X);
        assert_eq!(*wrappers_14[5], X);
        assert_eq!(*wrappers_14[6], X);

        drop(i);

        let wrappers_15 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_15.len(), 6);
        assert_eq!(*wrappers_15[0], X);
        assert_eq!(*wrappers_15[1], X);
        assert_eq!(*wrappers_15[2], X);
        assert_eq!(*wrappers_15[3], X);
        assert_eq!(*wrappers_15[4], X);
        assert_eq!(*wrappers_15[5], X);

        drop(a);

        let wrappers_16 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_16.len(), 5);
        assert_eq!(*wrappers_16[0], X);
        assert_eq!(*wrappers_16[1], X);
        assert_eq!(*wrappers_16[2], X);
        assert_eq!(*wrappers_16[3], X);
        assert_eq!(*wrappers_16[4], X);

        drop(c);

        let wrappers_17 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_17.len(), 4);
        assert_eq!(*wrappers_17[0], X);
        assert_eq!(*wrappers_17[1], X);
        assert_eq!(*wrappers_17[2], X);
        assert_eq!(*wrappers_17[3], X);

        drop(f);

        let wrappers_18 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_18.len(), 3);
        assert_eq!(*wrappers_18[0], X);
        assert_eq!(*wrappers_18[1], X);
        assert_eq!(*wrappers_18[2], X);

        drop(g);

        let wrappers_19 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_19.len(), 2);
        assert_eq!(*wrappers_19[0], X);
        assert_eq!(*wrappers_19[1], X);

        drop(h);

        let wrappers_20 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert_eq!(wrappers_20.len(), 1);
        assert_eq!(*wrappers_20[0], X);

        drop(j);

        let wrappers_20 = unsafe { ManuallyDrop::new(memory.iter().collect::<Vec<_>>()) };
        assert!(wrappers_20.is_empty());
    }

    #[test]
    fn inner_allocator_debug() {
        assert_eq!(
            format!("{:?}", InnerAllocator::<0, ()>::default()),
            "InnerAllocator { head: None, data: [] }"
        );
    }

    #[test]
    fn inner_allocator_default() {
        let _ = InnerAllocator::<0, ()>::default();
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
        let allocator = Allocator::<1, ()>::new(None);
        let wrapper = allocator.allocate(()).unwrap();
        assert_eq!(
            format!("{wrapper:?}"),
            "Wrapper { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. }), data: \
             UnsafeCell { .. } }), index: 0 }"
        );
    }

    #[test]
    fn wrapper_allocator() {
        let allocator = Allocator::<1, ()>::new(None);
        let wrapper = allocator.allocate(()).unwrap();
        let _ = wrapper.allocator();
    }

    #[test]
    fn wrapper_index() {
        let allocator = Allocator::<1, ()>::new(None);
        let wrapper = allocator.allocate(()).unwrap();
        assert_eq!(wrapper.index(), 0);
    }

    #[test]
    fn wrapper_deref() {
        let allocator = Allocator::<1, u8>::new(None);
        let wrapper = allocator.allocate(0).unwrap();
        assert_eq!(*wrapper, 0);
    }

    #[test]
    fn wrapper_deref_mut() {
        let allocator = Allocator::<1, u8>::new(None);
        let mut wrapper = allocator.allocate(0).unwrap();
        assert_eq!(*wrapper, 0);
        *wrapper = 1;
        assert_eq!(*wrapper, 1);
    }

    #[test]
    fn allocator_debug() {
        assert_eq!(
            format!("{:?}", Allocator::<0, ()>::new(None)),
            "Allocator(Mutex { lock: Mutex(UnsafeCell { .. }), data: UnsafeCell { .. } })"
        );
    }

    // `None` head
    #[test]
    fn drop_0() {
        let memory = Allocator::<1, ()>::new(None);
        memory.allocate(()).unwrap();
    }
    // `head > self.index`
    #[test]
    fn drop_1() {
        let memory = Allocator::<2, ()>::new(None);
        memory.allocate(()).unwrap();
    }
    // `head < self.index`
    #[test]
    fn drop_2() {
        let memory = Allocator::<3, ()>::new(None);
        let a = memory.allocate(()).unwrap();
        let b = memory.allocate(()).unwrap();
        drop(a);
        drop(b);
        drop(memory);
    }
    // `head < self.index` and `Some(next) = unsafe { inner_allocator.data[current].empty }`
    #[test]
    fn drop_3() {
        let memory = Allocator::<4, ()>::new(None);
        let a = memory.allocate(()).unwrap();
        let b = memory.allocate(()).unwrap();
        let c = memory.allocate(()).unwrap();
        drop(a);
        drop(b);
        drop(c);
        drop(memory);
    }
}
