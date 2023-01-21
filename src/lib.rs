#![feature(ptr_metadata)]
#![feature(int_roundings)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]

pub mod linked_list;

pub type LinkedListAllocator<const N: usize> = linked_list::Allocator<N>;
pub type LinkedListWrapper<'a, const N: usize> = linked_list::Wrapper<'a, N>;
pub type LinkedListValue<'a, const N: usize, T> = linked_list::Value<'a, N, T>;
pub type LinkedListSlice<'a, const N: usize, T> = linked_list::Slice<'a, N, T>;

pub mod slab;

pub type SlabAllocator<const N: usize, T> = slab::Allocator<N, T>;
pub type SlabWrapper<'a, const N: usize, T> = slab::Wrapper<'a, N, T>;

#[cfg(feature = "repr_c")]
pub(crate) mod mutex;
