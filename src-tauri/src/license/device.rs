use std::path::Path;

use rand::{rngs::OsRng, RngCore};
use tauri::{AppHandle, Manager};

use super::{write_atomic, CmdResult, CommandError};

const DEVICE_DIR: &str = "device";
const DEVICE_FILE: &str = "device_id.bin";

pub fn get_or_init_device_hash(app: &AppHandle) -> CmdResult<[u8; 32]> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| CommandError::io(e.to_string()))?
        .join(DEVICE_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| CommandError::io(e.to_string()))?;
    let file_path = dir.join(DEVICE_FILE);
    if file_path.exists() {
        match std::fs::read(&file_path) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut array = [0u8; 32];
                array.copy_from_slice(&bytes);
                Ok(array)
            }
            Ok(_) => regenerate(&file_path),
            Err(err) => Err(CommandError::io(err.to_string())),
        }
    } else {
        regenerate(&file_path)
    }
}

pub fn get_or_init_device_hash_hex(app: &AppHandle) -> CmdResult<String> {
    let bytes = get_or_init_device_hash(app)?;
    Ok(hex::encode(bytes))
}

fn regenerate(path: &Path) -> CmdResult<[u8; 32]> {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    write_atomic(path, &bytes).map_err(|e| CommandError::io(e.to_string()))?;
    Ok(bytes)
}
