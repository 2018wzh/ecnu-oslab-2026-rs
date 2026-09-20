//! VirtIO MMIO v2 描述符池和三描述符请求链。OS 层负责串行化、睡眠与唤醒。
use core::{mem::offset_of, ptr::{read_volatile, write_volatile}};
#[repr(C)]
#[derive(Clone, Copy)]
struct Desc { address: u64, len: u32, flags: u16, next: u16 }
#[repr(C)]
struct Avail { flags: u16, index: u16, ring: [u16; 8], event: u16 }
#[repr(C)]
#[derive(Clone, Copy)]
struct UsedEntry { id: u32, len: u32 }
#[repr(C)]
struct Used { flags: u16, index: u16, ring: [UsedEntry; 8], event: u16 }
#[repr(C, align(16))]
pub struct Header { pub kind: u32, pub reserved: u32, pub sector: u64 }
#[repr(C, align(4096))]
pub struct Queue { desc: [Desc; 8], avail: Avail, used: Used, status: [u8; 8] }
impl Queue {
    pub const ZERO: Self = Self {
        desc: [Desc { address: 0, len: 0, flags: 0, next: 0 }; 8],
        avail: Avail { flags: 0, index: 0, ring: [0; 8], event: 0 },
        used: Used { flags: 0, index: 0, ring: [UsedEntry { id: 0, len: 0 }; 8], event: 0 },
        status: [0; 8],
    };
}
pub struct VirtioBlock { base: usize, queue: *mut Queue, consumed: u16, free: [bool; 8], pa: usize, fence: fn() }
impl VirtioBlock {
    fn read(&self, off: usize) -> u32 {
        // SAFETY: init 的 MMIO 生命周期契约。
        unsafe { read_volatile((self.base + off) as *const u32) }
    }
    fn write(&self, off: usize, value: u32) {
        // SAFETY: init 的 MMIO 生命周期契约。
        unsafe { write_volatile((self.base + off) as *mut u32, value); }
    }
    fn address(&self, off: usize, pa: usize) { self.write(off, pa as u32); self.write(off + 4, (pa >> 32) as u32); }
    /// # Safety
    /// MMIO 有效；queue 稳定、独占、DMA 可达且 pa 是其物理地址。
    /// fence 提供平台所需的 DMA/MMIO 顺序保证；设备存活期间不能移动/释放 queue。
    pub unsafe fn init(base: usize, queue: *mut Queue, pa: usize, fence: fn()) -> Result<Self, ()> {
        if base == 0 { return Err(()); }
        let d = Self { base, queue, consumed: 0, free: [true; 8], pa, fence };
        if d.read(0) != 0x74726976 || d.read(4) != 2 || d.read(8) != 2 { return Err(()); }
        d.write(0x70, 0); fence(); d.write(0x70, 1); d.write(0x70, 3);
        d.write(0x14, 0); if d.read(0x10) & (1 << 5) != 0 { return Err(()); }
        d.write(0x14, 1); if d.read(0x10) & 1 == 0 { return Err(()); }
        d.write(0x24, 0); d.write(0x20, 0); d.write(0x24, 1); d.write(0x20, 1);
        d.write(0x70, 11); fence(); if d.read(0x70) & 8 == 0 { return Err(()); }
        d.write(0x30, 0); if d.read(0x44) != 0 || d.read(0x34) < 8 { return Err(()); }
        // SAFETY: 调用者独占队列，此时尚未启用 DMA。
        unsafe {
            queue.write(Queue::ZERO);
        }
        d.write(0x38, 8);
        d.address(0x80, pa + offset_of!(Queue, desc));
        d.address(0x90, pa + offset_of!(Queue, avail));
        d.address(0xa0, pa + offset_of!(Queue, used));
        fence(); d.write(0x44, 1); d.write(0x70, 15); Ok(d)
    }
    /// # Safety
    /// header_pa 指向 16 字节稳定请求头，data_pa 指向 4096 字节连续独占页。
    /// 两者在完成之前不可移动、释放或由 CPU 访问；调用者用条件锁串行化方法。
    pub unsafe fn submit(&mut self, header_pa: usize, data_pa: usize, write: bool) -> Option<usize> {
        let mut ids = [0; 3];
        let mut n = 0;
        for i in 0..8 { if self.free[i] && n < 3 { ids[n] = i; n += 1; } }
        if n != 3 { return None; }
        for i in ids { self.free[i] = false; }
        let h = ids[0];
        // SAFETY: 只更新本次新占用的描述符和驱动拥有的 avail 字段。
        unsafe {
            let q = self.queue;
            (*q).status[h] = 0xff;
            (*q).desc[h] = Desc { address: header_pa as u64, len: 16, flags: 1, next: ids[1] as u16 };
            (*q).desc[ids[1]] = Desc { address: data_pa as u64, len: 4096, flags: if write {1} else {3}, next: ids[2] as u16 };
            (*q).desc[ids[2]] = Desc { address: (self.pa + offset_of!(Queue, status) + h) as u64, len: 1, flags: 2, next: 0 };
            let index = (*q).avail.index;
            (*q).avail.ring[usize::from(index % 8)] = h as u16;
            (self.fence)(); write_volatile(&raw mut (*q).avail.index, index.wrapping_add(1));
        }
        (self.fence)(); self.write(0x50, 0); Some(h)
    }
    /// # Safety
    /// head 为本驱动已完成且尚未归还的请求头；设备已交还所有权。
    pub unsafe fn release(&mut self, mut head: usize) {
        loop {
            // SAFETY: 调用者持有条件锁，本链 DMA 已结束。
            let entry = unsafe { (*self.queue).desc[head] };
            self.free[head] = true;
            if entry.flags & 1 == 0 { break; }
            head = entry.next as usize;
        }
    }
    pub fn complete(&mut self) -> Option<(usize, Result<(), ()>)> {
        let irq = self.read(0x60); if irq != 0 { self.write(0x64, irq & 3); }
        // SAFETY: 仅 volatile 读取设备拥有字段；不建立全队列引用。
        unsafe {
            let q = self.queue;
            if read_volatile(&raw const (*q).used.index) == self.consumed { return None; }
            (self.fence)();
            let id = read_volatile(&raw const (*q).used.ring[usize::from(self.consumed % 8)].id) as usize;
            assert!(id < 8 && !self.free[id], "invalid device completion");
            let status = read_volatile(&raw const (*q).status[id]);
            self.consumed = self.consumed.wrapping_add(1);
            Some((id, if status == 0 { Ok(()) } else { Err(()) }))
        }
    }
}
