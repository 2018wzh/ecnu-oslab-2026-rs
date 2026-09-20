//! 教师提供 load_segment、含段遍历的 prepare_heap 和单页参数栈。
use crate::{fs::inode::{InodeGuard, ReadDst}, mem::{self, PageTable, kvm, pmem, uvm}};
pub struct Header { pub entry: usize, pub phoff: usize, pub phnum: usize }
fn le(p: &[u8]) -> usize { p.iter().enumerate().fold(0, |v, (i, b)| v | ((*b as usize) << (8 * i))) }
pub fn read_header(ip: &mut InodeGuard<'_>) -> Result<Header, ()> {
    let mut h = [0; 64];
    if ip.read_data(0, ReadDst::Kernel(&mut h)) != h.len() { return Err(()); }
    if le(&h[..4]) != 0x464c457f || h[4] != 2 || h[5] != 1
        || !oslab_hal::arch::exec::machine(le(&h[18..20]) as u16) { return Err(()); }
    Ok(Header { entry: le(&h[24..32]), phoff: le(&h[32..40]), phnum: le(&h[56..58]) })
}
/// # Safety
/// root 独占且未发布，目标用户页已分配并恒等映射可访问；文件区间适合 u32 偏移。
pub unsafe fn load_segment(ip: &mut InodeGuard<'_>, root: PageTable, offset: u32, va: usize, len: u32) {
    assert_eq!(va % 4096, 0);
    for done in (0..len as usize).step_by(4096) {
        // SAFETY: 调用者保证页表有效且目标页独占，读取内容只写此物理页。
        unsafe {
            let pte = *kvm::getpte(root, va + done, false).expect("load_segment mapping");
            assert!(pte & mem::V != 0);
            let n = (len as usize - done).min(4096);
            let dst = core::slice::from_raw_parts_mut(mem::pte_to_pa(pte) as *mut u8, n);
            assert_eq!(ip.read_data(offset + done as u32, ReadDst::Kernel(dst)), n, "load_segment read");
        }
    }
}
/// # Safety
/// root 独占、未发布；失败后调用者销毁新页表。inode 守卫保证 ELF 稳定。
pub unsafe fn prepare_heap(root: PageTable, ip: &mut InodeGuard<'_>, h: &Header) -> Result<usize, ()> {
    let mut top = crate::proc::USER_ENTRY;
    for i in 0..h.phnum {
        let mut ph = [0; 56];
        let off = h.phoff.checked_add(i.checked_mul(56).ok_or(())?).ok_or(())?;
        if off > u32::MAX as usize - 56 || ip.read_data(off as u32, ReadDst::Kernel(&mut ph)) != 56 { return Err(()); }
        if le(&ph[..4]) != 1 { continue; }
        let offset = le(&ph[8..16]); let va = le(&ph[16..24]);
        let filesz = le(&ph[32..40]); let memsz = le(&ph[40..48]);
        let end = va.checked_add(memsz).ok_or(())?;
        if memsz < filesz || va % 4096 != 0 || va < crate::proc::USER_ENTRY || end < top
            || end > uvm::MMAP_BEGIN || offset > u32::MAX as usize || filesz > u32::MAX as usize - offset { return Err(()); }
        let mut flags = mem::R;
        if le(&ph[4..8]) & 2 != 0 { flags |= mem::W; }
        if le(&ph[4..8]) & 1 != 0 { flags |= mem::X; }
        // SAFETY: 范围已校验，新地址空间独占；heap_grow 按页覆盖字节堆顶并清零。
        unsafe {
            top = uvm::heap_grow(root, top, end - top, flags);
            if top != end { return Err(()); }
            load_segment(ip, root, offset as u32, va, filesz as u32);
        }
    }
    Ok(top)
}
/// # Safety
/// root 独占、未发布且尚无栈页；失败由调用者销毁。参数切片不含 NUL。
pub unsafe fn prepare_stack(root: PageTable, argv: &[&[u8]]) -> Result<usize, ()> {
    if argv.len() > oslab_uapi::MAX_ARGS { return Err(()); }
    let bottom = crate::proc::USER_STACK_TOP - 4096;
    // SAFETY: 新页面独占，映射后所有权归 root；此临时切片不离开函数。
    let page = unsafe {
        let pa = pmem::alloc(false);
        core::ptr::write_bytes(pa as *mut u8, 0, 4096);
        kvm::mappages(root, bottom, pa, 4096, mem::R | mem::W | mem::U);
        &mut *(pa as *mut [u8; 4096])
    };
    let mut cursor = 4096usize;
    let mut pointers = [0usize; oslab_uapi::MAX_ARGS + 1];
    for (i, arg) in argv.iter().enumerate() {
        if arg.len() >= oslab_uapi::ARG_BYTES || arg.contains(&0) { return Err(()); }
        cursor = cursor.checked_sub((arg.len() + 1 + 15) & !15).ok_or(())?;
        page[cursor..cursor + arg.len()].copy_from_slice(arg);
        pointers[i] = bottom + cursor;
    }
    let bytes = (argv.len() + 1) * 8;
    cursor = cursor.checked_sub((bytes + 15) & !15).ok_or(())?;
    for (i, ptr) in pointers[..=argv.len()].iter().enumerate() {
        page[cursor + i * 8..cursor + i * 8 + 8].copy_from_slice(&ptr.to_le_bytes());
    }
    Ok(bottom + cursor)
}
