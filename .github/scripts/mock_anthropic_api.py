"""A local stand-in for the two Anthropic endpoints Klaudometer reads, and for
GitHub's latest-release endpoint the desktop app's update check asks, for CI
smoke tests and screenshots. Nothing here talks to Anthropic or GitHub.

  python3 mock_anthropic_api.py PORT [demo|calm]

demo: 5-hour at 81% with 2h left (warning colour, "runs out" pace), 7-day at
      47%, extra usage $12.34 of $50, and API spend $42.75 this month.
calm: 5-hour at 42%, 7-day at 63%, no extra usage.

It expects the OAuth token "fake-oauth-token" and the Admin key
"sk-ant-admin01-fake", and answers anything else with 401. The latest release
is always v99.0.0, so the update check always finds one.
"""
import json
import sys
import time
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

OAUTH_TOKEN = "fake-oauth-token"
ADMIN_KEY = "sk-ant-admin01-fake"
PROFILE = sys.argv[2] if len(sys.argv) > 2 else "demo"


def iso_in(seconds):
    return datetime.fromtimestamp(time.time() + seconds, timezone.utc).isoformat()


def usage():
    if PROFILE == "calm":
        return {
            "five_hour": {"utilization": 42.0, "resets_at": iso_in(3 * 3600 + 600)},
            "seven_day": {"utilization": 63.0, "resets_at": iso_in(2 * 86400 + 5 * 3600)},
            "extra_usage": {"is_enabled": False},
            "spend": {"enabled": False},
        }
    return {
        "five_hour": {"utilization": 81.0, "resets_at": iso_in(7200)},
        "seven_day": {"utilization": 47.0, "resets_at": iso_in(3 * 86400)},
        "extra_usage": {"is_enabled": True, "utilization": 24.68},
        "spend": {"enabled": True, "percent": 24.68,
                  "used": {"amount_minor": 1234, "currency": "USD", "exponent": 2},
                  "limit": {"amount_minor": 5000, "currency": "USD", "exponent": 2}},
    }


def bucket(start, amounts):
    return {"starting_at": start, "ending_at": start,
            "results": [{"amount": a, "currency": "USD"} for a in amounts]}


def cost_report(page):
    # Two pages: earlier days (dated so they never collide with today), then
    # today. Total $42.75, of which $12.50 today.
    if page != "p2":
        return {"data": [bucket("2000-01-01T00:00:00Z", ["1000.00", "250.5"]),
                         bucket("2000-01-02T00:00:00Z", ["1774.5"])],
                "has_more": True, "next_page": "p2"}
    today = datetime.now(timezone.utc).strftime("%Y-%m-%dT00:00:00Z")
    return {"data": [bucket(today, ["1200", "50"])], "has_more": False, "next_page": None}


class Handler(BaseHTTPRequestHandler):
    def send(self, code, body):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        url = urlparse(self.path)
        if url.path == "/api/oauth/usage":
            if self.headers.get("Authorization") != "Bearer " + OAUTH_TOKEN:
                return self.send(401, {"error": "bad token"})
            return self.send(200, usage())
        if url.path == "/v1/organizations/cost_report":
            if self.headers.get("x-api-key") != ADMIN_KEY:
                return self.send(401, {"error": "bad key"})
            return self.send(200, cost_report(parse_qs(url.query).get("page", [""])[0]))
        if url.path == "/repos/ajacks-595/klaudometer/releases/latest":
            return self.send(200, {"tag_name": "v99.0.0",
                                   "html_url": "https://github.com/ajacks-595/klaudometer/releases/tag/v99.0.0"})
        self.send(404, {"error": "not found"})

    def log_message(self, fmt, *args):
        print("mock:", fmt % args, flush=True)


HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
