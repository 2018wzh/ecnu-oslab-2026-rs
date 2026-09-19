//! 16550 轮询输出。
pub struct Uart { base: usize, shift: usize }
impl Uart {
    /// # Safety
    /// base 必须是有效且可访问的 UART MMIO 地址。
    pub const unsafe fn new(base: usize, shift: usize) -> Self { Self { base, shift } }
    fn write(&self, reg: usize, value: u8) {
        // SAFETY: 构造者保证 MMIO 地址有效，寄存器编号由驱动内部提供。
        unsafe { core::ptr::write_volatile((self.base + (reg << self.shift)) as *mut u8, value); }
    }
    pub fn init(&self, clock: usize) {
        let divisor = (clock + 8 * 115200) / (16 * 115200);
        self.write(1, 0); self.write(3, 0x80);
        self.write(0, divisor as u8); self.write(1, (divisor >> 8) as u8);
        self.write(3, 3); self.write(2, 7);
    }
    pub fn putc(&self, c: u8) {
        // SAFETY: LSR 位于 UART 寄存器窗口内。
        while unsafe { core::ptr::read_volatile((self.base + (5 << self.shift)) as *const u8) } & 0x20 == 0 {
            core::hint::spin_loop();
        }
        self.write(0, c);
    }
}
