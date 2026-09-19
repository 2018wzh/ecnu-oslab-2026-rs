//! 目录与路径解析。目录是一个内容被约定解释的普通文件, 它的数据是
//! 一串"名字 -> inode 号"的目录项; 本内核只支持绝对路径。
//!
//! 每个目录的前两个目录项固定是 `.` (指向自己) 与 `..` (指向父目录),
//! 很多工具靠它们存在来确认这是一个目录。名字用定长 (14 字节) 目录项,
//! 于是"找到第 i 项"是简单的乘法; 创建过长的名字要明确拒绝, 截断会让
//! 两个不同的名字变成同一个。

// 定长目录项的好处是"第 i 项"可直接算出: 偏移 = i * 16 (4 字节 inum
// + 2 字节 namelen + 14 字节 name), 也必须是方便的倍数, 否则跨块计算难看。
/// 目录项里文件名的最大长度 (字节)。
pub const MAX_NAME: usize = 14;

/// 一个磁盘上的目录项。
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DirEntry {
    /// 指向的 inode 号 (0 表示这一项是空的)。
    pub inum: u32,
    /// 名字长度。
    pub namelen: u16,
    /// 名字 (不以 0 结尾 —— 用 namelen 定长)。
    pub name: [u8; MAX_NAME],
}

/// 一个目录项占多少字节。
pub const DIRENT_SIZE: usize = 4 + 2 + MAX_NAME;

/// 一个块(扇区)里能放多少个目录项。
pub const DIRENTS_PER_BLOCK: usize = 512 / DIRENT_SIZE;

// ---------------------------------------------------------------------------
// 自检: 目录项大小与"每块多少项"必须是整数关系
// ---------------------------------------------------------------------------
// 若 DIRENT_SIZE 不能整除块大小, 就出现跨块的目录项, 其读取代码要分两段
// 拼, 忘记拼则表现为名字乱码。断言整除, 于是这类代码永远不需要存在。
const _: () = {
    assert!(DIRENT_SIZE == 20);
    // 一个块里能放 25 个目录项 (25 * 20 = 500, 尾部 12 字节不用)。
    assert!(DIRENTS_PER_BLOCK == 25);
    // 目录项必须落在块内 (DIRENTS_PER_BLOCK 按向下取整算)。
    assert!(DIRENTS_PER_BLOCK * DIRENT_SIZE <= 512);
    // 尾部 12 字节的浪费是显式接受的代价。
    assert!(512 - DIRENTS_PER_BLOCK * DIRENT_SIZE == 12);
};

impl DirEntry {
    /// 一个空目录项。
    pub const fn empty() -> Self {
        Self {
            inum: 0,
            namelen: 0,
            name: [0; MAX_NAME],
        }
    }

    /// 这一项是否为空 (可复用)。
    pub const fn is_empty(&self) -> bool {
        self.inum == 0
    }

    // 按字节比较而不是转字符串: 名字来自磁盘, 不一定是合法 UTF-8,
    // 转字符串会把"比较失败"与"名字不匹配"混为一谈。
    /// 名字是否等于 `target`。
    pub fn name_eq(&self, target: &[u8]) -> bool {
        if target.len() != self.namelen as usize {
            return false;
        }
        // 逐字节比较, 长度已经相等。
        let mut i = 0;
        while i < target.len() {
            if self.name[i] != target[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 名字的字节切片。
    pub fn name_bytes(&self) -> &[u8] {
        let n = core::cmp::min(self.namelen as usize, MAX_NAME);
        &self.name[..n]
    }

    // 截断会让两个长名字变成一个, 从而静默覆盖已存在的文件; 明确失败
    // 至少让调用者知道发生了什么。
    /// 写名字。返回是否成功 (太长则失败)。
    pub fn set_name(&mut self, name: &[u8]) -> bool {
        if name.len() > MAX_NAME {
            return false;
        }
        self.namelen = name.len() as u16;
        self.name = [0; MAX_NAME];
        let mut i = 0;
        while i < name.len() {
            self.name[i] = name[i];
            i += 1;
        }
        true
    }
}

// ===========================================================================
// 路径解析
// ===========================================================================

/// 路径解析的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// 路径不以 '/' 开头 (本内核只支持绝对路径)。
    NotAbsolute,
    /// 路径中间出现了空的分量 (例如 "a//b")。
    EmptyComponent,
    /// 某个分量在目录里找不到。
    NotFound,
    /// 路径太长。
    TooLong,
}

// 解析是递归的 (或需要一个与路径等长的栈), 不设上限会让精心构造的
// 超长路径耗光内核栈; 内核里所有递归都应有深度上限, 因为用户输入
// 必须被视为敌意。
/// 路径里最多有多少个分量。
pub const MAX_PATH_DEPTH: usize = 16;

// 切分与解析分开: 切分是纯字符串操作可独立测试, 解析要访问磁盘没法
// 单测。`out` 是调用者提供的缓冲区, 返回实际分量数。
/// 把绝对路径切成"分量"序列。
pub fn split_path<'a>(
    path: &'a [u8],
    out: &mut [&'a [u8]; MAX_PATH_DEPTH],
) -> Result<usize, PathError> {
    if path.is_empty() || path[0] != b'/' {
        return Err(PathError::NotAbsolute);
    }

    let mut n = 0;
    let mut start = 1; // 跳过开头的 '/'
    let mut i = 1;

    while i <= path.len() {
        // 遇到 '/' 或者到了末尾 -> 一个分量结束。
        if i == path.len() || path[i] == b'/' {
            let comp = &path[start..i];
            if !comp.is_empty() {
                if n >= MAX_PATH_DEPTH {
                    return Err(PathError::TooLong);
                }
                out[n] = comp;
                n += 1;
            } else if i != path.len() {
                // 中间出现了空分量 ("//")。明确报错而不是像 Unix 那样
                // 忽略, 让"学生以为写了什么"与"内核实际理解了什么"不偏差。
                return Err(PathError::EmptyComponent);
            }
            start = i + 1;
        }
        i += 1;
    }

    Ok(n)
}

// 用闭包而非直接访问文件系统, 让路径解析与"目录存在哪里、怎么读"
// 解耦: 可在无磁盘时测试切分/遍历逻辑, 也避免 `inode` 与 `dir` 循环依赖。
/// 路径解析过程中每一步的"进入下一级"操作由调用者提供。
pub fn resolve<'a, F>(path: &[u8], mut lookup: F) -> Result<u32, PathError>
where
    F: FnMut(u32, &[u8]) -> Option<u32>,
{ unimplemented!() }
