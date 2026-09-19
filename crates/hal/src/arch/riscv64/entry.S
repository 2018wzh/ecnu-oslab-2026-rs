.section .text.entry
.global _entry, secondary_entry
_entry:
    csrci sstatus, 2
    csrw sie, zero
    li t0, {hart_first}
    bltu a0, t0, .Lpark
    li t1, {hart_end}
    bgeu a0, t1, .Lpark
    la t0, __bss_start
    la t1, __bss_end
1:  bgeu t0, t1, 2f
    sd zero, 0(t0)
    addi t0, t0, 8
    j 1b
2:  la t0, boot_hart
    sd a0, 0(t0)
    j .Lstack
secondary_entry:
    csrci sstatus, 2
    csrw sie, zero
    li t0, {hart_first}
    bltu a0, t0, .Lpark
    li t1, {hart_end}
    bgeu a0, t1, .Lpark
.Lstack:
    li t0, {hart_first}
    sub t0, a0, t0
    addi t0, t0, 1
    slli t0, t0, {stack_shift}
    la sp, boot_stacks
    add sp, sp, t0
    mv tp, a0
    call start
.Lpark:
    wfi
    j .Lpark
.balign 4
.global early_trap
early_trap:
    csrci sstatus, 2
    j .Lpark
.section .bss
.balign 8
.global boot_hart
boot_hart: .quad 0
.section .stacks,"aw",@nobits
.balign 16
.global boot_stacks
boot_stacks: .space {stack_bytes}
