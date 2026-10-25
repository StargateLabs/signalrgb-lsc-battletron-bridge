// Encoder dei datapoint colore (replica della logica in src/App.vue):
// - dp24 (colour_data): h(2B) sat/10(1B) val/10(1B) + 00000100
// - dp61 (paint_colour_data): "0004" + 4 zone HSV(6B) + spliceNum(2B) + stripMap

/// rgbToHsv dell'app: h 0..359, s 0..1000, v 0..1000 (interi troncati).
pub fn rgb_to_hsv(rgb: [u8; 3]) -> (u16, u16, u16) {
    let (r, g, b) = (rgb[0] as f64, rgb[1] as f64, rgb[2] as f64);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let v = max / 255.0;
    let s = if max == 0.0 { 0.0 } else { 1.0 - min / max };
    let h = if max == min {
        0.0
    } else if max == r && g >= b {
        60.0 * ((g - b) / (max - min))
    } else if max == r && g < b {
        60.0 * ((g - b) / (max - min)) + 360.0
    } else if max == g {
        60.0 * ((b - r) / (max - min)) + 120.0
    } else {
        60.0 * ((r - g) / (max - min)) + 240.0
    };
    (
        h as u16,
        (s * 1000.0) as u16,
        (v * 1000.0) as u16,
    )
}

fn byte2(v: u16) -> Vec<u8> {
    vec![(v >> 8) as u8, v as u8]
}
fn byte1(v: u16) -> u8 {
    v as u8
}

/// dp24: 8 byte
pub fn dp24_value(hsv: (u16, u16, u16)) -> Vec<u8> {
    let (h, s, v) = hsv;
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&byte2(h));
    out.push(byte1(s / 10));
    out.push(byte1(v / 10));
    out.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    out
}

/// dp61: "0004" + 4× HSV(6B) + spliceNum(2B) + stripMap (1B per posizione).
/// strip_map: valori 1..4 (zona di appartenenza di ogni pixel fisico).
/// rotated: montaggio a 180° → stripMap invertita (replica del loop al contrario).
pub fn dp61_value(zones: &[(u16, u16, u16); 4], strip_map: &[u8], rotated: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + 24 + 2 + strip_map.len());
    out.extend_from_slice(&[0x00, 0x04]);
    for z in zones {
        out.extend_from_slice(&byte2(z.0));
        out.extend_from_slice(&byte2(z.1));
        out.extend_from_slice(&byte2(z.2));
    }
    out.extend_from_slice(&byte2(strip_map.len() as u16));
    if rotated {
        for b in strip_map.iter().rev() {
            out.push(*b);
        }
    } else {
        out.extend_from_slice(strip_map);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_of_pure_red() {
        assert_eq!(rgb_to_hsv([255, 0, 0]), (0, 1000, 1000));
    }

    #[test]
    fn hsv_of_black_is_zero() {
        assert_eq!(rgb_to_hsv([0, 0, 0]), (0, 0, 0));
    }

    #[test]
    fn hsv_of_white() {
        let (h, s, v) = rgb_to_hsv([255, 255, 255]);
        assert_eq!(v, 1000);
        assert_eq!(s, 0);
        assert_eq!(h, 0);
    }

    #[test]
    fn dp24_pure_red_matches_original() {
        // 0000 64 64 00000100
        assert_eq!(dp24_value((0, 1000, 1000)), hex::decode("0000646400000100").unwrap());
    }

    #[test]
    fn dp61_prefix_and_lengths() {
        let zones = [(0, 0, 0); 4];
        let map = vec![1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4];
        let v = dp61_value(&zones, &map, false);
        assert_eq!(&v[0..2], &[0x00, 0x04]);
        assert_eq!(v.len(), 2 + 24 + 2 + 12);
        assert_eq!(&v[2 + 24..2 + 24 + 2], &byte2(12));
    }

    #[test]
    fn dp61_rotated_reverses_map() {
        let zones = [(0, 0, 0); 4];
        let map = vec![1, 2, 3, 4];
        let normal = dp61_value(&zones, &map, false);
        let rotated = dp61_value(&zones, &map, true);
        assert_eq!(&normal[2 + 24 + 2..], &map);
        let expect: Vec<u8> = map.iter().rev().cloned().collect();
        assert_eq!(&rotated[2 + 24 + 2..], &expect);
    }
}
