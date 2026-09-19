//! `arch::trap_entry` — trap 入口的汇编实现 (RISC-V)。
//!
//! 全是 CSR 名字与 RISC-V 指令, 没有一行 OS 语义, 所以放 arch 层。
//! 暴露两样东西: [`TRAP_ENTRY`] (入口地址, 交给
//! [`crate::arch::trap::install_vector`] 装到 `stvec`) 与 `trap_entry` 符号。
//! kernel 提供一个 `extern "C" fn trap_handler(*mut TrapFrame)` 由汇编调用
//! —— 这是唯一接口。

use crate::arch::trap::{TrapFrame, TRAPFRAME_SIZE};

// 把汇编入口按语义化名字导出, 避开 `pub mod trap_entry` 与函数同名冲突
// (Rust 不允许同名既是模块又是值)。调用点读 `install_vector(arch::TRAP_ENTRY)`。
pub use self::asm_entry as TRAP_ENTRY;

unsafe extern "C" {
    /// 汇编写的 trap 入口。返回 `!` (never): 处理完执行 `sret` 回到被中断
    /// 的上下文, 不 `ret` 回调用者 (进入时没有"调用者")。
    #[link_name = "trap_entry"]
    pub fn asm_entry() -> !;
}

core::arch::global_asm!(
    r#"
.section .text
.align 4
.global trap_entry
trap_entry:
    # ---- 第 1 步: 原子换栈 ----
    # 交换后: sp = 内核栈顶, sscratch = 用户 sp。必须一条指令 (csrrw):
    # 拆开写中间若发生中断, 会在不可信的用户栈上执行 trap 处理。
    csrrw   sp, sscratch, sp

    # ---- 第 2 步: 判断 trap 来自哪个特权级 ----
    # sp 非 0 -> 用户态进来的正常路径; sp 为 0 -> 内核态路径。内核态运行时
    # sscratch 恒为 0 (返回用户态前才设成内核栈顶), 所以交换后 sp 拿到 0。
    beqz    sp, .Lfrom_kernel

    # ---- 第 3 步: 从用户态进来的路径: 在内核栈上分配一帧 trapframe ----
    # 栈向下增长, 所以减去一帧大小。
    addi    sp, sp, -{tf_size}

    # ---- 第 4 步: 保存 x1..x31 ----
    # 展开写而非循环: 循环需一个寄存器做计数器, 而它也需被保存 (鸡生蛋)。
    # 用明确的 sd 而非宏生成: 每一行对应一个寄存器/偏移, 出问题能用肉眼对表查。
    sd      ra, 0*8(sp)
    # 1*8 = sp, 稍后从 sscratch 取
    sd      gp, 2*8(sp)
    sd      tp, 3*8(sp)
    sd      t0, 4*8(sp)
    sd      t1, 5*8(sp)
    sd      t2, 6*8(sp)
    sd      s0, 7*8(sp)
    sd      s1, 8*8(sp)
    sd      a0, 9*8(sp)
    sd      a1, 10*8(sp)
    sd      a2, 11*8(sp)
    sd      a3, 12*8(sp)
    sd      a4, 13*8(sp)
    sd      a5, 14*8(sp)
    sd      a6, 15*8(sp)
    sd      a7, 16*8(sp)
    sd      s2, 17*8(sp)
    sd      s3, 18*8(sp)
    sd      s4, 19*8(sp)
    sd      s5, 20*8(sp)
    sd      s6, 21*8(sp)
    sd      s7, 22*8(sp)
    sd      s8, 23*8(sp)
    sd      s9, 24*8(sp)
    sd      s10, 25*8(sp)
    sd      s11, 26*8(sp)
    sd      t3, 27*8(sp)
    sd      t4, 28*8(sp)
    sd      t5, 29*8(sp)
    sd      t6, 30*8(sp)

    # ---- 第 5 步: 保存用户 sp (在 sscratch 里) ----
    # 它被第 1 步的 csrrw 交换进了 sscratch。
    csrr    t0, sscratch
    sd      t0, {off_sp}(sp)

    # ---- 第 6 步: 清零 sscratch ----
    # 此刻内核 sp 已在 sp 里, sscratch 暂无用; 清零让第 2 步的 beqz 走内核路径。
    csrw    sscratch, zero

    # 跳去公共的收尾部分。
    j       .Lsave_common

.Lfrom_kernel:
    # ---- 内核态 trap 的路径 ----
    # 第 1 步的 csrrw 对两条路径都执行了: 内核态时 sscratch 恒为 0, 所以那
    # 次交换把真正的内核 sp 换进了 sscratch, sp 变成 0。这里再交换一次把它
    # 拿回来, 同时 sscratch 恢复 0 (不变量不变)。此前直接 addi 会让 sp=-272,
    # 后续 sd ra 触发 store page fault, 表现为"内核随机崩溃"。
    csrrw   sp, sscratch, sp

    addi    sp, sp, -{tf_size}

    sd      ra, 0*8(sp)
    sd      gp, 2*8(sp)
    sd      tp, 3*8(sp)
    sd      t0, 4*8(sp)
    sd      t1, 5*8(sp)
    sd      t2, 6*8(sp)
    sd      s0, 7*8(sp)
    sd      s1, 8*8(sp)
    sd      a0, 9*8(sp)
    sd      a1, 10*8(sp)
    sd      a2, 11*8(sp)
    sd      a3, 12*8(sp)
    sd      a4, 13*8(sp)
    sd      a5, 14*8(sp)
    sd      a6, 15*8(sp)
    sd      a7, 16*8(sp)
    sd      s2, 17*8(sp)
    sd      s3, 18*8(sp)
    sd      s4, 19*8(sp)
    sd      s5, 20*8(sp)
    sd      s6, 21*8(sp)
    sd      s7, 22*8(sp)
    sd      s8, 23*8(sp)
    sd      s9, 24*8(sp)
    sd      s10, 25*8(sp)
    sd      s11, 26*8(sp)
    sd      t3, 27*8(sp)
    sd      t4, 28*8(sp)
    sd      t5, 29*8(sp)
    sd      t6, 30*8(sp)

    # 内核态的 sp: 就是当前的 sp (加回这一帧的大小)。
    addi    t0, sp, {tf_size}
    sd      t0, {off_sp}(sp)

    # 常量引用 (仅用于让编译器知道它们都被用到; 展开后是一条注释):
    #   tf_size={tf_size} off_sepc={off_sepc} off_ksp={off_ksp}
    #   off_status={off_status} off_sp={off_sp} off_tp={off_tp}

.Lsave_common:
    # ---- 第 7 步: 保存 sepc 与 sstatus ----
    # sepc: 触发 trap 的 PC。sstatus 必须存: sret 的目标特权级由 SPP 决定,
    # 而硬件在陷入时把 SPP 设成"进入前的特权级" —— 若不显式恢复, 从内核态
    # 陷入过一次后 SPP 停在 1, 直接 sret 会错误地返回到 S-mode。
    csrr    t0, sepc
    sd      t0, {off_sepc}(sp)
    csrr    t0, sstatus
    sd      t0, {off_status}(sp)

    # 内核栈顶记进 trapframe, 调试时能一眼看出这一帧属于哪个 CPU。
    addi    t0, sp, {tf_size}
    sd      t0, {off_ksp}(sp)

    # ---- 第 8 步: 调用 Rust 处理函数 ----
    # 参数 a0 = trapframe 指针。
    mv      a0, sp
    call    trap_handler

    # ---- 第 9 步: **重新加载** trapframe 指针 ----
    # a0 既是入参寄存器也是返回值寄存器: trap_handler 返回后 a0 装的是返回值,
    # 不再指向 trapframe。从 sp 重新取回 (sp 在整个 trap 处理期间始终指向帧)。
    mv      a0, sp
    j       .Lrestore

    # ---- 公共的返回路径 ----
    # 调用者可能是 trap_handler, 也可能是切换到一个从未运行过的新进程的
    # 调度器 —— 必须能从**任意** trapframe 恢复。约定: a0 = trapframe 指针,
    # 不要再在这里写 `mv a0, sp`: enter_user 在任意内核栈上调用本路径, sp
    # 并不指向 trapframe, 覆盖 a0 会让 sret 跳到地址 0 (instruction page fault)。
.global trap_return
trap_return:
    j       .Lrestore

.Lrestore:
    # 让 sp 指向 trapframe —— 后续用固定偏移恢复寄存器。
    mv      sp, a0

    # ---- 恢复 sepc ----
    ld      t0, {off_sepc}(sp)
    csrw    sepc, t0

    # ---- 恢复 sstatus ----
    # Rust 侧 trap_handler 返回前已把 trapframe.status 改成目标状态 (显式设置
    # SPP/SPIE, 见 build_user_return_status / build_kernel_return_status)。必须
    # 显式设置: 硬件在陷入时自动做的 SPP/SPIE 反映"进入时", 而非"离开时"。
    # 放在汇编里写, 保证恢复寄存器与写 sstatus 的顺序固定不可被插队。
    ld      t0, {off_status}(sp)
    csrw    sstatus, t0

    # ---- 恢复通用寄存器 ----
    ld      ra, 0*8(sp)
    ld      gp, 2*8(sp)
    ld      tp, 3*8(sp)
    ld      t0, 4*8(sp)
    ld      t1, 5*8(sp)
    ld      t2, 6*8(sp)
    ld      s0, 7*8(sp)
    ld      s1, 8*8(sp)
    ld      a0, 9*8(sp)
    ld      a1, 10*8(sp)
    ld      a2, 11*8(sp)
    ld      a3, 12*8(sp)
    ld      a4, 13*8(sp)
    ld      a5, 14*8(sp)
    ld      a6, 15*8(sp)
    ld      a7, 16*8(sp)
    ld      s2, 17*8(sp)
    ld      s3, 18*8(sp)
    ld      s4, 19*8(sp)
    ld      s5, 20*8(sp)
    ld      s6, 21*8(sp)
    ld      s7, 22*8(sp)
    ld      s8, 23*8(sp)
    ld      s9, 24*8(sp)
    ld      s10, 25*8(sp)
    ld      s11, 26*8(sp)
    ld      t3, 27*8(sp)
    ld      t4, 28*8(sp)
    ld      t5, 29*8(sp)
    ld      t6, 30*8(sp)

    # ---- 恢复被保存的 sp, 并处理 sscratch ----
    # 返回到用户态: sp = 用户 sp, sscratch = 内核栈顶 (为下次 trap 准备);
    # 返回到内核态: sp = 内核栈顶, sscratch 保持 0。用 trapframe 里保存的
    # sstatus.SPP 区分 (0 -> U, 1 -> S), 所以 sstatus 必须在恢复 sp 前读。
    ld      t0, {off_status}(sp)
    srli    t0, t0, 8
    andi    t0, t0, 1               # t0 = SPP
    bnez    t0, .Lrestore_kernel_sp

    # --- 目标是 U-mode: sp = 保存的用户 sp, sscratch = 内核栈顶 ---
    ld      t0, {off_sp}(sp)        # t0 = 用户 sp
    addi    t1, sp, {tf_size}       # t1 = 内核栈顶
    csrw    sscratch, t1            # 为下一次 trap 准备好 (必须在 sp 还是内核栈时写)
    mv      sp, t0                  # sp = 用户 sp
    sret

.Lrestore_kernel_sp:
    # --- 目标是 S-mode: sp = 内核栈顶, sscratch 保持 0 ---
    addi    sp, sp, {tf_size}
    sret
"#,
    // ---- 编译期常量: 全部来自结构体定义本身 (offset_of!) ----
    tf_size = const { TRAPFRAME_SIZE },
    off_sepc = const { core::mem::offset_of!(TrapFrame, sepc) },
    off_ksp = const { core::mem::offset_of!(TrapFrame, kernel_sp) },
    off_status = const { core::mem::offset_of!(TrapFrame, status) },
    // regs[1] = x2 = sp
    off_sp = const { (2 - 1) * 8 },
    // regs[3] = x4 = tp。汇编暂不按偏移访问它, 保留此常量"用"一下,
    // 避免将来需要时重新推导, 也让编译器不报未使用。
    off_tp = const { (4 - 1) * 8 },
);
