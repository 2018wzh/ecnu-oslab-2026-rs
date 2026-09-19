//! 挂载文件系统: 读超级块、校验, 并把全局的 `Fs` 与缓冲区缓存安装好。

use super::adapter::BlockAdapter;
use super::bio::{BufCache, BLOCK_SIZE};
use super::inode::{file_type, DiskInode, DISK_INODE_SIZE, MAX_FILE_SIZE};
use oslab_drivers::block::BlockDevice as DrvBlockDevice;

/// 超级块魔数 (与 `docs/abi-spec.md` 和 `xtask/src/mkfs.rs` 一致)。
pub const FSMAGIC: u32 = 0x1020_3040;

/// 根目录的 inode 号 (1-based)。
pub const ROOTINO: u32 = 1;

/// 超级块所在块号。
pub const SUPERBLOCK_BLOCK: usize = 0;

/// 从磁盘读出的超级块。
#[derive(Debug, Clone, Copy)]
pub struct Superblock {
    /// 魔数。
    pub magic: u32,
    /// 磁盘总块数。
    pub size: u32,
    /// inode 总数。
    pub ninodes: u32,
    /// inode 区起始块号。
    pub inodestart: u32,
    /// 根目录下的文件数。
    pub nfiles: u32,
}

impl Superblock {
    /// 从小端字节解析。
    pub fn parse(buf: &[u8]) -> Self {
        Self {
            magic: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            size: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            ninodes: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            inodestart: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
            nfiles: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
        }
    }
}

/// 挂载失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountError {
    /// 读超级块失败。
    Io,
    /// 魔数不对。
    BadMagic,
    /// 超级块参数不自洽。
    BadSuperblock,
    /// 根目录不是目录。
    BadRoot,
}

// 缓冲区缓存必须放在静态区, 不能是 `mount()` 里的局部变量: 它体积是
// NBUF * BLOCK_SIZE = 15 KB, 而启动栈只有 4 KB, 放栈上会直接写穿栈顶,
// 造成与并发无关、难以诊断的挂死。语义上它也本就该是全局的永久缓存。
static mut BCACHE: BufCache = BufCache::new();

// 作自由函数而非 `Fs` 的方法: 缓存是全局的, 写成方法会同时借用 `self`
// 与 `self.dev`, 借用检查不允许 —— 这也说明缓存不属于某一个挂载实例。
//
// SAFETY: 启动阶段只有一个 hart 访问; 挂载完成后由文件系统独占。
fn cache() -> &'static mut BufCache {
    // SAFETY: 见上。
    unsafe { &mut *core::ptr::addr_of_mut!(BCACHE) }
}

// 挂载在启动阶段完成, 而系统调用 (open/read/exec) 之后要随时访问。把它
// 挂在一个静态变量里让文件系统在整个生命周期可用; 真实内核用 VFS 挂载树,
// 教学内核只有一个根文件系统, 一个 `Option<Fs>` 就够。
static mut FS: Option<Fs<'static>> = None;

/// 安装全局文件系统 (只在启动时调用一次)。
pub fn install(fs: Fs<'static>) {
    // SAFETY: 启动阶段只有一个 hart 调用它, 且只调用一次。
    unsafe {
        *core::ptr::addr_of_mut!(FS) = Some(fs);
    }
}

// 返回 `None` 表示还没挂载; 这条检查把"忘了挂载"从随机崩溃变成明确错误。
/// 取全局文件系统。
pub fn fs() -> Option<&'static mut Fs<'static>> {
    // SAFETY: FS 只在启动时被写一次, 之后只读。
    unsafe { (*core::ptr::addr_of_mut!(FS)).as_mut() }
}

/// 一个已挂载的文件系统。
pub struct Fs<'a> {
    /// 挂载时读出的超级块 (只读)。
    pub sb: Superblock,
    /// 块设备适配器。
    dev: BlockAdapter<'a>,
}

impl<'a> Fs<'a> {
    /// 挂载: 读超级块、校验、验证根目录可访问。
    pub fn mount(dev: &'a mut dyn DrvBlockDevice) -> Result<Self, MountError> {
        let mut adapter = BlockAdapter::new(dev);

        let cache = cache();
        let i = unsafe { cache.bread(&mut adapter, SUPERBLOCK_BLOCK) };
        let sb = Superblock::parse(cache.block_data(i));
        cache.release(i);

        if sb.magic != FSMAGIC {
            return Err(MountError::BadMagic);
        }
        if sb.ninodes == 0 || sb.inodestart == 0 {
            return Err(MountError::BadSuperblock);
        }
        let inode_blocks = (sb.ninodes as usize + BLOCK_SIZE / DISK_INODE_SIZE - 1)
            / (BLOCK_SIZE / DISK_INODE_SIZE);
        if sb.inodestart as usize + inode_blocks > sb.size as usize {
            return Err(MountError::BadSuperblock);
        }

        let mut fs = Self { sb, dev: adapter };
        if fs.read_inode(ROOTINO).dtype != file_type::DIRECTORY {
            return Err(MountError::BadRoot);
        }
        Ok(fs)
    }



    /// 读一个 inode (inode 号 1-based, 下标 0-based)。
    pub fn read_inode(&mut self, inum: u32) -> DiskInode { crate::fs::inode::DiskInode { dtype: 0, major: 0, minor: 0, nlink: 0, size: 0, addrs: [0; 13] } }

    /// 读一个文件的全部内容。
    pub fn read_file(&mut self, inode: &DiskInode, out: &mut [u8]) -> (usize, bool) { (0, false) }

    // 系统调用需要从当前读写位置读一段 (`File::offset`), 而 `read_file`
    // 是整文件读出 (exec 用); 两者共用同一套块映射, 这里只多一"块内偏移"。
    /// 从文件的 `off` 处开始读, 最多填入 `out`。
    pub fn read_file_at(
        &mut self,
        inode: &DiskInode,
        off: usize,
        out: &mut [u8],
    ) -> (usize, bool) { unimplemented!() }

    /// 文件内第 n 块 -> 磁盘块号。
    fn block_of(&mut self, inode: &DiskInode, n: usize) -> Option<u32> { unimplemented!() }

    /// 按绝对路径查找 inode 号。
    pub fn lookup(&mut self, path: &[u8]) -> Option<u32> { unimplemented!() }
}

// 名字定长 14 字节且不保证 NUL 结尾, 所以取到第一个 NUL 为止再比较。
/// 在目录内容里查找一个名字。
pub fn find_dirent(data: &[u8], name: &[u8]) -> Option<u32> { unimplemented!() }
