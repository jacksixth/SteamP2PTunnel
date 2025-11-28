#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod net_manager;
mod steam_commands;

use app_state::AppState;
use native_dialog::{MessageDialog, MessageType};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use steamworks::networking_sockets::NetConnection;
use steamworks::networking_types::{
    NetConnectionInfo, NetConnectionStatusChanged, NetworkingConnectionState,
};
use steamworks_sys;
use tauri::Manager;
#[repr(C)]
struct NetConnectionStatusChangedHack {
    pub connection: steamworks_sys::HSteamNetConnection,
    pub connection_info: NetConnectionInfo,
    pub old_state: NetworkingConnectionState,
}

fn is_same_conn(a: &NetConnection, handle: steamworks_sys::HSteamNetConnection) -> bool {
    unsafe {
        let ptr = a as *const _ as *const steamworks_sys::HSteamNetConnection;
        *ptr == handle
    }
}

#[tokio::main]
async fn main() {
    let app_state = match AppState::new() {
        Ok(state) => state,
        Err(e) => {
            MessageDialog::new()
                .set_title("初始化失败 (Initialization Failed)")
                .set_text(&format!("无法连接到 Steam 客户端。\n请确保 Steam 正在运行并且您已登录。\n\n错误详情: {}", e))
                .set_type(MessageType::Error)
                .show_alert()
                .unwrap();

            return;
        }
    };
    env_logger::init();
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    let connections = app_state.connections.clone();
    let is_host_arc = app_state.is_host.clone();
    let current_lobby_arc = app_state.current_lobby.clone();

    let client = app_state.steam_client.clone();

    client.register_callback(move |event: NetConnectionStatusChanged| {
        let event_hack: &NetConnectionStatusChangedHack = unsafe { std::mem::transmute(&event) };
        let connection_handle = event_hack.connection;

        let mut conns = connections.lock().unwrap();

        match event_hack.connection_info.state() {
            Ok(NetworkingConnectionState::Connecting) => {
                let remote = event_hack.connection_info.identity_remote().unwrap();
                if let Some(steam_id) = remote.steam_id() {
                    println!("Incoming connection request from {:?}", steam_id);
                }

                // 状态检查
                let is_host = *is_host_arc.lock().unwrap();
                let in_lobby = current_lobby_arc.lock().unwrap().is_some();
                let should_accept = is_host || in_lobby;

                println!(
                    "Connection check: IsHost={}, InLobby={} -> Accept={}",
                    is_host, in_lobby, should_accept
                );

                if should_accept {
                    unsafe {
                        let sockets =
                            steamworks_sys::SteamAPI_SteamNetworkingSockets_SteamAPI_v012();
                        steamworks_sys::SteamAPI_ISteamNetworkingSockets_AcceptConnection(
                            sockets,
                            connection_handle,
                        );
                    }
                } else {
                    println!("REJECTING connection because we are disconnected.");
                    unsafe {
                        let sockets =
                            steamworks_sys::SteamAPI_SteamNetworkingSockets_SteamAPI_v012();
                        // 强制关闭
                        steamworks_sys::SteamAPI_ISteamNetworkingSockets_CloseConnection(
                            sockets,
                            connection_handle,
                            0,
                            std::ptr::null(),
                            false,
                        );
                    }
                }
            }
            Ok(NetworkingConnectionState::Connected) => {
                println!("Connection established.");
                let is_host = *is_host_arc.lock().unwrap();
                let in_lobby = current_lobby_arc.lock().unwrap().is_some();
                if !is_host && !in_lobby {
                    println!("Closing established connection because we are disconnected.");
                    unsafe {
                        let sockets =
                            steamworks_sys::SteamAPI_SteamNetworkingSockets_SteamAPI_v012();
                        steamworks_sys::SteamAPI_ISteamNetworkingSockets_CloseConnection(
                            sockets,
                            connection_handle,
                            0,
                            std::ptr::null(),
                            false,
                        );
                    }
                }
            }
            Ok(NetworkingConnectionState::ClosedByPeer)
            | Ok(NetworkingConnectionState::ProblemDetectedLocally) => {
                println!("Connection closed/problem");
                if let Some(pos) = conns
                    .iter()
                    .position(|c| is_same_conn(c, connection_handle))
                {
                    conns.remove(pos);
                    println!("Removed stale connection from list.");
                }
                unsafe {
                    let sockets = steamworks_sys::SteamAPI_SteamNetworkingSockets_SteamAPI_v012();
                    steamworks_sys::SteamAPI_ISteamNetworkingSockets_CloseConnection(
                        sockets,
                        connection_handle,
                        0,
                        std::ptr::null(),
                        false,
                    );
                }
            }
            _ => {}
        }
    });

    let client_for_loop = app_state.steam_client.clone();
    thread::spawn(move || {
        while shutdown_rx.try_recv().is_err() {
            client_for_loop.run_callbacks();
            thread::sleep(Duration::from_millis(10));
        }
        println!("Steam callback thread has shut down.");
    });

    let app = tauri::Builder::default()
        .manage(app_state.clone())
        .invoke_handler(tauri::generate_handler![
            steam_commands::get_friends,
            steam_commands::create_lobby,
            steam_commands::search_lobbies,
            steam_commands::join_lobby,
            steam_commands::connect_to_host,
            steam_commands::leave_lobby,
            steam_commands::start_hosting,
            steam_commands::stop_hosting,
            steam_commands::send_invite,
            steam_commands::get_lobby_members,
            steam_commands::get_network_status
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            println!("Tauri exit requested. Cleaning up network tasks...");
            api.prevent_exit();
            println!("The application will now exit.");
            std::process::exit(0);
        }
        _ => {}
    });

    println!("Tauri application has exited. Cleaning up background tasks...");

    net_manager::stop_network(&app_state);

    drop(shutdown_tx);

    println!("Cleanup complete. Process should now terminate gracefully.");
}
