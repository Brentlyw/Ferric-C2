# Ferric-C2

**Ferric-C2** is a simple Rust/Python-based Command and Control (C2) framework designed for ease of use. It is still under active development and lacks some advanced features until future updates.

## ⚠️ Disclaimer

**Ferric-C2** is intended **solely for educational and research purposes**. Unauthorized use of this software to conduct malicious activities is strictly prohibited. The author and contributors are not responsible for any misuse or damage caused by this tool. Use it responsibly and ethically, *please*.

## 🚧 Project Status

**Pre-Alpha**: Ferric-C2 is in its early stages of development. While core features are functional, the framework may undergo significant changes. Contributions and feedback are welcome! :-)  
**Detections**: https://kleenscan.com/scan_result/95909b36b6536d5631df35f23deceb9e0b4a563e3a653aba44814d3b40e89c96

## 🛠 Features

- **Asynchronous Connection Handling / Multiple Clients Supported**
  - Efficiently manages multiple client connections simultaneously using asynchronous paradigms.

- **Clean Flask Hosted WebUI**
  - User-friendly interactive web interface built with Flask for easy interaction and management (no more console-based C2!).

- **Remote Shell Command**
  - Execute any PowerShell command on connected clients remotely, enabling comprehensive control.

- **Remote File Manager (Interactive)**
  - **Download, Upload, Rename, Execute, Delete**: Manage files on remote systems through an interactive file browser interface within the WebUI.

- **Gevent Manager (Server-Sided Handling)**
  - Utilizes Gevent for effective server-side connection handling, ensuring scalability and performance.

## 📦 Installation

### Prerequisites

- **Rust**: Ensure you have Rust installed.
- **Python 3**: Required for the server hosting.
- **Gevent**: Install using pip:
- **Flask**: Install using pip:

  ```bash
  pip install gevent, flask
