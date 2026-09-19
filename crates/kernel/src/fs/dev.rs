//! 设备文件: 把设备也做成文件 (一切皆文件), 让 `read`/`write` 对普通
//! 文件与设备一视同仁。设备 inode 没有数据块, 读写走设备驱动, 判据是
//! inode 的 `mode` 类型位 (见 [`file_type`] 里的 `DEVICE`)。
//!
//! 设备 inode 号用固定的小数字, 它们是约定而非磁盘上真实存在的 inode。
//! 因此这些号不能被分配出去 —— 位图初始化时必须把它们标成已占用,
//! 否则创建文件后控制台就会失效。

// 从 1 开始而不是 0: 0 在 fd 表里用作"这个槽是空的"的标记
// (见 `File::empty`), 否则"fd 指向控制台"与"fd 是空的"无法区分。
/// 控制台设备的 inode 号。
pub const CONSOLE_INUM: u32 = 1;

// 空设备让"重定向到空"成为测试基础设施 (丢掉不关心的输出), 也是
// "设备 read/write 语义可以与文件不同"的具体例子 —— 一个永远空读的"文件"。
/// 空设备的 inode 号 (`/dev/null`)。读它永远返回 0 字节, 写它永远成功但丢弃数据。
pub const NULL_INUM: u32 = 2;

/// 设备类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// 控制台 (串口)。
    Console,
    /// 空设备。
    Null,
}

/// 判断一个 inode 号是不是设备。
pub const fn device_kind(inum: u32) -> Option<DeviceKind> {
    match inum {
        CONSOLE_INUM => Some(DeviceKind::Console),
        NULL_INUM => Some(DeviceKind::Null),
        _ => None,
    }
}

// "保留几个 inode 号"必须在位图初始化 (跳过) 与 inode 分配 (不分配)
// 两处保持一致, 做成同一个常量即可避免"加了设备忘了改位图"。
/// 设备 inode 一共占用了多少个 (即位图初始化时要跳过的前缀)。
pub const NR_DEVICE_INODES: u32 = NULL_INUM;

// 控制台走 `putchar` 而非直接调 UART 驱动: `putchar` 已处理输出后端选择
// (早期 SBI, 之后真实 UART) 与多核互斥, 这一层不应重复实现。
/// 写设备。返回写出的字节数, `None` 表示不存在 —— 实际用 `usize` 不会为 None。
pub fn dev_write(kind: DeviceKind, buf: &[u8]) -> usize {
    match kind {
        DeviceKind::Console => {
            for &b in buf {
                oslab_hal::putchar::putc(b);
            }
            buf.len()
        }
        // 空设备: 数据被丢弃, 但"写成功"必须报告 —— 否则调用者会重试。
        DeviceKind::Null => buf.len(),
    }
}

// 控制台的输入路径在 lab-9 之后实现 (需要 UART 接收中断与输入缓冲区)。
// 返回 0 表示 EOF/未读到数据 —— 未实现的功能返回明确的失败而非静默成功。
/// 读设备。返回读到的字节数。
pub fn dev_read(kind: DeviceKind, _buf: &mut [u8]) -> usize {
    match kind {
        DeviceKind::Console => 0,
        DeviceKind::Null => 0,
    }
}