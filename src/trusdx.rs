use serialport::SerialPort;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const CMD_RX: &[u8] = b";RX;";
const CMD_TX0: &[u8] = b";TX0;";

const CMD_FA_QUERY: &[u8] = b"FA;";

const CMD_MD0: &[u8] = b"MD0;";
const CMD_MD1_LSB: &[u8] = b"MD1;";
const CMD_MD2_USB: &[u8] = b"MD2;";
const CMD_MD3_CW: &[u8] = b"MD3;";
const CMD_MD4_FM: &[u8] = b"MD4;";
const CMD_MD5_AM: &[u8] = b"MD5;";

const CMD_UA0: &[u8] = b"UA0;";
const CMD_UA1: &[u8] = b"UA1;";
const CMD_UA2: &[u8] = b"UA2;";

static LAST_RTS: OnceLock<AtomicBool> = OnceLock::new();
static LAST_DTR: OnceLock<AtomicBool> = OnceLock::new();

fn set_last_rts(v: bool) { let _ = LAST_RTS.get_or_init(|| AtomicBool::new(v)).store(v, Ordering::Relaxed); }
fn set_last_dtr(v: bool) { let _ = LAST_DTR.get_or_init(|| AtomicBool::new(v)).store(v, Ordering::Relaxed); }
fn get_last_rts() -> bool { LAST_RTS.get_or_init(|| AtomicBool::new(false)).load(Ordering::Relaxed) }
pub fn last_rts_state() -> bool { get_last_rts() }

pub fn find_ch340() -> Option<String> {
    for entry in fs::read_dir("/dev").ok()? {
        let entry = entry.ok()?;
        let fname = entry.file_name().into_string().ok()?;
        if !fname.starts_with("ttyUSB") { continue; }
        let dev_path = PathBuf::from("/sys/class/tty").join(&fname).join("device");
        let dev_real_abs = std::fs::canonicalize(&dev_path).ok()?;
        let usb_device_path = dev_real_abs.parent()?.parent()?;
        let id_vendor_path = usb_device_path.join("idVendor");
        let id_product_path = usb_device_path.join("idProduct");
        if id_vendor_path.exists() && id_product_path.exists() {
            let vid = fs::read_to_string(id_vendor_path).ok()?.trim().to_lowercase();
            let pid = fs::read_to_string(id_product_path).ok()?.trim().to_lowercase();
            if vid == "1a86" && pid == "7523" {
                return Some(format!("/dev/{}", fname));
            }
        }
    }
    None
}

pub fn open_trusdx_serial() -> anyhow::Result<Box<dyn SerialPort + Send>> {
    let serial_device = find_ch340().ok_or_else(|| anyhow::anyhow!("No CH340 device found"))?;
    let port = serialport::new(&serial_device, 115200)
        .timeout(Duration::from_millis(10))
        .parity(serialport::Parity::None)
        .data_bits(serialport::DataBits::Eight)
        .stop_bits(serialport::StopBits::One)
        .flow_control(serialport::FlowControl::None)
        .open()?;
    Ok(port)
}

fn send_command_to_radio(s: &mut dyn SerialPort, data: &[u8]) -> std::io::Result<()> {
    s.write_all(data)
}


pub fn start_transmit_baseband(s: &mut dyn SerialPort) -> std::io::Result<()> { 
    let res = send_command_to_radio(s, CMD_TX0);
    log_line_state(s, "start_transmit_baseband:after_TX0");
    res
}

pub fn query_vfo_a(s: &mut dyn SerialPort) -> std::io::Result<()> { 
    send_command_to_radio(s, CMD_FA_QUERY) 
}

pub fn set_mode(s: &mut dyn SerialPort, mode: u8) -> std::io::Result<()> {
    let buf = match mode {
        0 => CMD_MD0,
        1 => CMD_MD1_LSB,
        2 => CMD_MD2_USB,
        3 => CMD_MD3_CW,
        4 => CMD_MD4_FM,
        5 => CMD_MD5_AM,
        _ => CMD_MD2_USB,
    };
    send_command_to_radio(s, buf)
}

pub fn disable_streaming(s: &mut dyn SerialPort) -> std::io::Result<()> { 
    send_command_to_radio(s, CMD_UA0) 
}

pub fn enable_streaming_speaker_off(s: &mut dyn SerialPort) -> std::io::Result<()> { 
    let _ = control_rts(s,true)?;
    let _ = send_command_to_radio(s, CMD_RX);
    let _ = control_rts(s,false)?;
    
    let _ = s.flush();
    std::thread::sleep(std::time::Duration::from_millis(10));
    
    let _ = control_rts(s,true)?;
    let result = send_command_to_radio(s, CMD_UA2);
    let _ = control_rts(s,false)?;
    result
}

pub fn enable_streaming_speaker_on(s: &mut dyn SerialPort) -> std::io::Result<()> { 
    let _ = control_rts(s,true)?;
    let _ = send_command_to_radio(s, CMD_RX);
    let _ = control_rts(s,false)?;
    
    let _ = s.flush();
    std::thread::sleep(std::time::Duration::from_millis(10));
    
    let _ = control_rts(s,true)?;
    let result = send_command_to_radio(s, CMD_UA1);
    let _ = control_rts(s,false)?;
    result
}

pub fn send_audio_stream(s: &mut dyn SerialPort, audio_data: &[u8]) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(2 + audio_data.len() + 1);
    buf.extend_from_slice(b"US");
    buf.extend_from_slice(audio_data);
    buf.push(b';');
    send_command_to_radio(s, &buf)
}

pub fn send_audio_stream_raw(s: &mut dyn SerialPort, audio_data: &[u8]) -> std::io::Result<()> {
    match s.write(audio_data) {
        Ok(_) => Ok(()),
        Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
    }
}

pub fn control_rts(s: &mut dyn SerialPort, high: bool) -> std::io::Result<()> {
    let r = s.write_request_to_send(high).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    if r.is_ok() { set_last_rts(high); }
    r
}

pub fn control_dtr(s: &mut dyn SerialPort, high: bool) -> std::io::Result<()> {
    let r = s.write_data_terminal_ready(high).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    if r.is_ok() { set_last_dtr(high); }
    r
}

pub fn log_line_state(_s: &mut dyn SerialPort, _context: &str) {
}


