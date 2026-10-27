#!/usr/bin/env python3
"""Klaudometer for the system tray: Claude usage on any Linux desktop.

Works wherever StatusNotifierItem / AppIndicator tray icons do: GNOME (with the
AppIndicator extension, built into Ubuntu), XFCE, Cinnamon, MATE, Budgie, LXQt
and KDE Plasma 5 or 6. The icon is two rings, the 5-hour window inside and the
7-day window outside. The menu has the numbers, reset times and pace.

Data comes from the same fetch_usage.py helper the Plasma widget uses, so the
two share one cache and one alert record. Settings live in
~/.config/klaudometer/tray.ini and are re-read when the file changes.

Run with --print to print the current status once instead of showing an icon.
"""
import argparse
import configparser
import fcntl
import json
import math
import os
import signal
import subprocess
import sys
import time
from datetime import datetime

HERE = os.path.dirname(os.path.realpath(__file__))
# Installed copies sit side by side; in the repository the helper lives with the widget.
HELPER = next(
    (p for p in (
        os.path.join(HERE, "fetch_usage.py"),
        os.path.join(HERE, "..", "package", "contents", "code", "fetch_usage.py"),
    ) if os.path.exists(p)),
    None,
)
APP_ICON = os.path.join(HERE, "klaudometer.svg")
CONFIG_PATH = os.path.join(
    os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config"), "klaudometer", "tray.ini"
)
CACHE_DIR = os.path.join(
    os.environ.get("XDG_CACHE_HOME") or os.path.expanduser("~/.cache"), "klaudometer"
)
ICON_DIR = os.path.join(os.environ.get("XDG_RUNTIME_DIR") or CACHE_DIR, "klaudometer-tray")
USAGE_PAGE = "https://claude.ai/settings/usage"

# --- Settings --------------------------------------------------------------

DEFAULTS = {
    "refresh_minutes": 3,
    "show_five_hour": True,
    "show_seven_day": True,
    "show_extra": True,
    "show_api": False,
    "api_budget": 100,
    "show_remaining": False,
    "label": "compact",
    "theme": "claude",
    "warning": 75,
    "critical": 90,
    "color_by_level": True,
    "show_pace": True,
    "notify_warning": True,
    "notify_critical": True,
    "notify_pace": False,
    "notify_reset": False,
    "five_hour_color": "",
    "seven_day_color": "",
    "spend_color": "",
    "warning_color": "",
    "critical_color": "",
}

CONFIG_TEMPLATE = """\
# Klaudometer tray settings. Changes apply within about 20 seconds.

[tray]
# How often to check usage, in minutes.
refresh_minutes = {refresh_minutes}

# What to show. Extra usage only appears while it's turned on for your plan.
# API spend needs an Admin API key; see the README.
show_five_hour = {show_five_hour}
show_seven_day = {show_seven_day}
show_extra = {show_extra}
show_api = {show_api}
api_budget = {api_budget}

# Numbers show usage used (no) or usage left (yes).
show_remaining = {show_remaining}

# Text next to the icon, for trays that show labels (many, including GNOME's
# AppIndicator extension, KDE and XFCE, don't):
# full ("5h 32% · 7d 70%"), compact ("32% · 70%") or off.
label = {label}

# Colours: claude, ocean, sunset, forest or mono. The *_color keys below
# override single colours, e.g. five_hour_color = #ff8800.
theme = {theme}
five_hour_color = {five_hour_color}
seven_day_color = {seven_day_color}
spend_color = {spend_color}
warning_color = {warning_color}
critical_color = {critical_color}

# Warning and critical levels, in percent, for ring colours and notifications.
warning = {warning}
critical = {critical}
color_by_level = {color_by_level}

# Project where usage is heading ("Runs out in ~1h 20m").
show_pace = {show_pace}

# Desktop notifications.
notify_warning = {notify_warning}
notify_critical = {notify_critical}
notify_pace = {notify_pace}
notify_reset = {notify_reset}
"""


def load_settings():
    parser = configparser.ConfigParser()
    parser.read(CONFIG_PATH)
    section = parser["tray"] if parser.has_section("tray") else {}
    settings = {}
    for key, default in DEFAULTS.items():
        raw = section.get(key) if section else None
        try:
            if isinstance(default, bool):
                settings[key] = parser.BOOLEAN_STATES[raw.lower()] if raw else default
            elif isinstance(default, int):
                settings[key] = int(raw) if raw else default
            else:
                settings[key] = raw.strip() if raw is not None else default
        except (KeyError, ValueError):
            settings[key] = default
    return settings


def save_settings(settings):
    os.makedirs(os.path.dirname(CONFIG_PATH), exist_ok=True)
    values = {k: ("yes" if v is True else "no" if v is False else v) for k, v in settings.items()}
    tmp = CONFIG_PATH + ".tmp"
    with open(tmp, "w") as f:
        f.write(CONFIG_TEMPLATE.format(**values))
    os.replace(tmp, CONFIG_PATH)


# --- Colours ---------------------------------------------------------------

THEMES = {
    "claude": {"five_hour": "#D97757", "seven_day": "#6A9BCC", "spend": "#8DB06E", "warning": "#E8A33D", "critical": "#E5484D"},
    "ocean": {"five_hour": "#38BDF8", "seven_day": "#2DD4BF", "spend": "#A78BFA", "warning": "#FBBF24", "critical": "#F43F5E"},
    "sunset": {"five_hour": "#FB923C", "seven_day": "#C084FC", "spend": "#F472B6", "warning": "#FACC15", "critical": "#EF4444"},
    "forest": {"five_hour": "#A3C55B", "seven_day": "#3FAE8C", "spend": "#5FA8D3", "warning": "#E9B949", "critical": "#D9534F"},
    # Mid greys, so they read on light and dark panels alike.
    "mono": {"five_hour": "#A8A8A8", "seven_day": "#787878", "spend": "#909090", "warning": "#E8A33D", "critical": "#E5484D"},
}


def palette(settings):
    colors = dict(THEMES.get(settings["theme"], THEMES["claude"]))
    for key in colors:
        override = settings.get(key + "_color", "")
        if override.startswith("#"):
            colors[key] = override
    return colors


# --- Meters (mirrors the widget's main.qml) --------------------------------

METER_DEFS = {
    "five_hour": {"name": "5-hour", "short": "5h", "color": "five_hour"},
    "seven_day": {"name": "7-day", "short": "7d", "color": "seven_day"},
    "extra": {"name": "Extra usage", "short": "Extra", "color": "spend", "money": True},
    "api": {"name": "API spend", "short": "API", "color": "spend", "money": True},
}


def meter_ids(settings, usage):
    ids = []
    if settings["show_five_hour"]:
        ids.append("five_hour")
    if settings["show_seven_day"]:
        ids.append("seven_day")
    if settings["show_extra"] and usage and usage.get("extra"):
        ids.append("extra")
    if settings["show_api"]:
        ids.append("api")
    return ids or ["five_hour", "seven_day"]


def wants_subscription(settings):
    s = settings
    return s["show_five_hour"] or s["show_seven_day"] or s["show_extra"] or not s["show_api"]


def build_meter(meter_id, usage, settings, colors, now):
    d = METER_DEFS[meter_id]
    m = {
        "id": meter_id, "name": d["name"], "short": d["short"], "money": d.get("money", False),
        "base_color": colors[d["color"]], "color": colors[d["color"]],
        "has_data": False, "stale": bool(usage and usage.get("stale")),
        "used": 0.0, "left": 100.0, "spent": None, "limit": None, "currency": "USD",
        "resets_at": 0, "seconds_left": -1, "length": 0, "elapsed": -1, "reset": False,
        "pace_known": False, "projected": None, "seconds_to_limit": None, "runs_out": False,
    }
    usage = usage or {}
    if meter_id in ("five_hour", "seven_day"):
        w = (usage.get("windows") or {}).get(meter_id)
        if w:
            m["has_data"] = True
            m["resets_at"] = w.get("resets_at") or 0
            m["seconds_left"] = max(0, m["resets_at"] - now) if m["resets_at"] else -1
            # The window rolled over since the last fetch, so nothing is used yet.
            m["reset"] = m["seconds_left"] == 0
            m["used"] = 0.0 if m["reset"] else max(0.0, min(100.0, w["used"]))
            m["length"] = w["length"]
            if m["seconds_left"] > 0:
                m["elapsed"] = min(1.0, 1 - m["seconds_left"] / w["length"])
    else:
        percent = None
        if meter_id == "api":
            a = usage.get("api")
            if a and a.get("spent") is not None:
                m.update(has_data=True, stale=bool(a.get("stale")), spent=a["spent"],
                         currency=a.get("currency") or "USD")
                m["limit"] = settings["api_budget"] if settings["api_budget"] > 0 else None
                m["resets_at"] = a.get("next_reset") or 0
                m["seconds_left"] = max(0, m["resets_at"] - now) if m["resets_at"] else -1
                if m["seconds_left"] > 0 and a.get("month_start"):
                    m["length"] = m["resets_at"] - a["month_start"]
                    m["elapsed"] = min(1.0, 1 - m["seconds_left"] / m["length"])
        else:
            x = usage.get("extra")
            if x:
                m.update(has_data=True, spent=x.get("used"), limit=x.get("limit"),
                         currency=x.get("currency") or "USD")
                percent = x.get("percent")
        if m["has_data"]:
            if percent is None and m["spent"] is not None and m["limit"]:
                percent = m["spent"] / m["limit"] * 100
            m["used"] = max(0.0, percent or 0.0)
    m["left"] = max(0.0, 100 - m["used"])
    m["shown"] = m["left"] if settings["show_remaining"] else m["used"]

    # Straight-line projection from the average rate so far, once a tenth of
    # the window (or month) has passed.
    if (m["has_data"] and not m["reset"] and m["elapsed"] >= 0.1 and m["length"] > 0
            and not (m["money"] and not m["limit"])):
        m["pace_known"] = True
        m["projected"] = m["used"] / m["elapsed"]
        if m["used"] >= 100:
            m["seconds_to_limit"] = 0
        elif m["used"] > 0:
            m["seconds_to_limit"] = (100 - m["used"]) * m["elapsed"] * m["length"] / m["used"]
        m["runs_out"] = m["seconds_to_limit"] is not None and m["seconds_to_limit"] < m["seconds_left"]

    if m["has_data"] and settings["color_by_level"]:
        if m["used"] >= settings["critical"]:
            m["color"] = colors["critical"]
        elif m["used"] >= settings["warning"]:
            m["color"] = colors["warning"]
    return m


# --- Formatting ------------------------------------------------------------

def fmt_duration(secs):
    d, h, m = int(secs // 86400), int(secs % 86400 // 3600), int(secs % 3600 // 60)
    if d > 0:
        return f"{d}d {h}h"
    if h > 0:
        return f"{h}h {m}m"
    return f"{max(1, m)}m"


def fmt_reset_time(epoch, now):
    when = datetime.fromtimestamp(epoch)
    if when.date() == datetime.fromtimestamp(now).date():
        return when.strftime("%H:%M")
    return when.strftime("%a %H:%M")


def fmt_money(value, currency="USD"):
    if value is None:
        return "–"
    symbol = "$" if currency in (None, "", "USD") else currency + " "
    return symbol + (f"{value:,.0f}" if abs(value) >= 100 else f"{value:,.2f}")


def fmt_ago(epoch, now):
    mins = int((now - epoch) // 60)
    if mins < 1:
        return "just now"
    if mins < 60:
        return f"{mins} minute{'s' if mins != 1 else ''} ago"
    return "at " + datetime.fromtimestamp(epoch).strftime("%H:%M")


def pace_text(m, settings):
    if not settings["show_pace"] or not m["pace_known"] or m["used"] >= 100:
        return ""
    if m["runs_out"]:
        when = fmt_duration(m["seconds_to_limit"])
        return f"Over budget in ~{when}" if m["money"] else f"Runs out in ~{when}"
    if m["money"]:
        return "On pace for ~" + fmt_money(m["spent"] / m["elapsed"], m["currency"])
    if settings["show_remaining"]:
        return f"~{round(100 - m['projected'])}% left at reset"
    return f"On pace for ~{round(m['projected'])}%"


def meter_lines(m, settings, now, ring=""):
    """Headline and detail line for one meter, as shown in the menu."""
    name = m["name"] + (f" ({ring} ring)" if ring else "")
    if not m["has_data"]:
        return f"{name}: no data", ""
    if m["reset"]:
        return f"{name}: just reset", ""
    if m["money"]:
        spent, limit = fmt_money(m["spent"], m["currency"]), fmt_money(m["limit"], m["currency"])
        head = f"{name}: {spent} of {limit} ({round(m['used'])}%)" if m["limit"] else f"{name}: {spent}"
        details = [f"Resets in {fmt_duration(m['seconds_left'])}"] if m["seconds_left"] > 0 else []
    else:
        head = f"{name}: {round(m['used'])}% used, {round(m['left'])}% left"
        details = []
        if m["seconds_left"] > 0:
            details.append(f"Resets in {fmt_duration(m['seconds_left'])} ({fmt_reset_time(m['resets_at'], now)})")
    pace = pace_text(m, settings)
    if pace:
        details.append(pace)
    return head, " · ".join(details)


def label_text(meters, settings):
    if settings["label"] not in ("full", "compact"):
        return ""
    parts = []
    for m in meters:
        if not m["has_data"]:
            value = "–"
        elif m["money"] and not m["limit"]:
            value = fmt_money(m["spent"], m["currency"])
        else:
            value = f"{round(m['shown'])}%"
        parts.append(f"{m['short']} {value}" if settings["label"] == "full" else value)
    return " · ".join(parts)


# --- Icon ------------------------------------------------------------------

def ring_svg(cx, cy, r, width, fraction, color):
    track = (f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="#888888" '
             f'stroke-opacity="0.4" stroke-width="{width}"/>')
    fraction = max(0.0, min(1.0, fraction))
    if fraction >= 0.999:
        arc = f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="{color}" stroke-width="{width}"/>'
    elif fraction <= 0.004:
        arc = ""
    else:
        angle = 2 * math.pi * fraction - math.pi / 2
        ex, ey = cx + r * math.cos(angle), cy + r * math.sin(angle)
        large = 1 if fraction > 0.5 else 0
        arc = (f'<path d="M {cx} {cy - r} A {r} {r} 0 {large} 1 {ex:.2f} {ey:.2f}" fill="none" '
               f'stroke="{color}" stroke-width="{width}" stroke-linecap="round"/>')
    return track + arc


def icon_svg(meters):
    """Up to two rings: the first meter inside, the second outside."""
    def fraction(m):
        if not m["has_data"] or (m["money"] and not m["limit"]):
            return 0.0
        return m["shown"] / 100
    if len(meters) == 1:
        body = ring_svg(16, 16, 11, 6, fraction(meters[0]), meters[0]["color"])
        stale = meters[0]["stale"]
    else:
        inner, outer = meters[0], meters[1]
        body = (ring_svg(16, 16, 6.25, 4.5, fraction(inner), inner["color"])
                + ring_svg(16, 16, 12.75, 4.5, fraction(outer), outer["color"]))
        stale = inner["stale"] or outer["stale"]
    opacity = ' opacity="0.5"' if stale else ""
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">'
            f'<g{opacity}>{body}</g></svg>')


# --- Status for --print and the menu -----------------------------------------

def status(usage, settings, now):
    """Everything the tray shows: (icon meters, label, menu rows). Rows are
    (key, text, detail) where detail rows are greyed out."""
    colors = palette(settings)
    ids = meter_ids(settings, usage)
    meters = [build_meter(i, usage, settings, colors, now) for i in ids]
    rings = meters[:2]
    ring_names = {} if len(rings) < 2 else {rings[0]["id"]: "inner", rings[1]["id"]: "outer"}
    rows = []
    plan = (usage or {}).get("plan")
    rows.append(("title", "Klaudometer" + (f" · {plan}" if plan else ""), True))
    for m in meters:
        head, detail = meter_lines(m, settings, now, ring_names.get(m["id"], ""))
        rows.append((m["id"], head, False))
        if detail:
            rows.append((m["id"] + ":detail", "    " + detail, True))
    errors = []
    if usage and usage.get("error") and wants_subscription(settings):
        errors.append(usage["error"])
    if settings["show_api"] and usage and (usage.get("api") or {}).get("error"):
        errors.append("API spend: " + usage["api"]["error"])
    if errors:
        rows.extend((f"error{i}", e, True) for i, e in enumerate(errors))
    elif usage and usage.get("fetched_at"):
        rows.append(("updated", "Updated " + fmt_ago(usage["fetched_at"], now), True))
    else:
        rows.append(("updated", "Loading…", True))
    return meters, rings, label_text(rings, settings), rows


def helper_args(settings, force):
    refresh = max(1, settings["refresh_minutes"]) * 60
    sources = (["sub"] if wants_subscription(settings) else []) + (["api"] if settings["show_api"] else [])
    return ["--sources", ",".join(sources),
            "--max-age", str(0 if force else max(30, refresh - 20)),
            "--api-max-age", str(0 if force else 600)]


# --- Alerts (same IDs as the widget, so the shared registry dedups both) ----

def alert_requests(meters, settings, usage, now):
    requests = {}
    month = time.strftime("%Y-%m", time.gmtime(now))
    for m in meters:
        if not m["has_data"] or m["stale"] or (not m["limit"] if m["money"] else m["resets_at"] <= 0):
            continue
        kinds = []
        if m["reset"]:
            had_usage = usage["windows"][m["id"]]["used"] > 0
            if settings["notify_reset"] and had_usage and now - m["resets_at"] < 600:
                kinds.append("reset")
        else:
            if settings["notify_critical"] and m["used"] >= settings["critical"]:
                kinds.append(f"critical@{settings['critical']}")
            elif settings["notify_warning"] and m["used"] >= settings["warning"]:
                kinds.append(f"warning@{settings['warning']}")
            if settings["notify_pace"] and m["runs_out"] and m["used"] < 100:
                kinds.append("pace")
        period = month if m["money"] else round(m["resets_at"] / 60)
        for kind in kinds:
            requests[f"{m['id']}:{kind}:{period}"] = alert_message(m, kind)
    return requests


def alert_message(m, kind):
    if kind == "reset":
        return (f"Claude {m['name']} limit has reset", f"Your full {m['name']} allowance is available again.", "low")
    if kind == "pace":
        if m["money"]:
            return ("Claude API spend on pace to pass your budget",
                    f"At this rate you'll spend ~{fmt_money(m['spent'] / m['elapsed'], m['currency'])} "
                    f"this month (budget {fmt_money(m['limit'], m['currency'])}).", "normal")
        return (f"Claude {m['name']} limit: on pace to run out",
                f"At this rate you'll hit it in ~{fmt_duration(m['seconds_to_limit'])}, "
                f"{fmt_duration(m['seconds_left'] - m['seconds_to_limit'])} before it resets.", "normal")
    level = "high" if kind.startswith("critical") else "normal"
    if m["money"]:
        spent, limit = fmt_money(m["spent"], m["currency"]), fmt_money(m["limit"], m["currency"])
        if m["id"] == "api":
            return (f"Claude API spend at {round(m['used'])}% of budget",
                    f"{spent} of your {limit} monthly budget spent.", level)
        return (f"Claude extra usage at {round(m['used'])}% of your limit", f"{spent} of {limit} used this month.", level)
    text = f"Only {round(m['left'])}% left." if level == "high" else f"{round(m['left'])}% left."
    if m["seconds_left"] > 0:
        text += f" Resets in {fmt_duration(m['seconds_left'])}."
    return (f"Claude {m['name']} usage at {round(m['used'])}%", text, level)


# --- The tray icon -----------------------------------------------------------

class Tray:
    TICK_SECONDS = 20

    def __init__(self, Gtk, GLib, Gio, Indicator):
        self.Gtk, self.GLib, self.Gio, self.Indicator = Gtk, GLib, Gio, Indicator
        self.settings = load_settings()
        self.settings_mtime = self._mtime()
        self.usage = None
        self.loading_since = 0
        self.last_forced = 0
        self.known_alerts = set()
        self.icon_counter = 0
        self.last_svg = None
        self.menu_keys = None
        self.items = {}
        self.notify_items = {}
        os.makedirs(ICON_DIR, exist_ok=True)
        # Only one tray runs at a time (see main), so earlier icons are leftovers.
        for name in os.listdir(ICON_DIR):
            if name.startswith("klaudometer-") and name.endswith(".svg"):
                os.remove(os.path.join(ICON_DIR, name))

        self.indicator = Indicator.Indicator.new(
            "klaudometer", "speedometer", Indicator.IndicatorCategory.APPLICATION_STATUS)
        self.indicator.set_status(Indicator.IndicatorStatus.ACTIVE)
        self.indicator.set_title("Klaudometer")
        self.indicator.set_icon_theme_path(ICON_DIR)
        self.update()
        self.refresh(False)
        self.refresh_timer = GLib.timeout_add_seconds(self._refresh_seconds(), self._on_refresh_timer)
        GLib.timeout_add_seconds(self.TICK_SECONDS, self._on_tick)

    # Data ----------------------------------------------------------------

    def _refresh_seconds(self):
        return max(1, self.settings["refresh_minutes"]) * 60

    def _mtime(self):
        try:
            return os.stat(CONFIG_PATH).st_mtime
        except OSError:
            return 0

    def run_helper(self, args, callback):
        proc = self.Gio.Subprocess.new(
            [sys.executable, HELPER] + args,
            self.Gio.SubprocessFlags.STDOUT_PIPE | self.Gio.SubprocessFlags.STDERR_PIPE)

        def done(p, result):
            try:
                _, out, err = p.communicate_utf8_finish(result)
            except self.GLib.Error as e:
                out, err = "", str(e)
            callback(out or "", err or "")
        proc.communicate_utf8_async(None, None, done)

    def refresh(self, force):
        now = time.time()
        if self.loading_since and now - self.loading_since < 90:
            return
        self.loading_since = now
        self.run_helper(helper_args(self.settings, force), self._on_usage)

    def _on_usage(self, out, err):
        self.loading_since = 0
        try:
            self.usage = json.loads(out)
        except ValueError:
            reason = err.strip().splitlines()[-1] if err.strip() else "Couldn't run the usage helper."
            self.usage = dict(self.usage or {}, stale=True, error=reason)
        self.update()

    def _on_refresh_timer(self):
        self.refresh(False)
        return True

    def _on_tick(self):
        mtime = self._mtime()
        if mtime != self.settings_mtime:
            old_refresh = self._refresh_seconds()
            self.settings, self.settings_mtime = load_settings(), mtime
            if self._refresh_seconds() != old_refresh:
                self.GLib.source_remove(self.refresh_timer)
                self.refresh_timer = self.GLib.timeout_add_seconds(self._refresh_seconds(), self._on_refresh_timer)
            self.refresh(False)
        meters = self.update()
        now = time.time()
        rolled_over = any(m["reset"] for m in meters)
        if rolled_over and now - self.last_forced > 120:
            self.last_forced = now
            self.refresh(True)
        return True

    # Display -------------------------------------------------------------

    def update(self):
        now = time.time()
        meters, rings, label, rows = status(self.usage, self.settings, now)
        self._set_icon(icon_svg(rings), label)
        self._set_menu(rows)
        if self.usage:
            self._check_alerts(meters, now)
        return meters

    def _set_icon(self, svg, label):
        if svg != self.last_svg:
            self.last_svg = svg
            # Hosts cache icons by name, so every change gets a new file.
            self.icon_counter += 1
            name = f"klaudometer-{os.getpid()}-{self.icon_counter}"
            with open(os.path.join(ICON_DIR, name + ".svg"), "w") as f:
                f.write(svg)
            self.indicator.set_icon_full(name, "Claude usage")
            stale = os.path.join(ICON_DIR, f"klaudometer-{os.getpid()}-{self.icon_counter - 2}.svg")
            if os.path.exists(stale):
                os.remove(stale)
        self.indicator.set_label(label, "")

    def _set_menu(self, rows):
        keys = [key for key, _, _ in rows]
        if keys == self.menu_keys:
            for key, text, _ in rows:
                self.items[key].set_label(text)
            return
        Gtk = self.Gtk
        menu = Gtk.Menu()
        self.items = {}
        for key, text, greyed in rows:
            item = Gtk.MenuItem(label=text)
            item.set_sensitive(not greyed)
            menu.append(item)
            self.items[key] = item
        menu.append(Gtk.SeparatorMenuItem())
        self._add_action(menu, "Refresh Now", lambda *_: self.refresh(True))
        self._add_action(menu, "Open Claude Usage Page", lambda *_: self._open(USAGE_PAGE))
        menu.append(Gtk.SeparatorMenuItem())
        notifications = Gtk.MenuItem(label="Notifications")
        submenu = Gtk.Menu()
        self.notify_items = {}
        for key, text in (("notify_warning", f"At the warning level ({self.settings['warning']}%)"),
                          ("notify_critical", f"At the critical level ({self.settings['critical']}%)"),
                          ("notify_pace", "When on pace to run out"),
                          ("notify_reset", "When a window resets")):
            check = Gtk.CheckMenuItem(label=text)
            check.set_active(self.settings[key])
            check.connect("toggled", self._on_notify_toggled, key)
            submenu.append(check)
            self.notify_items[key] = check
        notifications.set_submenu(submenu)
        menu.append(notifications)
        self._add_action(menu, "Edit Settings…", lambda *_: self._edit_settings())
        menu.append(Gtk.SeparatorMenuItem())
        self._add_action(menu, "Quit", lambda *_: Gtk.main_quit())
        menu.show_all()
        self.indicator.set_menu(menu)
        self.menu_keys = keys
        # Keep a reference; AppIndicator doesn't hold one on every version.
        self.menu = menu

    def _add_action(self, menu, text, handler):
        item = self.Gtk.MenuItem(label=text)
        item.connect("activate", handler)
        menu.append(item)

    def _on_notify_toggled(self, check, key):
        self.settings[key] = check.get_active()
        save_settings(self.settings)
        self.settings_mtime = self._mtime()

    def _open(self, uri):
        try:
            self.Gio.AppInfo.launch_default_for_uri(uri, None)
        except self.GLib.Error:
            subprocess.Popen(["xdg-open", uri], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def _edit_settings(self):
        if not os.path.exists(CONFIG_PATH):
            save_settings(self.settings)
        self._open(self.GLib.filename_to_uri(CONFIG_PATH, None))

    # Alerts --------------------------------------------------------------

    def _check_alerts(self, meters, now):
        requests = {k: v for k, v in alert_requests(meters, self.settings, self.usage, now).items()
                    if k not in self.known_alerts}
        if not requests:
            return
        self.known_alerts.update(requests)
        args = ["--instance", "tray"]
        for alert_id in requests:
            args += ["--claim", alert_id]

        def claimed(out, _err):
            try:
                ids = json.loads(out).get("claimed", [])
            except ValueError:
                ids = []
            for alert_id in ids:
                self._notify(*requests[alert_id])
        self.run_helper(args, claimed)

    def _notify(self, title, body, level):
        GLib = self.GLib
        hints = {"urgency": GLib.Variant("y", {"low": 0, "normal": 1, "high": 2}[level]),
                 "desktop-entry": GLib.Variant("s", "klaudometer-tray")}
        icon = APP_ICON if os.path.exists(APP_ICON) else "speedometer"
        try:
            bus = self.Gio.bus_get_sync(self.Gio.BusType.SESSION, None)
            bus.call("org.freedesktop.Notifications", "/org/freedesktop/Notifications",
                     "org.freedesktop.Notifications", "Notify",
                     GLib.Variant("(susssasa{sv}i)", ("Klaudometer", 0, icon, title, body, [], hints, -1)),
                     None, self.Gio.DBusCallFlags.NONE, -1, None, None, None)
        except GLib.Error as e:
            print(f"Couldn't send a notification: {e.message}", file=sys.stderr)


def import_gui():
    """Import GTK 3 and an AppIndicator library, or explain what's missing."""
    try:
        import gi
        gi.require_version("Gtk", "3.0")
        from gi.repository import Gio, GLib, Gtk
    except (ImportError, ValueError):
        sys.exit("Klaudometer's tray icon needs PyGObject and GTK 3. Run tray/install.sh --check to see what to install.")
    for namespace in ("AyatanaAppIndicator3", "AppIndicator3"):
        try:
            gi.require_version(namespace, "0.1")
            indicator = __import__("gi.repository." + namespace, fromlist=[namespace])
            return Gtk, GLib, Gio, indicator
        except (ImportError, ValueError):
            continue
    sys.exit("Klaudometer's tray icon needs the Ayatana AppIndicator library. Run tray/install.sh --check to see what to install.")


def print_status():
    settings = load_settings()
    out = subprocess.run([sys.executable, HELPER] + helper_args(settings, False),
                         capture_output=True, text=True, timeout=120)
    try:
        usage = json.loads(out.stdout)
    except ValueError:
        sys.exit(out.stderr.strip() or "Couldn't run the usage helper.")
    _, _, label, rows = status(usage, settings, time.time())
    for _, text, _ in rows:
        print(text)
    if label:
        print(f"Tray label: {label}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--print", action="store_true", help="print the current status once and exit")
    args = parser.parse_args()
    if not HELPER:
        sys.exit("Can't find fetch_usage.py next to this script. Reinstall with tray/install.sh.")
    if args.print:
        print_status()
        return

    os.makedirs(CACHE_DIR, exist_ok=True)
    lock = open(os.path.join(CACHE_DIR, "tray.lock"), "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        print("Klaudometer's tray icon is already running.")
        return

    Gtk, GLib, Gio, Indicator = import_gui()
    if not os.path.exists(CONFIG_PATH):
        save_settings(load_settings())
    Tray(Gtk, GLib, Gio, Indicator)
    # Python can't run its own handlers while GTK's loop is in C, so let
    # Ctrl+C and SIGTERM simply end the process.
    signal.signal(signal.SIGINT, signal.SIG_DFL)
    signal.signal(signal.SIGTERM, signal.SIG_DFL)
    Gtk.main()


if __name__ == "__main__":
    main()
