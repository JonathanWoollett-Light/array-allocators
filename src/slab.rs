use std::mem::ManuallyDrop;
use std::ops::Drop;

use super::Mutex;

#[repr(C)]
pub struct Allocator<const N: usize, T>(Mutex<InnerSlabArrayAllocator<N, T>>);

impl<const N: usize, T> Allocator<N, T> {
    #[must_use]
    pub fn new(attr: Option<nix::sys::pthread::MutexAttr>) -> Self {
        Self(Mutex::new(InnerSlabArrayAllocator::default(), attr))
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
}

#[repr(C)]
struct InnerSlabArrayAllocator<const N: usize, T> {
    head: Option<usize>,
    data: [Block<T>; N],
}
#[allow(clippy::needless_range_loop)]
impl<const N: usize, T> Default for InnerSlabArrayAllocator<N, T> {
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
                while let Some(next) = unsafe { inner_allocator.data[current].empty } {
                    if next > self.index {
                        unsafe {
                            ManuallyDrop::drop(&mut inner_allocator.data[self.index].full);
                        }
                        inner_allocator.data[self.index] = Block { empty: Some(next) };
                        inner_allocator.data[current].empty = Some(self.index);
                        break;
                    }
                    current = next;
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
    fn slab() {
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
    fn inner_slab_array_allocator_default_zero() {
        let _ = InnerSlabArrayAllocator::<0, ()>::default();
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
