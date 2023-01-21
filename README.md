# array-allocators

Memory allocators that manage memory within an array.

These are intended for usage in shared memory.

All types are [`#[repr(C)]`](https://doc.rust-lang.org/nomicon/other-reprs.html#reprc) by default, this can be disabled with `default-features = false`.