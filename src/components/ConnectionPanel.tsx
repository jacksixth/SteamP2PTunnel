import {useEffect, useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {toast} from "react-hot-toast";
import {ArrowRight, Gamepad2, Link} from "lucide-react";
import {useApp} from "../AppContext";
import {JoinLobbyResult} from "../types";

// import { LobbyBrowser } from "./LobbyBrowser"; // 公开房间 没有端口号进不去 隐藏

export function ConnectionPanel() {
    const {localPort, setLocalPort, setCurrentLobbyId} = useApp();
    const [activeTab, setActiveTab] = useState<'host' | 'join'>(() => (localStorage.getItem("mcct_last_tab") as 'host' | 'join') || 'host');
    const [lobbyIdInput, setLobbyIdInput] = useState(() => localStorage.getItem("mcct_last_lobby_id") || "");
    const [loading, setLoading] = useState(false);

    useEffect(() => {
        localStorage.setItem("mcct_last_tab", activeTab);
    }, [activeTab]);
    useEffect(() => {
        localStorage.setItem("mcct_last_lobby_id", lobbyIdInput);
    }, [lobbyIdInput]);

    const handleCreateLobby = async () => {
        setLoading(true);
        try {
            const id = await invoke<string>("create_lobby");
            toast.success(`房间创建成功! ID: ${id}`, {icon: '🎮'});
            await invoke("start_hosting", {localPort});
            setCurrentLobbyId(id);
        } catch (e) {
            toast.error("创建失败: " + String(e));
        } finally {
            setLoading(false);
        }
    };

    // 让 handleJoinLobby 接收参数，使其可被多个地方调用
    const handleJoinLobby = async (lobbyIdToJoin: string) => {
        if (!lobbyIdToJoin) return;
        setLoading(true);
        try {
            const result = await invoke<JoinLobbyResult>("join_lobby", {lobbyIdStr: lobbyIdToJoin});
            await invoke("connect_to_host", {
                hostIdStr: result.host_id,
                localPort: localPort
            });
            toast.success("成功加入房间", {icon: '🚀'});
            setCurrentLobbyId(result.lobby_id);
        } catch (e) {
            toast.error("加入失败: " + String(e));
        } finally {
            setLoading(false);
        }
    };

    return (
        <div className="space-y-6">
            <div className="flex p-1 bg-slate-950/50 rounded-xl border border-white/5">
                <button onClick={() => setActiveTab('host')}
                        className={`flex-1 py-2 text-sm font-medium rounded-lg transition-all ${activeTab === 'host' ? 'bg-slate-800 text-white shadow-sm ring-1 ring-white/10' : 'text-slate-400 hover:text-slate-200'}`}>
                    我是房主
                </button>
                <button onClick={() => setActiveTab('join')}
                        className={`flex-1 py-2 text-sm font-medium rounded-lg transition-all ${activeTab === 'join' ? 'bg-slate-800 text-white shadow-sm ring-1 ring-white/10' : 'text-slate-400 hover:text-slate-200'}`}>
                    加入游戏
                </button>
            </div>

            {activeTab === 'host' ? (
                // 房主面板
                <div className="space-y-4 animate-in fade-in slide-in-from-bottom-2 duration-300">
                    <div>
                        <label
                            className="block text-xs font-medium text-slate-400 mb-1.5 uppercase tracking-wider">本地游戏端口</label>
                        <div className="input-with-icon">
                            <Gamepad2 className="input-icon"/>
                            <input
                                type="number"
                                value={localPort}
                                onChange={(e) => setLocalPort(parseInt(e.target.value, 10) || 0)}
                                className="input-base"
                                placeholder="例如: 25565"
                            />
                        </div>
                        <p className="text-xs text-slate-500 mt-2">
                            我们将把其他人的流量转发到这个端口。
                        </p>
                    </div>
                    <button onClick={handleCreateLobby} disabled={loading} className="btn-primary w-full group">
                        {loading ? '创建中...' : '启动房间'}
                        {!loading && <ArrowRight size={18} className="group-hover:translate-x-1 transition-transform"/>}
                    </button>
                </div>
            ) : (
                // 加入游戏面板
                <div className="space-y-4 animate-in fade-in slide-in-from-bottom-2 duration-300">
                    <div>
                        <label
                            className="block text-xs font-medium text-slate-400 mb-1.5 uppercase tracking-wider">本地游戏端口</label>
                        <div className="input-with-icon">
                            <Gamepad2 className="input-icon"/>
                            <input
                                type="number"
                                value={localPort}
                                onChange={(e) => setLocalPort(parseInt(e.target.value, 10) || 0)}
                                className="input-base"
                                placeholder="例如: 25565"
                            />
                        </div>
                    </div>
                    <div>
                        <label className="block text-xs font-medium text-slate-400 mb-1.5 uppercase tracking-wider">通过房间
                            ID 加入</label>
                        <div className="flex items-center gap-2">
                            <div className="input-with-icon flex-1">
                                <Link className="input-icon"/>
                                <input
                                    type="text"
                                    value={lobbyIdInput}
                                    onChange={(e) => setLobbyIdInput(e.target.value)}
                                    className="input-base"
                                    placeholder="粘贴房主发来的ID"
                                />
                            </div>
                            <button onClick={() => handleJoinLobby(lobbyIdInput)} disabled={loading || !lobbyIdInput}
                                    className="btn-primary bg-green-600 hover:bg-green-500 group h-11 px-4">
                                <ArrowRight size={18}/>
                            </button>
                        </div>
                    </div>

                    {/*<div className="border-t border-white/10 pt-4 mt-4">*/}
                    {/*    <LobbyBrowser onJoinLobby={handleJoinLobby} isJoining={loading} />*/}
                    {/*</div>*/}
                </div>
            )}
        </div>
    );
}