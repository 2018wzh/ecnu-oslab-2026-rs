// major: 1 stdin、2 stdout、3 stderr、4 zero、5 null、6 gpt0；minor=INODE_MINOR_DEFAULT。
// 教师提供设备行为，学生实现设备表、权限检查及分派。
type Read = fn(&mut [u8]) -> usize;
type Write = fn(&[u8]) -> usize;
struct Device { name: [u8; super::NAME_BYTES], read: Option<Read>, write: Option<Write> }
impl Device { const EMPTY: Self = Self { name: [0; super::NAME_BYTES], read: None, write: None }; }
static mut DEVICE_TABLE: [Device; 7] = [const { Device::EMPTY }; 7];
/// 注册设备（教师辅助），不替学生选择设备、权限或创建 /dev 节点。
/// # Safety
/// 仅在初始化阶段独占设备表、尚无并发读者时调用。
unsafe fn register(major: u16, name: &[u8], read: Option<Read>, write: Option<Write>) -> Result<(), ()> {
    if major == 0 || major >= 7 || name.is_empty() || name.len() >= super::NAME_BYTES
        || name.contains(&0) || (read.is_none() && write.is_none()) { return Err(()); }
    let mut entry = Device { name: [0; super::NAME_BYTES], read, write };
    entry.name[..name.len()].copy_from_slice(name);
    // SAFETY: 编号已经验证，调用者独占初始化；不形成整个 static mut 的引用。
    unsafe { (&raw mut DEVICE_TABLE).cast::<Device>().add(usize::from(major)).write(entry); }
    Ok(())
}
pub fn stdin(dst: &mut [u8]) -> usize { crate::console_input::read(dst) }
pub fn stdout(src: &[u8]) -> usize { for b in src { crate::console::putc(*b); } src.len() }
pub fn stderr(src: &[u8]) -> usize { stdout(b"ERROR: "); stdout(src) }
pub fn zero(dst: &mut [u8]) -> usize { dst.fill(0); dst.len() }
pub fn null_read(_dst: &mut [u8]) -> usize { 0 }
pub fn null_write(src: &[u8]) -> usize { src.len() }
pub fn gpt(src: &[u8]) -> usize {
    let mut question = src;
    while matches!(question.last(), Some(b'\n' | b'\r')) { question = &question[..question.len() - 1]; }
    match question {
        b"Hello" => crate::println!("Hi, I am gpt0!"),
        b"Guess who I am" => {
            use oslab_hal::arch::cpu;
            cpu::push_off();
            let p = crate::proc::current();
            // SAFETY: 关闭中断期间 current 不会迁移；仅复制当前进程的字段，不保留借用。
            let (pid, name) = if p.is_null() { (0, [0u8; 16]) } else { unsafe { ((*p).pid, (*p).name) } };
            cpu::pop_off();
            let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
            let name = core::str::from_utf8(&name[..end]).unwrap_or("<invalid UTF-8>");
            crate::println!("Your procid is {} and name is {}.", pid, name);
        }
        b"How many free memory left" => {
            let kernel = crate::mem::pmem::stat(true);
            let user = crate::mem::pmem::stat(false);
            crate::println!("We have {} free pages in kernel space, {} free pages in user space!", kernel, user);
        }
        b"Good job" => crate::println!("Thanks for your kind words!"),
        _ => crate::println!("Sorry, I can not understand it."),
    }
    src.len()
}
// TODO(lab-9): 用 register 初始化设备表；建立 /dev 和六个设备 inode，重复启动幂等。
pub fn init() -> Result<(), ()> { todo!("lab-9: device::init") }
// TODO(lab-9): stdin/zero 只读，stdout/stderr/gpt0 只写，null 可读写。
pub fn open_check(_major: u16, _mode: usize) -> bool { todo!("lab-9: device::open_check") }
// TODO(lab-9): stdin 行缓冲读取，zero 填零，null EOF，其他 Err。
pub fn read(_major: u16, _dst: &mut [u8]) -> Result<usize, ()> { todo!("lab-9: device::read") }
// TODO(lab-9): stdout 输出，stderr 加 ERROR 前缀，null 丢弃，gpt0 固定问答。
pub fn write(_major: u16, _src: &[u8]) -> Result<usize, ()> { todo!("lab-9: device::write") }
