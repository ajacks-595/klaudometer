import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM
import org.kde.notification

KCM.SimpleKCM {
    property alias cfg_notifyWarning: warningCheck.checked
    property alias cfg_notifyCritical: criticalCheck.checked
    property alias cfg_notifyReset: resetCheck.checked
    property alias cfg_notifyPace: paceCheck.checked
    // Shown in the labels; edited on the General page.
    property int cfg_warningThreshold
    property int cfg_criticalThreshold

    Kirigami.FormLayout {
        QQC2.CheckBox {
            id: warningCheck
            Kirigami.FormData.label: i18n("Notify me:")
            text: i18n("When a meter reaches the warning level (%1%)", cfg_warningThreshold)
        }

        QQC2.CheckBox {
            id: criticalCheck
            text: i18n("When a meter reaches the critical level (%1%)", cfg_criticalThreshold)
        }

        QQC2.CheckBox {
            id: paceCheck
            text: i18n("When a meter is on pace to run out before it resets")
        }

        QQC2.CheckBox {
            id: resetCheck
            text: i18n("When a window resets")
        }

        QQC2.Label {
            Layout.maximumWidth: Kirigami.Units.gridUnit * 22
            text: i18n("Levels are set on the General page. For API spend they are a share of your monthly budget. Each alert is sent once per window or month, even if the widget is on several panels.")
            wrapMode: Text.WordWrap
            font: Kirigami.Theme.smallFont
            opacity: 0.7
        }

        Item {
            Kirigami.FormData.isSection: true
        }

        QQC2.Button {
            text: i18n("Send Test Notification")
            icon.name: "notifications"
            onClicked: testNotification.sendEvent()
        }
    }

    Notification {
        id: testNotification
        componentName: "plasma_workspace"
        eventId: "notification"
        iconName: "speedometer"
        autoDelete: false
        title: i18n("Klaudometer")
        text: i18n("Notifications are working.")
    }
}
