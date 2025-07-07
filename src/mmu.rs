// src/mmu.rs

use crate::cartridge::Cartridge;
use crate::ppu::{Ppu, PpuMode};
use crate::timer::Timer;
use crate::joypad::Joypad;
use crate::apu::Apu;
use chrono::Utc;

const WRAM_SIZE: usize = 8192;
const HRAM_SIZE: usize = 127;
const IO_REG_SIZE: usize = 128;
const OAM_SIZE: usize = 160;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mbc {
    RomOnly,
    Mbc1,
    Mbc2,
    Mbc3,
    Mbc5,
}

pub struct Mmu {
    pub cartridge: Cartridge,
    pub ppu: Ppu,
    pub timer: Timer,
    pub joypad: Joypad,
    pub apu: Apu,
    wram: [u8; WRAM_SIZE],
    hram: [u8; HRAM_SIZE],
    io_registers: [u8; IO_REG_SIZE],
    interrupt_enable_register: u8,
    pub external_ram: Vec<u8>,
    mbc: Mbc,
    current_rom_bank: usize,
    current_ram_bank: usize,
    ram_and_rtc_enabled: bool,
    mbc1_banking_mode: u8,
    mbc2_ram: [u8; 512],
    rtc_registers: [u8; 5],
    latched_rtc_registers: [u8; 5],
    rtc_latch_written_00: bool,
    rtc_last_timestamp: i64,
}


impl Mmu {
    pub fn new(cartridge: Cartridge, apu: Apu) -> Self {
        let ram_size_code = cartridge.ram_size_code;
        let external_ram_size = if (0x05..=0x06).contains(&cartridge.cartridge_type_code) {
            0
        } else {
            match ram_size_code {
                0x00 => 0, 0x01 => 2 * 1024, 0x02 => 8 * 1024, 0x03 => 32 * 1024,
                0x04 => 128 * 1024, 0x05 => 64 * 1024, _ => 0,
            }
        };
        let mbc_type = match cartridge.cartridge_type_code {
            0x00 | 0x08 | 0x09 => Mbc::RomOnly,
            0x01..=0x03 => Mbc::Mbc1,
            0x05..=0x06 => Mbc::Mbc2,
            0x0F..=0x13 => Mbc::Mbc3,
            0x19..=0x1E => Mbc::Mbc5,
            _ => panic!("Unsupported cartridge type: {:#04x}", cartridge.cartridge_type_code),
        };
        println!("MBC type detected: {:?}", mbc_type);
        let mut mmu = Self {
            cartridge,
            ppu: Ppu::new(),
            timer: Timer::new(),
            joypad: Joypad::new(),
            apu,
            wram: [0; WRAM_SIZE],
            hram: [0; HRAM_SIZE],
            io_registers: [0; IO_REG_SIZE],
            interrupt_enable_register: 0x00,
            external_ram: vec![0; external_ram_size],
            mbc: mbc_type,
            current_rom_bank: 1,
            current_ram_bank: 0,
            ram_and_rtc_enabled: false,
            mbc1_banking_mode: 0,
            mbc2_ram: [0; 512],
            rtc_registers: [0; 5],
            latched_rtc_registers: [0; 5],
            rtc_latch_written_00: false,
            rtc_last_timestamp: Utc::now().timestamp(),
        };
        mmu.io_registers[0x0F] = 0xE1;
        mmu
    }

    pub fn load_ram_and_rtc(&mut self, data: &[u8]) {
        if self.mbc == Mbc::Mbc2 {
            if data.len() >= 512 {
                self.mbc2_ram.copy_from_slice(&data[0..512]);
                println!("MBC2 RAM data loaded.");
            }
            return;
        }
        if self.external_ram.is_empty() { return; }
        let ram_size = self.external_ram.len();
        if data.len() >= ram_size {
            self.external_ram.copy_from_slice(&data[0..ram_size]);
        }
        if self.mbc == Mbc::Mbc3 && data.len() >= ram_size + std::mem::size_of::<i64>() + 5 {
             let rtc_data_start = ram_size;
             let timestamp_bytes: [u8; 8] = data[rtc_data_start..rtc_data_start+8].try_into().unwrap();
             self.rtc_last_timestamp = i64::from_le_bytes(timestamp_bytes);
             self.rtc_registers.copy_from_slice(&data[rtc_data_start+8..rtc_data_start+8+5]);
             self.update_rtc();
             println!("RTC data loaded.");
        }
    }
    
    pub fn get_ram_and_rtc_data(&mut self) -> Vec<u8> {
        if self.mbc == Mbc::Mbc2 {
            return self.mbc2_ram.to_vec();
        }
        let mut data = self.external_ram.clone();
        if self.mbc == Mbc::Mbc3 {
            self.update_rtc();
            data.extend_from_slice(&self.rtc_last_timestamp.to_le_bytes());
            data.extend_from_slice(&self.rtc_registers);
        }
        data
    }
    
    fn update_rtc(&mut self) {
        let now = Utc::now().timestamp();
        let elapsed_secs = now - self.rtc_last_timestamp;
        if elapsed_secs <= 0 { return; }
        if (self.rtc_registers[4] & 0x40) != 0 {
            self.rtc_last_timestamp = now;
            return;
        }
        let mut seconds = u64::from(self.rtc_registers[0]);
        let mut minutes = u64::from(self.rtc_registers[1]);
        let mut hours = u64::from(self.rtc_registers[2]);
        let mut days = (u64::from(self.rtc_registers[4] & 1) << 8) | u64::from(self.rtc_registers[3]);
        let total_seconds = seconds + minutes * 60 + hours * 3600 + days * 86400 + elapsed_secs as u64;
        days = total_seconds / 86400;
        let remaining_seconds = total_seconds % 86400;
        hours = remaining_seconds / 3600;
        let remaining_seconds = remaining_seconds % 3600;
        minutes = remaining_seconds / 60;
        seconds = remaining_seconds % 60;
        self.rtc_registers[0] = seconds as u8;
        self.rtc_registers[1] = minutes as u8;
        self.rtc_registers[2] = hours as u8;
        self.rtc_registers[3] = days as u8;
        self.rtc_registers[4] = (self.rtc_registers[4] & 0xFE) | ((days >> 8) & 1) as u8;
        if days > 511 { self.rtc_registers[4] |= 0x80; }
        self.rtc_last_timestamp = now;
    }
    
    pub fn tick_components(&mut self, cpu_t_cycles: u8) {
        let ppu_interrupt = self.ppu.step(cpu_t_cycles);
        match ppu_interrupt {
            crate::ppu::PpuInterruptType::VBlank => self.request_interrupt(0),
            crate::ppu::PpuInterruptType::LcdStat => self.request_interrupt(1),
            crate::ppu::PpuInterruptType::None => {}
        }
        self.timer.tick(cpu_t_cycles);
        if self.timer.take_interrupt_request() {
            self.request_interrupt(2);
        }
        self.apu.tick(cpu_t_cycles);
    }
    
    pub fn request_interrupt(&mut self, interrupt_bit: u8) {
        if interrupt_bit < 5 {
            self.io_registers[0x0F] |= 1 << interrupt_bit;
        }
    }
    
    fn dma_read_byte(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x7FFF => self.cartridge.raw_data[address as usize],
            0x8000..=0x9FFF => self.ppu.vram[(address - 0x8000) as usize],
            0xA000..=0xBFFF => {
                 if self.ram_and_rtc_enabled && !self.external_ram.is_empty() {
                    let ram_addr = (self.current_ram_bank * 0x2000) + (address - 0xA000) as usize;
                    if ram_addr < self.external_ram.len() { self.external_ram[ram_addr] } else { 0xFF }
                } else { 0xFF }
            }
            0xC000..=0xDFFF => self.wram[(address - 0xC000) as usize],
            0xE000..=0xFDFF => self.wram[(address - 0xE000) as usize],
            _ => 0xFF,
        }
    }

    pub fn read_byte(&self, address: u16) -> u8 {
        match address {
            // ★★★ 変更点: このブロックのロジックを大幅に簡略化 ★★★
            0x0000..=0x3FFF => {
                // この領域は常にROMの先頭16KB (バンク0) を指す固定領域。
                self.cartridge.raw_data[address as usize]
            },
            0x4000..=0x7FFF => {
                let offset = address as usize - 0x4000;
                let rom_addr = (self.current_rom_bank * 0x4000) + offset;
                if rom_addr < self.cartridge.raw_data.len() { self.cartridge.raw_data[rom_addr] } else { 0xFF }
            },
            0x8000..=0x9FFF => { if self.ppu.is_lcd_enabled() && self.ppu.current_mode == PpuMode::Drawing { return 0xFF; } self.ppu.vram[(address - 0x8000) as usize] },
            0xA000..=0xBFFF => {
                if self.mbc == Mbc::Mbc2 {
                    if self.ram_and_rtc_enabled {
                        return self.mbc2_ram[(address & 0x01FF) as usize] | 0xF0;
                    } else {
                        return 0xFF;
                    }
                }
                if !self.ram_and_rtc_enabled { return 0xFF; }
                if self.mbc == Mbc::Mbc3 && self.current_ram_bank >= 0x08 {
                    return self.latched_rtc_registers[self.current_ram_bank - 0x08];
                }
                if !self.external_ram.is_empty() {
                    let ram_addr = (self.current_ram_bank * 0x2000) + (address - 0xA000) as usize;
                    if ram_addr < self.external_ram.len() { self.external_ram[ram_addr] } else { 0xFF }
                } else { 0xFF }
            },
            0xC000..=0xDFFF => self.wram[(address - 0xC000) as usize],
            0xE000..=0xFDFF => self.wram[(address - 0xE000) as usize],
            0xFE00..=0xFE9F => { if self.ppu.is_lcd_enabled() && (self.ppu.current_mode == PpuMode::OamScan || self.ppu.current_mode == PpuMode::Drawing) { return 0xFF; } self.ppu.oam[(address - 0xFE00) as usize] },
            0xFEA0..=0xFEFF => 0xFF,
            0xFF00..=0xFF7F => self.read_io_register_byte(address),
            0xFF80..=0xFFFE => self.hram[(address - 0xFF80) as usize],
            0xFFFF => self.interrupt_enable_register,
        }
    }

    pub fn write_byte(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x7FFF => self.handle_mbc_write(address, value),
            0x8000..=0x9FFF => { if self.ppu.is_lcd_enabled() && self.ppu.current_mode == PpuMode::Drawing { return; } self.ppu.vram[(address - 0x8000) as usize] = value; },
            0xA000..=0xBFFF => {
                if self.mbc == Mbc::Mbc2 {
                    if self.ram_and_rtc_enabled {
                        self.mbc2_ram[(address & 0x01FF) as usize] = value & 0x0F;
                    }
                    return;
                }
                if !self.ram_and_rtc_enabled { return; }
                 if self.mbc == Mbc::Mbc3 && self.current_ram_bank >= 0x08 {
                    let rtc_reg_idx = self.current_ram_bank - 0x08;
                    self.rtc_registers[rtc_reg_idx] = value;
                    if rtc_reg_idx == 4 && (value & 0x40) == 0 {
                        self.update_rtc();
                    }
                    return;
                }
                if !self.external_ram.is_empty() {
                    let ram_addr = (self.current_ram_bank * 0x2000) + (address - 0xA000) as usize;
                    if ram_addr < self.external_ram.len() { self.external_ram[ram_addr] = value; }
                }
            },
            0xC000..=0xDFFF => self.wram[(address - 0xC000) as usize] = value,
            0xE000..=0xFDFF => self.wram[(address - 0xE000) as usize] = value,
            0xFE00..=0xFE9F => { if self.ppu.is_lcd_enabled() && (self.ppu.current_mode == PpuMode::OamScan || self.ppu.current_mode == PpuMode::Drawing) { return; } self.ppu.oam[(address - 0xFE00) as usize] = value; },
            0xFEA0..=0xFEFF => {},
            0xFF00..=0xFF7F => self.write_io_register_byte(address, value),
            0xFF80..=0xFFFE => self.hram[(address - 0xFF80) as usize] = value,
            0xFFFF => self.interrupt_enable_register = value,
        }
    }

    fn handle_mbc_write(&mut self, address: u16, value: u8) {
        match self.mbc {
            Mbc::RomOnly => { },
            Mbc::Mbc1 => self.handle_mbc1_write(address, value),
            Mbc::Mbc2 => self.handle_mbc2_write(address, value),
            Mbc::Mbc3 => self.handle_mbc3_write(address, value),
            Mbc::Mbc5 => self.handle_mbc5_write(address, value),
        }
    }
    
    fn handle_mbc2_write(&mut self, address: u16, value: u8) {
        if address < 0x4000 {
            if (address & 0x0100) == 0 {
                self.ram_and_rtc_enabled = (value & 0x0F) == 0x0A;
            } else {
                let bank = (value & 0x0F) as usize;
                self.current_rom_bank = if bank == 0 { 1 } else { bank };
                let num_rom_banks_actual = self.cartridge.raw_data.len() / 0x4000;
                if num_rom_banks_actual > 0 { self.current_rom_bank %= num_rom_banks_actual; }
            }
        }
    }

    fn handle_mbc5_write(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => { self.ram_and_rtc_enabled = (value & 0x0F) == 0x0A; },
            0x2000..=0x2FFF => { self.current_rom_bank = (self.current_rom_bank & 0x100) | (value as usize); },
            0x3000..=0x3FFF => { self.current_rom_bank = (self.current_rom_bank & 0x0FF) | (((value & 0x01) as usize) << 8); },
            0x4000..=0x5FFF => { self.current_ram_bank = (value & 0x0F) as usize; },
            _ => { }
        }
        let num_rom_banks_actual = self.cartridge.raw_data.len() / 0x4000;
        if num_rom_banks_actual > 0 { self.current_rom_bank %= num_rom_banks_actual; }
    }

    fn handle_mbc1_write(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => { self.ram_and_rtc_enabled = (value & 0x0F) == 0x0A; },
            0x2000..=0x3FFF => {
                let lower_bits = if (value & 0x1F) == 0 { 1 } else { value & 0x1F };
                self.current_rom_bank = (self.current_rom_bank & 0b0110_0000) | (lower_bits as usize);
            },
            0x4000..=0x5FFF => {
                if self.mbc1_banking_mode == 0 {
                    self.current_rom_bank = (self.current_rom_bank & 0b0001_1111) | (((value & 0x03) as usize) << 5);
                } else {
                    self.current_ram_bank = (value & 0x03) as usize;
                }
            },
            0x6000..=0x7FFF => { self.mbc1_banking_mode = value & 0x01; },
            _ => {},
        }
        let num_rom_banks_actual = self.cartridge.raw_data.len() / 0x4000;
        if num_rom_banks_actual > 0 { self.current_rom_bank %= num_rom_banks_actual; }
        if self.current_rom_bank == 0 { self.current_rom_bank = 1; }
    }

    fn handle_mbc3_write(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => { self.ram_and_rtc_enabled = (value & 0x0F) == 0x0A; },
            0x2000..=0x3FFF => {
                let bank = value & 0x7F;
                self.current_rom_bank = if bank == 0 { 1 } else { bank as usize };
                let num_rom_banks_actual = self.cartridge.raw_data.len() / 0x4000;
                 if num_rom_banks_actual > 0 { self.current_rom_bank %= num_rom_banks_actual; }
            },
            0x4000..=0x5FFF => { self.current_ram_bank = value as usize; },
            0x6000..=0x7FFF => {
                if value == 0x00 {
                    self.rtc_latch_written_00 = true;
                } else if value == 0x01 && self.rtc_latch_written_00 {
                    self.update_rtc();
                    self.latched_rtc_registers.copy_from_slice(&self.rtc_registers);
                    self.rtc_latch_written_00 = false;
                } else {
                    self.rtc_latch_written_00 = false;
                }
            },
            _ => {},
        }
    }

    pub fn read_io_register_byte(&self, address: u16) -> u8 {
        match address {
            0xFF00 => self.joypad.read_p1(),
            0xFF04 => self.timer.read_div(),
            0xFF05 => self.timer.read_tima(),
            0xFF06 => self.timer.read_tma(),
            0xFF07 => self.timer.read_tac(),
            0xFF0F => self.io_registers[0x0F],
            0xFF10..=0xFF26 | 0xFF30..=0xFF3F => self.apu.read_reg(address),
            0xFF40 => self.ppu.lcdc, 0xFF41 => self.ppu.stat,
            0xFF42 => self.ppu.scy, 0xFF43 => self.ppu.scx,
            0xFF44 => self.ppu.ly, 0xFF45 => self.ppu.lyc,
            0xFF47 => self.ppu.bgp, 0xFF48 => self.ppu.obp0,
            0xFF49 => self.ppu.obp1, 0xFF4A => self.ppu.wy,
            0xFF4B => self.ppu.wx,
            0xFF01..=0xFF02 | 0xFF4C..=0xFF7F => self.io_registers[(address - 0xFF00) as usize],
            _ => 0xFF,
        }
    }
    
    pub fn write_io_register_byte(&mut self, address: u16, value: u8) {
        match address {
            0xFF00 => self.joypad.write_p1(value),
            0xFF04 => self.timer.write_div(),
            0xFF05 => self.timer.write_tima(value),
            0xFF06 => self.timer.write_tma(value),
            0xFF07 => self.timer.write_tac(value),
            0xFF0F => self.io_registers[0x0F] = (value & 0x1F) | 0xE0,
            0xFF10..=0xFF26 | 0xFF30..=0xFF3F => self.apu.write_reg(address, value),
            0xFF40 => self.ppu.lcdc = value,
            0xFF41 => self.ppu.stat = (value & 0x78) | (self.ppu.stat & 0x87),
            0xFF42 => self.ppu.scy = value, 0xFF43 => self.ppu.scx = value,
            0xFF44 => {}, 0xFF45 => self.ppu.lyc = value,
            0xFF46 => self.dma_transfer(value),
            0xFF47 => self.ppu.bgp = value, 0xFF48 => self.ppu.obp0 = value,
            0xFF49 => self.ppu.obp1 = value, 0xFF4A => self.ppu.wy = value,
            0xFF4B => self.ppu.wx = value,
            0xFF01..=0xFF02 | 0xFF4C..=0xFF7F => {
                if address == 0xFF01 { self.io_registers[0x01] = value; }
                else if address == 0xFF02 { self.io_registers[0x02] = value & 0x81; if value == 0x81 { print!("{}", self.io_registers[0x01] as char); } }
                else { self.io_registers[(address - 0xFF00) as usize] = value; }
            },
            _ => {}
        }
    }
    
    fn dma_transfer(&mut self, start_address_high_byte: u8) {
        let start_address = (start_address_high_byte as u16) << 8;
        for i in 0..OAM_SIZE {
            let data = self.dma_read_byte(start_address + i as u16);
            self.ppu.oam[i] = data;
        }
    }

    pub fn read_u16(&self, address: u16) -> u16 {
        (self.read_byte(address.wrapping_add(1)) as u16) << 8 | (self.read_byte(address) as u16)
    }
    
    pub fn write_u16(&mut self, address: u16, value: u16) {
        self.write_byte(address, (value & 0xFF) as u8);
        self.write_byte(address.wrapping_add(1), (value >> 8) as u8);
    }
    
    pub fn dump_memory_range(&self, start_addr: u16, end_addr: u16) {
        println!("--- Memory Dump [{:#06X} - {:#06X}] ---", start_addr, end_addr);
        for addr_offset in 0..=(end_addr.saturating_sub(start_addr)) {
            let current_addr = start_addr.saturating_add(addr_offset);
            if addr_offset == 0 || addr_offset % 16 == 0 {
                if addr_offset != 0 { println!(); }
                print!("{:#06X}: ", current_addr);
            }
            print!("{:02X} ", self.read_byte(current_addr));
            if current_addr == end_addr { break; }
        }
        println!("\n-----------------------------");
    }
}