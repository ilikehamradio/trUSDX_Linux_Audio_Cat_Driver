use std::process::Command;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::Duration;
use std::io::Write;

use libpulse_binding as pulse;
use libpulse_binding::def::BufferAttr;
use libpulse_simple_binding as psimple;
use serialport;

pub fn cleanup_trusdx_audio() {
    if let Ok(output) = Command::new("pactl").arg("list").arg("short").arg("modules").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("TRUSDX") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(module_id) = parts.get(0) {
                    let _ = Command::new("pactl").arg("unload-module").arg(module_id).status();
                }
            }
        }
    }
}

pub fn create_trusdx_audio_interface(_audio_tx_rate: u32) -> Option<u32> {
    let sink_output = Command::new("pactl")
        .args([
            "load-module", 
            "module-null-sink", 
            "sink_name=TRUSDX", 
            "sink_properties=device.description=\"TRUSDX Audio\""
        ]) 
        .output();
    
    let sink_module_id = match sink_output {
        Ok(result) if result.status.success() => {
            let output_str = String::from_utf8_lossy(&result.stdout);
            output_str.trim().parse::<u32>().ok()
        },
        Ok(_result) => {
            None
        },
        Err(_e) => {
            None
        }
    };
    sink_module_id
}

#[derive(Clone)]
pub struct AudioHandles {
    pub pa_playback: Arc<psimple::Simple>,
    pub pa_record: Arc<psimple::Simple>,
}

pub fn setup_pulseaudio(audio_rx_rate: u32, audio_tx_rate: u32) -> anyhow::Result<AudioHandles> {
    let spec_rx = pulse::sample::Spec { format: pulse::sample::Format::F32le, channels: 1, rate: audio_rx_rate };
    let spec_tx = pulse::sample::Spec { format: pulse::sample::Format::S16le, channels: 1, rate: audio_tx_rate };

    let pb_attr = BufferAttr {
        maxlength: (audio_rx_rate / 4) * std::mem::size_of_val(&0f32) as u32,
        tlength: (audio_rx_rate / 50) * std::mem::size_of_val(&0f32) as u32,
        prebuf: (audio_rx_rate / 100) * std::mem::size_of_val(&0f32) as u32,
        minreq: (audio_rx_rate / 200) * std::mem::size_of_val(&0f32) as u32,
        fragsize: (audio_rx_rate / 100) * std::mem::size_of_val(&0f32) as u32,
    };

    let pa_playback = psimple::Simple::new(
        None,
        "trusdxAudio",
        pulse::stream::Direction::Playback,
        Some("TRUSDX"),
        "Radio RX Audio",
        &spec_rx,
        None,
        Some(&pb_attr),
    )?;

    
    let rec_attr = BufferAttr {
        maxlength: (audio_tx_rate / 4) * std::mem::size_of_val(&0i16) as u32,
        tlength: (audio_tx_rate / 50) * std::mem::size_of_val(&0i16) as u32,
        prebuf: (audio_tx_rate / 100) * std::mem::size_of_val(&0i16) as u32,
        minreq: (audio_tx_rate / 200) * std::mem::size_of_val(&0i16) as u32,
        fragsize: (audio_tx_rate / 100) * std::mem::size_of_val(&0i16) as u32,
    };

    let pa_record = psimple::Simple::new(
        None,
        "trusdxAudio",
        pulse::stream::Direction::Record,
        Some("TRUSDX.monitor"),
        "Radio TX Audio",
        &spec_tx,
        None,
        Some(&rec_attr),
    )?;

    Ok(AudioHandles { pa_playback: Arc::new(pa_playback), pa_record: Arc::new(pa_record) })
}

pub fn run_audio_bridge(
    ser: Arc<Mutex<Box<dyn serialport::SerialPort + Send>>>,
    audio: AudioHandles,
    stop_flag: Arc<Mutex<bool>>,
    input_level: Arc<Mutex<f32>>,
    output_level: Arc<Mutex<f32>>,
    freq_state: Arc<Mutex<u64>>,
    mode_state: Arc<Mutex<String>>,
    tx_state: Arc<Mutex<bool>>,
    cat_queue: Arc<Mutex<Vec<Vec<u8>>>>,
    streaming_started: Arc<AtomicBool>,
) {
    
    thread::spawn(move || {
        
        let mut state_rx_streaming = false;
        let mut text_buf: Vec<u8> = Vec::with_capacity(1024);
        let mut wave_buf: Vec<u8> = Vec::with_capacity(8192);
        let mut rx_tmp = [0u8; 512];
        let mut f32_buf: Vec<f32> = Vec::with_capacity(1024);
        
        let _audio_processing_buf = vec![0f32; 1024];
        
        
        let mut tx_i16_buf = vec![0i16; 48];
        let mut tx_byte_buf = vec![0u8; tx_i16_buf.len() * 2];
        let mut u8_buf = vec![0u8; 48];  
        
        
        let drain_cat = || {
            let mut writes: Vec<Vec<u8>> = Vec::new();
            {
                let mut guard = cat_queue.lock().unwrap();
                if !guard.is_empty() {
                    writes.extend(guard.drain(..));
                }
            }
            if !writes.is_empty() {
                if let Ok(mut s) = ser.lock() {
                    for w in writes { 
                        let _ = s.write_all(&w); 
                    }
                    let _ = crate::trusdx::flush_serial_line(&mut **s);
                }
            }
        };
        
        let mut last_idle_drain = std::time::Instant::now();
        let mut _sent_diag_us = false;
        let mut _tx_start_time = std::time::Instant::now();
        let mut _audio_frame_count = 0u32;
        
        let mut prev_tx = false;
        loop {
            if *stop_flag.lock().unwrap() { break; }
            
            let is_tx = *tx_state.lock().unwrap();
            let tx_rising = is_tx && !prev_tx;
            let tx_falling = !is_tx && prev_tx;
            
            if tx_rising {
                _tx_start_time = std::time::Instant::now();
                _audio_frame_count = 0;
                
                thread::sleep(Duration::from_millis(10));
                
                let mut drain_buf = vec![0u8; 1024];
                for _ in 0..10 {
                    match audio.pa_record.read(&mut drain_buf) {
                        Ok(_) => {
                        },
                        Err(_) => break,
                    }
                }
            }
            
            if tx_falling {
                state_rx_streaming = false;
                wave_buf.clear();
                text_buf.clear();
                streaming_started.store(false, Ordering::Relaxed);
                
                thread::sleep(Duration::from_millis(30));
                if let Ok(mut s) = ser.lock() {
                    let _ = crate::trusdx::enable_streaming_speaker_off(&mut **s);
                }
                
                let start = std::time::Instant::now();
                while !streaming_started.load(Ordering::Relaxed) && start.elapsed() < Duration::from_millis(200) {
                    thread::sleep(Duration::from_millis(5));
                }
                
                if !streaming_started.load(Ordering::Relaxed) {
                    if let Ok(mut s) = ser.lock() {
                        let _ = crate::trusdx::enable_streaming_speaker_off(&mut **s);
                    }
                    let start = std::time::Instant::now();
                    while !streaming_started.load(Ordering::Relaxed) && start.elapsed() < Duration::from_millis(100) {
                        thread::sleep(Duration::from_millis(5));
                    }
                }
            }
            
            prev_tx = is_tx;
            
            if is_tx {
                if !cat_queue.lock().unwrap().is_empty() {
                    drain_cat();
                }
                
                *input_level.lock().unwrap() = 0.0;

                match audio.pa_record.read(&mut tx_byte_buf) {
                    Ok(_) => {
                        _audio_frame_count += 1;
                        
                        for i in 0..tx_i16_buf.len() {
                            let lo = tx_byte_buf[i*2] as u16;
                            let hi = tx_byte_buf[i*2+1] as u16;
                            let word = (hi << 8) | lo;
                            tx_i16_buf[i] = word as i16;
                        }
                        let mut sum_sq = 0.0f32;
                        for &v in &tx_i16_buf { let f = (v as f32) / 32768.0; sum_sq += f * f; }
                        let rms = (sum_sq / (tx_i16_buf.len() as f32)).sqrt().min(1.0);
                        {
                            let mut lvl = output_level.lock().unwrap();
                            *lvl = rms;
                        }
                        
                        if rms < 0.05 {
                            continue;
                        }
                        
                        const TX_GAIN: f32 = 1.0;  
                        for (i, &x) in tx_i16_buf.iter().enumerate() {
                            let scaled = ((x as f32) * TX_GAIN).clamp(-32768.0, 32767.0) as i16;
                            let byte = 128i16 + (scaled / 256); 
                            u8_buf[i] = byte.clamp(0, 255) as u8;
                        }
                        
                        for b in &mut u8_buf { if *b == b';' { *b = b':'; } }
                        if let Ok(mut s) = ser.lock() { 
                            let _ = crate::trusdx::send_audio_stream_raw(&mut **s, &u8_buf);
                        }
                    }
                    Err(_e) => {
                        continue;
                    }
                }
            } else {
                _sent_diag_us = false;
                
                *output_level.lock().unwrap() = 0.0;
                
                let n = { 
                    if let Ok(mut s) = ser.lock() { 
                        s.read(&mut rx_tmp).unwrap_or(0) 
                    } else {
                        0
                    }
                };
                
                if n == 0 {
                    if last_idle_drain.elapsed() >= Duration::from_millis(50) {
                        if !cat_queue.lock().unwrap().is_empty() { 
                            drain_cat(); 
                        }
                        last_idle_drain = std::time::Instant::now();
                    }
                    
                    {
                        let mut lvl = input_level.lock().unwrap();
                        *lvl *= 0.85f32;
                    }
                    continue;
                }
                
                let mut i = 0usize;
                while i < n {
                    let b = rx_tmp[i];
                    i += 1;
                    
                    if state_rx_streaming {
                        
                        if b == b';' {
                            if !wave_buf.is_empty() {
                                
                                let mut peak = 0.0f32;
                                for &samp in &wave_buf {
                                    let f = ((samp as f32) - 128.0) / 128.0; 
                                    peak = peak.max(f.abs());
                                }
                                *input_level.lock().unwrap() = (peak * 2.1).min(1.0);
                                
                                f32_buf.clear();
                                for &samp in &wave_buf {
                                    f32_buf.push(((samp as f32) - 128.0) / 128.0);
                                }
                                let bytes = unsafe { 
                                    std::slice::from_raw_parts(
                                        f32_buf.as_ptr() as *const u8, 
                                        f32_buf.len() * std::mem::size_of::<f32>()
                                    ) 
                                };
                                let _ = audio.pa_playback.write(bytes);
                            }
                            wave_buf.clear();
                            state_rx_streaming = false;
                            
                            drain_cat();
                        } else {
                            wave_buf.push(b);
                            if wave_buf.len() >= 512 {
                                
                                let mut peak = 0.0f32;
                                for &samp in &wave_buf {
                                    let f = ((samp as f32) - 128.0) / 128.0;
                                    peak = peak.max(f.abs());
                                }
                                *input_level.lock().unwrap() = (peak * 2.1).min(1.0);
                                
                                f32_buf.clear();
                                for &samp in &wave_buf { 
                                    f32_buf.push(((samp as f32) - 128.0) / 128.0); 
                                }
                                let bytes = unsafe { 
                                    std::slice::from_raw_parts(
                                        f32_buf.as_ptr() as *const u8, 
                                        f32_buf.len() * std::mem::size_of::<f32>()
                                    ) 
                                };
                                let _ = audio.pa_playback.write(bytes);
                                wave_buf.clear();
                            }
                        }
                        continue;
                    }
                    
                    
                    text_buf.push(b);
                    if text_buf.len() == 2 && text_buf[0] == b'U' && text_buf[1] == b'S' {
                        text_buf.clear();
                        state_rx_streaming = true;
                        streaming_started.store(true, Ordering::Relaxed);
                        continue;
                    }
                    
                    if b == b';' {
                        if !text_buf.is_empty() {
                            
                            if text_buf.len() >= 2 && text_buf[0] == b'F' && text_buf[1] == b'A' {
                                if let Ok(s) = std::str::from_utf8(&text_buf) {
                                    let digits: String = s.chars().skip(2).take_while(|c| c.is_ascii_digit()).collect();
                                    if let Ok(v) = digits.parse::<u64>() { 
                                        *freq_state.lock().unwrap() = v; 
                                    }
                                }
                            }
                            if text_buf.len() >= 2 && text_buf[0] == b'M' && text_buf[1] == b'D' {
                                
                                let mode_str = match text_buf.get(2).copied().unwrap_or(b'2') {
                                    b'1' => "LSB",
                                    b'2' => "USB",
                                    b'3' => "CW",
                                    b'4' => "FM",
                                    b'5' => "AM",
                                    _ => "USB",
                                };
                                *mode_state.lock().unwrap() = mode_str.to_string();
                            }
                        }
                        text_buf.clear();
                        
                        if !cat_queue.lock().unwrap().is_empty() { 
                            drain_cat(); 
                        }
                    }
                }
            }
        }
    });

}


