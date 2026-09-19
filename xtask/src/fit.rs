//! `xtask::fit` — U-Boot FIT 镜像生成器 (纯 Rust, 不依赖 mkimage)。
//! FIT 本质是带 FDT 头的容器, 自己生成便于调试; 三个格式细节:
//! 内存保留块不能省、所有块 4 字节对齐、`data-offset`/`data-position`
//! 都是相对文件起点的偏移。

use std::path::Path;

// ---------------------------------------------------------------------------
// FDT 常量 (来自 devicetree 规范)
// ---------------------------------------------------------------------------

/// FDT 头部的魔数。大端序的 `0xd00dfeed`。
const FDT_MAGIC: u32 = 0xd00d_feed;
/// 结构块标记: 开始一个节点。
const FDT_BEGIN_NODE: u32 = 0x1;
/// 结构块标记: 结束一个节点。
const FDT_END_NODE: u32 = 0x2;
/// 结构块标记: 一个属性。
const FDT_PROP: u32 = 0x3;
/// 结构块标记: 结构块结束。
const FDT_END: u32 = 0x9;
/// FDT 格式版本。17 是当前版本 (支持 `#address-cells` 等)。
const FDT_VERSION: u32 = 17;
/// 向后兼容的最低版本。
const FDT_LAST_COMP_VERSION: u32 = 16;

// ===========================================================================
// 结构块与字符串块的构造器
// ===========================================================================

/// 结构块与字符串块的构造器: 两者成对维护 (写属性时同步加字符串名)。
struct FdtBuilder {
    /// 结构块。
    structure: Vec<u8>,
    /// 字符串块。
    strings: Vec<u8>,
}

impl FdtBuilder {
    fn new() -> Self {
        Self {
            structure: Vec::new(),
            strings: Vec::new(),
        }
    }

    /// 把结构块对齐到 4 字节 (规范要求每项从 4 字节边界开始)。
    fn align_structure(&mut self) {
        while self.structure.len() % 4 != 0 {
            self.structure.push(0);
        }
    }

    /// 开始一个节点。
    fn begin_node(&mut self, name: &str) {
        self.align_structure();
        self.structure
            .extend_from_slice(&FDT_BEGIN_NODE.to_be_bytes());
        // 节点名以 NUL 结尾。
        self.structure.extend_from_slice(name.as_bytes());
        self.structure.push(0);
        // 名字之后要补齐到 4 字节 (由下一次 align_structure 完成,
        // 但属性数据紧随其后, 所以这里就补 —— 否则属性头会错位)。
        self.align_structure();
    }

    /// 结束一个节点。
    fn end_node(&mut self) {
        self.align_structure();
        self.structure
            .extend_from_slice(&FDT_END_NODE.to_be_bytes());
    }

    /// 把一个属性名加进字符串块, 返回它的偏移。
    fn add_string(&mut self, name: &str) -> u32 {
        let offset = self.strings.len() as u32;
        self.strings.extend_from_slice(name.as_bytes());
        self.strings.push(0);
        offset
    }

    /// 写一个原始属性。
    fn prop_raw(&mut self, name: &str, value: &[u8]) {
        self.align_structure();
        let name_off = self.add_string(name);
        self.structure.extend_from_slice(&FDT_PROP.to_be_bytes());
        self.structure
            .extend_from_slice(&(value.len() as u32).to_be_bytes());
        self.structure.extend_from_slice(&name_off.to_be_bytes());
        self.structure.extend_from_slice(value);
        // 属性数据的长度也要补齐到 4 字节。
        self.align_structure();
    }

    /// 写一个字符串属性 (含结尾的 NUL —— 这是 FDT 的约定)。
    fn prop_str(&mut self, name: &str, value: &str) {
        let mut v = value.as_bytes().to_vec();
        v.push(0);
        self.prop_raw(name, &v);
    }

    /// 写一个 32 位整数属性。
    fn prop_u32(&mut self, name: &str, value: u32) {
        self.prop_raw(name, &value.to_be_bytes());
    }

    /// 写一个 64 位整数属性 (当前未使用; 预留大地址用)。
    #[allow(dead_code)]
    fn prop_u64(&mut self, name: &str, value: u64) {
        self.prop_raw(name, &value.to_be_bytes());
    }

    /// 结束结构块 (写 FDT_END), 返回 (结构块, 字符串块)。
    fn finish(mut self) -> (Vec<u8>, Vec<u8>) {
        self.align_structure();
        self.structure.extend_from_slice(&FDT_END.to_be_bytes());
        (self.structure, self.strings)
    }
}

// ===========================================================================
// 生成
// ===========================================================================

/// 生成一个 FIT 镜像。
///
/// 参数:
///   * `kernel_bin` — 内核裸二进制的路径 (由 `objcopy -O binary` 生成);
///   * `out` — 输出的 `.itb` 路径;
///   * `load_addr` / `entry_addr` — 写进 FIT 的 load/entry 属性;
///   * `description` — 人类可读的描述。
pub fn generate(
    kernel_bin: &Path,
    out: &Path,
    load_addr: u64,
    entry_addr: u64,
    arch: &str,
    description: &str,
) -> Result<(), String> {
    let data = std::fs::read(kernel_bin)
        .map_err(|e| format!("无法读取内核二进制 {}: {e}", kernel_bin.display()))?;
    if data.is_empty() {
        return Err(format!(
            "内核二进制 {} 是空的 —— objcopy 可能失败了。",
            kernel_bin.display()
        ));
    }

    // 地址必须能放进 32 位 —— FIT 的 load/entry 属性是 u32。
    // 如果放不下, 明确报错而不是截断 (截断的后果是 U-Boot 把内核
    // 加载到一个完全不同的地址, 然后跳过去执行垃圾)。
    if load_addr > u32::MAX as u64 || entry_addr > u32::MAX as u64 {
        return Err(format!(
            "load/entry 地址超过 32 位 (load={load_addr:#x}, entry={entry_addr:#x})。\n\
             FIT 的 load/entry 属性是 u32, 无法表达大于 4 GiB 的地址。"
        ));
    }

    // ---- 构造结构块与字符串块 ----
    let mut b = FdtBuilder::new();

    // 根节点。
    b.begin_node("");
    b.prop_str("description", description);
    // `#address-cells = 1` 表示下面所有地址/大小属性都是 32 位。
    // 这与 load/entry 是 u32 一致 —— 两者不匹配会让 U-Boot 用错误的
    // 宽度解读地址。
    b.prop_u32("#address-cells", 1);
    b.prop_u32("#size-cells", 0);

    // /images
    b.begin_node("images");
    // /images/kernel
    b.begin_node("kernel");
    b.prop_str("description", "OSLab kernel (raw binary)");
    b.prop_u32("data-size", data.len() as u32);
    // data-offset / data-position 依赖最终块大小, 先写占位 0 并记住
    // 位置, 稍后回填。位置 = 当前结构块长度 + 属性头 12 字节。
    b.align_structure();
    let offset_prop_pos = b.structure.len() + 12;
    b.prop_u32("data-offset", 0);
    b.align_structure();
    let position_prop_pos = b.structure.len() + 12;
    b.prop_u32("data-position", 0);

    b.prop_str("type", "kernel");
    // arch 必须是 "riscv" —— U-Boot 的 bootm 会校验它, 不匹配就
    // 拒绝启动。这正是 FIT 相对"裸二进制 + go"的价值: 它能拦住
    // "把 AArch64 的内核塞给 RISC-V U-Boot" 这种错误。
    b.prop_str("arch", arch);
    // os = "linux" 表示按 Linux 启动约定跳转 (a0=hartid, a1=dtb),
    // 即 S-mode 启动 ABI —— 正是内核期望的。
    b.prop_str("os", "linux");
    b.prop_str("compression", "none");
    b.prop_u32("load", load_addr as u32);
    b.prop_u32("entry", entry_addr as u32);
    b.end_node(); // kernel
    b.end_node(); // images

    // /configurations
    b.begin_node("configurations");
    b.prop_str("default", "conf-1");
    b.begin_node("conf-1");
    b.prop_str("description", description);
    b.prop_str("kernel", "kernel");
    b.end_node(); // conf-1
    b.end_node(); // configurations

    b.end_node(); // 根节点

    let (mut structure, strings) = b.finish();

    // ---- 计算各块的偏移 ----
    //
    // 布局: [头 40][保留块 16][结构块][字符串块][填充][数据]
    const HEADER_SIZE: usize = 40;
    const RSVMAP_SIZE: usize = 16;
    let off_struct = HEADER_SIZE + RSVMAP_SIZE;
    let off_strings = off_struct + structure.len();
    // 字符串块之后要填充到 4 字节 —— 因为数据也要 4 字节对齐。
    let off_data = align_up(off_strings + strings.len(), 4);
    let total = off_data + align_up(data.len(), 4);

    // ---- 回填 data-offset / data-position ----
    //
    // 两者取值相同 (都是相对文件起点的数据偏移), 用大端序写。
    let off_data_u32 = off_data as u32;
    structure[offset_prop_pos..offset_prop_pos + 4].copy_from_slice(&off_data_u32.to_be_bytes());
    structure[position_prop_pos..position_prop_pos + 4]
        .copy_from_slice(&off_data_u32.to_be_bytes());

    // ---- 组装镜像 ----
    let mut img = vec![0u8; total];

    // 头部 (40 字节, 全大端)。
    write_be32(&mut img, 0, FDT_MAGIC);
    write_be32(&mut img, 4, total as u32); // totalsize
    write_be32(&mut img, 8, off_struct as u32); // off_dt_struct
    write_be32(&mut img, 12, off_strings as u32); // off_dt_strings
    write_be32(&mut img, 16, HEADER_SIZE as u32); // off_mem_rsvmap
    write_be32(&mut img, 20, FDT_VERSION); // version
    write_be32(&mut img, 24, FDT_LAST_COMP_VERSION); // last_comp_version
    write_be32(&mut img, 28, 0); // boot_cpuid_phys
    write_be32(&mut img, 32, strings.len() as u32); // size_dt_strings
    write_be32(&mut img, 36, structure.len() as u32); // size_dt_struct

    // 内存保留块: 两个 8 字节全零对, 表示"无保留区域"。
    // 规范要求必须有 16 个零字节, 不能省。显式写出以表明这是格式要求。
    for i in 0..RSVMAP_SIZE {
        img[HEADER_SIZE + i] = 0;
    }

    // 结构块、字符串块、数据。
    img[off_struct..off_struct + structure.len()].copy_from_slice(&structure);
    img[off_strings..off_strings + strings.len()].copy_from_slice(&strings);
    img[off_data..off_data + data.len()].copy_from_slice(&data);

    // ---- 写文件 ----
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建 {}: {e}", parent.display()))?;
    }
    std::fs::write(out, &img).map_err(|e| format!("无法写入 {}: {e}", out.display()))?;

    // ---- 报告 ----
    println!("==> FIT 镜像");
    println!(
        "    内核数据 : {} 字节 ({} 扇区)",
        data.len(),
        data.len().div_ceil(512)
    );
    println!("    结构块   : {} 字节", structure.len());
    println!("    字符串块 : {} 字节", strings.len());
    println!("    数据偏移 : {:#x}", off_data);
    println!("    镜像总长 : {} 字节", total);
    println!("    load     : {:#x}", load_addr);
    println!("    entry    : {:#x}", entry_addr);

    // 结构自检。见 verify 的说明: 只查"这个文件的结构是否自洽",
    // 不试图把 FDT 完整解析一遍 —— 镜像能不能用最终由启动一次决定。
    verify(&img)?;

    Ok(())
}

/// 把一个 32 位大端值写到缓冲区的指定偏移。
fn write_be32(buf: &mut [u8], offset: usize, v: u32) {
    buf[offset..offset + 4].copy_from_slice(&v.to_be_bytes());
}

/// 向上对齐到 `align` 的倍数。
fn align_up(v: usize, align: usize) -> usize {
    (v + align - 1) & !(align - 1)
}

// ===========================================================================
// 自校验: 一个最小的 FDT 遍历器
// ===========================================================================

/// 生成后的**结构性**自检。
///
/// 偏移算错不会立刻报错 (文件有正确 magic/长度), 要等 U-Boot 解析才失败,
/// 所以生成后自查一遍结构。只查结构 (magic/totalsize/越界/节点配对),
/// 不逐属性完整解析 —— 镜像能不能用最终由真的启动一次决定。
fn verify(img: &[u8]) -> Result<(), String> {
    if img.len() < 40 {
        return Err("生成的镜像小于 FDT 头部大小".into());
    }
    let be32 = |off: usize| -> u32 {
        u32::from_be_bytes([img[off], img[off + 1], img[off + 2], img[off + 3]])
    };

    if be32(0) != FDT_MAGIC {
        return Err(format!("magic 不对: {:#x}", be32(0)));
    }
    if be32(4) as usize != img.len() {
        return Err(format!(
            "totalsize 字段 ({}) 与实际文件大小 ({}) 不一致",
            be32(4),
            img.len()
        ));
    }
    let off_struct = be32(8) as usize;
    let off_strings = be32(12) as usize;
    let size_strings = be32(32) as usize;
    let size_struct = be32(36) as usize;
    if off_struct + size_struct > img.len() || off_strings + size_strings > img.len() {
        return Err("结构块或字符串块超出了文件范围".into());
    }

    // 遍历结构块, 校验节点配对与属性长度不越界。
    let st = &img[off_struct..off_struct + size_struct];
    let mut pos = 0usize;
    let mut depth = 0i32;
    while pos + 4 <= st.len() {
        let tag = u32::from_be_bytes([st[pos], st[pos + 1], st[pos + 2], st[pos + 3]]);
        pos += 4;
        match tag {
            FDT_BEGIN_NODE => {
                depth += 1;
                // 节点名以 NUL 结尾, 再补齐到 4 字节。
                while pos < st.len() && st[pos] != 0 {
                    pos += 1;
                }
                pos = (pos + 4) & !3;
            }
            FDT_END_NODE => {
                depth -= 1;
                if depth < 0 {
                    return Err("结构块里出现了多余的 END_NODE".into());
                }
            }
            FDT_PROP => {
                if pos + 8 > st.len() {
                    return Err("属性头越过了结构块末尾".into());
                }
                let len =
                    u32::from_be_bytes([st[pos], st[pos + 1], st[pos + 2], st[pos + 3]]) as usize;
                pos = (pos + 8 + len + 3) & !3;
            }
            FDT_END => {
                return if depth == 0 {
                    Ok(())
                } else {
                    Err(format!("结构块结束时还有 {depth} 层节点没闭合"))
                };
            }
            _ => return Err(format!("结构块里出现未知的 tag {tag:#x}")),
        }
        if pos > st.len() {
            return Err("属性长度把游标推出了结构块".into());
        }
    }
    Err("结构块里没有找到 FDT_END".into())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// 生成一个 FIT 到临时目录, 然后用 `dtc` 解析它 (如果装了 dtc)。
    ///
    /// 这个测试是"生成的镜像真的能被 FDT 解析器读懂"的最直接证据。
    #[test]
    fn generates_parseable_fit() {
        let dir = std::env::temp_dir().join("oslab-fit-test");
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("kernel.bin");
        // 造一段假的"内核", 大小刻意不是 4 的倍数 —— 用来验证
        // 数据段的对齐填充是对的。
        std::fs::write(&bin, vec![0xaau8; 1003]).unwrap();
        let itb = dir.join("kernel.itb");
        generate(&bin, &itb, 0x4020_0000, 0x4020_0000, "riscv", "test image").unwrap();

        let bytes = std::fs::read(&itb).unwrap();
        verify(&bytes).unwrap();

        // 如果环境里有 dtc, 再交叉验证一次。
        if which_dtc() {
            let out = std::process::Command::new("dtc")
                .args(["-I", "dtb", "-O", "dts"])
                .arg(&itb)
                .output()
                .expect("dtc 应该能运行");
            assert!(
                out.status.success(),
                "dtc 拒绝了解析生成的 FIT:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let dts = String::from_utf8_lossy(&out.stdout);
            assert!(dts.contains("riscv"), "dts 里应该有 arch = riscv:\n{dts}");
            assert!(dts.contains("conf-1"), "dts 里应该有 conf-1:\n{dts}");
        }
    }

    fn which_dtc() -> bool {
        std::process::Command::new("which")
            .arg("dtc")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn aligns_data_to_four_bytes() {
        let dir = std::env::temp_dir().join("oslab-fit-align");
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("k.bin");
        std::fs::write(&bin, vec![0u8; 5]).unwrap();
        let itb = dir.join("k.itb");
        generate(&bin, &itb, 0x1000, 0x1000, "riscv", "t").unwrap();
        let bytes = std::fs::read(&itb).unwrap();
        // totalsize 必须是 4 的倍数 (每个块都对齐了)。
        assert_eq!(bytes.len() % 4, 0);
    }
}
