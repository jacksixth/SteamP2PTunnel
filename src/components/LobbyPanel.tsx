import { useState } from "react";
import { LogOut, Copy, Check } from "lucide-react";
import { useApp } from "../AppContext";
import { toast } from "react-hot-toast";

interface Props {
    onDisconnect: () => void;
}

export function LobbyPanel({ onDisconnect }: Props) {
    const { networkStatus, currentLobbyId, localPort } = useApp();
    const { isHost } = networkStatus;

    const [copied, setCopied] = useState(false);

    const handleCopy = () => {
        if (!currentLobbyId) {
            toast.error("Lobby ID not available yet.");
            return;
        }
        navigator.clipboard.writeText(currentLobbyId);
        setCopied(true);
        toast.success("房间 ID 已复制!");
        setTimeout(() => setCopied(false), 2000);
    };

    return (
        <div className="space-y-6">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                    <div className="relative">
                        <div className={`w-3 h-3 rounded-full ${isHost ? 'bg-blue-500' : 'bg-green-500'} animate-pulse`}></div>
                        <div className={`absolute inset-0 w-3 h-3 rounded-full ${isHost ? 'bg-blue-500' : 'bg-green-500'} animate-ping opacity-75`}></div>
                    </div>
                    <h2 className="text-xl font-bold text-white">
                        {isHost ? "正在主持" : "已连接"}
                    </h2>
                </div>
                {isHost && (
                    <button onClick={handleCopy} className="btn-secondary text-xs flex items-center gap-1.5">
                        {copied ? <Check size={14} /> : <Copy size={14} />}
                        <span>{copied ? '已复制' : '复制ID'}</span>
                    </button>
                )}
            </div>

            <div className="bg-slate-950/30 rounded-lg p-4 border border-white/5 space-y-3">
                <p className="text-sm text-slate-400">
                    {isHost
                        ? `P2P 隧道已建立。客户端连接 127.0.0.1:${localPort} 即可。`
                        : `隧道畅通。请在游戏中连接 127.0.0.1:${localPort}。`}
                </p>
                <div className="flex items-center gap-2 bg-black/40 p-2 rounded border border-white/10 font-mono text-green-400 justify-center text-lg select-all">
                    127.0.0.1:{localPort}
                </div>
            </div>

            <button onClick={onDisconnect} className="btn-danger w-full mt-4">
                <LogOut size={18} />
                {isHost ? "关闭房间" : "断开连接"}
            </button>
        </div>
    );
}