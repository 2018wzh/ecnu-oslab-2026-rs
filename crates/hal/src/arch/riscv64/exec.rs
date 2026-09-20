use super::trap::TrapFrame;
pub fn machine(machine: u16) -> bool { machine == 243 }
pub fn frame(frame: &mut TrapFrame, pc: usize, sp: usize, argc: usize, argv: usize) {
    frame.x = [0; 32]; frame.epc = pc; frame.status = 1 << 5;
    frame.x[2] = sp; frame.x[10] = argc; frame.x[11] = argv;
}
