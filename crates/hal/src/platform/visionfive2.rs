pub const NAME: &str = "visionfive2";
pub const HART_FIRST: usize = 1;
pub const NCPU: usize = 4;
pub const DRAM_BASE: usize = 0x4000_0000;
pub const DRAM_SIZE: usize = 128 * 1024 * 1024;
pub const UART_BASE: usize = 0x1000_0000;
pub const UART_CLOCK: usize = 24_000_000;
pub const UART_SHIFT: usize = 2;
pub const UART_IRQ: u32 = 32;
pub const PLIC_BASE: usize = 0x0c00_0000;
pub const PLIC_SIZE: usize = 0x0400_0000;
pub const TIMER_INTERVAL: usize = 400_000;
pub fn plic_context(hart: usize) -> usize { 2 * hart - 1 }
pub const BLOCK_BASE: usize = 0x1602_0000;
pub const BLOCK_IRQ: u32 = 75;
pub const CCACHE_BASE: usize = 0x0201_0000;
