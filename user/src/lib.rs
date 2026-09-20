#![no_std]
pub mod arch;
pub mod syscall;
pub use syscall::hello;
