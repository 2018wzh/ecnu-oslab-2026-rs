# QEMU virt — 地址清单 (供人工核对)

> 本仓库**不使用设备树**。机器的地址是编译期常量,
> 因此需要一份可以人工核对的清单。这份文件是那个核对入口。
>
> 用 `cargo xtask info --config riscv64-qemu-virt` 可以打印出
> 内核实际使用的值; 把输出与本表对照。
>
> 生成方式 (只用于**核对常量**, 运行期内核不解析 DTB):
>
> ```bash
> qemu-system-riscv64 -machine virt,dumpdtb=/tmp/virt.dtb -m 128M -smp 2 -nographic
> dtc -I dtb -O dts /tmp/virt.dtb | less
> ```

## 内存

| 项目 | 值 | 出处 / 说明 |
|---|---|---|
| DRAM base | `0x80000000` | QEMU virt 的内存映射。必须与 `-m` 一致 |
| DRAM size | `128 MiB` | `configs/riscv64-qemu-virt.toml` 的 `qemu.memory` |
| firmware base | `0x80000000` | OpenSBI 被 QEMU 加载到 DRAM 最前面 |
| firmware size | `2 MiB` | OpenSBI 实际约 333 KB, 但保留 2 MiB 是惯例 |
| kernel base | `0x80200000` | 固件跳转地址; 必须等于 `kernel_load_addr` |

## CPU

| 项目 | 值 | 出处 / 说明 |
|---|---|---|
| ncpu | `2` | 必须与 `-smp` 一致 |
| hart range | `[0, 1]` (闭区间) | QEMU 上所有 hart 都可用, 编号从 0 开始 |
| boot hart | `0` | OpenSBI 的 `Domain0 Boot HART` |

## 中断控制器 (SiFive PLIC)

| 项目 | 值 | 出处 / 说明 |
|---|---|---|
| PLIC base | `0x0c000000` | QEMU virt 内存映射 |
| PLIC size | `0x04000000` | 4 MiB 窗口 |

## 定时器

| 项目 | 值 | 出处 / 说明 |
|---|---|---|
| CLINT base | `0x02000000` | QEMU virt 内存映射 |
| mtime offset | `0xbff8` | **RISC-V 规范规定**, 不是 QEMU 的选择 |
| timer interval | `1000000` tick | timebase 10 MHz -> 0.1 秒 |
| timer kind | `Clint` | `mtime` 可直接读; 但**设置**中断仍必须走 SBI |

> `mtimecmp` 是 **M-mode** 寄存器, S-mode 写它会触发非法指令异常。
> 所以即使 `mtime` 能读, 设置下一次中断也只有 SBI 一条路。

## 串口 (16550 兼容)

| 项目 | 值 | 出处 / 说明 |
|---|---|---|
| UART0 base | `0x10000000` | QEMU virt 内存映射 |
| UART0 irq | `10` | PLIC 中断号 |
| UART0 clock | `3686400` Hz | `115200 * 32`; 使分频系数能整除 |
| 波特率 | `115200` 8N1 | 驱动里的常量, 两个平台相同 |

**校验方式**: 启动横幅会打印 `uart16550 ready @ 0x10000000
divisor=2 irq=10`。

* `divisor = 3686400 / (16 * 115200) = 2` ✓
* 如果打印的 divisor 不是 2, 说明 `uart0_clock` 抄错了。

## 块设备 (VirtIO-MMIO)

| 项目 | 值 | 出处 / 说明 |
|---|---|---|
| virtio0 base | `0x10001000` | 第一个 virtio-mmio 槽位 |
| virtio0 irq | `1` | PLIC 中断号 |
| 槽位数量 | `8` | QEMU virt 提供 8 个 4 KiB 槽位 |
| 槽位步长 | `0x1000` | virtio-mmio 规范: 每槽一页 |
| 设备型号 | `VirtioMmio` | 见 `platform::BlockKind` |

**驱动会扫描全部 8 个槽位**而不是直接用槽位 0, 因为槽位的分配
取决于 QEMU 命令行上设备的顺序。硬编码槽位 0 会在命令行一改时
失效, 而且失败方式是"读到一个网卡然后崩溃"。

## 启动参数

```bash
qemu-system-riscv64 \
    -machine virt \
    -cpu rv64 \
    -m 128M \
    -smp 2 \
    -bios default \
    -kernel target/riscv64-qemu-virt/kernel.elf \
    -nographic \
    -no-reboot
```

`xtask` 会用上面这条命令 (但加上 `-cpu` 与 `-machine` 分开写 ——
一个真实的坑: `-machine virt,cpu=rv64` 会报
`Property 'virt-machine.cpu' not found`, 而那个报错完全不提示
"应该改用 `-cpu`")。

## 已知的差异陷阱

| 陷阱 | 现象 |
|---|---|
| `-machine virt,cpu=rv64` | `Property 'virt-machine.cpu' not found` |
| `-device loader` 当内核加载器 | OpenSBI 打印 `Domain0 Next Address : 0x0`, 内核**完全无输出** — 因为它不设置固件的 payload |
| `-m` 与 `dram_size` 不一致 | 分配器可能发出指向不存在内存的页框, 写入被静默丢弃 |
| `-smp` 与 `ncpu` 不一致 | `-smp` 更大 -> 内核少起一个核; `-smp` 更小 -> `hart_start` 返回错误 |
