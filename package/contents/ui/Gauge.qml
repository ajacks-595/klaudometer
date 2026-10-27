import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

// One meter in the full view: ring with the big number, then captions.
// Usage windows show percentages; money meters (API spend, extra usage)
// show amounts against their budget or cap.
ColumnLayout {
    id: gauge

    required property var plasmoidItem
    required property var info      // an entry of plasmoidItem.meters
    required property real ringSize

    readonly property bool showRemaining: plasmoidItem.showRemaining
    readonly property string bigText: {
        if (!info.hasData) {
            return "–";
        }
        if (!info.money) {
            return Math.round(info.shown) + "%";
        }
        const amount = showRemaining && info.limit ? Math.max(0, info.limit - info.spent) : info.spent;
        return plasmoidItem.formatMoney(amount, info.currency);
    }
    readonly property string subText: {
        if (!info.money) {
            return showRemaining ? i18n("left") : i18n("used");
        }
        if (!info.limit) {
            return i18n("this month");
        }
        const limit = plasmoidItem.formatMoney(info.limit, info.currency);
        return showRemaining ? i18n("left of %1", limit) : i18n("of %1", limit);
    }

    spacing: Kirigami.Units.smallSpacing

    UsageRing {
        Layout.alignment: Qt.AlignHCenter
        Layout.preferredWidth: gauge.ringSize
        Layout.preferredHeight: gauge.ringSize
        value: gauge.info.money && !gauge.info.limit ? 0 : gauge.info.shown / 100
        segments: Plasmoid.configuration.segmentedRings ? gauge.info.segments : 1
        marker: Plasmoid.configuration.showElapsedTick && gauge.info.elapsed >= 0
            ? (gauge.showRemaining ? 1 - gauge.info.elapsed : gauge.info.elapsed) : -1
        color: gauge.info.color
        opacity: gauge.info.stale ? 0.5 : 1

        ColumnLayout {
            anchors.centerIn: parent
            width: gauge.ringSize * 0.8
            spacing: 0

            PlasmaComponents3.Label {
                Layout.alignment: Qt.AlignHCenter
                text: gauge.bigText
                font.pixelSize: Math.round(gauge.ringSize * (gauge.bigText.length <= 4 ? 0.24
                    : gauge.bigText.length <= 6 ? 0.19 : 0.15))
                font.weight: Font.Bold
            }
            PlasmaComponents3.Label {
                Layout.alignment: Qt.AlignHCenter
                Layout.maximumWidth: gauge.ringSize * 0.75
                visible: gauge.info.hasData
                text: gauge.subText
                font.pixelSize: Math.max(9, Math.round(gauge.ringSize * 0.1))
                opacity: 0.6
                elide: Text.ElideRight
            }
        }

        HoverHandler {
            id: hover
        }

        PlasmaComponents3.ToolTip {
            visible: hover.hovered && gauge.info.elapsed >= 0
            text: gauge.info.elapsed < 0 ? ""
                : gauge.info.money
                ? i18n("%1% of the month has passed (the tick on the ring).\n%2 spent today.",
                       Math.round(gauge.info.elapsed * 100),
                       gauge.plasmoidItem.formatMoney(gauge.info.today, gauge.info.currency))
                : i18n("%1% of this window's time has passed (the tick on the ring).\nResets %2.",
                       Math.round(gauge.info.elapsed * 100),
                       gauge.plasmoidItem.formatResetTime(gauge.info.resetsAt))
        }
    }

    RowLayout {
        Layout.alignment: Qt.AlignHCenter
        Layout.maximumWidth: gauge.width
        spacing: Kirigami.Units.smallSpacing

        Rectangle {
            implicitWidth: Kirigami.Units.smallSpacing * 2
            implicitHeight: implicitWidth
            radius: width / 2
            color: gauge.info.baseColor
        }

        PlasmaComponents3.Label {
            Layout.fillWidth: true
            text: gauge.info.title
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }
    }

    PlasmaComponents3.Label {
        Layout.fillWidth: true
        horizontalAlignment: Text.AlignHCenter
        visible: gauge.info.hasData && (!gauge.info.money || gauge.info.limit || gauge.info.today !== null)
        text: gauge.info.money && !gauge.info.limit
            ? i18n("%1 today", gauge.plasmoidItem.formatMoney(gauge.info.today, gauge.info.currency))
            : gauge.showRemaining ? i18n("%1% used", Math.round(gauge.info.used))
            : i18n("%1% left", Math.round(gauge.info.left))
        color: gauge.info.color
        elide: Text.ElideRight
    }

    PlasmaComponents3.Label {
        Layout.fillWidth: true
        horizontalAlignment: Text.AlignHCenter
        visible: gauge.info.hasData && (gauge.info.secondsLeft > 0 || gauge.info.reset)
        text: gauge.info.reset ? i18n("Just reset")
            : i18n("Resets in %1", gauge.plasmoidItem.formatDuration(gauge.info.secondsLeft))
        font: Kirigami.Theme.smallFont
        opacity: 0.65
        elide: Text.ElideRight
    }
}
