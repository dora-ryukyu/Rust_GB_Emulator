// src/timer.rs

pub struct Timer {
    internal_div_counter: u16,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    tima_reload_countdown: i8, // 0以下は遅延なし、>0でカウントダウン
    prev_timer_trigger_bit_state: bool, // 前回の relevant_bit の状態
    interrupt_request: bool,
}

impl Timer {
    pub fn new() -> Self {
        let tac = 0;
        Self {
            internal_div_counter: 0,
            tima: 0,
            tma: 0,
            tac,
            tima_reload_countdown: -1, // 遅延なし
            prev_timer_trigger_bit_state: Self::get_timer_enable_and_bit_state(0, tac), // 初期状態
            interrupt_request: false,
        }
    }

    // 現在のTACとDIVカウンタに基づいて、TIMAを駆動する最終的なANDゲートの出力がHighかどうか
    fn get_timer_enable_and_bit_state(div_counter: u16, tac_val: u8) -> bool {
        if (tac_val & 0b100) == 0 { // Timer Stop
            return false;
        }
        let selected_bit_pos = match tac_val & 0b11 {
            0b00 => 9, 0b01 => 3, 0b10 => 5, 0b11 => 7,
            _ => unreachable!(),
        };
        // (DIVの該当ビット) AND (TACの有効ビット)
        ((div_counter >> selected_bit_pos) & 1) != 0 // 有効ビットは上でチェック済みなので、DIVのビットのみでOK
    }


    pub fn tick(&mut self, t_cycles_elapsed: u8) {
        for _ in 0..t_cycles_elapsed {
            self.internal_div_counter = self.internal_div_counter.wrapping_add(1);

            if self.tima_reload_countdown > 0 {
                self.tima_reload_countdown -= 1;
                if self.tima_reload_countdown == 0 {
                    self.tima = self.tma;
                    self.interrupt_request = true;
                    self.tima_reload_countdown = -1; // 遅延完了
                }
            }

            let current_timer_enabled_and_bit_state = Self::get_timer_enable_and_bit_state(self.internal_div_counter, self.tac);

            // 立ち下がりエッジ: (前の状態がHigh) AND (現在の状態がLow)
            if self.prev_timer_trigger_bit_state && !current_timer_enabled_and_bit_state {
                if self.tima_reload_countdown <= 0 { // リロード中でなければ
                    self.tima = self.tima.wrapping_add(1);
                    if self.tima == 0 {
                        self.tima_reload_countdown = 4; // 4T後にTMAロード＆割り込み
                    }
                }
            }
            self.prev_timer_trigger_bit_state = current_timer_enabled_and_bit_state;
        }
    }
    pub fn read_div(&self) -> u8 { (self.internal_div_counter >> 8) as u8 }
    pub fn write_div(&mut self) {
        let old_trigger_state = Self::get_timer_enable_and_bit_state(self.internal_div_counter, self.tac);
        self.internal_div_counter = 0;
        let new_trigger_state = Self::get_timer_enable_and_bit_state(self.internal_div_counter, self.tac);
        if old_trigger_state && !new_trigger_state && self.tima_reload_countdown <= 0 {
            self.tima = self.tima.wrapping_add(1);
            if self.tima == 0 { self.tima_reload_countdown = 4; }
        }
        self.prev_timer_trigger_bit_state = new_trigger_state;
    }
    pub fn read_tima(&self) -> u8 { self.tima }
    pub fn write_tima(&mut self, value: u8) { if self.tima_reload_countdown <= 0 { self.tima = value; } }
    pub fn read_tma(&self) -> u8 { self.tma }
    pub fn write_tma(&mut self, value: u8) { self.tma = value; }
    pub fn read_tac(&self) -> u8 { self.tac | 0b1111_1000 }
    pub fn write_tac(&mut self, value: u8) {
        let old_trigger_state = Self::get_timer_enable_and_bit_state(self.internal_div_counter, self.tac);
        self.tac = value & 0b0000_0111;
        let new_trigger_state = Self::get_timer_enable_and_bit_state(self.internal_div_counter, self.tac);
        if old_trigger_state && !new_trigger_state && self.tima_reload_countdown <= 0 {
            self.tima = self.tima.wrapping_add(1);
            if self.tima == 0 { self.tima_reload_countdown = 4; }
        }
        self.prev_timer_trigger_bit_state = new_trigger_state;
    }
    pub fn take_interrupt_request(&mut self) -> bool { let req = self.interrupt_request; self.interrupt_request = false; req }
}