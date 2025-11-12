use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{Builder, ProgressBar, Statusbar, Window};

const GLADE_UI: &str = include_str!("gui.glade");

pub fn load_glade_file() -> Result<Builder, String> {
    Ok(Builder::from_string(GLADE_UI))
}

pub fn setup_gui(
    input_level: Arc<Mutex<f32>>,
    output_level: Arc<Mutex<f32>>,
    shutting_down: Arc<AtomicBool>,
    ser: Arc<Mutex<Box<dyn serialport::SerialPort + Send>>>,
    stop_audio: Arc<Mutex<bool>>,
    freq_state: Arc<Mutex<u64>>,
    mode_state: Arc<Mutex<String>>,
    tx_state: Arc<Mutex<bool>>,
) -> Result<(), String> {
    let display = std::env::var("DISPLAY").ok();
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    
    if display.is_none() && wayland.is_none() {
        return Err("Neither DISPLAY nor WAYLAND_DISPLAY environment variables are set. Cannot initialize GUI.".to_string());
    }
    
    gtk::init().map_err(|e| format!("Failed to initialize GTK: {:?}", e))?;
    
    let builder = load_glade_file()?;
    
    let window: Window = builder
        .object("window")
        .ok_or("Could not find window object in glade file")?;
    
    let prog_tx_level: ProgressBar = builder
        .object("prog_tx_level")
        .ok_or("Could not find prog_tx_level progress bar")?;
    
    let prog_rx_level: ProgressBar = builder
        .object("prog_rx_level")
        .ok_or("Could not find prog_rx_level progress bar")?;
    
    let statusbar: Statusbar = builder
        .object("statusbar")
        .ok_or("Could not find statusbar object in glade file")?;
    
    prog_tx_level.set_show_text(true);
    prog_rx_level.set_show_text(true);
    
    window.set_resizable(false);
    
    let shutting_down_clone = shutting_down.clone();
    let stop_audio_clone = stop_audio.clone();
    let ser_clone = ser.clone();
    window.connect_delete_event(move |_, _| {
        crate::shutdown::shutdown(
            shutting_down_clone.clone(),
            ser_clone.clone(),
            stop_audio_clone.clone(),
        );
        gtk::main_quit();
        gtk::glib::Propagation::Stop
    });
    
    window.show_all();
    
    let shutting_down_for_timeout = shutting_down.clone();
    let input_level_for_timeout = input_level.clone();
    let output_level_for_timeout = output_level.clone();
    let freq_state_for_timeout = freq_state.clone();
    let mode_state_for_timeout = mode_state.clone();
    let tx_state_for_timeout = tx_state.clone();
    let prog_tx_for_timeout = prog_tx_level.clone();
    let prog_rx_for_timeout = prog_rx_level.clone();
    let statusbar_for_timeout = statusbar.clone();
    
    let _ = glib::timeout_add_local(Duration::from_millis(50), move || {
        if shutting_down_for_timeout.load(Ordering::Relaxed) {
            return glib::ControlFlow::Break;
        }
        
        let in_lvl = *input_level_for_timeout.lock().unwrap();
        let out_lvl = *output_level_for_timeout.lock().unwrap();
        
        prog_rx_for_timeout.set_fraction(in_lvl.clamp(0.0, 1.0) as f64);
        let rx_text = format!("{:.1}%", in_lvl * 100.0);
        prog_rx_for_timeout.set_text(Some(&rx_text));
        
        prog_tx_for_timeout.set_fraction(out_lvl.clamp(0.0, 1.0) as f64);
        let tx_text = format!("{:.1}%", out_lvl * 100.0);
        prog_tx_for_timeout.set_text(Some(&tx_text));
        
        let freq = *freq_state_for_timeout.lock().unwrap();
        let mode = mode_state_for_timeout.lock().unwrap().clone();
        let tx_now = *tx_state_for_timeout.lock().unwrap();
        let freq_mhz = (freq as f64) / 1_000_000.0f64;
        let rts = if crate::trusdx::last_rts_state() { "H" } else { "L" };
        let status_text = format!("MODE: {} FREQ: {:.5} MHz STATE: {} RTS:{}", 
            mode, freq_mhz, if tx_now { "TX" } else { "RX" }, rts);
        statusbar_for_timeout.pop(0);
        statusbar_for_timeout.push(0, &status_text);
        
        glib::ControlFlow::Continue
    });
    
    gtk::main();
    
    Ok(())
}

pub fn spawn_gui(
    input_level: Arc<Mutex<f32>>,
    output_level: Arc<Mutex<f32>>,
    shutting_down: Arc<AtomicBool>,
    ser: Arc<Mutex<Box<dyn serialport::SerialPort + Send>>>,
    stop_audio: Arc<Mutex<bool>>,
    freq_state: Arc<Mutex<u64>>,
    mode_state: Arc<Mutex<String>>,
    tx_state: Arc<Mutex<bool>>,
) {
    thread::spawn(move || {
        if let Err(e) = setup_gui(
            input_level,
            output_level,
            shutting_down,
            ser,
            stop_audio,
            freq_state,
            mode_state,
            tx_state,
        ) {
            eprintln!("GUI error: {}", e);
        }
    });
}

