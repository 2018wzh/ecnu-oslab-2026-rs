//! 用户虚拟内存: 用户地址空间布局常量、进程地址空间装载/复制,
//! 以及从用户内存安全读写的唯一入口 (`copy_from_user` / `copy_to_user` /
//! `copy_str_from_user`) —— 它们走页表翻译, 不做裸解引用。
//!
//! 系统调用参数里的用户地址不能直接解引用 (可能未映射导致内核缺页,
//! 或映射到内核内存构成提权), 所有读写用户内存的路径都收敛到这里。

use oslab_hal::arch::TrapFrame;

use crate::mm::pmem;
use crate::proc;

/// 从用户地址读一个字节。
///
/// 返回 `None` 表示地址不属于用户空间 (或未映射)。逐字节版本便于
/// 看懂正确性, 本阶段不需处理跨页的批量拷贝。校验在此做全:
/// 地址必须低于内核基地址、必须已映射 —— 否则用户可让内核读任意内存。
pub(crate) unsafe fn copy_from_user(va: usize) -> Option<u8> { unimplemented!() }

/// 用**当前进程**的页表翻译一个用户虚拟地址。
///
/// 不能用 `kvm_translate` (它走内核全局页表): 每进程页表下同一虚拟
/// 地址在各进程指向不同的物理页, 用全局页表会读到别人的数据。
pub(crate) fn user_translate(va: usize) -> Option<usize> {
    let p = proc::current()?;
    if p.pgtbl == 0 {
        return None;
    }
    crate::mm::vm::kvm_translate_in(oslab_hal::arch::mm::PhysAddr(p.pgtbl).page_num(), va)
}

/// 把**一个字节**写到用户地址。
///
/// 与 [`copy_from_user`] 对称: 先确认目标在用户区且已映射再写, 否则
/// 用户传内核地址就能让内核改写内核内存。
///
/// # Safety
/// `va` 来自用户; 内部走页表翻译, 不做裸解引用。
pub(crate) unsafe fn copy_to_user(va: usize, b: u8) -> Option<()> {
    let kernel_base = oslab_hal::arch::cpu::platform().kernel_base;
    if va >= kernel_base {
        return None;
    }
    let pa = user_translate(va)?;
    // SAFETY: pa 已映射且来自用户区。
    unsafe {
        *(pa as *mut u8) = b;
    }
    Some(())
}

/// 从用户地址读一个以 `'\0'` 结尾的字符串。
///
/// `dst` 的长度是硬性护栏: 用户字符串长度不可信, 读满还没遇到结尾
/// 就返回 `false`。拷一份进内核缓冲区: 路径解析可能睡眠, 睡眠期间
/// 用户可能改掉那块内存 (TOCTOU)。
///
/// # Safety
/// `va` 来自用户, 内部逐字节走 [`copy_from_user`], 不做裸解引用。
pub(crate) unsafe fn copy_str_from_user(va: usize, dst: &mut [u8]) -> bool {
    for i in 0..dst.len() {
        // SAFETY: 调用者保证这是系统调用参数里的用户地址。
        match unsafe { copy_from_user(va + i) } {
            Some(0) => {
                dst[i] = 0;
                return true;
            }
            Some(b) => dst[i] = b,
            None => return false,
        }
    }
    false
}

// ---- 用户地址空间: 布局常量 + 装载/复制 (自 proc::user 迁入) ----
/// 一个用户进程的初始栈大小 (字节)。
///
/// 一页。用户程序的第一个栈帧 (argc/argv 之类) 放在这一页的高端。
pub const USER_STACK_SIZE: usize = pmem::PAGE_SIZE;

/// 用户程序在虚拟地址空间里的加载地址。
///
/// 必须与 `configs/arch/<arch>.toml` 的 `user_base` (0x1000) 一致,
/// 不一致的后果很隐蔽 (见 xtask 的架构规则检查会核对这两个值)。
/// 从 0x1000 而非 0 开始: 第 0 页不映射, 空指针解引用立刻缺页而非
/// 静默读到有效内存。
pub const USER_BASE: usize = 0x1000;

/// 装载一段用户程序映像到新进程, 并让它"准备好在用户态运行" ——
/// 建立每进程页表、映射代码+栈、填 trapframe。
pub fn proc_make_user(pid: usize, image: &[u8]) -> bool { unimplemented!() }

/// 复制一个进程的用户地址空间 (fork 用)。
///
/// "真的拷贝每一页": fork 要求父子互不影响, 只复制页表项会让它们
/// 指向同一批物理页 (那成了线程)。此处直接全量拷贝, 语义正确、实现
/// 简单, 代价是 fork 慢一些 (真实内核用 COW)。
///
/// # Safety
/// 两个根页号都必须来自 `kvm_create_process_table` / 内核页表, 且
/// 调用期间没有别的 hart 在改这两张表。
pub unsafe fn copy_user_space(parent_root_pa: usize, child_root_pa: usize) -> bool { unimplemented!() }

/// 用户栈的虚拟地址。
///
/// 固定地址而非紧跟数据段: 后者让栈起点随程序大小变化, 栈溢出会踩到
/// 全局变量 (难定位); 固定地址让栈溢出撞上明确的空洞, 立刻缺页。
/// 取 1 MiB: 高于用户代码段 (0x1000 起), 又低于两平台最低的设备地址
/// (CLINT 0x0200_0000) —— 否则会覆盖设备 MMIO 映射, 表现为"进用户态
/// 后一个字符都打不出"。
pub const USER_STACK_BASE: usize = 0x0010_0000;

// ---------------------------------------------------------------------------
// 编译期断言: 用户栈不能与设备 MMIO 重叠。
// `PLATFORM` 是 const, 断言在编译期求值, 换平台时地址不成立会直接编译失败。
// ---------------------------------------------------------------------------
const _: () = {
    // 栈必须在用户代码段之上 (否则会覆盖代码)。
    assert!(USER_STACK_BASE >= 0x1000 + 0x1000);
    // 栈必须整体落在最低的设备地址之下。
    assert!(USER_STACK_BASE + USER_STACK_SIZE <= oslab_hal::platform::PLATFORM.devices_base);
};

// ---------------------------------------------------------------------------
// 用户堆 / 栈 / mmap (对齐 2025 lab-5 任务 2-5)
// 这些是本阶段的学生任务 (unimplemented)。学生按 2025 的语义实现:
//   uvm_heap_grow/ungrow  用户堆顶增长/回缩 (brk)
//   uvm_ustack_grow       用户栈缺页自动增长
//   uvm_mmap/uvm_munmap   mmap 区域建立/解除
//   uvm_destroy/uvm_copy_address_space  用户页表销毁/复制 (fork 用)
// ---------------------------------------------------------------------------

/// 用户堆顶增长到 `cur_heap_top + len`; 需要时为堆分配并映射新的物理页。
pub fn uvm_heap_grow(cur_heap_top: usize, len: usize) -> Option<usize> {
    unimplemented!()
}

/// 用户堆顶回缩到 `cur_heap_top - len`; 需要时回收超出部分的物理页。
pub fn uvm_heap_ungrow(cur_heap_top: usize, len: usize) -> Option<usize> {
    unimplemented!()
}

/// 用户栈自动增长: 判断 `fault_addr` 是否为合理的栈扩展地址, 是则分配并映射新页。
pub fn uvm_ustack_grow(old_ustack_npage: usize, fault_addr: usize) -> Option<usize> {
    unimplemented!()
}

/// 建立/返回一个 mmap 区域。
pub fn uvm_mmap(begin: usize, npages: u32, permit_write: bool) -> Option<usize> {
    unimplemented!()
}

/// 解除一段 mmap 区域 (回收其物理页)。
pub fn uvm_munmap(begin: usize, npages: u32) {
    unimplemented!()
}

/// 销毁整个用户地址空间 (回收所有用户物理页)。
pub fn uvm_destroy() {
    unimplemented!()
}

/// 把旧用户地址空间的用户部分复制到新的用户页表 (fork 用)。
pub unsafe fn uvm_copy_address_space(old_root_pa: usize, new_root_pa: usize) -> bool {
    unimplemented!()
}
