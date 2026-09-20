//! 教师观察辅助，不添加测试组。
use core::fmt::{self, Write};
use crate::syscall;
pub fn check(ok: bool) { if !ok { syscall::write(2, b"test fail\n"); syscall::exit(1); } }
struct Fd(usize);
impl Write for Fd {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if syscall::write(self.0, s.as_bytes()) == s.len() { Ok(()) } else { Err(fmt::Error) }
    }
}
pub fn print(fd: usize, args: fmt::Arguments<'_>) { let _ = Fd(fd).write_fmt(args); }
pub fn stat(s: &oslab_uapi::FileStat, name: &str) {
    let kind = match s.kind { 0 => "data", 1 => "dir", 2 => "device", _ => "unknown" };
    print(1, format_args!("file {}: type={} inum={} nlink={} size={} offset={}\n", name, kind, s.inode_num, s.nlink, s.size, s.offset));
}
pub fn dentries(bytes: &[u8], name: &str) {
    print(1, format_args!("directory {}:\n", name));
    for (i, e) in bytes.chunks_exact(64).enumerate() {
        let n = u32::from_le_bytes(e[60..64].try_into().unwrap());
        let end = e[..60].iter().position(|b| *b == 0).unwrap_or(60);
        print(1, format_args!("dentry {}: inum={} name=", i, n));
        syscall::write(1, &e[..end]); syscall::write(1, b"\n");
    }
}
