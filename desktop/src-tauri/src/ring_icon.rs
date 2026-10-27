//! Draws the tray icon: up to two rings (the first meter inside, the second
//! outside) as an RGBA image, with no image libraries. Matches the Linux tray
//! icon's SVG geometry.

use klaudometer_core::Meter;

/// One ring to draw: radius and stroke width (on a 32-unit canvas), filled
/// fraction and colour.
struct Ring {
    radius: f64,
    width: f64,
    fraction: f64,
    color: [f64; 3],
}

fn parse_color(hex: &str) -> [f64; 3] {
    let hex = hex.trim_start_matches('#');
    let channel = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("88"), 16).unwrap_or(0x88) as f64 / 255.0;
    [channel(0), channel(2), channel(4)]
}

fn fraction(m: &Meter) -> f64 {
    if !m.has_data || (m.money && m.limit.is_none()) {
        0.0
    } else {
        (m.shown / 100.0).clamp(0.0, 1.0)
    }
}

/// Render `meters` (the first two are used) as a `size`×`size` RGBA image.
pub fn render(meters: &[Meter], size: u32) -> Vec<u8> {
    let rings: Vec<Ring> = match meters {
        [] => vec![],
        [only] => vec![Ring { radius: 11.0, width: 6.0, fraction: fraction(only), color: parse_color(&only.color) }],
        [inner, outer, ..] => vec![
            Ring { radius: 6.25, width: 4.5, fraction: fraction(inner), color: parse_color(&inner.color) },
            Ring { radius: 12.75, width: 4.5, fraction: fraction(outer), color: parse_color(&outer.color) },
        ],
    };
    let stale = meters.iter().take(2).any(|m| m.stale);
    let opacity = if stale { 0.5 } else { 1.0 };
    let track = [0.533, 0.533, 0.533];
    let scale = 32.0 / size as f64;
    const SAMPLES: usize = 4;

    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            // Average premultiplied colour over a 4×4 grid of sub-pixels.
            let mut acc = [0.0f64; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = (px as f64 + (sx as f64 + 0.5) / SAMPLES as f64) * scale - 16.0;
                    let y = (py as f64 + (sy as f64 + 0.5) / SAMPLES as f64) * scale - 16.0;
                    if let Some((color, alpha)) = shade(&rings, x, y, track) {
                        for c in 0..3 {
                            acc[c] += color[c] * alpha;
                        }
                        acc[3] += alpha;
                    }
                }
            }
            let n = (SAMPLES * SAMPLES) as f64;
            let alpha = acc[3] / n * opacity;
            let i = ((py * size + px) * 4) as usize;
            if alpha > 0.0 {
                for c in 0..3 {
                    rgba[i + c] = ((acc[c] / acc[3]) * 255.0).round() as u8;
                }
                rgba[i + 3] = (alpha * 255.0).round() as u8;
            }
        }
    }
    rgba
}

/// Colour and alpha at a point (relative to the centre), or None outside the rings.
fn shade(rings: &[Ring], x: f64, y: f64, track: [f64; 3]) -> Option<([f64; 3], f64)> {
    let distance = (x * x + y * y).sqrt();
    // Clockwise angle from 12 o'clock, 0..1.
    let turn = (x.atan2(-y) / std::f64::consts::TAU).rem_euclid(1.0);
    for ring in rings {
        let half = ring.width / 2.0;
        let in_band = (distance - ring.radius).abs() <= half;
        let on_arc = ring.fraction > 0.004 && turn <= ring.fraction;
        // Round caps at both ends of the arc, as in the SVG version.
        let cap = |t: f64| {
            let angle = t * std::f64::consts::TAU;
            let (cx, cy) = (ring.radius * angle.sin(), -ring.radius * angle.cos());
            ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= half
        };
        let on_cap = ring.fraction > 0.004 && ring.fraction < 0.999 && (cap(0.0) || cap(ring.fraction));
        if (in_band && on_arc) || on_cap {
            return Some((ring.color, 1.0));
        }
        if in_band {
            return Some((track, 0.4));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(shown: f64, color: &str) -> Meter {
        let mut m = klaudometer_core::build_meters(None, &Default::default(), 0.0).remove(0);
        m.has_data = true;
        m.shown = shown;
        m.color = color.into();
        m
    }

    fn pixel(img: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn draws_filled_arc_and_track() {
        let size = 64;
        // Inner ring 25% coral, outer ring 75% blue.
        let img = render(&[meter(25.0, "#D97757"), meter(75.0, "#6A9BCC")], size);
        assert_eq!(img.len(), (size * size * 4) as usize);
        // Centre and corners are transparent.
        assert_eq!(pixel(&img, size, 32, 32)[3], 0);
        assert_eq!(pixel(&img, size, 0, 0)[3], 0);
        // Outer ring at 3 o'clock (within 75%) is solid blue.
        assert_eq!(pixel(&img, size, 32 + 25, 32), [0x6A, 0x9B, 0xCC, 255]);
        // Outer ring at about 10 o'clock (past 75%) is translucent grey track.
        let track = pixel(&img, size, 11, 17);
        assert!(track[3] > 80 && track[3] < 120 && track[0] == track[1]);
        // Inner ring at 3 o'clock is just past 25%... but its round cap reaches it.
        assert_eq!(pixel(&img, size, 32 + 12, 32)[0], 0xD9);
        // Inner ring at 6 o'clock is track.
        assert!(pixel(&img, size, 32, 32 + 12)[3] < 120);
    }
}
