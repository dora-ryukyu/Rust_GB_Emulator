// 資料 1.6 ジョイパッド入力 (レジスタ $FF00)

// ボタンのビット表現 (P1レジスタ下位4ビット、押されたら0)
const BUTTON_A_OR_RIGHT: u8 = 0b0001; // Bit 0
const BUTTON_B_OR_LEFT: u8  = 0b0010; // Bit 1
const BUTTON_SELECT_OR_UP: u8= 0b0100; // Bit 2
const BUTTON_START_OR_DOWN:u8= 0b1000; // Bit 3

pub struct Joypad {
    // P1レジスタ ($FF00) の内容を模倣
    // Bit 5: P15 ボタンキー選択 (0=選択)
    // Bit 4: P14 方向キー選択 (0=選択)
    // Bit 3-0: 入力ライン (押されていたら0)
    p1_register_select: u8, // 上位4ビットは常に1、Bit 5と4が書き込み可能

    // 各ボタンの現在の状態 (true = 押されている)
    button_a: bool,
    button_b: bool,
    select: bool,
    start: bool,
    right: bool,
    left: bool,
    up: bool,
    down: bool,
}

impl Joypad {
    pub fn new() -> Self {
        Self {
            p1_register_select: 0xCF, // 初期状態: 何も選択されていない (Bit5=1, Bit4=1), 上位は1
            button_a: false, button_b: false, select: false, start: false,
            right: false, left: false, up: false, down: false,
        }
    }

    // P1/JOYPレジスタへの書き込み (資料 表 1.6.A)
    // value の Bit 5と4のみが影響
    pub fn write_p1(&mut self, value: u8) {
        // Bit 5 (ボタンキー) と Bit 4 (方向キー) の選択状態のみを更新
        // 他のビットへの書き込みは無視されるか、特定の効果はない
        self.p1_register_select = (self.p1_register_select & 0xCF) | (value & 0x30);
    }

    // P1/JOYPレジスタからの読み出し
    pub fn read_p1(&self) -> u8 {
        let mut result = self.p1_register_select | 0xC0; // 上位2ビットは常に1 (ハードウェア仕様による)

        // Bit 5 (P15) が0ならボタンキーが選択されている
        if (self.p1_register_select & 0b0010_0000) == 0 {
            if self.start    { result &= !BUTTON_START_OR_DOWN; }
            if self.select   { result &= !BUTTON_SELECT_OR_UP; }
            if self.button_b { result &= !BUTTON_B_OR_LEFT; }
            if self.button_a { result &= !BUTTON_A_OR_RIGHT; }
        }
        // Bit 4 (P14) が0なら方向キーが選択されている
        if (self.p1_register_select & 0b0001_0000) == 0 {
            if self.down  { result &= !BUTTON_START_OR_DOWN; }
            if self.up    { result &= !BUTTON_SELECT_OR_UP; }
            if self.left  { result &= !BUTTON_B_OR_LEFT; }
            if self.right { result &= !BUTTON_A_OR_RIGHT; }
        }
        // 何も選択されていない場合 (Bit5=1, Bit4=1)、または両方選択された場合(ハードウェア的には不定だが、ここではORで両方読む)
        // 一般的には、何も選択されていない場合は下位4ビットは$F (押されていない状態) を返す
        if (self.p1_register_select & 0x30) == 0x30 {
            result |= 0x0F;
        }
        result
    }

    // ボタンが押されたことを通知 (true = 押された)
    pub fn button_down(&mut self, key: GameboyKey) -> bool { // 割り込み要求のために状態変化を返す
        let mut changed = false;
        match key {
            GameboyKey::Right  => { if !self.right { self.right = true; changed = true; } }
            GameboyKey::Left   => { if !self.left  { self.left  = true; changed = true; } }
            GameboyKey::Up     => { if !self.up    { self.up    = true; changed = true; } }
            GameboyKey::Down   => { if !self.down  { self.down  = true; changed = true; } }
            GameboyKey::A      => { if !self.button_a { self.button_a = true; changed = true; } }
            GameboyKey::B      => { if !self.button_b { self.button_b = true; changed = true; } }
            GameboyKey::Select => { if !self.select   { self.select   = true; changed = true; } }
            GameboyKey::Start  => { if !self.start    { self.start    = true; changed = true; } }
        }
        // ジョイパッド割り込みは、選択されているラインのボタンが押された (High->Low遷移) 時に発生
        // ここでは単純化のため、いずれかのボタンが新たに押されたらtrueを返す
        changed && self.is_selected_line_active(key)
    }

    // ボタンが離されたことを通知
    pub fn button_up(&mut self, key: GameboyKey) {
        match key {
            GameboyKey::Right  => self.right = false,
            GameboyKey::Left   => self.left  = false,
            GameboyKey::Up     => self.up    = false,
            GameboyKey::Down   => self.down  = false,
            GameboyKey::A      => self.button_a = false,
            GameboyKey::B      => self.button_b = false,
            GameboyKey::Select => self.select = false,
            GameboyKey::Start  => self.start  = false,
        }
    }
    
    // 押されたキーが現在P1レジスタで選択されているラインに属するか
    fn is_selected_line_active(&self, key: GameboyKey) -> bool {
        let action_keys_selected = (self.p1_register_select & 0b0010_0000) == 0;
        let direction_keys_selected = (self.p1_register_select & 0b0001_0000) == 0;

        match key {
            GameboyKey::A | GameboyKey::B | GameboyKey::Select | GameboyKey::Start => action_keys_selected,
            GameboyKey::Right | GameboyKey::Left | GameboyKey::Up | GameboyKey::Down => direction_keys_selected,
        }
    }

    pub fn get_input_state_debug(&self) -> String {
        format!("A:{} B:{} Sel:{} St:{} R:{} L:{} U:{} D:{} (P1 Sel:{:02X})",
            self.button_a as u8, self.button_b as u8, self.select as u8, self.start as u8,
            self.right as u8, self.left as u8, self.up as u8, self.down as u8,
            self.p1_register_select)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum GameboyKey { Right, Left, Up, Down, A, B, Select, Start }