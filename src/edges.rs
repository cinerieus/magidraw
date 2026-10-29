use std::collections::VecDeque;

use image::{GrayImage, Luma};
use imageproc::{
    contrast::adaptive_threshold,
    edges::canny,
    filter::bilateral::{GaussianEuclideanColorDistance, bilateral_filter},
};

const DIRECTIONS: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

pub struct EdgeRegions {
    pub paths: Vec<Vec<(u32, u32)>>,
    pub interior: Vec<bool>,
}

fn enclosed_pixels(edges: &[bool], visible: &[bool], width: usize, height: usize) -> Vec<bool> {
    let mut outside = vec![false; edges.len()];
    let mut queue = VecDeque::new();
    let mut seed = |index: usize| {
        if !edges[index] && !outside[index] {
            outside[index] = true;
            queue.push_back(index);
        }
    };
    for x in 0..width {
        seed(x);
        seed((height - 1) * width + x);
    }
    for y in 0..height {
        seed(y * width);
        seed(y * width + width - 1);
    }
    while let Some(index) = queue.pop_front() {
        let x = index % width;
        let y = index / width;
        for next in [
            (x > 0).then_some(index.saturating_sub(1)),
            (x + 1 < width).then_some(index + 1),
            (y > 0).then_some(index.saturating_sub(width)),
            (y + 1 < height).then_some(index + width),
        ]
        .into_iter()
        .flatten()
        {
            if !edges[next] && !outside[next] {
                outside[next] = true;
                queue.push_back(next);
            }
        }
    }
    edges
        .iter()
        .zip(visible)
        .zip(outside)
        .map(|((&edge, &shown), exterior)| shown && !edge && !exterior)
        .collect()
}

fn neighbor(index: usize, direction: usize, width: usize, height: usize) -> Option<usize> {
    let x = index % width;
    let y = index / width;
    let (dx, dy) = DIRECTIONS[direction];
    let nx = x as i32 + dx;
    let ny = y as i32 + dy;
    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
        None
    } else {
        Some(ny as usize * width + nx as usize)
    }
}

fn connected(
    index: usize,
    direction: usize,
    mask: &[bool],
    width: usize,
    height: usize,
) -> Option<usize> {
    let next = neighbor(index, direction, width, height)?;
    if !mask[next] {
        return None;
    }
    let (dx, dy) = DIRECTIONS[direction];
    if dx != 0 && dy != 0 {
        let x_side = (index as i32 + dx) as usize;
        let y_side = (index as i32 + dy * width as i32) as usize;
        if mask[x_side] || mask[y_side] {
            return None;
        }
    }
    Some(next)
}

fn trace(mask: &[bool], width: usize, height: usize, min_points: usize) -> Vec<Vec<(u32, u32)>> {
    let mut degree = vec![0u8; mask.len()];
    for (i, &on) in mask.iter().enumerate() {
        if on {
            degree[i] = (0..8)
                .filter(|&d| connected(i, d, mask, width, height).is_some())
                .count() as u8;
        }
    }
    let mut visited = vec![0u8; mask.len()];
    let mut paths = Vec::new();
    for junction_pass in [true, false] {
        for start in 0..mask.len() {
            if !mask[start] || (degree[start] != 2) != junction_pass {
                continue;
            }
            for first_direction in 0..8 {
                if visited[start] & (1 << first_direction) != 0
                    || connected(start, first_direction, mask, width, height).is_none()
                {
                    continue;
                }
                let mut path = vec![(start % width, start / width)];
                let mut current = start;
                let mut direction = first_direction;
                while let Some(next) = connected(current, direction, mask, width, height) {
                    visited[current] |= 1 << direction;
                    visited[next] |= 1 << ((direction + 4) % 8);
                    current = next;
                    path.push((current % width, current / width));
                    if degree[current] != 2 {
                        break;
                    }
                    let Some(next_direction) = (0..8).find(|&d| {
                        visited[current] & (1 << d) == 0
                            && connected(current, d, mask, width, height).is_some()
                    }) else {
                        break;
                    };
                    direction = next_direction;
                }
                if path.len() >= min_points {
                    paths.push(
                        path.into_iter()
                            .map(|(x, y)| (x as u32, y as u32))
                            .collect(),
                    );
                }
            }
        }
    }
    if min_points == 1 {
        for (index, &on) in mask.iter().enumerate() {
            if on && degree[index] == 0 {
                paths.push(vec![(
                    index as u32 % width as u32,
                    index as u32 / width as u32,
                )]);
            }
        }
    }
    paths
}

pub fn edge_regions(
    gray: &[i32],
    visible: &[bool],
    width: u32,
    height: u32,
    threshold: u8,
    smoothing: u8,
) -> EdgeRegions {
    if width < 3 || height < 3 {
        return EdgeRegions {
            paths: Vec::new(),
            interior: vec![false; gray.len()],
        };
    }
    let source = GrayImage::from_fn(width, height, |x, y| {
        Luma([gray[(y * width + x) as usize] as u8])
    });
    let filtered = if smoothing == 0 {
        source
    } else {
        bilateral_filter(
            &source,
            smoothing,
            smoothing as f32 + 1.0,
            GaussianEuclideanColorDistance::new(25.0),
        )
    };
    let high = threshold as f32 * 3.0;
    let edge_image = canny(&filtered, high * 0.4, high);
    let mask: Vec<bool> = edge_image
        .pixels()
        .enumerate()
        .map(|(i, pixel)| visible[i] && pixel[0] != 0)
        .collect();
    let interior = enclosed_pixels(&mask, visible, width as usize, height as usize);
    let paths = trace(&mask, width as usize, height as usize, 4);
    EdgeRegions { paths, interior }
}

pub fn ink_mask(
    gray: &[i32],
    visible: &[bool],
    width: u32,
    height: u32,
    threshold: u8,
    detail: u8,
    invert: bool,
) -> Vec<bool> {
    let adaptive = if detail == 0 {
        None
    } else {
        let source = GrayImage::from_fn(width, height, |x, y| {
            let value = gray[(y * width + x) as usize].clamp(0, 255) as u8;
            Luma([if invert { 255 - value } else { value }])
        });
        let radius = (width.min(height) / 64).clamp(4, 32);
        Some(adaptive_threshold(&source, radius, detail as i32))
    };
    gray.iter()
        .zip(visible)
        .enumerate()
        .map(|(index, (&value, &shown))| {
            shown
                && ((value < threshold as i32) != invert
                    || adaptive
                        .as_ref()
                        .is_some_and(|image| image.as_raw()[index] == 0))
        })
        .collect()
}

pub fn ink_contours(selected: &[bool], width: u32, height: u32) -> Vec<Vec<(u32, u32)>> {
    let width = width as usize;
    let height = height as usize;
    let mut mask = selected.to_vec();
    if width < 3 || height < 3 {
        return trace(&mask, width, height, 1);
    }
    let mut active = Vec::new();
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let i = y * width + x;
            if mask[i]
                && [i - width, i + width, i - 1, i + 1]
                    .iter()
                    .any(|&neighbor| !mask[neighbor])
            {
                active.push(i);
            }
        }
    }
    let mut queued = vec![false; mask.len()];
    loop {
        let mut changed = false;
        for pass in 0..2 {
            let mut remove = Vec::new();
            for &i in &active {
                if !mask[i] {
                    continue;
                }
                let ring = [
                    mask[i - width],
                    mask[i - width + 1],
                    mask[i + 1],
                    mask[i + width + 1],
                    mask[i + width],
                    mask[i + width - 1],
                    mask[i - 1],
                    mask[i - width - 1],
                ];
                let count = ring.iter().filter(|&&on| on).count();
                let transitions = (0..8).filter(|&k| !ring[k] && ring[(k + 1) % 8]).count();
                if !(2..=6).contains(&count) || transitions != 1 {
                    continue;
                }
                let erase_connection = if pass == 0 {
                    (ring[0] && ring[2] && ring[4]) || (ring[2] && ring[4] && ring[6])
                } else {
                    (ring[0] && ring[2] && ring[6]) || (ring[0] && ring[4] && ring[6])
                };
                if !erase_connection {
                    remove.push(i);
                }
            }
            changed |= !remove.is_empty();
            for &i in &remove {
                mask[i] = false;
            }
            let mut next = Vec::with_capacity(active.len());
            for &i in &active {
                if mask[i] {
                    queued[i] = true;
                    next.push(i);
                }
            }
            for i in remove {
                let x = i % width;
                let y = i / width;
                for ny in y.saturating_sub(1)..=(y + 1).min(height - 2) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(width - 2) {
                        let neighbor = ny * width + nx;
                        if nx > 0 && ny > 0 && mask[neighbor] && !queued[neighbor] {
                            queued[neighbor] = true;
                            next.push(neighbor);
                        }
                    }
                }
            }
            for &i in &next {
                queued[i] = false;
            }
            active = next;
        }
        if !changed {
            break;
        }
    }
    trace(&mask, width, height, 1)
}

#[cfg(test)]
mod tests {
    use super::{ink_contours, ink_mask, trace};

    #[test]
    fn short_lines_and_single_pixels_are_kept() {
        let mut mask = vec![false; 25];
        mask[11] = true;
        mask[12] = true;
        mask[24] = true;
        let paths = trace(&mask, 5, 5, 1);
        assert!(paths.iter().any(|path| path.len() == 2));
        assert!(paths.iter().any(|path| path == &[(4, 4)]));
    }

    #[test]
    fn thick_ink_thins_past_old_iteration_limit() {
        let mut mask = vec![false; 100 * 100];
        for y in 10..90 {
            for x in 10..90 {
                mask[y * 100 + x] = true;
            }
        }
        let paths = ink_contours(&mask, 100, 100);
        assert!(!paths.is_empty());
        assert!(
            paths
                .iter()
                .flatten()
                .all(|&(x, y)| (30..=70).contains(&x) && (30..=70).contains(&y))
        );
    }

    #[test]
    fn adaptive_detail_finds_light_ink_without_filling_background() {
        let mut gray = vec![220; 32 * 32];
        for y in 4..28 {
            gray[y * 32 + 16] = 190;
        }
        let mask = ink_mask(&gray, &vec![true; gray.len()], 32, 32, 90, 15, false);
        assert!(mask[16 * 32 + 16]);
        assert!(!mask[16 * 32 + 20]);
    }
}
