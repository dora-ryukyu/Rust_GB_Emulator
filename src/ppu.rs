// src/ppu.rs

pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;
const VRAM_SIZE: usize = 8192;
const OAM_SIZE: usize = 160;
pub const PUB_OAM_SIZE: usize = OAM_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuMode { HBlank = 0, VBlank = 1, OamScan = 2, Drawing = 3 }
impl PpuMode { fn to_stat_bits(self) -> u8 { self as u8 } }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuInterruptType { None, VBlank, LcdStat }

// ★ ここから追加 ★
// 事前にカラーパレットを定義しておく
const PALETTES: [[u32; 4]; 4] = [
    [0x00E0F8D0, 0x0088C070, 0x00346856, 0x00081820], // デフォルト (薄緑)
    [0x00FFFFFF, 0x00AAAAAA, 0x00555555, 0x00000000], // グレースケール
    [0x00f8e4a0, 0x00d0a068, 0x00a06030, 0x00302010], // セピア
    [0x00d0f0f8, 0x0070a0c0, 0x00406080, 0x00102030], // クール
];
// ★ ここまで追加 ★

pub struct Ppu {
    pub vram: [u8; VRAM_SIZE],
    pub oam: [u8; OAM_SIZE],
    pub lcdc: u8,
    pub stat: u8,
    pub scy: u8,
    pub scx: u8,
    pub ly: u8,
    pub lyc: u8,
    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,
    pub wy: u8,
    pub wx: u8,
    pub current_mode: PpuMode,
    cycles_in_current_mode: u32,
    pub frame_buffer: [u32; SCREEN_WIDTH * SCREEN_HEIGHT],
    pub frame_ready: bool,
    scanline_sprites: Vec<(usize, u8)>, // (oam_address_base, x_pos)
    // ★ ここから追加 ★
    colors: [u32; 4],
    palette_index: usize,
    // ★ ここまで追加 ★
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            vram: [0; VRAM_SIZE], oam: [0; OAM_SIZE], lcdc: 0x91, stat: 0x80, scy: 0, scx: 0,
            ly: 0, lyc: 0, bgp: 0xFC, obp0: 0xFF, obp1: 0xFF, wy: 0, wx: 0,
            current_mode: PpuMode::OamScan, cycles_in_current_mode: 0,
            frame_buffer: [PALETTES[0][0]; SCREEN_WIDTH * SCREEN_HEIGHT],
            frame_ready: false,
            scanline_sprites: Vec::with_capacity(10),
            colors: PALETTES[0], // ★ 変更: デフォルトパレットで初期化
            palette_index: 0,    // ★ 追加
        }
    }

    pub fn get_colors(&self) -> &[u32; 4] {
        &self.colors
    }
    
    // ★ ここから追加 ★
    /// カラーパレットを次のプリセットに切り替えます。
    pub fn cycle_palette(&mut self) {
        self.palette_index = (self.palette_index + 1) % PALETTES.len();
        self.colors = PALETTES[self.palette_index];
        println!("Palette changed to index {}", self.palette_index);
    }
    // ★ ここまで追加 ★

    pub fn step(&mut self, cycles: u8) -> PpuInterruptType {
        // (step関数の中身は変更なし)
        if !self.is_lcd_enabled() {
            self.cycles_in_current_mode = 0;
            self.ly = 0;
            self.stat &= 0b11111100; // Mode 0
            self.current_mode = PpuMode::HBlank;
            return PpuInterruptType::None;
        }

        self.cycles_in_current_mode += cycles as u32;
        let mut interrupt = PpuInterruptType::None;

        match self.current_mode {
            PpuMode::OamScan => {
                if self.cycles_in_current_mode >= 80 {
                    self.cycles_in_current_mode -= 80;
                    if let PpuInterruptType::LcdStat = self.change_mode(PpuMode::Drawing) {
                        interrupt = PpuInterruptType::LcdStat;
                    }
                }
            }
            PpuMode::Drawing => {
                if self.cycles_in_current_mode >= 172 {
                    self.cycles_in_current_mode -= 172;
                    self.render_scanline();
                    if let PpuInterruptType::LcdStat = self.change_mode(PpuMode::HBlank) {
                        interrupt = PpuInterruptType::LcdStat;
                    }
                }
            }
            PpuMode::HBlank => {
                if self.cycles_in_current_mode >= 204 {
                    self.cycles_in_current_mode -= 204;
                    self.ly += 1;
                    if self.check_lyc_coincidence() {
                        interrupt = PpuInterruptType::LcdStat;
                    }

                    if self.ly == SCREEN_HEIGHT as u8 {
                        self.frame_ready = true;
                        interrupt = PpuInterruptType::VBlank;
                        if let PpuInterruptType::LcdStat = self.change_mode(PpuMode::VBlank) {
                            // STAT割り込みも同時に発生させる
                        }
                    } else {
                        if let PpuInterruptType::LcdStat = self.change_mode(PpuMode::OamScan) {
                           if interrupt == PpuInterruptType::None { interrupt = PpuInterruptType::LcdStat; }
                        }
                    }
                }
            }
            PpuMode::VBlank => {
                if self.cycles_in_current_mode >= 456 {
                    self.cycles_in_current_mode -= 456;
                    self.ly += 1;
                    if self.ly > 153 {
                        self.ly = 0;
                        if let PpuInterruptType::LcdStat = self.change_mode(PpuMode::OamScan) {
                            if interrupt == PpuInterruptType::None { interrupt = PpuInterruptType::LcdStat; }
                        }
                    }
                    if self.check_lyc_coincidence() {
                       if interrupt == PpuInterruptType::None { interrupt = PpuInterruptType::LcdStat; }
                    }
                }
            }
        }
        interrupt
    }
    
    fn change_mode(&mut self, new_mode: PpuMode) -> PpuInterruptType {
        // (change_mode関数の中身は変更なし)
        self.current_mode = new_mode;
        self.stat = (self.stat & 0b11111100) | self.current_mode.to_stat_bits();
        
        if new_mode == PpuMode::OamScan {
            self.perform_oam_scan();
        }

        let should_interrupt = match new_mode {
            PpuMode::HBlank if (self.stat & 0b00001000) != 0 => true,
            PpuMode::VBlank if (self.stat & 0b00010000) != 0 => true,
            PpuMode::OamScan if (self.stat & 0b00100000) != 0 => true,
            _ => false,
        };
        if should_interrupt { PpuInterruptType::LcdStat } else { PpuInterruptType::None }
    }
    
    fn check_lyc_coincidence(&mut self) -> bool {
        // (check_lyc_coincidence関数の中身は変更なし)
        if self.ly == self.lyc {
            self.stat |= 0b00000100;
            (self.stat & 0b01000000) != 0
        } else {
            self.stat &= !0b00000100;
            false
        }
    }

    fn perform_oam_scan(&mut self) {
        // (perform_oam_scan関数の中身は変更なし)
        self.scanline_sprites.clear();
        if !self.is_sprites_enabled() { return; }
        
        let sprite_height = self.get_sprite_height();
        let mut count = 0;
        for i in 0..40 {
            if count >= 10 { break; }
            let oam_addr_base = i * 4;
            let y_pos = self.oam[oam_addr_base];
            
            let screen_y_top = y_pos.wrapping_sub(16);
            if self.ly >= screen_y_top && self.ly < screen_y_top.wrapping_add(sprite_height) {
                let x_pos = self.oam[oam_addr_base + 1];
                self.scanline_sprites.push((oam_addr_base, x_pos));
                count += 1;
            }
        }
    }

    fn render_scanline(&mut self) {
        if self.ly >= SCREEN_HEIGHT as u8 { return; }
        let mut bg_pixel_color_ids: [u8; SCREEN_WIDTH] = [0; SCREEN_WIDTH];
        self.render_background_and_window(&mut bg_pixel_color_ids);
        if self.is_sprites_enabled() {
            self.render_sprites(&bg_pixel_color_ids);
        }
    }

    fn render_background_and_window(&mut self, bg_pixel_color_ids: &mut [u8; SCREEN_WIDTH]) {
        let bg_display_enabled = (self.lcdc & 1) != 0;
        let window_display_enabled = (self.lcdc & 0b00100000) != 0 && self.wy <= self.ly;
        let tile_data_base_addr: u16 = if (self.lcdc & 0b00010000) != 0 { 0x8000 } else { 0x9000 };
        let signed_addressing = (self.lcdc & 0b00010000) == 0;
        let frame_buffer_line_start_idx = self.ly as usize * SCREEN_WIDTH;
        for screen_x in 0..SCREEN_WIDTH {
            let on_window = window_display_enabled && (screen_x as u8) >= self.wx.saturating_sub(7);
            let (map_base, map_y, map_x) = if on_window {
                let win_map_base = if (self.lcdc & 0b01000000) != 0 { 0x9C00 } else { 0x9800 };
                (win_map_base, self.ly - self.wy, (screen_x as u8).wrapping_sub(self.wx.wrapping_sub(7)))
            } else {
                let bg_map_base = if (self.lcdc & 0b00001000) != 0 { 0x9C00 } else { 0x9800 };
                (bg_map_base, self.ly.wrapping_add(self.scy), (screen_x as u8).wrapping_add(self.scx))
            };
            let mut final_color_id = 0;
            if bg_display_enabled || on_window {
                let tile_row = (map_y / 8) as u16; let tile_col = (map_x / 8) as u16;
                let tile_map_addr = map_base + tile_row * 32 + tile_col;
                let tile_index = self.vram[(tile_map_addr - 0x8000) as usize];
                let tile_data_addr = if signed_addressing { (tile_data_base_addr as i32 + ((tile_index as i8) as i32 * 16)) as u16 } else { tile_data_base_addr + (tile_index as u16 * 16) };
                let y_in_tile = (map_y % 8) as u16; let line_data_addr = tile_data_addr + y_in_tile * 2;
                let byte1 = self.vram[(line_data_addr - 0x8000) as usize]; let byte2 = self.vram[(line_data_addr + 1 - 0x8000) as usize];
                let x_in_tile = 7 - (map_x % 8); let color_bit_1 = (byte1 >> x_in_tile) & 1; let color_bit_2 = (byte2 >> x_in_tile) & 1;
                final_color_id = (color_bit_2 << 1) | color_bit_1;
            }
            bg_pixel_color_ids[screen_x] = final_color_id;
            let shade_index = (self.bgp >> (final_color_id * 2)) & 0b11;
            self.frame_buffer[frame_buffer_line_start_idx + screen_x] = self.colors[shade_index as usize]; // ★ 変更: DMG_COLORS -> self.colors
        }
    }

    fn render_sprites(&mut self, bg_pixel_color_ids: &[u8; SCREEN_WIDTH]) {
        self.scanline_sprites.sort_unstable_by_key(|&(oam_idx, x_pos)| (x_pos, oam_idx));

        let sprite_height = self.get_sprite_height();
        let mut drawn_pixels: [bool; SCREEN_WIDTH] = [false; SCREEN_WIDTH];

        for (oam_addr_base, sprite_x_pos_raw) in self.scanline_sprites.iter() {
            let sprite_y_pos_raw = self.oam[*oam_addr_base];
            let tile_index = self.oam[*oam_addr_base + 2];
            let attributes = self.oam[*oam_addr_base + 3];

            let sprite_x = sprite_x_pos_raw.wrapping_sub(8);
            let y_flip = (attributes & 0b01000000) != 0;
            let x_flip = (attributes & 0b00100000) != 0;
            let palette = if (attributes & 0b00010000) != 0 { self.obp1 } else { self.obp0 };
            let bg_over_obj = (attributes & 0b10000000) != 0;
            
            let mut y_in_tile = self.ly.wrapping_sub(sprite_y_pos_raw.wrapping_sub(16));
            if y_flip { y_in_tile = sprite_height - 1 - y_in_tile; }

            let mut final_tile_index = tile_index as u16;
            if sprite_height == 16 {
                final_tile_index &= 0xFE;
                if y_in_tile >= 8 {
                    final_tile_index += 1;
                    y_in_tile -= 8;
                }
            }
            let tile_data_addr = 0x8000u16 + (final_tile_index * 16) + (y_in_tile as u16 * 2);

            let byte1 = self.vram[(tile_data_addr - 0x8000) as usize];
            let byte2 = self.vram[(tile_data_addr + 1 - 0x8000) as usize];

            for x_offset in 0..8 {
                let screen_x = sprite_x.wrapping_add(x_offset);
                if screen_x >= SCREEN_WIDTH as u8 || drawn_pixels[screen_x as usize] {
                    continue;
                }

                let color_bit_pos = if x_flip { x_offset } else { 7 - x_offset };
                let color_id = (((byte2 >> color_bit_pos) & 1) << 1) | ((byte1 >> color_bit_pos) & 1);

                if color_id == 0 { continue; }

                if bg_over_obj && bg_pixel_color_ids[screen_x as usize] != 0 {
                    continue;
                }
                
                let shade_index = (palette >> (color_id * 2)) & 0b11;
                let fb_idx = self.ly as usize * SCREEN_WIDTH + screen_x as usize;
                self.frame_buffer[fb_idx] = self.colors[shade_index as usize]; // ★ 変更: DMG_COLORS -> self.colors
                drawn_pixels[screen_x as usize] = true;
            }
        }
    }
    
    pub fn is_lcd_enabled(&self) -> bool { (self.lcdc & 0b10000000) != 0 }
    fn is_sprites_enabled(&self) -> bool { (self.lcdc & 0b00000010) != 0 }
    fn get_sprite_height(&self) -> u8 { if (self.lcdc & 0b00000100) != 0 { 16 } else { 8 } }
}
