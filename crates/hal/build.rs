// hal 的 build.rs: 把"恰好选中一个平台/架构"变成编译期保证。
// 若只用 #[cfg(feature)] 表达, 一个都不选会报 "cannot find value
// UART0_BASE"、选两个会报 "duplicate definitions", 都指向错误的地方;
// 在 build.rs 提前检查, 把错误信息变成人能看懂的话 (build.rs 是 cargo
// 保证在任何编译前执行的用户代码)。

fn main() {
    // 让 cargo 在 feature 变化时重新跑本脚本。
    println!("cargo::rerun-if-changed=build.rs");

    // ---- 架构: 必须恰好选一个 ----
    // 架构用 feature 而非 target_arch 表达, "忘选"与"选两个"都能给
    // 人话报错, 而不是让学生去看 "cannot find module arch"。
    let arch_riscv64 = cfg!(feature = "arch-riscv64");
    let arch_count = [arch_riscv64].iter().filter(|x| **x).count();
    if arch_count == 0 {
        panic!(
            "\n\
             ============================================================\n\
             错误: 没有选中任何架构。\n\
             \n\
             架构决定 CPU 语义 (控制寄存器、trap 入口、页表格式),\n\
             这些知识来自 arch 层。没有选架构, hal 里就没有这些东西。\n\
             \n\
             正确用法 (xtask 会替你加上正确的 --features):\n\
             \n\
                 cargo xtask build --config riscv64-qemu-virt\n\
                 cargo xtask build --config riscv64-visionfive2\n\
             \n\
             如果你确实要用裸 cargo, 需要显式指定两个维度:\n\
             \n\
                 cargo build --target riscv64gc-unknown-none-elf \\\n\
                     -p oslab-kernel --features arch-riscv64,platform-qemu-virt\n\
             ============================================================\n"
        );
    }

    let qemu = cfg!(feature = "platform-qemu-virt");
    let vf2 = cfg!(feature = "platform-visionfive2");

    match (qemu, vf2) {
        (true, true) => panic!(
            "\n\
             ============================================================\n\
             错误: 同时选中了两个平台 feature。\n\
             \n\
             platform-qemu-virt 与 platform-visionfive2 是互斥的。\n\
             同一份内核只能构建给一台机器。\n\
             \n\
             如果你是用 cargo 手工构建的, 请只保留一个 --features。\n\
             正常用法是通过 xtask:\n\
             \n\
                 cargo xtask build --config riscv64-qemu-virt\n\
                 cargo xtask build --config riscv64-visionfive2\n\
             ============================================================\n"
        ),
        (false, false) => panic!(
            "\n\
             ============================================================\n\
             错误: 没有选中任何平台。\n\
             \n\
             内核需要知道 UART/PLIC 的地址和 CPU 拓扑, 这些知识来自\n\
             platform 层。没有选平台, hal 里就没有任何地址常量。\n\
             \n\
             正确用法 (xtask 会替你加上正确的 --features):\n\
             \n\
                 cargo xtask build --config riscv64-qemu-virt\n\
                 cargo xtask build --config riscv64-visionfive2\n\
             \n\
             如果你确实要用裸 cargo, 需要显式指定:\n\
             \n\
                 cargo build --target riscv64gc-unknown-none-elf \\\n\
                     -p oslab-kernel --features platform-qemu-virt\n\
             ============================================================\n"
        ),
        _ => {}
    }

    // 把选中的平台名编译进二进制 (供启动横幅与自检用)。platform/*.rs 里
    // 还有一份"代码认定的平台", 两者必须一致, kernel 启动时比较它们。
    let name = if qemu { "qemu-virt" } else { "visionfive2" };
    println!("cargo::rustc-env=OSLAB_PLATFORM_NAME={name}");

    // 架构名同样编译进去。
    let arch = if arch_riscv64 { "riscv64" } else { "unknown" };
    println!("cargo::rustc-env=OSLAB_ARCH_NAME={arch}");
}