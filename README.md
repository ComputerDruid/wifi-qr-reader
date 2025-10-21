Example:

```
❯ cargo run --release
   Compiling wifi-qr-reader v0.1.0 (/home/cdruid/src/wifi-qr-reader)
    Finished `release` profile [optimized] target(s) in 10.72s
     Running `target/release/wifi-qr-reader`
Captured Warmup frame 0 2048000
Captured Warmup frame 1 2048000
Captured Warmup frame 2 2048000
[2025-10-21T16:56:21Z INFO  wifi_qr_reader::qrcode] [1] rqrr found code "WIFI:S:Not a real network;T:WPA;P:password;H:false;;"
[2025-10-21T16:56:21Z INFO  wifi_qr_reader::qrcode] (qr code)


To connect, run:
  nmcli device wifi connect 'Not a real network' password password
```

If having trouble getting the QR code to scan, try using `RUST_LOG=wifi_qr_reader=debug` and a sixel enabled terminal to see preview images every 10th frame.

Current status:
- Produces an `nmcli` command you can run to connect to the wifi network (on Linux)
- Has worked at least 4 different webcams
- Panics when it sees a wifi QR code it doesn't understand. Since it prints the text format first, you can probably still figure it out by hand, but error reports with (redacted) WIFI URIs would help.
