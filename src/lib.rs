#![feature(ptr_metadata)]
#![feature(int_roundings)]
#![feature(nonnull_slice_from_raw_parts)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::let_and_return
)]

pub mod linked_list;

pub type LinkedListArrayAllocator<const N: usize> = linked_list::ArrayAllocator<N>;
pub type LinkedListAllocator = linked_list::Allocator;
pub type LinkedListWrapper<'a> = linked_list::Wrapper<'a>;
pub type LinkedListValue<'a, T> = linked_list::Value<'a, T>;
pub type LinkedListSlice<'a, T> = linked_list::Slice<'a, T>;

pub mod slab;

pub type SlabArrayAllocator<const N: usize, T> = slab::ArrayAllocator<N, T>;
pub type SlabAllocator<T> = slab::Allocator<T>;
pub type SlabWrapper<'a, T> = slab::Wrapper<'a, T>;

pub(crate) mod mutex;
