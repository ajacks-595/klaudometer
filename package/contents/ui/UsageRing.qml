import QtQuick
import QtQuick.Shapes
import org.kde.kirigami as Kirigami

// Circular gauge filled clockwise from 12 o'clock. It can be split into up to
// 7 segments (one per hour of the 5-hour window, one per day of the 7-day
// window) and can show a tick for how much of the time window has passed.
Item {
    id: ring

    property real value: 0      // 0..1
    property real marker: -1    // 0..1, hidden when negative
    property int segments: 1    // 1..7
    property color color: "#D97757"
    property color trackColor: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g,
                                       Kirigami.Theme.textColor.b, 0.13)
    property real thickness: Math.max(3, Math.min(width, height) * 0.1)

    readonly property real radius: (Math.min(width, height) - thickness) / 2
    readonly property real cx: width / 2
    readonly property real cy: height / 2
    readonly property int segmentCount: Math.max(1, Math.min(7, segments))
    readonly property real span: 360 / segmentCount
    // Gap between segments in degrees, a little over a pixel wide at any size.
    readonly property real gap: segmentCount > 1 && radius > 0
        ? Math.max(1.2, thickness * 0.45) / radius * 180 / Math.PI : 0

    property real shownValue: Math.max(0, Math.min(1, value))
    Behavior on shownValue {
        NumberAnimation { duration: Kirigami.Units.veryLongDuration; easing.type: Easing.OutCubic }
    }

    // Unused segment slots collapse to nothing at 12 o'clock.
    function segmentStart(i) {
        return -90 + (i < segmentCount ? i * span + gap / 2 : 0);
    }

    function trackSweep(i) {
        return i < segmentCount ? span - gap : 0;
    }

    function fillSweep(i) {
        return Math.max(0, Math.min(trackSweep(i), shownValue * 360 - i * span - gap / 2));
    }

    component Segment: PathAngleArc {
        required property var gauge
        required property int index
        property bool filled: false
        centerX: gauge.cx
        centerY: gauge.cy
        radiusX: gauge.radius
        radiusY: gauge.radius
        startAngle: gauge.segmentStart(index)
        sweepAngle: filled ? gauge.fillSweep(index) : gauge.trackSweep(index)
    }

    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            fillColor: "transparent"
            strokeColor: ring.trackColor
            strokeWidth: ring.thickness
            capStyle: ShapePath.FlatCap
            Segment { gauge: ring; index: 0 }
            Segment { gauge: ring; index: 1 }
            Segment { gauge: ring; index: 2 }
            Segment { gauge: ring; index: 3 }
            Segment { gauge: ring; index: 4 }
            Segment { gauge: ring; index: 5 }
            Segment { gauge: ring; index: 6 }
        }

        ShapePath {
            fillColor: "transparent"
            strokeColor: ring.shownValue > 0.002 ? ring.color : "transparent"
            strokeWidth: ring.thickness
            capStyle: ring.segmentCount === 1 ? ShapePath.RoundCap : ShapePath.FlatCap
            Segment { gauge: ring; index: 0; filled: true }
            Segment { gauge: ring; index: 1; filled: true }
            Segment { gauge: ring; index: 2; filled: true }
            Segment { gauge: ring; index: 3; filled: true }
            Segment { gauge: ring; index: 4; filled: true }
            Segment { gauge: ring; index: 5; filled: true }
            Segment { gauge: ring; index: 6; filled: true }
        }
    }

    Rectangle {
        readonly property real angle: (ring.marker * 360 - 90) * Math.PI / 180
        visible: ring.marker >= 0
        width: Math.max(2, ring.thickness * 0.3)
        height: ring.thickness * 1.5
        radius: width / 2
        x: ring.cx + ring.radius * Math.cos(angle) - width / 2
        y: ring.cy + ring.radius * Math.sin(angle) - height / 2
        rotation: ring.marker * 360
        color: Kirigami.Theme.textColor
        opacity: 0.8
    }
}
