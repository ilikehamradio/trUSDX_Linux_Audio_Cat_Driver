use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use crate::trusdx;
use serialport;


fn handle_rigctl_client(
    mut stream: TcpStream,
    _ser: Arc<Mutex<Box<dyn serialport::SerialPort + Send>>>,
    freq_state: Arc<Mutex<u64>>,
    tx_state: Arc<Mutex<bool>>,
    cat_queue: Arc<Mutex<Vec<Vec<u8>>>>,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        // Check if line read failed (client disconnected)
        if reader.read_line(&mut line).is_err() {
            break;
        }
        // Check if line is empty (end of input)
        if line.is_empty() {
            break;
        }
        let cmd = line.trim();
        // Check if command is empty after trimming
        if cmd.is_empty() {
            continue;
        }
        match cmd.chars().next().unwrap_or('\0') {
            '\\' => {
                let meta = &cmd[1..];
                match meta {
                    "chk_vfo" => {
                        let _ = writeln!(stream, "0");
                    }
                    "get_powerstat" => {
                        let _ = writeln!(stream, "1");
                    }
                    "dump_state" => {
                        let lines = [
                            "0",
                            "0",
                            "0",
                            "0 0 0 0 0 0 0",
                            "0 0 0 0 0 0 0",
                            "0 0",
                            "0 0",
                            "0",
                            "0",
                            "0",
                            "0",
                            "0 0 0 0 0 0 0",
                            "0 0 0 0 0 0 0",
                            "0",
                            "0",
                            "0",
                            "0",
                            "0",
                            "0",
                        ];
                        for l in lines {
                            let _ = writeln!(stream, "{}", l);
                        }
                    }
                    "dump_caps" => {
                        let _ = writeln!(stream, "RPRT 0");
                    }
                    _ => {
                        let _ = writeln!(stream, "RPRT 0");
                    }
                }
            }
            'f' => {
                let hz = *freq_state.lock().unwrap();
                let _ = writeln!(stream, "{}", hz);
            }
            'F' => {
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                // Check if command has frequency parameter
                if parts.len() >= 2 {
                    let parsed_hz: Option<u64> = if let Ok(hz_int) = parts[1].parse::<u64>() {
                        Some(hz_int)
                    } else if let Ok(hz_f) = parts[1].parse::<f64>() {
                        Some(hz_f.round() as u64)
                    } else {
                        None
                    };
                    // Check if frequency parsed successfully
                    if let Some(hz) = parsed_hz {
                        *freq_state.lock().unwrap() = hz;
                        {
                            let mut q = cat_queue.lock().unwrap();
                            q.push(format!("FA{:011};", hz).into_bytes());
                        }
                        let _ = writeln!(stream, "RPRT 0");
                        continue;
                    }
                }
                let _ = writeln!(stream, "RPRT -1");
            }
            'm' => {
                let _ = writeln!(stream, "USB");
                let _ = writeln!(stream, "2400");
            }
            'M' => {
                {
                    let mut q = cat_queue.lock().unwrap();
                    q.push(b"MD2;".to_vec());
                }
                let _ = writeln!(stream, "RPRT 0");
            }
            'v' => {
                let _ = writeln!(stream, "VFOA");
            }
            'V' => {
                let _ = writeln!(stream, "RPRT 0");
            }
            't' => {
                let on = *tx_state.lock().unwrap();
                let _ = writeln!(stream, "{}", if on { 1 } else { 0 });
            }
            'T' => {
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                // Check if command has TX state parameter
                if parts.len() >= 2 {
                    let on = parts[1].parse::<i32>().map(|v| v != 0).unwrap_or(false);
                    // Check if serial port lock acquired successfully
                    if let Ok(mut s) = _ser.lock() {
                        // Check if TX should be enabled
                        if on {
                            let _ = trusdx::start_transmit_baseband(&mut **s);
                        } else {
                            let _ = trusdx::enable_streaming_speaker_off(&mut **s);
                        }
                    }
                    *tx_state.lock().unwrap() = on;
                    let _ = writeln!(stream, "RPRT 0");
                } else {
                    let _ = writeln!(stream, "RPRT 0");
                }
            }
            'q' => {
                let _ = writeln!(stream, "RPRT 0");
                break;
            }
            _ => {
                let _ = writeln!(stream, "RPRT 0");
            }
        }
    }
}

pub fn spawn_rigctl_server(
    ser: Arc<Mutex<Box<dyn serialport::SerialPort + Send>>>,
    freq_state: Arc<Mutex<u64>>,
    tx_state: Arc<Mutex<bool>>,
    cat_queue: Arc<Mutex<Vec<Vec<u8>>>>,
) {
    let _ = std::process::Command::new("pkill")
        .args(["-f", "rigctl"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            let _ = child.try_wait();
            Ok(())
        });
    
    let _ = std::process::Command::new("fuser")
        .args(["-k", "4532/tcp"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            let _ = child.try_wait();
            Ok(())
        });
    
    // Check if lsof command executed successfully
    if let Ok(output) = std::process::Command::new("lsof")
        .args(["-ti:4532"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .output()
    {
        // Check if port 4532 is in use
        if !output.stdout.is_empty() {
            let pid_str = String::from_utf8_lossy(&output.stdout);
            let pid = pid_str.trim();
            let current_pid = std::process::id().to_string();
            // Check if process using port is not current process
            if pid != current_pid {
                let _ = std::process::Command::new("kill")
                    .args(["-9", pid])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .stdin(std::process::Stdio::null())
                    .spawn()
                    .and_then(|mut child| {
                        let _ = child.try_wait();
                        Ok(())
                    });
            }
        }
    }

    std::thread::spawn(move || {
        let addr = ("127.0.0.1", 4532);
        // Check if TCP listener bound successfully
        if let Ok(listener) = TcpListener::bind(addr) {
            for stream in listener.incoming() {
                // Check if client connection accepted successfully
                if let Ok(stream) = stream {
                    handle_rigctl_client(
                        stream,
                        ser.clone(),
                        freq_state.clone(),
                        tx_state.clone(),
                        cat_queue.clone(),
                    );
                }
            }
        } else {
            eprintln!("rigctl: failed to bind 127.0.0.1:4532");
        }
    });
}
