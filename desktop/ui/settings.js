// The settings window. Every change is saved straight away: Rust checks the
// values, writes desktop.json and wakes the background thread, so the tray
// and the card update at once.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const SVG = "http://www.w3.org/2000/svg";

// Setting key, palette key and label for each colour that can be overridden.
const COLORS = [
  ["five_hour_color", "five_hour", "5-hour"],
  ["seven_day_color", "seven_day", "7-day"],
  ["spend_color", "spend", "Spending"],
  ["warning_color", "warning", "Warning"],
  ["critical_color", "critical", "Critical"],
];

let view; // What get_settings returned: settings, themes, platform...
let settings;
let keyStatus = null;

function el(tag, attrs = {}, text) {
  const node = ["svg", "circle"].includes(tag) ? document.createElementNS(SVG, tag) : document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  if (text !== undefined) node.textContent = text;
  return node;
}

// --- Tabs ---------------------------------------------------------------------

const tabs = [...document.querySelectorAll("[role=tab]")];

function selectTab(tab, focus = false) {
  for (const t of tabs) {
    const on = t === tab;
    t.setAttribute("aria-selected", on);
    t.tabIndex = on ? 0 : -1;
    $(t.getAttribute("aria-controls")).hidden = !on;
  }
  if (focus) tab.focus();
}

// settings.html#spending opens on that page.
function selectTabFromHash() {
  const tab = tabs.find((t) => t.getAttribute("aria-controls") === location.hash.slice(1));
  if (tab) selectTab(tab);
}
window.addEventListener("hashchange", selectTabFromHash);
selectTabFromHash();

for (const tab of tabs) {
  tab.addEventListener("click", () => selectTab(tab));
  tab.addEventListener("keydown", (e) => {
    const step = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
    if (step) selectTab(tabs[(tabs.indexOf(tab) + step + tabs.length) % tabs.length], true);
  });
}

// --- Showing the settings -------------------------------------------------------

function theme() {
  return view.themes.find((t) => t.id === settings.theme) || view.themes[0];
}

// The theme's two rings, as in the tray icon: 5-hour inside, 7-day outside.
function rings(colors) {
  const svg = el("svg", { width: 36, height: 36, viewBox: "0 0 36 36", "aria-hidden": "true" });
  for (const [r, fraction, color] of [[8, 0.62, colors.five_hour], [15, 0.4, colors.seven_day]]) {
    const length = 2 * Math.PI * r;
    const ring = { cx: 18, cy: 18, r, fill: "none", "stroke-width": 4 };
    svg.append(el("circle", { ...ring, stroke: "var(--track)" }));
    svg.append(el("circle", {
      ...ring,
      stroke: color,
      "stroke-linecap": "round",
      "stroke-dasharray": `${fraction * length} ${length}`,
      transform: "rotate(-90 18 18)",
    }));
  }
  return svg;
}

function buildThemes() {
  $("themes").replaceChildren(
    ...view.themes.map((t) => {
      const input = el("input", { type: "radio", name: "theme", value: t.id });
      input.addEventListener("change", () => change({ theme: t.id }));
      const label = el("label", { class: "theme" });
      label.append(input, rings(t.colors), el("span", {}, t.name));
      return label;
    }),
  );
}

function buildColors() {
  $("colors").replaceChildren(
    ...COLORS.map(([key, , name]) => {
      const row = el("div", { class: "color" });
      const input = el("input", { type: "color", "data-color": key, "aria-label": `${name} colour` });
      input.addEventListener("change", () => change({ [key]: input.value.toUpperCase() }));
      const reset = el("button", { type: "button", "data-reset": key }, "Reset");
      reset.addEventListener("click", () => change({ [key]: "" }));
      row.append(input, el("span", {}, name), reset);
      return row;
    }),
  );
}

// Put the current settings into every control.
function fill() {
  for (const input of document.querySelectorAll("[data-key]")) {
    const value = settings[input.dataset.key];
    if (input.type === "checkbox") input.checked = value;
    else input.value = value;
  }
  for (const radio of document.querySelectorAll("input[name=show_remaining]")) {
    radio.checked = String(settings.show_remaining) === radio.value;
  }
  for (const radio of document.querySelectorAll("input[name=theme]")) {
    radio.checked = radio.value === settings.theme;
    radio.parentElement.classList.toggle("selected", radio.checked);
  }
  for (const [key, paletteKey] of COLORS) {
    document.querySelector(`[data-color=${key}]`).value = (settings[key] || theme().colors[paletteKey]).toLowerCase();
    document.querySelector(`[data-reset=${key}]`).hidden = !settings[key];
  }
  for (const level of document.querySelectorAll("[data-level]")) level.textContent = settings[level.dataset.level];
  $("budget").disabled = !settings.show_api;
}

// --- Saving -------------------------------------------------------------------

let savedTimer;
function footer(text, error = false) {
  const saved = $("saved");
  saved.textContent = text;
  saved.className = error ? "error" : "";
  clearTimeout(savedTimer);
  if (!error) savedTimer = setTimeout(() => (saved.textContent = "Changes are saved as you make them."), 2000);
}

async function change(changes) {
  settings = { ...settings, ...changes };
  try {
    settings = await invoke("save_settings", { settings });
    footer("Saved");
  } catch (e) {
    footer(String(e), true);
  }
  fill();
  showApiCheck();
}

for (const input of document.querySelectorAll("[data-key]")) {
  // Numbers save when you leave the field or press Enter, not on every keystroke.
  input.addEventListener("change", () => {
    const value = input.type === "checkbox" ? input.checked : input.type === "number" ? Number(input.value) : input.value;
    change({ [input.dataset.key]: value });
  });
}
for (const radio of document.querySelectorAll("input[name=show_remaining]")) {
  radio.addEventListener("change", () => change({ show_remaining: radio.value === "true" }));
}

$("start-at-login").addEventListener("change", async (e) => {
  const box = e.target;
  $("login-error").hidden = true;
  try {
    box.checked = await invoke("set_start_at_login", { enabled: box.checked });
  } catch (err) {
    box.checked = !box.checked;
    $("login-error").textContent = String(err);
    $("login-error").hidden = false;
  }
});

// --- Admin API key ----------------------------------------------------------------

const KEY_SOURCES = {
  environment: "Using the key in $ANTHROPIC_ADMIN_KEY",
  Keychain: "Saved in your Keychain",
  "Credential Manager": "Saved in Windows Credential Manager",
  wallet: "Saved in your wallet",
  file: "Saved in a private key file",
};

const KEY_HOMES = {
  macos: "Klaudometer keeps the key in your Keychain and only ever sends it to Anthropic's API.",
  windows: "Klaudometer keeps the key in Windows Credential Manager and only ever sends it to Anthropic's API.",
  linux:
    "Klaudometer keeps the key in your wallet (KDE Wallet or GNOME Keyring) through secret-tool. Without one, it goes into ~/.config/klaudometer/admin-key, readable only by you. The key is only ever sent to Anthropic's API.",
};

function showKey(status) {
  keyStatus = status;
  const box = $("key-status");
  box.className = status.source ? "status found" : status.problem ? "status problem" : "status";
  box.querySelector(".text").textContent = status.source
    ? KEY_SOURCES[status.source] || `Saved (${status.source})`
    : status.problem || "No key saved";
  // A key in the environment isn't Klaudometer's to remove.
  $("key-remove").hidden = !status.source || status.source === "environment";
  $("key-where").textContent =
    status.source === "environment"
      ? "It's used instead of any saved key. To use a saved key, unset the variable and restart Klaudometer."
      : KEY_HOMES[view.platform] || "";
  showApiCheck();
}

function keyMessage(text, error = false) {
  $("key-message").textContent = text;
  $("key-message").className = error ? "hint error" : "hint";
}

// How the last API spend fetch went, from the latest summary.
let lastSummary = null;
function showApiCheck() {
  const line = $("api-check");
  if (!lastSummary || !settings?.show_api || !keyStatus?.source) {
    line.textContent = "";
    return;
  }
  const error = lastSummary.errors.find((e) => e.startsWith("API spend: "));
  const card = lastSummary.cards.find((c) => c.id === "api");
  if (error) {
    line.textContent = error.slice("API spend: ".length);
    line.className = "hint error";
  } else if (card && card.big !== "–") {
    line.textContent = `Working: ${card.big} ${card.sub}`;
    line.className = "hint";
  } else {
    line.textContent = "Checking with Anthropic…";
    line.className = "hint";
  }
}

$("key-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const input = $("key-input");
  $("key-save").disabled = true;
  keyMessage("Saving…");
  try {
    const status = await invoke("save_admin_key", { key: input.value });
    input.value = "";
    keyMessage("");
    lastSummary = null; // Wait for the check that saving started.
    showKey(status);
    if (!settings.show_api) await change({ show_api: true });
    $("api-check").textContent = "Checking with Anthropic…";
  } catch (err) {
    keyMessage(String(err), true);
  }
  $("key-save").disabled = false;
});

// Remove asks once more before it deletes anything.
let confirmTimer;
$("key-remove").addEventListener("click", async () => {
  const button = $("key-remove");
  if (!button.dataset.confirm) {
    button.dataset.confirm = "1";
    button.textContent = "Remove Key?";
    confirmTimer = setTimeout(() => {
      delete button.dataset.confirm;
      button.textContent = "Remove";
    }, 4000);
    return;
  }
  clearTimeout(confirmTimer);
  delete button.dataset.confirm;
  button.textContent = "Remove";
  try {
    showKey(await invoke("remove_admin_key"));
    keyMessage("Key removed.");
  } catch (err) {
    keyMessage(String(err), true);
  }
});

$("admin-keys").addEventListener("click", (e) => {
  e.preventDefault();
  invoke("open_link", { link: "admin-keys" });
});

// --- Notifications ------------------------------------------------------------------

const NOTIFY_HELP = {
  macos: "Sent. If nothing appeared, allow notifications for Klaudometer in System Settings › Notifications.",
  windows: "Sent. If nothing appeared, check Settings › System › Notifications, and that Do not disturb is off.",
  linux: "Sent. If nothing appeared, check your desktop's notification settings and Do not disturb.",
};

$("test-notification").addEventListener("click", async () => {
  try {
    await invoke("test_notification");
    $("test-result").textContent = NOTIFY_HELP[view.platform] || "Sent.";
    $("test-result").className = "hint";
  } catch (err) {
    $("test-result").textContent = String(err);
    $("test-result").className = "hint error";
  }
});

// --- Start ----------------------------------------------------------------------------

async function start() {
  view = await invoke("get_settings");
  settings = view.settings;
  document.documentElement.dataset.platform = view.platform;
  $("version").textContent = `Klaudometer ${view.version}`;
  $("start-at-login").checked = view.start_at_login;
  // Windows has no text beside tray icons; Linux panels show it only sometimes.
  $("label-row").hidden = view.platform === "windows";
  if (view.platform === "linux") {
    $("label-title").textContent = "Panel text";
    $("label-hint").hidden = false;
  }
  buildThemes();
  buildColors();
  fill();
  showKey(await invoke("admin_key_status"));
}

listen("summary", (event) => {
  lastSummary = event.payload;
  showApiCheck();
});
invoke("get_summary").then((summary) => {
  lastSummary = lastSummary || summary;
  showApiCheck();
});
start();
