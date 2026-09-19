//! `mkfs` — 生成磁盘镜像 (宿主工具)。格式与 `docs/abi-spec.md` 一致:
//! 块 0 超级块、块 1 空闲位图、块 2.. 为 inode 区, 之后是数据区;
//! 块大小 512 字节、小端序。

use std::fs;
use std::path::Path;

// 块大小 (字节), 与 `docs/abi-spec.md` 一致。
pub const BSIZE: usize = 512;
// 超级块魔数。
pub const FSMAGIC: u32 = 0x1020_3040;
// 直接块指针数量。
pub const NDIRECT: usize = 12;
// 一级间接块能存放的指针数量。
pub const NINDIRECT: usize = BSIZE / 4;
// 文件名最大长度 (定长, 不保证以 NUL 结尾)。
pub const MAXNAME: usize = 14;
// inode 总数。
pub const NINODES: usize = 200;
// 根目录的 inode 号 (inode 号是 1-based)。
pub const ROOTINO: u32 = 1;
// 镜像总块数。
pub const FSSIZE: usize = 2000;

// 磁盘上的 inode, 紧凑 64 字节。
// 必须 `repr(C, packed)`: Rust 默认会插对齐填充, 让结构体大于 64 字节,
// 与磁盘格式对不上会读到错位数据。
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct Dinode {
    pub type_: u16,
    pub major: u16,
    pub minor: u16,
    pub nlink: u16,
    pub size: u32,
    pub addrs: [u32; NDIRECT + 1],
}

// 目录项, 16 字节 (2 字节 inode 号 + 14 字节定长名字)。
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Dirent {
    pub inum: u16,
    pub name: [u8; MAXNAME],
}

// 编译期断言: 让"磁盘格式"成为编译器的强制约束。
const _: () = {
    assert!(std::mem::size_of::<Dinode>() == 64, "Dinode 必须是 64 字节");
    assert!(std::mem::size_of::<Dirent>() == 16, "Dirent 必须是 16 字节");
    assert!(NDIRECT * 4 + 4 + 8 + 4 == 64, "inode 字段偏移之和必须是 64");
};

// inode 类型。
pub const T_DIR: u16 = 1;
pub const T_FILE: u16 = 2;

// 磁盘布局常量。
pub const BITMAP_BLOCK: u32 = 1;
pub const INODE_START_BLOCK: u32 = 2;

// inode 区占多少个块。
pub const fn inode_blocks() -> u32 {
    let per_block = BSIZE / 64;
    ((NINODES + per_block - 1) / per_block) as u32
}

// 数据区从第几块开始。
pub const fn data_start_block() -> u32 {
    INODE_START_BLOCK + inode_blocks()
}

// 生成磁盘镜像。`files` 是 `(镜像内文件名, 宿主路径)` 列表。
// 名字显式传入, 因为镜像内名字限 14 字节而宿主路径可能很长。
pub fn build_image(out: &Path, files: &[(String, std::path::PathBuf)]) -> Result<(), String> {
    let mut img = vec![0u8; FSSIZE * BSIZE];

    // ---- 位图分配 ----
    // 用 bool 数组在内存中跟踪占用块, 最后统一写进位图, 简单直接。
    let mut used = vec![false; FSSIZE];

    // 超级块、位图、inode 区都标记为已用。
    for b in 0..data_start_block() as usize {
        used[b] = true;
    }

    // ---- 创建根目录 ----
    // 必须为根目录预分配一个数据块, 否则 size 为 0, 遍历时一条目录项都读不到。
    let mut root = Dinode {
        type_: T_DIR,
        nlink: 1,
        size: BSIZE as u32,
        ..Default::default()
    };
    let root_blk = alloc_block(&mut used)?;
    root.addrs[0] = root_blk;
    // 把 inode 写进镜像。
    write_inode(&mut img, ROOTINO, &root);

    // ---- 把每个文件写入镜像 ----
    let mut next_inum = 2u32; // 1 是根目录
    let mut dir_off = 0usize;

    for (name, path) in files {
        if name.len() > MAXNAME {
            return Err(format!("文件名 {name:?} 超过 {MAXNAME} 字节"));
        }
        if next_inum as usize > NINODES {
            return Err("inode 用尽".into());
        }

        let data = fs::read(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;

        // ---- 分配数据块并拷入内容 ----
        // addrs[0..12] 直接块 (最多 6KB), addrs[12] 一级间接块 (再 128 块 / 64KB)。
        // 内核侧已实现两层, mkfs 必须生成同样的结构, 否则大文件读到空洞。
        let nblocks = (data.len() + BSIZE - 1) / BSIZE;
        let max_blocks = NDIRECT + NINDIRECT;
        if nblocks > max_blocks {
            return Err(format!(
                "{name} 需要 {nblocks} 个块, 超过上限 {max_blocks} 个块 (70 KB)"
            ));
        }

        let mut f = Dinode {
            type_: T_FILE,
            nlink: 1,
            size: data.len() as u32,
            ..Default::default()
        };
        for k in 0..nblocks {
            let b = alloc_block(&mut used)?;
            let off = k * BSIZE;
            let n = std::cmp::min(BSIZE, data.len() - off);
            img[b as usize * BSIZE..b as usize * BSIZE + n]
                .copy_from_slice(&data[off..off + n]);

            if k < NDIRECT {
                // 直接块。
                f.addrs[k] = b;
            } else {
                // 一级间接: 第一个溢出块是"块号数组"。它本身也要占一个磁盘块,
                // 若忘掉分配, 间接位置上是别的数据, 文件前 6KB 正常后面全是垃圾。
                if f.addrs[NDIRECT] == 0 {
                    let ind = alloc_block(&mut used)?;
                    f.addrs[NDIRECT] = ind;
                }
                let ind = f.addrs[NDIRECT] as usize;
                let idx = k - NDIRECT;
                let slot = ind * BSIZE + idx * 4;
                img[slot..slot + 4].copy_from_slice(&b.to_le_bytes());
            }
        }
        write_inode(&mut img, next_inum, &f);

        // ---- 在根目录里加一条目录项 ----
        let mut de = Dirent {
            inum: next_inum as u16,
            name: [0u8; MAXNAME],
        };
        de.name[..name.len()].copy_from_slice(name.as_bytes());
        let base = root_blk as usize * BSIZE + dir_off;
        // SAFETY: img 足够大, 且 base..base+16 落在 root_blk 这一块内
        // (dir_off 从 0 开始, 每项 16 字节, 最多 BSIZE/16 项)。
        unsafe {
            std::ptr::write_unaligned(img[base..].as_mut_ptr() as *mut Dirent, de);
        }
        dir_off += std::mem::size_of::<Dirent>();

        next_inum += 1;
    }

    // ---- 写位图 ----
    for b in 0..FSSIZE {
        if used[b] {
            img[BITMAP_BLOCK as usize * BSIZE + b / 8] |= 1 << (b % 8);
        }
    }

    // ---- 写超级块 (逐字段小端) ----
    put32(&mut img, 0, FSMAGIC);
    put32(&mut img, 4, FSSIZE as u32);
    put32(&mut img, 8, NINODES as u32);
    put32(&mut img, 12, INODE_START_BLOCK);
    put32(&mut img, 16, 0); // 保留 (日志区大小)
    put32(&mut img, 20, files.len() as u32);

    fs::write(out, &img).map_err(|e| format!("写出 {} 失败: {e}", out.display()))?;
    Ok(())
}

// 分配一个空闲数据块。
fn alloc_block(used: &mut [bool]) -> Result<u32, String> {
    for b in data_start_block() as usize..FSSIZE {
        if !used[b] {
            used[b] = true;
            return Ok(b as u32);
        }
    }
    Err("磁盘空间耗尽".into())
}

// 把 inode 写进镜像 (inode 号 1-based, 数组下标 0-based)。
fn write_inode(img: &mut [u8], inum: u32, d: &Dinode) {
    // 必须减 1: inode 号 1-based (0 表示目录项空闲), 下标 0-based; 漏掉会让根目录错位。
    let idx = (inum - 1) as usize;
    let off = INODE_START_BLOCK as usize * BSIZE + idx * 64;
    // SAFETY: off + 64 落在 inode 区内 (inum <= NINODES)。
    unsafe {
        std::ptr::write_unaligned(img[off..].as_mut_ptr() as *mut Dinode, *d);
    }
}

// 小端写入 u32。
fn put32(img: &mut [u8], off: usize, v: u32) {
    img[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
