# Linux IO Uring
[![github actions](https://github.com/quininer/linux-io-uring/workflows/Rust/badge.svg)](https://github.com/quininer/linux-io-uring/actions)
[![crates](https://img.shields.io/crates/v/linux-io-uring.svg)](https://crates.io/crates/linux-io-uring)
[![license](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/quininer/linux-io-uring/blob/master/LICENSE-MIT)
[![license](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://github.com/quininer/linux-io-uring/blob/master/LICENSE-APACHE)
[![docs.rs](https://docs.rs/linux-io-uring/badge.svg)](https://docs.rs/linux-io-uring/)

The [`io\_uring`](https://kernel.dk/io_uring.pdf) userspace interface for Rust.

## Safety

All APIs are safe except for pushing entries into submission queue.
This means that the developer must ensure that entry is valid, otherwise it will cause UB.

I am trying to develop a proactor library to provide a safety abstraction.

## Why Rust ?

I don't think it needs a special reason.

The `io_uring` api design is so simple and elegant
that implementing the new `io_uring` library is not much more complicated than wrapping `liburing`.

This has some advantages over wrapping c library,
it have more freedom (see concurrent mod), and it can be easier to static link.

### License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.
