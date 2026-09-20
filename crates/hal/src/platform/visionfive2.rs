pub const NAME: &str = "visionfive2";
pub const HART_FIRST: usize = 1;
pub const NCPU: usize = 4;
pub const DRAM_BASE: usize = 0x4000_0000;
pub const DRAM_SIZE: usize = 128 * 1024 * 1024;
pub const UART_BASE: usize = 0x1000_0000;
pub const UART_CLOCK: usize = 24_000_000;
pub const UART_SHIFT: usize = 2;
// 本章只映射 PLIC；中断控制留待下一章。
pub const PLIC_BASE: usize = 0x0c00_0000;
pub const PLIC_SIZE: usize = 0x0400_0000;
