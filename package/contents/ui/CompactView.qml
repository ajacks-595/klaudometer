import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

// Panel view: one small ring per window with the percentage inside.
// Styles: "minimal" (rings only), "standard" (coloured 5h/7d labels) and
// "detailed" (labels plus reset countdowns). Click to open the full card,
// middle-click to refresh.
MouseArea {
    id: compact

    required property var plasmoidItem

    readonly property bool vertical: Plasmoid.formFactor === PlasmaCore.Types.Vertical
    readonly property string panelStyle: Plasmoid.configuration.panelStyle
    readonly property real ringSize: Math.max(Kirigami.Units.iconSizes.small,
        Math.round(vertical ? width * 0.7 : height * 0.92))

    Layout.minimumWidth: vertical ? 0 : grid.implicitWidth
    Layout.preferredWidth: vertical ? -1 : grid.implicitWidth
    Layout.minimumHeight: vertical ? grid.implicitHeight : 0
    Layout.preferredHeight: vertical ? grid.implicitHeight : -1

    hoverEnabled: true
    acceptedButtons: Qt.LeftButton | Qt.MiddleButton
    onClicked: (mouse) => {
        if (mouse.button === Qt.MiddleButton) {
            plasmoidItem.refresh(true);
        } else {
            plasmoidItem.expanded = !plasmoidItem.expanded;
        }
    }

    GridLayout {
        id: grid
        anchors.centerIn: parent
        flow: compact.vertical ? GridLayout.TopToBottom : GridLayout.LeftToRight
        rowSpacing: Kirigami.Units.largeSpacing
        columnSpacing: Kirigami.Units.largeSpacing

        Repeater {
            model: compact.plasmoidItem.meterIds

            delegate: GridLayout {
                id: cell

                required property var modelData
                readonly property var info: compact.plasmoidItem.meters[modelData]

                flow: compact.vertical ? GridLayout.TopToBottom : GridLayout.LeftToRight
                rowSpacing: 0
                columnSpacing: Kirigami.Units.smallSpacing

                // Standard style in a horizontal panel: "5h" before the ring.
                PlasmaComponents3.Label {
                    visible: compact.panelStyle === "standard" && !compact.vertical
                    text: cell.info.short
                    color: cell.info.baseColor
                    font.pixelSize: Math.round(compact.ringSize * 0.42)
                    font.weight: Font.DemiBold
                }

                UsageRing {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: compact.ringSize
                    Layout.preferredHeight: compact.ringSize
                    thickness: Math.max(2.5, compact.ringSize * 0.12)
                    value: cell.info.shown / 100
                    segments: Plasmoid.configuration.segmentedRings ? cell.info.segments : 1
                    color: cell.info.color
                    opacity: cell.info.stale ? 0.5 : 1

                    PlasmaComponents3.Label {
                        anchors.centerIn: parent
                        text: cell.info.hasData ? Math.round(cell.info.shown) : "–"
                        font.pixelSize: Math.round(compact.ringSize * (cell.info.shown >= 99.5 ? 0.3 : 0.36))
                        font.weight: Font.DemiBold
                    }
                }

                // Detailed style (and standard in a vertical panel): label and
                // reset countdown beside or under the ring.
                ColumnLayout {
                    Layout.alignment: compact.vertical ? Qt.AlignHCenter : Qt.AlignVCenter
                    visible: compact.panelStyle === "detailed"
                        || (compact.vertical && compact.panelStyle === "standard")
                    spacing: 0

                    PlasmaComponents3.Label {
                        Layout.alignment: compact.vertical ? Qt.AlignHCenter : Qt.AlignLeft
                        text: cell.info.short
                        color: cell.info.baseColor
                        font: Kirigami.Theme.smallFont
                    }

                    PlasmaComponents3.Label {
                        Layout.alignment: compact.vertical ? Qt.AlignHCenter : Qt.AlignLeft
                        visible: compact.panelStyle === "detailed"
                            && (cell.info.money ? cell.info.hasData : cell.info.secondsLeft > 0)
                        // Running out before the reset matters more than when
                        // the reset is, so it takes this line over.
                        readonly property bool warnPace: Plasmoid.configuration.showPace
                            && cell.info.runsOut && cell.info.used < 100
                        color: warnPace ? compact.plasmoidItem.colors.warning : Kirigami.Theme.textColor
                        text: {
                            const item = compact.plasmoidItem;
                            const info = cell.info;
                            if (warnPace) {
                                const when = item.formatDuration(info.secondsToLimit);
                                return compact.vertical ? "~" + when
                                    : info.money ? i18n("over in ~%1", when) : i18n("out in ~%1", when);
                            }
                            if (info.money) {
                                const spent = item.formatMoney(info.spent, info.currency);
                                return info.limit && !compact.vertical
                                    ? i18n("%1 of %2", spent, item.formatMoney(info.limit, info.currency)) : spent;
                            }
                            const left = item.formatDuration(info.secondsLeft);
                            return compact.vertical ? left : i18n("resets %1", left);
                        }
                        font: Kirigami.Theme.smallFont
                        opacity: warnPace ? 1 : 0.7
                    }
                }
            }
        }
    }
}
