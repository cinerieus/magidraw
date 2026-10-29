use std::collections::HashMap;

use image::{DynamicImage, GenericImageView, imageops::FilterType};

use crate::{contrast, edges};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Edges,
    LineArt,
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub step: u32,
    pub threshold: u8,
    pub balance: u8,
    pub detail: u8,
    pub smoothing: u8,
    pub fill_enclosed: bool,
    pub fill_spacing: u32,
    pub mode: Mode,
    pub invert: bool,
}

#[derive(Clone)]
pub struct Stroke {
    pub points: Vec<(i32, i32)>,
}

pub struct Plan {
    pub strokes: Vec<Stroke>,
    pub fill_strokes: usize,
    pub joined_strokes: usize,
    pub preview: eframe::egui::ColorImage,
    pub image_width: u32,
    pub image_height: u32,
    pub point_step: u32,
}

fn extend_at_endpoint(
    chain: &mut Vec<(i32, i32)>,
    paths: &[Stroke],
    endpoints: &HashMap<(i32, i32), Vec<usize>>,
    used: &mut [bool],
) {
    while let Some(&end) = chain.last() {
        let Some(&next) = endpoints
            .get(&end)
            .and_then(|indices| indices.iter().find(|&&index| !used[index]))
        else {
            break;
        };
        used[next] = true;
        let points = &paths[next].points;
        if points.first() == Some(&end) {
            chain.extend(points.iter().skip(1).copied());
        } else {
            chain.extend(points.iter().rev().skip(1).copied());
        }
    }
}

fn merge_touching(paths: Vec<Stroke>) -> Vec<Stroke> {
    let mut endpoints: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (index, path) in paths.iter().enumerate() {
        if let (Some(&first), Some(&last)) = (path.points.first(), path.points.last()) {
            endpoints.entry(first).or_default().push(index);
            if last != first {
                endpoints.entry(last).or_default().push(index);
            }
        }
    }
    let mut used = vec![false; paths.len()];
    let mut merged = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if used[index] || path.points.is_empty() {
            continue;
        }
        used[index] = true;
        let mut chain = path.points.clone();
        extend_at_endpoint(&mut chain, &paths, &endpoints, &mut used);
        chain.reverse();
        extend_at_endpoint(&mut chain, &paths, &endpoints, &mut used);
        chain.reverse();
        merged.push(Stroke { points: chain });
    }
    merged
}

fn join_fill_rows(runs: &[(u32, u32, u32)], interior: &[bool], width: u32) -> Vec<Vec<(u32, u32)>> {
    let mut paths = Vec::new();
    let mut chain = Vec::new();
    let mut previous: Option<(u32, u32, u32)> = None;
    for &(start, end, y) in runs {
        let join_x = previous.and_then(|(old_start, old_end, old_y)| {
            if y <= old_y {
                return None;
            }
            let overlap_start = start.max(old_start);
            let overlap_end = end.min(old_end);
            if overlap_start > overlap_end {
                return None;
            }
            let last_x: u32 = chain.last().map(|&(x, _)| x)?;
            let x = last_x.clamp(overlap_start, overlap_end);
            (old_y..=y)
                .all(|row| interior[(row * width + x) as usize])
                .then_some(x)
        });
        if let Some(x) = join_x {
            let old_y = previous.map(|(_, _, row)| row).unwrap_or(y);
            if chain.last().is_some_and(|&(last_x, _)| last_x != x) {
                chain.push((x, old_y));
            }
            chain.push((x, y));
            if x == start {
                if end != start {
                    chain.push((end, y));
                }
            } else if x == end {
                chain.push((start, y));
            } else if x - start <= end - x {
                chain.push((start, y));
                chain.push((end, y));
            } else {
                chain.push((end, y));
                chain.push((start, y));
            }
        } else {
            if !chain.is_empty() {
                paths.push(std::mem::take(&mut chain));
            }
            chain.push((start, y));
            if end != start {
                chain.push((end, y));
            }
        }
        previous = Some((start, end, y));
    }
    if !chain.is_empty() {
        paths.push(chain);
    }
    paths
}

fn simplify(path: &[(u32, u32)], tolerance: f64) -> Vec<(u32, u32)> {
    if path.len() <= 2 {
        return path.to_vec();
    }
    let mut keep = vec![false; path.len()];
    keep[0] = true;
    keep[path.len() - 1] = true;
    let mut pending = vec![(0, path.len() - 1)];
    while let Some((first, last)) = pending.pop() {
        if last <= first + 1 {
            continue;
        }
        let (ax, ay) = (path[first].0 as f64, path[first].1 as f64);
        let (bx, by) = (path[last].0 as f64, path[last].1 as f64);
        let length_sq = (bx - ax).powi(2) + (by - ay).powi(2);
        let mut farthest = first;
        let mut largest = 0.0;
        for (index, &(x, y)) in path.iter().enumerate().take(last).skip(first + 1) {
            let (px, py) = (x as f64, y as f64);
            let distance_sq = if length_sq == 0.0 {
                (px - ax).powi(2) + (py - ay).powi(2)
            } else {
                let t =
                    (((px - ax) * (bx - ax) + (py - ay) * (by - ay)) / length_sq).clamp(0.0, 1.0);
                (px - ax - t * (bx - ax)).powi(2) + (py - ay - t * (by - ay)).powi(2)
            };
            if distance_sq > largest {
                largest = distance_sq;
                farthest = index;
            }
        }
        if largest > tolerance * tolerance {
            keep[farthest] = true;
            pending.push((first, farthest));
            pending.push((farthest, last));
        }
    }
    path.iter()
        .zip(keep)
        .filter_map(|(&point, selected)| selected.then_some(point))
        .collect()
}

fn preview(strokes: &[Stroke], image_width: u32, image_height: u32) -> eframe::egui::ColorImage {
    let divisor = image_width.max(image_height).div_ceil(1024).max(1);
    let width = image_width.div_ceil(divisor) as usize;
    let height = image_height.div_ceil(divisor) as usize;
    let mut pixels = vec![eframe::egui::Color32::WHITE; width * height];
    let mut mark = |x: i32, y: i32| {
        let x = (x / divisor as i32).clamp(0, width as i32 - 1) as usize;
        let y = (y / divisor as i32).clamp(0, height as i32 - 1) as usize;
        pixels[y * width + x] = eframe::egui::Color32::BLACK;
    };
    for stroke in strokes {
        let Some(&first) = stroke.points.first() else {
            continue;
        };
        mark(first.0, first.1);
        for pair in stroke.points.windows(2) {
            let (mut x, mut y) = pair[0];
            let (end_x, end_y) = pair[1];
            let dx = (end_x - x).abs();
            let dy = -(end_y - y).abs();
            let sx = if x < end_x { 1 } else { -1 };
            let sy = if y < end_y { 1 } else { -1 };
            let mut error = dx + dy;
            loop {
                mark(x, y);
                if x == end_x && y == end_y {
                    break;
                }
                let twice = 2 * error;
                if twice >= dy {
                    error += dy;
                    x += sx;
                }
                if twice <= dx {
                    error += dx;
                    y += sy;
                }
            }
        }
    }
    eframe::egui::ColorImage::new([width, height], pixels)
}

pub fn plan(image: &DynamicImage, settings: Settings) -> Plan {
    let source_w = image.width().max(1);
    let source_h = image.height().max(1);
    let scale =
        (settings.width as f64 / source_w as f64).min(settings.height as f64 / source_h as f64);
    let image_width = (source_w as f64 * scale).round().max(1.0) as u32;
    let image_height = (source_h as f64 * scale).round().max(1.0) as u32;
    let analysis_step = image_width.max(image_height).div_ceil(2048).max(1);
    let grid_w = image_width.div_ceil(analysis_step);
    let grid_h = image_height.div_ceil(analysis_step);
    let resized = image.resize_exact(grid_w, grid_h, FilterType::Lanczos3);
    let mut gray = vec![255i32; (grid_w * grid_h) as usize];
    let mut visible = vec![false; gray.len()];
    for y in 0..grid_h {
        for x in 0..grid_w {
            let pixel = resized.get_pixel(x, y);
            let i = (y * grid_w + x) as usize;
            visible[i] = pixel[3] >= 128;
            let luminance =
                (299 * pixel[0] as i32 + 587 * pixel[1] as i32 + 114 * pixel[2] as i32) / 1000;
            gray[i] = (luminance * pixel[3] as i32 + 255 * (255 - pixel[3] as i32)) / 255;
        }
    }
    let gray = contrast::balance(&gray, &visible, grid_w, grid_h, settings.balance);
    let map_point = |x: u32, y: u32| {
        (
            (x * analysis_step).min(image_width - 1) as i32,
            (y * analysis_step).min(image_height - 1) as i32,
        )
    };
    let mut strokes = Vec::new();
    let mut fill_strokes = 0;
    let mut joined_strokes = 0;
    match settings.mode {
        Mode::Edges | Mode::LineArt => {
            let (contours, interior) = if settings.mode == Mode::Edges {
                let regions = edges::edge_regions(
                    &gray,
                    &visible,
                    grid_w,
                    grid_h,
                    settings.threshold,
                    settings.smoothing,
                );
                (regions.paths, Some(regions.interior))
            } else {
                let selected = edges::ink_mask(
                    &gray,
                    &visible,
                    grid_w,
                    grid_h,
                    settings.threshold,
                    settings.detail,
                    false,
                );
                (edges::ink_contours(&selected, grid_w, grid_h), None)
            };
            if settings.fill_enclosed
                && let Some(interior) = interior
            {
                let stride = settings.fill_spacing.div_ceil(analysis_step).max(1) as usize;
                let mut runs = Vec::new();
                for y in (0..grid_h).step_by(stride) {
                    let mut x = 0;
                    while x < grid_w {
                        if !interior[(y * grid_w + x) as usize] {
                            x += 1;
                            continue;
                        }
                        let start = x;
                        while x < grid_w && interior[(y * grid_w + x) as usize] {
                            x += 1;
                        }
                        runs.push((start, x - 1, y));
                    }
                }
                let fill_paths = join_fill_rows(&runs, &interior, grid_w);
                fill_strokes = fill_paths.len();
                joined_strokes += runs.len() - fill_strokes;
                strokes.extend(fill_paths.into_iter().map(|path| Stroke {
                    points: path.into_iter().map(|(x, y)| map_point(x, y)).collect(),
                }));
            }
            let tolerance = (settings.step as f64 * 0.4).clamp(0.7, 1.5);
            let contour_strokes: Vec<_> = contours
                .into_iter()
                .map(|contour| Stroke {
                    points: simplify(&contour, tolerance)
                        .into_iter()
                        .map(|(x, y)| map_point(x, y))
                        .collect(),
                })
                .collect();
            let before = contour_strokes.len();
            let merged = merge_touching(contour_strokes);
            joined_strokes += before - merged.len();
            strokes.extend(merged);
        }
        Mode::Dark => {
            let selected = edges::ink_mask(
                &gray,
                &visible,
                grid_w,
                grid_h,
                settings.threshold,
                settings.detail,
                settings.invert,
            );
            let stride = settings.step.div_ceil(analysis_step).max(1) as usize;
            for y in (0..grid_h).step_by(stride) {
                let mut x = 0;
                while x < grid_w {
                    let i = (y * grid_w + x) as usize;
                    if !selected[i] {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < grid_w {
                        let i = (y * grid_w + x) as usize;
                        if !selected[i] {
                            break;
                        }
                        x += 1;
                    }
                    let end = x - 1;
                    let mut points = vec![map_point(start, y)];
                    if end > start {
                        points.push(map_point(end, y));
                    }
                    strokes.push(Stroke { points });
                }
            }
        }
    }
    let preview = preview(&strokes, image_width, image_height);
    Plan {
        strokes,
        fill_strokes,
        joined_strokes,
        preview,
        image_width,
        image_height,
        point_step: settings.step.max(1),
    }
}
