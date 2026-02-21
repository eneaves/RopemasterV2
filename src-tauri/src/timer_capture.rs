use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serialport::{SerialPort, SerialPortInfo};
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// Representa un evento de tiempo capturado del Timer Polaris
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerEvent {
    /// Tiempo en segundos (parseado del formato variable)
    pub time_seconds: f64,
    /// Texto raw recibido del timer
    pub raw_text: String,
    /// Timestamp de captura
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Gestor de la conexión con el Timer Polaris
pub struct PolarisTimerCapture {
    port: Arc<Mutex<Option<Box<dyn SerialPort>>>>,
    event_sender: Arc<Mutex<Option<mpsc::UnboundedSender<TimerEvent>>>>,
}

impl PolarisTimerCapture {
    pub fn new() -> Self {
        Self {
            port: Arc::new(Mutex::new(None)),
            event_sender: Arc::new(Mutex::new(None)),
        }
    }

    /// Lista todos los puertos seriales disponibles
    pub fn list_ports() -> Result<Vec<SerialPortInfo>> {
        serialport::available_ports().context("Failed to list serial ports")
    }

    /// Conecta al puerto serial especificado (1200 baud, 8N1)
    pub fn connect(&self, port_name: &str) -> Result<()> {
        let port = serialport::new(port_name, 1200)
            .timeout(Duration::from_millis(100))
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .open()
            .context("Failed to open serial port")?;

        let mut port_lock = self.port.lock().unwrap();
        *port_lock = Some(port);

        Ok(())
    }

    /// Desconecta del puerto serial
    pub fn disconnect(&self) {
        let mut port_lock = self.port.lock().unwrap();
        *port_lock = None;

        let mut sender_lock = self.event_sender.lock().unwrap();
        *sender_lock = None;
    }

    /// Verifica si está conectado
    pub fn is_connected(&self) -> bool {
        self.port.lock().unwrap().is_some()
    }

    /// Inicia la captura de eventos en un thread separado
    pub fn start_capture(&self) -> Result<mpsc::UnboundedReceiver<TimerEvent>> {
        let port_lock = self.port.lock().unwrap();
        if port_lock.is_none() {
            anyhow::bail!("Serial port not connected");
        }

        // Clonar el port para usar en el thread
        let port_clone = port_lock
            .as_ref()
            .unwrap()
            .try_clone()
            .context("Failed to clone serial port")?;
        drop(port_lock);

        let (tx, rx) = mpsc::unbounded_channel();

        // Guardar sender para poder cerrarlo
        let mut sender_lock = self.event_sender.lock().unwrap();
        *sender_lock = Some(tx.clone());
        drop(sender_lock);

        // Spawn thread para leer datos
        std::thread::spawn(move || {
            let mut reader = BufReader::new(port_clone);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        // Remover CR/LF
                        let trimmed = line.trim();

                        // Ignorar líneas vacías
                        if trimmed.is_empty() {
                            continue;
                        }

                        // Buscar líneas que contengan tiempos
                        if let Some(event) = parse_timer_line(trimmed) {
                            if tx.send(event).is_err() {
                                break; // Channel cerrado
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        continue; // Timeout normal, continuar esperando
                    }
                    Err(e) => {
                        tracing::error!("Error reading serial port: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

/// Parsea una línea del timer buscando tiempos válidos
///
/// Formatos soportados:
/// - 103.474
/// - 103.47
/// - 1:43.474
/// - 1:43.47
///
/// Los tiempos están precedidos por 2 caracteres de control (double-high/wide)
fn parse_timer_line(line: &str) -> Option<TimerEvent> {
    // Remover caracteres de control (bytes < 32 excepto espacio)
    let cleaned: String = line.chars().filter(|c| *c >= ' ' || *c == '\t').collect();

    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Buscar patrones de tiempo
    // Patrón 1: MM:SS.mmm o MM:SS.mm
    if let Some(time_sec) = parse_minutes_seconds(trimmed) {
        return Some(TimerEvent {
            time_seconds: time_sec,
            raw_text: line.to_string(),
            timestamp: chrono::Utc::now(),
        });
    }

    // Patrón 2: SS.mmm o SS.mm (solo segundos)
    if let Some(time_sec) = parse_seconds_only(trimmed) {
        return Some(TimerEvent {
            time_seconds: time_sec,
            raw_text: line.to_string(),
            timestamp: chrono::Utc::now(),
        });
    }

    None
}

/// Parsea formato MM:SS.mmm
fn parse_minutes_seconds(text: &str) -> Option<f64> {
    // Regex simple: dígitos:dígitos.dígitos
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() != 2 {
        return None;
    }

    let minutes = parts[0].trim().parse::<f64>().ok()?;
    let seconds = parts[1].trim().parse::<f64>().ok()?;

    Some(minutes * 60.0 + seconds)
}

/// Parsea formato SS.mmm (solo segundos con decimales)
fn parse_seconds_only(text: &str) -> Option<f64> {
    // Buscar el primer número con punto decimal
    let mut number_str = String::new();
    let mut found_digit = false;

    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            number_str.push(ch);
            found_digit = true;
        } else if found_digit && !number_str.is_empty() {
            // Terminó el número
            break;
        }
    }

    if number_str.is_empty() || !number_str.contains('.') {
        return None;
    }

    number_str.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_equal(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn test_parse_seconds_only() {
        let val = parse_seconds_only("103.474").unwrap();
        assert!(approx_equal(val, 103.474, 1e-6));
        let val = parse_seconds_only("103.47").unwrap();
        assert!(approx_equal(val, 103.47, 1e-6));
        let val = parse_seconds_only("  103.47  ").unwrap();
        assert!(approx_equal(val, 103.47, 1e-6));
        let val = parse_seconds_only("Time: 103.47").unwrap();
        assert!(approx_equal(val, 103.47, 1e-6));
    }

    #[test]
    fn test_parse_minutes_seconds() {
        let val = parse_minutes_seconds("1:43.474").unwrap();
        assert!(approx_equal(val, 103.474, 1e-6));
        let val = parse_minutes_seconds("1:43.47").unwrap();
        assert!(approx_equal(val, 103.47, 1e-6));
        let val = parse_minutes_seconds("2:30.00").unwrap();
        assert!(approx_equal(val, 150.0, 1e-6));
    }

    #[test]
    fn test_parse_timer_line() {
        let event = parse_timer_line("103.474");
        assert!(event.is_some());
        assert!(approx_equal(event.unwrap().time_seconds, 103.474, 1e-6));

        let event = parse_timer_line("1:43.47");
        assert!(event.is_some());
        assert!(approx_equal(event.unwrap().time_seconds, 103.47, 1e-6));
    }
}
