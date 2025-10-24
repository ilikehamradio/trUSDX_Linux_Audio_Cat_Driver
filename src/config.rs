use std::sync::atomic::AtomicBool;

pub static _LOG_ENABLED: AtomicBool = AtomicBool::new(true);

pub static DIAG_US_AT_TX_START: AtomicBool = AtomicBool::new(false);

pub static DIAG_US_FRAME_TX: AtomicBool = AtomicBool::new(false);


