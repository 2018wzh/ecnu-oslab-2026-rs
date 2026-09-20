pub const NAME: &str = "qemu-virt";
pub const HART_FIRST: usize = 0;
pub const NCPU: usize = 2;
pub const DRAM_BASE: usize = 0x8000_0000;
pub const DRAM_SIZE: usize = 128 * 1024 * 1024;
pub const UART_BASE: usize = 0x1000_0000;
pub const UART_CLOCK: usize = 3_686_400;
pub const UART_SHIFT: usize = 0;
// 本章只映射 PLIC；中断控制留待下一章。
pub const PLIC_BASE: usize = 0x0c00_0000;
pub const PLIC_SIZE: usize = 0x0400_0000;
