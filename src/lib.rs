#![no_std]
#![forbid(unsafe_code)]

#[cfg(kani)]
extern crate kani;

pub mod funding;
pub mod margin;
pub mod math;
