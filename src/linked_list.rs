use std::cmp::Ordering;
use std::marker::PhantomData;
use std::mem::size_of;
use std::ops::{Deref, DerefMut, Drop};

use super::Mutex;

#[derive(Debug)]
#[repr(C)]
pub struct Allocator<const N: usize>(Mutex<InnerLinkedListArrayAllocator<N>>);

impl<const N: usize> Allocator<N> {
    #[must_use]
    pub fn new(attr: Option<nix::sys::pthread::MutexAttr>) -> Self {
        Self(Mutex::new(InnerLinkedListArrayAllocator::default(), attr))
    }

    pub fn allocate(&self, blocks: usize) -> Option<Wrapper<N>> {
        let mut allocator = self.0.lock();

        if let Some(next) = allocator.head {
            match blocks.cmp(&allocator.data[next].size) {
                Ordering::Equal => {
                    allocator.head = allocator.data[next].next;
                    Some(Wrapper {
                        allocator: self,
                        index: next,
                        size: blocks,
                    })
                }
                Ordering::Less => {
                    let new_index = next + blocks;
                    allocator.data[new_index] = Block {
                        size: allocator.data[next].size - blocks,
                        next: allocator.data[next].next,
                    };
                    allocator.head = Some(new_index);
                    Some(Wrapper {
                        allocator: self,
                        index: next,
                        size: blocks,
                    })
                }
                Ordering::Greater => {
                    let mut next_opt = allocator.data[next].next;
                    while let Some(next) = next_opt {
                        match blocks.cmp(&allocator.data[next].size) {
                            Ordering::Equal => {
                                allocator.head = allocator.data[next].next;
                                return Some(Wrapper {
                                    allocator: self,
                                    index: next,
                                    size: blocks,
                                });
                            }
                            Ordering::Less => {
                                let new_index = next + blocks;
                                allocator.data[new_index] = Block {
                                    size: allocator.data[next].size - blocks,
                                    next: allocator.data[next].next,
                                };
                                allocator.head = Some(new_index);
                                return Some(Wrapper {
                                    allocator: self,
                                    index: next,
                                    size: blocks,
                                });
                            }
                            Ordering::Greater => {
                                next_opt = allocator.data[next].next;
                            }
                        }
                    }
                    None
                }
            }
        } else {
            None
        }
    }

    pub fn allocate_value<T>(&self) -> Option<Value<N, T>> {
        let blocks = ((size_of::<T>() as f32) / (size_of::<Block>() as f32)).ceil() as usize;
        self.allocate(blocks).map(|wrapper| Value {
            wrapper,
            __marker: PhantomData,
        })
    }

    pub fn allocate_slice<T>(&self, len: usize) -> Option<Slice<N, T>> {
        let blocks =
            (((len * size_of::<T>()) as f32) / (size_of::<Block>() as f32)).ceil() as usize;
        self.allocate(blocks).map(|wrapper| Slice {
            wrapper,
            len,
            __marker: PhantomData,
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
#[repr(C)]
struct InnerLinkedListArrayAllocator<const N: usize> {
    head: Option<usize>,
    data: [Block; N],
}
impl<const N: usize> Default for InnerLinkedListArrayAllocator<N> {
    fn default() -> Self {
        if N > 0 {
            let mut data_memory = InnerLinkedListArrayAllocator {
                head: Some(0),
                data: unsafe { std::mem::zeroed() },
            };
            unsafe {
                std::ptr::write(
                    &mut data_memory.data[0],
                    Block {
                        size: N,
                        next: None,
                    },
                );
            }
            data_memory
        } else {
            InnerLinkedListArrayAllocator {
                head: None,
                data: unsafe { std::mem::zeroed() },
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Block {
    size: usize,
    next: Option<usize>,
}

#[derive(Debug)]
#[repr(C)]
pub struct Value<'a, const N: usize, T> {
    pub wrapper: Wrapper<'a, N>,
    __marker: PhantomData<T>,
}

impl<'a, const N: usize, T> Value<'a, N, T> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator<N> {
        self.wrapper.allocator
    }

    #[must_use]
    pub fn index(&self) -> usize {
        self.wrapper.index
    }

    #[must_use]
    pub fn size(&self) -> usize {
        self.wrapper.size
    }
}

impl<'a, const N: usize, T> Deref for Value<'a, N, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*std::ptr::addr_of!(*self.wrapper).cast() }
    }
}
impl<'a, const N: usize, T> DerefMut for Value<'a, N, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *std::ptr::addr_of_mut!(*self.wrapper).cast() }
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Wrapper<'a, const N: usize> {
    allocator: &'a Allocator<N>,
    index: usize,
    size: usize,
}

impl<'a, const N: usize> Wrapper<'a, N> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator<N> {
        self.allocator
    }

    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }
}

impl<'a, const N: usize> Deref for Wrapper<'a, N> {
    type Target = [Block];

    fn deref(&self) -> &Self::Target {
        let allocator = unsafe { &*self.allocator.0.get() };
        &allocator.data[self.index..self.index + self.size]
    }
}
impl<'a, const N: usize> DerefMut for Wrapper<'a, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let allocator = unsafe { &mut *self.allocator.0.get() };
        &mut allocator.data[self.index..self.index + self.size]
    }
}

impl<'a, const N: usize> Drop for Wrapper<'a, N> {
    fn drop(&mut self) {
        if self.size == 0 {
            return;
        }
        // let self_blocks = self.block.into_inner();

        // let Wrapper
        let mut inner_allocator = self.allocator.0.lock();

        // let blocks = self.block.get_mut();
        // dbg!(&allocator);

        // ┌───┬─────┬───┐
        // │...│index│...│
        // └───┴─────┴───┘
        // If there is at least 1 free block
        if let Some(head) = inner_allocator.head {
            let end = self.index + self.size;
            match end.cmp(&head) {
                // ┌───┬────┬────┬───┐
                // │...│self│head│...│
                // └───┴────┴────┴───┘
                Ordering::Equal => {
                    inner_allocator.data[self.index] = Block {
                        size: self.size + inner_allocator.data[head].size,
                        next: inner_allocator.data[head].next,
                    };
                    inner_allocator.head = Some(self.index);
                }
                // ┌───┬────┬───┬────┬───┐
                // │...│self│...│head│...│
                // └───┴────┴───┴────┴───┘
                Ordering::Less => {
                    inner_allocator.data[self.index] = Block {
                        size: self.size,
                        next: inner_allocator.head,
                    };
                    inner_allocator.head = Some(self.index);
                }
                // ┌───┬────┬───┬────┬───┐
                // │...│head│...│self│...│
                // └───┴────┴───┴────┴───┘
                Ordering::Greater => {
                    // If `self` was allocated properly
                    let mut current_index = head;
                    loop {
                        let current_end = current_index + inner_allocator.data[current_index].size;

                        match (
                            current_end == self.index,
                            inner_allocator.data[current_index].next,
                        ) {
                            // ┌───┬─────┬────┬────┬───┐
                            // │...│index│self│next│...│
                            // └───┴─────┴────┴────┴───┘
                            // The self block starts at the current block and ends at the next
                            // block.
                            (true, Some(next_index)) if next_index == end => {
                                // Update the size and next of the current block and return.
                                inner_allocator.data[current_index].next =
                                    inner_allocator.data[next_index].next;
                                inner_allocator.data[current_index].size +=
                                    self.size + inner_allocator.data[next_index].size;
                                // ┌───┬───────────────┬───┐
                                // │...│index          │...│
                                // └───┴───────────────┴───┘
                                break;
                            }
                            // ┌───┬─────┬────┬───┬────┬───┐
                            // │...│index│self│...│next│...│
                            // └───┴─────┴────┴───┴────┴───┘
                            // The self block starts at the current block and ends before the next
                            // block.
                            (true, Some(next_index)) => {
                                // Update the size of the current block and return.
                                debug_assert!(next_index > end);
                                inner_allocator.data[current_index].size += self.size;
                                // ┌───┬──────────┬───┬────┬───┐
                                // │...│index     │...│next│...│
                                // └───┴──────────┴───┴────┴───┘
                                break;
                            }
                            // ┌───┬─────┬────┬───┐
                            // │...│index│self│...│
                            // └───┴─────┴────┴───┘
                            // The self block starts at the current block and there is no next
                            // block.
                            (true, None) => {
                                inner_allocator.data[current_index].size += self.size;
                                // ┌───┬──────────┬───┐
                                // │...│index     │...│
                                // └───┴──────────┴───┘
                                break;
                            }
                            // ┌───┬─────┬───┬────┬────┬───┐
                            // │...│index│...│self│next│...│
                            // └───┴─────┴───┴────┴────┴───┘
                            // The self block starts after the current block and ends at the next
                            // block.
                            (false, Some(next_index)) if next_index == end => {
                                // Update the size of the self block and the next of the current
                                // block.
                                inner_allocator.data[self.index] = Block {
                                    size: self.size + inner_allocator.data[next_index].size,
                                    next: inner_allocator.data[next_index].next,
                                };
                                inner_allocator.data[current_index].next = Some(self.index);
                                // ┌───┬─────┬───┬─────────┬───┐
                                // │...│index│...│self     │...│
                                // └───┴─────┴───┴─────────┴───┘
                                break;
                            }
                            // ┌───┬─────┬───┬────┬───┬────┬───┐
                            // │...│index│...│self│...│next│...│
                            // └───┴─────┴───┴────┴───┴────┴───┘
                            // The self block starts after the current block and ends before the
                            // next block.
                            (false, Some(next_index)) if next_index > end => {
                                inner_allocator.data[self.index] = Block {
                                    size: self.size,
                                    next: inner_allocator.data[current_index].next,
                                };
                                inner_allocator.data[current_index].next = Some(self.index);
                                break;
                            }
                            // ┌───┬─────┬───┬────┬───┬────┬───┐
                            // │...│index│...│next│...│self│...│
                            // └───┴─────┴───┴────┴───┴────┴───┘
                            // The self block starts after the next block.
                            (false, Some(next_index)) => {
                                debug_assert!(next_index < self.index);
                                current_index = next_index;
                                continue;
                            }
                            // ┌───┬─────┬───┬────┬───┐
                            // │...│index│...│self│...│
                            // └───┴─────┴───┴────┴───┘
                            // The self block starts after the current block and there is no next
                            // block.
                            (false, None) => {
                                inner_allocator.data[self.index] = Block {
                                    size: self.size,
                                    next: None,
                                };
                                inner_allocator.data[current_index].next = Some(self.index);
                                break;
                            }
                        }
                    }
                }
            }
        }
        // ┌───┐
        // │...│
        // └───┘
        // If there are no free blocks.
        else {
            inner_allocator.head = Some(self.index);
            inner_allocator.data[self.index] = Block {
                size: self.size,
                next: None,
            }
        }
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Slice<'a, const N: usize, T> {
    pub wrapper: Wrapper<'a, N>,
    len: usize,
    __marker: PhantomData<T>,
}

impl<'a, const N: usize, T> Slice<'a, N, T> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator<N> {
        self.wrapper.allocator
    }

    #[must_use]
    pub fn index(&self) -> usize {
        self.wrapper.index
    }

    #[must_use]
    pub fn size(&self) -> usize {
        self.wrapper.size
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn resize(&mut self, len: usize) -> Option<()> {
        let new = self.wrapper.allocator.allocate_slice(len)?;
        *self = new;
        Some(())
    }
}

impl<'a, const N: usize, T> Deref for Slice<'a, N, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        unsafe { &*std::ptr::from_raw_parts(std::ptr::addr_of!(*self.wrapper).cast(), self.len) }
    }
}
impl<'a, const N: usize, T> DerefMut for Slice<'a, N, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            &mut *std::ptr::from_raw_parts_mut(
                std::ptr::addr_of_mut!(*self.wrapper).cast(),
                self.len,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::pedantic)]

    use super::*;

    #[test]
    fn slice_debug() {
        let allocator = Allocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert_eq!(
            format!("{wrapper:?}"),
            "Slice { wrapper: Wrapper { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. \
             }), data: UnsafeCell { .. } }), index: 0, size: 1 }, len: 3, __marker: \
             PhantomData<u8> }"
        );
    }
    #[test]
    fn slice_allocator() {
        let allocator = Allocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        let _ = wrapper.allocator();
    }
    #[test]
    fn slice_index() {
        let allocator = Allocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn slice_size() {
        let allocator = Allocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert_eq!(wrapper.size(), 1);
    }
    #[test]
    fn slice_len() {
        let allocator = Allocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert_eq!(wrapper.len(), 3);
    }
    #[test]
    fn slice_is_empty() {
        let allocator = Allocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert!(!wrapper.is_empty());
    }
    #[test]
    fn slice_resize() {
        let allocator = Allocator::<5>::new(None);
        let mut wrapper = allocator.allocate_slice::<u8>(2).unwrap();
        wrapper.resize(3).unwrap();
    }
    #[test]
    fn slice_deref() {
        let allocator = Allocator::<3>::new(None);
        let mut wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        wrapper[0] = 0;
        assert_eq!(wrapper[0], 0);
        wrapper[1] = 1;
        assert_eq!(wrapper[1], 1);
        wrapper[2] = 2;
        assert_eq!(wrapper[2], 2);
    }
    #[test]
    fn slice_deref_mut() {
        let allocator = Allocator::<3>::new(None);
        let mut wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        wrapper[0] = 0;
        assert_eq!(wrapper[0], 0);
        wrapper[1] = 1;
        assert_eq!(wrapper[1], 1);
        wrapper[2] = 2;
        assert_eq!(wrapper[2], 2);
    }

    #[test]
    fn value_debug() {
        let allocator = Allocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        assert_eq!(
            format!("{wrapper:?}"),
            "Value { wrapper: Wrapper { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. \
             }), data: UnsafeCell { .. } }), index: 0, size: 1 }, __marker: PhantomData<u8> }"
        );
    }
    #[test]
    fn value_allocator() {
        let allocator = Allocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        let _ = wrapper.allocator();
    }
    #[test]
    fn value_index() {
        let allocator = Allocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn value_size() {
        let allocator = Allocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        assert_eq!(wrapper.size(), 1);
    }
    #[test]
    fn value_deref() {
        let allocator = Allocator::<1>::new(None);
        let mut wrapper = allocator.allocate_value::<u8>().unwrap();
        *wrapper = 0;
        assert_eq!(*wrapper, 0);
    }
    #[test]
    fn value_deref_mut() {
        let allocator = Allocator::<1>::new(None);
        let mut wrapper = allocator.allocate_value::<u8>().unwrap();
        *wrapper = 0;
        assert_eq!(*wrapper, 0);
    }

    #[test]
    fn inner_linked_list_array_allocator_debug() {
        assert_eq!(
            format!("{:?}", InnerLinkedListArrayAllocator::<0>::default()),
            "InnerLinkedListArrayAllocator { head: None, data: [] }"
        );
    }
    #[test]
    fn block_debug() {
        assert_eq!(
            format!(
                "{:?}",
                Block {
                    size: 0,
                    next: None
                }
            ),
            "Block { size: 0, next: None }"
        );
    }
    #[test]
    fn wrapper_debug() {
        let allocator = Allocator::<0>::new(None);
        assert_eq!(
            format!(
                "{:?}",
                Wrapper {
                    allocator: &allocator,
                    index: 0,
                    size: 0
                }
            ),
            "Wrapper { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. }), data: \
             UnsafeCell { .. } }), index: 0, size: 0 }"
        );
    }
    #[test]
    fn wrapper_allocator() {
        let allocator = Allocator::<1>::new(None);
        let wrapper = allocator.allocate(1).unwrap();
        let _ = wrapper.allocator();
    }
    #[test]
    fn wrapper_index() {
        let allocator = Allocator::<1>::new(None);
        let wrapper = allocator.allocate(1).unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn wrapper_size() {
        let allocator = Allocator::<1>::new(None);
        let wrapper = allocator.allocate(1).unwrap();
        assert_eq!(wrapper.size(), 1);
    }

    #[test]
    fn linked_list() {
        // We hold items in a vec to prevent them being dropped;
        let memory = Allocator::<5>::new(None);
        let mut vec = Vec::new();

        assert_eq!(
            *memory.0.lock(),
            InnerLinkedListArrayAllocator {
                head: Some(0),
                data: [
                    Block {
                        size: 5,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                ]
            }
        );

        vec.push(memory.allocate(1));

        assert_eq!(
            *memory.0.lock(),
            InnerLinkedListArrayAllocator {
                head: Some(1),
                data: [
                    Block {
                        size: 5,
                        next: None,
                    },
                    Block {
                        size: 4,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                ]
            }
        );

        vec.push(memory.allocate(2));

        assert_eq!(
            *memory.0.lock(),
            InnerLinkedListArrayAllocator {
                head: Some(3),
                data: [
                    Block {
                        size: 5,
                        next: None,
                    },
                    Block {
                        size: 4,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                    Block {
                        size: 2,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                ]
            }
        );

        vec.pop();

        assert_eq!(
            *memory.0.lock(),
            InnerLinkedListArrayAllocator {
                head: Some(1),
                data: [
                    Block {
                        size: 5,
                        next: None,
                    },
                    Block {
                        size: 4,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                    Block {
                        size: 2,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                ]
            }
        );

        vec.pop();

        assert_eq!(
            *memory.0.lock(),
            InnerLinkedListArrayAllocator {
                head: Some(0),
                data: [
                    Block {
                        size: 5,
                        next: None,
                    },
                    Block {
                        size: 4,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                    Block {
                        size: 2,
                        next: None,
                    },
                    Block {
                        size: 0,
                        next: None,
                    },
                ]
            }
        );

        drop(vec);
    }

    #[test]
    fn allocate_value() {
        let allocator = Allocator::<1>::new(None);
        allocator.allocate_value::<()>().unwrap();
    }
    #[test]
    fn allocate_slice() {
        let allocator = Allocator::<1>::new(None);
        allocator.allocate_slice::<u8>(size_of::<Block>()).unwrap();
    }

    #[test]
    fn linked_list_debug() {
        assert_eq!(
            format!("{:?}", Allocator::<0>::new(None)),
            "Allocator(Mutex { lock: Mutex(UnsafeCell { .. }), data: UnsafeCell { .. } })"
        );
    }

    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Equal` case.
    #[test]
    fn allocate_0() {
        let memory = Allocator::<1>::new(None);
        memory.allocate(1).unwrap();
    }
    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Less` case.
    #[test]
    fn allocate_1() {
        let memory = Allocator::<2>::new(None);
        memory.allocate(1).unwrap();
    }
    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Greater`
    // `blocks.cmp(&allocator.data[next].size) == Equal` case.
    #[test]
    fn allocate_2() {
        let memory = Allocator::<4>::new(None);
        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap();

        drop(a);

        memory.allocate(2).unwrap();

        drop(b);
    }
    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Greater`
    // `blocks.cmp(&allocator.data[next].size) == Less` case.
    #[test]
    fn allocate_3() {
        let memory = Allocator::<5>::new(None);
        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap();

        drop(a);

        memory.allocate(2).unwrap();

        drop(b);
    }
    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Greater`
    // `blocks.cmp(&allocator.data[next].size) == Greater` case.
    #[test]
    fn allocate_4() {
        let memory = Allocator::<7>::new(None);
        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap();
        let c = memory.allocate(2).unwrap();

        drop(b);

        memory.allocate(3).unwrap();

        drop(a);
        drop(c);
    }
    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Greater` `None` case.
    #[test]
    fn allocate_5() {
        let memory = Allocator::<6>::new(None);
        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap();
        let c = memory.allocate(2).unwrap();

        drop(b);

        assert!(memory.allocate(3).is_none());

        drop(a);
        drop(c);
    }
    // Tests `Wrapper::allocate` `None` header case.
    #[test]
    fn allocate_6() {
        let memory = Allocator::<0>::new(None);
        assert!(memory.allocate(1).is_none());
    }

    // Tests `Wrapper` drop case of:
    // ┌───┐
    // │...│
    // └───┘
    #[test]
    fn drop_1() {
        let memory = Allocator::<1>::new(None);
        let item = memory.allocate(1).unwrap();
        drop(item);
        drop(memory);
    }
    // Tests `Wrapper` drop case of:
    // ┌───┬────┬────┬───┐
    // │...│self│head│...│
    // └───┴────┴────┴───┘
    #[test]
    fn drop_2() {
        let memory = Allocator::<1>::new(None);
        let item = memory.allocate(1).unwrap();
        drop(item);
        drop(memory);
    }
    // Tests `Wrapper` drop case of:
    // ┌───┬────┬───┬────┬───┐
    // │...│self│...│head│...│
    // └───┴────┴───┴────┴───┘
    #[test]
    fn drop_3() {
        let memory = Allocator::<2>::new(None);

        let first = memory.allocate(1).unwrap();
        let second = memory.allocate(1).unwrap(); // self

        drop(second); // This tests our drop case.
        drop(first);

        drop(memory);
    }
    // Tests `Wrapper` drop case of:
    // ┌───┬─────┬────┬────┬───┐
    // │...│index│self│next│...│
    // └───┴─────┴────┴────┴───┘
    #[test]
    fn drop_4() {
        let memory = Allocator::<4>::new(None);

        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap(); // index
        let c = memory.allocate(1).unwrap(); // self
        let d = memory.allocate(1).unwrap(); // next

        // Dropping this results in the following drop entering the case:
        // ┌───┬────┬───┬────┬───┐
        // │...│head│...│self│...│
        // └───┴────┴───┴────┴───┘
        drop(a);

        drop(c); // This tests our drop case.

        // We don't care what order we drop these, only that we drop them after.
        drop(b);
        drop(d);

        drop(memory);
    }
    // Tests `Wrapper` drop case of:
    // ┌───┬─────┬────┬───┬────┬───┐
    // │...│index│self│...│next│...│
    // └───┴─────┴────┴───┴────┴───┘
    #[test]
    fn drop_5() {
        let memory = Allocator::<5>::new(None);

        let a = memory.allocate(1).unwrap(); // ...
        let b = memory.allocate(1).unwrap(); // index
        let c = memory.allocate(1).unwrap(); // self
        let d = memory.allocate(1).unwrap(); // ...
        let e = memory.allocate(1).unwrap(); // next

        // Dropping this results in the following drop entering the case:
        // ┌───┬────┬───┬────┬───┐
        // │...│head│...│self│...│
        // └───┴────┴───┴────┴───┘
        drop(a);
        drop(b);
        drop(e);

        drop(c); // This tests our drop case.

        drop(d);

        drop(memory);
    }
    // Tests `Wrapper` drop case of:
    // ┌───┬─────┬────┬───┐
    // │...│index│self│...│
    // └───┴─────┴────┴───┘
    #[test]
    fn drop_6() {
        let memory = Allocator::<3>::new(None);

        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap(); // index
        let c = memory.allocate(1).unwrap(); // self

        // Dropping this results in the following drop entering the case:
        // ┌───┬────┬───┬────┬───┐
        // │...│head│...│self│...│
        // └───┴────┴───┴────┴───┘
        drop(a);

        drop(c); // This tests our drop case.

        // We don't care what order we drop these, only that we drop them after.
        drop(b);

        drop(memory);
    }
    // Tests `Wrapper` drop case of:
    // ┌───┬─────┬───┬────┬────┬───┐
    // │...│index│...│self│next│...│
    // └───┴─────┴───┴────┴────┴───┘
    #[test]
    fn drop_7() {
        let memory = Allocator::<5>::new(None);

        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap(); // index
        let c = memory.allocate(1).unwrap(); // ...
        let d = memory.allocate(1).unwrap(); // self
        let e = memory.allocate(1).unwrap(); // next

        // Dropping this results in the following drop entering the case:
        // ┌───┬────┬───┬────┬───┐
        // │...│head│...│self│...│
        // └───┴────┴───┴────┴───┘
        drop(a);

        drop(c);

        drop(d); // This tests our drop case.

        // We don't care what order we drop these, only that we drop them after.
        drop(b);
        drop(e);

        drop(memory);
    }
    // Tests `Wrapper` drop case of:
    // ┌───┬─────┬───┬────┬───┬────┬───┐
    // │...│index│...│self│...│next│...│
    // └───┴─────┴───┴────┴───┴────┴───┘
    #[test]
    fn drop_8() {
        let memory = Allocator::<6>::new(None);

        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap(); // index
        let c = memory.allocate(1).unwrap(); // ...
        let d = memory.allocate(1).unwrap(); // self
        let e = memory.allocate(1).unwrap(); // ...
        let f = memory.allocate(1).unwrap(); // next

        // Dropping this results in the following drop entering the case:
        // ┌───┬────┬───┬────┬───┐
        // │...│head│...│self│...│
        // └───┴────┴───┴────┴───┘
        drop(a);

        drop(b);
        drop(f);

        drop(d); // This tests our drop case.

        // We don't care what order we drop these, only that we drop them after.
        drop(c);
        drop(e);

        drop(memory);
    }
    // Tests `Wrapper` drop case of:
    // ┌───┬─────┬───┬────┬───┬────┬───┐
    // │...│index│...│next│...│self│...│
    // └───┴─────┴───┴────┴───┴────┴───┘
    #[test]
    fn drop_9() {
        let memory = Allocator::<6>::new(None);

        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap(); // index
        let c = memory.allocate(1).unwrap(); // ...
        let d = memory.allocate(1).unwrap(); // next
        let e = memory.allocate(1).unwrap(); // ...
        let f = memory.allocate(1).unwrap(); // self

        // Dropping this results in the following drop entering the case:
        // ┌───┬────┬───┬────┬───┐
        // │...│head│...│self│...│
        // └───┴────┴───┴────┴───┘
        drop(a);

        drop(c);
        drop(e);

        drop(f); // This tests our drop case.

        // We don't care what order we drop these, only that we drop them after.
        drop(b);
        drop(d);

        drop(memory);
    }
    // Tests `Wrapper` drop case of:
    // ┌───┬─────┬───┬────┬───┐
    // │...│index│...│self│...│
    // └───┴─────┴───┴────┴───┘
    #[test]
    fn drop_10() {
        let memory = Allocator::<4>::new(None);

        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap(); // index
        let c = memory.allocate(1).unwrap(); // ...
        let d = memory.allocate(1).unwrap(); // self

        // Dropping this results in the following drop entering the case:
        // ┌───┬────┬───┬────┬───┐
        // │...│head│...│self│...│
        // └───┴────┴───┴────┴───┘
        drop(a);

        drop(c);

        drop(d); // This tests our drop case.

        // We don't care what order we drop these, only that we drop them after.
        drop(b);

        drop(memory);
    }
}
