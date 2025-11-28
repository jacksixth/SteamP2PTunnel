use crate::app_state::AppState;
use crate::net_manager;
use serde::Serialize;
use steamworks::{FriendFlags, LobbyId, LobbyType, SteamError, SteamId};
use steamworks_sys;
use tauri::State;
use tokio::sync::oneshot;
use std::mem;

#[derive(Serialize, Clone)]
pub struct FriendInfo {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Clone)]
pub struct LobbyInfo {
    pub id: String,
    pub name: String,
    pub member_count: usize,
    pub max_members: usize,
}

#[derive(Serialize)]
pub struct JoinLobbyResult {
    lobby_id: String,
    host_id: String,
}

#[derive(Serialize, Clone)]
pub struct NetworkStatusInfo {
    #[serde(rename = "isHost")]
    is_host: bool,
    #[serde(rename = "isConnected")]
    is_connected: bool,
    #[serde(rename = "tcpClientCount")]
    tcp_client_count: usize,
    #[serde(rename = "statusMessage")]
    status_message: String,
    ping: i32,
}

#[derive(Serialize, Clone)]
pub struct MemberInfo {
    pub id: String,
    pub name: String,
    pub ping: i32,
    pub relay: String,
}

#[tauri::command]
pub fn get_friends(state: State<'_, AppState>) -> Vec<FriendInfo> {
    let friends = state.steam_client.friends();
    let list = friends.get_friends(FriendFlags::IMMEDIATE);
    list.into_iter()
        .map(|f| FriendInfo {
            id: f.id().raw().to_string(),
            name: f.name(),
        })
        .collect()
}

#[tauri::command]
pub async fn create_lobby(state: State<'_, AppState>) -> Result<String, String> {
    let (tx, rx) = oneshot::channel();
    {
        let matchmaking = state.steam_client.matchmaking();
        matchmaking.create_lobby(
            LobbyType::Public,
            4,
            move |result: Result<LobbyId, SteamError>| {
                let _ = tx.send(result);
            },
        );
    }
    let lobby_id = rx
        .await
        .map_err(|_| "Canceled".to_string())?
        .map_err(|e| format!("Failed to create lobby: {:?}", e))?;
    {
        let mut current_lobby = state.current_lobby.lock().unwrap();
        *current_lobby = Some(lobby_id);
    }
    let friends = state.steam_client.friends();
    friends.set_rich_presence("steam_display", Some("#Status_InLobby"));
    friends.set_rich_presence("connect", Some(&lobby_id.raw().to_string()));
    Ok(lobby_id.raw().to_string())
}

#[tauri::command]
pub async fn search_lobbies(state: State<'_, AppState>) -> Result<Vec<LobbyInfo>, String> {
    let (tx, rx) = oneshot::channel();
    {
        let matchmaking = state.steam_client.matchmaking();
        matchmaking.request_lobby_list(move |lobbies: Result<Vec<LobbyId>, SteamError>| {
            let _ = tx.send(lobbies);
        });
    }
    let lobbies = rx
        .await
        .map_err(|_| "Canceled".to_string())?
        .map_err(|e| format!("Failed to search: {:?}", e))?;
    let matchmaking = state.steam_client.matchmaking();
    let mut result = Vec::new();
    for lobby_id in lobbies {
        let member_count = matchmaking.lobby_member_count(lobby_id);
        let member_limit = matchmaking.lobby_member_limit(lobby_id).unwrap_or(4);
        let name = format!("Lobby {}", lobby_id.raw());
        result.push(LobbyInfo {
            id: lobby_id.raw().to_string(),
            name,
            member_count,
            max_members: member_limit,
        });
    }
    Ok(result)
}

#[tauri::command]
pub async fn join_lobby(state: State<'_, AppState>, lobby_id_str: String) -> Result<JoinLobbyResult, String> {
    let lobby_id_u64 = lobby_id_str
        .parse::<u64>()
        .map_err(|_| "Invalid Lobby ID")?;
    let lobby_id = LobbyId::from_raw(lobby_id_u64);
    net_manager::stop_network(&state);
    let (tx, rx) = oneshot::channel();
    {
        let matchmaking = state.steam_client.matchmaking();
        matchmaking.join_lobby(lobby_id, move |result: Result<LobbyId, ()>| {
            let _ = tx.send(result);
        });
    }
    let joined_lobby_id = rx
        .await
        .map_err(|_| "Canceled".to_string())?
        .map_err(|_| format!("Failed to join lobby"))?;
    {
        let mut current = state.current_lobby.lock().unwrap();
        *current = Some(joined_lobby_id);
    }
    state.steam_client.friends().set_rich_presence("steam_display", Some("#Status_InLobby"));
    state.steam_client.friends().set_rich_presence("connect", Some(&lobby_id_str));
    let host_id = {
        let matchmaking = state.steam_client.matchmaking();
        matchmaking.lobby_owner(joined_lobby_id)
    };
    Ok(JoinLobbyResult {
        lobby_id: joined_lobby_id.raw().to_string(),
        host_id: host_id.raw().to_string(),
    })
}

#[tauri::command]
pub async fn connect_to_host(state: State<'_, AppState>, host_id_str: String, local_port: u16) -> Result<(), String> {
    let host_id_u64 = host_id_str.parse::<u64>().map_err(|_| "Invalid Host ID")?;
    let host_id = SteamId::from_raw(host_id_u64);
    let my_id = state.steam_client.user().steam_id();
    if host_id != my_id {
        net_manager::start_network_client(&state, host_id, local_port).await
    } else {
        log::info!("Player is the host, no need to connect_to_host.");
        Ok(())
    }
}

#[tauri::command]
pub fn leave_lobby(state: State<'_, AppState>) {
    net_manager::stop_network(&state);
    let mut current = state.current_lobby.lock().unwrap();
    if let Some(lobby_id) = current.take() {
        state.steam_client.matchmaking().leave_lobby(lobby_id);
        state.steam_client.friends().clear_rich_presence();
        log::info!("Left lobby and cleared rich presence.");
    }
}

#[tauri::command]
pub fn start_hosting(state: State<'_, AppState>, local_port: u16) -> Result<(), String> {
    {
        let mut port = state.local_game_port.lock().unwrap();
        *port = local_port;
    }
    net_manager::start_network_host(&state)
}

#[tauri::command]
pub fn stop_hosting(state: State<'_, AppState>) {
    leave_lobby(state);
}

#[tauri::command]
pub fn send_invite(state: State<'_, AppState>, friend_id_str: String) -> Result<(), String> {
    let friend_id_u64 = friend_id_str.parse::<u64>().map_err(|_| "Invalid Friend ID")?;
    let current_lobby = state.current_lobby.lock().unwrap();
    if let Some(lobby_id) = *current_lobby {
        let success = unsafe {
            let mm_ptr = steamworks_sys::SteamAPI_SteamMatchmaking_v009();
            steamworks_sys::SteamAPI_ISteamMatchmaking_InviteUserToLobby(mm_ptr, lobby_id.raw(), friend_id_u64)
        };
        if success { Ok(()) } else { Err("Failed to send invite".to_string()) }
    } else {
        Err("Not in a lobby".to_string())
    }
}

#[tauri::command]
pub fn get_lobby_members(state: State<'_, AppState>) -> Vec<MemberInfo> {
    let lobby_id_opt = { *state.current_lobby.lock().unwrap() };
    if let Some(lobby_id) = lobby_id_opt {
        let matchmaking = state.steam_client.matchmaking();
        let friends = state.steam_client.friends();
        let members = matchmaking.lobby_members(lobby_id);
        let connections = state.connections.lock().unwrap();
        let my_id = state.steam_client.user().steam_id();

        members
            .into_iter()
            .map(|member_id| {
                let friend_obj = friends.get_friend(member_id);

                let mut ping = -1;
                let mut relay = "Unknown".to_string();

                if member_id == my_id {
                    ping = 0;
                    relay = "Local".to_string();
                } else {
                    unsafe {
                        let sockets = steamworks_sys::SteamAPI_SteamNetworkingSockets_SteamAPI_v012();
                        for conn in connections.iter() {
                            let mut conn_info: steamworks_sys::SteamNetConnectionInfo_t = mem::zeroed();
                            if steamworks_sys::SteamAPI_ISteamNetworkingSockets_GetConnectionInfo(sockets, *(conn as *const _ as *const u32), &mut conn_info) {
                                
                                let identity_ptr = &conn_info.m_identityRemote as *const _ as *mut steamworks_sys::SteamNetworkingIdentity;
                                let remote_steam_id_64 = steamworks_sys::SteamAPI_SteamNetworkingIdentity_GetSteamID64(identity_ptr);
                                
                                if remote_steam_id_64 == member_id.raw() {
                                    let mut status: steamworks_sys::SteamNetConnectionRealTimeStatus_t = mem::zeroed();
                                    let result = steamworks_sys::SteamAPI_ISteamNetworkingSockets_GetConnectionRealTimeStatus(sockets, *(conn as *const _ as *const u32), &mut status, 0, std::ptr::null_mut());
                                    
                                    if result == steamworks_sys::EResult::k_EResultOK {
                                        ping = status.m_nPing;
                                        relay = "P2P".to_string();
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }

                MemberInfo {
                    id: member_id.raw().to_string(),
                    name: friend_obj.name(),
                    ping,
                    relay,
                }
            })
            .collect()
    } else {
        Vec::new()
    }
}

#[tauri::command]
pub fn get_network_status(state: State<'_, AppState>) -> NetworkStatusInfo {
    let is_host = *state.is_host.lock().unwrap();
    let connections = state.connections.lock().unwrap();
    let is_connected = is_host || !connections.is_empty();
    let client_count = connections.len();

    let mut ping = 0;

    if !is_host && is_connected {
        if let Some(conn_to_host) = connections.first() {
            unsafe {
                let sockets = steamworks_sys::SteamAPI_SteamNetworkingSockets_SteamAPI_v012();
                let mut status: steamworks_sys::SteamNetConnectionRealTimeStatus_t = mem::zeroed();
                let result = steamworks_sys::SteamAPI_ISteamNetworkingSockets_GetConnectionRealTimeStatus(sockets, *(conn_to_host as *const _ as *const u32), &mut status, 0, std::ptr::null_mut());
                
                if result == steamworks_sys::EResult::k_EResultOK {
                    ping = status.m_nPing;
                } else {
                    ping = -1;
                }
            }
        }
    }

    let status_message = if is_host {
        format!("Hosting with {} players", client_count)
    } else if is_connected {
        "Connected to host".to_string()
    } else {
        "Idle".to_string()
    };

    NetworkStatusInfo {
        is_host,
        is_connected,
        tcp_client_count: client_count,
        status_message,
        ping,
    }
}