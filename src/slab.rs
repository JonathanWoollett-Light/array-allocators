use std::mem::ManuallyDrop;
use std::ops::Drop;

use super::Mutex;

#[repr(C)]
pub struct SlabArrayAllocator<const N: usize, T>(Mutex<InnerSlabArrayAllocator<N, T>>);

impl<const N: usize, T> SlabArrayAllocator<N, T> {
    pub fn new(attr: Option<nix::sys::pthread::MutexAttr>) -> Self {
        Self(Mutex::new(InnerSlabArrayAllocator::default(), attr))
    }
    pub fn allocate(&self, x: T) -> Option<SlabArrayWrapper<N, T>> {
        let mut inner_allocator = self.0.lock();
        if let Some(head) = inner_allocator.head {
            let index = head;
            inner_allocator.head = unsafe { inner_allocator.data[index].empty };
            inner_allocator.data[index] = SlabArrayBlock {
                full: ManuallyDrop::new(x),
            };
            Some(SlabArrayWrapper {
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
    data: [SlabArrayBlock<T>; N],
}
#[allow(clippy::needless_range_loop)]
impl<const N: usize, T> Default for InnerSlabArrayAllocator<N, T> {
    fn default() -> Self {
        if N > 0 {
            let mut data: [SlabArrayBlock<T>; N] = unsafe { std::mem::zeroed() };
            for i in 0..(N - 1) {
                data[i] = SlabArrayBlock { empty: Some(i + 1) };
            }
            data[N - 1] = SlabArrayBlock { empty: None };

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
union SlabArrayBlock<T> {
    empty: Option<usize>,
    full: ManuallyDrop<T>,
}

#[repr(C)]
pub struct SlabArrayWrapper<'a, const N: usize, T> {
    allocator: &'a SlabArrayAllocator<N, T>,
    index: usize,
}

impl<'a, const N: usize, T> SlabArrayWrapper<'a, N, T> {
    pub fn allocator(&self) -> &SlabArrayAllocator<N, T> {
        self.allocator
    }
    pub fn index(&self) -> usize {
        self.index
    }
}

impl<'a, const N: usize, T> Drop for SlabArrayWrapper<'a, N, T> {
    fn drop(&mut self) {
        let mut inner_allocator = self.allocator.0.lock();

        if let Some(head) = inner_allocator.head {
            debug_assert_ne!(head, self.index);
            if head > self.index {
                unsafe {
                    ManuallyDrop::drop(&mut inner_allocator.data[self.index].full);
                }
                inner_allocator.data[self.index] = SlabArrayBlock { empty: Some(head) };
                inner_allocator.head = Some(self.index);
            } else {
                debug_assert!(head < self.index);
                let mut current = head;
                while let Some(next) = unsafe { inner_allocator.data[current].empty } {
                    if next > self.index {
                        unsafe {
                            ManuallyDrop::drop(&mut inner_allocator.data[self.index].full);
                        }
                        inner_allocator.data[self.index] = SlabArrayBlock { empty: Some(next) };
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
            inner_allocator.data[self.index] = SlabArrayBlock { empty: None };
        }
    }
}

impl<'a, const N: usize, T> std::ops::Deref for SlabArrayWrapper<'a, N, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let allocator = unsafe { &*self.allocator.0.get() };
        unsafe { &*(&allocator.data[self.index] as *const SlabArrayBlock<T>).cast() }
    }
}
impl<'a, const N: usize, T> std::ops::DerefMut for SlabArrayWrapper<'a, N, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let allocator = unsafe { &mut *self.allocator.0.get() };
        unsafe { &mut *(&mut allocator.data[self.index] as *mut SlabArrayBlock<T>).cast() }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::pedantic)]

    use super::*;

    use rand::Rng;
    use std::time::{Duration, Instant};

    #[test]
    fn slab() {
        const SIZE: usize = 100;
        const MAX: usize = 1_000_000;

        let memory = SlabArrayAllocator::<SIZE, u64>::new(None);

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
    fn slab_array_wrapper_allocator() {
        let allocator = SlabArrayAllocator::<1, ()>::new(None);
        let wrapper = allocator.allocate(()).unwrap();
        wrapper.allocator();
    }

    #[test]
    fn slab_array_wrapper_index() {
        let allocator = SlabArrayAllocator::<1, ()>::new(None);
        let wrapper = allocator.allocate(()).unwrap();
        assert_eq!(wrapper.index(), 0);
    }

    #[test]
    fn slab_array_wrapper_deref() {
        let allocator = SlabArrayAllocator::<1, ()>::new(None);
        let wrapper = allocator.allocate(()).unwrap();
        assert_eq!(*wrapper, ());
    }

    #[test]
    fn slab_array_wrapper_deref_mut() {
        let allocator = SlabArrayAllocator::<1, u8>::new(None);
        let mut wrapper = allocator.allocate(0).unwrap();
        assert_eq!(*wrapper, 0);
        *wrapper = 1;
        assert_eq!(*wrapper, 1);
    }

    // `None` head
    #[test]
    fn slab_drop_0() {
        let memory = SlabArrayAllocator::<1, ()>::new(None);
        memory.allocate(()).unwrap();
    }
    // `head > self.index`
    #[test]
    fn slab_drop_1() {
        let memory = SlabArrayAllocator::<2, ()>::new(None);
        memory.allocate(()).unwrap();
    }
    // `head < self.index`
    #[test]
    fn slab_drop_2() {
        let memory = SlabArrayAllocator::<3, ()>::new(None);
        let a = memory.allocate(()).unwrap();
        let b = memory.allocate(()).unwrap();
        drop(a);
        drop(b);
        drop(memory);
    }
    // `head < self.index` and `Some(next) = unsafe { inner_allocator.data[current].empty }`
    #[test]
    fn slab_drop_3() {
        let memory = SlabArrayAllocator::<4, ()>::new(None);
        let a = memory.allocate(()).unwrap();
        let b = memory.allocate(()).unwrap();
        let c = memory.allocate(()).unwrap();
        drop(a);
        drop(b);
        drop(c);
        drop(memory);
    }
}
