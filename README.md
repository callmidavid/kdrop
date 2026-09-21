<div align="center">

# 💧 kdrop

**An open-source, AirDrop-inspired local file sharing tool with Touch-to-Transfer proximity sensing.**

Share files, photos, videos, and documents seamlessly across **Linux, iPhone, Android, Windows, and macOS** on the same Wi-Fi network. No internet connection, no third-party servers, no accounts, and no cables.

[![Release](https://img.shields.io/github/v/release/kafy-os/kdrop?color=blue&logo=github)](https://github.com/kafy-os/kdrop/releases)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20macOS%20%7C%20iOS%20%7C%20Android-purple)](#downloads)

</div>

---

## ⚡ Touch-to-Transfer (AirDrop-Style Proximity)

kdrop features **hardware-free proximity sensing** using Bluetooth Low Energy (BLE) signal strength (RSSI).

- **Bring to Bump:** Place your iPhone or Android phone right beside your laptop (< 20 cm).
- **Instant Detection:** kdrop senses the radio signal spike (`> -44 dBm`) using an Exponential Moving Average (EMA) filter.
- **1-Click Drop:** The app immediately illuminates with a glowing **⚡ Device Bump** banner:
  > _"⚡ Device Bump: 'iPhone 15' is right beside laptop! [⚡ Drop Files]"_
- Click **Drop Files**, and your transfer begins instantly without having to browse device lists or scan codes.

---

## 📥 Downloads

Pre-built binaries are ready for instant download on the [**GitHub Releases**](https://github.com/kafy-os/kdrop/releases) page.

| Platform                              | Format                                                                   | Description                                                                 |
| ------------------------------------- | ------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| **Linux (Debian / Ubuntu / Kafy OS)** | [`.deb Package`](https://github.com/kafy-os/kdrop/releases/latest)       | Installs system-wide with application launcher and desktop icon.            |
| **Linux (Universal)**                 | [`.tar.gz Standalone`](https://github.com/kafy-os/kdrop/releases/latest) | Portable x86_64 binary. Extract and double-click to run.                    |
| **Windows**                           | [`.zip (kdrop.exe)`](https://github.com/kafy-os/kdrop/releases/latest)   | Portable Windows executable for Windows 10 & 11.                            |
| **macOS**                             | [`.tar.gz`](https://github.com/kafy-os/kdrop/releases/latest)            | Universal binary for Apple Silicon (M1/M2/M3) and Intel Macs.               |
| **iOS (iPhone / iPad)**               | **No App Required**                                                      | Open Safari and scan the QR code or navigate to `http://<laptop-ip>:53317`. |
| **Android**                           | **No App Required**                                                      | Open Chrome and scan the QR code or navigate to `http://<laptop-ip>:53317`. |

### Linux Quick Install via Terminal (.deb):

```bash
sudo dpkg -i kdrop_*_amd64.deb
```

---

## 🔍 How It Works

kdrop uses a decentralized, peer-to-peer architecture inspired by LocalSend and AirDrop:

```
                  ┌─────────────────────────────────────────┐
                  │          PROXIMITY & DISCOVERY          │
                  │  • Bluetooth LE RSSI (< 20 cm Bump)     │
                  │  • Multicast UDP Beacon (224.0.0.167)   │
                  └────────────────────┬────────────────────┘
                                       │
                                       ▼
┌───────────────────┐        1. Transfer Request       ┌───────────────────┐
│                   │ ───────────────────────────────▶ │                   │
│   Sender Device   │                                  │  Receiver Device  │
│   (Linux Laptop)  │ ◀─────────────────────────────── │  (Phone / Laptop) │
│                   │     2. Accept / Decline Modal    │                   │
│                   │                                  └───────────────────┘
│                   │        3. Binary Data Stream               │
│                   │ ───────────────────────────────────────────▶
│                   │       (Direct local HTTP socket)           │ Saved to
└───────────────────┘                                            │ ~/Downloads/kdrop/
```

### 1. Discovery Phase

- **Wi-Fi Multicast:** Devices broadcast lightweight JSON announcement packets via UDP multicast on `224.0.0.167:53317`. This makes kdrop **wire-compatible with LocalSend** — devices running either app discover each other automatically on the LAN.
- **BLE Proximity Sensor:** On Linux, kdrop passively monitors Bluetooth Low Energy advertising packets via BlueZ. As a phone moves from across the room (`-75 dBm`) to touching the laptop (`-38 dBm`), kdrop classifies its distance zone in real time.

### 2. Session Handshake Phase

- When a sender clicks **Send Files** or triggers the **Bump** gesture, it sends a `POST /api/send/request` with file metadata (names, types, byte sizes).
- The receiving device displays a confirmation modal:
  > _"King's PC wants to send 3 photos (12.4 MB). [Decline] [Accept]"_

### 3. Transfer Phase

- Once accepted, files stream directly over a raw TCP connection using chunked HTTP streaming (`POST /api/receive/:sessionId/:fileId`).
- No data ever touches the cloud or an external server. Files transfer at the maximum speed of your local Wi-Fi router (typically 30–80 MB/s).

---

## 📱 Using kdrop on iPhone & Android (Zero Installation)

You don't need to install anything from the App Store or Google Play:

1. Launch **kdrop** on your PC.
2. Connect your phone to the same Wi-Fi network.
3. Open your phone camera and **scan the QR code** on your laptop screen (or type the URL shown in the app).
4. **To receive files:** Simply accept incoming transfers when prompted.
5. **To send files to your PC:** Tap **Choose Files** or drag and drop photos/videos directly in your mobile browser. Progress bars track the upload in real time.

---

## 🌐 Network Requirements

- **Local Network:** Both devices must be connected to the same Wi-Fi router or mobile hotspot.
- **Port 53317:** Ensure firewall rules permit TCP & UDP traffic on port `53317`.
- **AP / Client Isolation:** In university, hotel, or public guest Wi-Fi networks, routers often turn on _Client Isolation_ (blocking devices from seeing each other). If this happens, connect both devices to a portable Wi-Fi hotspot.

---

## 🛠️ Building from Source (Developers)

If you'd like to build kdrop yourself:

```bash
# Clone the repository
git clone https://github.com/kafy-os/kdrop.git
cd kdrop

# Run native GUI
cargo run --release

# Run in headless server mode (prints QR code directly in terminal)
cargo run --release -- --headless
```

---

## 📄 License

kdrop is open-source under the [MIT License](LICENSE).
