import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM
import org.kde.kquickcontrols as KQuickControls
import "Themes.js" as Themes

KCM.SimpleKCM {
    id: page

    property string cfg_theme
    property string cfg_panelStyle
    property alias cfg_customFiveHour: fiveHourColor.color
    property alias cfg_customSevenDay: sevenDayColor.color
    property alias cfg_customWarning: warningColor.color
    property alias cfg_customCritical: criticalColor.color
    property alias cfg_customSpend: spendColor.color
    property alias cfg_segmentedRings: segmentedCheck.checked
    property alias cfg_showElapsedTick: tickCheck.checked

    readonly property var preview: Themes.colors(cfg_theme, {
        highlight: Kirigami.Theme.highlightColor,
        visited: Kirigami.Theme.visitedLinkColor,
        positive: Kirigami.Theme.positiveTextColor,
        neutral: Kirigami.Theme.neutralTextColor,
        negative: Kirigami.Theme.negativeTextColor,
        text: Kirigami.Theme.textColor,
        textDim: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.5),
        textMid: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.75),
    }, {
        fiveHour: fiveHourColor.color,
        sevenDay: sevenDayColor.color,
        spend: spendColor.color,
        warning: warningColor.color,
        critical: criticalColor.color,
    })

    component Swatch: ColumnLayout {
        property color swatchColor
        property string caption
        spacing: Kirigami.Units.smallSpacing

        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            implicitWidth: Kirigami.Units.gridUnit * 1.5
            implicitHeight: implicitWidth
            radius: width / 2
            color: parent.swatchColor
        }

        QQC2.Label {
            Layout.alignment: Qt.AlignHCenter
            text: parent.caption
            font: Kirigami.Theme.smallFont
        }
    }

    Kirigami.FormLayout {
        QQC2.ComboBox {
            id: themeCombo
            Kirigami.FormData.label: i18n("Theme:")
            model: Themes.presets
            textRole: "name"
            valueRole: "id"
            onActivated: page.cfg_theme = currentValue
            Component.onCompleted: currentIndex = Math.max(0, indexOfValue(page.cfg_theme))
        }

        RowLayout {
            spacing: Kirigami.Units.largeSpacing

            Swatch { swatchColor: page.preview.fiveHour; caption: i18n("5-hour") }
            Swatch { swatchColor: page.preview.sevenDay; caption: i18n("7-day") }
            Swatch { swatchColor: page.preview.spend; caption: i18n("Spending") }
            Swatch { swatchColor: page.preview.warning; caption: i18n("Warning") }
            Swatch { swatchColor: page.preview.critical; caption: i18n("Critical") }
        }

        KQuickControls.ColorButton {
            id: fiveHourColor
            visible: page.cfg_theme === "custom"
            Kirigami.FormData.label: i18n("5-hour colour:")
        }

        KQuickControls.ColorButton {
            id: sevenDayColor
            visible: page.cfg_theme === "custom"
            Kirigami.FormData.label: i18n("7-day colour:")
        }

        KQuickControls.ColorButton {
            id: spendColor
            visible: page.cfg_theme === "custom"
            Kirigami.FormData.label: i18n("Spending colour:")
        }

        KQuickControls.ColorButton {
            id: warningColor
            visible: page.cfg_theme === "custom"
            Kirigami.FormData.label: i18n("Warning colour:")
        }

        KQuickControls.ColorButton {
            id: criticalColor
            visible: page.cfg_theme === "custom"
            Kirigami.FormData.label: i18n("Critical colour:")
        }

        Item {
            Kirigami.FormData.isSection: true
        }

        QQC2.ComboBox {
            id: panelStyleCombo
            Kirigami.FormData.label: i18n("In a panel:")
            textRole: "text"
            valueRole: "value"
            model: [
                { value: "minimal", text: i18n("Minimal: rings only") },
                { value: "standard", text: i18n("Standard: rings with 5h / 7d labels") },
                { value: "detailed", text: i18n("Detailed: labels and reset times") },
            ]
            onActivated: page.cfg_panelStyle = currentValue
            Component.onCompleted: currentIndex = Math.max(0, indexOfValue(page.cfg_panelStyle))
        }

        Item {
            Kirigami.FormData.isSection: true
        }

        QQC2.CheckBox {
            id: segmentedCheck
            Kirigami.FormData.label: i18n("Rings:")
            text: i18n("Split into hours (5h) and days (7d)")
        }

        QQC2.CheckBox {
            id: tickCheck
            text: i18n("Show a tick for time passed in the window")
        }
    }
}
