// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
//! Bounded inspection of actual native timeline output, never source thumbnails.

use crate::preview::{FrameSpec, Monitor};
use concat_export::ExportClip;
use concat_project::{DocumentSettings, Project};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Native contact sheet with row-major tiles and exact sample times.
pub struct ContactSheet {
    /// RGBA8 sheet, rendered at full project geometry before downsampling.
    pub pixels: Vec<u8>,
    /// Sheet dimensions, not source dimensions.
    pub width: u32,
    /// Sheet height.
    pub height: u32,
    /// Row-major timeline timestamps, in seconds. Blank last-row slots are not samples.
    pub times: Vec<f64>,
}

/// Cover native cut boundaries and changed regions in both projects.
/// Refuse excess coverage rather than silently omit a changed region.
pub fn inspection_times(
    before: &Project,
    after: &Project,
    requested: f64,
    fps: f64,
) -> Result<Vec<f64>, String> {
    if !requested.is_finite() || requested < 0.0 || !fps.is_finite() || fps <= 0.0 {
        return Err("Invalid inspection time or frame rate".into());
    }
    let mut times = vec![requested];
    for (project, other) in [(before, after), (after, before)] {
        for clip in &project.active().clips {
            let end = clip.start + clip.duration;
            if !clip.start.is_finite()
                || clip.start < 0.0
                || !end.is_finite()
                || clip.duration <= 0.0
            {
                return Err("Invalid native clip timing".into());
            }
            times.extend([clip.start, (end - 1.0 / fps).max(clip.start), end]);
            if !other.active().clips.iter().any(|old| old == clip) {
                times.push(clip.start + clip.duration / 2.0);
            }
        }
    }
    times.sort_by(f64::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    if times.len() > 16 {
        return Err(format!(
            "Inspection needs {} samples; limit is 16. Use a smaller proposal, not an incomplete preview.",
            times.len()
        ));
    }
    Ok(times)
}

fn sheet_geometry(width: u32, height: u32, count: usize) -> Result<(u32, u32, u32, u32), String> {
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > 8_388_608
        || !(1..=16).contains(&count)
    {
        return Err("Contact sheet exceeds full-frame or sample budget".into());
    }
    let scale = (320.0 / f64::from(width.max(height))).min(1.0);
    let tile_width = (f64::from(width) * scale).round().max(1.0) as u32;
    let tile_height = (f64::from(height) * scale).round().max(1.0) as u32;
    let columns = count.min(3) as u32;
    let rows = (count as u32).div_ceil(columns);
    Ok((
        columns * tile_width,
        rows * tile_height,
        tile_width,
        tile_height,
    ))
}

/// Render the supplied flattened native timeline with title/shape frames held
/// in `monitor`. Full frame coordinates keep text/effects/export geometry exact.
/// The caller owns the isolated pool and releases it on every return path.
pub fn render_sheet(
    monitor: &Monitor,
    clips: Arc<Vec<ExportClip>>,
    settings: &DocumentSettings,
    times: &[f64],
    color_space: concat_project::model::ColorSpace,
    cancel: &AtomicBool,
) -> Result<ContactSheet, String> {
    let (width, height, tw, th) = sheet_geometry(settings.width, settings.height, times.len())?;
    if times.iter().any(|t| !t.is_finite() || *t < 0.0) {
        return Err("Invalid contact sheet timestamp".into());
    }
    let mut pixels = vec![0; width as usize * height as usize * 4];
    for (index, time) in times.iter().enumerate() {
        if cancel.load(Ordering::Acquire) {
            return Err("Contact sheet canceled".into());
        }
        let frame = monitor.frame_checked(
            clips.clone(),
            settings,
            FrameSpec {
                time: *time,
                width: settings.width,
                height: settings.height,
                moving: false,
                proxy: false,
                color_space,
            },
        )?;
        if frame.len() != settings.width as usize * settings.height as usize * 4 {
            return Err("Invalid compositor frame".into());
        }
        let ox = (index as u32 % (width / tw)) * tw;
        let oy = (index as u32 / (width / tw)) * th;
        for y in 0..th {
            for x in 0..tw {
                let sx = ((x as f64 + 0.5) * settings.width as f64 / tw as f64 - 0.5).max(0.0);
                let sy = ((y as f64 + 0.5) * settings.height as f64 / th as f64 - 0.5).max(0.0);
                let x0 = (sx.floor() as u32).min(settings.width - 1);
                let y0 = (sy.floor() as u32).min(settings.height - 1);
                let x1 = (x0 + 1).min(settings.width - 1);
                let y1 = (y0 + 1).min(settings.height - 1);
                let dx = sx - x0 as f64;
                let dy = sy - y0 as f64;
                let out = (((oy + y) * width + ox + x) * 4) as usize;
                for channel in 0..4 {
                    let at =
                        |xx, yy| frame[((yy * settings.width + xx) * 4) as usize + channel] as f64;
                    pixels[out + channel] = ((at(x0, y0) * (1.0 - dx) + at(x1, y0) * dx)
                        * (1.0 - dy)
                        + (at(x0, y1) * (1.0 - dx) + at(x1, y1) * dx) * dy)
                        .round() as u8;
                }
            }
        }
    }
    if cancel.load(Ordering::Acquire) {
        return Err("Contact sheet canceled".into());
    }
    Ok(ContactSheet {
        pixels,
        width,
        height,
        times: times.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_project::{Command, Editor};

    #[test]
    fn changed_regions_include_both_sides_of_cuts_and_midpoint() {
        let mut editor = Editor::new();
        let before = editor.project().clone();
        editor
            .apply(Command::AddTextClip {
                track_id: None,
                above: true,
                start: 1.0,
                style: None,
                duration: Some(3.0),
                offset_y: None,
            })
            .unwrap();
        let times = inspection_times(&before, editor.project(), 0.0, 24.0).unwrap();
        assert!(times.contains(&0.0));
        assert!(times.contains(&1.0));
        assert!(times.contains(&2.5));
        assert!(
            times
                .iter()
                .any(|time| (*time - (4.0 - 1.0 / 24.0)).abs() < 1e-8)
        );
        assert!(times.contains(&4.0));
        assert!(times.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn excessive_coverage_and_invalid_geometry_refuse_not_truncate() {
        let mut editor = Editor::new();
        let before = editor.project().clone();
        for index in 0..10 {
            editor
                .apply(Command::AddTextClip {
                    track_id: None,
                    above: true,
                    start: index as f64 * 4.0,
                    style: None,
                    duration: Some(2.0),
                    offset_y: None,
                })
                .unwrap();
        }
        assert!(inspection_times(&before, editor.project(), 0.0, 24.0).is_err());
        assert!(sheet_geometry(0, 1080, 2).is_err());
        assert!(sheet_geometry(16384, 16384, 2).is_err());
        assert!(sheet_geometry(1920, 1080, 17).is_err());
        assert_eq!(sheet_geometry(1920, 1080, 5).unwrap(), (960, 360, 320, 180));
    }
}
