use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

#[derive(Debug)]
pub struct Cartridge {
    pub raw_data: Vec<u8>,
    pub title: String,
    pub cartridge_type_code: u8,
    pub rom_size_code: u8,
    pub ram_size_code: u8,
}

impl Cartridge {
    pub fn load<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut raw_data = Vec::new();
        file.read_to_end(&mut raw_data)?;

        if raw_data.len() < 0x014F + 1 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "ROM file is too small to contain a valid header."));
        }

        let title_bytes = &raw_data[0x0134..=0x0143];
        let title = title_bytes.iter()
            .take_while(|&&byte| byte != 0)
            .map(|&byte| byte as char)
            .collect::<String>()
            .trim_end_matches('\0')
            .to_string();

        let cartridge_type_code = raw_data[0x0147];
        let rom_size_code = raw_data[0x0148];
        let ram_size_code = raw_data[0x0149];

        Ok(Self {
            raw_data,
            title,
            cartridge_type_code,
            rom_size_code,
            ram_size_code,
        })
    }

    // ★ ここから追加 ★
    /// カートリッジがバッテリーバックアップを持っているか判定します。
    pub fn has_battery(&self) -> bool {
        matches!(self.cartridge_type_code,
            0x03 | 0x06 | 0x09 | 0x0D | 0x0F | 0x10 | 0x13 | 0x1B | 0x1E | 0x22 | 0xFF)
    }
    // ★ ここまで追加 ★

    pub fn cartridge_type_name(&self) -> String {
        match self.cartridge_type_code {
            0x00 => "ROM ONLY".to_string(),
            0x01 => "MBC1".to_string(),
            0x02 => "MBC1+RAM".to_string(),
            0x03 => "MBC1+RAM+BATTERY".to_string(),
            0x05 => "MBC2".to_string(),
            0x06 => "MBC2+BATTERY".to_string(),
            0x08 => "ROM+RAM".to_string(),
            0x09 => "ROM+RAM+BATTERY".to_string(),
            0x0B => "MMM01".to_string(),
            0x0C => "MMM01+RAM".to_string(),
            0x0D => "MMM01+RAM+BATTERY".to_string(),
            0x0F => "MBC3+TIMER+BATTERY".to_string(),
            0x10 => "MBC3+TIMER+RAM+BATTERY".to_string(),
            0x11 => "MBC3".to_string(),
            0x12 => "MBC3+RAM".to_string(),
            0x13 => "MBC3+RAM+BATTERY".to_string(),
            0x19 => "MBC5".to_string(),
            0x1A => "MBC5+RAM".to_string(),
            0x1B => "MBC5+RAM+BATTERY".to_string(),
            0x1C => "MBC5+RUMBLE".to_string(),
            0x1D => "MBC5+RUMBLE+RAM".to_string(),
            0x1E => "MBC5+RUMBLE+RAM+BATTERY".to_string(),
            0x20 => "MBC6".to_string(),
            0x22 => "MBC7+SENSOR+RUMBLE+RAM+BATTERY".to_string(),
            0xFC => "POCKET CAMERA".to_string(),
            0xFD => "BANDAI TAMA5".to_string(),
            0xFE => "HuC3".to_string(),
            0xFF => "HuC1+RAM+BATTERY".to_string(),
            _ => format!("Unknown ({:#04x})", self.cartridge_type_code),
        }
    }

    pub fn rom_size_str(&self) -> String {
        if self.rom_size_code <= 0x08 {
            format!("{} KB", 32 * (1 << self.rom_size_code))
        } else {
            format!("Unknown code ({:#04x})", self.rom_size_code)
        }
    }

    pub fn ram_size_str(&self) -> String {
        match self.ram_size_code {
            0x00 => "None".to_string(),
            0x01 => "2 KB (Unofficial)".to_string(),
            0x02 => "8 KB".to_string(),
            0x03 => "32 KB (4 banks of 8KB)".to_string(),
            0x04 => "128 KB (16 banks of 8KB)".to_string(),
            0x05 => "64 KB (8 banks of 8KB)".to_string(),
            _ => format!("Unknown code ({:#04x})", self.ram_size_code),
        }
    }

    pub fn print_header_info(&self) {
        println!("--- Cartridge Header Info ---");
        println!("Title: {}", self.title);
        println!("Cartridge Type: {} ({:#04x})", self.cartridge_type_name(), self.cartridge_type_code);
        println!("ROM Size: {} ({:#04x})", self.rom_size_str(), self.rom_size_code);
        println!("RAM Size: {} ({:#04x})", self.ram_size_str(), self.ram_size_code);
    }
}
