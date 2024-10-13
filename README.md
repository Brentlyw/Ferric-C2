# Ferric-C2

**Ferric-C2** is a simple Rust/Python-based Command and Control (C2) framework designed for ease of use. It is still under active development and lacks some advanced features until future updates.

## Disclaimer

**Ferric-C2** is intended **solely for educational and research purposes**. Unauthorized use of this software to conduct malicious activities is strictly prohibited. The author and contributors are not responsible for any misuse or damage caused by this tool. Use it responsibly and ethically, *please*.

## Project Status

**Pre-Alpha**: Ferric-C2 is in its early stages of development. While core features are functional, the framework may undergo significant changes. Contributions and feedback are welcome! :-)  
**Detections**: https://kleenscan.com/scan_result/95909b36b6536d5631df35f23deceb9e0b4a563e3a653aba44814d3b40e89c96

## Features

- **Asynchronous Connection Handling / Multiple Clients Supported**
  - Efficiently manages multiple client connections simultaneously using asynchronous paradigms.
  - Grabs basic identifiers from clients: IP, Subsystem, GUID.
  - Last Seen Tracking & Online Status

- **Clean Flask Hosted WebUI**
  - User-friendly interactive web interface built with Flask for easy interaction and management (no more console-based C2 interfaces!)
  - All CSS/JS hosted in one html file.

- **Remote Shell Command**
  - Execute any PowerShell command on connected clients remotely, enabling comprehensive control.
  - Supports multi-line responses, does not support sending multi-line commands.
- **Remote screen capture**
  - Remotely capture and display the screen (2 FPS max).
    - This is to be futher optimized using compression, and more effecient transfers.
- **Remote File Manager (Interactive)**
  - **Download, Upload, Rename, Execute, Delete**: Manage files on remote clients through an interactive file browser interface within the WebUI.

- **Gevent Manager (Server-Sided Handling)**
  - Utilizes Gevent for effective server-side connection handling, offering scalability and performance.

