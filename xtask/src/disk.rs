use crate::{Result, config::Config, root};
use std::{fs::OpenOptions, io::{Write, Seek, SeekFrom}, path::PathBuf};
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
    seed(&mut file)?;
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

// 教师提供的初始内容；按固定偏移写 LE 字节，不序列化宿主结构体。
fn seed(file: &mut std::fs::File) -> Result<()> {
    fn block(file: &mut std::fs::File, number: u32, bytes: &[u8; BLOCK_SIZE]) -> Result<()> {
        file.seek(SeekFrom::Start(u64::from(number) * BLOCK_SIZE as u64))?;
        file.write_all(bytes)?; Ok(())
    }
    let mut bytes = [0u8; BLOCK_SIZE]; bytes[0] = 7;
    block(file, INODE_BITMAP_FIRST, &bytes)?;
    bytes[0] = 127; block(file, DATA_BITMAP_FIRST, &bytes)?;
    bytes.fill(0);
    let sizes = [256u32, 5200, 13000];
    let first = [DATA_FIRST, DATA_FIRST + 1, DATA_FIRST + 3];
    for i in 0..3 {
        let ip = &mut bytes[i * 64..i * 64 + 64];
        for (j, v) in [if i == 0 { INODE_DIR } else { INODE_DATA }, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT, 1].iter().enumerate() {
            ip[j * 2..j * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        ip[8..12].copy_from_slice(&sizes[i].to_le_bytes());
        for j in 0..sizes[i].div_ceil(BLOCK_SIZE as u32) as usize {
            ip[12 + j * 4..16 + j * 4].copy_from_slice(&(first[i] + j as u32).to_le_bytes());
        }
    }
    block(file, INODE_FIRST, &bytes)?; bytes.fill(0);
    for entry in bytes.chunks_exact_mut(64) { entry[60..64].copy_from_slice(&INVALID_INODE_NUM.to_le_bytes()); }
    for (i, (name, number)) in [(b".".as_slice(), ROOT_INODE), (b"..", ROOT_INODE), (b"ABCD.txt", 1), (b"abcd.txt", 2)].iter().enumerate() {
        bytes[i * 64..i * 64 + name.len()].copy_from_slice(name);
        bytes[i * 64 + 60..i * 64 + 64].copy_from_slice(&number.to_le_bytes());
    }
    block(file, DATA_FIRST, &bytes)?;
    for i in 1..3 {
        for offset in (0..sizes[i]).step_by(BLOCK_SIZE) {
            bytes.fill(0);
            for j in 0..(sizes[i] - offset).min(BLOCK_SIZE as u32) as usize {
                bytes[j] = (if i == 1 { b'A' } else { b'a' }) + ((offset + j as u32) % 26) as u8;
            }
            block(file, first[i] + offset / BLOCK_SIZE as u32, &bytes)?;
        }
    }
    Ok(())
}
