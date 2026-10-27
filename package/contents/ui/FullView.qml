import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

// The desktop card (and the popup when clicked in the panel).
ColumnLayout {
    id: full

    required property var plasmoidItem

    readonly property int count: plasmoidItem.meterIds.length
    // Room left for the rings once the header, footer and captions are laid out.
    readonly property real captionsHeight: Kirigami.Units.gridUnit * (plasmoidItem.cfg.showPace ? 4.3 : 3.4)
    readonly property real ringSize: Math.max(Kirigami.Units.gridUnit * 3, Math.min(
        Kirigami.Units.gridUnit * 9,
        (width - gauges.spacing * (count - 1)) / count * 0.8,
        height - header.height - footer.height - captionsHeight - spacing * 3))

    Layout.preferredWidth: Kirigami.Units.gridUnit * Math.max(18, count * 8)
    Layout.preferredHeight: Kirigami.Units.gridUnit * 14
    Layout.minimumWidth: Kirigami.Units.gridUnit * Math.max(8, count * 6.5)
    Layout.minimumHeight: Kirigami.Units.gridUnit * 10

    spacing: Kirigami.Units.smallSpacing * 2

    RowLayout {
        id: header
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing * 2

        Kirigami.Icon {
            Layout.preferredWidth: Kirigami.Units.iconSizes.small
            Layout.preferredHeight: Kirigami.Units.iconSizes.small
            source: "speedometer"
        }

        PlasmaComponents3.Label {
            Layout.fillWidth: true
            text: "Klaudometer"
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }

        Rectangle {
            visible: full.plasmoidItem.plan !== ""
            implicitWidth: planLabel.implicitWidth + Kirigami.Units.largeSpacing * 1.5
            implicitHeight: planLabel.implicitHeight + Kirigami.Units.smallSpacing
            radius: height / 2
            color: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.1)

            PlasmaComponents3.Label {
                id: planLabel
                anchors.centerIn: parent
                text: full.plasmoidItem.plan
                font: Kirigami.Theme.smallFont
                opacity: 0.85
            }
        }

        PlasmaComponents3.ToolButton {
            icon.name: "view-refresh"
            text: i18n("Refresh now")
            display: QQC2.AbstractButton.IconOnly
            enabled: !full.plasmoidItem.loading
            onClicked: full.plasmoidItem.refresh(true)

            PlasmaComponents3.ToolTip {
                text: parent.text
            }
        }
    }

    RowLayout {
        id: gauges
        Layout.fillWidth: true
        Layout.fillHeight: true
        spacing: Kirigami.Units.largeSpacing

        Repeater {
            model: full.plasmoidItem.meterIds

            delegate: Gauge {
                required property var modelData
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                Layout.alignment: Qt.AlignTop
                plasmoidItem: full.plasmoidItem
                info: full.plasmoidItem.meters[modelData]
                ringSize: full.ringSize
            }
        }
    }

    PlasmaComponents3.Label {
        id: footer
        Layout.fillWidth: true
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.WordWrap
        maximumLineCount: 2
        elide: Text.ElideRight
        font: Kirigami.Theme.smallFont
        color: full.plasmoidItem.errorText ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.textColor
        opacity: full.plasmoidItem.errorText ? 1 : 0.55
        text: {
            const item = full.plasmoidItem;
            if (item.errorText) {
                return item.errorText;
            }
            if (!item.usage) {
                return i18n("Loading…");
            }
            return i18n("Updated %1", item.formatAgo(item.usage.fetched_at));
        }
    }
}
