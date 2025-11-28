import {useEffect, useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {MemberInfo} from "../types";
import {Network, User} from "lucide-react";

export function MemberList() {
    const [members, setMembers] = useState<MemberInfo[]>([]);

    useEffect(() => {
        const fetchMembers = async () => {
            try {
                const res = await invoke<MemberInfo[]>("get_lobby_members");
                setMembers(res);
            } catch (e) {
                console.error(e);
            }
        };

        fetchMembers();
        const interval = setInterval(fetchMembers, 2000);
        return () => clearInterval(interval);
    }, []);

    return (
        <div className="w-full">
            <table className="w-full text-left border-collapse">
                <thead>
                <tr className="border-b border-white/5 text-xs uppercase tracking-wider text-slate-500">
                    <th className="p-4 font-medium">用户</th>
                    <th className="p-4 font-medium">状态</th>
                    <th className="p-4 font-medium">连接类型</th>
                </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                {members.map((member) => (
                    <tr key={member.id} className="hover:bg-white/5 transition-colors group">
                        <td className="p-4">
                            <div className="flex items-center gap-3">
                                <div
                                    className="w-8 h-8 rounded bg-gradient-to-br from-slate-700 to-slate-800 flex items-center justify-center text-slate-400 group-hover:text-white transition-colors">
                                    <User size={16}/>
                                </div>
                                <span className="font-medium text-slate-200">{member.name}</span>
                            </div>
                        </td>
                        <td className="p-4">
                            <div className="flex items-center gap-2">
                                <Network size={14}
                                         className={member.ping < 100 ? "text-green-500" : "text-yellow-500"}/>
                                <span className="font-mono text-sm text-slate-400">
                    {member.ping === 0 ? "本机" : `${member.ping}ms`}
                  </span>
                            </div>
                        </td>
                        <td className="p-4">
                <span className="text-xs px-2 py-1 rounded bg-slate-800 text-slate-400 border border-slate-700">
                  {member.relay}
                </span>
                        </td>
                    </tr>
                ))}
                {members.length === 0 && (
                    <tr>
                        <td colSpan={3} className="p-8 text-center text-slate-500 italic">
                            正在等待数据同步...
                        </td>
                    </tr>
                )}
                </tbody>
            </table>
        </div>
    );
}