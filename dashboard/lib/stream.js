export function updateLastSeen(lastSeen, payload) {
  if (!payload || typeof payload !== "object") return lastSeen;
  if (payload.type === "event") {
    const eventId = Number(payload.event_id);
    if (Number.isFinite(eventId)) {
      return Math.max(lastSeen, eventId);
    }
  }
  if (payload.type === "snapshot") {
    const snapshotId = Number(payload.snapshot_id);
    if (Number.isFinite(snapshotId)) {
      return Math.max(lastSeen, snapshotId);
    }
  }
  return lastSeen;
}
