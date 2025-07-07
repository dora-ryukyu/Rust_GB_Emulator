use crate::mmu::Mmu;
use std::fmt;

// --- (定数、CpuRegisters 構造体、CpuRegisters impl は変更なし) ---
pub const VBLANK_INTERRUPT_ADDR: u16 = 0x0040; pub const LCD_STAT_INTERRUPT_ADDR: u16 = 0x0048; pub const TIMER_INTERRUPT_ADDR: u16 = 0x0050; pub const SERIAL_INTERRUPT_ADDR: u16 = 0x0058; pub const JOYPAD_INTERRUPT_ADDR: u16 = 0x0060;
#[derive(Debug, Default, Clone, Copy)]
pub struct CpuRegisters { pub a: u8, pub f: u8, pub b: u8, pub c: u8, pub d: u8, pub e: u8, pub h: u8, pub l: u8, pub sp: u16, pub pc: u16, }
impl CpuRegisters { pub fn new() -> Self { Self { a: 0x01, f: 0xB0, b: 0x00, c: 0x13, d: 0x00, e: 0xD8, h: 0x01, l: 0x4D, sp: 0xFFFE, pc: 0x0100 } } pub fn af(&self) -> u16 { ((self.a as u16) << 8) | (self.f as u16) } pub fn bc(&self) -> u16 { ((self.b as u16) << 8) | (self.c as u16) } pub fn de(&self) -> u16 { ((self.d as u16) << 8) | (self.e as u16) } pub fn hl(&self) -> u16 { ((self.h as u16) << 8) | (self.l as u16) } pub fn set_af(&mut self, val: u16) { self.a = (val >> 8) as u8; self.f = (val & 0x00F0) as u8; } pub fn set_bc(&mut self, val: u16) { self.b = (val >> 8) as u8; self.c = (val & 0xFF) as u8; } pub fn set_de(&mut self, val: u16) { self.d = (val >> 8) as u8; self.e = (val & 0xFF) as u8; } pub fn set_hl(&mut self, val: u16) { self.h = (val >> 8) as u8; self.l = (val & 0xFF) as u8; } const ZERO_FLAG_POS: u8 = 7; const SUBTRACT_FLAG_POS: u8 = 6; const HALF_CARRY_FLAG_POS: u8 = 5; const CARRY_FLAG_POS: u8 = 4; pub fn f_z(&self) -> bool { (self.f & (1 << Self::ZERO_FLAG_POS)) != 0 } pub fn f_n(&self) -> bool { (self.f & (1 << Self::SUBTRACT_FLAG_POS)) != 0 } pub fn f_h(&self) -> bool { (self.f & (1 << Self::HALF_CARRY_FLAG_POS)) != 0 } pub fn f_c(&self) -> bool { (self.f & (1 << Self::CARRY_FLAG_POS)) != 0 } fn set_flag_value(&mut self, bit: u8, value: bool) { if value { self.f |= 1 << bit; } else { self.f &= !(1 << bit); } self.f &= 0xF0; } pub fn set_f_z(&mut self, val: bool) { self.set_flag_value(7, val); } pub fn set_f_n(&mut self, val: bool) { self.set_flag_value(6, val); } pub fn set_f_h(&mut self, val: bool) { self.set_flag_value(5, val); } pub fn set_f_c(&mut self, val: bool) { self.set_flag_value(4, val); } }
impl fmt::Display for CpuRegisters { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "AF:{:04X} BC:{:04X} DE:{:04X} HL:{:04X} SP:{:04X} PC:{:04X} Flags[Z:{} N:{} H:{} C:{}]", self.af(), self.bc(), self.de(), self.hl(), self.sp, self.pc, self.f_z() as u8, self.f_n() as u8, self.f_h() as u8, self.f_c() as u8 ) } }

pub struct Cpu { pub registers: CpuRegisters, pub mmu: Mmu, pub ime: bool, halted: bool, current_instruction_cycles: u8, pub total_clock_cycles: u64, }
impl Cpu {
    pub fn new(mmu: Mmu) -> Self { Self { registers: CpuRegisters::new(), mmu, ime: false, halted: false, current_instruction_cycles: 0, total_clock_cycles: 0, } }
    fn handle_interrupts(&mut self) -> bool { let ie = self.mmu.read_byte(0xFFFF); let mut if_val = self.mmu.read_io_register_byte(0xFF0F); let pending_and_enabled = if_val & ie & 0x1F; if self.halted && pending_and_enabled != 0 { self.halted = false; } if !self.ime || pending_and_enabled == 0 { return false; } self.ime = false; self.current_instruction_cycles += 20; for bit_num in 0..5 { if (pending_and_enabled & (1 << bit_num)) != 0 { if_val &= !(1 << bit_num); self.mmu.write_io_register_byte(0xFF0F, if_val); self.push_u16(self.registers.pc); self.registers.pc = match bit_num { 0 => VBLANK_INTERRUPT_ADDR, 1 => LCD_STAT_INTERRUPT_ADDR, 2 => TIMER_INTERRUPT_ADDR, 3 => SERIAL_INTERRUPT_ADDR, 4 => JOYPAD_INTERRUPT_ADDR, _ => unreachable!(), }; return true; } } false }
    pub fn step(&mut self) -> u8 { self.current_instruction_cycles = 0; if self.handle_interrupts() { self.total_clock_cycles += self.current_instruction_cycles as u64; return self.current_instruction_cycles; } if self.halted { self.current_instruction_cycles = 4; self.total_clock_cycles += self.current_instruction_cycles as u64; self.mmu.tick_components(self.current_instruction_cycles); return self.current_instruction_cycles; } let opcode_addr = self.registers.pc; let opcode = self.read_byte(opcode_addr); self.registers.pc = self.registers.pc.wrapping_add(1); self.execute_opcode(opcode); self.total_clock_cycles += self.current_instruction_cycles as u64; self.mmu.tick_components(self.current_instruction_cycles); self.current_instruction_cycles }
    fn read_byte(&mut self, addr: u16) -> u8 { let val = self.mmu.read_byte(addr); self.current_instruction_cycles += 4; val }
    fn write_byte(&mut self, addr: u16, val: u8) { self.mmu.write_byte(addr, val); self.current_instruction_cycles += 4; }
    fn write_word(&mut self, addr: u16, val: u16) { self.write_byte(addr, (val & 0xFF) as u8); self.write_byte(addr.wrapping_add(1), (val >> 8) as u8); }
    fn fetch_byte_operand(&mut self) -> u8 { let val = self.read_byte(self.registers.pc); self.registers.pc = self.registers.pc.wrapping_add(1); val }
    fn fetch_word_operand(&mut self) -> u16 { let low = self.fetch_byte_operand() as u16; let high = self.fetch_byte_operand() as u16; (high << 8) | low }
    fn push_u16(&mut self, val: u16) { self.registers.sp = self.registers.sp.wrapping_sub(1); self.write_byte(self.registers.sp, (val >> 8) as u8); self.registers.sp = self.registers.sp.wrapping_sub(1); self.write_byte(self.registers.sp, (val & 0xFF) as u8); self.current_instruction_cycles += 4; }
    fn pop_u16(&mut self) -> u16 { let low = self.read_byte(self.registers.sp) as u16; self.registers.sp = self.registers.sp.wrapping_add(1); let high = self.read_byte(self.registers.sp) as u16; self.registers.sp = self.registers.sp.wrapping_add(1); (high << 8) | low }
    fn alu_add_u8(&mut self, value: u8, use_carry: bool) -> u8 { let a = self.registers.a; let carry_val = if use_carry && self.registers.f_c() { 1 } else { 0 }; let result_u16 = a as u16 + value as u16 + carry_val as u16; let result_u8 = result_u16 as u8; self.registers.set_f_z(result_u8 == 0); self.registers.set_f_n(false); self.registers.set_f_h((a & 0x0F) + (value & 0x0F) + carry_val > 0x0F); self.registers.set_f_c(result_u16 > 0xFF); result_u8 }
    fn alu_sub_u8(&mut self, value: u8, use_carry: bool) -> u8 { let a = self.registers.a; let carry_val = if use_carry && self.registers.f_c() { 1 } else { 0 }; let result_u8 = a.wrapping_sub(value).wrapping_sub(carry_val); self.registers.set_f_z(result_u8 == 0); self.registers.set_f_n(true); self.registers.set_f_h((a & 0x0F) < (value & 0x0F) + carry_val); self.registers.set_f_c((a as u16) < (value as u16) + (carry_val as u16)); result_u8 }
    fn alu_and_u8(&mut self, value: u8) { self.registers.a &= value; self.registers.set_f_z(self.registers.a == 0); self.registers.set_f_n(false); self.registers.set_f_h(true); self.registers.set_f_c(false); }
    fn alu_or_u8(&mut self, value: u8) { self.registers.a |= value; self.registers.set_f_z(self.registers.a == 0); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(false); }
    fn alu_xor_u8(&mut self, value: u8) { self.registers.a ^= value; self.registers.set_f_z(self.registers.a == 0); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(false); }
    fn alu_cp_u8(&mut self, value: u8) { let a = self.registers.a; let result_u8 = a.wrapping_sub(value); self.registers.set_f_z(result_u8 == 0); self.registers.set_f_n(true); self.registers.set_f_h((a & 0x0F) < (value & 0x0F)); self.registers.set_f_c(a < value); }
    fn alu_inc_u8(&mut self, value: u8) -> u8 { let result = value.wrapping_add(1); self.registers.set_f_z(result == 0); self.registers.set_f_n(false); self.registers.set_f_h((value & 0x0F) == 0x0F); result }
    fn alu_dec_u8(&mut self, value: u8) -> u8 { let result = value.wrapping_sub(1); self.registers.set_f_z(result == 0); self.registers.set_f_n(true); self.registers.set_f_h((value & 0x0F) == 0x00); result }
    fn alu_add_hl_rr(&mut self, value: u16) { let hl = self.registers.hl(); let result = hl.wrapping_add(value); self.registers.set_f_n(false); self.registers.set_f_h((hl & 0x0FFF) + (value & 0x0FFF) > 0x0FFF); self.registers.set_f_c((hl as u32) + (value as u32) > 0xFFFF); self.registers.set_hl(result); self.current_instruction_cycles += 4; }
    fn cb_rlc(&mut self, value: u8) -> u8 { let c = (value >> 7) & 1; let res = (value << 1) | c; self.registers.set_f_z(res == 0); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(c == 1); res }
    fn cb_rrc(&mut self, value: u8) -> u8 { let c = value & 1; let res = (value >> 1) | (c << 7); self.registers.set_f_z(res == 0); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(c == 1); res }
    fn cb_rl(&mut self, value: u8) -> u8 { let oc = self.registers.f_c() as u8; let nc = (value >> 7) & 1; let res = (value << 1) | oc; self.registers.set_f_z(res == 0); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(nc == 1); res }
    fn cb_rr(&mut self, value: u8) -> u8 { let oc = self.registers.f_c() as u8; let nc = value & 1; let res = (value >> 1) | (oc << 7); self.registers.set_f_z(res == 0); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(nc == 1); res }
    fn cb_sla(&mut self, value: u8) -> u8 { let c = (value >> 7) & 1; let res = value << 1; self.registers.set_f_z(res == 0); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(c == 1); res }
    fn cb_sra(&mut self, value: u8) -> u8 { let c = value & 1; let res = (value >> 1) | (value & 0x80); self.registers.set_f_z(res == 0); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(c == 1); res }
    fn cb_swap(&mut self, value: u8) -> u8 { let res = (value << 4) | (value >> 4); self.registers.set_f_z(res == 0); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(false); res }
    fn cb_srl(&mut self, value: u8) -> u8 { let c = value & 1; let res = value >> 1; self.registers.set_f_z(res == 0); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(c == 1); res }
    fn execute_cb_prefixed(&mut self) { let cb_opcode = self.fetch_byte_operand(); let reg_idx = cb_opcode & 0x07; let operation_sub_type = (cb_opcode >> 3) & 0x07; let operation_main_type = cb_opcode >> 6; let (current_value, is_hl) = if reg_idx == 0x06 { (self.read_byte(self.registers.hl()), true) } else { (self.get_reg_by_idx(reg_idx), false) }; let result_value = match operation_main_type { 0x00 => { match operation_sub_type { 0x00 => self.cb_rlc(current_value), 0x01 => self.cb_rrc(current_value), 0x02 => self.cb_rl(current_value), 0x03 => self.cb_rr(current_value), 0x04 => self.cb_sla(current_value), 0x05 => self.cb_sra(current_value), 0x06 => self.cb_swap(current_value), 0x07 => self.cb_srl(current_value), _ => unreachable!(), } } 0x01 => { let bit_to_test = operation_sub_type; self.registers.set_f_z((current_value & (1 << bit_to_test)) == 0); self.registers.set_f_n(false); self.registers.set_f_h(true); current_value } 0x02 => current_value & !(1 << operation_sub_type), 0x03 => current_value | (1 << operation_sub_type), _ => unreachable!(), }; if operation_main_type != 0x01 { if is_hl { self.write_byte(self.registers.hl(), result_value); } else { self.set_reg_by_idx(reg_idx, result_value); } } }
    fn get_reg_by_idx(&self, idx: u8) -> u8 { match idx { 0 => self.registers.b, 1 => self.registers.c, 2 => self.registers.d, 3 => self.registers.e, 4 => self.registers.h, 5 => self.registers.l, 7 => self.registers.a, _ => unreachable!(), } }
    fn set_reg_by_idx(&mut self, idx: u8, val: u8) { match idx { 0 => self.registers.b = val, 1 => self.registers.c = val, 2 => self.registers.d = val, 3 => self.registers.e = val, 4 => self.registers.h = val, 5 => self.registers.l = val, 7 => self.registers.a = val, _ => unreachable!(), } }

    fn execute_opcode(&mut self, opcode: u8) {
        match opcode {
            // ... (多数の命令、変更がない箇所は省略) ...
            
            // ★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★
            //            DAA, ADD SP,e8, LD HL,SP+e8 の実装を信頼性の高いものに置換
            // ★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★★
            0x27 => { // DAA
                let mut a = self.registers.a;
                let mut adjust = if self.registers.f_c() { 0x60 } else { 0x00 };
                if self.registers.f_h() {
                    adjust |= 0x06;
                }
                if !self.registers.f_n() { // add
                    if a & 0x0F > 0x09 {
                        adjust |= 0x06;
                    }
                    if a > 0x99 {
                        adjust |= 0x60;
                    }
                    a = a.wrapping_add(adjust);
                } else { // sub
                    a = a.wrapping_sub(adjust);
                }
                
                self.registers.set_f_c(adjust >= 0x60);
                self.registers.set_f_h(false);
                self.registers.set_f_z(a == 0);
                self.registers.a = a;
            },

            0xE8 => { // ADD SP, e8
                let offset = self.fetch_byte_operand() as i8 as i16 as u16;
                let sp = self.registers.sp;
                self.registers.set_f_z(false);
                self.registers.set_f_n(false);
                self.registers.set_f_h((sp & 0x000F) + (offset & 0x000F) > 0x000F);
                self.registers.set_f_c((sp & 0x00FF) + (offset & 0x00FF) > 0x00FF);
                self.registers.sp = sp.wrapping_add(offset);
                self.current_instruction_cycles += 8;
            },
            
            0xF8 => { // LD HL, SP+e8
                let offset = self.fetch_byte_operand() as i8 as i16 as u16;
                let sp = self.registers.sp;
                let result = sp.wrapping_add(offset);
                self.registers.set_f_z(false);
                self.registers.set_f_n(false);
                self.registers.set_f_h((sp & 0x000F) + (offset & 0x000F) > 0x000F);
                self.registers.set_f_c((sp & 0x00FF) + (offset & 0x00FF) > 0x00FF);
                self.registers.set_hl(result);
                self.current_instruction_cycles += 4;
            },
            
            // ... (他の命令は変更なし) ...
            0x00 => { /* NOP */ }, 0x02 => { self.write_byte(self.registers.bc(), self.registers.a); }, 0x06 => { let n = self.fetch_byte_operand(); self.registers.b = n; }, 0x08 => { let addr = self.fetch_word_operand(); self.write_word(addr, self.registers.sp); }, 0x0A => { self.registers.a = self.read_byte(self.registers.bc()); }, 0x0E => { let n = self.fetch_byte_operand(); self.registers.c = n; }, 0x12 => { self.write_byte(self.registers.de(), self.registers.a); }, 0x16 => { let n = self.fetch_byte_operand(); self.registers.d = n; }, 0x1A => { self.registers.a = self.read_byte(self.registers.de()); }, 0x1E => { let n = self.fetch_byte_operand(); self.registers.e = n; }, 0x22 => { self.write_byte(self.registers.hl(), self.registers.a); self.registers.set_hl(self.registers.hl().wrapping_add(1)); }, 0x26 => { let n = self.fetch_byte_operand(); self.registers.h = n; }, 0x2A => { self.registers.a = self.read_byte(self.registers.hl()); self.registers.set_hl(self.registers.hl().wrapping_add(1)); }, 0x2E => { let n = self.fetch_byte_operand(); self.registers.l = n; }, 0x32 => { self.write_byte(self.registers.hl(), self.registers.a); self.registers.set_hl(self.registers.hl().wrapping_sub(1)); }, 0x36 => { let n = self.fetch_byte_operand(); self.write_byte(self.registers.hl(), n); }, 0x3A => { self.registers.a = self.read_byte(self.registers.hl()); self.registers.set_hl(self.registers.hl().wrapping_sub(1)); }, 0x3E => { let n = self.fetch_byte_operand(); self.registers.a = n; },
            0x40 => {}, 0x41 => self.registers.b = self.registers.c, 0x42 => self.registers.b = self.registers.d, 0x43 => self.registers.b = self.registers.e, 0x44 => self.registers.b = self.registers.h, 0x45 => self.registers.b = self.registers.l, 0x46 => { self.registers.b = self.read_byte(self.registers.hl()); }, 0x47 => self.registers.b = self.registers.a,
            0x48 => self.registers.c = self.registers.b, 0x49 => {}, 0x4A => self.registers.c = self.registers.d, 0x4B => self.registers.c = self.registers.e, 0x4C => self.registers.c = self.registers.h, 0x4D => self.registers.c = self.registers.l, 0x4E => { self.registers.c = self.read_byte(self.registers.hl()); }, 0x4F => self.registers.c = self.registers.a,
            0x50 => self.registers.d = self.registers.b, 0x51 => self.registers.d = self.registers.c, 0x52 => {}, 0x53 => self.registers.d = self.registers.e, 0x54 => self.registers.d = self.registers.h, 0x55 => self.registers.d = self.registers.l, 0x56 => { self.registers.d = self.read_byte(self.registers.hl()); }, 0x57 => self.registers.d = self.registers.a,
            0x58 => self.registers.e = self.registers.b, 0x59 => self.registers.e = self.registers.c, 0x5A => self.registers.e = self.registers.d, 0x5B => {}, 0x5C => self.registers.e = self.registers.h, 0x5D => self.registers.e = self.registers.l, 0x5E => { self.registers.e = self.read_byte(self.registers.hl()); }, 0x5F => self.registers.e = self.registers.a,
            0x60 => self.registers.h = self.registers.b, 0x61 => self.registers.h = self.registers.c, 0x62 => self.registers.h = self.registers.d, 0x63 => self.registers.h = self.registers.e, 0x64 => {}, 0x65 => self.registers.h = self.registers.l, 0x66 => { self.registers.h = self.read_byte(self.registers.hl()); }, 0x67 => self.registers.h = self.registers.a,
            0x68 => self.registers.l = self.registers.b, 0x69 => self.registers.l = self.registers.c, 0x6A => self.registers.l = self.registers.d, 0x6B => self.registers.l = self.registers.e, 0x6C => self.registers.l = self.registers.h, 0x6D => {}, 0x6E => { self.registers.l = self.read_byte(self.registers.hl()); }, 0x6F => self.registers.l = self.registers.a,
            0x70 => { self.write_byte(self.registers.hl(), self.registers.b); }, 0x71 => { self.write_byte(self.registers.hl(), self.registers.c); }, 0x72 => { self.write_byte(self.registers.hl(), self.registers.d); }, 0x73 => { self.write_byte(self.registers.hl(), self.registers.e); }, 0x74 => { self.write_byte(self.registers.hl(), self.registers.h); }, 0x75 => { self.write_byte(self.registers.hl(), self.registers.l); }, 0x77 => { self.write_byte(self.registers.hl(), self.registers.a); },
            0x78 => self.registers.a = self.registers.b, 0x79 => self.registers.a = self.registers.c, 0x7A => self.registers.a = self.registers.d, 0x7B => self.registers.a = self.registers.e, 0x7C => self.registers.a = self.registers.h, 0x7D => self.registers.a = self.registers.l, 0x7E => { self.registers.a = self.read_byte(self.registers.hl()); }, 0x7F => {},
            0xE0 => { let offset = self.fetch_byte_operand() as u16; self.write_byte(0xFF00 + offset, self.registers.a); }, 0xF0 => { let offset = self.fetch_byte_operand() as u16; self.registers.a = self.read_byte(0xFF00 + offset); }, 0xE2 => { self.write_byte(0xFF00 + self.registers.c as u16, self.registers.a); }, 0xF2 => { self.registers.a = self.read_byte(0xFF00 + self.registers.c as u16); }, 0xEA => { let addr = self.fetch_word_operand(); self.write_byte(addr, self.registers.a); }, 0xFA => { let addr = self.fetch_word_operand(); self.registers.a = self.read_byte(addr); },
            0x01 => { let nn = self.fetch_word_operand(); self.registers.set_bc(nn); }, 0x11 => { let nn = self.fetch_word_operand(); self.registers.set_de(nn); }, 0x21 => { let nn = self.fetch_word_operand(); self.registers.set_hl(nn); }, 0x31 => { let nn = self.fetch_word_operand(); self.registers.sp = nn; },
            0xF9 => { self.registers.sp = self.registers.hl(); self.current_instruction_cycles += 4; },
            0xF5 => { self.push_u16(self.registers.af()); }, 0xC5 => { self.push_u16(self.registers.bc()); }, 0xD5 => { self.push_u16(self.registers.de()); }, 0xE5 => { self.push_u16(self.registers.hl()); },
            0xF1 => { let val = self.pop_u16(); self.registers.set_af(val); }, 0xC1 => { let val = self.pop_u16(); self.registers.set_bc(val); }, 0xD1 => { let val = self.pop_u16(); self.registers.set_de(val); }, 0xE1 => { let val = self.pop_u16(); self.registers.set_hl(val); },
            0x80..=0x85 | 0x87 => { self.registers.a = self.alu_add_u8(self.get_reg_by_idx(opcode & 0x07), false); } 0x86 => { let val = self.read_byte(self.registers.hl()); self.registers.a = self.alu_add_u8(val, false); } 0xC6 => { let val = self.fetch_byte_operand(); self.registers.a = self.alu_add_u8(val, false); }
            0x88..=0x8D | 0x8F => { self.registers.a = self.alu_add_u8(self.get_reg_by_idx(opcode & 0x07), true); } 0x8E => { let val = self.read_byte(self.registers.hl()); self.registers.a = self.alu_add_u8(val, true); } 0xCE => { let val = self.fetch_byte_operand(); self.registers.a = self.alu_add_u8(val, true); }
            0x90..=0x95 | 0x97 => { self.registers.a = self.alu_sub_u8(self.get_reg_by_idx(opcode & 0x07), false); } 0x96 => { let val = self.read_byte(self.registers.hl()); self.registers.a = self.alu_sub_u8(val, false); } 0xD6 => { let val = self.fetch_byte_operand(); self.registers.a = self.alu_sub_u8(val, false); }
            0x98..=0x9D | 0x9F => { self.registers.a = self.alu_sub_u8(self.get_reg_by_idx(opcode & 0x07), true); } 0x9E => { let val = self.read_byte(self.registers.hl()); self.registers.a = self.alu_sub_u8(val, true); } 0xDE => { let val = self.fetch_byte_operand(); self.registers.a = self.alu_sub_u8(val, true); }
            0xA0..=0xA5 | 0xA7 => { self.alu_and_u8(self.get_reg_by_idx(opcode & 0x07)); } 0xA6 => { let val = self.read_byte(self.registers.hl()); self.alu_and_u8(val); } 0xE6 => { let val = self.fetch_byte_operand(); self.alu_and_u8(val); }
            0xB0..=0xB5 | 0xB7 => { self.alu_or_u8(self.get_reg_by_idx(opcode & 0x07)); } 0xB6 => { let val = self.read_byte(self.registers.hl()); self.alu_or_u8(val); } 0xF6 => { let val = self.fetch_byte_operand(); self.alu_or_u8(val); }
            0xA8..=0xAD | 0xAF => { self.alu_xor_u8(self.get_reg_by_idx(opcode & 0x07)); } 0xAE => { let val = self.read_byte(self.registers.hl()); self.alu_xor_u8(val); } 0xEE => { let val = self.fetch_byte_operand(); self.alu_xor_u8(val); }
            0xB8..=0xBD | 0xBF => { self.alu_cp_u8(self.get_reg_by_idx(opcode & 0x07)); } 0xBE => { let val = self.read_byte(self.registers.hl()); self.alu_cp_u8(val); } 0xFE => { let val = self.fetch_byte_operand(); self.alu_cp_u8(val); }
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x3C => { let reg_idx = (opcode >> 3) & 0b111; let old_val = self.get_reg_by_idx(reg_idx); let new_val = self.alu_inc_u8(old_val); self.set_reg_by_idx(reg_idx, new_val); }
            0x34 => { let addr = self.registers.hl(); let val = self.read_byte(addr); let result = self.alu_inc_u8(val); self.write_byte(addr, result); }
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x3D => { let reg_idx = (opcode >> 3) & 0b111; let old_val = self.get_reg_by_idx(reg_idx); let new_val = self.alu_dec_u8(old_val); self.set_reg_by_idx(reg_idx, new_val); }
            0x35 => { let addr = self.registers.hl(); let val = self.read_byte(addr); let result = self.alu_dec_u8(val); self.write_byte(addr, result); }
            0x09 => { self.alu_add_hl_rr(self.registers.bc()); } 0x19 => { self.alu_add_hl_rr(self.registers.de()); } 0x29 => { self.alu_add_hl_rr(self.registers.hl()); } 0x39 => { self.alu_add_hl_rr(self.registers.sp); }
            0x03 => { self.registers.set_bc(self.registers.bc().wrapping_add(1)); self.current_instruction_cycles += 4; } 0x13 => { self.registers.set_de(self.registers.de().wrapping_add(1)); self.current_instruction_cycles += 4; } 0x23 => { self.registers.set_hl(self.registers.hl().wrapping_add(1)); self.current_instruction_cycles += 4; } 0x33 => { self.registers.sp = self.registers.sp.wrapping_add(1); self.current_instruction_cycles += 4; }
            0x0B => { self.registers.set_bc(self.registers.bc().wrapping_sub(1)); self.current_instruction_cycles += 4; } 0x1B => { self.registers.set_de(self.registers.de().wrapping_sub(1)); self.current_instruction_cycles += 4; } 0x2B => { self.registers.set_hl(self.registers.hl().wrapping_sub(1)); self.current_instruction_cycles += 4; } 0x3B => { self.registers.sp = self.registers.sp.wrapping_sub(1); self.current_instruction_cycles += 4; }
            0x07 => { let a = self.registers.a; let c = (a >> 7) & 1; self.registers.a = (a << 1) | c; self.registers.set_f_z(false); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(c == 1); }
            0x0F => { let a = self.registers.a; let c = a & 1; self.registers.a = (a >> 1) | (c << 7); self.registers.set_f_z(false); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(c == 1); }
            0x17 => { let a = self.registers.a; let oc = self.registers.f_c() as u8; let nc = (a >> 7) & 1; self.registers.a = (a << 1) | oc; self.registers.set_f_z(false); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(nc == 1); }
            0x1F => { let a = self.registers.a; let oc = self.registers.f_c() as u8; let nc = a & 1; self.registers.a = (a >> 1) | (oc << 7); self.registers.set_f_z(false); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(nc == 1); }
            0x2F => { self.registers.a = !self.registers.a; self.registers.set_f_n(true); self.registers.set_f_h(true); }
            0x37 => { self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(true); }
            0x3F => { let c = self.registers.f_c(); self.registers.set_f_n(false); self.registers.set_f_h(false); self.registers.set_f_c(!c); }
            0x10 => { self.fetch_byte_operand(); /* STOP */ }
            0xC3 => { let addr = self.fetch_word_operand(); self.registers.pc = addr; self.current_instruction_cycles += 4;} 0xE9 => { self.registers.pc = self.registers.hl(); }
            0x20 => { let offset = self.fetch_byte_operand() as i8; if !self.registers.f_z() { self.registers.pc = self.registers.pc.wrapping_add_signed(offset as i16); self.current_instruction_cycles += 4; } }
            0x28 => { let offset = self.fetch_byte_operand() as i8; if  self.registers.f_z() { self.registers.pc = self.registers.pc.wrapping_add_signed(offset as i16); self.current_instruction_cycles += 4; } }
            0x30 => { let offset = self.fetch_byte_operand() as i8; if !self.registers.f_c() { self.registers.pc = self.registers.pc.wrapping_add_signed(offset as i16); self.current_instruction_cycles += 4; } }
            0x38 => { let offset = self.fetch_byte_operand() as i8; if  self.registers.f_c() { self.registers.pc = self.registers.pc.wrapping_add_signed(offset as i16); self.current_instruction_cycles += 4; } }
            0x18 => { let offset = self.fetch_byte_operand() as i8; self.registers.pc = self.registers.pc.wrapping_add_signed(offset as i16); self.current_instruction_cycles += 4; }
            0xC2 => { let addr = self.fetch_word_operand(); if !self.registers.f_z() { self.registers.pc = addr; self.current_instruction_cycles += 4;} }
            0xCA => { let addr = self.fetch_word_operand(); if  self.registers.f_z() { self.registers.pc = addr; self.current_instruction_cycles += 4;} }
            0xD2 => { let addr = self.fetch_word_operand(); if !self.registers.f_c() { self.registers.pc = addr; self.current_instruction_cycles += 4;} }
            0xDA => { let addr = self.fetch_word_operand(); if  self.registers.f_c() { self.registers.pc = addr; self.current_instruction_cycles += 4;} }
            0xCD => { let addr = self.fetch_word_operand(); self.push_u16(self.registers.pc); self.registers.pc = addr; }
            0xC4 => { let addr = self.fetch_word_operand(); if !self.registers.f_z() { self.push_u16(self.registers.pc); self.registers.pc = addr; } else {} }
            0xCC => { let addr = self.fetch_word_operand(); if  self.registers.f_z() { self.push_u16(self.registers.pc); self.registers.pc = addr; } else {} }
            0xD4 => { let addr = self.fetch_word_operand(); if !self.registers.f_c() { self.push_u16(self.registers.pc); self.registers.pc = addr; } else {} }
            0xDC => { let addr = self.fetch_word_operand(); if  self.registers.f_c() { self.push_u16(self.registers.pc); self.registers.pc = addr; } else {} }
            0xC9 => { self.registers.pc = self.pop_u16(); self.current_instruction_cycles += 4; }
            0xD9 => { self.registers.pc = self.pop_u16(); self.ime = true; self.current_instruction_cycles += 4; }
            0xC0 => { if !self.registers.f_z() { self.registers.pc = self.pop_u16(); self.current_instruction_cycles += 8; } else { self.current_instruction_cycles +=4; } }
            0xC8 => { if  self.registers.f_z() { self.registers.pc = self.pop_u16(); self.current_instruction_cycles += 8; } else { self.current_instruction_cycles +=4; } }
            0xD0 => { if !self.registers.f_c() { self.registers.pc = self.pop_u16(); self.current_instruction_cycles += 8; } else { self.current_instruction_cycles +=4; } }
            0xD8 => { if  self.registers.f_c() { self.registers.pc = self.pop_u16(); self.current_instruction_cycles += 8; } else { self.current_instruction_cycles +=4; } }
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => { self.push_u16(self.registers.pc); self.registers.pc = (opcode & 0b00111000) as u16; }
            0xF3 => { self.ime = false; } 0xFB => { self.ime = true; } 0x76 => { self.halted = true; }
            0xCB => { self.execute_cb_prefixed(); },

            _ => { panic!( "Unknown or Unhandled opcode: {:#04X} at PC: {:#06X}", opcode, self.registers.pc.wrapping_sub(1) ); }
        }
    }
    pub fn print_registers(&self) { println!("CPU: {} IME: {}", self.registers, self.ime); println!("Total Clock Cycles: {}", self.total_clock_cycles); }
}