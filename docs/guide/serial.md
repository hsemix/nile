# Serial ports: `@nile/serial`

COM ports, USB-serial adapters and Bluetooth serial, for scales, card readers, barcode scanners, receipt printers and lab
gear. It is part of the Nile binary (the `serial` cargo feature, on by default in `nile-engine`; the `serialport` crate
built without libudev, so Linux needs no extra packages).

```js
import serial from "@nile/serial";

const ports = await serial.list();   // [{ path, type: "usb"|"bluetooth"|"pci"|"unknown", vendorId, productId, manufacturer, product, serialNumber }]
const port = await serial.open("COM3", { baudRate: 9600 });   // "/dev/ttyUSB0", "/dev/cu.usbserial-1410" ...

port.onData((bytes) => console.log(new TextDecoder().decode(bytes)));   // Uint8Array chunks as they arrive
await port.write("W\r\n");           // string (UTF-8), Uint8Array, ArrayBuffer or array of bytes

const reading = await port.readUntil("\n", { timeout: 2000 });   // through the delimiter
const four = await port.read(4, { timeout: 2000 });              // exactly four bytes
await port.close();
```

- **Options:** `baudRate` (9600), `dataBits` 5-8 (8), `stopBits` 1|2 (1), `parity` `"none"|"even"|"odd"`, `flowControl`
  `"none"|"hardware"|"software"`, `timeoutMs` (write timeout, 1000).
- **Receiving:** with a listener set (`onData`) chunks go to it. With none, bytes wait in a 1 MiB buffer (the oldest are dropped)
  for `read(length)` / `readUntil(delimiter)`; both take `{ timeout }` in ms and reject with `code: "SERIAL_TIMEOUT"`.
  `port.discard()` empties the buffer. Protocol code that sends a command and waits for a line uses `write` + `readUntil`.
- **Signals:** `port.setSignals({ dataTerminalReady, requestToSend, break })`, `port.getSignals()` gives
  `{ clearToSend, dataSetReady, ringIndicator, carrierDetect, bytesToRead, bytesToWrite }`.
- **Unplugging:** `port.onClose((message) => ...)` runs when the device goes away; pending reads reject.
- **Errors** carry a `code`: `ENOENT` (no such port), `EACCES` (no permission), `ENODEV`, `EINVAL` (bad option), `EIO`, plus
  `SERIAL_CLOSED` and `SERIAL_TIMEOUT`. On Linux the user must be in the `dialout` (or `uucp`) group to open `/dev/ttyUSB0`;
  Windows ports above COM9 are written `\\.\COM10` (the crate adds that for you).
- Everything runs on worker threads, so a slow device never freezes the window. A port belongs to the program until `close()`
  (or exit).

`examples/serial.html` is a small terminal: pick a port, open it, send text, toggle DTR.

**Checked:** the macOS device nodes (list, open, signals, close, a missing port's error). The data path is tested on Linux
CI against a pseudo-terminal pair (macOS refuses to open a pty slave by name), and nothing here has talked to real serial
hardware or run on Windows yet. Not included: USB/HID devices (a separate, heavier dependency) and Bluetooth discovery.
