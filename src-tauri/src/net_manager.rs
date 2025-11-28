use crate::app_state::AppState;
use bytes::{Buf, BufMut, BytesMut};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use steamworks::{
    networking_sockets::ListenSocket,
    networking_types::{ListenSocketEvent, SendFlags},
    SteamId,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

const ID_LEN: usize = 7;
const HEADER_SIZE: usize = ID_LEN + 4;

struct TunnelPacket {
    client_id: String,
    msg_type: u32,
    payload: Vec<u8>,
}

impl TunnelPacket {
    fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(HEADER_SIZE + self.payload.len());
        let id_bytes = self.client_id.as_bytes();
        let len = std::cmp::min(id_bytes.len(), 6);
        buf.put_slice(&id_bytes[0..len]);
        for _ in len..ID_LEN {
            buf.put_u8(0);
        }
        buf.put_u32_le(self.msg_type);
        if self.msg_type == 0 {
            buf.put_slice(&self.payload);
        }
        buf.to_vec()
    }

    fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_SIZE {
            return None;
        }
        let id_slice = &data[0..6];
        let client_id = String::from_utf8_lossy(id_slice)
            .trim_matches(char::from(0))
            .to_string();
        let mut slice = &data[7..];
        if slice.len() < 4 {
            return None;
        }
        let msg_type = slice.get_u32_le();
        let payload = slice.to_vec();
        Some(TunnelPacket {
            client_id,
            msg_type,
            payload,
        })
    }
}

// --- 公共函数 ---
pub fn stop_network(state: &AppState) {
    let mut signal = state.stop_signal.lock().unwrap();
    if let Some(sender) = signal.take() {
        let _ = sender.send(());
        log::info!("Network stop signal sent");
    }
    *state.is_host.lock().unwrap() = false;
    state.connections.lock().unwrap().clear();
}

pub async fn start_network_client(
    state: &AppState,
    host_id: SteamId,
    local_port: u16,
) -> Result<(), String> {
    stop_network(state);
    *state.local_game_port.lock().unwrap() = local_port;

    let networking = state.steam_client.networking_sockets();
    let net_identity = steamworks::networking_types::NetworkingIdentity::new_steam_id(host_id);

    match networking.connect_p2p(net_identity, 0, None) {
        Ok(conn) => {
            log::info!("P2P Connect initiated to Host: {:?}", host_id);
            state.connections.lock().unwrap().push(conn);
            let (tx_stop, rx_stop) = oneshot::channel();
            *state.stop_signal.lock().unwrap() = Some(tx_stop);
            let state_clone = state.clone();
            tauri::async_runtime::spawn(async move {
                client_loop(state_clone, rx_stop).await;
            });
            Ok(())
        }
        Err(_) => Err("Failed to connect P2P".to_string()),
    }
}

pub fn start_network_host(state: &AppState) -> Result<(), String> {
    stop_network(state);
    let networking = state.steam_client.networking_sockets();

    match networking.create_listen_socket_p2p(0, None) {
        Ok(socket) => {
            log::info!("Started Hosting P2P Listen Socket");
            *state.is_host.lock().unwrap() = true;
            let (tx_stop, rx_stop) = oneshot::channel();
            *state.stop_signal.lock().unwrap() = Some(tx_stop);
            let state_clone = state.clone();
            tauri::async_runtime::spawn(async move {
                host_loop(state_clone, socket, rx_stop).await;
            });
            Ok(())
        }
        Err(e) => Err(format!("Failed to start hosting: {:?}", e)),
    }
}

// --- Host 逻辑 ---
struct HostOutgoingPacket {
    target_steam_id: SteamId,
    packet: TunnelPacket,
}

async fn host_loop(
    state: AppState,
    listen_socket: ListenSocket,
    mut rx_stop: oneshot::Receiver<()>,
) {
    type TcpSender = mpsc::UnboundedSender<Vec<u8>>;
    let socket_map: Arc<Mutex<HashMap<String, TcpSender>>> = Arc::new(Mutex::new(HashMap::new()));
    let (tx_p2p, mut rx_p2p) = mpsc::unbounded_channel::<HostOutgoingPacket>();
    let local_port = *state.local_game_port.lock().unwrap();

    loop {
        while let Some(event) = listen_socket.try_receive_event() {
            match event {
                ListenSocketEvent::Connecting(request) => {
                    let _ = request.accept();
                }
                ListenSocketEvent::Connected(connected) => {
                    log::info!("Client connected via ListenSocket");
                    state
                        .connections
                        .lock()
                        .unwrap()
                        .push(connected.take_connection());
                }
                ListenSocketEvent::Disconnected(_disconnected) => {
                    log::info!("Client disconnected from ListenSocket, will be cleaned up during next poll.");
                }
            }
        }

        tokio::select! {
            Some(out) = rx_p2p.recv() => {
                let data = out.packet.to_bytes();
                let mut conns_guard = state.connections.lock().unwrap();
                let sockets = state.steam_client.networking_sockets();
                if let Some(conn) = conns_guard.iter_mut().find(|c| {
                    sockets.get_connection_info(c).ok()
                        .and_then(|info| info.identity_remote().and_then(|id| id.steam_id())) == Some(out.target_steam_id)
                }) {
                    let _ = conn.send_message(&data, SendFlags::RELIABLE);
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                let mut conns_guard = state.connections.lock().unwrap();
                let mut dead_indices = Vec::new();
                let sockets = state.steam_client.networking_sockets();

                for (i, conn) in conns_guard.iter_mut().enumerate() {
                    match conn.receive_messages(10) {
                        Ok(messages) => {
                            for msg in messages {
                                if let Ok(info) = sockets.get_connection_info(conn) {
                                    if let (Some(packet), Some(remote_id)) = (TunnelPacket::from_bytes(msg.data()), info.identity_remote().and_then(|id| id.steam_id())) {
                                        let client_id = packet.client_id.clone();
                                        let mut map_guard = socket_map.lock().unwrap();
                                        if !map_guard.contains_key(&client_id) && packet.msg_type == 0 {
                                            spawn_local_bridge_host(client_id.clone(), local_port, remote_id, tx_p2p.clone(), &mut map_guard);
                                        }
                                        if let Some(sender) = map_guard.get(&client_id) {
                                            let _ = sender.send(packet.payload);
                                        }
                                        if packet.msg_type == 1 {
                                            map_guard.remove(&client_id);
                                        }
                                    }
                                }
                            }
                        },
                        Err(_) => { dead_indices.push(i); }
                    }
                }
                for i in dead_indices.iter().rev() { conns_guard.remove(*i); }
            }
            _ = &mut rx_stop => { break; }
        }
    }
}

fn spawn_local_bridge_host(
    client_id: String,
    local_port: u16,
    target_steam_id: SteamId,
    tx_p2p: mpsc::UnboundedSender<HostOutgoingPacket>,
    map: &mut HashMap<String, mpsc::UnboundedSender<Vec<u8>>>,
) {
    let (tx_tcp, mut rx_tcp) = mpsc::unbounded_channel::<Vec<u8>>();
    map.insert(client_id.clone(), tx_tcp);

    tauri::async_runtime::spawn(async move {
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", local_port)).await {
            let (mut rd, mut wr) = stream.into_split();
            let id_clone_read = client_id.clone();
            let tx_p2p_read = tx_p2p.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];
                while let Ok(n) = rd.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let p = TunnelPacket {
                        client_id: id_clone_read.clone(),
                        msg_type: 0,
                        payload: buf[0..n].to_vec(),
                    };
                    if tx_p2p_read
                        .send(HostOutgoingPacket {
                            target_steam_id,
                            packet: p,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                let p_disconnect = TunnelPacket {
                    client_id: id_clone_read,
                    msg_type: 1,
                    payload: vec![],
                };
                let _ = tx_p2p_read.send(HostOutgoingPacket {
                    target_steam_id,
                    packet: p_disconnect,
                });
            });
            while let Some(data) = rx_tcp.recv().await {
                if wr.write_all(&data).await.is_err() {
                    break;
                }
            }
        }
    });
}

// --- Client 逻辑 ---
async fn client_loop(state: AppState, mut rx_stop: oneshot::Receiver<()>) {
    let local_port = *state.local_game_port.lock().unwrap();
    let listener = match TcpListener::bind(("127.0.0.1", local_port)).await {
        Ok(l) => l,
        Err(_) => return,
    };
    let socket_map: Arc<Mutex<HashMap<String, mpsc::Sender<Vec<u8>>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let (tx_p2p, mut rx_p2p) = mpsc::channel::<TunnelPacket>(100);

    loop {
        tokio::select! {
            Ok((socket, _)) = listener.accept() => {
                let id = nanoid::nanoid!(6);
                let (mut rd, mut wr) = socket.into_split();
                let (tx_socket, mut rx_socket) = mpsc::channel::<Vec<u8>>(100);
                socket_map.lock().unwrap().insert(id.clone(), tx_socket);

                let tx_p2p_clone = tx_p2p.clone();
                let id_clone_read = id.clone();
                let map_clone_read = socket_map.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 2048];
                    while let Ok(n) = rd.read(&mut buf).await {
                        if n == 0 { break; }
                        let packet = TunnelPacket { client_id: id_clone_read.clone(), msg_type: 0, payload: buf[0..n].to_vec() };
                        if tx_p2p_clone.send(packet).await.is_err() { break; }
                    }
                    let packet = TunnelPacket { client_id: id_clone_read.clone(), msg_type: 1, payload: vec![] };
                    let _ = tx_p2p_clone.send(packet).await;
                    map_clone_read.lock().unwrap().remove(&id_clone_read);
                });

                tokio::spawn(async move {
                    while let Some(data) = rx_socket.recv().await {
                        if wr.write_all(&data).await.is_err() { break; }
                    }
                });
            }
            Some(packet) = rx_p2p.recv() => {
                let data = packet.to_bytes();
                let mut conns = state.connections.lock().unwrap();
                if let Some(conn) = conns.iter_mut().next() {
                    let _ = conn.send_message(&data, SendFlags::RELIABLE);
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                let mut conns = state.connections.lock().unwrap();
                if let Some(conn) = conns.iter_mut().next() {
                    if let Ok(messages) = conn.receive_messages(10) {
                        for msg in messages {
                            if let Some(packet) = TunnelPacket::from_bytes(msg.data()) {
                               let map = socket_map.lock().unwrap();
                               if let Some(sender) = map.get(&packet.client_id) {
                                   let _ = sender.try_send(packet.payload);
                               }
                            }
                        }
                    } else {
                        break;
                    }
                } else if !(*state.is_host.lock().unwrap()) {
                    break;
                }
            }
            _ = &mut rx_stop => { break; }
        }
    }
}
