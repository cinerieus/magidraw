use std::collections::VecDeque;

use image::{GrayImage, Luma};
use imageproc::{
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

fn trace(mask: &[bool], width: usize, height: usize) -> Vec<Vec<(u32, u32)>> {
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
                if path.len() >= 4 {
                    paths.push(
                        path.into_iter()
                            .map(|(x, y)| (x as u32, y as u32))
                            .collect(),
                    );
                }
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
    let paths = trace(&mask, width as usize, height as usize);
    EdgeRegions { paths, interior }
}

pub fn ink_contours(
    gray: &[i32],
    visible: &[bool],
    width: u32,
    height: u32,
    threshold: u8,
) -> Vec<Vec<(u32, u32)>> {
    let width = width as usize;
    let height = height as usize;
    let mut mask: Vec<bool> = gray
        .iter()
        .zip(visible)
        .map(|(&value, &shown)| shown && value < threshold as i32)
        .collect();
    if width < 3 || height < 3 {
        return trace(&mask, width, height);
    }
    for _ in 0..32 {
        let mut changed = false;
        for pass in 0..2 {
            let mut remove = Vec::new();
            for y in 1..height - 1 {
                for x in 1..width - 1 {
                    let i = y * width + x;
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
                    if erase_connection {
                        continue;
                    }
                    remove.push(i);
                }
            }
            if !remove.is_empty() {
                changed = true;
            }
            for i in remove {
                mask[i] = false;
            }
        }
        if !changed {
            break;
        }
    }
    trace(&mask, width, height)
}
