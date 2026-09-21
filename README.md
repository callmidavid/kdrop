# 💧 kdrop

Fast, private, local file sharing across Linux, iPhone, Android, and other devices on the same Wi-Fi network.

- **No cloud or accounts:** Direct peer-to-peer over local network.
- **LocalSend compatible:** Uses multicast UDP discovery on `224.0.0.167:53317`.
- **Works with any phone:** Open the local URL or scan the QR code in Safari/Chrome.
- **Desktop GUI:** Built with Rust & egui matching the Kafy OS theme.
- **Bidirectional:** Send and receive between any device with accept/decline approval.

## Usage

### Run with GUI

```bash
cargo run --release
```

### Run Headless (CLI / Server mode)

```bash
cargo run --release -- --headless
```

Prints the ASCII QR code and local web URL directly in your terminal.
