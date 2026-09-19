//! `arch::trap` — 陷入 (trap) 的架构侧。
//!
//! trap 是 CPU 中止当前执行流跳到内核登记入口这件事, 分异常 (同步, 由
//! 刚才那条指令引起) 与中断 (异步, 由外部事件引起), 两者进同一个 `stvec`,
//! 靠 `scause` 最高位区分。这里只处理架构层面的事: 定义 trapframe 布局、
//! 解析 `scause`、安装 `stvec`。trap 来了要做什么是 OS 语义, 在 kernel/。
//!
//! trapframe 布局避免 C 版的 bug: 偏移量由 Rust `offset_of!` 经 build.rs
//! 生成常量文件给汇编, 唯一来源是结构体定义; 寄存器用数组 `[usize; 31]`
//! 而非 31 个具名字段。

use crate::arch::csr;

/// 通用寄存器的个数 (x1..x31)。x0 恒为 0, 不需要保存。
pub const N_REGISTERS: usize = 31;

/// 陷入内核时保存的完整现场。
///
/// `#[repr(C)]` 固定字段顺序 (汇编按固定偏移访问); `regs` 索引 `i` 对应
/// 寄存器 `x(i+1)` (x0 恒 0 不保存)。布局示意:
/// `regs[0..=30]` = x1..x31, 之后 `sepc`, `kernel_sp`, `status`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TrapFrame {
    /// x1..x31。索引 `i` 对应寄存器 `x(i+1)`。
    pub regs: [usize; N_REGISTERS],
    /// 触发 trap 的 PC。必须保存, 否则 `sret` 跳回原地, 同一 `ecall` 无限重复。
    pub sepc: usize,
    /// 进入 trap 前的内核栈顶 (内核态 trap 时 sp 未过 `sscratch`, 需它还原)。
    pub kernel_sp: usize,
    /// 进入 trap 时的 `sstatus` 快照。
    ///
    /// `sret` 的目标特权级由 `sstatus.SPP` 决定。若曾**从内核态**陷入,
    /// SPP 被设成 1, 直接 `sret` 会以 S-mode 执行用户代码 —— 静默卡死。
    /// 所以完整快照存进 trapframe, 返回时用它重建, 并显式置 SPP=0/SPIE=1。
    pub status: usize,
}

impl TrapFrame {
    /// 全部清零, 用于创建新进程时构造"空白现场"。
    pub const fn zeroed() -> Self {
        Self {
            regs: [0; N_REGISTERS],
            sepc: 0,
            kernel_sp: 0,
            status: 0,
        }
    }

    /// 按**寄存器号**读 (`1..=31`)。越界返回 0 而非 panic (trap 处理里再 panic 会加倍出错)。
    #[inline]
    pub fn reg(&self, regno: usize) -> usize {
        if (1..=N_REGISTERS).contains(&regno) {
            self.regs[regno - 1]
        } else {
            0
        }
    }

    /// 按**寄存器号**写 (`1..=31`)。
    #[inline]
    pub fn set_reg(&mut self, regno: usize, v: usize) {
        if (1..=N_REGISTERS).contains(&regno) {
            self.regs[regno - 1] = v;
        }
    }

    /// 栈指针 (x2)。
    #[inline]
    pub fn sp(&self) -> usize {
        self.reg(2)
    }
    /// 返回地址 (x1)。
    #[inline]
    pub fn ra(&self) -> usize {
        self.reg(1)
    }
    /// 系统调用号所在寄存器 (x17 = a7)。
    #[inline]
    pub fn a7(&self) -> usize {
        self.reg(17)
    }
    /// 系统调用第 0 个参数 (x10 = a0)。
    #[inline]
    pub fn a0(&self) -> usize {
        self.reg(10)
    }
    /// 取"陷入时正在执行的指令地址"。
    ///
    /// 不叫 `sepc`: 它是 RISC-V 寄存器名, kernel 想表达的语义是"那次陷入
    /// 发生在哪条指令"(aarch64 上是 `elr_el1`)。
    #[inline]
    pub fn faulting_pc(&self) -> usize {
        self.sepc
    }

    /// 设置"陷入时正在执行的指令地址"。
    ///
    /// 用途: 为用户程序伪造一个"从未发生过的陷入现场" (见 `kernel::proc::user`)。
    pub fn set_faulting_pc(&mut self, v: usize) {
        self.sepc = v;
    }

    /// 取"进入本次陷入前的特权级对应的栈指针"。
    ///
    /// 从用户态进来时这是**用户的** sp, 内核用它定位用户栈。
    pub fn user_sp(&self) -> usize {
        self.reg(2)
    }

    /// 设置用户栈指针。
    pub fn set_user_sp(&mut self, v: usize) {
        self.set_reg(2, v);
    }

    /// 取"本 hart 的内核栈顶"。
    ///
    /// 由内核在陷入时写入, trap 入口汇编换栈时会用它。
    pub fn kernel_stack_top(&self) -> usize {
        self.kernel_sp
    }

    /// 设置本 hart 的内核栈顶。
    pub fn set_kernel_stack_top(&mut self, v: usize) {
        self.kernel_sp = v;
    }

    /// 取"返回时应当恢复到的处理器状态字"。
    pub fn saved_status(&self) -> usize {
        self.status
    }

    /// 设置"返回时应当恢复到的处理器状态字"。
    pub fn set_saved_status(&mut self, v: usize) {
        self.status = v;
    }

    /// 取第 1 个参数寄存器 (`a1`)。
    ///
    /// ABI (`docs/abi-spec.md` §1) 规定系统调用参数放 `a0`..`a5`;
    /// 具名访问器避免调用点散落裸寄存器号。
    pub fn a1(&self) -> usize {
        self.reg(11)
    }

    /// 取第 2 个参数寄存器 (`a2`)。
    pub fn a2(&self) -> usize {
        self.reg(12)
    }

    /// 取第 3 个参数寄存器 (`a3`)。
    pub fn a3(&self) -> usize {
        self.reg(13)
    }

    /// 设置返回值寄存器 (`a0`)。
    ///
    /// 系统调用返回、`fork` 在子进程里返回 0 —— 都靠写这一个寄存器。
    pub fn set_a0(&mut self, v: usize) {
        self.set_reg(10, v);
    }
}

// ===========================================================================
// 编译期自检: 布局与汇编使用的偏移必须一致
// ===========================================================================
const _: () = {
    assert!(core::mem::size_of::<TrapFrame>() == 34 * 8);
    assert!(core::mem::align_of::<TrapFrame>() == 8);
    // 下面三条是"结构体真的按 repr(C) 排布"的证据 (offset_of! 是 const 的)。
    assert!(core::mem::offset_of!(TrapFrame, regs) == 0);
    assert!(core::mem::offset_of!(TrapFrame, sepc) == 31 * 8);
    assert!(core::mem::offset_of!(TrapFrame, kernel_sp) == 32 * 8);
    assert!(core::mem::offset_of!(TrapFrame, status) == 33 * 8);
};

/// trapframe 的总字节数, 供汇编使用。
pub const TRAPFRAME_SIZE: usize = core::mem::size_of::<TrapFrame>();

/// 陷入原因。
///
/// 用枚举而非 `usize` (`scause` 原值是位域): 一次性解码后分发只需 `match`,
/// 且编译器检查穷尽性, 不会再漏看最高位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapCause {
    /// 中断: 时钟。
    TimerInterrupt,
    /// 中断: 外部设备 (来自 PLIC)。
    ExternalInterrupt,
    /// 中断: 核间 (软件) 中断。
    SoftwareInterrupt,
    /// 中断: 编号无法识别。
    ///
    /// 保留而非丢弃, 因为"收到不认识的中断"必须**明确处理** (通常关掉,
    /// 否则会无限重复)。
    UnknownInterrupt(usize),
    /// 异常: U-mode 发起的 `ecall` —— 也就是系统调用。
    SyscallFromUser,
    /// 异常: S-mode 发起的 `ecall`。
    ///
    /// 内核自己 `ecall` 调 SBI 时, 参数/扩展号错会成一次 S-mode 异常回抛;
    /// 看到它就说明"SBI 调用被拒绝了"。
    SyscallFromKernel,
    /// 异常: 取指缺页。
    InstructionPageFault,
    /// 异常: 读缺页。
    LoadPageFault,
    /// 异常: 写缺页。
    StorePageFault,
    /// 异常: 非法指令。
    IllegalInstruction,
    /// 异常: 指令地址未对齐。
    InstructionMisaligned,
    /// 异常: 其他编号无法识别的异常。
    UnknownException(usize),
}

impl TrapCause {
    /// 从 `scause` 的原始值解码。
    pub fn decode(scause: usize) -> Self {
        let code = csr::scause_code(scause);
        if csr::scause_is_interrupt(scause) {
            match code {
                csr::IRQ_S_TIMER => TrapCause::TimerInterrupt,
                csr::IRQ_S_EXTERNAL => TrapCause::ExternalInterrupt,
                csr::IRQ_S_SOFTWARE => TrapCause::SoftwareInterrupt,
                other => TrapCause::UnknownInterrupt(other),
            }
        } else {
            match code {
                csr::EXC_ECALL_FROM_U => TrapCause::SyscallFromUser,
                csr::EXC_ECALL_FROM_S => TrapCause::SyscallFromKernel,
                csr::EXC_INST_PAGE_FAULT => TrapCause::InstructionPageFault,
                csr::EXC_LOAD_PAGE_FAULT => TrapCause::LoadPageFault,
                csr::EXC_STORE_PAGE_FAULT => TrapCause::StorePageFault,
                csr::EXC_ILLEGAL_INST => TrapCause::IllegalInstruction,
                csr::EXC_INST_MISALIGNED => TrapCause::InstructionMisaligned,
                other => TrapCause::UnknownException(other),
            }
        }
    }

    /// 是否是一次中断 (异步)。
    pub const fn is_interrupt(&self) -> bool {
        matches!(
            self,
            TrapCause::TimerInterrupt
                | TrapCause::ExternalInterrupt
                | TrapCause::SoftwareInterrupt
                | TrapCause::UnknownInterrupt(_)
        )
    }

    /// 人类可读的名字。用于日志与 panic 信息。
    pub const fn name(&self) -> &'static str {
        match self {
            TrapCause::TimerInterrupt => "timer interrupt",
            TrapCause::ExternalInterrupt => "external interrupt",
            TrapCause::SoftwareInterrupt => "software interrupt",
            TrapCause::UnknownInterrupt(_) => "unknown interrupt",
            TrapCause::SyscallFromUser => "syscall (from U)",
            TrapCause::SyscallFromKernel => "syscall (from S)",
            TrapCause::InstructionPageFault => "instruction page fault",
            TrapCause::LoadPageFault => "load page fault",
            TrapCause::StorePageFault => "store page fault",
            TrapCause::IllegalInstruction => "illegal instruction",
            TrapCause::InstructionMisaligned => "instruction misaligned",
            TrapCause::UnknownException(_) => "unknown exception",
        }
    }
}

/// 安装 trap 向量。
///
/// # Safety
/// `handler` 必须符合 RISC-V trap 入口约定: 第一条指令要在不依赖任何
/// 已有寄存器 (包括 sp) 的前提下工作, 因为 trap 可发生在任意上下文。
/// 安装后任何 trap 都会跳到它, 装错会直接跑飞。返回 `-> !` 保证普通
/// "处理完就返回"的函数无法被误装进来。
pub unsafe fn install_vector(handler: unsafe extern "C" fn() -> !) {
    // `write_stvec` 内部强制清低 2 位 (模式位); 本内核用 Direct 模式:
    // 所有 trap 到同一地址, 软件按 `scause` 分发, 避免 Vectored 的跳转表。
    // SAFETY: 由调用者保证 handler 满足 trap 入口约定。
    unsafe {
        csr::write_stvec(handler as usize);
    }
}

/// 一个"什么都不做"的 trap 占位入口, 在真正处理函数就绪前安装。
///
/// 防止启动早期 `stvec` 还是 0 时 trap 跳到地址 0 的静默死机 —— 先装
/// 这个死循环停车点, 行为变成"明确卡在这里", 用 gdb 看 `pc` 即知发生了什么。
///
/// # Safety
/// 它不是合法 trap 入口, 但作为占位安全: 不接受参数、不返回, 且无限循环。
pub unsafe extern "C" fn park_forever() -> ! {
    loop {
        // `wfi` 进低功耗等待; QEMU 上等价 `nop`, 真机省电且便于 JTAG 接管。
        unsafe {
            ::core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
        }
    }
}

// ===========================================================================
// 从 trap 返回时的 sstatus 处理
// ===========================================================================

/// 构造一个"返回 U-mode"所需的 `sstatus` 值。
///
/// `sret` 的行为由 sstatus 决定: `SPP` (bit 8) 决定返回哪个特权级,
/// `SPIE` (bit 5) 决定返回后 SIE 的值。硬件在陷入时**自动**设
/// `SPIE = SIE, SIE = 0, SPP = 陷入前特权级` —— 那反映"进入时"而非
/// "目标"状态: 一旦跑过"从内核态陷入"的路径, SPP 停在 1, `sret`
/// 会"返回"到 S-mode 却执行用户地址, 静默卡死。所以这里**显式**设置,
/// 不依赖硬件, 并把 SUM/FS 等也恢复到位。
#[inline]
pub fn build_user_return_status(saved: usize) -> usize {
    // 从保存的快照出发, 清掉所有与"返回目标"有关的位, 再显式设置。
    let mut s = saved;
    s &= !csr::SSTATUS_SPP; // 清 SPP
    s &= !csr::SSTATUS_SPIE; // 清 SPIE
    s &= !csr::SSTATUS_SIE; // 清 SIE (返回瞬间是关着的, 由 SPIE 决定)
    s |= csr::SSTATUS_SPIE; // SPIE = 1 -> sret 之后开中断
                            // SPP 保持 0 -> 返回 U-mode
                            // SUM 保持内核态的值 (内核需要能访问用户页面的数据)。
    s |= csr::SSTATUS_SUM;
    s
}

/// 构造一个"返回 S-mode"所需的 `sstatus` 值 (内核 trap 处理完后返回)。
///
/// 与 [`build_user_return_status`] 的区别只有 SPP: 这里是 1。若内核 trap
/// 由用户态 trap 嵌套引起, 硬件会把 SPP 清成 0, 直接 `sret` 会回到 U-mode
/// 却执行内核地址 —— 显式置 SPP=1 消除这个歧义。
#[inline]
pub fn build_kernel_return_status(saved: usize) -> usize {
    let mut s = saved;
    s &= !csr::SSTATUS_SPP;
    s |= csr::SSTATUS_SPP; // SPP = 1 -> 返回 S-mode
    s &= !csr::SSTATUS_SPIE;
    s |= csr::SSTATUS_SPIE; // 返回后开中断
    s
}

/// 把 [`build_user_return_status`] / [`build_kernel_return_status`] 的结果写进 `sstatus`。
///
/// # Safety
/// 必须在 `sret` 前最后时刻调用, 中间不能有任何改 `sstatus` 的操作。
#[inline]
pub unsafe fn apply_return_status(status: usize) {
    // SAFETY: 由调用者保证传进来的是一个合法的目标状态。
    unsafe {
        csr::write_sstatus(status);
    }
}

// ===========================================================================
// 给上层用的语义化查询接口
// ===========================================================================
// 这些是 kernel 读取 trap 现场的唯一入口。不直接暴露 `csr::read_scause()`
// 这些 RISC-V 名字 —— facade 上的名字描述意图 (如"上次陷阱的原因"),
// 换架构时 kernel 一行不改。

/// 上一次 trap 的**原因** (已解码的枚举)。
pub fn last_fault_cause() -> TrapCause {
    TrapCause::decode(csr::read_scause())
}

/// 上一次 trap 是从**用户态**进来的吗?
///
/// 依据是陷入前的特权级 (`sstatus.SPP`, 刚进入 trap 时可靠); 系统调用
/// 则优先用 `scause` (`ecall from U` 必是用户态), 本函数用于缺页/非法指令
/// 这类"可能来自两者"的情况。
pub fn came_from_user_mode() -> bool {
    csr::read_sstatus() & csr::SSTATUS_SPP == 0
}

/// 上一次 trap 的**原因的编号** (原始值)。
///
/// 返回编号而非枚举, 因为未能识别的编号正是崩溃打印最需要的信息。
pub fn last_cause_raw() -> usize {
    csr::read_scause()
}

/// 上一次 trap 的**原因名** (人类可读, 已解码)。
pub fn last_cause_name() -> &'static str {
    TrapCause::decode(csr::read_scause()).name()
}

/// 上一次 trap 是否来自中断 (而不是异常)。
pub fn last_cause_was_interrupt() -> bool {
    TrapCause::decode(csr::read_scause()).is_interrupt()
}

/// 上一次 trap 的**出错指令地址**。
///
/// 不叫 `read_sepc`: kernel 关心"出错在哪一条指令", 而非哪个 CSR 有值。
pub fn last_fault_pc() -> usize {
    csr::read_sepc()
}

/// 上一次 trap 的**附加信息**: 缺页地址、非法指令编码, 或 0。
pub fn last_fault_address() -> usize {
    csr::read_stval()
}

/// 上一次 trap 的完整现场快照 (供崩溃打印)。
#[derive(Debug, Clone, Copy)]
pub struct FaultInfo {
    /// 原因编号 (原始值)。
    pub cause_raw: usize,
    /// 原因名。
    pub cause_name: &'static str,
    /// 是否是中断。
    pub is_interrupt: bool,
    /// 出错指令的地址。
    pub pc: usize,
    /// 附加信息 (缺页地址等)。
    pub address: usize,
}

/// 一次性取回上一次 trap 的全部信息。
///
/// 这些 CSR 只在 trap 处理期间有意义; 若分两次读, 中间来的新 trap 会让
/// `scause` 与 `sepc` 来自不同 trap, 拼出假现场。一次取全 (中断关闭时
/// 读取) 是"原子"的。
pub fn last_fault_info() -> FaultInfo {
    let cause_raw = csr::read_scause();
    let cause = TrapCause::decode(cause_raw);
    FaultInfo {
        cause_raw,
        cause_name: cause.name(),
        is_interrupt: cause.is_interrupt(),
        pc: csr::read_sepc(),
        address: csr::read_stval(),
    }
}

/// 取出一个 `scause` 的编号部分 (去掉最高位的中断标志)。
pub fn cause_code(raw: usize) -> usize {
    csr::scause_code(raw)
}

/// 原因编号里表示"这是中断"的最高位掩码。
///
/// 上层用它从原始值滤掉中断标志位, 无需知道位号或名字。
pub const INTERRUPT_BIT: usize = csr::SCAUSE_INTERRUPT;

/// 中断使能位的分类掩码, 供上层"使能某一类中断"时使用。
///
/// 返回不透明掩码, 上层传给 [`crate::arch::irq::enable_source`] 即可。
pub mod source {
    use crate::arch::csr;

    /// 定时器中断。
    pub const TIMER: usize = csr::SIE_STIE;
    /// 外部设备中断 (来自中断控制器)。
    pub const EXTERNAL: usize = csr::SIE_SEIE;
    /// 核间中断。
    pub const SOFTWARE: usize = csr::SIE_SSIE;
}

/// 把 trapframe 里的返回地址推进到下一条指令 (仅用于系统调用)。
///
/// `ecall` 陷入时硬件把返回地址指向 `ecall` **本身**; 不加 4 返回会
/// 无限重复这条指令 (症状: write 刷屏、exit 退不出)。只对系统调用做 ——
/// 对其他异常 (如缺页) 推进就是跳过那条指令, 语义错误。
#[inline]
pub fn advance_return_address_for_syscall(tf: &mut TrapFrame) {
    // 一条 RISC-V 指令宽 4 字节, `ecall` 只有 4 字节编码 (无压缩形式)。
    tf.sepc = tf.sepc.wrapping_add(4);
}
