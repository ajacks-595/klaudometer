.pragma library

// Ring colour palettes. Each gives one colour per usage window, one for money
// (API spend and extra usage), and the warning/critical colours rings switch
// to as usage rises. "plasma" and "mono" are built from the desktop colour
// scheme, "custom" from the user's own picks.
var presets = [
    { id: "claude", name: "Claude", fiveHour: "#D97757", sevenDay: "#6A9BCC", spend: "#8DB06E", warning: "#E8A33D", critical: "#E5484D" },
    { id: "plasma", name: "Plasma accent" },
    { id: "ocean", name: "Ocean", fiveHour: "#38BDF8", sevenDay: "#2DD4BF", spend: "#A78BFA", warning: "#FBBF24", critical: "#F43F5E" },
    { id: "sunset", name: "Sunset", fiveHour: "#FB923C", sevenDay: "#C084FC", spend: "#F472B6", warning: "#FACC15", critical: "#EF4444" },
    { id: "forest", name: "Forest", fiveHour: "#A3C55B", sevenDay: "#3FAE8C", spend: "#5FA8D3", warning: "#E9B949", critical: "#D9534F" },
    { id: "mono", name: "Monochrome" },
    { id: "custom", name: "Custom" },
];

// system: { highlight, visited, positive, neutral, negative, text, textDim, textMid } from Kirigami.Theme.
// custom: { fiveHour, sevenDay, spend, warning, critical } from the widget's settings.
function colors(id, system, custom) {
    if (id === "plasma") {
        return { fiveHour: system.highlight, sevenDay: system.visited, spend: system.positive, warning: system.neutral, critical: system.negative };
    }
    if (id === "mono") {
        return { fiveHour: system.text, sevenDay: system.textDim, spend: system.textMid, warning: system.neutral, critical: system.negative };
    }
    if (id === "custom") {
        return custom;
    }
    for (var i = 0; i < presets.length; i++) {
        if (presets[i].id === id && presets[i].fiveHour) {
            return presets[i];
        }
    }
    return presets[0];
}
