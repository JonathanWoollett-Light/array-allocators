use std::cmp::Ordering;
use std::marker::PhantomData;
use std::mem::size_of;
use std::num::NonZeroUsize;
use std::ops::{Deref, DerefMut, Drop};
use std::ptr::NonNull;

#[cfg(feature = "log")]
use log::trace;

#[derive(Debug)]
#[repr(C)]
pub struct ArrayAllocator<const N: usize> {
    allocator: Allocator,
    data: [Block; N],
}
impl<const N: usize> ArrayAllocator<N> {
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

impl<const N: usize> Deref for ArrayAllocator<N> {
    type Target = Allocator;

    fn deref(&self) -> &Self::Target {
        &self.allocator
    }
}
impl<const N: usize> DerefMut for ArrayAllocator<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.allocator
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Allocator(super::mutex::Mutex<InnerAllocator>);

impl Allocator {
    /// Initializes `Self` at `ptr`.
    ///
    /// # Safety
    ///
    /// `ptr` must be valid.
    ///
    /// # Panics
    ///
    /// When failing to initialize the inner mutex.

    pub unsafe fn init(ptr: *mut Self, attr: Option<nix::sys::pthread::MutexAttr>, n: usize) {
        #[cfg(feature = "log")]
        trace!("Allocator::init");
        (*ptr).0.lock = nix::sys::pthread::Mutex::new(attr).unwrap();

        #[cfg(feature = "log")]
        trace!("Allocator::init 2");
        <InnerAllocator>::init((*ptr).0.get(), n);
    }

    /// Allocates zero blocks.
    pub fn allocate_zero(&self) -> Wrapper {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate_zero");

        Wrapper {
            allocator: self,
            index: 0,
            size: 0,
        }
    }

    /// Allocates a non-zero number of blocks.
    ///
    /// # Panics
    ///
    /// When locking the mutex fails.
    pub fn allocate_nonzero(&self, blocks: NonZeroUsize) -> Option<Wrapper> {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate_nonzero");

        let blocks = blocks.get();

        let mut allocator_guard = self.0.lock().unwrap();
        let allocator = &mut *allocator_guard;
        let data = unsafe { allocator.data().as_mut() };

        let rtn = if let Some(next) = allocator.head {
            match blocks.cmp(&data[next].size) {
                Ordering::Equal => {
                    allocator.head = data[next].next;
                    Some(Wrapper {
                        allocator: self,
                        index: next,
                        size: blocks,
                    })
                }
                Ordering::Less => {
                    let new_index = next + blocks;
                    data[new_index] = Block {
                        size: data[next].size - blocks,
                        next: data[next].next,
                    };
                    allocator.head = Some(new_index);
                    Some(Wrapper {
                        allocator: self,
                        index: next,
                        size: blocks,
                    })
                }
                Ordering::Greater => {
                    let mut next_opt = data[next].next;
                    loop {
                        if let Some(next) = next_opt {
                            match blocks.cmp(&data[next].size) {
                                Ordering::Equal => {
                                    allocator.head = data[next].next;
                                    break Some(Wrapper {
                                        allocator: self,
                                        index: next,
                                        size: blocks,
                                    });
                                }
                                Ordering::Less => {
                                    let new_index = next + blocks;
                                    data[new_index] = Block {
                                        size: data[next].size - blocks,
                                        next: data[next].next,
                                    };
                                    allocator.head = Some(new_index);
                                    break Some(Wrapper {
                                        allocator: self,
                                        index: next,
                                        size: blocks,
                                    });
                                }
                                Ordering::Greater => {
                                    next_opt = data[next].next;
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

    /// Allocates a given number of blocks.
    ///
    /// # Panics
    ///
    /// When locking the mutex fails.
    pub fn allocate(&self, blocks: usize) -> Option<Wrapper> {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate");

        if let Ok(nonzero) = NonZeroUsize::try_from(blocks) {
            self.allocate_nonzero(nonzero)
        } else {
            Some(self.allocate_zero())
        }
    }

    pub fn allocate_value<T>(&self) -> Option<Value<T>> {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate_value");
        let blocks = size_of::<T>().div_ceil(size_of::<Block>());
        self.allocate(blocks).map(|wrapper| Value {
            wrapper,
            __marker: PhantomData,
        })
    }

    /// Allocates `[T]` where `length == 0`.
    pub fn allocate_zero_slice<T>(&self) -> Slice<T> {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate_zero_slice");

        Slice {
            wrapper: self.allocate_zero(),
            len: 0,
            __marker: PhantomData,
        }
    }

    /// Allocates `[T]` where `length > 0`.
    ///
    /// # Panics
    ///
    /// When locking the mutex fails.
    pub fn allocate_nonzero_slice<T>(&self, len: NonZeroUsize) -> Option<Slice<T>> {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate_nonzero_slice");

        let len = len.get();

        let blocks =
            NonZeroUsize::try_from((len * size_of::<T>()).div_ceil(size_of::<Block>())).unwrap();

        debug_assert!(blocks.get() * size_of::<Block>() >= len * size_of::<T>());

        self.allocate_nonzero(blocks).map(|wrapper| Slice {
            wrapper,
            len,
            __marker: PhantomData,
        })
    }

    /// Allocates `[T]`.
    ///
    /// # Panics
    ///
    /// When locking the mutex fails.
    pub fn allocate_slice<T>(&self, len: usize) -> Option<Slice<T>> {
        #[cfg(feature = "log")]
        trace!("Allocator::allocate_slice");

        if let Ok(nonzero) = NonZeroUsize::try_from(len) {
            self.allocate_nonzero_slice(nonzero)
        } else {
            Some(self.allocate_zero_slice())
        }
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.

    pub unsafe fn inner(&self) -> &super::mutex::Mutex<InnerAllocator> {
        &self.0
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.

    pub unsafe fn inner_mut(&mut self) -> &mut super::mutex::Mutex<InnerAllocator> {
        &mut self.0
    }
}

#[derive(Debug, Eq, PartialEq)]
#[repr(C)]
pub struct InnerAllocator {
    head: Option<usize>,
    size: usize,
}

impl InnerAllocator {
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
    ///
    /// # Panics
    ///
    /// When `&self == std::ptr::null()`.
    #[must_use]
    pub unsafe fn data(&mut self) -> NonNull<[Block]> {
        #[cfg(feature = "log")]
        trace!("InnerAllocator::data");

        std::ptr::NonNull::slice_from_raw_parts(
            NonNull::new((self as *mut Self).add(1).cast()).unwrap(),
            self.size,
        )
    }

    unsafe fn init(ptr: *mut Self, n: usize) {
        #[cfg(feature = "log")]
        trace!("InnerAllocator::init");

        if n > 0 {
            #[cfg(feature = "log")]
            trace!("InnerAllocator::init non-empty");

            (*ptr).head = Some(0);
            (*ptr).size = n;

            #[cfg(feature = "log")]
            trace!("InnerAllocator::init head written");

            std::ptr::write(
                ptr.add(1).cast(),
                Block {
                    size: n,
                    next: None,
                },
            );
        } else {
            #[cfg(feature = "log")]
            trace!("InnerAllocator::init empty");

            (*ptr).head = None;
            (*ptr).size = 0;
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
pub struct Value<'a, T> {
    pub wrapper: Wrapper<'a>,
    __marker: PhantomData<T>,
}

impl<'a, T> Value<'a, T> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator {
        #[cfg(feature = "log")]
        trace!("Value::allocator");

        self.wrapper.allocator
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn allocator_mut(&mut self) -> &mut &'a Allocator {
        #[cfg(feature = "log")]
        trace!("Slice::allocator_mut");

        &mut self.wrapper.allocator
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
    pub fn wrapper(&self) -> &Wrapper<'a> {
        #[cfg(feature = "log")]
        trace!("Value::wrapper");

        &self.wrapper
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn wrapper_mut(&mut self) -> &mut Wrapper<'a> {
        #[cfg(feature = "log")]
        trace!("Value::wrapper_mut");

        &mut self.wrapper
    }
}

impl<'a, T> Deref for Value<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        #[cfg(feature = "log")]
        trace!("Value::deref");

        // TODO Test this deref is to the correct ptr
        unsafe { &*self.wrapper[..].as_ptr().cast() }
    }
}
impl<'a, T> DerefMut for Value<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[cfg(feature = "log")]
        trace!("Value::deref_mut");

        // TODO Test this deref is to the correct ptr
        unsafe { &mut *self.wrapper[..].as_mut_ptr().cast() }
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Wrapper<'a> {
    allocator: &'a Allocator,
    index: usize,
    size: usize,
}

impl<'a> Wrapper<'a> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator {
        #[cfg(feature = "log")]
        trace!("Wrapper::allocator");

        self.allocator
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn allocator_mut(&mut self) -> &mut &'a Allocator {
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

impl<'a> Deref for Wrapper<'a> {
    type Target = [Block];

    fn deref(&self) -> &Self::Target {
        #[cfg(feature = "log")]
        trace!("Wrapper::deref enter");

        // We circumvent acquiring a guard as we don't need to lock to safely dereference allocated
        // memory.

        let inner_allocator = unsafe { &mut *(self.allocator.0.get()) };

        let slice = unsafe { &inner_allocator.data().as_ref()[self.index..self.index + self.size] };

        #[cfg(feature = "log")]
        trace!("Wrapper::deref exit");

        slice
    }
}
impl<'a> DerefMut for Wrapper<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[cfg(feature = "log")]
        trace!("Wrapper::deref_mut enter");

        // We circumvent acquiring a guard as we don't need to lock to safely dereference allocated
        // memory.

        let inner_allocator = unsafe { &mut *(self.allocator.0.get()) };

        let slice =
            unsafe { &mut inner_allocator.data().as_mut()[self.index..self.index + self.size] };

        #[cfg(feature = "log")]
        trace!("Wrapper::deref_mut exit");

        slice
    }
}

impl<'a> Drop for Wrapper<'a> {
    fn drop(&mut self) {
        #[cfg(feature = "log")]
        trace!("Wrapper::drop enter");

        if self.size == 0 {
            return;
        }

        let mut inner_allocator_guard = self.allocator.0.lock().unwrap();
        // To avoid a massive number of mutex deref calls we deref here.
        let inner_allocator = &mut *inner_allocator_guard;
        let data = unsafe { inner_allocator.data().as_mut() };

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
                    data[self.index] = Block {
                        size: self.size + data[head].size,
                        next: data[head].next,
                    };
                    inner_allocator.head = Some(self.index);
                }
                // ┌───┬────┬───┬────┬───┐
                // │...│self│...│head│...│
                // └───┴────┴───┴────┴───┘
                Ordering::Less => {
                    data[self.index] = Block {
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
                        let current_end = current_index + data[current_index].size;

                        match (current_end == self.index, data[current_index].next) {
                            // ┌───┬─────┬────┬────┬───┐
                            // │...│index│self│next│...│
                            // └───┴─────┴────┴────┴───┘
                            // The self block starts at the current block and ends at the next
                            // block.
                            (true, Some(next_index)) if next_index == end => {
                                // Update the size and next of the current block and return.
                                data[current_index].next = data[next_index].next;
                                data[current_index].size += self.size + data[next_index].size;
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
                                data[current_index].size += self.size;
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
                                data[current_index].size += self.size;
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
                                data[self.index] = Block {
                                    size: self.size + data[next_index].size,
                                    next: data[next_index].next,
                                };
                                data[current_index].next = Some(self.index);
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
                                data[self.index] = Block {
                                    size: self.size,
                                    next: data[current_index].next,
                                };
                                data[current_index].next = Some(self.index);
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
                                data[self.index] = Block {
                                    size: self.size,
                                    next: None,
                                };
                                data[current_index].next = Some(self.index);
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
            data[self.index] = Block {
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
#[repr(C)]
pub struct Slice<'a, T> {
    pub wrapper: Wrapper<'a>,
    len: usize,
    __marker: PhantomData<T>,
}

impl<'a, T> Slice<'a, T> {
    #[must_use]
    pub fn allocator(&self) -> &Allocator {
        #[cfg(feature = "log")]
        trace!("Slice::allocator");

        self.wrapper.allocator
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn allocator_mut(&mut self) -> &mut &'a Allocator {
        #[cfg(feature = "log")]
        trace!("Slice::allocator_mut");

        &mut self.wrapper.allocator
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

    pub fn wrapper(&mut self) -> &Wrapper<'a> {
        &self.wrapper
    }

    /// # Safety
    ///
    /// You almost definitely should not use this, it is extremely unsafe and can invalidate all
    /// memory of the allocator to which this belongs.
    pub unsafe fn wrapper_mut(&mut self) -> &mut Wrapper<'a> {
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

impl<'a, T> Deref for Slice<'a, T> {
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
impl<'a, T> DerefMut for Slice<'a, T> {
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
        let allocator = ArrayAllocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();

        let expected = "Slice { wrapper: Wrapper { allocator: Allocator(Mutex { lock: \
                        Mutex(UnsafeCell { .. }), data: UnsafeCell { .. } }), index: 0, size: 1 \
                        }, len: 3, __marker: PhantomData<u8> }";

        assert_eq!(format!("{wrapper:?}"), expected);
    }
    #[test]
    fn slice_allocator() {
        let allocator = ArrayAllocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        let _ = wrapper.allocator();
    }
    #[test]
    fn slice_index() {
        let allocator = ArrayAllocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn slice_size() {
        let allocator = ArrayAllocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert_eq!(wrapper.size(), 1);
    }
    #[test]
    fn slice_len() {
        let allocator = ArrayAllocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert_eq!(wrapper.len(), 3);
    }
    #[test]
    fn slice_is_empty() {
        let allocator = ArrayAllocator::<3>::new(None);
        let wrapper = allocator.allocate_slice::<u8>(3).unwrap();
        assert!(!wrapper.is_empty());
    }
    #[test]
    fn slice_resize() {
        let allocator = ArrayAllocator::<5>::new(None);
        let mut wrapper = allocator.allocate_slice::<u8>(2).unwrap();
        wrapper[0] = 0;
        wrapper[1] = 1;
        wrapper.resize(3).unwrap();
        wrapper[0] = 0;
        wrapper[1] = 1;
    }
    #[test]
    fn slice_deref() {
        let allocator = ArrayAllocator::<3>::new(None);
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
        let allocator = ArrayAllocator::<3>::new(None);
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
        let allocator = ArrayAllocator::<SIZE>::new(None);

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
        let allocator = ArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();

        let expected = "Value { wrapper: Wrapper { allocator: Allocator(Mutex { lock: \
                        Mutex(UnsafeCell { .. }), data: UnsafeCell { .. } }), index: 0, size: 1 \
                        }, __marker: PhantomData<u8> }";

        assert_eq!(format!("{wrapper:?}"), expected);
    }
    #[test]
    fn value_allocator() {
        let allocator = ArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        let _ = wrapper.allocator();
    }
    #[test]
    fn value_index() {
        let allocator = ArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn value_size() {
        let allocator = ArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate_value::<u8>().unwrap();
        assert_eq!(wrapper.size(), 1);
    }
    #[test]
    fn value_deref() {
        let allocator = ArrayAllocator::<1>::new(None);
        let mut wrapper = allocator.allocate_value::<u8>().unwrap();
        *wrapper = 0;
        assert_eq!(*wrapper, 0);
    }
    #[test]
    fn value_deref_mut() {
        let allocator = ArrayAllocator::<1>::new(None);
        let mut wrapper = allocator.allocate_value::<u8>().unwrap();
        *wrapper = 0;
        assert_eq!(*wrapper, 0);
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
        let allocator = ArrayAllocator::<0>::new(None);

        let expected = "Wrapper { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. }), \
                        data: UnsafeCell { .. } }), index: 0, size: 0 }";

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
        let allocator = ArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate(1).unwrap();
        let _ = wrapper.allocator();
    }
    #[test]
    fn wrapper_allocator_mut() {
        let allocator_one = ArrayAllocator::<1>::new(None);
        let allocator_two = ArrayAllocator::<1>::new(None);
        let mut wrapper = allocator_one.allocate(1).unwrap();
        assert_eq!(
            wrapper.allocator() as *const Allocator as usize,
            &allocator_one as *const ArrayAllocator::<1> as usize
        );
        unsafe {
            *wrapper.allocator_mut() = &allocator_two;
        }
        assert_eq!(
            wrapper.allocator() as *const Allocator as usize,
            &allocator_two as *const ArrayAllocator::<1> as usize
        );
    }
    #[test]
    fn wrapper_index() {
        let allocator = ArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate(1).unwrap();
        assert_eq!(wrapper.index(), 0);
    }
    #[test]
    fn wrapper_size() {
        let allocator = ArrayAllocator::<1>::new(None);
        let wrapper = allocator.allocate(1).unwrap();
        assert_eq!(wrapper.size(), 1);
    }

    #[test]
    fn allocator() {
        // We hold items in a vec to prevent them being dropped;
        const SIZE: usize = 5;
        let memory = ArrayAllocator::<SIZE>::new(None);
        let mut vec = Vec::new();

        {
            let mut guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(0),
                    size: SIZE
                }
            );
            assert_eq!(
                unsafe { guard.data().as_ref() },
                [
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
            );
        }

        vec.push(memory.allocate(1));

        {
            let mut guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(1),
                    size: SIZE
                }
            );
            assert_eq!(
                unsafe { guard.data().as_ref() },
                [
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
            );
        }

        vec.push(memory.allocate(2));

        {
            let mut guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(3),
                    size: SIZE
                }
            );
            assert_eq!(
                unsafe { guard.data().as_ref() },
                [
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
            );
        }

        vec.pop();

        {
            let mut guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(1),
                    size: SIZE
                }
            );
            assert_eq!(
                unsafe { guard.data().as_ref() },
                [
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
            );
        }

        vec.pop();

        {
            let mut guard = memory.0.lock().unwrap();
            assert_eq!(
                *guard,
                InnerAllocator {
                    head: Some(0),
                    size: SIZE
                }
            );
            assert_eq!(
                unsafe { guard.data().as_ref() },
                [
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
            );
        }

        drop(vec);
    }

    #[test]
    fn array_allocator_debug() {
        let expected = "ArrayAllocator { allocator: Allocator(Mutex { lock: Mutex(UnsafeCell { .. \
                        }), data: UnsafeCell { .. } }), data: [] }";

        assert_eq!(format!("{:?}", ArrayAllocator::<0>::new(None)), expected);
    }

    #[test]
    fn allocator_init() {
        let _ = ArrayAllocator::<3>::new(None);
    }

    #[test]
    fn allocator_init_zero() {
        let _ = ArrayAllocator::<0>::new(None);
    }

    #[test]
    fn allocate_value() {
        let allocator = ArrayAllocator::<1>::new(None);
        allocator.allocate_value::<()>().unwrap();
    }
    #[test]
    fn allocate_slice() {
        let allocator = ArrayAllocator::<1>::new(None);
        allocator.allocate_slice::<u8>(size_of::<Block>()).unwrap();
    }

    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Equal` case.
    #[test]
    fn allocate_0() {
        let memory = ArrayAllocator::<1>::new(None);
        memory.allocate(1).unwrap();
    }
    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Less` case.
    #[test]
    fn allocate_1() {
        let memory = ArrayAllocator::<2>::new(None);
        memory.allocate(1).unwrap();
    }
    // Tests `Wrapper::allocate` `blocks.cmp(&allocator.data[next].size) == Greater`
    // `blocks.cmp(&allocator.data[next].size) == Equal` case.
    #[test]
    fn allocate_2() {
        let memory = ArrayAllocator::<4>::new(None);
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
        let memory = ArrayAllocator::<5>::new(None);
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
        let memory = ArrayAllocator::<7>::new(None);
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
        let memory = ArrayAllocator::<6>::new(None);
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
        let memory = ArrayAllocator::<0>::new(None);
        assert!(memory.allocate(1).is_none());
    }

    // Tests `Wrapper` drop case of:
    // ┌───┐
    // │...│
    // └───┘
    #[test]
    fn drop_1() {
        let memory = ArrayAllocator::<1>::new(None);
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
        let memory = ArrayAllocator::<1>::new(None);
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
        let memory = ArrayAllocator::<2>::new(None);

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
        let memory = ArrayAllocator::<4>::new(None);

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
        let memory = ArrayAllocator::<5>::new(None);

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
        let memory = ArrayAllocator::<3>::new(None);

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
        let memory = ArrayAllocator::<5>::new(None);

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
        let memory = ArrayAllocator::<6>::new(None);

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
        let memory = ArrayAllocator::<6>::new(None);

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
        let memory = ArrayAllocator::<4>::new(None);

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
