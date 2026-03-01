const ANSI_COLORS = [
  "#000", "#c00", "#0a0", "#c50", "#00c", "#c0c", "#0cc", "#ccc",
  "#555", "#f55", "#5f5", "#ff5", "#55f", "#f5f", "#5ff", "#fff",
];

export function color256(n) {
  if (n == null) return "inherit";
  if (n < 16) return ANSI_COLORS[n];
  if (n >= 232) {
    const g = 8 + (n - 232) * 10;
    return `rgb(${g},${g},${g})`;
  }
  const idx = n - 16;
  const r = Math.floor(idx / 36);
  const g = Math.floor((idx % 36) / 6);
  const b = idx % 6;
  const map = [0, 95, 135, 175, 215, 255];
  return `rgb(${map[r]},${map[g]},${map[b]})`;
}

export function ansiCodesToStyle(codes) {
  if (codes.length === 1 && codes[0] === 0) return null;
  let parts = [];
  for (let ci = 0; ci < codes.length; ci++) {
    const c = codes[ci];
    if (c === 0) return null;
    else if (c === 1) parts.push("font-weight:bold");
    else if (c === 2) parts.push("opacity:0.7");
    else if (c === 3) parts.push("font-style:italic");
    else if (c === 4) parts.push("text-decoration:underline");
    else if (c === 7) parts.push("filter:invert(1)");
    else if (c === 9) parts.push("text-decoration:line-through");
    else if (c >= 30 && c <= 37) parts.push(`color:${ANSI_COLORS[c - 30]}`);
    else if (c === 38 && codes[ci + 1] === 5) {
      parts.push(`color:${color256(codes[ci + 2])}`);
      ci += 2;
    } else if (c === 38 && codes[ci + 1] === 2) {
      parts.push(`color:rgb(${codes[ci + 2]},${codes[ci + 3]},${codes[ci + 4]})`);
      ci += 4;
    } else if (c >= 40 && c <= 47) parts.push(`background:${ANSI_COLORS[c - 40]}`);
    else if (c === 48 && codes[ci + 1] === 5) {
      parts.push(`background:${color256(codes[ci + 2])}`);
      ci += 2;
    } else if (c === 48 && codes[ci + 1] === 2) {
      parts.push(`background:rgb(${codes[ci + 2]},${codes[ci + 3]},${codes[ci + 4]})`);
      ci += 4;
    } else if (c >= 90 && c <= 97) parts.push(`color:${ANSI_COLORS[c - 90 + 8]}`);
    else if (c >= 100 && c <= 107) parts.push(`background:${ANSI_COLORS[c - 100 + 8]}`);
    else if (c === 39) parts.push("color:inherit");
    else if (c === 49) parts.push("background:inherit");
  }
  return parts.length ? parts.join(";") : "";
}

export function ansiToHtml(text) {
  let html = "";
  let i = 0;
  let openSpans = 0;
  while (i < text.length) {
    if (text[i] === "\x1b" && text[i + 1] === "[") {
      let j = i + 2;
      while (j < text.length && text[j] !== "m" && j - i < 20) j++;
      if (text[j] === "m") {
        const codes = text
          .slice(i + 2, j)
          .split(";")
          .map((c) => parseInt(c, 10) || 0);
        const styles = ansiCodesToStyle(codes);
        if (styles === null) {
          while (openSpans > 0) {
            html += "</span>";
            openSpans--;
          }
        } else if (styles) {
          html += `<span style="${styles}">`;
          openSpans++;
        }
        i = j + 1;
        continue;
      }
    }
    if (text[i] === "&") html += "&amp;";
    else if (text[i] === "<") html += "&lt;";
    else if (text[i] === ">") html += "&gt;";
    else html += text[i];
    i++;
  }
  while (openSpans > 0) {
    html += "</span>";
    openSpans--;
  }
  return html;
}
