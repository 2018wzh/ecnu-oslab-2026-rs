use crate::{Result, config::Config, root};
use std::{fs::OpenOptions, io::Write, path::PathBuf};
use oslab_uapi::disk::*;
pub fn path(c: &Config) -> PathBuf { root().join("target").join(&c.name).join("disk.img") }
pub fn create(c: &Config, force: bool) -> Result<()> {
    let path = path(c);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut options = OpenOptions::new();
    options.write(true);
    if force { options.create(true).truncate(true); } else { options.create_new(true); }
    let mut file = options.open(&path)?;
    let fields = [FS_MAGIC, BLOCK_SIZE as u32, TOTAL_BLOCKS, N_INODE, INODE_BITMAP_FIRST, INODE_BITMAP_BLOCKS,
        INODE_FIRST, INODE_BLOCKS, DATA_BITMAP_FIRST, DATA_BITMAP_BLOCKS, DATA_FIRST, N_DATA_BLOCK];
    let mut sb = [0u8; BLOCK_SIZE];
    for (i, value) in fields.iter().enumerate() { sb[4 * i..4 * i + 4].copy_from_slice(&value.to_le_bytes()); }
    file.write_all(&sb)?;
    file.set_len(u64::from(TOTAL_BLOCKS) * BLOCK_SIZE as u64)?;
    file.sync_all()?;
    println!("Disk: {}", path.display()); Ok(())
}
pub fn ensure(c: &Config) -> Result<()> {
    match std::fs::metadata(path(c)) {
        Ok(meta) if meta.is_file() => Ok(()),
        Ok(_) => Err("disk.img 不是普通文件".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err("请先显式运行 cargo xtask disk 创建磁盘".into()),
        Err(e) => Err(e.into()),
    }
}
