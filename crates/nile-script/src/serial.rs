//! `@nile/serial`: serial ports (COM ports, USB-serial adapters, Bluetooth serial) through the `serialport` crate, built
//! without libudev so Linux needs no extra packages. One entry point, `call(op, args)`; every op may block (opening
//! waits on the driver, a read waits for bytes), so `sys::start` runs them on worker threads when the op is prefixed
//! `serial:`.
//!
//! Ops: `list` -> JSON array; `open(json options)` -> id; `write(id, base64)` -> bytes written;
//! `read(id, max, wait_ms)` -> "D" + base64, "T" (nothing came within `wait_ms`; 0 waits for ever) or "E" (closed);
//! `signals(id, json)`; `status(id)` -> JSON; `close(id)`. Reads use their own handle of the port, so a write is never
//! stuck behind a waiting read, and a read ends within 50 ms of `close`.

#[cfg(feature = "serial")]
mod ports {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use serde_json::{json, Value};
    use serialport::{DataBits, FlowControl, Parity, SerialPort, SerialPortType, StopBits};
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    struct Port {
        writer: Mutex<Box<dyn SerialPort>>,
        reader: Mutex<Box<dyn SerialPort>>,
    }

    fn table() -> &'static Mutex<HashMap<u32, Arc<Port>>> {
        static T: OnceLock<Mutex<HashMap<u32, Arc<Port>>>> = OnceLock::new();
        T.get_or_init(Default::default)
    }

    fn get(id: &str) -> Result<Arc<Port>, String> {
        id.parse::<u32>().ok().and_then(|i| table().lock().unwrap().get(&i).cloned()).ok_or_else(|| "EBADF: the serial port is closed".to_string())
    }

    fn message(e: &serialport::Error) -> String {
        use serialport::ErrorKind::*;
        let code = match e.kind {
            NoDevice => "ENODEV",
            InvalidInput => "EINVAL",
            Io(std::io::ErrorKind::PermissionDenied) => "EACCES",
            Io(std::io::ErrorKind::NotFound) => "ENOENT",
            _ => "EIO",
        };
        format!("{code}: {}", e.description)
    }

    fn list() -> Result<String, String> {
        let found = serialport::available_ports().map_err(|e| message(&e))?;
        let rows: Vec<Value> = found
            .into_iter()
            .map(|p| match p.port_type {
                SerialPortType::UsbPort(u) => json!({
                    "path": p.port_name, "type": "usb", "vendorId": u.vid, "productId": u.pid,
                    "manufacturer": u.manufacturer, "product": u.product, "serialNumber": u.serial_number,
                }),
                SerialPortType::BluetoothPort => json!({ "path": p.port_name, "type": "bluetooth" }),
                SerialPortType::PciPort => json!({ "path": p.port_name, "type": "pci" }),
                SerialPortType::Unknown => json!({ "path": p.port_name, "type": "unknown" }),
            })
            .collect();
        Ok(Value::Array(rows).to_string())
    }

    fn open(options: &str) -> Result<String, String> {
        let o: Value = serde_json::from_str(options).map_err(|_| "EINVAL: bad options".to_string())?;
        let path = o["path"].as_str().filter(|p| !p.is_empty()).ok_or("EINVAL: a port path is required")?;
        let baud = o["baudRate"].as_u64().unwrap_or(9600);
        if baud == 0 || baud > u32::MAX as u64 {
            return Err("EINVAL: baudRate out of range".into());
        }
        let data_bits = match o["dataBits"].as_u64().unwrap_or(8) {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            8 => DataBits::Eight,
            _ => return Err("EINVAL: dataBits must be 5, 6, 7 or 8".into()),
        };
        let stop_bits = match o["stopBits"].as_u64().unwrap_or(1) {
            1 => StopBits::One,
            2 => StopBits::Two,
            _ => return Err("EINVAL: stopBits must be 1 or 2".into()),
        };
        let parity = match o["parity"].as_str().unwrap_or("none") {
            "none" => Parity::None,
            "even" => Parity::Even,
            "odd" => Parity::Odd,
            _ => return Err("EINVAL: parity must be none, even or odd".into()),
        };
        let flow = match o["flowControl"].as_str().unwrap_or("none") {
            "none" => FlowControl::None,
            "hardware" => FlowControl::Hardware,
            "software" => FlowControl::Software,
            _ => return Err("EINVAL: flowControl must be none, hardware or software".into()),
        };
        let timeout = o["timeoutMs"].as_u64().unwrap_or(1000).min(60_000);
        let writer = serialport::new(path, baud as u32)
            .data_bits(data_bits)
            .stop_bits(stop_bits)
            .parity(parity)
            .flow_control(flow)
            .timeout(Duration::from_millis(timeout))
            .open()
            .map_err(|e| message(&e))?;
        let mut reader = writer.try_clone().map_err(|e| message(&e))?;
        let _ = reader.set_timeout(Duration::from_millis(50));
        static NEXT: AtomicU32 = AtomicU32::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        table().lock().unwrap().insert(id, Arc::new(Port { writer: Mutex::new(writer), reader: Mutex::new(reader) }));
        Ok(id.to_string())
    }

    fn read(id: &str, max: usize, wait_ms: u64) -> Result<String, String> {
        let started = Instant::now();
        let mut buf = vec![0u8; max.clamp(1, 1 << 20)];
        loop {
            // The table's reference is the port's life: a closed port ends the wait within one poll.
            let Ok(port) = get(id) else { return Ok("E".into()) };
            let result = port.reader.lock().unwrap().read(&mut buf);
            match result {
                Ok(0) => {}
                Ok(n) => return Ok(format!("D{}", B64.encode(&buf[..n]))),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {}
                Err(e) => return Err(format!("EIO: {e}")),
            }
            if wait_ms > 0 && started.elapsed() >= Duration::from_millis(wait_ms) {
                return Ok("T".into());
            }
        }
    }

    pub fn call(op: &str, a: &[String]) -> Result<String, String> {
        let arg = |i: usize| a.get(i).map_or("", String::as_str);
        match op {
            "list" => list(),
            "open" => open(arg(0)),
            "write" => {
                let data = B64.decode(arg(1)).map_err(|_| "EINVAL: bad data".to_string())?;
                let port = get(arg(0))?;
                let mut w = port.writer.lock().unwrap();
                w.write_all(&data).and_then(|()| w.flush()).map_err(|e| format!("EIO: {e}"))?;
                Ok(data.len().to_string())
            }
            "read" => read(arg(0), arg(1).parse().unwrap_or(16384), arg(2).parse().unwrap_or(0)),
            "signals" => {
                let o: Value = serde_json::from_str(arg(1)).unwrap_or(Value::Null);
                let port = get(arg(0))?;
                let mut w = port.writer.lock().unwrap();
                if let Some(v) = o["dataTerminalReady"].as_bool() {
                    w.write_data_terminal_ready(v).map_err(|e| message(&e))?;
                }
                if let Some(v) = o["requestToSend"].as_bool() {
                    w.write_request_to_send(v).map_err(|e| message(&e))?;
                }
                match o["break"].as_bool() {
                    Some(true) => w.set_break().map_err(|e| message(&e))?,
                    Some(false) => w.clear_break().map_err(|e| message(&e))?,
                    None => {}
                }
                Ok(String::new())
            }
            "status" => {
                let port = get(arg(0))?;
                let mut w = port.writer.lock().unwrap();
                let m = |e: serialport::Error| message(&e);
                Ok(json!({
                    "clearToSend": w.read_clear_to_send().map_err(m)?, "dataSetReady": w.read_data_set_ready().map_err(m)?,
                    "ringIndicator": w.read_ring_indicator().map_err(m)?, "carrierDetect": w.read_carrier_detect().map_err(m)?,
                    "bytesToRead": w.bytes_to_read().map_err(m)?, "bytesToWrite": w.bytes_to_write().map_err(m)?,
                })
                .to_string())
            }
            "close" => {
                let id: u32 = arg(0).parse().map_err(|_| "EBADF: bad port".to_string())?;
                table().lock().unwrap().remove(&id);
                Ok(String::new())
            }
            _ => Err(format!("unknown serial op: {op}")),
        }
    }
}

#[cfg(feature = "serial")]
pub(crate) use ports::call;

#[cfg(not(feature = "serial"))]
pub(crate) fn call(_op: &str, _a: &[String]) -> Result<String, String> {
    Err("ENOSYS: serial ports are not built in".into())
}
