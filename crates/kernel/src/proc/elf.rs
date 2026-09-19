//! ELF64 加载器。
//!
//! 之前用户程序以扁平二进制嵌入 (见 build.rs 的 generate_user_images),
//! 有两个问题: 入口地址靠约定 (内核必须知道入口=USER_BASE), 段间空洞
//! 无法表达 (按字节装入会错位)。ELF 把这两件事显式写下来: `e_entry`
//! 即入口地址, 每个 program header 描述"把文件 off 开始的 filesz 字节
//! 装到 vaddr, 清零到 memsz"。装载于是变成"照着 program header 搬"。
//! 只实现可执行文件需要的部分, 不需要符号表/重定位/动态链接/节头表。

use crate::mm::{pmem, vm};
use oslab_hal::arch::mm::{Perms, PhysAddr};

/// ELF magic: `\x7fELF`。
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// `EI_CLASS` = 2 表示 64 位。
const ELFCLASS64: u8 = 2;

/// 程序头类型: 可装载段。
const PT_LOAD: u32 = 1;

/// 程序头标志: 可执行。
const PF_X: u32 = 1;
/// 程序头标志: 可写。
const PF_W: u32 = 2;
/// 程序头标志: 可读。
const PF_R: u32 = 4;

/// 用户地址空间的下界。
///
/// 与 `proc::user::USER_BASE` 一致 —— 不映射第 0 页, 空指针解引用立刻
/// 缺页而非静默读到有效内存。
pub const USER_MIN: usize = 0x1000;

/// 用户地址空间的上界 (不含)。
///
/// 挡两类错误: 装到内核地址 (ELF 声称装 0x80200000, 照做等于让用户覆盖
/// 内核 → 提权); 装到设备 MMIO (把设备映射覆盖成普通内存页, 设备"消失",
/// 本项目踩过 —— 用户栈曾被放 0x1000_0000, 恰是 UART0)。所以上界取
/// "最低的设备地址" (两平台都是 CLINT 0x0200_0000), 而非"看起来够大"
/// 的常数。
pub const USER_MAX: usize = oslab_hal::platform::PLATFORM.devices_base;

// 编译期断言: 用户地址空间整体位于设备 MMIO 之下 (钉死上面这个前提)。
const _: () = {
    assert!(USER_MIN < USER_MAX);
    assert!(USER_MAX <= oslab_hal::platform::PLATFORM.devices_base);
};

/// 装载失败的原因。
///
/// 区分这么细, 因为各指向不同的排查方向 (文件不是 ELF / 是 32 位 /
/// 是别的架构 / 想装到内核或设备地址上 / 内存不够)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    /// 文件太小, 连 ELF header 都不完整。
    TooSmall,
    /// magic 不对 —— 不是 ELF 文件。
    BadMagic,
    /// 不是 64 位 ELF。
    Not64,
    /// 不是小端序。
    NotLittleEndian,
    /// 不是 RISC-V 的目标文件。
    WrongMachine,
    /// 段落在用户地址空间之外。
    OutOfUserRange,
    /// 分配物理页失败 (内存不足)。
    LoadFailed,
    /// 没有可装载的段。
    NoLoadableSegment,
}

/// ELF64 header 里需要的字段。
///
/// 逐字段解析而非按 `repr(C)` 结构体映射: 编译器会插入对齐填充, 而
/// ELF 字段偏移是规范规定死的 (如 e_entry 在偏移 24); 类型宽度猜错
/// 会全部错位且"看起来是合理数字", 不立即暴露。
#[derive(Debug, Clone, Copy)]
pub struct ElfHeader {
    /// 入口地址。
    pub entry: u64,
    /// 程序头表在文件里的偏移。
    pub phoff: u64,
    /// 程序头的个数。
    pub phnum: u16,
    /// 每个程序头的大小 (规范允许不是 56)。
    pub phentsize: u16,
}

/// 一个可装载段。
#[derive(Debug, Clone, Copy)]
pub struct LoadSegment {
    /// 段在文件里的偏移。
    pub offset: u64,
    /// 段应当被装到的虚拟地址。
    pub vaddr: u64,
    /// 文件里有多少字节要拷过去。
    pub filesz: u64,
    /// 在内存里占多少字节 (多出的部分清零, 那是 .bss)。
    pub memsz: u64,
    /// 段标志 (PF_R / PF_W / PF_X)。
    pub flags: u32,
}

/// 读一个小端 u16。
#[inline]
fn rd_u16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

/// 读一个小端 u32。
#[inline]
fn rd_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// 读一个小端 u64。
#[inline]
fn rd_u64(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes([
        b[off],
        b[off + 1],
        b[off + 2],
        b[off + 3],
        b[off + 4],
        b[off + 5],
        b[off + 6],
        b[off + 7],
    ])
}

/// 解析 ELF header。
///
/// 字段偏移来自 ELF 规范 (64 位), 写全以便对着规范核对:
///   e_ident[16] (0) / e_type u16 (16) / e_machine u16 (18) /
///   e_entry u64 (24) / e_phoff u64 (32) / e_phentsize u16 (54) /
///   e_phnum u16 (56)。
pub fn parse_header(image: &[u8]) -> Result<ElfHeader, ElfError> {
    if image.len() < 64 {
        return Err(ElfError::TooSmall);
    }
    if image[0..4] != ELF_MAGIC {
        return Err(ElfError::BadMagic);
    }
    if image[4] != ELFCLASS64 {
        return Err(ElfError::Not64);
    }
    // 期望的字节序与机器类型来自 arch 层 (架构的事实, 加载器不负责
    // 知道哪种机器码是本内核的)。
    if image[5] != oslab_hal::arch::ELF_DATA {
        return Err(ElfError::NotLittleEndian);
    }
    if rd_u16(image, 18) != oslab_hal::arch::ELF_MACHINE {
        return Err(ElfError::WrongMachine);
    }

    Ok(ElfHeader {
        entry: rd_u64(image, 24),
        phoff: rd_u64(image, 32),
        phentsize: rd_u16(image, 54),
        phnum: rd_u16(image, 56),
    })
}

/// 解析第 `i` 个程序头。
pub fn parse_segment(image: &[u8], hdr: &ElfHeader, i: usize) -> Result<LoadSegment, ElfError> {
    let off = hdr.phoff as usize + i * hdr.phentsize as usize;
    if off + 56 > image.len() {
        return Err(ElfError::TooSmall);
    }
    Ok(LoadSegment {
        flags: rd_u32(image, off + 4),
        offset: rd_u64(image, off + 8),
        vaddr: rd_u64(image, off + 16),
        filesz: rd_u64(image, off + 32),
        memsz: rd_u64(image, off + 40),
    })
}

/// 把段标志翻译成页权限。
///
/// ELF 权限是段级的、页表是页级的, 直接按段给权限取并集 (偏宽松;
/// 课程序很小, 每段的页边界清楚, 简化安全)。每个用户页都必须带 USER 位。
fn perm_of(flags: u32) -> Perms {
    let mut p = Perms::NONE;
    if flags & PF_R != 0 {
        p = p.or(Perms::READ);
    }
    if flags & PF_W != 0 {
        p = p.or(Perms::WRITE);
    }
    if flags & PF_X != 0 {
        p = p.or(Perms::EXEC);
    }
    p.or(Perms::USER)
}

/// 把 `image` 里的可装载段装进用户地址空间, 返回入口地址。
///
/// 每个段三件事: 按页分配物理内存并映射到 vaddr 对齐后的页; 把文件里
/// filesz 字节拷进去; 把 filesz..memsz 清零 (.bss)。第 3 步不能省:
/// 文件不存 .bss, 那部分内存是残留 (忘了清症状是"全局变量初值非 0")。
/// 实际上 `pmem_alloc` 返回的页已清零, 只需要覆盖有内容的部分。
pub fn load_into(
    pgtbl: &mut vm::PageTable,
    image: &[u8],
) -> Result<u64, ElfError> { unimplemented!() }

/// 检查入口地址落在用户地址空间内。
///
/// 段检查只保证"装进来的内存"在用户区, 入口是单独字段: 恶意/损坏的
/// ELF 可声明合法段 + 指向内核的入口, `sret` 过去就是提权。
pub fn check_entry(entry: u64) -> Result<(), ElfError> {
    let e = entry as usize;
    if e < USER_MIN || e >= USER_MAX {
        return Err(ElfError::OutOfUserRange);
    }
    Ok(())
}