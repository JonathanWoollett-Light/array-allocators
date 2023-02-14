# Changelog

## 0.1.0 (2023-02-14)


### ⚠ BREAKING CHANGES

* Restructure + slicing
* Remove Mutex

### Features

* `allocator_mut()` ([80df0c9](https://github.com/JonathanWoollett-Light/array-allocators/commit/80df0c960b78ca7af25949a121fa498f5c554c82))
* add logs ([a0bc851](https://github.com/JonathanWoollett-Light/array-allocators/commit/a0bc851c7089029b32c91faa601631c3296d2e12))
* Dynamic size allocators ([1bcef24](https://github.com/JonathanWoollett-Light/array-allocators/commit/1bcef24bdc9e198c8c7e066ca6f8137af3581d1b))
* init ([14fbcff](https://github.com/JonathanWoollett-Light/array-allocators/commit/14fbcff5689b983965688c59706a3aee89a13864))
* Initial commit ([9d953fe](https://github.com/JonathanWoollett-Light/array-allocators/commit/9d953feaf714558ed43aa1f830ad668606077674))
* Linked list inner allocator accessors ([75baaa5](https://github.com/JonathanWoollett-Light/array-allocators/commit/75baaa54314511c6115e702fbda65e67618540d7))
* Linked list unsafe accessors ([5e52176](https://github.com/JonathanWoollett-Light/array-allocators/commit/5e52176e829ff5f1d963c298a3caf801e99d064f))
* Linked list wrappers `allocator_mut` ([5cdd3a8](https://github.com/JonathanWoollett-Light/array-allocators/commit/5cdd3a8b11675eeaff8cf354e9c2f01a05f3ea4e))
* log ([ad534f6](https://github.com/JonathanWoollett-Light/array-allocators/commit/ad534f6fcbe1ce5de807a0ae68ea61d91027eb95))
* non-repr(C) and big fixes ([cf937b8](https://github.com/JonathanWoollett-Light/array-allocators/commit/cf937b80dcea6ee9c304260ea046af1ad0be2f61))
* Remove Mutex ([75fc2cc](https://github.com/JonathanWoollett-Light/array-allocators/commit/75fc2cccfc7eb2399d2fa042332768dc8fb38f2c))
* resize ([5d296f3](https://github.com/JonathanWoollett-Light/array-allocators/commit/5d296f33111aaee78d6dc7ecb433aed9907fba96))
* Restructure + slicing ([15f5cae](https://github.com/JonathanWoollett-Light/array-allocators/commit/15f5cae6bca34aa5549c8531add32c56e62005de))
* Significant improvements ([c53e6d3](https://github.com/JonathanWoollett-Light/array-allocators/commit/c53e6d3e15140e62a993639c821c992449fdcb41))
* Used block iterator for slab allocator ([b554b0a](https://github.com/JonathanWoollett-Light/array-allocators/commit/b554b0afac756b44a5f3ee1c819241d09434984d))
* Zero and Non-Zero linked list allocate functions ([ead5452](https://github.com/JonathanWoollett-Light/array-allocators/commit/ead5452e0179510960d258ed27eba3940a87662d))


### Bug Fixes

* Add clippy ([70e93d9](https://github.com/JonathanWoollett-Light/array-allocators/commit/70e93d9dcf1afb6041e1d46332d8985ed19d17ab))
* Add required crate for make file testing ([9dc3996](https://github.com/JonathanWoollett-Light/array-allocators/commit/9dc399642121d483743bc362b0a477df447ce016))
* Ensure `MutexGuard`doesn't deference `self` ([286d9e0](https://github.com/JonathanWoollett-Light/array-allocators/commit/286d9e01bce0c15a2c2ec9aed490e8cae74f3535))
* Ensure `Wrapper` does not deref itself. ([213115f](https://github.com/JonathanWoollett-Light/array-allocators/commit/213115ffaf927cca5f7266ffec10b5a4307cfad2))
* Fixes ([ddedede](https://github.com/JonathanWoollett-Light/array-allocators/commit/ddededea0a032476ad77ede71aa3169e4620a1fd))
* Fixing clippy ([5b3684f](https://github.com/JonathanWoollett-Light/array-allocators/commit/5b3684f688579895f23dfc8533889d6eb145c8b5))
* init ([22e00ba](https://github.com/JonathanWoollett-Light/array-allocators/commit/22e00badfb1b3d45a40ecb3428421de7c83f720c))
* Inner functions ([3b754f1](https://github.com/JonathanWoollett-Light/array-allocators/commit/3b754f13ed1af9bb13cdb77f3e09e597338f5312))
* Linked list slice & value dereferencing ([594c037](https://github.com/JonathanWoollett-Light/array-allocators/commit/594c03706872cf416e0e72520048a6bb5fd09fbe))
* linked list slice resize ([0dbea29](https://github.com/JonathanWoollett-Light/array-allocators/commit/0dbea297ed1d8027a81ccb53d4a0ed4d8568f49d))
* log debug print ptr ([153417a](https://github.com/JonathanWoollett-Light/array-allocators/commit/153417af018abb9b1cb4292d9f40a67873f2c6d5))
* More logs ([6bec6fb](https://github.com/JonathanWoollett-Light/array-allocators/commit/6bec6fb25d9a8ce029c40ee924283abe7ccc7b0b))
* Reduce mutex dereferences ([c0bcc97](https://github.com/JonathanWoollett-Light/array-allocators/commit/c0bcc97793738577bebc78ab639d538e669d8f33))
* Reduce mutex dereferences continuation ([15a0606](https://github.com/JonathanWoollett-Light/array-allocators/commit/15a0606c2c89651271ba77a33325f40901b6ee7c))
* Remove `Debug` trait requirement ([ded5664](https://github.com/JonathanWoollett-Light/array-allocators/commit/ded5664d1d57a13c95fd3484e7a0291071a45518))
* resize ([5a7ba55](https://github.com/JonathanWoollett-Light/array-allocators/commit/5a7ba550e7c4257c7ee29d61e3748b48defa604e))
* Some extra logging ([2c9587a](https://github.com/JonathanWoollett-Light/array-allocators/commit/2c9587a85ccbf9f47412054c6ec2d753197cb7e4))
* Update nix ([ef4a854](https://github.com/JonathanWoollett-Light/array-allocators/commit/ef4a854c20aad0666199fc042d38a41e992b4c0d))
* Update Nix version ([bdaaebc](https://github.com/JonathanWoollett-Light/array-allocators/commit/bdaaebc6e974016518fba099c664fe97a3792568))
