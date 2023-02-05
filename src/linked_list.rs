use std::cmp::Ordering;
use std::marker::PhantomData;
use std::mem::size_of;
use std::ops::{Deref, DerefMut, Drop};

#[cfg(feature = "log")]
use log::trace;

#[cfg(feature = "repr_c")]
#[derive(Debug)]
#[repr(C)]
pub struct Allocator<const N: usize>(super::mutex::Mutex<InnerAllocator<N>>);

#[cfg(not(feature = "repr_c"))]
#[derive(Debug)]
pub struct Allocator<const N: usize>(std::sync::Mutex<InnerAllocator<N>>);

impl<const N: usize> Allocator<N> {
    #[cfg(feature = "repr_c")]
    #[must_use]
    pub fn new(attr: Option<nix::sys::pthread::MutexAttr>) -> Self {
        #[cfg(feature = "log")]
        trace!("Allocator::new");
        Self(super::mutex::Mutex::new(InnerAllocator::default(), attr))
    }

    #[cfg(not(feature = "repr_c"))]
    #[must_use]
    pub fn new() -> Self {
        #[cfg(feature = "log")]
        trace!("Allocator::new");
        Self(std::sync::Mutex::new(InnerAllocator::default()))
    }

    /// Initializes `Self` at `ptr`.
    ///
    /// # Safety
    ///
    /// `ptr` must be valid.
    ///
    /// # Panics
    ///
    /// When failing to initialize the inner mutex.
    #[cfg(feature = "repr_c")]
    pub unsafe fn init(ptr: *mut Self, attr: Option<nix::sys::pthread::MutexAttr>) {
        #[cfg(feature = "log")]
        trace!("Allocator::init");
        (*ptr).0.lock = nix::sys::pthread::Mutex::new(attr).unwrap();

        #[cfg(feature = "log")]
        trace!("Allocator::init 2");
        <InnerAllocator<N>>::init((*ptr).0.get());
    }

    /// Allocates a given number of blocks.
    ///
    /// # Panics
    ///
    /// When locking the mutex fails.
    pub fn allocate(&self, blocks: usize) -> Option<Wrapper<N>> {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate");

        if blocks == 0 {
            return Some(Wrapper {
                allocator: self,
                index: 0,
                size: 0,
            });
        }

        let mut allocator_guard = self.0.lock().unwrap();
        let allocator = &mut *allocator_guard;

        let rtn = if let Some(next) = allocator.head {
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
                    loop {
                        if let Some(next) = next_opt {
                            match blocks.cmp(&allocator.data[next].size) {
                                Ordering::Equal => {
                                    allocator.head = allocator.data[next].next;
                                    break Some(Wrapper {
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
                                    break Some(Wrapper {
                                        allocator: self,
                                        index: next,
                                        size: blocks,
                                    });
                                }
                                Ordering::Greater => {
                                    next_opt = allocator.data[next].next;
                                }
                            }
                        } else {
                            break None;
                        }
                    }
                }
            }
        } else {
            None
        };

        drop(allocator_guard);

        rtn
    }

    pub fn allocate_value<T>(&self) -> Option<Value<N, T>> {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate_value");
        let blocks = size_of::<T>().div_ceil(size_of::<Block>());
        self.allocate(blocks).map(|wrapper| Value {
            wrapper,
            __marker: PhantomData,
        })
    }

    /// Allocates `[T]`.
    ///
    /// # Panics
    ///
    /// When locking the mutex fails.
    pub fn allocate_slice<T>(&self, len: usize) -> Option<Slice<N, T>> {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate_slice");

        let blocks = (len * size_of::<T>()).div_ceil(size_of::<Block>());

        debug_assert!(blocks * size_of::<Block>() >= len * size_of::<T>());
        #[cfg(debug_assertions)]
        if len == 0 {
            assert_eq!(blocks, 0);
        }

        self.allocate(blocks).map(|wrapper| Slice {
            wrapper,
            len,
            __marker: PhantomData,
        })
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    #[cfg(feature = "repr_c")]
    pub unsafe fn inner(&self) -> &super::mutex::Mutex<InnerAllocator<N>> {
        &self.0
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    #[cfg(feature = "repr_c")]
    pub unsafe fn inner_mut(&mut self) -> &mut super::mutex::Mutex<InnerAllocator<N>> {
        &mut self.0
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    #[cfg(not(feature = "repr_c"))]
    pub unsafe fn inner(&self) -> &std::sync::Mutex<InnerAllocator<N>> {
        &self.0
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    #[cfg(not(feature = "repr_c"))]
    pub unsafe fn inner_mut(&mut self) -> &mut std::sync::Mutex<InnerAllocator<N>> {
        &mut self.0
    }
}

impl<const N: usize> Default for Allocator<N> {
    fn default() -> Self {
        #[cfg(feature = "repr_c")]
        let rtn = Self::new(None);

        #[cfg(not(feature = "repr_c"))]
        let rtn = Self::new();

        rtn
    }
}

#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(feature = "repr_c", repr(C))]
pub struct InnerAllocator<const N: usize> {
    head: Option<usize>,
    data: [Block; N],
}

#[cfg(feature = "repr_c")]
impl<const N: usize> InnerAllocator<N> {
    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    #[must_use]
    pub unsafe fn head(&self) -> &Option<usize> {
        #[cfg(feature = "log")]
        trace!("InnerAllocator::head");

        &self.head
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn head_mut(&mut self) -> &mut Option<usize> {
        #[cfg(feature = "log")]
        trace!("InnerAllocator::head_mut");

        &mut self.head
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    #[must_use]
    pub unsafe fn data(&self) -> &[Block; N] {
        #[cfg(feature = "log")]
        trace!("InnerAllocator::data");

        &self.data
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn data_mut(&mut self) -> &mut [Block; N] {
        #[cfg(feature = "log")]
        trace!("InnerAllocator::data_mut");

        &mut self.data
    }

    unsafe fn init(ptr: *mut Self) {
        #[cfg(feature = "log")]
        trace!("InnerAllocator::init");

        if N > 0 {
            #[cfg(feature = "log")]
            trace!("InnerAllocator::init non-empty");

            (*ptr).head = Some(0);

            #[cfg(feature = "log")]
            trace!("InnerAllocator::init head written");

            let data_ref = &mut (*ptr).data;
            data_ref[0] = Block {
                size: N,
                next: None,
            };
        } else {
            #[cfg(feature = "log")]
            trace!("InnerAllocator::init empty");

            (*ptr).head = None;
        }
    }
}

impl<const N: usize> Default for InnerAllocator<N> {
    fn default() -> Self {
        #[cfg(feature = "log")]
        trace!("InnerAllocator::default");

        if N > 0 {
            #[cfg(feature = "log")]
            trace!("InnerAllocator::default non-empty");

            let mut data_memory = InnerAllocator {
                head: Some(0),
                data: unsafe { std::mem::zeroed() },
            };

            #[cfg(feature = "log")]
            trace!("InnerAllocator::default head written");

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
            #[cfg(feature = "log")]
            trace!("InnerAllocator::default empty");

            InnerAllocator {
                head: None,
                data: unsafe { std::mem::zeroed() },
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(feature = "repr_c", repr(C))]
pub struct Block {
    size: usize,
    next: Option<usize>,
}

#[derive(Debug)]
#[cfg_attr(feature = "repr_c", repr(C))]
pub struct Value<'a, const N: usize, T> {
    pub wrapper: Wrapper<'a, N>,
    __marker: PhantomData<T>,
}

impl<'a, const N: usize, T> Value<'a, N, T> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator<N> {
        #[cfg(feature = "log")]
        trace!("Value::allocator");

        self.wrapper.allocator
    }

    #[must_use]
    pub fn index(&self) -> usize {
        #[cfg(feature = "log")]
        trace!("Value::index");

        self.wrapper.index
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn index_mut(&mut self) -> &mut usize {
        #[cfg(feature = "log")]
        trace!("Value::index_mut");

        &mut self.wrapper.index
    }

    #[must_use]
    pub fn size(&self) -> usize {
        #[cfg(feature = "log")]
        trace!("Value::size");

        self.wrapper.size
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn size_mut(&mut self) -> &mut usize {
        #[cfg(feature = "log")]
        trace!("Value::size_mut");

        &mut self.wrapper.size
    }

    #[must_use]
    pub fn wrapper(&self) -> &Wrapper<'a, N> {
        #[cfg(feature = "log")]
        trace!("Value::wrapper");

        &self.wrapper
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn wrapper_mut(&mut self) -> &mut Wrapper<'a, N> {
        #[cfg(feature = "log")]
        trace!("Value::wrapper_mut");

        &mut self.wrapper
    }
}

impl<'a, const N: usize, T> Deref for Value<'a, N, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        #[cfg(feature = "log")]
        trace!("Value::deref");

        // TODO Test this deref is to the correct ptr
        unsafe { &*self.wrapper[..].as_ptr().cast() }
    }
}
impl<'a, const N: usize, T> DerefMut for Value<'a, N, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[cfg(feature = "log")]
        trace!("Value::deref_mut");

        // TODO Test this deref is to the correct ptr
        unsafe { &mut *self.wrapper[..].as_mut_ptr().cast() }
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "repr_c", repr(C))]
pub struct Wrapper<'a, const N: usize> {
    allocator: &'a Allocator<N>,
    index: usize,
    size: usize,
}

impl<'a, const N: usize> Wrapper<'a, N> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator<N> {
        #[cfg(feature = "log")]
        trace!("Wrapper::allocator");

        self.allocator
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn allocator_mut(&mut self) -> &mut &'a Allocator<N> {
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

    #[must_use]
    pub fn size(&self) -> usize {
        #[cfg(feature = "log")]
        trace!("Wrapper::size");

        self.size
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn size_mut(&mut self) -> &mut usize {
        #[cfg(feature = "log")]
        trace!("Wrapper::size_mut");

        &mut self.size
    }
}

impl<'a, const N: usize> Deref for Wrapper<'a, N> {
    type Target = [Block];

    fn deref(&self) -> &Self::Target {
        #[cfg(feature = "log")]
        trace!("Wrapper::deref enter");

        // We circumvent acquiring a guard as we don't need to lock to safely dereference allocated
        // memory.

        #[cfg(feature = "repr_c")]
        let inner_allocator = unsafe { &*(self.allocator.0.get()) };

        #[cfg(not(feature = "repr_c"))]
        #[allow(mutable_transmutes)]
        let inner_allocator = unsafe {
            std::mem::transmute::<
                &std::sync::Mutex<InnerAllocator<N>>,
                &mut std::sync::Mutex<InnerAllocator<N>>,
            >(&self.allocator.0)
            .get_mut()
            .unwrap()
        };

        let slice = &inner_allocator.data[self.index..self.index + self.size];

        #[cfg(feature = "log")]
        trace!("Wrapper::deref exit");

        slice
    }
}
impl<'a, const N: usize> DerefMut for Wrapper<'a, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[cfg(feature = "log")]
        trace!("Wrapper::deref_mut enter");

        // We circumvent acquiring a guard as we don't need to lock to safely dereference allocated
        // memory.

        #[cfg(feature = "repr_c")]
        let inner_allocator = unsafe { &mut *(self.allocator.0.get()) };

        #[cfg(not(feature = "repr_c"))]
        #[allow(mutable_transmutes)]
        let inner_allocator = unsafe {
            std::mem::transmute::<
                &std::sync::Mutex<InnerAllocator<N>>,
                &mut std::sync::Mutex<InnerAllocator<N>>,
            >(&self.allocator.0)
            .get_mut()
            .unwrap()
        };

        let slice = &mut inner_allocator.data[self.index..self.index + self.size];

        #[cfg(feature = "log")]
        trace!("Wrapper::deref_mut exit");

        slice
    }
}

impl<'a, const N: usize> Drop for Wrapper<'a, N> {
    fn drop(&mut self) {
        #[cfg(feature = "log")]
        trace!("Wrapper::drop enter");

        if self.size == 0 {
            return;
        }

        let mut inner_allocator_guard = self.allocator.0.lock().unwrap();
        // To avoid a massive number of mutex deref calls we deref here.
        let inner_allocator = &mut *inner_allocator_guard;

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
            };
        }

        drop(inner_allocator_guard);

        #[cfg(feature = "log")]
        trace!("Wrapper::drop exit");
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "repr_c", repr(C))]
pub struct Slice<'a, const N: usize, T> {
    pub wrapper: Wrapper<'a, N>,
    len: usize,
    __marker: PhantomData<T>,
}

impl<'a, const N: usize, T> Slice<'a, N, T> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator<N> {
        #[cfg(feature = "log")]
        trace!("Slice::allocator");

        self.wrapper.allocator
    }

    #[must_use]
    pub fn index(&self) -> usize {
        #[cfg(feature = "log")]
        trace!("Slice::index");

        self.wrapper.index
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn index_mut(&mut self) -> &mut usize {
        #[cfg(feature = "log")]
        trace!("Slice::index_mut");

        &mut self.wrapper.index
    }

    #[must_use]
    pub fn size(&self) -> usize {
        #[cfg(feature = "log")]
        trace!("Slice::size");

        self.wrapper.size
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn size_mut(&mut self) -> &mut usize {
        #[cfg(feature = "log")]
        trace!("Slice::size_mut");

        &mut self.wrapper.size
    }

    pub fn wrapper(&mut self) -> &Wrapper<'a, N> {
        &self.wrapper
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn wrapper_mut(&mut self) -> &mut Wrapper<'a, N> {
        &mut self.wrapper
    }

    #[must_use]
    pub fn len(&self) -> usize {
        #[cfg(feature = "log")]
        trace!("Slice::len");

        self.len
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn len_mut(&mut self) -> &mut usize {
        &mut self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        #[cfg(feature = "log")]
        trace!("Slice::is_empty");

        self.len == 0
    }

    pub fn resize(&mut self, len: usize) -> Option<()> {
        #[cfg(feature = "log")]
        trace!("Slice::resize enter");

        // If resizing to current size, we can do nothing.
        if self.len() == len {
            return Some(());
        }

        // Allocate new slice.
        let mut new = self.wrapper.allocator.allocate_slice(len)?;

        // Copy data to new allocation
        let from = self[..].as_ptr();
        let to = new[..].as_mut_ptr();
        let n = std::cmp::min(len, self.len);
        unsafe {
            std::ptr::copy(from, to, n);
        }

        // Update wrapper
        let old = std::mem::replace(self, new);
        drop(old);

        #[cfg(feature = "log")]
        trace!("Slice::resize exit");

        Some(())
    }
}

impl<'a, const N: usize, T> Deref for Slice<'a, N, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        #[cfg(feature = "log")]
        trace!("Slice::deref enter");

        // TODO Test this deref is to the correct ptr
        let slice =
            unsafe { &*std::ptr::from_raw_parts(self.wrapper[..].as_ptr().cast(), self.len) };

        #[cfg(feature = "log")]
        trace!("Slice::deref exit");

        slice
    }
}
impl<'a, const N: usize, T> DerefMut for Slice<'a, N, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[cfg(feature = "log")]
        trace!("Slice::deref_mut enter");

        // TODO Test this deref is to the correct ptr
        let slice = unsafe {
            &mut *std::ptr::from_raw_parts_mut(self.wrapper[..].as_mut_ptr().cast(), self.len)
        };

        #[cfg(feature = "log")]
        trace!("Slice::deref_mut exit");

        slice
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::pedantic)]

    use rand::Rng;

    use super::*;

    #[test]
    fn slice_debug() {
        let allocator = Allocator::<3>::default();
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();

        #[cfg(feature = "repr_c")]
        let expected = "Slice { wrapper: Wrapper { allocator: Allocator(Mutex { lock: \
                        Mutex(UnsafeCell { .. }), data: UnsafeCell { .. } }), index: 0, size: 1 \
                        }, len: 3, __marker: PhantomData<u8> }";

        #[cfg(not(feature = "repr_c"))]
        let expected = "Slice { wrapper: Wrapper { allocator: Allocator(Mutex { data: \
                        InnerAllocator { head: Some(1), data: [Block { size: 3, next: None }, \
                        Block { size: 2, next: None }, Block { size: 0, next: None }] }, \
                        poisoned: false, .. }), index: 0, size: 1 }, len: 3, __marker: \
                        PhantomData<u8> }";

        assert_eq!(format!("{wrapper:?}"), expected);
    }
    #[test]
    fn slice_allocator() {
        let allocator = Allocator::<3>::default();
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        let _ = wrapper.allocator();
    }
    #[test]
    fn slice_index() {
        let allocator = Allocator::<3>::default();
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn slice_size() {
        let allocator = Allocator::<3>::default();
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert_eq!(wrapper.size(), 1);
    }
    #[test]
    fn slice_len() {
        let allocator = Allocator::<3>::default();
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert_eq!(wrapper.len(), 3);
    }
    #[test]
    fn slice_is_empty() {
        let allocator = Allocator::<3>::default();
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert!(!wrapper.is_empty());
    }
    #[test]
    fn slice_resize() {
        let allocator = Allocator::<5>::default();
        let mut wrapper = allocator.allocate_slice::<u8>(2).unwrap();
        wrapper[0] = 0;
        wrapper[1] = 1;
        wrapper.resize(3).unwrap();
        wrapper[0] = 0;
        wrapper[1] = 1;
    }
    #[test]
    fn slice_deref() {
        let allocator = Allocator::<3>::default();
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
        let allocator = Allocator::<3>::default();
        let mut wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        wrapper[0] = 0;
        assert_eq!(wrapper[0], 0);
        wrapper[1] = 1;
        assert_eq!(wrapper[1], 1);
        wrapper[2] = 2;
        assert_eq!(wrapper[2], 2);
    }
    #[test]
    fn slice_parallel_resize() {
        const THREADS: usize = 64;
        const SAMPLES: usize = 256;

        const NUM: std::ops::Range<usize> = 0..5;
        const SIZE: usize = 2 * THREADS * NUM.end;
        let allocator = Allocator::<SIZE>::default();

        let arc = std::sync::Arc::new(allocator);
        let handles = (0..THREADS)
            .map(move |_| {
                let arc_clone = arc.clone();
                std::thread::spawn(move || {
                    let mut rng = rand::thread_rng();

                    let mut slice = arc_clone.allocate_slice::<u8>(rng.gen_range(NUM)).unwrap();
                    for _ in 0..SAMPLES {
                        slice.resize(rng.gen_range(NUM)).unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn value_debug() {
        let allocator = Allocator::<1>::default();
        let wrapper = allocator.allocate_value::<u8>().unwrap();

        #[cfg(feature = "repr_c")]
        let expected = "Value { wrapper: Wrapper { allocator: Allocator(Mutex { lock: \
                        Mutex(UnsafeCell { .. }), data: UnsafeCell { .. } }), index: 0, size: 1 \
                        }, __marker: PhantomData<u8> }";

        #[cfg(not(feature = "repr_c"))]
        let expected = "Value { wrapper: Wrapper { allocator: Allocator(Mutex { data: \
                        InnerAllocator { head: None, data: [Block { size: 1, next: None }] }, \
                        poisoned: false, .. }), index: 0, size: 1 }, __marker: PhantomData<u8> }";

        assert_eq!(format!("{wrapper:?}"), expected);
    }
    #[test]
    fn value_allocator() {
        let allocator = Allocator::<1>::default();
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        let _ = wrapper.allocator();
    }
    #[test]
    fn value_index() {
        let allocator = Allocator::<1>::default();
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn value_size() {
        let allocator = Allocator::<1>::default();
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        assert_eq!(wrapper.size(), 1);
    }
    #[test]
    fn value_deref() {
        let allocator = Allocator::<1>::default();
        let mut wrapper = allocator.allocate_value::<u8>().unwrap();
        *wrapper = 0;
        assert_eq!(*wrapper, 0);
    }
    #[test]
    fn value_deref_mut() {
        let allocator = Allocator::<1>::default();
        let mut wrapper = allocator.allocate_value::<u8>().unwrap();
        *wrapper = 0;
        assert_eq!(*wrapper, 0);
    }

    #[test]
    fn inner_allocator_debug() {
        assert_eq!(
            format!("{:?}", InnerAllocator::<0>::default()),
            "InnerAllocator { head: None, data: [] }"
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
        let allocator = Allocator::<0>::default();

        #[cfg(feature = "repr_c")]
        let expected = "Wrapper { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. }), \
                        data: UnsafeCell { .. } }), index: 0, size: 0 }";

        #[cfg(not(feature = "repr_c"))]
        let expected = "Wrapper { allocator: Allocator(Mutex { data: InnerAllocator { head: None, \
                        data: [] }, poisoned: false, .. }), index: 0, size: 0 }";

        assert_eq!(
            format!(
                "{:?}",
                Wrapper {
                    allocator: &allocator,
                    index: 0,
                    size: 0
                }
            ),
            expected
        );
    }
    #[test]
    fn wrapper_allocator() {
        let allocator = Allocator::<1>::default();
        let wrapper = allocator.allocate(1).unwrap();
        let _ = wrapper.allocator();
    }
    #[test]
    fn wrapper_allocator_mut() {
        let allocator_one = Allocator::<1>::default();
        let allocator_two = Allocator::<1>::default();
        let mut wrapper = allocator_one.allocate(1).unwrap();
        assert_eq!(
            wrapper.allocator() as *const Allocator::<1> as usize,
            &allocator_one as *const Allocator::<1> as usize
        );
        unsafe {
            *wrapper.allocator_mut() = &allocator_two;
        }
        assert_eq!(
            wrapper.allocator() as *const Allocator::<1> as usize,
            &allocator_two as *const Allocator::<1> as usize
        );
    }
    #[test]
    fn wrapper_index() {
        let allocator = Allocator::<1>::default();
        let wrapper = allocator.allocate(1).unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn wrapper_size() {
        let allocator = Allocator::<1>::default();
        let wrapper = allocator.allocate(1).unwrap();
        assert_eq!(wrapper.size(), 1);
    }

    #[test]
    fn allocator() {
        // We hold items in a vec to prevent them being dropped;
        let memory = Allocator::<5>::default();
        let mut vec = Vec::new();

        assert_eq!(
            *memory.0.lock().unwrap(),
            InnerAllocator {
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
            *memory.0.lock().unwrap(),
            InnerAllocator {
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
            *memory.0.lock().unwrap(),
            InnerAllocator {
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
            *memory.0.lock().unwrap(),
            InnerAllocator {
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
            *memory.0.lock().unwrap(),
            InnerAllocator {
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
    fn allocator_debug() {
        #[cfg(feature = "repr_c")]
        let expected =
            "Allocator(Mutex { lock: Mutex(UnsafeCell { .. }), data: UnsafeCell { .. } })";
        #[cfg(not(feature = "repr_c"))]
        let expected = "Allocator(Mutex { data: InnerAllocator { head: None, data: [] }, \
                        poisoned: false, .. })";

        assert_eq!(format!("{:?}", Allocator::<0>::default()), expected);
    }
    #[cfg(feature = "repr_c")]
    #[test]
    fn allocator_init() {
        let mut uninit_allocator = std::mem::MaybeUninit::uninit();
        unsafe {
            <Allocator<3>>::init(uninit_allocator.as_mut_ptr(), None);
        }
    }
    #[cfg(feature = "repr_c")]
    #[test]
    fn allocator_init_zero() {
        let mut uninit_allocator = std::mem::MaybeUninit::uninit();
        unsafe {
            <Allocator<0>>::init(uninit_allocator.as_mut_ptr(), None);
        }
    }

    #[test]
    fn allocate_value() {
        let allocator = Allocator::<1>::default();
        allocator.allocate_value::<()>().unwrap();
    }
    #[test]
    fn allocate_slice() {
        let allocator = Allocator::<1>::default();
        allocator.allocate_slice::<u8>(size_of::<Block>()).unwrap();
    }

    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Equal` case.
    #[test]
    fn allocate_0() {
        let memory = Allocator::<1>::default();
        memory.allocate(1).unwrap();
    }
    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Less` case.
    #[test]
    fn allocate_1() {
        let memory = Allocator::<2>::default();
        memory.allocate(1).unwrap();
    }
    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Greater`
    // `blocks.cmp(&allocator.data[next].size) == Equal` case.
    #[test]
    fn allocate_2() {
        let memory = Allocator::<4>::default();
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
        let memory = Allocator::<5>::default();
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
        let memory = Allocator::<7>::default();
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
        let memory = Allocator::<6>::default();
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
        let memory = Allocator::<0>::default();
        assert!(memory.allocate(1).is_none());
    }

    // Tests `Wrapper` drop case of:
    // ┌───┐
    // │...│
    // └───┘
    #[test]
    fn drop_1() {
        let memory = Allocator::<1>::default();
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
        let memory = Allocator::<1>::default();
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
        let memory = Allocator::<2>::default();

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
        let memory = Allocator::<4>::default();

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
        let memory = Allocator::<5>::default();

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
        let memory = Allocator::<3>::default();

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
        let memory = Allocator::<5>::default();

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
        let memory = Allocator::<6>::default();

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
        let memory = Allocator::<6>::default();

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
        let memory = Allocator::<4>::default();

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
