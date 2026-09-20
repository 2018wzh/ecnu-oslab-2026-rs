//! JH7110 SDIO1 教师后端；来源和 U-Boot 前置条件见 docs/visionfive2-sd.md。
use core::ptr::{read_volatile, write_volatile};
const ERROR: u32 = 0xbfc2;
const START: u32 = 1 << 31;
const HOLD: u32 = 1 << 29;
const RESP: u32 = (1 << 6) | (1 << 8);
const FIRST: u64 = 2097152;
const SECTORS: u64 = 10494296;
#[repr(C, align(64))]
struct Descriptor([u32; 16]);
// 与 Sd 状态分开，避免借用驱动时覆盖仍属于设备的描述符内存。
static mut DESC: Descriptor = Descriptor([0; 16]);
pub struct Sd { base: usize, cache: usize, fence: fn(), wide: bool, active: bool,
    seen: u32, dma_seen: u32, data: usize }
impl Sd {
    pub const NEW: Self = Self { base: 0, cache: 0, fence: || {}, wide: false,
        active: false, seen: 0, dma_seen: 0, data: 0 };
    fn rd(&self, off: usize) -> u32 {
        // SAFETY: init 要求有效且常驻的 MMIO 映射。
        unsafe { read_volatile((self.base + off) as *const u32) }
    }
    fn wr(&self, off: usize, value: u32) {
        // SAFETY: 寄存器为对齐的 32 位 MMIO。
        unsafe { write_volatile((self.base + off) as *mut u32, value); }
    }
    fn ids(&self) -> usize { if self.wide { 0x90 } else { 0x8c } }
    fn idi(&self) -> usize { if self.wide { 0x94 } else { 0x90 } }
    fn sync_dma(&self, pa: usize, size: usize) {
        (self.fence)();
        for line in (pa..pa + size).step_by(64) {
            // SAFETY: CCACHE FLUSH64 接受物理 cache line 地址；并非普通 fence 刷新。
            unsafe { write_volatile((self.cache + 0x200) as *mut u64, line as u64); }
            (self.fence)();
        }
    }
    fn clear(&self, off: usize, mask: u32) -> Result<(), ()> {
        for _ in 0..10000000 { if self.rd(off) & mask == 0 { return Ok(()); } }
        Err(())
    }
    fn command(&self, cmd: u32, arg: u32, flags: u32) -> Result<(), ()> {
        self.wr(0x44, u32::MAX); self.wr(0x28, arg); (self.fence)();
        self.wr(0x2c, START | HOLD | flags | cmd);
        for _ in 0..10000000 {
            let status = self.rd(0x44);
            if status & ERROR != 0 { return Err(()); }
            if status & 4 != 0 { self.wr(0x44, status); return Ok(()); }
        }
        Err(())
    }
    fn clock(&self, div: u32) -> Result<(), ()> {
        self.wr(0x10, 0); self.wr(0x08, div); self.wr(0x0c, 0);
        self.wr(0x2c, START | HOLD | (1 << 21) | (1 << 13)); self.clear(0x2c, START)?;
        self.wr(0x10, 1); self.wr(0x2c, START | HOLD | (1 << 21) | (1 << 13)); self.clear(0x2c, START)
    }
    /// # Safety
    /// 单个静态实例，独占 SDIO1 和 DESC；MMIO/CCACHE 映射长期有效，DESC 恒等映射。
    /// U-Boot 已建立 3.3V、引脚、电源和 <=200MHz ciu；无其他设备使用本 DMA 内存。
    pub unsafe fn init(&mut self, base: usize, cache: usize, fence: fn()) -> Result<(), ()> {
        self.base = base; self.cache = cache; self.fence = fence;
        self.wr(0x24, 0); self.wr(0, 7); self.clear(0, 7)?;
        self.wide = self.rd(0x70) & (1 << 27) != 0;
        self.wr(self.idi(), 0); self.wr(0x80, 1); self.clear(0x80, 1)?;
        self.wr(4, 1); self.wr(0x18, 0); self.wr(0x74, 0); self.wr(0x14, u32::MAX);
        self.clock(255)?; self.command(0, 0, 1 << 15)?; self.command(8, 0x1aa, RESP)?;
        if self.rd(0x30) & 0xfff != 0x1aa { return Err(()); }
        let mut ocr = 0;
        for _ in 0..10000 {
            self.command(55, 0, RESP)?; self.command(41, 0x40300000, 1 << 6)?;
            ocr = self.rd(0x30); if ocr & START != 0 { break; }
        }
        if ocr & 0xc0000000 != 0xc0000000 { return Err(()); }
        self.command(2, 0, RESP | (1 << 7))?; self.command(3, 0, RESP)?;
        let rca = self.rd(0x30) & 0xffff0000;
        if rca == 0 { return Err(()); }
        self.command(9, rca, RESP | (1 << 7))?;
        if self.rd(0x3c) >> 30 != 1 { return Err(()); }
        let sectors = (1 + u64::from((self.rd(0x34) >> 16) | ((self.rd(0x38) & 0x3f) << 16))) * 1024;
        if sectors < FIRST + SECTORS { return Err(()); }
        self.command(7, rca, RESP)?; self.clear(0x48, 1 << 9)?; self.clock(4)?;
        self.wr(0x4c, (2 << 28) | (15 << 16) | 16);
        self.wr(self.ids(), u32::MAX); self.wr(0x44, u32::MAX);
        self.wr(0, (1 << 25) | (1 << 5) | (1 << 4)); self.active = false; Ok(())
    }
    /// # Safety
    /// pa 是页对齐、4096 字节连续 DRAM；完成前 CPU 不访问/释放该页。
    /// 调用者的条件锁串行化所有方法；Sd 本身不跨睡眠借用。
    pub unsafe fn submit(&mut self, pa: usize, block: u32, write: bool) -> Result<(), ()> {
        if self.active || u64::from(block) * 8 + 8 > SECTORS || pa % 4096 != 0 { return Err(()); }
        self.clear(0x48, 1 << 9)?;
        self.wr(0x24, 0); self.wr(self.idi(), 0); self.wr(0, self.rd(0) | 6); self.clear(0, 6)?;
        self.wr(0x80, 1); self.clear(0x80, 1)?;
        let desc = (&raw mut DESC).cast::<u32>();
        // SAFETY: 上次请求已经完成，CPU 独占 DESC，原始指针不生成 DMA 期间的引用。
        unsafe {
            for i in 0..16 { desc.add(i).write(0); }
            desc.write(START | 8 | 4);
            desc.add(if self.wide {2} else {1}).write(4096);
            desc.add(if self.wide {4} else {2}).write(pa as u32);
            if self.wide { desc.add(5).write((pa >> 32) as u32); }
        }
        self.data = pa; self.sync_dma(pa, 4096); self.sync_dma(desc as usize, 64);
        self.wr(0x88, desc as usize as u32); if self.wide { self.wr(0x8c, ((desc as usize) >> 32) as u32); }
        self.wr(self.ids(), u32::MAX); self.wr(0x44, u32::MAX); self.seen = 0; self.dma_seen = 0;
        self.wr(0x1c, 512); self.wr(0x20, 4096);
        self.wr(0x80, (1 << 7) | 2); self.wr(self.idi(), 0x337); self.wr(0x24, ERROR | 4 | 8 | (1 << 14));
        self.active = true;
        self.wr(0x28, (FIRST + u64::from(block) * 8) as u32); (self.fence)();
        self.wr(0x2c, START | HOLD | RESP | (1 << 9) | (1 << 12) | (1 << 13) | if write {(1 << 10) | 25} else {18});
        Ok(())
    }
    /// # Safety
    /// init 已成功且 MMIO 仍映射，调用者持有与 submit 相同的条件锁。
    pub unsafe fn complete(&mut self) -> Option<Result<(), ()>> {
        let status = self.rd(0x44); let dma = self.rd(self.ids());
        self.wr(0x44, status); self.wr(self.ids(), dma);
        if !self.active { return None; }
        self.seen |= status; self.dma_seen |= dma;
        // 错误时停止内核流程并保留 DMA 内存，不能在所有权未确定时归还页。
        assert!(self.seen & ERROR == 0 && self.dma_seen & 0x234 == 0, "SDIO transfer error; DMA memory retained");
        let done = 4 | 8 | (1 << 14);
        if self.seen & done != done || self.dma_seen & 3 == 0 { return None; }
        assert!(self.rd(0x30) & 0xfdffe008 == 0, "SD card R1 error");
        self.clear(0x48, 1 << 9).expect("SD card busy; DMA memory retained");
        self.wr(0x24, 0); self.wr(self.idi(), 0); self.wr(0x80, 0);
        let desc = (&raw mut DESC).cast::<u32>();
        self.sync_dma(desc as usize, 64);
        // SAFETY: 完成和 DATA_BUSY 已确认；刷新后只读取控制字确认所有权。
        assert!(unsafe { read_volatile(desc) } & (START | (1 << 30)) == 0, "SD descriptor ownership");
        self.sync_dma(self.data, 4096); self.active = false; Some(Ok(()))
    }
}
