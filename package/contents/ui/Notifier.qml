import QtQuick
import org.kde.notification

// One desktop notification. Kept in its own file and created on demand, so a
// system without the KNotifications QML module just loses alerts instead of
// failing to load the whole widget.
Notification {
    property string level: "normal"     // "low", "normal" or "high"

    componentName: "plasma_workspace"
    eventId: "notification"
    iconName: "speedometer"
    autoDelete: false
    urgency: level === "high" ? Notification.HighUrgency
        : level === "low" ? Notification.LowUrgency : Notification.NormalUrgency
    onClosed: destroy()
}
