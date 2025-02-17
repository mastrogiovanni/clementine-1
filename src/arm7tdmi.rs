use std::convert::TryInto;

use crate::alu_instruction::ArmModeAluInstruction;
use crate::bitwise::Bits;
use crate::instruction::ArmModeInstruction;
use crate::{condition::Condition, cpsr::Cpsr, cpu::Cpu};

/// Contains the 16 registers for the CPU, latest (R15) is special because
/// is the program counter.
#[derive(Default)]
struct Registers([u32; 16]);

impl Registers {
    pub fn program_counter(&self) -> usize {
        self.0[15].try_into().unwrap()
    }

    #[cfg(test)] // TODO: remove cfg when this API will be used at least one in prod code.
    pub fn set_program_counter(&mut self, new_value: u32) {
        self.0[15] = new_value
    }

    pub fn advance_program_counter(&mut self, bytes: u32) {
        self.0[15] = self.0[15].wrapping_add(bytes);
    }

    #[allow(clippy::only_used_in_recursion)] // FIXME: Possible bug of clippy?
    pub fn set_register_at(&mut self, reg: usize, new_value: u32) {
        self.0[reg] = new_value;
    }

    pub const fn register_at(&self, reg: usize) -> u32 {
        self.0[reg]
    }
}

pub struct Arm7tdmi {
    rom: Vec<u8>,

    registers: Registers,
    cpsr: Cpsr,
}

const OPCODE_ARM_SIZE: usize = 4;

impl Cpu for Arm7tdmi {
    type OpCodeType = u32;
    type InstructionType = ArmModeInstruction;

    fn fetch(&self) -> Self::OpCodeType {
        let instruction_index = self.registers.program_counter();
        let end_instruction = instruction_index + OPCODE_ARM_SIZE;
        let data_instruction: [u8; 4] = self.rom[instruction_index..end_instruction]
            .try_into()
            .expect("`istruction` conversion into [u8; 4]");

        let op_code = u32::from_le_bytes(data_instruction);
        println!();
        println!("opcode -> {:b}", op_code);

        op_code
    }

    fn decode(&self, op_code: Self::OpCodeType) -> (Condition, Self::InstructionType) {
        let condition: u8 = (op_code >> 28) // bit 31..=28
            .try_into()
            .expect("conversion `condition` to u8");
        println!("condition -> {:x}", condition);

        let instruction: ArmModeInstruction = match op_code.try_into() {
            Ok(instruction) => instruction,
            Err(e) => todo!("{}", e),
        };

        println!("instruction -> {:?}", instruction);

        (condition.into(), instruction)
    }

    fn execute(&mut self, op_code: u32, instruction_type: ArmModeInstruction) {
        use ArmModeInstruction::*;
        match instruction_type {
            Branch => {
                self.branch(op_code);
            }
            BranchLink => {
                self.branch_link(op_code);
            }
            DataProcessing1 | DataProcessing2 | DataProcessing3 => {
                self.data_processing(op_code);
            }
            DataTransfer => {
                self.single_data_transfer(op_code);
            }
        }

        self.registers.advance_program_counter(4);
    }

    fn step(&mut self) {
        let op_code = self.fetch();

        let (condition, instruction) = self.decode(op_code);
        if self.cpsr.can_execute(condition) {
            self.execute(op_code, instruction)
        }
    }
}

impl Arm7tdmi {
    pub(crate) fn new(rom: Vec<u8>) -> Self {
        Self {
            rom,
            registers: Registers::default(),
            cpsr: Cpsr::default(),
        }
    }

    fn branch(&mut self, op_code: u32) {
        let offset = op_code & 0b0000_0000_1111_1111_1111_1111_1111_1111;
        println!("offset: {:?}", offset);

        self.registers.advance_program_counter(8 + offset * 4);
        println!("PC: {:?}", self.registers.program_counter());
    }

    fn branch_link(&mut self, op_code: u32) {
        let pc: u32 = self.registers.program_counter().try_into().unwrap();
        self.registers.set_register_at(14, pc.wrapping_add(4)); // R14 = LR

        let offset = op_code & 0b0000_0000_1111_1111_1111_1111_1111_1111;
        println!("offset: {:?}", offset);

        self.registers.advance_program_counter(8 + offset * 4);
        println!("PC: {:?}", self.registers.program_counter());
    }

    fn data_processing(&mut self, opcode: u32) {
        // bit [25] is I = Immediate Flag
        let i: bool = opcode.get_bit(25);
        // bits [24-21]
        let alu_opcode = opcode.get_bits(21..=24);
        // bit [20] is sets condition codes
        let _s = opcode.get_bit(20);
        // bits [15-12] are the Rd
        let rd = opcode.get_bits(12..=15);
        // bits [19-16] are the Rn
        let rn = opcode.get_bits(16..=19);

        let op2 = match i {
            // Register as 2nd Operand
            false => {
                // bits [6-5] - Shift Type (0=LSL, 1=LSR, 2=ASR, 3=ROR)
                let shift_type = opcode.get_bits(5..=6);
                // bit [4] - is Shift by Register Flag (0=Immediate, 1=Register)
                let r = opcode.get_bit(4);
                // bits [0-3] 2nd Operand Register (R0..R15) (including PC=R15)
                let mut op2 = opcode.get_bits(0..=3);

                match r {
                    // 0=Immediate, 1=Register
                    // Shift by amount
                    false => {
                        // bits [7-11] - Shift amount
                        let shift_amount = opcode.get_bits(7..=11);
                        op2 = self.shift(shift_type, shift_amount, op2);
                    }
                    // Shift by register
                    true => {
                        // bits [11-8] - Shift register (R0-R14) - only lower 8bit 0-255 used
                        let rs = opcode.get_bits(8..=11);
                        let shift_amount = self
                            .registers
                            .register_at(rs.try_into().unwrap())
                            .get_bits(0..=7);
                        op2 = self.shift_immediate(shift_amount, shift_type, op2);
                    }
                };

                op2
            }
            // Immediate as 2nd Operand
            true => {
                // bits [11-8] are ROR-Shift applied to nn
                let is = opcode.get_bits(8..=11);
                // bits [7-0] are the immediate value
                let nn = opcode.get_bits(0..=7);

                // I'm not sure about `* 2`
                nn.rotate_right(is * 2) // TODO: review "ROR-Shift applied to nn (0-30, in steps of 2)"
            }
        };

        match ArmModeAluInstruction::from(alu_opcode) {
            ArmModeAluInstruction::Mov => self.mov(rd.try_into().unwrap(), op2),
            ArmModeAluInstruction::Teq => self.teq(rn, op2),
            _ => todo!(),
        }
    }

    fn single_data_transfer(&mut self, opcode: u32) {
        let immediate = opcode.get_bit(25);
        let up_down = opcode.get_bit(23);

        // bits [19-16] - Base register
        let rn = opcode.get_bits(16..=19);

        // 0xF is register of PC
        let address = if rn == 0xF {
            let pc: u32 = self.registers.program_counter().try_into().unwrap();
            pc + 8_u32
        } else {
            self.registers.register_at(rn.try_into().unwrap())
        };

        // bits [15-12] - Source/Destination Register
        let rd = opcode.get_bits(12..=15);

        let offset: u32 = if immediate {
            todo!()
        } else {
            opcode.get_bits(0..=11)
        };

        let load_store: SingleDataTransfer =
            opcode.try_into().expect("convert to Single Data Transfer");

        match load_store {
            SingleDataTransfer::Ldr => self.registers.set_register_at(
                rd.try_into().unwrap(),
                if up_down {
                    address.wrapping_sub(offset)
                } else {
                    address.wrapping_add(offset)
                },
            ),
            _ => todo!(),
        }
    }

    fn mov(&mut self, rd: usize, op2: u32) {
        self.registers.set_register_at(rd, op2);
    }

    fn teq(&mut self, rn: u32, op2: u32) {
        let value = self.registers.register_at(rn.try_into().unwrap()) ^ op2;
        self.cpsr.set_sign_flag(value.is_bit_on(31));
        self.cpsr.set_zero_flag(value == 0);
    }

    fn shift(&mut self, shift_type: u32, shift_amount: u32, mut value: u32) -> u32 {
        match shift_amount {
            0 => match shift_type {
                // LSL#0: No shift performed, ie. directly value=Rm, the C flag is NOT affected.
                0 => (), // TODO: It's better to implement the logical instruction in order to execute directly LSL#0?
                // LSR#0: Interpreted as LSR#32, ie. value becomes zero, C becomes Bit 31 of Rm.
                1 => {
                    // TODO: It's better to implement the logical instruction in order to execute directly LSR#0?
                    let rm = self.registers.register_at(value.try_into().unwrap());
                    self.cpsr.set_sign_flag(rm.get_bit(31));
                    value = 0;
                }
                // ASR#0: Interpreted as ASR#32, ie. value and C are filled by Bit 31 of Rm.
                2 => {
                    // TODO: It's better to implement the logical instruction in order to execute directly ASR#0?
                    let rm = self.registers.register_at(value.try_into().unwrap());
                    match rm.get_bit(31) {
                        true => {
                            value = 1;
                            self.cpsr.set_sign_flag(true)
                        }
                        false => {
                            value = 0;
                            self.cpsr.set_sign_flag(true)
                        }
                    }
                }
                // ROR#0: Interpreted as RRX#1 (RCR), like ROR#1, but value Bit 31 set to old C.
                3 => {
                    // TODO: It's better to implement the logical instruction in order to execute directly RRX#0?
                    todo!("value Bit 31 set to old C"); // I'm not sure what "old C" means
                }
                _ => unreachable!(),
            },
            shift_amount => value = self.shift_immediate(shift_type, shift_amount, value),
        };

        value
    }

    fn shift_immediate(&self, shift_type: u32, shift_amount: u32, mut value: u32) -> u32 {
        match shift_type {
            // Logical Shift Left
            0 => value <<= shift_amount,
            // Logical Shift Right
            1 => value >>= shift_amount,
            // Arithmetic Shift Right
            2 => value = ((value as i32) >> shift_amount) as u32, // TODO: Review rust arithmetic shift right
            // Rotate Right
            3 => value = value.rotate_right(shift_amount as u32),
            _ => unreachable!(),
        }

        value
    }
}

enum SingleDataTransfer {
    Ldr,
    Str,
    Pld,
}

impl From<u32> for SingleDataTransfer {
    fn from(op_code: u32) -> Self {
        // TODO: possible improvements
        // - op_code.are_bits_on(31..28)
        // - op_code.is_on(31).and(30).and(29)...
        let must_for_pld = op_code.is_bit_on(31)
            && op_code.is_bit_on(30)
            && op_code.is_bit_on(29)
            && op_code.is_bit_on(28);
        if op_code.get_bit(20) {
            if must_for_pld {
                Self::Pld
            } else {
                Self::Ldr
            }
        } else {
            Self::Str
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::instruction::ArmModeInstruction;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn decode_branch() {
        let output: Result<ArmModeInstruction, String> =
            0b1110_1010_0000_0000_0000_0000_0111_1111.try_into();
        assert_eq!(output, Ok(ArmModeInstruction::Branch));
    }

    #[test]
    fn decode_branch_link() {
        let output: Result<ArmModeInstruction, String> =
            0b1110_1011_0000_0000_0000_0000_0111_1111.try_into();
        assert_eq!(output, Ok(ArmModeInstruction::BranchLink));
    }

    #[test]
    fn test_registers_14_after_branch_link() {
        let mut cpu: Arm7tdmi = Arm7tdmi::new(vec![]);
        cpu.registers = Registers([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        let pc: u32 = cpu.registers.program_counter().try_into().unwrap();
        cpu.branch_link(0b0);
        assert_eq!(cpu.registers.register_at(14), pc.wrapping_add(4));
    }

    #[test]
    fn check_mov_rx_immediate() {
        // MOV R0, 0
        let mut opcode: u32 = 0b1110_0011_1010_0000_0000_0000_0000_0000;

        // bits [11-8] are ROR-Shift applied to nn
        let is = opcode & 0b0000_0000_0000_0000_0000_1111_0000_0000;

        // MOV Rx,x
        let mut cpu = Arm7tdmi::new(vec![]);
        for rx in 0..=0xF {
            let register_for_op = rx << 12;
            let immediate_value = rx;

            // Rd parameter
            opcode = (opcode & 0b1111_1111_1111_1111_0000_1111_1111_1111) + register_for_op;
            // Immediate parameter
            opcode = (opcode & 0b1111_1111_1111_1111_1111_1111_0000_0000) + immediate_value;

            let (condition, instruction_type) = cpu.decode(opcode);
            assert_eq!(condition, Condition::AL);
            assert_eq!(instruction_type, ArmModeInstruction::DataProcessing3);

            cpu.execute(opcode, instruction_type);
            let rotated = rx.rotate_right(is * 2);
            if rotated == 15 {
                // NOTE: since is R15 you should also consider the advance of 4 bytes after execution.
                assert_eq!(
                    cpu.registers.register_at(rx.try_into().unwrap()),
                    rotated + 4
                );
            } else {
                assert_eq!(cpu.registers.register_at(rx.try_into().unwrap()), rotated);
            }
        }
    }

    #[test]
    fn check_teq() {
        let op_code: u32 = 0b1110_0001_0010_1001_0011_0000_0000_0000;
        let mut cpu = Arm7tdmi::new(vec![]);

        let (_, instruction) = cpu.decode(op_code);
        assert_eq!(instruction, ArmModeInstruction::DataProcessing1);

        let rn = 9_usize;
        cpu.registers.set_register_at(rn, 100);
        cpu.execute(op_code, instruction);
        assert!(!cpu.cpsr.sign_flag());
        assert!(!cpu.cpsr.zero_flag());
    }

    // TODO: this is only one case of these kind of instruction.
    // create other cases or other tests :).
    #[test]
    fn check_single_data_transfer() {
        let op_code: u32 = 0b1110_0101_1001_1111_1101_0000_0001_1000;
        let mut cpu = Arm7tdmi::new(vec![]);

        let (_, instruction) = cpu.decode(op_code);
        assert_eq!(instruction, ArmModeInstruction::DataTransfer);

        let rd: u8 = ((op_code & 0b0000_0000_0000_0000_1111_0000_0000_0000) >> 12)
            .try_into()
            .expect("conversion `rd` to u8");

        assert_eq!(rd, 13);

        // because in this specific case address will be
        // then will be 92 + 8 (.wrapping_sub(offset))
        cpu.registers.set_program_counter(92);

        cpu.execute(op_code, instruction);
        assert_eq!(cpu.registers.register_at(13), 76);
        assert_eq!(cpu.registers.program_counter(), 96);
    }
}
