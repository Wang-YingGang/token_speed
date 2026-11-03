export function isActive(session) { return ["running", "active"].includes(session.status); }

export function statusLabel(status) {
  return { running: "运行中", active: "近期活动", completed: "已完成", unknown: "状态未知" }[status] || "状态未知";
}

export function durationLabel(start, now = Date.now()) {
  if (!Number.isFinite(start) || start <= 0) return "";
  const seconds = Math.max(0, Math.floor((now - start) / 1000));
  const minutes = Math.floor(seconds / 60);
  return minutes >= 60 ? `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}` : `${String(minutes).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
}

export function sessionCandidates(sessions, appType, seconds = 15, now = Date.now()) {
  const matching = sessions.filter((s) => s.appType === appType);
  const active = matching.filter(isActive);
  if (active.length) return active;
  return matching.filter((s) => now >= s.lastActiveAt && now - s.lastActiveAt < seconds * 1000)
    .sort((a, b) => b.lastActiveAt - a.lastActiveAt).slice(0, 1);
}

export function sessionOutput(session) {
  const current = session.entries.filter((e) => !session.startedAt || e.timestamp >= session.startedAt);
  const entry = current.at(-1);
  if (!entry) return session.preview || "等待输出…";
  return entry.kind === "assistant" ? entry.text : `${entry.title} · ${entry.text}`;
}

export class SessionRotation {
  constructor() { this.selected = new Map(); }
  current(appType, sessions) {
    const selected = sessions.find((s) => s.id === this.selected.get(appType)) || sessions[0];
    if (selected) this.selected.set(appType, selected.id);
    else this.selected.delete(appType);
    return selected || null;
  }
  next(appType, sessions, direction = 1) {
    const selected = this.current(appType, sessions);
    if (!selected) return null;
    const index = sessions.indexOf(selected);
    const next = sessions[(index + direction % sessions.length + sessions.length) % sessions.length];
    this.selected.set(appType, next.id);
    return next;
  }
}
