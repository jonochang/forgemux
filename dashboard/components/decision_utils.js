export function severityRank(severity) {
  switch ((severity || "").toLowerCase()) {
    case "critical":
      return 0;
    case "high":
      return 1;
    case "medium":
      return 2;
    case "low":
      return 3;
    default:
      return 4;
  }
}

export function decisionAgeMinutes(createdAt, now = new Date()) {
  if (!createdAt) return 0;
  const created = new Date(createdAt);
  if (Number.isNaN(created.getTime())) return 0;
  return Math.max(0, Math.floor((now.getTime() - created.getTime()) / 60000));
}

export function formatAge(createdAt, now = new Date()) {
  const mins = decisionAgeMinutes(createdAt, now);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  const rem = mins % 60;
  return rem ? `${hours}h ${rem}m` : `${hours}h`;
}

export function sortDecisions(list) {
  return [...list].sort((a, b) => {
    const ra = severityRank(a.severity);
    const rb = severityRank(b.severity);
    if (ra !== rb) return ra - rb;
    const at = new Date(a.created_at || a.createdAt || 0).getTime();
    const bt = new Date(b.created_at || b.createdAt || 0).getTime();
    return at - bt;
  });
}

export function filterByRepo(list, repoId) {
  if (!repoId || repoId === "all") return list;
  return list.filter((d) => d.repo_id === repoId);
}
