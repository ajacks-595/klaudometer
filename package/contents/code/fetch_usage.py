#!/usr/bin/env python3
"""Fetch Claude usage and spend for Klaudometer.

Two sources, picked with --sources:

  sub  Subscription limits (5-hour and 7-day windows, plus extra usage when it
       is turned on). Reads the OAuth token Claude Code keeps in
       ~/.claude/.credentials.json and calls the endpoint Claude Code's /usage
       screen uses.
  api  Claude API spend for the current month, from the Console organization's
       cost report. Needs an Admin API key, read from $ANTHROPIC_ADMIN_KEY,
       the desktop's Secret Service (KDE Wallet / GNOME Keyring), or a
       private key file for systems without one.

Always prints exactly one JSON object on stdout. When a fetch fails, the last
good result is returned with "stale": true and a human-readable "error".

Several widget instances (desktop + panel) share one cache, guarded by a lock,
so only one of them actually hits the network per refresh interval.

With --claim ID (repeatable) it fetches nothing. Instead it records alert IDs in
a shared registry and prints {"claimed": [...]}, listing only the IDs no
instance had claimed before. Instances notify only for what they claim, so each
alert pops up once even with the widget on several panels.

With --check-key it only reports whether an Admin API key can be found.
"""
import argparse
import fcntl
import glob
import json
import os
import shutil
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timedelta, timezone

API_BASE = (os.environ.get("KLAUDOMETER_API_BASE") or "https://api.anthropic.com").rstrip("/")
USAGE_URL = API_BASE + "/api/oauth/usage"
COST_URL = API_BASE + "/v1/organizations/cost_report"
USER_AGENT = "klaudometer/1.0"
CONFIG_DIR = os.environ.get("CLAUDE_CONFIG_DIR") or os.path.expanduser("~/.claude")
CREDS_PATH = os.path.join(CONFIG_DIR, ".credentials.json")
CACHE_DIR = os.path.join(
    os.environ.get("XDG_CACHE_HOME") or os.path.expanduser("~/.cache"), "klaudometer"
)
CACHE_PATH = os.path.join(CACHE_DIR, "usage.json")
API_CACHE_PATH = os.path.join(CACHE_DIR, "api.json")
LOCK_PATH = os.path.join(CACHE_DIR, "fetch.lock")
CLAIMS_PATH = os.path.join(CACHE_DIR, "notified.json")
# Secret Service attributes the Admin API key is stored under.
KEY_ATTRIBUTES = ["service", "klaudometer", "key", "admin"]
# Fallback for systems without a Secret Service; must be readable only by you.
KEY_FILE = os.path.join(
    os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config"), "klaudometer", "admin-key"
)
# Where Claude Code's CLI usually lives. Plasma's PATH often lacks the per-user
# ones (native installer, npm/bun/volta globals, the old ~/.claude/local).
CLAUDE_DIRS = [
    "~/.local/bin", "~/.claude/local", "~/.npm-global/bin", "~/.bun/bin",
    "~/.volta/bin", "~/bin", "/usr/local/bin",
]

WINDOWS = {"five_hour": 5 * 3600, "seven_day": 7 * 86400}
ERROR_RETRY_SECONDS = 60
# Alert IDs embed their window's reset time or month, so they can't recur
# after a little over a month.
CLAIM_TTL_SECONDS = 35 * 86400


class FetchError(Exception):
    def __init__(self, message, retry_after=ERROR_RETRY_SECONDS, auth=False):
        super().__init__(message)
        self.retry_after = retry_after
        self.auth = auth


def request_json(url, headers, auth_errors):
    """GET a JSON document. auth_errors maps HTTP status codes to messages for
    credential problems, which are raised with auth=True."""
    req = urllib.request.Request(
        url, headers={**headers, "Accept": "application/json", "User-Agent": USER_AGENT}
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        if e.code in auth_errors:
            raise FetchError(auth_errors[e.code], 120, auth=True)
        if e.code == 429:
            retry = int(e.headers.get("Retry-After") or 300)
            raise FetchError("Rate limited by the Anthropic API, retrying later.", retry)
        raise FetchError(f"Anthropic API error (HTTP {e.code}).")
    except (urllib.error.URLError, TimeoutError, OSError):
        raise FetchError("Offline, showing last known values.")
    except ValueError:
        raise FetchError("Unexpected response from the Anthropic API.")


# --- Subscription limits ----------------------------------------------------

def load_credentials():
    try:
        with open(CREDS_PATH) as f:
            return json.load(f)["claudeAiOauth"]
    except (OSError, ValueError, KeyError):
        raise FetchError("Not signed in to Claude Code. Run `claude` and log in.", 300)


def token_expired(creds):
    return creds.get("expiresAt", 0) / 1000 < time.time() + 60


def refresh_via_cli():
    """Ask the Claude Code CLI to refresh its own login (best effort).

    We never refresh the token ourselves: refresh tokens rotate, and racing
    Claude Code for one could log it out.
    """
    extra = [os.path.expanduser(p) for p in CLAUDE_DIRS]
    extra += sorted(glob.glob(os.path.expanduser("~/.nvm/versions/node/*/bin")), reverse=True)
    path = os.pathsep.join(extra + [os.environ.get("PATH", "/usr/bin:/bin")])
    claude = shutil.which("claude", path=path)
    if not claude:
        return
    try:
        subprocess.run(
            [claude, "auth", "status"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=30,
            env={**os.environ, "PATH": path},
        )
    except (OSError, subprocess.SubprocessError):
        pass


def request_usage(token):
    return request_json(
        USAGE_URL,
        {"Authorization": f"Bearer {token}", "anthropic-beta": "oauth-2025-04-20"},
        {401: "Login expired. Open Claude Code to refresh it."},
    )


def fetch_raw():
    creds = load_credentials()
    if token_expired(creds):
        refresh_via_cli()
        creds = load_credentials()
        if token_expired(creds):
            raise FetchError("Login expired. Open Claude Code to refresh it.", 120)
    try:
        return request_usage(creds["accessToken"]), creds
    except FetchError as e:
        if not e.auth:
            raise
        refresh_via_cli()
        creds = load_credentials()
        return request_usage(creds["accessToken"]), creds


def plan_name(creds):
    kind = (creds.get("subscriptionType") or "").lower()
    tier = (creds.get("rateLimitTier") or "").lower()
    if kind == "max":
        for mult in ("20x", "5x"):
            if mult in tier:
                return f"Max {mult}"
    return kind.capitalize() if kind else ""


def to_epoch(iso):
    if not iso:
        return None
    try:
        return datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def money(value):
    """Convert {"amount_minor": 1234, "exponent": 2} to 12.34, else None."""
    if isinstance(value, dict) and isinstance(value.get("amount_minor"), (int, float)):
        return value["amount_minor"] / 10 ** int(value.get("exponent") or 0)
    return None


def number(value):
    return float(value) if isinstance(value, (int, float)) else None


def extra_usage(raw):
    """Extra usage (usage credits past the plan's limits), or None when it's off."""
    spend = raw.get("spend") if isinstance(raw.get("spend"), dict) else {}
    extra = raw.get("extra_usage") if isinstance(raw.get("extra_usage"), dict) else {}
    if not (spend.get("enabled") or extra.get("is_enabled")):
        return None
    used = money(spend.get("used"))
    limit = money(spend.get("limit"))
    percent = number(spend.get("percent"))
    if percent is None:
        percent = number(extra.get("utilization"))
    if percent is None and used is not None and limit:
        percent = used / limit * 100
    currency = (spend.get("used") or {}).get("currency") if isinstance(spend.get("used"), dict) else None
    return {"used": used, "limit": limit, "percent": percent, "currency": currency or "USD"}


def fetch_subscription(now):
    raw, creds = fetch_raw()
    windows = {}
    for key, length in WINDOWS.items():
        w = raw.get(key)
        if isinstance(w, dict) and w.get("utilization") is not None:
            windows[key] = {
                "used": float(w["utilization"]),
                "resets_at": to_epoch(w.get("resets_at")),
                "length": length,
            }
    if not windows:
        raise FetchError("The usage API returned no 5-hour or 7-day data.")
    return {
        "fetched_at": now,
        "plan": plan_name(creds),
        "windows": windows,
        "extra": extra_usage(raw),
        "stale": False,
        "error": None,
        "next_attempt_at": 0,
    }


# --- Claude API spend ------------------------------------------------------

def wallet_key():
    """The key from the Secret Service, or None if there's no tool, service or key."""
    tool = shutil.which("secret-tool")
    if not tool:
        return None
    try:
        out = subprocess.run(
            [tool, "lookup", *KEY_ATTRIBUTES],
            stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return out.stdout.strip() if out.returncode == 0 else None


def file_key():
    """The key from KEY_FILE, or None if it doesn't exist. Refuses a file
    others can read."""
    try:
        with open(KEY_FILE) as f:
            if os.fstat(f.fileno()).st_mode & 0o077:
                raise FetchError(f"{KEY_FILE} must be private. Run: chmod 600 {KEY_FILE}", 300)
            return f.read().strip() or None
    except FileNotFoundError:
        return None
    except OSError:
        raise FetchError(f"Couldn't read {KEY_FILE}.", 300)


def load_admin_key():
    """Return (key, where it came from). The key is never printed or cached."""
    key = os.environ.get("ANTHROPIC_ADMIN_KEY")
    if key:
        return key, "environment"
    key = wallet_key()
    if key:
        return key, "wallet"
    key = file_key()
    if key:
        return key, "file"
    if not shutil.which("secret-tool"):
        raise FetchError("No Admin API key found, and secret-tool isn't installed. See Klaudometer's Spending settings.", 300)
    raise FetchError("No Admin API key stored. See Klaudometer's Spending settings.", 300)


def fetch_api_spend(now):
    key, _ = load_admin_key()
    today = datetime.fromtimestamp(now, timezone.utc).replace(hour=0, minute=0, second=0, microsecond=0)
    month_start = today.replace(day=1)
    next_month = (month_start + timedelta(days=32)).replace(day=1)
    params = {"starting_at": month_start.strftime("%Y-%m-%dT%H:%M:%SZ"), "bucket_width": "1d", "limit": "31"}
    # Amounts are decimal strings in cents.
    total = today_total = 0.0
    page = None
    for _ in range(5):
        query = urllib.parse.urlencode({**params, "page": page} if page else params)
        report = request_json(
            COST_URL + "?" + query,
            {"x-api-key": key, "anthropic-version": "2023-06-01"},
            {
                401: "The Admin API key was rejected.",
                403: "This key can't read cost data. Use an Admin API key.",
            },
        )
        for bucket in report.get("data") or []:
            cents = sum(float(r.get("amount") or 0) for r in bucket.get("results") or [])
            total += cents
            if to_epoch(bucket.get("starting_at")) == today.timestamp():
                today_total += cents
        page = report.get("next_page")
        if not report.get("has_more") or not page:
            break
    return {
        "spent": total / 100,
        "today": today_total / 100,
        "currency": "USD",
        "month_start": month_start.timestamp(),
        "next_reset": next_month.timestamp(),
        "fetched_at": now,
        "stale": False,
        "error": None,
        "next_attempt_at": 0,
    }


# --- Shared plumbing -------------------------------------------------------

def read_json(path):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def write_json(path, data):
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(data, f)
    os.replace(tmp, path)


def cached(path, max_age, now, fetch, empty):
    """Return a cached result younger than max_age, or fetch and cache a new
    one. On failure the last good result comes back marked stale. Call under
    the lock."""
    cache = read_json(path)
    if cache and max_age > 0:
        fresh = not cache.get("error") and now - cache.get("fetched_at", 0) < max_age
        backing_off = now < cache.get("next_attempt_at", 0)
        if fresh or backing_off:
            return cache
    try:
        result = fetch(now)
    except FetchError as e:
        result = dict(cache or empty)
        result.update(stale=True, error=str(e), next_attempt_at=now + e.retry_after)
    write_json(path, result)
    return result


def claim(ids, now):
    """Record alert IDs and return the ones nobody had claimed yet. Call under the lock."""
    claims = read_json(CLAIMS_PATH)
    if not isinstance(claims, dict):
        claims = {}
    claims = {k: t for k, t in claims.items() if now - t < CLAIM_TTL_SECONDS}
    fresh = [i for i in dict.fromkeys(ids) if i not in claims]
    for i in fresh:
        claims[i] = now
    write_json(CLAIMS_PATH, claims)
    return fresh


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--sources", default="sub",
        help="comma-separated sources to fetch: sub, api (default: sub)",
    )
    parser.add_argument(
        "--max-age", type=float, default=0,
        help="reuse a cached subscription result younger than this many seconds",
    )
    parser.add_argument(
        "--api-max-age", type=float, default=0,
        help="reuse a cached API spend result younger than this many seconds",
    )
    parser.add_argument(
        "--claim", action="append", metavar="ID",
        help="claim an alert ID instead of fetching (repeatable)",
    )
    parser.add_argument(
        "--check-key", action="store_true",
        help="only report whether an Admin API key can be found",
    )
    parser.add_argument(
        "--instance", metavar="ID",
        help="ignored; keeps each widget's command unique so Plasma doesn't share its output",
    )
    args = parser.parse_args()

    if args.check_key:
        try:
            _, source = load_admin_key()
            print(json.dumps({"found": True, "source": source, "error": None}))
        except FetchError as e:
            print(json.dumps({"found": False, "source": None, "error": str(e)}))
        return

    os.makedirs(CACHE_DIR, exist_ok=True)
    with open(LOCK_PATH, "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        now = time.time()
        if args.claim:
            print(json.dumps({"claimed": claim(args.claim, now)}))
            return

        sources = set(args.sources.split(","))
        result = {"windows": {}, "plan": "", "fetched_at": 0, "stale": False, "error": None}
        if "sub" in sources:
            result = cached(CACHE_PATH, args.max_age, now, fetch_subscription, result)
        if "api" in sources:
            result["api"] = cached(
                API_CACHE_PATH, args.api_max_age, now, fetch_api_spend,
                {"spent": None, "fetched_at": 0},
            )
        print(json.dumps(result))


if __name__ == "__main__":
    main()
