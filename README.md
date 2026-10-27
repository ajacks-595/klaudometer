# Klaudometer

A KDE Plasma 6 widget that shows how much of your Claude subscription's **5-hour** and **7-day** limits you've used.
It can also show your subscription's **extra usage** and your **Claude API spend** for the month.

- **Desktop:** a card with one ring per meter, how much is left, and when each one resets.
- **Panel:** small rings with the percentage inside. Click to open the card,
  middle-click to refresh, hover for details.

Telling the meters apart:
- The **5-hour** ring is split into 5 segments, one per hour, and the **7-day** ring into 7, one per day.
  Money rings (extra usage, API spend) are continuous.
- Each meter has its own colour, and the `5h` / `7d` / `Extra` / `API` labels and title dots match it.
- The small tick on a ring marks how much of that window's (or month's) *time* has passed. If the arc
  is behind the tick, you're using it slower than the limit allows.

Rings switch to the warning colour at 75% and to the critical colour at 90% (both adjustable).

**Pace:** each meter projects where you'll end up at your average rate so far, for example
"On pace for ~82%", or "Runs out in ~42m" in the warning colour when you'll hit the limit
before it resets. It appears once a tenth of the window (or month) has passed, so early
guesses aren't noise. In the panel's *Detailed* style, "out in ~42m" replaces the reset time
when that's the more urgent news.

## Requirements

Klaudometer is a **KDE Plasma 6** widget. It runs on any distro with Plasma 6. It doesn't run on
GNOME, Cinnamon, XFCE or other desktops, or on Plasma 5.

| Distro | Plasma | Status |
|---|---|---|
| Fedora 44 KDE | 6.7 | Developed and run here |
| Kubuntu / Ubuntu 26.04 LTS | 6.6 | Everything needed comes with `kubuntu-desktop` |
| Debian 13 (trixie) | 6.3 | Everything needed comes with `kde-plasma-desktop` |
| Arch Linux, Manjaro, EndeavourOS | 6.8 | Everything needed comes with `plasma-desktop` |
| openSUSE Tumbleweed | 6.8 | Everything needed comes with the `kde_plasma` pattern |
| Kubuntu 24.04 LTS, Debian 12 | 5.27 | **Not supported** (Plasma 5) |

The widget itself has only been run on Fedora. For the other distros, I checked that each
standard Plasma install includes every package the widget needs, and ran the helper under that
distro's own Python (3.13 to 3.14).

`secret-tool` is the one thing no standard install includes. You only need it to keep the Admin
API key for [API spend](#claude-api-spend) in your wallet.

On a minimal install, `./install.sh --check` lists anything missing along with the command to
install it. The full set for each distro:

**Debian / Ubuntu**

```bash
sudo apt install python3 qml6-module-org-kde-plasma-plasma5support qml6-module-qtquick-shapes qml6-module-org-kde-kcmutils qml6-module-org-kde-kquickcontrols qml6-module-org-kde-notifications libsecret-tools
```

**Fedora**

```bash
sudo dnf install python3 plasma5support qt6-qtdeclarative kf6-kcmutils kf6-kdeclarative kf6-knotifications libsecret
```

**Arch**

```bash
sudo pacman -S python plasma5support qt6-declarative kcmutils kdeclarative knotifications libsecret
```

**openSUSE**

```bash
sudo zypper install python3 plasma5support6 qt6-declarative-imports kf6-kcmutils-imports kf6-kdeclarative-imports kf6-knotifications-imports secret-tool
```

The notifications package and `secret-tool` are optional. Without the first, alerts are off.
Without the second, the Admin API key can go in a private file instead (see API spend).
Everything else works either way.

## Install / update

```bash
./install.sh
```

The installer checks for Plasma 6 and the packages above first, then installs the widget for
your user. It uses `kpackagetool6` if it's there, or copies the widget into
`~/.local/share/plasma/plasmoids/` if not. Then right-click the desktop or panel, choose
**Add or Manage Widgets**, and pick **Klaudometer**.

Each panel and desktop gets its own copy with its own settings. After an update, run
`systemctl --user restart plasma-plasmashell` so widgets already placed pick up the new code.

Uninstall: `kpackagetool6 -t Plasma/Applet -r io.github.ajacks-595.klaudometer`, or delete
`~/.local/share/plasma/plasmoids/io.github.ajacks-595.klaudometer`.

## Settings

Right-click the widget and choose **Configure Klaudometer…**

**General**
- How often to check (default every 3 minutes)
- What to show: the 5-hour window, the 7-day window, and extra usage (only appears while extra usage is turned on for your plan)
- Whether numbers show usage *used* or usage *left*
- Whether to show the pace projection (on by default)
- Warning and critical levels, used for ring colours and notifications (colour changes can be turned off)

**Appearance**
- Theme: Claude, Plasma accent (follows your colour scheme), Ocean, Sunset, Forest, Monochrome, or Custom (pick all five colours)
- Panel style:
  - *Minimal*: rings only
  - *Standard*: rings with labels
  - *Detailed*: labels plus reset countdowns, or amounts for money meters
- Ring segments and the time-passed tick can each be turned off
- Desktop background style (standard, translucent, none) is in the widget's own handle menu.

**Spending**
- Show this month's Claude API spend against a monthly budget you set (default $100)
- Shows whether your Admin API key was found, and the command to store it

**Notifications**
- A desktop notification when a meter reaches the warning or critical level (on by default).
  For API spend the levels are a share of your budget.
- A notification when a meter is on pace to run out before it resets (off by default)
- A notification when a window resets (off by default)
- A *Send Test Notification* button
- Each alert is sent once per window or month, even with the widget on several panels. The copies share a
  record in `~/.cache/klaudometer/notified.json`, so a restart or re-login doesn't repeat alerts.
  Changing a level counts as a new alert.

## Claude API spend

API spend comes from your Console organization's
[cost report](https://platform.claude.com/docs/en/manage-claude/usage-cost-api), which updates within
a few minutes. It needs an **Admin API key** (`sk-ant-admin01-…`). Individual Console accounts can't
create one; set up an organization first. Create the key at
[platform.claude.com/settings/admin-keys](https://platform.claude.com/settings/admin-keys), then store it
in your wallet (KDE Wallet or GNOME Keyring):

```bash
secret-tool store --label='Klaudometer Admin API key' service klaudometer key admin
```

Paste the key when it asks for a password. Klaudometer reads the key from the wallet each time it checks,
and never writes it to a file. The console has no API for its spend limit, so the budget is the one you
set in the widget.

If your system has no wallet service, put the key on its own in `~/.config/klaudometer/admin-key` and
make the file private with `chmod 600 ~/.config/klaudometer/admin-key`. Klaudometer refuses a key file
that other users can read.

## How it gets the numbers

`package/contents/code/fetch_usage.py` does all the fetching. It only talks to `api.anthropic.com`,
and there's no telemetry.

- **Subscription limits and extra usage:** it reads the login Claude Code stores in
  `~/.claude/.credentials.json` and calls `https://api.anthropic.com/api/oauth/usage`, the same
  endpoint Claude Code's `/usage` screen uses. The endpoint isn't documented, so it could change.
  The widget never refreshes your login token itself, because that could log Claude Code out.
  If the token has expired, it runs `claude auth status` and shows the last known values until
  Claude Code has a fresh token.
- **API spend:** `GET /v1/organizations/cost_report` for the current month (UTC), checked at most
  every 10 minutes.
- Results are cached in `~/.cache/klaudometer/`, so every copy of the widget shares a single request.
- You can run it by hand: `python3 package/contents/code/fetch_usage.py --sources sub,api`.
  For testing, `KLAUDOMETER_API_BASE` points it at another server, and `ANTHROPIC_ADMIN_KEY`
  overrides the wallet.
