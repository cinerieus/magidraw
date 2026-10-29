// Contrast-limited local histogram equalization plus local detail recovery.
// Preserving each tile's mean keeps solid dark regions dark.
pub fn balance(gray: &[i32], visible: &[bool], width: u32, height: u32, strength: u8) -> Vec<i32> {
    if strength == 0 || width == 0 || height == 0 {
        return gray.to_vec();
    }
    let width = width as usize;
    let height = height as usize;
    let tile_w = width.div_ceil(8).max(1);
    let tile_h = height.div_ceil(8).max(1);
    let tiles_x = width.div_ceil(tile_w);
    let tiles_y = height.div_ceil(tile_h);
    let mut maps = vec![[0u8; 256]; tiles_x * tiles_y];

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let mut histogram = [0u32; 256];
            let mut count = 0u32;
            for y in ty * tile_h..((ty + 1) * tile_h).min(height) {
                for x in tx * tile_w..((tx + 1) * tile_w).min(width) {
                    let index = y * width + x;
                    if visible[index] {
                        histogram[gray[index].clamp(0, 255) as usize] += 1;
                        count += 1;
                    }
                }
            }
            let map = &mut maps[ty * tiles_x + tx];
            if count == 0 {
                for (value, entry) in map.iter_mut().enumerate() {
                    *entry = value as u8;
                }
                continue;
            }

            let limit = (count * 2).div_ceil(256).max(1);
            let mut excess = 0;
            for bin in &mut histogram {
                if *bin > limit {
                    excess += *bin - limit;
                    *bin = limit;
                }
            }
            let spread = excess / 256;
            let remainder = excess % 256;
            let mut cumulative = 0u32;
            let mut raw = [0i32; 256];
            for value in 0..256 {
                cumulative += histogram[value] + spread + u32::from(value < remainder as usize);
                raw[value] = (cumulative * 255 / count) as i32;
            }

            // Recenter the equalized tile so uniform ink/background keeps its tone.
            let mut original_sum = 0i64;
            let mut mapped_sum = 0i64;
            for y in ty * tile_h..((ty + 1) * tile_h).min(height) {
                for x in tx * tile_w..((tx + 1) * tile_w).min(width) {
                    let index = y * width + x;
                    if visible[index] {
                        let value = gray[index].clamp(0, 255) as usize;
                        original_sum += value as i64;
                        mapped_sum += raw[value] as i64;
                    }
                }
            }
            let shift = ((original_sum - mapped_sum) / count as i64) as i32;
            for value in 0..256 {
                map[value] = (raw[value] + shift).clamp(0, 255) as u8;
            }
        }
    }

    let amount = strength.min(100) as i32;
    let pitch = width + 1;
    let mut sums = vec![0u64; pitch * (height + 1)];
    let mut counts = vec![0u32; pitch * (height + 1)];
    for y in 0..height {
        let mut row_sum = 0u64;
        let mut row_count = 0u32;
        for x in 0..width {
            let index = y * width + x;
            if visible[index] {
                row_sum += gray[index].clamp(0, 255) as u64;
                row_count += 1;
            }
            sums[(y + 1) * pitch + x + 1] = sums[y * pitch + x + 1] + row_sum;
            counts[(y + 1) * pitch + x + 1] = counts[y * pitch + x + 1] + row_count;
        }
    }
    let radius = (width.min(height) / 64).clamp(4, 32);
    let mut output = gray.to_vec();
    for y in 0..height {
        let fy = ((y as f32 + 0.5) / tile_h as f32 - 0.5).clamp(0.0, (tiles_y - 1) as f32);
        let y0 = fy.floor() as usize;
        let y1 = (y0 + 1).min(tiles_y - 1);
        let wy = fy.fract();
        for x in 0..width {
            let index = y * width + x;
            if !visible[index] {
                continue;
            }
            let fx = ((x as f32 + 0.5) / tile_w as f32 - 0.5).clamp(0.0, (tiles_x - 1) as f32);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(tiles_x - 1);
            let wx = fx.fract();
            let value = gray[index].clamp(0, 255) as usize;
            let top = maps[y0 * tiles_x + x0][value] as f32 * (1.0 - wx)
                + maps[y0 * tiles_x + x1][value] as f32 * wx;
            let bottom = maps[y1 * tiles_x + x0][value] as f32 * (1.0 - wx)
                + maps[y1 * tiles_x + x1][value] as f32 * wx;
            let adjusted = (top * (1.0 - wy) + bottom * wy).round() as i32;
            let x_start = x.saturating_sub(radius);
            let y_start = y.saturating_sub(radius);
            let x_end = (x + radius + 1).min(width);
            let y_end = (y + radius + 1).min(height);
            let sum = sums[y_end * pitch + x_end] + sums[y_start * pitch + x_start]
                - sums[y_start * pitch + x_end]
                - sums[y_end * pitch + x_start];
            let count = counts[y_end * pitch + x_end] + counts[y_start * pitch + x_start]
                - counts[y_start * pitch + x_end]
                - counts[y_end * pitch + x_start];
            let local_detail = gray[index] - (sum / count.max(1) as u64) as i32;
            output[index] =
                ((gray[index] * (100 - amount) + (adjusted + local_detail) * amount + 50) / 100)
                    .clamp(0, 255);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::balance;

    #[test]
    fn uniform_dark_region_stays_dark() {
        let gray = vec![25; 64 * 64];
        let result = balance(&gray, &vec![true; gray.len()], 64, 64, 100);
        assert!(result.iter().all(|&value| value < 90));
    }

    #[test]
    fn local_detail_gains_contrast() {
        let mut gray = vec![120; 64 * 64];
        for y in 8..56 {
            gray[y * 64 + 24] = 110;
        }
        let result = balance(&gray, &vec![true; gray.len()], 64, 64, 100);
        assert!(result[32 * 64 + 24] < result[32 * 64 + 30]);
        assert!(result[32 * 64 + 30] - result[32 * 64 + 24] > 10);
    }
}
