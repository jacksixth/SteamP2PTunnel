use crate::app_state::AppState;
use bytes::{Buf, BufMut, BytesMut};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use steamworks::{
    networking_sockets::{ListenSocket, NetConnection},
    networking_types::{ListenSocketEvent, SendFlags},
    SteamId,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

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

unsafe fn clone_net_conn(conn: &NetConnection) -> NetConnection {
    std::ptr::read(conn)
}

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
    let connection = networking.connect_p2p(net_identity, 0, None);

    if let Ok(conn) = connection {
        log::info!("P2P Connect initiated to Host: {:?}", host_id);

        let conn_for_list = unsafe { clone_net_conn(&conn) };
        state.connections.lock().unwrap().push(conn_for_list);

        let (tx_stop, rx_stop) = tokio::sync::oneshot::channel();
        *state.stop_signal.lock().unwrap() = Some(tx_stop);
        let state_client = state.steam_client.clone();

        tokio::spawn(async move {
            client_loop(state_client, conn, rx_stop, local_port).await;
        });
        Ok(())
    } else {
        Err("Failed to connect P2P".to_string())
    }
}

pub fn start_network_host(state: &AppState) -> Result<(), String> {
    stop_network(state);
    let networking = state.steam_client.networking_sockets();

    match networking.create_listen_socket_p2p(0, None) {
        Ok(socket) => {
            log::info!("Started Hosting P2P Listen Socket");
            *state.is_host.lock().unwrap() = true;

            let (tx_stop, rx_stop) = tokio::sync::oneshot::channel();
            *state.stop_signal.lock().unwrap() = Some(tx_stop);

            let state_client = state.steam_client.clone();
            let local_port = *state.local_game_port.lock().unwrap();
            let connections = state.connections.clone();

            tokio::spawn(async move {
                host_loop(state_client, socket, local_port, rx_stop, connections).await;
            });

            Ok(())
        }
        Err(e) => Err(format!("Failed to start hosting: {:?}", e)),
    }
}

struct HostOutgoingPacket {
    target_conn: NetConnection,
    packet: TunnelPacket,
}

async fn host_loop(
    steam_client: steamworks::Client,
    listen_socket: ListenSocket,
    local_port: u16,
    mut rx_stop: tokio::sync::oneshot::Receiver<()>,
    connections: Arc<Mutex<Vec<NetConnection>>>,
) {
    let _networking = steam_client.networking_sockets();
    type TcpSender = mpsc::UnboundedSender<Vec<u8>>;
    let socket_map: Arc<Mutex<HashMap<String, TcpSender>>> = Arc::new(Mutex::new(HashMap::new()));
    let (tx_p2p, mut rx_p2p) = mpsc::unbounded_channel::<HostOutgoingPacket>();

    log::info!(
        "Host Loop Started. Forwarding to local port: {}",
        local_port
    );

    loop {
        let sleep = tokio::time::sleep(std::time::Duration::from_millis(1));

        while let Some(event) = listen_socket.try_receive_event() {
            match event {
                ListenSocketEvent::Connecting(request) => {
                    log::info!("Incoming connection request");
                    let _ = request.accept();
                }
                ListenSocketEvent::Connected(connected) => {
                    log::info!("Client connected via ListenSocket");
                    let conn = connected.take_connection();
                    connections.lock().unwrap().push(conn);
                }
                ListenSocketEvent::Disconnected(_) => {
                    log::info!("Client disconnected");
                }
            }
        }

        tokio::select! {
            Some(out) = rx_p2p.recv() => {
                let data = out.packet.to_bytes();
                if let Err(e) = out.target_conn.send_message(&data, SendFlags::RELIABLE) {
                    log::error!("Failed to send P2P message: {:?}", e);
                }
            }
            _ = sleep => {
                let mut conns = connections.lock().unwrap();
                let mut dead_indices = Vec::new();

                for (i, conn) in conns.iter_mut().enumerate() {
                    match conn.receive_messages(10) {
                        Ok(messages) => {
                            for msg in messages {
                                let data = msg.data();
                                if let Some(packet) = TunnelPacket::from_bytes(data) {
                                    let client_id = packet.client_id.clone();

                                    if packet.msg_type == 1 {
                                        let mut map = socket_map.lock().unwrap();
                                        map.remove(&client_id);
                                        continue;
                                    }

                                    let mut map = socket_map.lock().unwrap();
                                    if let Some(sender) = map.get(&client_id) {
                                        let _ = sender.send(packet.payload);
                                    } else {
                                        log::info!("New TCP Client {} -> Local:{}", client_id, local_port);
                                        let conn_clone = unsafe { clone_net_conn(conn) };
                                        spawn_local_bridge(client_id.clone(), local_port, conn_clone, tx_p2p.clone(), &mut map);
                                    }
                                }
                            }
                        },
                        Err(_) => {
                            dead_indices.push(i);
                        }
                    }
                }

                for i in dead_indices.iter().rev() {
                    conns.remove(*i);
                }
            }
            _ = &mut rx_stop => { break; }
        }
    }
}

fn spawn_local_bridge(
    client_id: String,
    local_port: u16,
    conn_clone: NetConnection,
    tx_p2p: mpsc::UnboundedSender<HostOutgoingPacket>,
    map: &mut HashMap<String, mpsc::UnboundedSender<Vec<u8>>>,
) {
    let (tx_tcp, mut rx_tcp) = mpsc::unbounded_channel::<Vec<u8>>();
    map.insert(client_id.clone(), tx_tcp);

    tokio::spawn(async move {
        match TcpStream::connect(("127.0.0.1", local_port)).await {
            Ok(stream) => {
                let (mut rd, mut wr) = stream.into_split();
                let id_clone = client_id.clone();
                let conn_clone_2 = unsafe { clone_net_conn(&conn_clone) };
                let tx_p2p_2 = tx_p2p.clone();

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 1024];
                    loop {
                        match rd.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => {
                                let p = TunnelPacket {
                                    client_id: id_clone.clone(),
                                    msg_type: 0,
                                    payload: buf[0..n].to_vec(),
                                };
                                if tx_p2p_2
                                    .send(HostOutgoingPacket {
                                        target_conn: unsafe { clone_net_conn(&conn_clone_2) },
                                        packet: p,
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    let _ = tx_p2p_2.send(HostOutgoingPacket {
                        target_conn: unsafe { clone_net_conn(&conn_clone_2) },
                        packet: TunnelPacket {
                            client_id: id_clone,
                            msg_type: 1,
                            payload: vec![],
                        },
                    });
                });

                while let Some(data) = rx_tcp.recv().await {
                    if wr.write_all(&data).await.is_err() {
                        break;
                    }
                }
            }
            Err(e) => {
                log::error!(
                    "Failed to connect to local game server {}: {}",
                    local_port,
                    e
                );
            }
        }
    });
}

async fn client_loop(
    steam_client: steamworks::Client,
    conn: NetConnection,
    mut rx_stop: tokio::sync::oneshot::Receiver<()>,
    local_port: u16,
) {
    let _networking = steam_client.networking_sockets();
    let listener = match TcpListener::bind(("127.0.0.1", local_port)).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("Bind local port {} failed: {}", local_port, e);
            return;
        }
    };

    type SocketMap = Arc<Mutex<HashMap<String, mpsc::Sender<Vec<u8>>>>>;
    let socket_map: SocketMap = Arc::new(Mutex::new(HashMap::new()));
    let (tx_p2p, mut rx_p2p) = mpsc::channel::<TunnelPacket>(100);
    let socket_map_reader = socket_map.clone();

    loop {
        tokio::select! {
            Ok((socket, _)) = listener.accept() => {
                let id = nanoid::nanoid!(6);
                let (mut rd, mut wr) = socket.into_split();
                let (tx_socket, mut rx_socket) = mpsc::channel::<Vec<u8>>(100);
                {
                    let mut map = socket_map.lock().unwrap();
                    map.insert(id.clone(), tx_socket);
                }
                let tx_p2p_clone = tx_p2p.clone();
                let id_clone = id.clone();
                let map_clone = socket_map.clone();

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 1024];
                    loop {
                        match rd.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => {
                                let packet = TunnelPacket {
                                    client_id: id_clone.clone(),
                                    msg_type: 0,
                                    payload: buf[0..n].to_vec(),
                                };
                                if tx_p2p_clone.send(packet).await.is_err() { break; }
                            }
                            Err(_) => break,
                        }
                    }
                    let packet = TunnelPacket { client_id: id_clone.clone(), msg_type: 1, payload: vec![] };
                    let _ = tx_p2p_clone.send(packet).await;
                    {
                        let mut map = map_clone.lock().unwrap();
                        map.remove(&id_clone);
                    }
                });

                tokio::spawn(async move {
                    while let Some(data) = rx_socket.recv().await {
                        if wr.write_all(&data).await.is_err() { break; }
                    }
                });
            }
            Some(packet) = rx_p2p.recv() => {
                let data = packet.to_bytes();
                if let Err(e) = conn.send_message(&data, SendFlags::RELIABLE) {
                    log::error!("Send client p2p error: {:?}", e);
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(1)) => {
               let mut conn_clone = unsafe { clone_net_conn(&conn) };
               if let Ok(messages) = conn_clone.receive_messages(10) {
                   for msg in messages {
                       let data = msg.data();
                       if let Some(packet) = TunnelPacket::from_bytes(data) {
                           if packet.msg_type == 0 {
                                let map = socket_map_reader.lock().unwrap();
                                if let Some(sender) = map.get(&packet.client_id) {
                                    let _ = sender.try_send(packet.payload);
                                }
                           } else if packet.msg_type == 1 {
                               let mut map = socket_map_reader.lock().unwrap();
                               map.remove(&packet.client_id);
                           }
                       }
                   }
               }
            }
            _ = &mut rx_stop => { break; }
        }
    }
}
