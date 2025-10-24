use std::io::Read;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::os::unix::io::AsRawFd;

use serialport;

pub fn spawn_esc_handler(
    shutting_down: Arc<AtomicBool>,
    ser: Arc<Mutex<Box<dyn serialport::SerialPort + Send>>>,
    stop_audio: Arc<Mutex<bool>>,
) {
    thread::spawn(move || {
        use termios::*;
        let tty = std::fs::OpenOptions::new().read(true).open("/dev/tty").or_else(|_| std::fs::OpenOptions::new().read(true).open("/dev/stdin"));
        if let Ok(mut tty_file) = tty {
            let fd = tty_file.as_raw_fd();
            if let Ok(mut term) = Termios::from_fd(fd) {
                let orig = term.clone();
                term.c_lflag &= !(ICANON | ECHO);
                term.c_cc[VMIN] = 1;
                term.c_cc[VTIME] = 0;
                let _ = tcsetattr(fd, TCSANOW, &term);
                let mut buf = [0u8; 1];
                loop {
                    if tty_file.read(&mut buf).ok() == Some(1) {
                        if buf[0] == 0x1B {
                            shutting_down.store(true, Ordering::Relaxed);
                            *stop_audio.lock().unwrap() = true;
                            if let Ok(mut s) = ser.lock() { 
                                let _ = crate::trusdx::enable_streaming_speaker_on(&mut **s);
                            }
                            break;
                        }
                    }
                }
                let _ = tcsetattr(fd, TCSANOW, &orig);
            }
        }
    });
}

pub fn print_console_header() {
    
    print!("\x1B[2J\x1B[H");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    
    println!("");
    println!("");
    println!("");
}

pub fn render_levels(
    input_level: f32,
    output_level: f32,
    freq_hz: u64,
    mode: &str,
    tx_now: bool,
) {
    fn bar(level: f32) -> String {
        let width = 50usize;
        let filled = ((level.clamp(0.0, 1.0)) * width as f32) as usize;
        let empty = width - filled;
        format!("[{}{}]", "#".repeat(filled), "-".repeat(empty))
    }
    
    print!("\x1B[3F");
    print!("\x1B[2K\r");
    println!("INPUT  {} {:5.1}%", bar(input_level), input_level*100.0);
    print!("\x1B[2K\r");
    println!("OUTPUT {} {:5.1}%", bar(output_level), output_level*100.0);
    print!("\x1B[2K\r");
    let freq_mhz = (freq_hz as f64) / 1_000_000.0f64;
    let rts = if crate::trusdx::last_rts_state() { "H" } else { "L" };
    println!("MODE: {} FREQ: {:.5} MHz STATE: {} RTS:{}", mode, freq_mhz, if tx_now { "TX" } else { "RX" }, rts);
    std::io::Write::flush(&mut std::io::stdout()).ok();
}


