//! 虚拟内存与 Sv39 页表: 负责"内核应映射哪些区域、权限怎么给"。
//! PTE 位偏移等 RISC-V 规范细节交给 `hal::arch::mm`, 地址来自平台常量。

use core::sync::atomic::{AtomicUsize, Ordering};

use oslab_hal::arch;
use oslab_hal::platform;

use super::pmem;

/// Sv39 每个页表 512 项, 每项 8 字节, 正好一页。
pub const PTE_PER_TABLE: usize = 512;

/// 页大小 —— 与 [`pmem::PAGE_SIZE`] 同一个值, 都来自 arch 层。
pub use oslab_hal::arch::mm::PAGE_SIZE;

// 链接脚本提供的符号: 内核镜像 (含 .bss 与内核栈) 之后的第一个可用地址。
unsafe extern "C" {
    static ALLOC_BEGIN: u8;
}

/// 一个内核页表。
///
/// 只存根页的物理页号: 恒等映射下物理地址可直接当指针读写 PTE。
/// `mapped` 只用于自检与打印。
#[derive(Debug, Clone, Copy)]
pub struct PageTable {
    /// 根页表的物理页号。
    root: arch::mm::PhysPageNum,
    /// 已映射的页数 (仅用于自检与打印)。
    mapped: usize,
}

impl PageTable {
    /// 用一个已有的根页表物理页号构造 (用于每进程页表)。
    pub fn from_root(root: arch::mm::PhysPageNum) -> Self {
        Self { root, mapped: 0 }
    }

    /// 根页表的物理页号。
    pub fn root_ppn(&self) -> arch::mm::PhysPageNum {
        self.root
    }
}

/// 编译期断言: 一个页表正好放得下一页的 PTE。
const _: () = {
    assert!(PTE_PER_TABLE * core::mem::size_of::<usize>() == PAGE_SIZE);
};

// 内核全局页表。
static mut KERNEL_PT: Option<PageTable> = None;

// 页表是否已建立。
static PT_READY: AtomicUsize = AtomicUsize::new(0);

/// 分配一个清零的页表页, 返回它的物理页号。
///
/// 无效 PTE 就是全 0 (RISC-V: V=0 无效), 所以页表页必须清零。
/// `pmem::pmem_alloc` 保证返回的页已清零, 这里依赖该不变量。
fn alloc_table() -> Option<arch::mm::PhysPageNum> {
    let pa = pmem::pmem_alloc(pmem::Pool::Kernel);
    if pa == 0 {
        return None;
    }
    debug_assert!(pa % PAGE_SIZE == 0);
    Some(arch::mm::PhysAddr(pa).page_num())
}

/// 取出第 `idx` 项的**可写引用**。
///
/// # Safety
/// `ppn` 必须是有效的页表页, `idx` 必须小于 [`PTE_PER_TABLE`]。
unsafe fn pte_at(ppn: arch::mm::PhysPageNum, idx: usize) -> &'static mut arch::mm::PageTableEntry {
    debug_assert!(idx < PTE_PER_TABLE);
    // 恒等映射: 物理地址可以直接当指针用。
    let base = ppn.0 << 12;
    // SAFETY: 由调用者保证 ppn 有效且 idx 在界内。'static 是因为
    // 页表页的生命周期与内核相同 (内核页表从不释放)。
    unsafe { &mut *((base + idx * 8) as *mut arch::mm::PageTableEntry) }
}

/// 沿页表下行, 返回承载第 `level` 级那些项的那个页表页 (必要时创建)。
///
/// 注意返回的是"表"不是"表项": `level=0` 返回末级 (装 4KiB 叶子)
/// 的页表页, `level=1` 返回第二级 (装 2MiB 大页) 的页表页。
///
/// 循环从 `level+1` 下行到 `PAGE_LEVELS-1`, 只建出 level 以上的
/// 中间级。若某级已有大页叶子则返回 `None` (避免悄悄覆盖生效映射)。
fn walk_create(pt: &mut PageTable, va: usize, level: usize) -> Option<arch::mm::PhysPageNum> { unimplemented!() }

/// 把一个 4 KiB 页映射进页表。
///
/// 参数收齐"虚拟地址 + 物理页号 + 权限", 把页号/权限的转换收进这里,
/// 上层只表达意图。
///
/// # Safety
/// `va` 必须页对齐; 调用者必须保证不冲突, 且映射后当前代码与栈
/// 仍然有映射 (若随后会激活此页表)。
pub unsafe fn map(
    pt: &mut PageTable,
    va: usize,
    ppn: arch::mm::PhysPageNum,
    perms: arch::mm::Perms,
) -> Result<(), &'static str> { unimplemented!() }

/// 2 MiB 大页的大小。
///
/// 大页是"能不能映射得下"的问题: 按 4KiB 映射 128MiB DRAM 需要约
/// 32832 个页表页 (需从内核池出), 按 2MiB 只须 2 页。地址与大小需
/// 按 2MiB 对齐, 不对齐部分由 [`map_range`] 退回 4KiB 映射。
pub const MEGAPAGE_SIZE: usize = 2 * 1024 * 1024;

/// 用 2 MiB 大页映射一个 2 MiB 对齐的区间。
///
/// # Safety
/// `base` 与 `size` 都必须按 [`MEGAPAGE_SIZE`] 对齐; 调用者必须保证
/// 不冲突, 且映射后当前代码与栈仍可访问 (若随后会激活此页表)。
pub unsafe fn map_megapages(
    pt: &mut PageTable,
    base: usize,
    size: usize,
    perms: arch::mm::Perms,
) -> Result<(), &'static str> {
    if base % MEGAPAGE_SIZE != 0 || size % MEGAPAGE_SIZE != 0 {
        return Err("map_megapages: 地址或大小没有 2 MiB 对齐");
    }
    let vaddr = arch::mm::VirtAddr(base);
    // level=1 -> 返回装着 2MiB 大叶子的第二级页表。
    let table = walk_create(pt, base, 1).ok_or("map_megapages: 无法分配二级页表页")?;
    let count = size / MEGAPAGE_SIZE;
    for i in 0..count {
        let idx = vaddr.vpn(1) + i;
        if idx >= PTE_PER_TABLE {
            return Err("map_megapages: 跨过了 1 GiB 边界");
        }
        // SAFETY: table 是 walk_create 保证存在的二级页表页; idx 在界内。
        let pte = unsafe { pte_at(table, idx) };
        // 2MiB 叶子的 PPN 低 9 位必须为 0, 对齐由上面的检查保证。
        let pa = arch::mm::PhysAddr(base + i * MEGAPAGE_SIZE);
        *pte = arch::mm::PageTableEntry::new(pa.page_num(), arch::mm::perm_to_pte(perms));
        pt.mapped += 1;
    }
    Ok(())
}

/// 映射一段连续的地址区间 (恒等映射: va == pa)。
///
/// 尽量用 2 MiB 大页: 两头不足 2 MiB 的部分退回 4 KiB 页。这是
/// 内核建立映射最常用的入口。
///
/// # Safety
/// 同 [`map`]。
pub unsafe fn map_range(
    pt: &mut PageTable,
    base: usize,
    size: usize,
    perms: arch::mm::Perms,
) -> Result<(), &'static str> {
    if size == 0 {
        return Ok(());
    }
    let start = base & !(PAGE_SIZE - 1);
    let end = pmem::align_up(base + size, PAGE_SIZE);

    // 先对齐到 2MiB 边界, 用大页覆盖中间的大块, 两头不足的用 4KiB。
    let mega_start = pmem::align_up(start, MEGAPAGE_SIZE);
    let mega_end = pmem::align_down(end, MEGAPAGE_SIZE);

    let head_end = core::cmp::min(mega_start, end);
    let mut va = start;
    while va < head_end {
        // SAFETY: 由调用者保证 (见函数文档)。
        unsafe { map(pt, va, arch::mm::PhysAddr(va).page_num(), perms)? };
        va += PAGE_SIZE;
    }

    if mega_start < mega_end {
        // SAFETY: 已按 2 MiB 对齐 (上面两个 align 保证)。
        unsafe { map_megapages(pt, mega_start, mega_end - mega_start, perms)? };
    }

    let mut va = core::cmp::max(mega_end, head_end);
    while va < end {
        // SAFETY: 由调用者保证。
        unsafe { map(pt, va, arch::mm::PhysAddr(va).page_num(), perms)? };
        va += PAGE_SIZE;
    }
    Ok(())
}

// ===========================================================================

/// 建立内核页表 (恒等映射)。
///
/// 映射三类区域: 内核镜像、全部 DRAM、设备 MMIO。设备类最容易漏。
pub fn kvm_init() { }

/// 为一个新进程创建页表: 复制内核页表的映射, 但用户部分留空。
///
/// 新建一张根表, 把内核根表内容整份抄过来, 之后在低地址项里填本
/// 进程的用户页面。两个进程由此可在同一虚拟地址放各自的代码 (fork
/// 成立的前提)。
pub fn kvm_create_process_table() -> Option<arch::mm::PhysPageNum> {
    let kernel = kvm_global()?;
    let root = alloc_table()?;
    // SAFETY: kernel.root 与 root 都是有效页表页且二者不同, 不存在别名。
    unsafe {
        for i in 0..PTE_PER_TABLE {
            let src = pte_at(kernel.root, i).clone();
            *pte_at(root, i) = src;
        }
    }

    // 关键一步: 把根表第 0 项 (覆盖 [0, 1GiB), 含用户地址空间与
    // 设备 MMIO) 再深拷一层。若只做值复制, 父子进程会共用同一张
    // 中间级页表, 子进程改 0x1000 的叶子会连累父进程。
    //
    // SAFETY: 与上面的复制同理; L1 是新分配页, 不与 kernel 的表重叠。
    unsafe {
        let l0 = pte_at(kernel.root, 0);
        if l0.is_valid() && !l0.is_leaf() {
            let Some(copy) = alloc_table() else {
                return None;
            };
            let src = l0.ppn();
            for i in 0..PTE_PER_TABLE {
                let e = pte_at(src, i).clone();
                *pte_at(copy, i) = e;
            }
            *pte_at(root, 0) = arch::mm::PageTableEntry::new(copy, arch::mm::PTE_V);
        }
    }
    Some(root)
}

/// 用给定的根页表翻译一个虚拟地址。
///
/// 与 [`kvm_translate`] 的区别只是用哪张表: 系统调用必须用当前进程
/// 的表, 否则会去翻译别的进程 (或内核) 的地址。
pub fn kvm_translate_in(root: arch::mm::PhysPageNum, va: usize) -> Option<usize> {
    let pt = PageTable { root, mapped: 0 };
    translate_with(&pt, va)
}

/// 取内核全局页表。
pub fn kvm_global() -> Option<&'static mut PageTable> {
    if PT_READY.load(Ordering::Acquire) == 0 {
        return None;
    }
    // SAFETY: PT_READY 为 1 说明 KERNEL_PT 已初始化。内核页表在整个
    // 生命周期内一直存在, 且只有启动核创建; 之后的访问由调用者保证
    // 不并发修改同一项。
    //
    // 用 `addr_of_mut!` 而不是 `KERNEL_PT.as_mut()`: Rust 2024 把
    // `static_mut_refs` 提升为错误, 先取裸指针再解引用把别名风险
    // 显式写在代码里。
    unsafe { (*core::ptr::addr_of_mut!(KERNEL_PT)).as_mut() }
}

/// 在当前 hart 上激活内核页表。
///
/// 每个 hart 都要调用: 页表基址寄存器是每个 hart 独立的, 启动核
/// 激活了不等于从核也有。
pub fn kvm_init_hart() {
    let Some(pt) = kvm_global() else {
        oslab_hal::putchar::puts("[oslab-rs] FATAL: kvm_init_hart 之前没有 kvm_init\n");
        arch::time::park_current_hart();
    };
    let root = pt.root;

    // SAFETY: 恒等映射已覆盖内核镜像、DRAM 与设备 —— 即当前代码、
    // 栈及后续访问的全部数据 (activate_page_table 的 Safety 前提)。
    unsafe {
        arch::mm::activate_page_table(root);
    }
}

/// 页表统计: (根页表物理地址, 已映射页数)。给启动自检打印用。
pub fn kvm_stat() -> Option<(usize, usize)> {
    kvm_global().map(|pt| (pt.root.0 << 12, pt.mapped))
}

/// 手工翻译一个虚拟地址 (调试用)。
///
/// 恒等映射下应返回相同值 —— 这条"应当相同"就是 lab-3 最有价值的
/// 一条自检, 它验证页表真的在工作。
pub fn kvm_translate(va: usize) -> Option<usize> {
    let pt = kvm_global()?;
    translate_with(pt, va)
}

/// 用给定的页表翻译地址 (内部实现)。
fn translate_with(pt: &PageTable, va: usize) -> Option<usize> {
    // 偏移掩码取决于叶子在哪一级: level 0 -> &0xfff, level 1 -> &0x1f_ffff。
    // 用错粒度会在 2MiB 大页内部丢掉高位偏移。
    let (pte, level) = walk_leveled(pt, va)?;
    let mask = match level {
        1 => MEGAPAGE_SIZE - 1,
        _ => PAGE_SIZE - 1,
    };
    let pa = (pte.ppn().0 << 12) | (va & mask);
    Some(pa)
}

/// 与遍历页表相同, 但**同时返回叶子所在的级数**。
///
/// 翻译需要级数来决定偏移掩码 (4KiB 用 0xfff, 2MiB 用 0x1f_ffff)。
fn walk_leveled(pt: &PageTable, va: usize) -> Option<(arch::mm::PageTableEntry, usize)> {
    let vaddr = arch::mm::VirtAddr(va);
    let mut ppn = pt.root;
    for level in (0..arch::mm::PAGE_LEVELS).rev() {
        // SAFETY: ppn 由 root 出发逐级取得, 每一级都检查过 is_valid。
        let pte = unsafe { pte_at(ppn, vaddr.vpn(level)) };
        if !pte.is_valid() {
            return None;
        }
        if pte.is_leaf() {
            return Some((*pte, level));
        }
        ppn = pte.ppn();
    }
    None
}

/// 分页是否已经打开。
pub fn paging_enabled() -> bool {
    arch::mm::paging_enabled()
}