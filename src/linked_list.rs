use std::cmp::Ordering;
use std::marker::PhantomData;
use std::mem::size_of;
use std::ops::Deref;
use std::ops::DerefMut;
use std::ops::Drop;

use super::Mutex;

#[derive(Debug)]
#[repr(C)]
pub struct LinkedListArrayAllocator<const N: usize>(Mutex<InnerLinkedListArrayAllocator<N>>);

impl<const N: usize> LinkedListArrayAllocator<N> {
    pub fn new(attr: Option<nix::sys::pthread::MutexAttr>) -> Self {
        Self(Mutex::new(InnerLinkedListArrayAllocator::default(), attr))
    }
    pub fn allocate(&self, blocks: usize) -> Option<LinkedListArrayWrapper<N>> {
        let mut allocator = self.0.lock();

        if let Some(next) = allocator.head {
            match blocks.cmp(&allocator.data[next].size) {
                Ordering::Equal => {
                    allocator.head = allocator.data[next].next;
                    Some(LinkedListArrayWrapper {
                        allocator: self,
                        index: next,
                        size: blocks,
                    })
                }
                Ordering::Less => {
                    let new_index = next + blocks;
                    allocator.data[new_index] = LinkedListArrayBlock {
                        size: allocator.data[next].size - blocks,
                        next: allocator.data[next].next,
                    };
                    allocator.head = Some(new_index);
                    Some(LinkedListArrayWrapper {
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
                                return Some(LinkedListArrayWrapper {
                                    allocator: self,
                                    index: next,
                                    size: blocks,
                                });
                            }
                            Ordering::Less => {
                                let new_index = next + blocks;
                                allocator.data[new_index] = LinkedListArrayBlock {
                                    size: allocator.data[next].size - blocks,
                                    next: allocator.data[next].next,
                                };
                                allocator.head = Some(new_index);
                                return Some(LinkedListArrayWrapper {
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
    
    pub fn allocate_value<T>(&self) -> Option<TypedLinkedListArrayWrapper<N, T>> {
        let blocks =
            ((size_of::<T>() as f32) / (size_of::<LinkedListArrayBlock>() as f32)).ceil() as usize;
        self.allocate(blocks)
            .map(|wrapper| TypedLinkedListArrayWrapper {
                wrapper,
                __marker: PhantomData,
            })
    }
    pub fn allocate_slice<T>(&self, len: usize) -> Option<TypedLinkedListArrayWrapper<N, [T]>> {
        let blocks = (((len * size_of::<T>()) as f32) / (size_of::<LinkedListArrayBlock>() as f32))
            .ceil() as usize;
        self.allocate(blocks)
            .map(|wrapper| TypedLinkedListArrayWrapper {
                wrapper,
                __marker: PhantomData,
            })
    }
}

#[derive(Debug, Eq, PartialEq)]
#[repr(C)]
struct InnerLinkedListArrayAllocator<const N: usize> {
    head: Option<usize>,
    data: [LinkedListArrayBlock; N],
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
                    LinkedListArrayBlock {
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
pub struct LinkedListArrayBlock {
    size: usize,
    next: Option<usize>,
}

pub struct TypedLinkedListArrayWrapper<'a, const N: usize, T: ?Sized> {
    pub wrapper: LinkedListArrayWrapper<'a, N>,
    __marker: PhantomData<T>,
}

impl<'a, const N: usize, T> TypedLinkedListArrayWrapper<'a, N, T> {
    pub fn allocator(&self) -> &LinkedListArrayAllocator<N> {
        self.wrapper.allocator
    }
    pub fn index(&self) -> usize {
        self.wrapper.index
    }
    pub fn size(&self) -> usize {
        self.wrapper.size
    }
}

impl<'a, const N: usize, T> Deref for TypedLinkedListArrayWrapper<'a, N, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.wrapper.deref() as *const [LinkedListArrayBlock]).cast() }
    }
}
impl<'a, const N: usize, T> DerefMut for TypedLinkedListArrayWrapper<'a, N, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *(self.wrapper.deref_mut() as *mut [LinkedListArrayBlock]).cast() }
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct LinkedListArrayWrapper<'a, const N: usize> {
    allocator: &'a LinkedListArrayAllocator<N>,
    index: usize,
    size: usize,
}

impl<'a, const N: usize> LinkedListArrayWrapper<'a, N> {
    pub fn allocator(&self) -> &LinkedListArrayAllocator<N> {
        self.allocator
    }
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn size(&self) -> usize {
        self.size
    }
}

impl<'a, const N: usize> Deref for LinkedListArrayWrapper<'a, N> {
    type Target = [LinkedListArrayBlock];

    fn deref(&self) -> &Self::Target {
        let allocator = unsafe { &*self.allocator.0.get() };
        &allocator.data[self.index..self.index + self.size]
    }
}
impl<'a, const N: usize> DerefMut for LinkedListArrayWrapper<'a, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let allocator = unsafe { &mut *self.allocator.0.get() };
        &mut allocator.data[self.index..self.index + self.size]
    }
}

impl<'a, const N: usize> Drop for LinkedListArrayWrapper<'a, N> {
    fn drop(&mut self) {
        if self.size == 0 {
            return;
        }
        // let self_blocks = self.block.into_inner();

        // let LinkedListArrayWrapper
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
                    inner_allocator.data[self.index] = LinkedListArrayBlock {
                        size: self.size + inner_allocator.data[head].size,
                        next: inner_allocator.data[head].next,
                    };
                    inner_allocator.head = Some(self.index);
                }
                // ┌───┬────┬───┬────┬───┐
                // │...│self│...│head│...│
                // └───┴────┴───┴────┴───┘
                Ordering::Less => {
                    inner_allocator.data[self.index] = LinkedListArrayBlock {
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
                            // The self block starts at the current block and ends at the next block.
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
                            // The self block starts at the current block and ends before the next block.
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
                            // The self block starts at the current block and there is no next block.
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
                            // The self block starts after the current block and ends at the next block.
                            (false, Some(next_index)) if next_index == end => {
                                // Update the size of the self block and the next of the current block.
                                inner_allocator.data[self.index] = LinkedListArrayBlock {
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
                            // The self block starts after the current block and ends before the next block.
                            (false, Some(next_index)) if next_index > end => {
                                inner_allocator.data[self.index] = LinkedListArrayBlock {
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
                            // The self block starts after the current block and there is no next block.
                            (false, None) => {
                                inner_allocator.data[self.index] = LinkedListArrayBlock {
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
            inner_allocator.data[self.index] = LinkedListArrayBlock {
                size: self.size,
                next: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::pedantic)]

    use super::*;

    #[test]
    fn typed_linked_list_array_wrapper_allocator() {
        let allocator = LinkedListArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        wrapper.allocator();
    }

    #[test]
    fn typed_linked_list_array_wrapper_index() {
        let allocator = LinkedListArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn typed_linked_list_array_wrapper_size() {
        let allocator = LinkedListArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        assert_eq!(wrapper.size(), 1);
    }

    #[test]
    fn typed_linked_list_array_wrapper_deref() {
        let allocator = LinkedListArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<()>().unwrap();
        assert_eq!(*wrapper, ());
    }
    #[test]
    fn typed_linked_list_array_wrapper_deref_mut() {
        let allocator = LinkedListArrayAllocator::<1>::new(None);
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
    fn linked_list_array_block_debug() {
        assert_eq!(
            format!(
                "{:?}",
                LinkedListArrayBlock {
                    size: 0,
                    next: None
                }
            ),
            "LinkedListArrayBlock { size: 0, next: None }"
        );
    }

    #[test]
    fn linked_list_array_wrapper_debug() {
        let allocator = LinkedListArrayAllocator::<0>::new(None);
        assert_eq!(format!("{:?}",LinkedListArrayWrapper { allocator: &allocator, index:0,size: 0}),"LinkedListArrayWrapper { allocator: LinkedListArrayAllocator(Mutex { lock: Mutex(UnsafeCell { .. }), data: UnsafeCell { .. } }), index: 0, size: 0 }");
    }

    #[test]
    fn linked_list_array_wrapper_allocator() {
        let allocator = LinkedListArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate(1).unwrap();
        wrapper.allocator();
    }

    #[test]
    fn linked_list_array_wrapper_index() {
        let allocator = LinkedListArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate(1).unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn linked_list_array_wrapper_size() {
        let allocator = LinkedListArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate(1).unwrap();
        assert_eq!(wrapper.size(), 1);
    }

    #[test]
    fn linked_list() {
        // We hold items in a vec to prevent them being dropped;
        let memory = LinkedListArrayAllocator::<5>::new(None);
        let mut vec = Vec::new();

        assert_eq!(
            *memory.0.lock(),
            InnerLinkedListArrayAllocator {
                head: Some(0),
                data: [
                    LinkedListArrayBlock {
                        size: 5,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 0,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 0,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 0,
                        next: None,
                    },
                    LinkedListArrayBlock {
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
                    LinkedListArrayBlock {
                        size: 5,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 4,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 0,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 0,
                        next: None,
                    },
                    LinkedListArrayBlock {
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
                    LinkedListArrayBlock {
                        size: 5,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 4,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 0,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 2,
                        next: None,
                    },
                    LinkedListArrayBlock {
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
                    LinkedListArrayBlock {
                        size: 5,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 4,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 0,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 2,
                        next: None,
                    },
                    LinkedListArrayBlock {
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
                    LinkedListArrayBlock {
                        size: 5,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 4,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 0,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 2,
                        next: None,
                    },
                    LinkedListArrayBlock {
                        size: 0,
                        next: None,
                    },
                ]
            }
        );

        drop(vec);
    }

    #[test]
    fn linked_list_allocate_value() {
        let allocator = LinkedListArrayAllocator::<1>::new(None);
        allocator.allocate_value::<()>().unwrap();
    }
    #[test]
    fn linked_list_allocate_slice() {
        let allocator = LinkedListArrayAllocator::<1>::new(None);
        allocator
            .allocate_slice::<u8>(size_of::<LinkedListArrayBlock>())
            .unwrap();
    }

    #[test]
    fn linked_list_debug() {
        assert_eq!(format!("{:?}", LinkedListArrayAllocator::<0>::new(None)),"LinkedListArrayAllocator(Mutex { lock: Mutex(UnsafeCell { .. }), data: UnsafeCell { .. } })");
    }

    // Tests `LinkedListArrayWrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Equal` case.
    #[test]
    fn linked_list_allocate_0() {
        let memory = LinkedListArrayAllocator::<1>::new(None);
        memory.allocate(1).unwrap();
    }
    // Tests `LinkedListArrayWrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Less` case.
    #[test]
    fn linked_list_allocate_1() {
        let memory = LinkedListArrayAllocator::<2>::new(None);
        memory.allocate(1).unwrap();
    }
    // Tests `LinkedListArrayWrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Greater` `blocks.cmp(&allocator.data[next].size) == Equal` case.
    #[test]
    fn linked_list_allocate_2() {
        let memory = LinkedListArrayAllocator::<4>::new(None);
        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap();

        drop(a);

        memory.allocate(2).unwrap();

        drop(b);
    }
    // Tests `LinkedListArrayWrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Greater` `blocks.cmp(&allocator.data[next].size) == Less` case.
    #[test]
    fn linked_list_allocate_3() {
        let memory = LinkedListArrayAllocator::<5>::new(None);
        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap();

        drop(a);

        memory.allocate(2).unwrap();

        drop(b);
    }
    // Tests `LinkedListArrayWrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Greater` `blocks.cmp(&allocator.data[next].size) == Greater` case.
    #[test]
    fn linked_list_allocate_4() {
        let memory = LinkedListArrayAllocator::<7>::new(None);
        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap();
        let c = memory.allocate(2).unwrap();

        drop(b);

        memory.allocate(3).unwrap();

        drop(a);
        drop(c);
    }
    // Tests `LinkedListArrayWrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Greater` `None` case.
    #[test]
    fn linked_list_allocate_5() {
        let memory = LinkedListArrayAllocator::<6>::new(None);
        let a = memory.allocate(1).unwrap();
        let b = memory.allocate(1).unwrap();
        let c = memory.allocate(2).unwrap();

        drop(b);

        assert!(memory.allocate(3).is_none());

        drop(a);
        drop(c);
    }
    // Tests `LinkedListArrayWrapper::allocate` `None` header case.
    #[test]
    fn linked_list_allocate_6() {
        let memory = LinkedListArrayAllocator::<0>::new(None);
        assert!(memory.allocate(1).is_none());
    }

    // Tests `LinkedListArrayWrapper` drop case of:
    // ┌───┐
    // │...│
    // └───┘
    #[test]
    fn linked_list_drop_1() {
        let memory = LinkedListArrayAllocator::<1>::new(None);
        let item = memory.allocate(1).unwrap();
        drop(item);
        drop(memory);
    }
    // Tests `LinkedListArrayWrapper` drop case of:
    // ┌───┬────┬────┬───┐
    // │...│self│head│...│
    // └───┴────┴────┴───┘
    #[test]
    fn linked_list_drop_2() {
        let memory = LinkedListArrayAllocator::<1>::new(None);
        let item = memory.allocate(1).unwrap();
        drop(item);
        drop(memory);
    }
    // Tests `LinkedListArrayWrapper` drop case of:
    // ┌───┬────┬───┬────┬───┐
    // │...│self│...│head│...│
    // └───┴────┴───┴────┴───┘
    #[test]
    fn linked_list_drop_3() {
        let memory = LinkedListArrayAllocator::<2>::new(None);

        let first = memory.allocate(1).unwrap();
        let second = memory.allocate(1).unwrap(); // self

        drop(second); // This tests our drop case.
        drop(first);

        drop(memory);
    }
    // Tests `LinkedListArrayWrapper` drop case of:
    // ┌───┬─────┬────┬────┬───┐
    // │...│index│self│next│...│
    // └───┴─────┴────┴────┴───┘
    #[test]
    fn linked_list_drop_4() {
        let memory = LinkedListArrayAllocator::<4>::new(None);

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
    // Tests `LinkedListArrayWrapper` drop case of:
    // ┌───┬─────┬────┬───┬────┬───┐
    // │...│index│self│...│next│...│
    // └───┴─────┴────┴───┴────┴───┘
    #[test]
    fn linked_list_drop_5() {
        let memory = LinkedListArrayAllocator::<5>::new(None);

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
    // Tests `LinkedListArrayWrapper` drop case of:
    // ┌───┬─────┬────┬───┐
    // │...│index│self│...│
    // └───┴─────┴────┴───┘
    #[test]
    fn linked_list_drop_6() {
        let memory = LinkedListArrayAllocator::<3>::new(None);

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
    // Tests `LinkedListArrayWrapper` drop case of:
    // ┌───┬─────┬───┬────┬────┬───┐
    // │...│index│...│self│next│...│
    // └───┴─────┴───┴────┴────┴───┘
    #[test]
    fn linked_list_drop_7() {
        let memory = LinkedListArrayAllocator::<5>::new(None);

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
    // Tests `LinkedListArrayWrapper` drop case of:
    // ┌───┬─────┬───┬────┬───┬────┬───┐
    // │...│index│...│self│...│next│...│
    // └───┴─────┴───┴────┴───┴────┴───┘
    #[test]
    fn linked_list_drop_8() {
        let memory = LinkedListArrayAllocator::<6>::new(None);

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
    // Tests `LinkedListArrayWrapper` drop case of:
    // ┌───┬─────┬───┬────┬───┬────┬───┐
    // │...│index│...│next│...│self│...│
    // └───┴─────┴───┴────┴───┴────┴───┘
    #[test]
    fn linked_list_drop_9() {
        let memory = LinkedListArrayAllocator::<6>::new(None);

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
    // Tests `LinkedListArrayWrapper` drop case of:
    // ┌───┬─────┬───┬────┬───┐
    // │...│index│...│self│...│
    // └───┴─────┴───┴────┴───┘
    #[test]
    fn linked_list_drop_10() {
        let memory = LinkedListArrayAllocator::<4>::new(None);

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
