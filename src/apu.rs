// src/apu.rs

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const CPU_FREQ: u32 = 4_194_304;
const FRAME_SEQUENCER_DIVISOR: u32 = CPU_FREQ / 512;
// ★★★ 変更: バッファサイズを増やし、より長い時間軸の波形を保持 ★★★
const WAVEFORM_BUFFER_SIZE: usize = 512;


#[derive(Default, Clone, Copy)]
pub struct LengthCounter { pub counter: u16, pub enabled: bool, pub max_len: u16 }
impl LengthCounter { fn new(max_len: u16) -> Self { Self { counter: 0, enabled: false, max_len } } fn trigger(&mut self) { if self.counter == 0 { self.counter = self.max_len; } } fn tick(&mut self) -> bool { if self.enabled && self.counter > 0 { self.counter -= 1; if self.counter == 0 { return false; } } self.counter > 0 } fn load(&mut self, length_data: u8) { self.counter = self.max_len - (length_data as u16); } }

#[derive(Default, Clone, Copy)]
pub struct VolumeEnvelope { pub initial_volume: u8, pub direction: bool, pub period: u8, pub timer: u8, pub volume: u8, pub dac_enabled: bool }
impl VolumeEnvelope { fn trigger(&mut self) { self.timer = if self.period == 0 { 8 } else { self.period }; self.volume = self.initial_volume; } fn tick(&mut self) { if self.period == 0 { return; } self.timer = self.timer.saturating_sub(1); if self.timer == 0 { self.timer = if self.period == 0 { 8 } else { self.period }; let new_vol = if self.direction { self.volume.saturating_add(1) } else { self.volume.saturating_sub(1) }; if new_vol <= 15 { self.volume = new_vol; } } } }

#[derive(Default, Clone, Copy)]
pub struct Sweep { pub period: u8, pub direction: bool, pub shift: u8, pub timer: u8, pub enabled: bool, pub shadow_freq: u16 }
impl Sweep { fn trigger(&mut self, freq: u16) { self.shadow_freq = freq; self.timer = if self.period > 0 { self.period } else { 8 }; self.enabled = self.period > 0 || self.shift > 0; if self.shift > 0 { if self.calculate_new_freq() >= 2048 { self.enabled = false; } } } fn tick(&mut self, freq_reg: &mut u16, channel_enabled: &mut bool) { if !self.enabled { return; } self.timer = self.timer.saturating_sub(1); if self.timer == 0 { self.timer = if self.period > 0 { self.period } else { 8 }; if self.enabled && self.period > 0 { let new_freq = self.calculate_new_freq(); if new_freq < 2048 && self.shift > 0 { *freq_reg = new_freq; self.shadow_freq = new_freq; if self.calculate_new_freq() >= 2048 { *channel_enabled = false; } } else if new_freq >= 2048 { *channel_enabled = false; } } } } fn calculate_new_freq(&mut self) -> u16 { let offset = self.shadow_freq >> self.shift; if self.direction { self.shadow_freq.wrapping_sub(offset) } else { self.shadow_freq.wrapping_add(offset) } } }

const DUTY_PATTERNS: [[u8; 8]; 4] = [[0, 0, 0, 0, 0, 0, 0, 1], [1, 0, 0, 0, 0, 0, 0, 1], [1, 0, 0, 0, 0, 1, 1, 1], [0, 1, 1, 1, 1, 1, 1, 0]];
#[derive(Default, Clone, Copy)]
pub struct PulseChannel { pub enabled: bool, pub length_counter: LengthCounter, pub envelope: VolumeEnvelope, pub sweep: Sweep, pub freq_timer: u32, pub freq_reg: u16, pub duty_pattern: u8, pub duty_step: u8 }
impl PulseChannel { fn new(with_sweep: bool) -> Self { Self { length_counter: LengthCounter::new(64), sweep: if with_sweep { Sweep::default() } else { Sweep { enabled: false, ..Default::default() } }, ..Default::default() } } fn tick(&mut self, cycles: u32) { self.freq_timer = self.freq_timer.saturating_sub(cycles); if self.freq_timer == 0 { let period = (2048 - self.freq_reg as u32) * 4; self.freq_timer = if period == 0 { 8192 * 4 } else { period }; self.duty_step = (self.duty_step + 1) % 8; } } fn output(&self) -> u8 { if !self.enabled || !self.envelope.dac_enabled { return 0; } if DUTY_PATTERNS[self.duty_pattern as usize][self.duty_step as usize] == 1 { self.envelope.volume } else { 0 } } }

#[derive(Default, Clone, Copy)]
pub struct WaveChannel { pub enabled: bool, pub dac_enabled: bool, pub length_counter: LengthCounter, pub volume_level: u8, pub freq_timer: u32, pub freq_reg: u16, pub sample_index: u8, pub wave_ram: [u8; 16], pub sample_buffer: u8 }
impl WaveChannel { fn new() -> Self { Self { length_counter: LengthCounter::new(256), ..Default::default() } } fn tick(&mut self, cycles: u32) { self.freq_timer = self.freq_timer.saturating_sub(cycles); if self.freq_timer == 0 { let period = (2048 - self.freq_reg as u32) * 2; self.freq_timer = if period == 0 { 4096 * 2 } else { period }; self.sample_index = (self.sample_index + 1) % 32; let ram_byte = self.wave_ram[(self.sample_index / 2) as usize]; self.sample_buffer = if self.sample_index % 2 == 0 { ram_byte >> 4 } else { ram_byte & 0x0F }; } } fn output(&self) -> u8 { if !self.enabled || !self.dac_enabled { return 0; } let shift = match self.volume_level { 1 => 0, 2 => 1, 3 => 2, _ => 4, }; self.sample_buffer >> shift } }

#[derive(Default, Clone, Copy)]
pub struct NoiseChannel { pub enabled: bool, pub length_counter: LengthCounter, pub envelope: VolumeEnvelope, pub freq_timer: u32, pub lfsr: u16, pub width_mode: bool, pub clock_shift: u8, pub divisor_code: u8 }
impl NoiseChannel { fn new() -> Self { Self { length_counter: LengthCounter::new(64), lfsr: 0x7FFF, ..Default::default() } } fn tick(&mut self, cycles: u32) { self.freq_timer = self.freq_timer.saturating_sub(cycles); if self.freq_timer == 0 { let divisor = [8, 16, 32, 48, 64, 80, 96, 112]; let d = divisor[self.divisor_code as usize]; let period = (d as u32) << self.clock_shift; self.freq_timer = period; let xor_res = (self.lfsr & 1) ^ ((self.lfsr >> 1) & 1); self.lfsr >>= 1; self.lfsr |= xor_res << 14; if self.width_mode { self.lfsr = (self.lfsr & !(1 << 6)) | (xor_res << 6); } } } fn output(&self) -> u8 { if !self.enabled || !self.envelope.dac_enabled { return 0; } if (self.lfsr & 1) == 0 { self.envelope.volume } else { 0 } } }

#[derive(Default, Clone, Copy)]
pub struct ApuChannelState {
    pub enabled: bool,
    pub volume: u8,
    pub freq_reg: u16,
}

#[derive(Default, Clone)]
pub struct ApuState {
    pub ch1: ApuChannelState,
    pub ch2: ApuChannelState,
    pub ch3: ApuChannelState,
    pub ch4: ApuChannelState,
    pub wave_ram: [u8; 16],
}

pub struct Apu {
    ch1: PulseChannel,
    ch2: PulseChannel,
    ch3: WaveChannel,
    ch4: NoiseChannel,
    master_power: bool,
    master_vol_left: u8,
    master_vol_right: u8,
    panning: u8,
    cycle_counter: u32,
    frame_seq_counter: u32,
    frame_seq_step: u8,
    cycles_per_output_sample: u32,
    sample_buffer: Arc<Mutex<VecDeque<(f32, f32)>>>,
    ch_waveforms: [VecDeque<f32>; 4],
    hpf_cap_l: f32,
    hpf_cap_r: f32,
    last_raw_out_l: f32,
    last_raw_out_r: f32,
    hpf_alpha: f32,
}

impl Apu {
    pub fn new(output_sample_rate: u32) -> Self {
        const CUTOFF_FREQ: f32 = 20.0;
        let dt = 1.0 / output_sample_rate as f32;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * CUTOFF_FREQ);
        let hpf_alpha = rc / (rc + dt);

        Self {
            ch1: PulseChannel::new(true),
            ch2: PulseChannel::new(false),
            ch3: WaveChannel::new(),
            ch4: NoiseChannel::new(),
            master_power: false,
            master_vol_left: 0,
            master_vol_right: 0,
            panning: 0,
            cycle_counter: 0,
            frame_seq_counter: 0,
            frame_seq_step: 0,
            cycles_per_output_sample: CPU_FREQ / output_sample_rate,
            sample_buffer: Arc::new(Mutex::new(VecDeque::with_capacity(8192))),
            ch_waveforms: [
                VecDeque::with_capacity(WAVEFORM_BUFFER_SIZE),
                VecDeque::with_capacity(WAVEFORM_BUFFER_SIZE),
                VecDeque::with_capacity(WAVEFORM_BUFFER_SIZE),
                VecDeque::with_capacity(WAVEFORM_BUFFER_SIZE),
            ],
            hpf_cap_l: 0.0,
            hpf_cap_r: 0.0,
            last_raw_out_l: 0.0,
            last_raw_out_r: 0.0,
            hpf_alpha,
        }
    }
    
    pub fn get_sample_buffer_handle(&self) -> Arc<Mutex<VecDeque<(f32, f32)>>> {
        self.sample_buffer.clone()
    }
    
    pub fn get_channel_waveforms(&self) -> [Vec<f32>; 4] {
        [
            self.ch_waveforms[0].iter().cloned().collect(),
            self.ch_waveforms[1].iter().cloned().collect(),
            self.ch_waveforms[2].iter().cloned().collect(),
            self.ch_waveforms[3].iter().cloned().collect(),
        ]
    }

    pub fn tick(&mut self, cycles: u8) {
        let cycles_u32 = cycles as u32;

        if self.master_power {
            self.ch1.tick(cycles_u32);
            self.ch2.tick(cycles_u32);
            self.ch3.tick(cycles_u32);
            self.ch4.tick(cycles_u32);

            self.frame_seq_counter += cycles_u32;
            while self.frame_seq_counter >= FRAME_SEQUENCER_DIVISOR {
                self.frame_seq_counter -= FRAME_SEQUENCER_DIVISOR;
                self.step_frame_sequencer();
            }
        }
        
        self.cycle_counter += cycles_u32;
        while self.cycle_counter >= self.cycles_per_output_sample {
            self.cycle_counter -= self.cycles_per_output_sample;
            self.generate_sample();
        }
    }

    fn step_frame_sequencer(&mut self) {
        if self.frame_seq_step % 2 == 0 {
            if !self.ch1.length_counter.tick() { self.ch1.enabled = false; }
            if !self.ch2.length_counter.tick() { self.ch2.enabled = false; }
            if !self.ch3.length_counter.tick() { self.ch3.enabled = false; }
            if !self.ch4.length_counter.tick() { self.ch4.enabled = false; }
        }
        if self.frame_seq_step == 2 || self.frame_seq_step == 6 {
            self.ch1.sweep.tick(&mut self.ch1.freq_reg, &mut self.ch1.enabled);
        }
        if self.frame_seq_step == 7 {
            self.ch1.envelope.tick();
            self.ch2.envelope.tick();
            self.ch4.envelope.tick();
        }
        self.frame_seq_step = (self.frame_seq_step + 1) % 8;
    }

    fn generate_sample(&mut self) {
        let mut buffer = self.sample_buffer.lock().unwrap();
        if buffer.len() >= 4096 { return; }

        let mut raw_out_l = 0.0;
        let mut raw_out_r = 0.0;
        
        let s1 = (self.ch1.output() as f32 / 7.5) - 1.0;
        let s2 = (self.ch2.output() as f32 / 7.5) - 1.0;
        let s3 = (self.ch3.output() as f32 / 7.5) - 1.0;
        let s4 = (self.ch4.output() as f32 / 7.5) - 1.0;
        
        let ch_outputs = [s1, s2, s3, s4];
        for i in 0..4 {
            if self.ch_waveforms[i].len() >= WAVEFORM_BUFFER_SIZE {
                self.ch_waveforms[i].pop_front();
            }
            self.ch_waveforms[i].push_back(ch_outputs[i]);
        }

        if self.master_power {
            if (self.panning & 0x80) != 0 { raw_out_r += s4; }
            if (self.panning & 0x40) != 0 { raw_out_r += s3; }
            if (self.panning & 0x20) != 0 { raw_out_r += s2; }
            if (self.panning & 0x10) != 0 { raw_out_r += s1; }
            if (self.panning & 0x08) != 0 { raw_out_l += s4; }
            if (self.panning & 0x04) != 0 { raw_out_l += s3; }
            if (self.panning & 0x02) != 0 { raw_out_l += s2; }
            if (self.panning & 0x01) != 0 { raw_out_l += s1; }
        }
        
        raw_out_l = raw_out_l / 4.0;
        raw_out_r = raw_out_r / 4.0;
        
        let filtered_out_l = self.hpf_alpha * (self.hpf_cap_l + raw_out_l - self.last_raw_out_l);
        self.hpf_cap_l = filtered_out_l;
        self.last_raw_out_l = raw_out_l;

        let filtered_out_r = self.hpf_alpha * (self.hpf_cap_r + raw_out_r - self.last_raw_out_r);
        self.hpf_cap_r = filtered_out_r;
        self.last_raw_out_r = raw_out_r;

        let final_l = filtered_out_l * (((self.master_vol_left & 7) as f32 + 1.0) / 8.0);
        let final_r = filtered_out_r * (((self.master_vol_right & 7) as f32 + 1.0) / 8.0);

        buffer.push_back((final_l.clamp(-1.0, 1.0), final_r.clamp(-1.0, 1.0)));
    }
    
    pub fn get_apu_state(&self) -> ApuState {
        ApuState {
            ch1: ApuChannelState { enabled: self.ch1.enabled, volume: self.ch1.envelope.volume, freq_reg: self.ch1.freq_reg },
            ch2: ApuChannelState { enabled: self.ch2.enabled, volume: self.ch2.envelope.volume, freq_reg: self.ch2.freq_reg },
            ch3: ApuChannelState { enabled: self.ch3.enabled, volume: match self.ch3.volume_level { 0 => 0, 1 => 15, 2 => 15/2, 3 => 15/4, _=> 0}, freq_reg: self.ch3.freq_reg },
            ch4: ApuChannelState { enabled: self.ch4.enabled, volume: self.ch4.envelope.volume, freq_reg: (self.ch4.clock_shift as u16) << 8 | self.ch4.divisor_code as u16 },
            wave_ram: self.ch3.wave_ram,
        }
    }
    pub fn read_reg(&self, addr: u16) -> u8 {
        match addr {
            0xFF10 => (self.ch1.sweep.period << 4) | ((self.ch1.sweep.direction as u8) << 3) | self.ch1.sweep.shift | 0x80,
            0xFF11 => (self.ch1.duty_pattern << 6) | 0x3F,
            0xFF12 => (self.ch1.envelope.initial_volume << 4) | ((self.ch1.envelope.direction as u8) << 3) | self.ch1.envelope.period,
            0xFF13 => 0xFF,
            0xFF14 => (if self.ch1.length_counter.enabled { 0x40 } else { 0x00 }) | 0xBF,
            0xFF16 => (self.ch2.duty_pattern << 6) | 0x3F,
            0xFF17 => (self.ch2.envelope.initial_volume << 4) | ((self.ch2.envelope.direction as u8) << 3) | self.ch2.envelope.period,
            0xFF18 => 0xFF,
            0xFF19 => (if self.ch2.length_counter.enabled { 0x40 } else { 0x00 }) | 0xBF,
            0xFF1A => (if self.ch3.dac_enabled { 0x80 } else { 0x00 }) | 0x7F,
            0xFF1B => 0xFF,
            0xFF1C => (self.ch3.volume_level << 5) | 0x9F,
            0xFF1D => 0xFF,
            0xFF1E => (if self.ch3.length_counter.enabled { 0x40 } else { 0x00 }) | 0xBF,
            0xFF20 => 0xFF,
            0xFF21 => (self.ch4.envelope.initial_volume << 4) | ((self.ch4.envelope.direction as u8) << 3) | self.ch4.envelope.period,
            0xFF22 => (self.ch4.clock_shift << 4) | ((self.ch4.width_mode as u8) << 3) | self.ch4.divisor_code,
            0xFF23 => (if self.ch4.length_counter.enabled { 0x40 } else { 0x00 }) | 0xBF,
            0xFF24 => ((self.master_vol_left & 7) << 4) | (self.master_vol_right & 7),
            0xFF25 => self.panning,
            0xFF26 => { (if self.master_power { 0x80 } else { 0 }) | (if self.ch4.enabled { 0x08 } else { 0 }) | (if self.ch3.enabled { 0x04 } else { 0 }) | (if self.ch2.enabled { 0x02 } else { 0 }) | (if self.ch1.enabled { 0x01 } else { 0 }) | 0x70 },
            0xFF30..=0xFF3F => if self.ch3.enabled { 0xFF } else { self.ch3.wave_ram[(addr - 0xFF30) as usize] },
            _ => 0xFF,
        }
    }
    pub fn write_reg(&mut self, addr: u16, val: u8) {
        let was_master_power_on = self.master_power;
        if addr == 0xFF26 {
            self.master_power = (val & 0x80) != 0;
            if !was_master_power_on && self.master_power { self.frame_seq_step = 0; }
            else if was_master_power_on && !self.master_power { for i in 0xFF10..=0xFF25 { self.write_reg(i, 0); } }
        }

        if !self.master_power { return; }

        match addr {
            0xFF10 => { self.ch1.sweep.period = (val >> 4) & 7; self.ch1.sweep.direction = (val & 8) != 0; self.ch1.sweep.shift = val & 7; },
            0xFF11 => { self.ch1.duty_pattern = val >> 6; self.ch1.length_counter.load(val & 0x3F); },
            0xFF12 => { self.ch1.envelope.initial_volume = val >> 4; self.ch1.envelope.direction = (val & 8) != 0; self.ch1.envelope.period = val & 7; self.ch1.envelope.dac_enabled = (val & 0xF8) != 0; if !self.ch1.envelope.dac_enabled { self.ch1.enabled = false; } },
            0xFF13 => { self.ch1.freq_reg = (self.ch1.freq_reg & 0xFF00) | (val as u16); },
            0xFF14 => { self.ch1.freq_reg = (self.ch1.freq_reg & 0x00FF) | (((val & 7) as u16) << 8); self.ch1.length_counter.enabled = (val & 0x40) != 0; if (val & 0x80) != 0 { if self.ch1.envelope.dac_enabled { self.ch1.enabled = true; } self.ch1.length_counter.trigger(); self.ch1.envelope.trigger(); self.ch1.sweep.trigger(self.ch1.freq_reg); } },
            0xFF16 => { self.ch2.duty_pattern = val >> 6; self.ch2.length_counter.load(val & 0x3F); },
            0xFF17 => { self.ch2.envelope.initial_volume = val >> 4; self.ch2.envelope.direction = (val & 8) != 0; self.ch2.envelope.period = val & 7; self.ch2.envelope.dac_enabled = (val & 0xF8) != 0; if !self.ch2.envelope.dac_enabled { self.ch2.enabled = false; } },
            0xFF18 => { self.ch2.freq_reg = (self.ch2.freq_reg & 0xFF00) | (val as u16); },
            0xFF19 => { self.ch2.freq_reg = (self.ch2.freq_reg & 0x00FF) | (((val & 7) as u16) << 8); self.ch2.length_counter.enabled = (val & 0x40) != 0; if (val & 0x80) != 0 { if self.ch2.envelope.dac_enabled { self.ch2.enabled = true; } self.ch2.length_counter.trigger(); self.ch2.envelope.trigger(); } },
            0xFF1A => { self.ch3.dac_enabled = (val & 0x80) != 0; if !self.ch3.dac_enabled { self.ch3.enabled = false; } },
            0xFF1B => { self.ch3.length_counter.load(val); },
            0xFF1C => { self.ch3.volume_level = (val >> 5) & 3; },
            0xFF1D => { self.ch3.freq_reg = (self.ch3.freq_reg & 0xFF00) | (val as u16); },
            0xFF1E => { self.ch3.freq_reg = (self.ch3.freq_reg & 0x00FF) | (((val & 7) as u16) << 8); self.ch3.length_counter.enabled = (val & 0x40) != 0; if (val & 0x80) != 0 { if self.ch3.dac_enabled { self.ch3.enabled = true; } self.ch3.length_counter.trigger(); self.ch3.sample_index = 0; } },
            0xFF20 => { self.ch4.length_counter.load(val & 0x3F); },
            0xFF21 => { self.ch4.envelope.initial_volume = val >> 4; self.ch4.envelope.direction = (val & 8) != 0; self.ch4.envelope.period = val & 7; self.ch4.envelope.dac_enabled = (val & 0xF8) != 0; if !self.ch4.envelope.dac_enabled { self.ch4.enabled = false; } },
            0xFF22 => { self.ch4.clock_shift = val >> 4; self.ch4.width_mode = (val & 8) != 0; self.ch4.divisor_code = val & 7; },
            0xFF23 => { self.ch4.length_counter.enabled = (val & 0x40) != 0; if (val & 0x80) != 0 { if self.ch4.envelope.dac_enabled { self.ch4.enabled = true; } self.ch4.length_counter.trigger(); self.ch4.envelope.trigger(); self.ch4.lfsr = 0xFFFF; } },
            0xFF24 => { self.master_vol_right = val & 7; self.master_vol_left = (val >> 4) & 7; },
            0xFF25 => { self.panning = val; },
            0xFF26 => {},
            0xFF30..=0xFF3F => { if !self.ch3.enabled { self.ch3.wave_ram[(addr - 0xFF30) as usize] = val; } },
            _ => (),
        }
    }
}
