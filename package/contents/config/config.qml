import QtQuick
import org.kde.plasma.configuration

ConfigModel {
    ConfigCategory {
        name: i18n("General")
        icon: "configure"
        source: "ConfigGeneral.qml"
    }
    ConfigCategory {
        name: i18n("Appearance")
        icon: "preferences-desktop-color"
        source: "ConfigAppearance.qml"
    }
    ConfigCategory {
        name: i18n("Spending")
        icon: "wallet-open"
        source: "ConfigSpending.qml"
    }
    ConfigCategory {
        name: i18n("Notifications")
        icon: "notifications"
        source: "ConfigNotifications.qml"
    }
}
