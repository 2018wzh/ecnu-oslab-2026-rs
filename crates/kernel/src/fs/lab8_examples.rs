//! 四组教师观察例程；在 fs::init 末尾的可睡眠进程上下文中选择一组。
use super::{inode::{self, ReadDst, WriteSrc}, dentry, bitmap, *};
use crate::mem::pmem;
fn data() -> inode::InodeRef { inode::create(INODE_DATA, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT) }
fn dir() -> inode::InodeRef { inode::create(INODE_DIR, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT) }
#[unsafe(no_mangle)]
pub extern "C" fn lab8_examples() {
    match option_env!("OSLAB_LAB8_TEST").unwrap_or("0") {
        "0" => (), "1" => test1(), "2" => test2(), "3" => test3(), "4" => test4(),
        _ => panic!("OSLAB_LAB8_TEST must be 0..4"),
    }
}
fn show_inode_bitmap() {
    // SAFETY: fs::init 已发布超级块，之后不再修改；这里只借用不可变元数据。
    let sb = unsafe { (&*(&raw const super::SUPERBLOCK)).as_ref().expect("fs init") };
    bitmap::print(sb, false);
}
fn test1() {
    crate::println!("============= test begin =============");
    let root = inode::get(ROOT_INODE);
    root.lock().print("root");
    show_inode_bitmap();
    let ip1 = dir(); let ip2 = data();
    let mut g1 = ip1.lock(); let mut g2 = ip2.lock();
    let duplicate = ip2.dup();
    g1.print("dir"); g2.print("data");
    show_inode_bitmap();
    g1.info_mut().nlink = 0; g2.info_mut().nlink = 0;
    drop(g1); drop(g2); drop(ip1); drop(ip2);
    show_inode_bitmap();
    drop(duplicate);
    show_inode_bitmap();
    crate::println!("============= test end =============");
    // 和观察序列一致，root 引用保持到测试停住。
    core::mem::forget(root);
    loop { core::hint::spin_loop(); }
}
fn test2() {
    crate::println!("============= test begin =============");
    let mut small_src = [0u8; 40]; let mut small_dst = [0u8; 40];
    for i in 0..10u32 { small_src[i as usize * 4..i as usize * 4 + 4].copy_from_slice(&i.to_le_bytes()); }
    let ip1 = data(); let mut g1 = ip1.lock(); g1.print("small_data");
    crate::println!("writing data...");
    for offset in (0..400 * 40).step_by(40) {
        assert_eq!(g1.write_data(offset, WriteSrc::Kernel(&small_src)), 40, "write fail 1");
    }
    g1.print("small_data");
    assert_eq!(g1.read_data(120 * 40 + 4, ReadDst::Kernel(&mut small_dst)), 40, "read fail 1");
    crate::print!("read data:");
    for b in small_dst.chunks_exact(4) { crate::print!(" {}", u32::from_le_bytes(b.try_into().unwrap())); }
    crate::println!();
    g1.info_mut().nlink = 0; drop(g1); drop(ip1);
    // 分别申请五页，逐页验证连续；不能对用户地址作此处理。
    let big = pmem::alloc(true);
    for i in 1..5 { assert_eq!(pmem::alloc(true), big + i * 4096, "contiguous fail"); }
    {
        // SAFETY: 五个内核页均独占且刚刚验证连续，切片在 free 前结束使用。
        let big_src = unsafe { core::slice::from_raw_parts_mut(big as *mut u8, 5 * 4096) };
        for (i, b) in big_src.iter_mut().enumerate() { *b = b'A' + (i % 8) as u8; }
        let ip2 = data(); let mut g2 = ip2.lock(); g2.print("big_data");
        crate::println!("writing data...");
        let cut_len = 4096 * 4 + 1110; // 17494
        for offset in (0..cut_len * 10000).step_by(cut_len as usize) {
            assert_eq!(g2.write_data(offset, WriteSrc::Kernel(&big_src[..cut_len as usize])), cut_len as usize, "write fail 2");
        }
        g2.print("big_data");
        let mut big_dst = [0u8; 9];
        // 尾部读取必须是仍持锁的大文件 ip2。
        assert_eq!(g2.read_data(cut_len * 10000 - 8, ReadDst::Kernel(&mut big_dst[..8])), 8, "read fail 2");
        crate::println!("read data: {}", core::str::from_utf8(&big_dst[..8]).unwrap());
        g2.info_mut().nlink = 0; drop(g2); drop(ip2);
    }
    for i in 0..5 {
        // SAFETY: 对应本测试独占申请的五页，数据引用均已结束。
        unsafe { pmem::free(big + i * 4096, true); }
    }
    crate::println!("============= test end =============");
    loop { core::hint::spin_loop(); }
}
fn test3() {
    crate::println!("============= test begin =============");
    let root = inode::get(ROOT_INODE);
    let mut gr = root.lock();
    let n1 = dentry::search(&mut gr, b"ABCD.txt").expect("invalid inode num");
    let n2 = dentry::search(&mut gr, b"abcd.txt").expect("invalid inode num");
    let n3 = dentry::search(&mut gr, b".").expect("invalid inode num");
    dentry::print(&gr).unwrap(); drop(gr);
    let ip1 = inode::get(n1); let mut g1 = ip1.lock();
    let ip2 = inode::get(n2); let mut g2 = ip2.lock();
    let ip3 = inode::get(n3); let g3 = ip3.lock();
    g1.print("ABCD.txt"); g2.print("abcd.txt"); g3.print("root");
    let mut tmp = [0u8; 10];
    assert_eq!(g1.read_data(0, ReadDst::Kernel(&mut tmp[..9])), 9, "read fail 1");
    crate::println!("read data: {}", core::str::from_utf8(&tmp[..9]).unwrap());
    assert_eq!(g2.read_data(0, ReadDst::Kernel(&mut tmp[..9])), 9, "read fail 2");
    crate::println!("read data: {}", core::str::from_utf8(&tmp[..9]).unwrap());
    drop(g1); drop(g2); drop(g3); drop(ip1); drop(ip2); drop(ip3);
    let mut gr = root.lock(); let new_dir = dir();
    let offset = dentry::create(&mut gr, new_dir.number(), b"new_dir").unwrap();
    let number = dentry::search(&mut gr, b"new_dir").unwrap();
    crate::println!("new dentry offset = {}\nnew dentry inode_num = {}", offset, number);
    dentry::print(&gr).unwrap();
    assert_eq!(number, dentry::delete(&mut gr, b"new_dir").unwrap(), "inode num is not equal");
    dentry::print(&gr).unwrap(); drop(gr); drop(root);
    // 来源仅删除目录项，保留新建 inode 的引用和链接状态供观察。
    core::mem::forget(new_dir);
    crate::println!("============= test end =============");
    loop { core::hint::spin_loop(); }
}
fn test4() {
    crate::println!("============= test begin =============");
    let root = inode::get(ROOT_INODE); let ip1 = dir(); let ip2 = dir(); let ip3 = data();
    let mut gr = root.lock(); let mut g1 = ip1.lock(); let mut g2 = ip2.lock(); let mut g3 = ip3.lock();
    dentry::create(&mut gr, ip1.number(), b"AABBC").expect("dentry_create fail 1");
    dentry::create(&mut g1, ip2.number(), b"aaabb").expect("dentry_create fail 2");
    dentry::create(&mut g2, ip3.number(), b"file.txt").expect("dentry_create fail 3");
    let tmp1 = b"This is file context!\0";
    g3.write_data(0, WriteSrc::Kernel(tmp1));
    gr.rw(true); g1.rw(true); g2.rw(true);
    drop(gr); drop(g1); drop(g2); drop(g3); drop(root); drop(ip1); drop(ip2); drop(ip3);
    let path = b"///AABBC///aaabb/file.txt";
    let ip4 = dentry::lookup(path).ok().expect("invalid ip4");
    let (ip5, name) = dentry::parent(path).ok().expect("invalid ip5");
    let end = name.iter().position(|b| *b == 0).unwrap_or(NAME_BYTES);
    crate::println!("get a name = {}", core::str::from_utf8(&name[..end]).unwrap());
    let mut g4 = ip4.lock(); let g5 = ip5.lock();
    g4.print("file.txt"); g5.print("aaabb");
    let mut tmp2 = [0u8; 32]; g4.read_data(0, ReadDst::Kernel(&mut tmp2));
    let end = tmp2.iter().position(|b| *b == 0).unwrap_or(32);
    crate::println!("read data: {}", core::str::from_utf8(&tmp2[..end]).unwrap());
    drop(g4); drop(g5); drop(ip4); drop(ip5);
    crate::println!("============= test end =============");
}
