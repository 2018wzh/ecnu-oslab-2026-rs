//! PLIC。非零 claim 的完成由守卫负责。
use core::marker::PhantomData;
pub struct Plic { base: usize }
/// 一次非零领取的完成义务；守卫存活期间处理设备，离开作用域完成响应。
pub struct Claim<'a> {
    plic: &'a Plic, // 保持控制器借用有效。
    context: usize, // 必须向领取时的同一 context 完成响应。
    pub irq: u32, // 非零来源号，供学生分派。
    _local: PhantomData<*mut ()>, // 禁止 Send/Sync，不能把完成义务移到其他核。
}
impl Plic {
    /// # Safety
    /// base 是对齐且在驱动存活期间可访问的 64MiB PLIC MMIO 窗口；
    /// 初始化同一优先级/使能字不得并发，claim 使用当前 hart 的有效 context。
    pub const unsafe fn new(base: usize) -> Self { Self { base } }
    /// 教师驱动：设置设备中断优先级。
    pub fn init(&self, irq: u32) {
        assert!(irq > 0 && irq < 1024);
        // SAFETY: 构造契约保证 MMIO 有效，irq 限制在优先级区域内。
        unsafe { core::ptr::write_volatile((self.base + 4 * irq as usize) as *mut u32, 1); }
    }
    /// 每核初始化自己的 context，保留同一字中其他设备的使能位。
    pub fn enable(&self, context: usize, irq: u32) {
        assert!(irq > 0 && irq < 1024);
        assert!(context < (0x04000000 - 0x200000) / 0x1000);
        // SAFETY: 构造契约和范围检查保证寄存器有效且初始化不并发。
        unsafe {
            let enable = (self.base + 0x2000 + 0x80 * context + 4 * (irq as usize / 32)) as *mut u32;
            core::ptr::write_volatile(enable, core::ptr::read_volatile(enable) | (1u32 << (irq % 32)));
            core::ptr::write_volatile((self.base + 0x200000 + 0x1000 * context) as *mut u32, 0);
        }
    }
    /// 无来源返回 None；非零返回守卫，不能 forget 或重复 complete。
    pub fn claim(&self, context: usize) -> Option<Claim<'_>> {
        assert!(context < (0x04000000 - 0x200000) / 0x1000);
        // SAFETY: 构造契约保证当前 hart 的 context 可访问。
        let irq = unsafe { core::ptr::read_volatile((self.base + 0x200004 + 0x1000 * context) as *const u32) };
        if irq == 0 { None } else { Some(Claim { plic: self, context, irq, _local: PhantomData }) }
    }
}
impl Drop for Claim<'_> {
    fn drop(&mut self) {
        // SAFETY: Claim 只由有效 PLIC context 的非零 claim 构造，当前核独占它。
        unsafe { core::ptr::write_volatile((self.plic.base + 0x200004 + 0x1000 * self.context) as *mut u32, self.irq); }
    }
}
