import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.plasma5support as P5Support
import org.kde.kirigami as Kirigami
import org.kde.notification
import "Themes.js" as Themes

PlasmoidItem {
    id: root

    // Last parsed output of contents/code/fetch_usage.py.
    property var usage: null
    property bool loading: false
    property real loadingSince: 0
    property real lastForcedRefresh: 0
    // Seconds since the epoch, ticked so countdowns stay current between fetches.
    property real now: Date.now() / 1000
    // Alert IDs this instance has already offered to the helper, and the
    // messages waiting on its answer.
    property var knownAlerts: ({})
    property var pendingAlerts: ({})

    readonly property var cfg: Plasmoid.configuration
    readonly property string scriptPath: decodeURIComponent(
        Qt.resolvedUrl("../code/fetch_usage.py").toString().replace(/^file:\/\//, ""))
    readonly property int refreshSeconds: Math.max(1, cfg.refreshMinutes) * 60
    readonly property bool showRemaining: cfg.showRemaining

    // Subscription data is only fetched when something shows it. If nothing at
    // all is ticked, meterIds falls back to the 5h/7d windows.
    readonly property bool wantSubscription: cfg.showFiveHour || cfg.showSevenDay || cfg.showExtra || !cfg.showApi
    readonly property bool stale: !!(usage && usage.stale)
    readonly property string errorText: {
        const errors = [];
        if (wantSubscription && usage && usage.error) {
            errors.push(usage.error);
        }
        if (cfg.showApi && usage && usage.api && usage.api.error) {
            errors.push(i18n("API spend: %1", usage.api.error));
        }
        return errors.join("\n");
    }
    readonly property string plan: usage && usage.plan ? usage.plan : ""

    readonly property var colors: Themes.colors(cfg.theme, {
        highlight: Kirigami.Theme.highlightColor,
        visited: Kirigami.Theme.visitedLinkColor,
        positive: Kirigami.Theme.positiveTextColor,
        neutral: Kirigami.Theme.neutralTextColor,
        negative: Kirigami.Theme.negativeTextColor,
        text: Kirigami.Theme.textColor,
        textDim: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.5),
        textMid: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.75),
    }, {
        fiveHour: cfg.customFiveHour,
        sevenDay: cfg.customSevenDay,
        spend: cfg.customSpend,
        warning: cfg.customWarning,
        critical: cfg.customCritical,
    })

    // Fixed facts about each meter. Window rings are split into one segment
    // per hour / per day; money rings are continuous.
    readonly property var meterDefs: ({
        five_hour: {
            name: i18n("5-hour"),
            title: i18n("5-hour window"),
            short: i18nc("short for 5-hour window", "5h"),
            segments: 5,
            colorKey: "fiveHour",
        },
        seven_day: {
            name: i18n("7-day"),
            title: i18n("7-day window"),
            short: i18nc("short for 7-day window", "7d"),
            segments: 7,
            colorKey: "sevenDay",
        },
        extra: {
            name: i18n("Extra usage"),
            title: i18n("Extra usage"),
            short: i18nc("short for extra usage", "Extra"),
            segments: 1,
            colorKey: "spend",
            money: true,
        },
        api: {
            name: i18n("API spend"),
            title: i18n("API spend"),
            short: i18nc("short for Claude API spend", "API"),
            segments: 1,
            colorKey: "spend",
            money: true,
        },
    })

    // Meters the user chose to show, in display order. Extra usage only
    // appears while it's turned on for the account.
    readonly property var meterIds: {
        const ids = [];
        if (cfg.showFiveHour) {
            ids.push("five_hour");
        }
        if (cfg.showSevenDay) {
            ids.push("seven_day");
        }
        if (cfg.showExtra && usage && usage.extra) {
            ids.push("extra");
        }
        if (cfg.showApi) {
            ids.push("api");
        }
        return ids.length > 0 ? ids : ["five_hour", "seven_day"];
    }

    // Live state of every meter. Views look meters up by id so they update in
    // place instead of being recreated on every tick.
    readonly property var meters: ({
        five_hour: meter("five_hour"),
        seven_day: meter("seven_day"),
        extra: meter("extra"),
        api: meter("api"),
    })

    function meter(id) {
        const def = meterDefs[id];
        const base = colors[def.colorKey];
        const m = {
            id: id,
            name: def.name,
            title: def.title,
            short: def.short,
            segments: def.segments,
            money: !!def.money,
            baseColor: base,
            color: base,
            hasData: false,
            stale: stale,
            used: 0,            // percent of the window, budget or cap
            left: 100,
            shown: 0,
            spent: null,        // money meters, in currency units
            limit: null,
            today: null,
            currency: "USD",
            resetsAt: 0,
            secondsLeft: -1,
            elapsed: -1,
            reset: false,
        };
        if (def.money) {
            fillMoney(m);
        } else {
            fillWindow(m);
        }
        if (m.hasData && cfg.colorByLevel) {
            if (m.used >= cfg.criticalThreshold) {
                m.color = colors.critical;
            } else if (m.used >= cfg.warningThreshold) {
                m.color = colors.warning;
            }
        }
        return m;
    }

    function fillWindow(m) {
        const w = usage && usage.windows ? usage.windows[m.id] : undefined;
        if (!w) {
            return;
        }
        m.hasData = true;
        m.resetsAt = w.resets_at || 0;
        m.secondsLeft = w.resets_at ? Math.max(0, w.resets_at - now) : -1;
        // The window rolled over since the last fetch, so nothing is used yet.
        m.reset = m.secondsLeft === 0;
        m.used = m.reset ? 0 : Math.max(0, Math.min(100, w.used));
        m.left = 100 - m.used;
        m.shown = showRemaining ? m.left : m.used;
        m.elapsed = m.secondsLeft > 0 ? Math.min(1, 1 - m.secondsLeft / w.length) : -1;
    }

    function fillMoney(m) {
        let percent = null;
        if (m.id === "api") {
            const a = usage && usage.api;
            if (!a || a.spent === null || a.spent === undefined) {
                return;
            }
            m.stale = !!a.stale;
            m.spent = a.spent;
            m.today = a.today;
            m.currency = a.currency || "USD";
            m.limit = cfg.apiBudget > 0 ? cfg.apiBudget : null;
            m.resetsAt = a.next_reset || 0;
            m.secondsLeft = m.resetsAt ? Math.max(0, m.resetsAt - now) : -1;
            if (m.secondsLeft > 0 && a.month_start) {
                m.elapsed = Math.min(1, 1 - m.secondsLeft / (m.resetsAt - a.month_start));
            }
        } else {
            const x = usage && usage.extra;
            if (!x) {
                return;
            }
            m.spent = x.used;
            m.limit = x.limit;
            m.currency = x.currency || "USD";
            percent = x.percent;
        }
        if (percent === null && m.spent !== null && m.limit) {
            percent = m.spent / m.limit * 100;
        }
        m.hasData = true;
        // Spend can run past the budget; the ring just stays full.
        m.used = Math.max(0, percent || 0);
        m.left = Math.max(0, 100 - m.used);
        m.shown = showRemaining ? m.left : m.used;
    }

    function formatMoney(value, currency) {
        if (value === null || value === undefined) {
            return "–";
        }
        const symbol = !currency || currency === "USD" ? "$" : currency + " ";
        return symbol + Number(value).toLocaleString(Qt.locale(), "f", Math.abs(value) >= 100 ? 0 : 2);
    }

    function formatDuration(secs) {
        const d = Math.floor(secs / 86400);
        const h = Math.floor(secs % 86400 / 3600);
        const m = Math.floor(secs % 3600 / 60);
        if (d > 0) {
            return i18nc("duration: days and hours", "%1d %2h", d, h);
        }
        if (h > 0) {
            return i18nc("duration: hours and minutes", "%1h %2m", h, m);
        }
        return i18nc("duration: minutes", "%1m", Math.max(1, m));
    }

    function formatResetTime(epoch) {
        const date = new Date(epoch * 1000);
        const time = date.toLocaleTimeString(Qt.locale(), Locale.ShortFormat);
        if (date.toDateString() === new Date(now * 1000).toDateString()) {
            return time;
        }
        return Qt.locale().dayName(date.getDay(), Locale.ShortFormat) + " " + time;
    }

    function formatAgo(epoch) {
        const mins = Math.floor((now - epoch) / 60);
        if (mins < 1) {
            return i18n("just now");
        }
        if (mins < 60) {
            return i18np("%1 minute ago", "%1 minutes ago", mins);
        }
        return i18n("at %1", new Date(epoch * 1000).toLocaleTimeString(Qt.locale(), Locale.ShortFormat));
    }

    function describe(m) {
        const dot = "<font color='" + m.baseColor + "'>●</font> ";
        if (!m.hasData) {
            return dot + i18n("%1: no data", m.name);
        }
        if (m.money) {
            let money = m.limit
                ? i18n("%1: %2 of %3 (%4%)", m.name, formatMoney(m.spent, m.currency), formatMoney(m.limit, m.currency), Math.round(m.used))
                : i18n("%1: %2", m.name, formatMoney(m.spent, m.currency));
            if (m.id === "api" && m.secondsLeft > 0) {
                money += " · " + i18n("resets in %1", formatDuration(m.secondsLeft));
            }
            return dot + money;
        }
        if (m.reset) {
            return dot + i18n("%1: just reset", m.name);
        }
        let text = dot + i18n("%1: %2% used, %3% left", m.name, Math.round(m.used), Math.round(m.left));
        if (m.secondsLeft > 0) {
            text += " · " + i18n("resets in %1 (%2)", formatDuration(m.secondsLeft), formatResetTime(m.resetsAt));
        }
        return text;
    }

    // Queue a notification for each shown meter that has crossed a level (or
    // just reset). The helper hands each alert to only one widget instance.
    function checkAlerts() {
        if (!usage) {
            return;
        }
        const requests = [];
        const month = new Date(now * 1000).toISOString().slice(0, 7);
        for (const id of meterIds) {
            const m = meters[id];
            // Money meters need a budget or cap for levels to mean anything.
            if (!m.hasData || m.stale || (m.money ? !m.limit : m.resetsAt <= 0)) {
                continue;
            }
            let kind = "";
            if (m.reset) {
                const hadUsage = usage.windows[id].used > 0;
                if (cfg.notifyReset && hadUsage && now - m.resetsAt < 600) {
                    kind = "reset";
                }
            } else if (cfg.notifyCritical && m.used >= cfg.criticalThreshold) {
                kind = "critical@" + cfg.criticalThreshold;
            } else if (cfg.notifyWarning && m.used >= cfg.warningThreshold) {
                kind = "warning@" + cfg.warningThreshold;
            }
            // Money resets monthly. Window reset times jitter by fractions of a
            // second between fetches, so they're rounded to the minute.
            const period = m.money ? month : Math.round(m.resetsAt / 60);
            const alertId = id + ":" + kind + ":" + period;
            if (!kind || knownAlerts[alertId]) {
                continue;
            }
            knownAlerts[alertId] = true;
            pendingAlerts[alertId] = alertMessage(m, kind);
            requests.push(alertId);
        }
        if (requests.length > 0) {
            // Plasma runs identical commands once and shares the output with
            // every widget, so tag the command with this widget's id.
            runner.connectSource("python3 '" + scriptPath + "' --instance " + Plasmoid.id
                + requests.map(a => " --claim '" + a + "'").join(""));
        }
    }

    function alertMessage(m, kind) {
        if (kind === "reset") {
            return {
                title: i18n("Claude %1 limit has reset", m.name),
                text: i18n("Your full %1 allowance is available again.", m.name),
                urgency: Notification.LowUrgency,
            };
        }
        const critical = kind.startsWith("critical");
        const urgency = critical ? Notification.HighUrgency : Notification.NormalUrgency;
        if (m.money) {
            const spent = formatMoney(m.spent, m.currency);
            const limit = formatMoney(m.limit, m.currency);
            return m.id === "api" ? {
                title: i18n("Claude API spend at %1% of budget", Math.round(m.used)),
                text: i18n("%1 of your %2 monthly budget spent.", spent, limit),
                urgency: urgency,
            } : {
                title: i18n("Claude extra usage at %1% of your limit", Math.round(m.used)),
                text: i18n("%1 of %2 used this month.", spent, limit),
                urgency: urgency,
            };
        }
        let text = critical ? i18n("Only %1% left.", Math.round(m.left)) : i18n("%1% left.", Math.round(m.left));
        if (m.secondsLeft > 0) {
            text += " " + i18n("Resets in %1 (%2).", formatDuration(m.secondsLeft), formatResetTime(m.resetsAt));
        }
        return {
            title: i18n("Claude %1 usage at %2%", m.name, Math.round(m.used)),
            text: text,
            urgency: urgency,
        };
    }

    function handleClaims(sourceName, stdout) {
        let claimed = [];
        try {
            claimed = JSON.parse(stdout).claimed || [];
        } catch (e) {
            // Unclaimed alerts are simply not shown.
        }
        for (const id of claimed) {
            const alert = pendingAlerts[id];
            if (alert) {
                notificationComponent.createObject(root, alert).sendEvent();
            }
        }
        const requested = /--claim '([^']+)'/g;
        let match;
        while ((match = requested.exec(sourceName)) !== null) {
            delete pendingAlerts[match[1]];
        }
    }

    function refresh(force) {
        now = Date.now() / 1000;
        if (loading && now - loadingSince < 90) {
            return;
        }
        loading = true;
        loadingSince = now;
        // Every widget instance shares the helper's cache, so a non-forced
        // refresh only reaches the network once per interval.
        const maxAge = force ? 0 : Math.max(30, refreshSeconds - 20);
        const sources = wantSubscription ? ["sub"] : [];
        if (cfg.showApi) {
            sources.push("api");
        }
        // Cost data lands within minutes and the month total moves slowly,
        // so API spend is checked at most every 10 minutes.
        runner.connectSource("python3 '" + scriptPath + "' --sources " + sources.join(",")
            + " --max-age " + maxAge + " --api-max-age " + (force ? 0 : 600));
    }

    switchWidth: Kirigami.Units.gridUnit * 11
    switchHeight: Kirigami.Units.gridUnit * 8

    Plasmoid.backgroundHints: PlasmaCore.Types.DefaultBackground | PlasmaCore.Types.ConfigurableBackground

    toolTipMainText: "Klaudometer"
    toolTipTextFormat: Text.RichText
    toolTipSubText: {
        const lines = meterIds.map(id => describe(meters[id]));
        if (errorText) {
            lines.push("<i>" + errorText + "</i>");
        }
        return lines.join("<br>");
    }

    compactRepresentation: CompactView {
        plasmoidItem: root
    }

    fullRepresentation: FullView {
        plasmoidItem: root
    }

    Plasmoid.contextualActions: [
        PlasmaCore.Action {
            text: i18n("Refresh Now")
            icon.name: "view-refresh"
            onTriggered: root.refresh(true)
        },
        PlasmaCore.Action {
            text: i18n("Open Usage Page")
            icon.name: "internet-web-browser"
            onTriggered: Qt.openUrlExternally("https://claude.ai/settings/usage")
        }
    ]

    P5Support.DataSource {
        id: runner
        engine: "executable"
        connectedSources: []
        onNewData: (sourceName, data) => {
            disconnectSource(sourceName);
            if (sourceName.indexOf(" --claim ") >= 0) {
                root.handleClaims(sourceName, data.stdout);
                return;
            }
            root.loading = false;
            root.now = Date.now() / 1000;
            try {
                root.usage = JSON.parse(data.stdout);
            } catch (e) {
                const stderr = (data.stderr || "").trim().split("\n").pop();
                root.usage = Object.assign({}, root.usage, {
                    stale: true,
                    error: stderr || i18n("Couldn't run the usage helper. Is python3 installed?"),
                });
            }
            root.checkAlerts();
        }
    }

    Component {
        id: notificationComponent

        Notification {
            componentName: "plasma_workspace"
            eventId: "notification"
            iconName: "speedometer"
            autoDelete: false
            onClosed: destroy()
        }
    }

    Timer {
        interval: root.refreshSeconds * 1000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: root.refresh(false)
    }

    Timer {
        interval: 20000
        running: true
        repeat: true
        onTriggered: {
            root.now = Date.now() / 1000;
            root.checkAlerts();
            const rolledOver = root.meters.five_hour.reset || root.meters.seven_day.reset;
            if (rolledOver && root.now - root.lastForcedRefresh > 120) {
                root.lastForcedRefresh = root.now;
                root.refresh(true);
            }
        }
    }
}
