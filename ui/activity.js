// Only completed request records are available. "Waiting" describes our feed,
// and must never be presented as a measurement of an in-flight request.
export function recordTime(record) {
  const value = record?.createdAt;
  if (!Number.isFinite(value) || value <= 0) return null;
  return value < 1e12 ? value * 1000 : value;
}

export class ActivityTracker {
  constructor() { this.apps = new Map(); this.initialized = false; }

  update(snapshot, now = Date.now()) {
    for (const app of snapshot.apps) {
      const record = app.latest;
      const previous = this.apps.get(app.appType);
      if (record?.requestId && record.requestId !== previous?.requestId) {
        this.apps.set(app.appType, {
          requestId: record.requestId,
          // At startup historical records stay historical. A new completed
          // record arriving while running gets a full display interval.
          receivedAt: this.initialized ? now : recordTime(record),
        });
      } else if (!record) this.apps.delete(app.appType);
    }
    this.initialized = true;
  }

  status(app, seconds = 15, now = Date.now()) {
    if (!app?.latest) return "empty";
    const time = this.apps.get(app.appType)?.receivedAt;
    return Number.isFinite(time) && time <= now && now - time < seconds * 1000 ? "recent" : "idle";
  }
}
