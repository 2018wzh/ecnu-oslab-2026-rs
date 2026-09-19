//! 内核同步原语。两种锁适用场景互斥:
//!   SpinLock  持锁期间绝不睡眠 (临界区几条指令), 拿不到就自旋;
//!   SleepLock 持锁期间可睡眠 (磁盘 I/O 等毫秒级), 拿不到就让出 CPU。
//! 自旋锁不能保护"可能睡眠"的临界区: 持锁进程睡去后, 别的 hart 拿
//! 锁会永远拿不到 (单核上必然死锁)。

use core::sync::atomic::{AtomicBool, Ordering};

// ===========================================================================
// 自旋锁
// ===========================================================================

/// 一个极简自旋锁。
///
/// 保护的几乎都是全局 `static` 状态, 所以不把数据包装进锁里; 用注释
/// 说明"锁保护什么"比用 `Mutex<T>` 的类型把握更贴合教学。
/// `intena` 记录拿锁前中断是否开启, 解锁时据此恢复。
pub struct SpinLock {
    locked: AtomicBool,
    /// 拿锁时是否关闭了中断 (用于解锁时恢复)。
    ///
    /// 记在锁里而非返回给调用者: 调用者"可以忘记"恢复, 而 unlock
    /// 一定会恢复, 嵌套加锁也不会弄乱中断状态。
    intena: AtomicBool,
}

impl SpinLock {
    /// 新建一把未上锁的自旋锁。
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            intena: AtomicBool::new(false),
        }
    }

    /// 获取锁, 拿不到就自旋。
    ///
    /// 先关中断再抢锁, 不能反过来: 抢到锁之后关中断之前若来了中断,
    /// 而中断处理又拿同一把锁, 就会自旋等一个由自己持有、永不释放
    /// 的锁 (自己和自己死锁)。
    pub fn lock(&self) {
        // 第 1 步: 关中断 (保存原状态)。
        let was_enabled = oslab_hal::arch::irq::is_enabled();
        oslab_hal::arch::irq::disable();
        self.intena.store(was_enabled, Ordering::Relaxed);

        // 第 2 步: 抢锁。
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // 自旋时给 CPU 一个"我在等锁"的 pause 提示, 减少争用。
            core::hint::spin_loop();
        }
    }

    /// 释放锁并恢复中断状态。
    ///
    /// # Safety
    /// 必须由持有该锁的 hart 调用且只能一次; 重复释放会让两个 hart
    /// 同时进临界区。
    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
        // 只有当初关了中断才恢复 —— 无条件开中断会把中断提前放进
        // 一个还没准备好的上下文。
        if self.intena.load(Ordering::Relaxed) {
            oslab_hal::arch::irq::enable();
        }
    }

    /// 尝试获取锁, 不阻塞。
    pub fn try_lock(&self) -> bool {
        let was_enabled = oslab_hal::arch::irq::is_enabled();
        oslab_hal::arch::irq::disable();
        if self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            self.intena.store(was_enabled, Ordering::Relaxed);
            true
        } else {
            // 没拿到则恢复中断状态, 避免调用者莫名运行在关中断下。
            if was_enabled {
                oslab_hal::arch::irq::enable();
            }
            false
        }
    }

    /// 锁是否被持有 (仅用于断言与调试)。
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }
}

impl Default for SpinLock {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 睡眠锁
// ===========================================================================

/// 一把"持锁期间可以睡眠"的锁。
///
/// 自旋锁等待时占着 CPU, 睡眠锁等待时让出 CPU —— 适合"等几毫秒的
/// 磁盘 I/O"(与几十纳秒的 CPU 周期差六个数理级)。三条纪律: 不能在
/// 中断上下文获取 (中断不能睡眠)、不能嵌套、持有期间不做长计算。
///
/// 实现是一个锁位 + 一个"睡在 lk 地址上"的睡眠队列: 用
/// `proc::sleep/wakeup` 一对原语, 不需要自维护队列; 唤醒是广播的
/// (惊群), 代价是所有等待者都醒来看一眼再睡回, 换一个无需额外
/// 数据结构的正确实现。
pub struct SleepLock {
    locked: SpinLock,
    /// 是否有人持有。
    held: AtomicBool,
}

impl SleepLock {
    /// 新建一把未上锁的睡眠锁。
    pub const fn new() -> Self {
        Self {
            locked: SpinLock::new(),
            held: AtomicBool::new(false),
        }
    }

    /// 获取锁, 已被持有则睡眠等待。
    ///
    /// 唤醒是广播的, 被叫醒不保证锁已空 (可能别人先拿到), 所以
    /// 必须 `loop` 重新检查而非 `if`。
    pub fn lock(&self) {
        loop {
            // 用自旋锁保护"检查 + 占位", 对别的 hart 表现为原子操作。
            self.locked.lock();
            if !self.held.load(Ordering::Acquire) {
                self.held.store(true, Ordering::Release);
                self.locked.unlock();
                return;
            }
            self.locked.unlock();

            // 锁被占着 -> 睡在这个 SleepLock 的地址上。
            //
            // SAFETY: 上面已释放自旋锁, 且传入的正是保护睡眠条件的
            // 那把锁 (sleep 的契约)。醒来后 loop 会重新检查 held。
            unsafe {
                crate::proc::sleep(self as *const _ as usize, &self.locked);
            }
        }
    }

    /// 释放锁, 并唤醒所有等待者。
    pub fn unlock(&self) {
        self.locked.lock();
        self.held.store(false, Ordering::Release);
        self.locked.unlock();

        // SAFETY: 持锁进行唤醒, 与 sleep 的契约匹配。
        unsafe {
            crate::proc::wakeup(self as *const _ as usize);
        }
    }
}

impl Default for SleepLock {
    fn default() -> Self {
        Self::new()
    }
}