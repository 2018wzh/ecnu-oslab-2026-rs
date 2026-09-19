//! `arch::mm` — Sv39 页表与地址翻译。
//!
//! Sv39: 虚拟地址 39 位 = VPN[2](9)+VPN[1](9)+VPN[0](9)+offset(12);
//! 三级页表, 每级 512 项 × 8 字节。PTE 布局: 63:54 reserved | 53:10 PPN
//! | 9:8 RSW | 7 D | 6 A | 5 G | 4 U | 3 X | 2 W | 1 R | 0 V。`V=1` 且
//! R/W/X 全 0 表示指向下一级页表, 否则是叶子项。
//!
//! 用 [`PhysAddr`]/[`VirtAddr`]/[`PhysPageNum`] 三个 newtype 区分
//! 地址、页号与 PTE, 把"忘记右移 12 位"从运行期静默卡死变成编译期报错。

use crate::arch::csr;

/// 页大小: 4 KiB。
pub const PAGE_SIZE: usize = 4096;
/// 页内偏移的位数。
pub const PAGE_SHIFT: usize = 12;
/// Sv39 每级页表的项数: 4096 / 8 = 512。
pub const PTE_PER_TABLE: usize = 512;
/// Sv39 的页表级数。
pub const PAGE_LEVELS: usize = 3;
/// Sv39 虚拟地址的有效位数。
pub const VA_BITS: usize = 39;

/// 物理地址。
///
/// `#[repr(transparent)]` 使内存表示与 `usize` 相同, 零开销, 可用于 `asm!`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysAddr(pub usize);

/// 虚拟地址。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtAddr(pub usize);

/// 物理页号 = 物理地址 >> 12。
///
/// `satp` 存 PPN 而非物理地址。有了这个类型,"把地址当页号用"在编译期
/// 不可能: 只能通过 [`PhysAddr::page_num`] 或 [`PhysAddr::align_down`] 构造。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysPageNum(pub usize);

impl PhysAddr {
    /// 向下对齐到页边界。
    pub const fn align_down(self) -> PhysAddr {
        PhysAddr(self.0 & !(PAGE_SIZE - 1))
    }

    /// 取页号 (右移 12 位)。这是**唯一**能把物理地址变成 PPN 的地方。
    pub const fn page_num(self) -> PhysPageNum {
        PhysPageNum(self.0 >> PAGE_SHIFT)
    }

    /// 页内偏移。
    pub const fn page_offset(self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }

    /// 是否页对齐。
    pub const fn is_aligned(self) -> bool {
        self.page_offset() == 0
    }

    /// 裸值。
    pub const fn raw(self) -> usize {
        self.0
    }
}

impl VirtAddr {
    /// 向下对齐到页边界。
    pub const fn align_down(self) -> VirtAddr {
        VirtAddr(self.0 & !(PAGE_SIZE - 1))
    }

    /// 向上对齐到页边界。
    pub const fn align_up(self) -> VirtAddr {
        VirtAddr((self.0 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
    }

    /// 页内偏移。
    pub const fn page_offset(self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }

    /// 是否页对齐。
    pub const fn is_aligned(self) -> bool {
        self.page_offset() == 0
    }

    /// 取出第 `level` 级页号 (9 位)。
    ///
    /// `level` 取 0/1/2, 对应 VPN[0]/VPN[1]/VPN[2]。
    pub const fn vpn(self, level: usize) -> usize {
        (self.0 >> (PAGE_SHIFT + 9 * level)) & 0x1ff
    }

    /// 裸值。
    pub const fn raw(self) -> usize {
        self.0
    }

    /// Sv39 虚拟地址是否满足"第 38 位符号扩展" (canonical)。
    ///
    /// Sv39 只用 39 位地址, 高位必须是第 38 位的符号扩展, 否则是"非规范
    /// 地址", 硬件在翻译前直接触发缺页。那样无法与"页表真没映射"区分,
    /// 提前检查能给出更明确的错误。
    pub const fn is_canonical(self) -> bool {
        // 合法 <=> 高 25 位 (63:39) 全部等于第 38 位。
        let bit38 = (self.0 >> 38) & 1;
        let upper = self.0 >> 39;
        if bit38 == 1 {
            upper == (1 << 25) - 1
        } else {
            upper == 0
        }
    }
}

// ===========================================================================
// PTE 标志位
// ===========================================================================

/// 有效位。
pub const PTE_V: usize = 1 << 0;
/// 可读。
pub const PTE_R: usize = 1 << 1;
/// 可写。
pub const PTE_W: usize = 1 << 2;
/// 可执行。
pub const PTE_X: usize = 1 << 3;
/// 用户态可访问。
pub const PTE_U: usize = 1 << 4;
/// 全局映射 (不随 ASID 切换失效)。
pub const PTE_G: usize = 1 << 5;
/// 已访问 (硬件在翻译时置位)。
pub const PTE_A: usize = 1 << 6;
/// 已写过 (硬件在写入时置位)。
pub const PTE_D: usize = 1 << 7;

/// 内存权限, 与架构无关的表述。
///
/// VM 代码只用这些标志 (不含 RISC-V 位号); 翻译集中在 [`perm_to_pte`]
/// 一处 —— 换架构只需重写那一个函数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Perms(pub usize);

impl Perms {
    /// 只读。
    pub const READ: Perms = Perms(1);
    /// 可写。
    pub const WRITE: Perms = Perms(2);
    /// 可执行。
    pub const EXEC: Perms = Perms(4);
    /// 用户态可访问。
    pub const USER: Perms = Perms(8);

    /// 空权限。
    pub const NONE: Perms = Perms(0);

    /// 组合两个权限。
    pub const fn or(self, other: Perms) -> Perms {
        Perms(self.0 | other.0)
    }

    /// 是否包含某个权限。
    pub const fn has(self, other: Perms) -> bool {
        self.0 & other.0 == other.0
    }
}

/// 把与架构无关的权限翻译成 RISC-V 的 PTE 位。
///
/// 这里**显式**置 A/D 位, 不依赖硬件: RISC-V 规范允许但**不强制**硬件在
/// 翻译时自动置位, 某些真实 hart (如 VF2 的 U74) 在严格模式下不置,
/// 会导致"刚开机就随机缺页"。
pub const fn perm_to_pte(p: Perms) -> usize {
    let mut pte = PTE_V;
    if p.has(Perms::READ) {
        pte |= PTE_R;
    }
    if p.has(Perms::WRITE) {
        pte |= PTE_W;
    }
    if p.has(Perms::EXEC) {
        pte |= PTE_X;
    }
    if p.has(Perms::USER) {
        pte |= PTE_U;
    }
    // 显式置 A/D; 纯执行页 (R=0,W=0,X=1) 规范禁止置 D, 只置 A。
    if p.has(Perms::READ) || p.has(Perms::WRITE) {
        pte |= PTE_A | PTE_D;
    } else {
        pte |= PTE_A;
    }
    pte
}

/// 一个页表项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct PageTableEntry(pub usize);

impl PageTableEntry {
    /// 空项。
    pub const fn empty() -> Self {
        PageTableEntry(0)
    }

    /// 是否有效。
    pub const fn is_valid(&self) -> bool {
        self.0 & PTE_V != 0
    }

    /// 是否是叶子项 (直接映射一个物理页), 而非指向下一级页表。
    ///
    /// 判断依据: `V=1` 且 `R|W|X` 不全为 0 (RISC-V 规范的约定)。
    pub const fn is_leaf(&self) -> bool {
        self.is_valid() && (self.0 & (PTE_R | PTE_W | PTE_X)) != 0
    }

    /// 取出指向的物理页号。
    ///
    /// PPN 从第 10 位开始 (低 10 位是标志位), 偏移是 RISC-V 特有的,
    /// 所以这个函数在这里而非内核通用代码。
    pub const fn ppn(&self) -> PhysPageNum {
        PhysPageNum((self.0 >> 10) & ((1 << 44) - 1))
    }

    /// 从物理页号和标志位构造一个 PTE。
    pub const fn new(ppn: PhysPageNum, flags: usize) -> Self {
        PageTableEntry(((ppn.0 & ((1 << 44) - 1)) << 10) | flags)
    }
}

// ===========================================================================
// satp
// ===========================================================================

/// 构造 Sv39 模式的 `satp` 值。
///
/// 参数是 `PhysPageNum` 而非 `PhysAddr`, 让"把物理地址写进 satp"
/// 变成编译错误而不是运行期静默卡死。
pub const fn make_satp_sv39(root: PhysPageNum) -> usize {
    // MODE (bit 63:60) | ASID (59:44, 本内核恒为 0) | PPN (43:0)
    (csr::SATP_MODE_SV39 << 60) | (root.0 & ((1 << 44) - 1))
}

/// 激活一个页表: 写 `satp` 并刷新 TLB。
///
/// # Safety
/// 调用者必须保证目标页表已为**当前代码、当前栈及接下来访问的所有数据**
/// 建立映射, 否则写 `satp` 后下一条指令取指就缺页。
pub unsafe fn activate_page_table(root: PhysPageNum) {
    // `write_satp_raw` 内部已做 `sfence.vma`, 把"切页表必须刷 TLB"
    // 与写 satp 放在同一处而非让每个调用点记得。
    unsafe {
        csr::write_satp_raw(make_satp_sv39(root));
    }
}

/// 关闭分页 (回到 Bare 模式)。
///
/// # Safety
/// 关闭后所有地址按物理地址解释。本内核用恒等映射, 所以安全; 若内核
/// 在开启分页后用了与物理地址不同的虚拟地址, 调用它会立刻跑飞。
pub unsafe fn deactivate_page_table() {
    unsafe {
        csr::write_satp_raw(0);
    }
}

/// 读取当前 `satp` 里的根页表页号。
pub fn current_root_ppn() -> PhysPageNum {
    PhysPageNum(csr::read_satp() & ((1 << 44) - 1))
}

/// `satp` 当前是否处于分页开启状态。
pub fn paging_enabled() -> bool {
    let mode = csr::read_satp() >> 60;
    mode == csr::SATP_MODE_SV39
}

// ===========================================================================
// 编译期自检
// ===========================================================================
const _: () = {
    // newtype 必须与 usize 同大小, 才能在 `asm!` 里直接用。
    assert!(core::mem::size_of::<PhysAddr>() == 8);
    assert!(core::mem::size_of::<VirtAddr>() == 8);
    assert!(core::mem::size_of::<PhysPageNum>() == 8);
    assert!(core::mem::size_of::<PageTableEntry>() == 8);

    // 页号换算正确性 (移位数写错正是 satp bug 的根源)。
    assert!(PhysAddr(0x8020_0000).page_num().0 == 0x80200);
    assert!(PhysAddr(0x8020_0fff).page_num().0 == 0x80200);
    assert!(PhysAddr(0x8020_1000).page_num().0 == 0x80201);

    // satp 编码: MODE 在最高 4 位, PPN 在低 44 位。
    let s = make_satp_sv39(PhysPageNum(0x80200));
    assert!(s >> 60 == 8);
    assert!(s & ((1 << 44) - 1) == 0x80200);

    // 虚拟地址分级 (页号算错会让"映射了但访问缺页"无从下手)。
    // 0x8020_0000 >> 12 = 0x80200 -> VPN[0]; >> 21 = 0x401 -> VPN[1];
    // >> 30 = 0x2 -> VPN[2]。
    assert!(VirtAddr(0x8020_0000).vpn(0) == 0x000);
    assert!(VirtAddr(0x8020_0000).vpn(1) == 0x001);
    assert!(VirtAddr(0x8020_0000).vpn(2) == 0x002);

    // 规范地址检查。
    assert!(VirtAddr(0x8020_0000).is_canonical());
    assert!(VirtAddr(0).is_canonical());
    assert!(VirtAddr(0xffff_ffff_ffff_ffff).is_canonical());
    // 0x0000_0080_0000_0000 高位不全为 0, 也不是符号扩展。
    assert!(!VirtAddr(0x0000_0080_0000_0000).is_canonical());

    // 权限翻译。用 `has_bits` 而非 trait 方法, 因为 trait 方法在
    // const 上下文里不可调用。
    assert!(has_bits(
        perm_to_pte(Perms::READ),
        PTE_V | PTE_R | PTE_A | PTE_D
    ));
    assert!(!has_bits(perm_to_pte(Perms::READ), PTE_W));
    assert!(has_bits(perm_to_pte(Perms::EXEC), PTE_X));
    // 纯执行页不能置 D (规范禁止 D=1 而 R=0,W=0)。
    assert!(!has_bits(perm_to_pte(Perms::EXEC), PTE_D));
};

/// 编译期辅助: `v` 是否包含 `bits` 里的**全部**位。
pub const fn has_bits(v: usize, bits: usize) -> bool {
    v & bits == bits
}
