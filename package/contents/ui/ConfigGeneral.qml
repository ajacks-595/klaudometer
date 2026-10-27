import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM

KCM.SimpleKCM {
    property alias cfg_refreshMinutes: refreshSpin.value
    property bool cfg_showRemaining
    property alias cfg_showFiveHour: fiveHourCheck.checked
    property alias cfg_showSevenDay: sevenDayCheck.checked
    property alias cfg_showExtra: extraCheck.checked
    property alias cfg_colorByLevel: colorByLevelCheck.checked
    property alias cfg_warningThreshold: warningSpin.value
    property alias cfg_criticalThreshold: criticalSpin.value

    Kirigami.FormLayout {
        QQC2.SpinBox {
            id: refreshSpin
            Kirigami.FormData.label: i18n("Check usage every:")
            from: 1
            to: 60
            textFromValue: (value) => i18np("%1 minute", "%1 minutes", value)
        }

        Item {
            Kirigami.FormData.isSection: true
        }

        // With nothing ticked here or on the Spending page, both windows show.
        QQC2.CheckBox {
            id: fiveHourCheck
            Kirigami.FormData.label: i18n("Show:")
            text: i18n("5-hour window")
        }

        QQC2.CheckBox {
            id: sevenDayCheck
            text: i18n("7-day window")
        }

        QQC2.CheckBox {
            id: extraCheck
            text: i18n("Extra usage, while it's turned on for your plan")
        }

        QQC2.RadioButton {
            Kirigami.FormData.label: i18n("Numbers show:")
            text: i18n("How much is used")
            checked: !cfg_showRemaining
            onToggled: cfg_showRemaining = !checked
        }

        QQC2.RadioButton {
            text: i18n("How much is left")
            checked: cfg_showRemaining
            onToggled: cfg_showRemaining = checked
        }

        Item {
            Kirigami.FormData.isSection: true
        }

        // Used for ring colours and for notifications.
        QQC2.SpinBox {
            id: warningSpin
            Kirigami.FormData.label: i18n("Warning level:")
            from: 1
            to: 100
            textFromValue: (value) => i18n("%1% used", value)
        }

        QQC2.SpinBox {
            id: criticalSpin
            Kirigami.FormData.label: i18n("Critical level:")
            from: 1
            to: 100
            textFromValue: (value) => i18n("%1% used", value)
        }

        QQC2.CheckBox {
            id: colorByLevelCheck
            text: i18n("Change ring colour at these levels")
        }
    }
}
