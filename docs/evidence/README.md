# 实测证据

这些文件是**真实命令的输出**, 不是手写的示例。
环境: rustc 1.97.1, QEMU 11.1.1, dtc 1.8.1 (见 `00-environment.txt`)。

| 文件 | 命令 | 证明什么 |
|---|---|---|
| `00-environment.txt` | `rustc/qemu/dtc --version` | 工具链版本 |
| `01-list.txt` | `cargo xtask list` | 两个配置都被正确解析 |
| `02-build-qemu-virt.txt` | `cargo xtask build --config riscv64-qemu-virt` | QEMU 配置构建成功, 链接地址自检通过 |
| `03-run-qemu-virt.txt` | `cargo xtask run --config riscv64-qemu-virt --timeout 8` | **内核真的在 QEMU/OpenSBI 上启动了** (含完整 OpenSBI 输出) |
| `03b-kernel-output.txt` | (上面那份的内核部分) | 启动横幅 + 多核启动的完整输出 |
| `04-build-visionfive2.txt` | `cargo xtask build --config riscv64-visionfive2` | VF2 配置构建成功, 链接地址 0x40200000 |
| `05-image-visionfive2.txt` | `cargo xtask image --config riscv64-visionfive2` | FIT 生成 + 自校验 + libfdt 交叉验证全部通过 |
| `06-dtc-parse.txt` | `dtc -I dtb -O dts .../kernel.itb` + `fdtget` | **dtc 能解析生成的 FIT**; libfdt 按数字读出的值正确 |
| `07-check-arch.txt` | `cargo xtask check-arch` | kernel/ 守住了架构边界 |
| `08-xtask-tests.txt` | `cargo test -p xtask` | 12 个单元测试通过 |

## 关于 `dtc` 输出里的 `load = "@ ", ""`

**这不是镜像的问题。** `0x40200000` 的大端字节是 `40 20 00 00`,
也就是 `'@'`, `' '`, NUL, NUL —— `dtc` 看到"末尾是 0 且其余可打印",
就**猜**它是一个字符串。

**FDT 的属性没有类型**, 类型的知识在读它的驱动手里 (U-Boot 按属性名
`load` / `data-size` 决定怎么读)。用 libfdt 的 `fdtget` 按数字读
(这才是 U-Boot 内部的做法):

```text
$ fdtget -t x target/riscv64-visionfive2/kernel.itb /images/kernel load
40200000
$ fdtget -t x target/riscv64-visionfive2/kernel.itb /images/kernel entry
40200000
```

`cargo xtask image` 会自动做这个 libfdt 交叉验证 (见
`05-image-visionfive2.txt`), 所以"load/entry 是不是真的对"这件事
不依赖人眼解读 `dtc` 的输出。

## 复现

```bash
cd /home/wzh/oslab/ecnu-oslab-2026-rs
cargo xtask build --config riscv64-qemu-virt
cargo xtask run   --config riscv64-qemu-virt --timeout 8
cargo xtask build --config riscv64-visionfive2
cargo xtask image --config riscv64-visionfive2
dtc -I dtb -O dts target/riscv64-visionfive2/kernel.itb
cargo xtask check-arch
cargo test -p xtask
```
