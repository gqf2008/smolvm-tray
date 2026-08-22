// Programmatic 32x32 tray icon: dark monitor bezel + colored status screen +
// white outline + stand/base + power dot. Pure RGBA painting, no image assets.
// Geometry ported from the C# SetIcon (16px logical coords scaled to 32px here).
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

pub const GREEN: (u8, u8, u8) = (60, 180, 75);
pub const YELLOW: (u8, u8, u8) = (220, 180, 40);
pub const RED: (u8, u8, u8) = (200, 60, 60);

pub fn render_icon(rgb: (u8, u8, u8)) -> Image {
    const SIZE: usize = 32;
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(SIZE as u32, SIZE as u32);
    let px = buf.make_mut_bytes();

    let bezel = color(43, 47, 54);
    let outline = color(255, 255, 255);
    let stand = color(150, 160, 170);

    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut acc = [0u8; 4]; // accumulator in the painting order

            paint(&mut acc, inrounded_rect(fx, fy, 16.0, 17.0, 11.0, 9.0, 4.0), bezel);
            paint(&mut acc, inrect(fx, fy, 9.0, 12.0, 14.0, 9.0), [rgb.0, rgb.1, rgb.2, 255]); // status screen
            // white outline: 1px band around the screen rect (C# 1.4f stroke)
            let ob = inrect_band(fx, fy, 9.0, 12.0, 14.0, 9.0, 1.0);
            paint(&mut acc, ob, outline);
            paint(&mut acc, inrect(fx, fy, 15.0, 23.0, 3.0, 4.0), stand);
            paint(&mut acc, inrect(fx, fy, 11.0, 26.0, 11.0, 2.0), stand);
            paint(&mut acc, incircle(fx, fy, 23.5, 11.5, 1.5), [rgb.0, rgb.1, rgb.2, 255]); // power dot

            let i = (y * SIZE + x) * 4;
            px[i] = acc[0];
            px[i + 1] = acc[1];
            px[i + 2] = acc[2];
            px[i + 3] = acc[3];
        }
    }
    Image::from_rgba8(buf)
}

fn color(r: u8, g: u8, b: u8) -> [u8; 4] {
    [r, g, b, 255]
}

/// Alpha-composite `src` onto `acc` with coverage `cov` (0..1).
fn paint(acc: &mut [u8; 4], cov: f32, src: [u8; 4]) {
    if cov <= 0.0 {
        return;
    }
    let a = src[3] as f32 / 255.0 * cov.min(1.0);
    if a <= 0.0 {
        return;
    }
    let out_a = a + (acc[3] as f32 / 255.0) * (1.0 - a);
    if out_a <= 0.0 {
        return;
    }
    for c in 0..3 {
        let s = src[c] as f32;
        let d = acc[c] as f32;
        acc[c] = ((s * a + d * (acc[3] as f32 / 255.0) * (1.0 - a)) / out_a).round() as u8;
    }
    acc[3] = (out_a * 255.0).round() as u8;
}

fn coverage(dist: f32) -> f32 {
    // 1px anti-aliasing: inside (negative dist) fully covered, 1px falloff.
    (0.5 - dist).clamp(0.0, 1.0)
}

/// Signed distance to an axis-aligned box (negative = inside).
fn box_sdf(dx: f32, dy: f32, hw: f32, hh: f32) -> f32 {
    let qx = dx.abs() - hw;
    let qy = dy.abs() - hh;
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0)
}

fn inrect(x: f32, y: f32, rx: f32, ry: f32, w: f32, h: f32) -> f32 {
    coverage(box_sdf(x - (rx + w / 2.0), y - (ry + h / 2.0), w / 2.0, h / 2.0))
}

/// 1px band around the border of a rect (matches the C# outline stroke).
fn inrect_band(x: f32, y: f32, rx: f32, ry: f32, w: f32, h: f32, bw: f32) -> f32 {
    let inner = inrect(x, y, rx + bw, ry + bw, w - 2.0 * bw, h - 2.0 * bw);
    let outer = inrect(x, y, rx, ry, w, h);
    (outer - inner).clamp(0.0, 1.0)
}

fn inrounded_rect(x: f32, y: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    coverage(box_sdf(x - cx, y - cy, hw - r, hh - r) - r)
}

fn incircle(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> f32 {
    coverage(((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezel_corner_transparent_screen_center_colored() {
        let img = render_icon(GREEN);
        let buf = img.to_rgba8().unwrap();
        let w = buf.width() as usize;
        let bytes = buf.as_bytes();
        let at = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            (bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3])
        };
        // outside the bezel: fully transparent
        assert_eq!(at(0, 0).3, 0);
        // center of the status screen: green, opaque
        let (r, g, b, a) = at(16, 16);
        assert!(a > 200, "screen alpha {a}");
        assert!((g as i32 - 180).abs() < 40, "green channel {g}");
        assert!(r < 120, "red channel {r}");
        assert!(b < 120, "blue channel {b}");
    }

    #[test]
    fn colors_have_alpha() {
        for c in [GREEN, YELLOW, RED] {
            let img = render_icon(c);
            assert!(img.to_rgba8().is_some());
        }
    }
}
