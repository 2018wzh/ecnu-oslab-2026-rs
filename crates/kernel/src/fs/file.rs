//! 文件描述符表: 进程与打开的文件之间的间接层。用户程序拿到一个小整数
//! fd, 内核用它查对应的 inode、读写位置与权限。间接是必须的: 读写位置
//! 与权限都是 per-fd 而非 per-inode, 用户也拿不到内核指针。
//!
//! fd 0/1/2 (stdin/stdout/stderr) 是约定而非内核特殊逻辑: 内核只保证新
//! 进程启动时这三个 fd 已被打开。内核自己的输出走 `oslab_hal::putchar`,
//! 不经过 fd 表, 因此不会因用户程序关掉自己的 fd 而消失。

/// 每个进程最多打开多少个文件。
pub const NOFILE: usize = 16;

/// 一个打开的文件。
#[derive(Debug, Clone, Copy)]
pub struct File {
    /// 它对应的 inode 号 (0 表示这个槽是空的)。
    pub inum: u32,
    /// 当前读写位置 (字节)。
    pub offset: usize,
    /// 是否可读。
    pub readable: bool,
    /// 是否可写。
    pub writable: bool,
    /// 是否被占用。
    pub used: bool,
}

impl File {
    /// 一个空闲的 fd 槽。
    pub const fn empty() -> Self {
        Self {
            inum: 0,
            offset: 0,
            readable: false,
            writable: false,
            used: false,
        }
    }
}

// fd 号是进程私有的命名空间 (进程 A 的 fd 3 与进程 B 的无关); 做成
// 全局的话, fork 后子进程关一个 fd 会连带关掉父进程的。
/// 一个进程的 fd 表。
#[derive(Debug, Clone, Copy)]
pub struct FdTable {
    /// fd 数组。下标就是 fd 号。
    pub files: [File; NOFILE],
}

impl FdTable {
    /// 一个空表 (所有 fd 都未打开)。
    pub const fn empty() -> Self {
        Self {
            files: [const { File::empty() }; NOFILE],
        }
    }

    // 从 0 开始找第一个空槽是 POSIX 约定且被程序依赖 (close(0);
    // open("f") 期望 fd==0); 否则重定向静默失效。
    /// 分配一个 fd, 指向 `inum`。
    pub fn alloc(&mut self, inum: u32, readable: bool, writable: bool) -> Option<usize> { unimplemented!() }

    /// 取一个 fd。
    pub fn get(&self, fd: usize) -> Option<&File> {
        if fd < NOFILE && self.files[fd].used {
            Some(&self.files[fd])
        } else {
            None
        }
    }

    /// 取一个 fd (可变)。
    pub fn get_mut(&mut self, fd: usize) -> Option<&mut File> {
        if fd < NOFILE && self.files[fd].used {
            Some(&mut self.files[fd])
        } else {
            None
        }
    }

    // 关闭一个已关闭的 fd 在 POSIX 是 EBADF 错误而非"什么也不做";
    // 静默成功会让 fd 表泄漏难以发现, 还会关掉别人的文件。
    /// 关闭一个 fd。
    pub fn close(&mut self, fd: usize) -> bool { unimplemented!() }

    // 新进程必须调用它: 漏掉则用户程序第一行输出就失败。三个 fd 都指向
    // 同一个"控制台" inode (本内核里控制台由 `fs::dev` 用约定号表示)。
    /// 把标准输入/输出/错误建立好。
    pub fn open_stdio(&mut self) { }

    /// 当前打开了多少个 fd (自检用)。
    pub fn count_open(&self) -> usize {
        (0..NOFILE).filter(|&i| self.files[i].used).count()
    }
}

impl Default for FdTable {
    fn default() -> Self {
        Self::empty()
    }
}