// src/main.rs

use std::env;
use std::time::{Duration, Instant};
use std::fs;
use std::path::Path;
use std::io::{Write, Read}; // ★★★ 変更点: Readを追加 ★★★
use image::{ImageBuffer, Rgba};
use chrono::Local;

use rust_gb_emulator::cartridge::Cartridge;
use rust_gb_emulator::cpu::Cpu;
use rust_gb_emulator::mmu::Mmu;
use rust_gb_emulator::ppu;
use rust_gb_emulator::apu;
use rust_gb_emulator::joypad::GameboyKey;
use rust_gb_emulator::debug_view;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use minifb::{Key, Window, WindowOptions, Scale, ScaleMode, KeyRepeat};

const TARGET_FPS: u64 = 60;
const CYCLES_PER_FRAME: u64 = 4_194_304 / TARGET_FPS;
const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / TARGET_FPS);
const TURBO_MULTIPLIER: u64 = 4;


fn get_save_path(rom_path: &str) -> String {
    let rom_path_obj = Path::new(rom_path);
    rom_path_obj.with_extension("sav").to_string_lossy().to_string()
}

fn save_screenshot(frame_buffer: &[u32], width: usize, height: usize) {
    let mut buffer = ImageBuffer::new(width as u32, height as u32);
    for (y, row) in frame_buffer.chunks_exact(width).enumerate() {
        for (x, pixel_val) in row.iter().enumerate() {
            let r = ((*pixel_val >> 16) & 0xFF) as u8;
            let g = ((*pixel_val >> 8) & 0xFF) as u8;
            let b = (*pixel_val & 0xFF) as u8;
            buffer.put_pixel(x as u32, y as u32, Rgba([r, g, b, 255]));
        }
    }
    
    if fs::create_dir_all("screenshots").is_err() {
        eprintln!("Failed to create screenshots directory.");
        return;
    }
    let timestamp = Local::now().format("%Y%m%d-%H%M%S");
    let path = format!("screenshots/screenshot-{}.png", timestamp);

    match buffer.save(&path) {
        Ok(_) => println!("Screenshot saved to {}", path),
        Err(e) => eprintln!("Failed to save screenshot: {}", e),
    }
}


fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <rom_file_path>", args[0]);
        return Ok(());
    }
    let rom_path = &args[1];

    println!("Loading ROM from: {}", rom_path);
    let cartridge = Cartridge::load(rom_path).expect("Failed to load ROM");
    cartridge.print_header_info();

    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device available");
    let config = device.default_output_config().expect("Failed to get default output config");
    let sample_rate = config.sample_rate().0;
    println!("Audio device sample rate: {} Hz", sample_rate);
    let stream_config = config.into();

    let apu = apu::Apu::new(sample_rate);
    let sample_buffer_handle = apu.get_sample_buffer_handle();
    
    let mut mmu = Mmu::new(cartridge, apu);
    let save_path = get_save_path(rom_path);
    // ★★★ 変更点: セーブデータロード処理をMMUの専用関数に置き換え ★★★
    if mmu.cartridge.has_battery() {
        if let Ok(save_data) = fs::read(&save_path) {
            mmu.load_ram_and_rtc(&save_data);
            println!("Loaded save data from {}", save_path);
        }
    }
    let mut cpu = Cpu::new(mmu);

    let stream = device.build_output_stream(&stream_config, move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
         let mut buffer = sample_buffer_handle.lock().unwrap();
         for frame in data.chunks_mut(2) {
             let (l, r) = buffer.pop_front().unwrap_or((0.0, 0.0));
             frame[0] = l;
             frame[1] = r;
         }
     }, |err| eprintln!("an error occurred on stream: {}", err), None).unwrap();
    stream.play().unwrap();


    let mut game_window = Window::new(
        "Rust Game Boy Emulator",
        ppu::SCREEN_WIDTH,
        ppu::SCREEN_HEIGHT,
        WindowOptions { resize: true, scale: Scale::X4, scale_mode: ScaleMode::AspectRatioStretch, ..WindowOptions::default() }
    ).expect("Failed to create game window");

    let mut debug_window: Option<Window> = None;
    let mut debug_buffer: Vec<u32> = vec![0; debug_view::DEBUG_WIDTH * debug_view::DEBUG_HEIGHT];
    
    let mut is_paused = false;
    let mut frame_skip_enabled = false;
    let mut frame_counter = 0u64;
    
    let mut f2_key_was_pressed = false;

    println!("\n--- Starting Emulation Loop ---");
    println!("================================ Controls ================================");
    println!("  - Gamepad:  Arrow Keys, Z (A), X (B), Enter (Start), Backspace (Select)");
    println!("  - Features: Tab (Turbo), P (Palette), F1 (Pause), F2 (Toggle Debug View)");
    println!("  -           F12 (Screenshot)");
    println!("==========================================================================");
    
    let mut fps = 0.0;
    
    while game_window.is_open() {
        let frame_start_time = Instant::now();
        
        let is_f2_down = game_window.is_key_down(Key::F2);
        if is_f2_down && !f2_key_was_pressed {
            if debug_window.is_some() {
                debug_window = None;
                println!("Debug View: OFF");
            } else {
                debug_window = Some(Window::new("Debug View", debug_view::DEBUG_WIDTH, debug_view::DEBUG_HEIGHT, WindowOptions::default()).unwrap());
                frame_skip_enabled = false; 
                println!("Debug View: ON");
            }
        }
        f2_key_was_pressed = is_f2_down;

        let keys_just_pressed = game_window.get_keys_pressed(KeyRepeat::No);
        if !keys_just_pressed.is_empty() {
            for key in keys_just_pressed {
                match key {
                    Key::F1 => {
                        is_paused = !is_paused;
                        println!("Game {}", if is_paused { "Paused" } else { "Resumed" });
                    },
                    Key::P => cpu.mmu.ppu.cycle_palette(),
                    Key::F12 => save_screenshot(&cpu.mmu.ppu.frame_buffer, ppu::SCREEN_WIDTH, ppu::SCREEN_HEIGHT),
                    _ => (),
                }
            }
        }
        
        if !is_paused {
            let is_turbo = game_window.is_key_down(Key::Tab);

            let keys_down = game_window.get_keys();
            let mut joypad_interrupt_requested = false;
            if keys_down.contains(&Key::Right) { if cpu.mmu.joypad.button_down(GameboyKey::Right) { joypad_interrupt_requested = true; } } else { cpu.mmu.joypad.button_up(GameboyKey::Right); }
            if keys_down.contains(&Key::Left)  { if cpu.mmu.joypad.button_down(GameboyKey::Left)  { joypad_interrupt_requested = true; } } else { cpu.mmu.joypad.button_up(GameboyKey::Left); }
            if keys_down.contains(&Key::Up)    { if cpu.mmu.joypad.button_down(GameboyKey::Up)    { joypad_interrupt_requested = true; } } else { cpu.mmu.joypad.button_up(GameboyKey::Up); }
            if keys_down.contains(&Key::Down)  { if cpu.mmu.joypad.button_down(GameboyKey::Down)  { joypad_interrupt_requested = true; } } else { cpu.mmu.joypad.button_up(GameboyKey::Down); }
            if keys_down.contains(&Key::Z)     { if cpu.mmu.joypad.button_down(GameboyKey::A)      { joypad_interrupt_requested = true; } } else { cpu.mmu.joypad.button_up(GameboyKey::A); }
            if keys_down.contains(&Key::X)     { if cpu.mmu.joypad.button_down(GameboyKey::B)      { joypad_interrupt_requested = true; } } else { cpu.mmu.joypad.button_up(GameboyKey::B); }
            if keys_down.contains(&Key::Backspace) { if cpu.mmu.joypad.button_down(GameboyKey::Select) { joypad_interrupt_requested = true; } } else { cpu.mmu.joypad.button_up(GameboyKey::Select); }
            if keys_down.contains(&Key::Enter) { if cpu.mmu.joypad.button_down(GameboyKey::Start)  { joypad_interrupt_requested = true; } } else { cpu.mmu.joypad.button_up(GameboyKey::Start); }
            if joypad_interrupt_requested { cpu.mmu.request_interrupt(4); }
            
            let mut cycles_this_frame: u64 = 0;
            let target_cycles = if is_turbo { CYCLES_PER_FRAME * TURBO_MULTIPLIER } else { CYCLES_PER_FRAME };
            while cycles_this_frame < target_cycles {
                cycles_this_frame += cpu.step() as u64;
            }
            frame_counter += 1;

            let should_draw_frame = !frame_skip_enabled || frame_counter % 2 == 0;
            if should_draw_frame {
                if let Some(win) = &mut debug_window {
                    if win.is_open() {
                        let apu_state = cpu.mmu.apu.get_apu_state();
                        let waveforms = cpu.mmu.apu.get_channel_waveforms();
                        let ie = cpu.mmu.read_byte(0xFFFF);
                        let iff = cpu.mmu.read_byte(0xFF0F);
                        debug_view::draw(&mut debug_buffer, cpu.registers, cpu.ime, &apu_state, &cpu.mmu.ppu, &cpu.mmu.timer, ie, iff, fps, &waveforms);
                        win.update_with_buffer(&debug_buffer, debug_view::DEBUG_WIDTH, debug_view::DEBUG_HEIGHT).unwrap();
                    } else {
                        debug_window = None;
                    }
                }
                
                if cpu.mmu.ppu.frame_ready {
                    game_window.update_with_buffer(&cpu.mmu.ppu.frame_buffer, ppu::SCREEN_WIDTH, ppu::SCREEN_HEIGHT).unwrap();
                    cpu.mmu.ppu.frame_ready = false;
                } else {
                    game_window.update();
                }

                fps = 1.0 / frame_start_time.elapsed().as_secs_f64();
                let title = if is_paused { "Rust Game Boy Emulator - [PAUSED]".to_string() } else { format!("Rust Game Boy Emulator - FPS: {:.1}", fps) };
                game_window.set_title(&title);
            }
            
            if !is_turbo {
                let elapsed = frame_start_time.elapsed();
                if elapsed < FRAME_DURATION {
                    std::thread::sleep(FRAME_DURATION - elapsed);
                }
            }

        } else {
            game_window.update();
            std::thread::sleep(Duration::from_millis(16));
        }
    }
    
    // ★★★ 変更点: セーブデータ書き出し処理をMMUの専用関数に置き換え ★★★
    if cpu.mmu.cartridge.has_battery() && !cpu.mmu.external_ram.is_empty() {
        let save_data = cpu.mmu.get_ram_and_rtc_data();
        if let Ok(mut file) = fs::File::create(&save_path) {
            if let Err(e) = file.write_all(&save_data) {
                eprintln!("Failed to write save data: {}", e);
            } else {
                println!("Saved data to {}", save_path);
            }
        }
    }
    
    Ok(())
}