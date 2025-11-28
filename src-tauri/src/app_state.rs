use std::sync::{Arc, Mutex};
use steamworks::networking_sockets::NetConnection;
use steamworks::{Client, LobbyId};
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct AppState {
    pub steam_client: Client,
    pub current_lobby: Arc<Mutex<Option<LobbyId>>>,
    pub is_host: Arc<Mutex<bool>>,
    pub local_game_port: Arc<Mutex<u16>>,
    pub stop_signal: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    pub connections: Arc<Mutex<Vec<NetConnection>>>,
}

impl AppState {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let client = Client::init_app(480)?;

        Ok(Self {
            steam_client: client,
            current_lobby: Arc::new(Mutex::new(None)),
            is_host: Arc::new(Mutex::new(false)),
            local_game_port: Arc::new(Mutex::new(0)),
            stop_signal: Arc::new(Mutex::new(None)),
            connections: Arc::new(Mutex::new(Vec::new())),
        })
    }
}
