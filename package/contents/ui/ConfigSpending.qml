import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM
import org.kde.plasma.plasma5support as P5Support

KCM.SimpleKCM {
    id: page

    property alias cfg_showApi: apiCheck.checked
    property alias cfg_apiBudget: budgetSpin.value

    readonly property string scriptPath: decodeURIComponent(
        Qt.resolvedUrl("../code/fetch_usage.py").toString().replace(/^file:\/\//, ""))
    readonly property string storeCommand:
        "secret-tool store --label='Klaudometer Admin API key' service klaudometer key admin"

    // null while checking, then the helper's --check-key answer.
    property var keyStatus: null

    function checkKey() {
        keyStatus = null;
        keyChecker.connectSource("python3 '" + scriptPath + "' --check-key");
    }

    Component.onCompleted: checkKey()

    P5Support.DataSource {
        id: keyChecker
        engine: "executable"
        connectedSources: []
        onNewData: (sourceName, data) => {
            disconnectSource(sourceName);
            try {
                page.keyStatus = JSON.parse(data.stdout);
            } catch (e) {
                page.keyStatus = { found: false, error: i18n("Couldn't run the key check. Is python3 installed?") };
            }
        }
    }

    Kirigami.FormLayout {
        QQC2.CheckBox {
            id: apiCheck
            Kirigami.FormData.label: i18n("Claude API:")
            text: i18n("Show this month's API spend")
        }

        QQC2.SpinBox {
            id: budgetSpin
            Kirigami.FormData.label: i18n("Monthly budget:")
            enabled: apiCheck.checked
            from: 1
            to: 1000000
            stepSize: 10
            textFromValue: (value) => "$" + value
            valueFromText: (text) => parseInt(text.replace(/[^0-9]/g, "")) || 1
            editable: true
        }

        QQC2.Label {
            Layout.maximumWidth: Kirigami.Units.gridUnit * 24
            text: i18n("Spend comes from your Console organization's cost report and updates within a few minutes. It needs an Admin API key. Individual Console accounts can't create one; set up an organization first. The budget is only used for the ring and alerts.")
            wrapMode: Text.WordWrap
            font: Kirigami.Theme.smallFont
            opacity: 0.7
        }

        Item {
            Kirigami.FormData.isSection: true
        }

        RowLayout {
            Kirigami.FormData.label: i18n("Admin API key:")
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                Layout.preferredWidth: Kirigami.Units.iconSizes.small
                Layout.preferredHeight: Kirigami.Units.iconSizes.small
                source: page.keyStatus === null ? "view-refresh"
                    : page.keyStatus.found ? "dialog-ok-apply" : "dialog-warning"
            }

            QQC2.Label {
                Layout.maximumWidth: Kirigami.Units.gridUnit * 18
                wrapMode: Text.WordWrap
                text: page.keyStatus === null ? i18n("Checking…")
                    : page.keyStatus.found
                    ? (page.keyStatus.source === "wallet" ? i18n("Found in your wallet")
                        : page.keyStatus.source === "file" ? i18n("Found in ~/.config/klaudometer/admin-key")
                        : i18n("Found in $ANTHROPIC_ADMIN_KEY"))
                    : page.keyStatus.error
            }

            QQC2.Button {
                text: i18n("Check Again")
                icon.name: "view-refresh"
                enabled: page.keyStatus !== null
                onClicked: page.checkKey()
            }
        }

        QQC2.Label {
            Layout.maximumWidth: Kirigami.Units.gridUnit * 24
            text: i18n("To store your key, run this in a terminal and paste the key when it asks for a password. The key goes into your wallet (KDE Wallet or GNOME Keyring), never into a file.")
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.maximumWidth: Kirigami.Units.gridUnit * 24
            spacing: Kirigami.Units.smallSpacing

            QQC2.TextField {
                id: commandField
                Layout.fillWidth: true
                readOnly: true
                text: page.storeCommand
                font.family: "monospace"
            }

            QQC2.Button {
                text: i18n("Copy")
                icon.name: "edit-copy"
                onClicked: {
                    commandField.selectAll();
                    commandField.copy();
                    commandField.deselect();
                }
            }
        }

        QQC2.Label {
            Layout.maximumWidth: Kirigami.Units.gridUnit * 24
            text: i18n("secret-tool comes from libsecret-tools on Debian and Ubuntu, secret-tool on openSUSE, and libsecret on Fedora and Arch. With no wallet service at all, put the key alone in ~/.config/klaudometer/admin-key and run chmod 600 on it.")
            wrapMode: Text.WordWrap
            font: Kirigami.Theme.smallFont
            opacity: 0.7
        }

        Kirigami.UrlButton {
            text: i18n("Create an Admin API key in the Claude Console")
            url: "https://platform.claude.com/settings/admin-keys"
        }
    }
}
