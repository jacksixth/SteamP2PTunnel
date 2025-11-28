import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FriendInfo } from "../types";
import { toast } from "react-hot-toast";
import { Search, UserPlus, User, ShieldCheck } from "lucide-react";

export function FriendList() {
    const [friends, setFriends] = useState<FriendInfo[]>([]);
    const [filter, setFilter] = useState("");

    useEffect(() => {
        invoke<FriendInfo[]>("get_friends")
            .then(setFriends)
            .catch(console.error);
    }, []);

    const handleInvite = async (id: string, name: string) => {
        try {
            await invoke("send_invite", { friendIdStr: id });
            toast.success(`已邀请 ${name}`);
        } catch (e) {
            toast.error(`邀请失败: ${e}`);
        }
    };

    const filteredFriends = friends.filter(f =>
        f.name.toLowerCase().includes(filter.toLowerCase())
    );

    return (
        <div className="flex flex-col h-full">
            {/* 顶部标题栏：标题 + 搜索框在同一行 */}
            <div className="flex items-center justify-between p-3 border-b border-white/5 bg-slate-900/50 shrink-0 gap-3">
                <h2 className="font-semibold text-slate-200 flex items-center gap-2 whitespace-nowrap">
                    <ShieldCheck size={18} className="text-blue-400"/>
                    邀请好友
                </h2>

                {/* 紧凑的搜索框 */}
                <div className="relative flex-1 max-w-[180px]">
                    <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-slate-500" />
                    <input
                        type="text"
                        value={filter}
                        onChange={(e) => setFilter(e.target.value)}
                        placeholder="搜索..."
                        className="w-full bg-slate-950/80 border border-slate-700/50 rounded-full pl-8 pr-3 py-1.5 text-xs text-white focus:outline-none focus:border-blue-500/50 focus:bg-slate-900 transition-all placeholder:text-slate-600"
                    />
                </div>
            </div>

            {/* 滚动列表区域 */}
            <div className="flex-1 overflow-y-auto custom-scrollbar p-2 space-y-1 min-h-0">
                {filteredFriends.map(friend => (
                    <div key={friend.id} className="flex items-center justify-between p-2 rounded-lg hover:bg-white/5 transition group border border-transparent hover:border-white/5">
                        <div className="flex items-center gap-3 overflow-hidden">
                            <div className="w-8 h-8 rounded-full bg-slate-700 flex items-center justify-center shrink-0">
                                <User size={14} className="text-slate-400" />
                            </div>
                            <span className="text-slate-200 text-sm truncate font-medium">{friend.name}</span>
                        </div>
                        <button
                            onClick={() => handleInvite(friend.id, friend.name)}
                            className="bg-blue-600/20 hover:bg-blue-600 text-blue-400 hover:text-white p-1.5 rounded-md transition-all opacity-0 group-hover:opacity-100"
                            title="邀请加入"
                        >
                            <UserPlus size={16} />
                        </button>
                    </div>
                ))}
                {filteredFriends.length === 0 && (
                    <div className="text-center text-slate-600 text-xs py-4">
                        未找到好友
                    </div>
                )}
            </div>
        </div>
    );
}