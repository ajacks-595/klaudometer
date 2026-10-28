// The card: draws each meter as a ring, the way the Plasma widget does.
// All text arrives preformatted from the Rust core in summary.cards.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const SVG = "http://www.w3.org/2000/svg";

function el(tag, attrs = {}, text) {
  const node = tag === "svg" || tag === "path" || tag === "line" ? document.createElementNS(SVG, tag) : document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  if (text !== undefined) node.textContent = text;
  return node;
}

// An arc of the ring, in degrees clockwise from 12 o'clock.
function arcPath(c, r, start, sweep) {
  const point = (deg) => {
    const a = ((deg - 90) * Math.PI) / 180;
    return `${(c + r * Math.cos(a)).toFixed(2)} ${(c + r * Math.sin(a)).toFixed(2)}`;
  };
  const end = start + Math.min(sweep, 359.99);
  return `M ${point(start)} A ${r} ${r} 0 ${sweep > 180 ? 1 : 0} 1 ${point(end)}`;
}

// A segmented ring: one segment per hour or day, filled clockwise, with a
// tick for how much of the window's time has passed.
function ring(card, size) {
  const stroke = Math.max(4, size * 0.1);
  const r = (size - stroke) / 2;
  const c = size / 2;
  const n = Math.max(1, card.segments);
  const span = 360 / n;
  const gap = n > 1 ? ((Math.max(1.2, stroke * 0.45) / r) * 180) / Math.PI : 0;
  const filled = card.fraction * 360;
  const svg = el("svg", { width: size, height: size, viewBox: `0 0 ${size} ${size}` });
  for (let i = 0; i < n; i++) {
    const start = i * span + gap / 2;
    const length = n === 1 ? 360 : span - gap;
    svg.append(el("path", { d: arcPath(c, r, start, length), fill: "none", stroke: "var(--track)", "stroke-width": stroke }));
    const fill = Math.min(length, filled - i * span - gap / 2);
    if (fill > 0.5) {
      svg.append(el("path", {
        d: arcPath(c, r, start, fill),
        fill: "none",
        stroke: card.color,
        "stroke-width": stroke,
        "stroke-linecap": n === 1 ? "round" : "butt",
      }));
    }
  }
  if (card.marker >= 0) {
    const a = ((card.marker * 360 - 90) * Math.PI) / 180;
    const inner = r - stroke * 0.75;
    const outer = r + stroke * 0.75;
    svg.append(el("line", {
      x1: c + inner * Math.cos(a), y1: c + inner * Math.sin(a),
      x2: c + outer * Math.cos(a), y2: c + outer * Math.sin(a),
      stroke: "var(--fg)", "stroke-opacity": 0.8, "stroke-width": Math.max(2, stroke * 0.3), "stroke-linecap": "round",
    }));
  }
  return svg;
}

// Wide enough for each meter's title on one line, and for the header.
function cardWidth(count) {
  return Math.max(320, 28 + count * 112 + (count - 1) * 10);
}

// Size the window to the card, telling Rust only when that changes.
let fitted = "";
function fit() {
  const card = document.querySelector(".card");
  const size = { width: card.offsetWidth, height: card.offsetHeight };
  if (`${size.width}x${size.height}` !== fitted) {
    fitted = `${size.width}x${size.height}`;
    invoke("fit_card", size);
  }
}

function render(summary) {
  const meters = document.getElementById("meters");
  const count = Math.max(1, summary.cards.length);
  document.querySelector(".card").style.width = `${cardWidth(count)}px`;
  const size = Math.min(118, Math.floor((meters.clientWidth - (count - 1) * 10) / count) - 8);
  meters.replaceChildren(
    ...summary.cards.map((card) => {
      const box = el("div", { class: card.stale ? "meter stale" : "meter" });
      const holder = el("div", { class: "ring", style: `width:${size}px;height:${size}px` });
      holder.append(ring(card, size));
      const value = el("div", { class: "value" });
      const bigSize = size * (card.big.length <= 4 ? 0.24 : card.big.length <= 6 ? 0.19 : 0.15);
      value.append(el("span", { class: "big", style: `font-size:${Math.round(bigSize)}px` }, card.big));
      value.append(el("span", { class: "sub", style: `font-size:${Math.max(10, Math.round(size * 0.1))}px` }, card.sub));
      holder.append(value);
      box.append(holder);

      const title = el("div", { class: "title" });
      title.append(el("span", { class: "dot", style: `background:${card.base_color}` }), card.title);
      box.append(title);
      if (card.caption) box.append(el("div", { class: "caption", style: `color:${card.color}` }, card.caption));
      if (card.resets) box.append(el("div", { class: "small" }, card.resets));
      if (card.pace) box.append(el("div", { class: card.runs_out ? "small warn" : "small" }, card.pace));
      return box;
    }),
  );

  const plan = document.getElementById("plan");
  plan.hidden = !summary.plan;
  plan.textContent = summary.plan;
  const update = document.getElementById("update");
  update.hidden = !summary.update;
  update.textContent = summary.update ? `Update available: Klaudometer ${summary.update.version}` : "";
  const footer = document.getElementById("footer");
  footer.textContent = summary.footer;
  footer.className = summary.errors.length ? "error" : "";
  fit();
}

document.getElementById("refresh").addEventListener("click", () => invoke("refresh_now"));
document.getElementById("close").addEventListener("click", () => invoke("hide_card"));
document.getElementById("usage-page").addEventListener("click", (e) => {
  e.preventDefault();
  invoke("open_link", { link: "usage" });
});
document.getElementById("update").addEventListener("click", (e) => {
  e.preventDefault();
  invoke("open_link", { link: "update" });
});
document.getElementById("settings").addEventListener("click", (e) => {
  e.preventDefault();
  invoke("open_settings");
});
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") invoke("hide_card");
});

listen("summary", (event) => render(event.payload));
invoke("get_summary").then((summary) => summary && render(summary));
