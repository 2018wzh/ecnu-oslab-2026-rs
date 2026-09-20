fn main() {
    println!("cargo:rerun-if-changed=arch/riscv64/user.ld");
}
