//! MMIO 访问原语: 统一提供 volatile 的寄存器读写。
//!
//! 用 `write_volatile`/`read_volatile` 防止编译器删除或缓存设备读写,
//! 类型化的宽度 API 避免写错字节数。不做地址合法性检查 —— 地址
//! 正确性由 platform 层的编译期断言和页表映射保证。

use core::marker::PhantomData;

/// 一个内存映射 I/O 区域, 封装基地址。
#[derive(Debug, Clone, Copy)]
pub struct Mmio {
    base: usize,
}

impl Mmio {
    /// 从物理地址构造。
    ///
    /// # Safety
    /// `base` 必须指向已映射的设备寄存器区域, 且生命周期覆盖本值的使用期。
    pub const unsafe fn new(base: usize) -> Self {
        Self { base }
    }

    /// 基地址。
    pub const fn base(&self) -> usize {
        self.base
    }

    /// 读一个 8 位寄存器。
    ///
    /// # Safety
    /// `offset` 必须落在该设备的合法寄存器范围内。
    #[inline]
    pub unsafe fn read_u8(&self, offset: usize) -> u8 {
        let p = (self.base + offset) as *const u8;
        // SAFETY: 由调用者保证地址已映射且 `offset` 合法。
        // `read_volatile` 阻止编译器缓存或删除这次读。
        unsafe { core::ptr::read_volatile(p) }
    }

    /// 写一个 8 位寄存器。
    ///
    /// # Safety
    /// `offset` 必须落在该设备的合法寄存器范围内。
    #[inline]
    pub unsafe fn write_u8(&self, offset: usize, v: u8) {
        let p = (self.base + offset) as *mut u8;
        // SAFETY: 由调用者保证地址已映射且 `offset` 合法。
        unsafe { core::ptr::write_volatile(p, v) }
    }

    /// 读一个 16 位寄存器。
    ///
    /// # Safety
    /// `offset` 必须落在合法寄存器范围内且 2 字节对齐。
    #[inline]
    pub unsafe fn read_u16(&self, offset: usize) -> u16 {
        let p = (self.base + offset) as *const u16;
        // SAFETY: 由调用者保证地址已映射且 `offset` 合法、2 字节对齐。
        unsafe { core::ptr::read_volatile(p) }
    }

    /// 写一个 16 位寄存器。
    ///
    /// # Safety
    /// 同上, 且 2 字节对齐。
    #[inline]
    pub unsafe fn write_u16(&self, offset: usize, v: u16) {
        let p = (self.base + offset) as *mut u16;
        // SAFETY: 见上。
        unsafe { core::ptr::write_volatile(p, v) }
    }

    /// 读一个 32 位寄存器。
    ///
    /// # Safety
    /// `offset` 必须落在合法范围内且 4 字节对齐 —— RISC-V 对未对齐的设备访问可能触发异常。
    #[inline]
    pub unsafe fn read_u32(&self, offset: usize) -> u32 {
        let p = (self.base + offset) as *const u32;
        // SAFETY: 见上。
        unsafe { core::ptr::read_volatile(p) }
    }

    /// 写一个 32 位寄存器。
    ///
    /// # Safety
    /// 同 [`Mmio::read_u32`]。
    #[inline]
    pub unsafe fn write_u32(&self, offset: usize, v: u32) {
        let p = (self.base + offset) as *mut u32;
        // SAFETY: 见上。
        unsafe { core::ptr::write_volatile(p, v) }
    }

    /// 读一个 64 位寄存器。
    ///
    /// # Safety
    /// 同 [`Mmio::read_u32`], 且要求 8 字节对齐。
    #[inline]
    pub unsafe fn read_u64(&self, offset: usize) -> u64 {
        let p = (self.base + offset) as *const u64;
        // SAFETY: 见上。
        unsafe { core::ptr::read_volatile(p) }
    }

    /// 写一个 64 位寄存器。
    ///
    /// # Safety
    /// 同 [`Mmio::read_u64`]。
    #[inline]
    pub unsafe fn write_u64(&self, offset: usize, v: u64) {
        let p = (self.base + offset) as *mut u64;
        // SAFETY: 见上。
        unsafe { core::ptr::write_volatile(p, v) }
    }

    /// 读-改-写: 把 `bits` 置位。
    ///
    /// # Safety
    /// 同 [`Mmio::read_u32`]。注意读-改-写不是原子的, 初始化单线程可用,
    /// 运行期共享的寄存器需自备互斥。
    #[inline]
    pub unsafe fn set_bits_u32(&self, offset: usize, bits: u32) {
        // SAFETY: 由调用者保证地址与 offset 合法。
        let v = unsafe { self.read_u32(offset) };
        unsafe { self.write_u32(offset, v | bits) };
    }

    /// 读-改-写: 把 `bits` 清零。
    ///
    /// # Safety
    /// 同 [`Mmio::set_bits_u32`]。
    #[inline]
    pub unsafe fn clear_bits_u32(&self, offset: usize, bits: u32) {
        // SAFETY: 由调用者保证地址与 offset 合法。
        let v = unsafe { self.read_u32(offset) };
        unsafe { self.write_u32(offset, v & !bits) };
    }
}

/// 一个"必须按特定顺序访问"的寄存器序列的标记类型。
///
/// 文档性占位: 让类型签名能表达"这个操作序列不可重排"。真正的顺序
/// 保证靠 `fence_w()`, 因为 RISC-V 是弱内存序架构。
#[derive(Debug)]
pub struct Ordered<T> {
    _marker: PhantomData<T>,
}
