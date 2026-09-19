# VisionFive2 部署步骤

> 这份文档与 `configs/riscv64-visionfive2.toml` 的 `[uboot] deploy_hint`
> 是同一份内容。`cargo xtask run --config riscv64-visionfive2`
> 会打印它。

## 0. 为什么 xtask 不自动烧卡

`cargo xtask run --config riscv64-visionfive2` **不会**尝试写 SD 卡。
它只构建、生成 FIT 镜像、然后打印下面的步骤。

理由: 烧写 SD 卡需要 `sudo` 和一个设备名, 而**设备名写错会清掉
你的硬盘** (`/dev/sda` 与 `/dev/sdb` 只差一个字母)。这类不可逆的
危险操作必须由人来确认。构建工具应该明确承认自己能力的边界。

## 1. 生成镜像

```bash
cargo xtask image --config riscv64-visionfive2
```

产物:

```text
  target/riscv64-visionfive2/kernel.elf   调试用 (带 DWARF)
  target/riscv64-visionfive2/kernel.bin   裸二进制 (objcopy -O binary)
  target/riscv64-visionfive2/kernel.itb   ★ U-Boot FIT 镜像, 要拷这个
  target/riscv64-visionfive2/kernel.asm   反汇编 (排查启动问题用)
  target/riscv64-visionfive2/kernel.sym   符号表
```

`cargo xtask image` 会自己验证生成的 FIT:

* 自校验 FDT 结构 (魔数、各块偏移、节点配对、属性名偏移);
* 检查 `data-offset + data-size` 落在文件范围内;
* 用 `fdtget` (libfdt —— U-Boot 内部用的就是它) 读出 8 个关键
  属性并与期望值对比;
* 检查 `type=kernel`、`arch=riscv`、`os=linux`、`compression=none`
  —— 这四个属性任意一个不对, U-Boot 的 `bootm` 都会**拒绝**这个
  镜像。

## 2. 准备 SD 卡

1. 把 SD 卡的**第一个分区**格式化为 FAT32。
2. 把 `kernel.itb` 拷贝到该分区的根目录:

```bash
cp target/riscv64-visionfive2/kernel.itb /media/$USER/<你的分区>/
```

## 3. 连接串口

* 波特率 **115200**, 8 数据位, 无校验, 1 停止位 (8N1)。
* Linux: `minicom -D /dev/ttyUSB0 -b 115200` 或
  `picocom -b 115200 /dev/ttyUSB0`
* Windows: Terra Term / PuTTY。

> **注意**: 这个波特率与 VisionFive2 的 U-Boot 默认一致。如果
> 输出乱码, 先怀疑波特率 —— 而不是内核。内核这一侧的波特率分频
> 是用 `platform/visionfive2.rs` 的 `uart0_clock` (24 MHz) 算的,
> 那个值抄错同样会导致乱码 (见 `docs/porting.md` 的陷阱表)。

## 4. 在 U-Boot 里加载

上电, 在看到 `StarFive #` 提示符时按任意键进入 U-Boot, 然后:

```text
mmc dev 1
fatload mmc 1:1 ${kernel_addr_r} kernel.itb
bootm ${kernel_addr_r}
```

### 为什么用 `bootm` 而不是裸 `go`

| | `go` | `bootm` |
|---|---|---|
| 校验架构 | 否 | 是 (读 FIT 的 `arch` 属性) |
| 校验镜像类型 | 否 | 是 (读 `type` 属性) |
| 加载地址 | 由人手工保证 | 由 FIT 的 `load` 属性声明 |
| 入口地址 | 由人手工保证 | 由 FIT 的 `entry` 属性声明 |
| 地址写错时 | **静默跑飞** | 明确报错 |

一句话: `go` 是无条件跳转, `bootm` 会先读懂镜像。FIT 存在的
全部意义就是让"U-Boot 认识我的内核"这件事有据可依。

### 环境变量确认

如果 `${kernel_addr_r}` 在你的 U-Boot 里没有定义 (某些版本会),
直接用一个地址:

```text
fatload mmc 1:1 0x48000000 kernel.itb
bootm 0x48000000
```

**注意这个地址与内核自己的加载地址 (0x40200000) 是两回事**:
`0x48000000` 是"把 .itb 文件读进内存的哪里", 而 FIT 里的
`load = 0x40200000` 才是 `bootm` 把**内核数据**搬到哪里。
两者不能重合 —— 重合的话 `bootm` 在搬运时会覆盖它自己正在读的
数据。`0x48000000` 与 `0x40200000` 相距 128 MiB, 是安全的。

## 5. 期望的输出

```text
[oslab-rs] kernel entry reached (SBI console)

========================================================
  ECNU OSLab 2026  (Rust)
========================================================
  config      : riscv64-visionfive2
  platform    : visionfive2
  arch/boot   : riscv64 / uboot
--------------------------------------------------------
  DRAM base   : 0x40000000  size 128 MiB
  kernel base : 0x40200000  (config says 0x40200000)
  firmware    : 0x40000000 .. 0x40200000
  cpus        : 4  hart range [1..4]  boot hart 1
  uart0       : 0x10000000  irq 32  clock 24000000 Hz
  plic        : 0xc000000
  block       : dw-mshc (sd) @ 0x16000000
  timer       : interval 400000 ticks, source sbi only
--------------------------------------------------------
  this hart   : hartid 1  cpuid 0  (boot hart)
  sbi version : ...
========================================================

[oslab-rs] uart16550 ready @ 0x10000000 divisor=...
[oslab-rs] hart 2 (cpu 1) online, sp=0x...
[oslab-rs] hart 3 (cpu 2) online, sp=0x...
[oslab-rs] hart 4 (cpu 3) online, sp=0x...
[oslab-rs] smp: requested 3 secondary hart(s), 3 accepted by firmware, 4 now online

[oslab-rs] boot complete. entering idle loop.
```

### 关键的三处核对

1. **`hart range [1..4]` 与 `this hart : hartid 1`** —— 这确认了
   平台常量正确地把 hart 0 (S7 监控核, 被 U-Boot 占用) 排除在外。
2. **`4 now online` 与三行 "hart N online"** —— 这确认了多核启动
   真的走到了从核的 Rust 代码里 (不只是"固件接受了请求")。
3. **`divisor` 非零且输出可读** —— 这确认了 24 MHz 的 UART 时钟
   算出的分频是对的。

## 6. 排查

| 现象 | 最可能的原因 |
|---|---|
| `bootm` 直接报 "FIT description error" | `.itb` 拷贝时被截断 (重新 `cp` 并比对 md5) |
| `bootm` 报 "Cannot find a valid FIT" | 拷的是 `kernel.bin` 而不是 `kernel.itb` |
| `bootm` 报架构不匹配 | FIT 的 `arch` 属性被改坏了; 用 `cargo xtask image` 重新生成 |
| 内核一行输出都没有 | 串口波特率不对; 或者 `kernel_load_addr` 与 U-Boot 实际加载地址不符 |
| 只起来 3 个核 | `harts` 区间写成了 `0..3` —— 检查 `platform/visionfive2.rs` |
| 输出乱码 | `uart0_clock` 不是 24 MHz |
| 启动时打印 `CONFIG ERROR: platform mismatch` | 用 `--features` 与 `--config` 混搭了不同平台; 用 `cargo xtask` |

## 7. 调试

真机调试需要 JTAG, 本课程不要求。但在 QEMU 上可以用同一份
`kernel/` 代码调试 (QEMU 配置用 `--config riscv64-qemu-virt`):

```bash
cargo xtask debug --config riscv64-qemu-virt
```

它会在一个端口上等待 gdb, 并打印出完整的 gdb 命令。因为
`kernel/` 在两个平台上完全相同, 在 QEMU 上定位到的逻辑问题
在真机上同样成立 —— 只有平台常量相关的差异需要单独核对。
