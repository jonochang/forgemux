export function connectWS(path, { onMessage, onStatus }) {
  let ws = null;
  let stopped = false;

  const connect = () => {
    if (stopped) return;
    const protocol = window.location.protocol === "https:" ? "wss" : "ws";
    const url = `${protocol}://${window.location.host}${path}`;
    ws = new WebSocket(url);
    onStatus?.("connecting");

    ws.onopen = () => onStatus?.("live");
    ws.onmessage = (evt) => onMessage?.(evt.data);
    ws.onclose = () => {
      onStatus?.("reconnecting");
      if (!stopped) setTimeout(connect, 3000);
    };
    ws.onerror = () => {
      onStatus?.("reconnecting");
      ws.close();
    };
  };

  connect();

  return () => {
    stopped = true;
    if (ws) ws.close();
  };
}
