// SPDX-License-Identifier: AGPL-3.0-or-later
//! Persistent shot data shared by direction, QA, subtitles and assembly.
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};

/// A local, ordered film. File references never represent remote uploads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Storyboard {
    pub version: u32,
    pub title: String,
    pub shots: Vec<Shot>,
    #[serde(default)]
    pub output: Option<PathBuf>,
}
/// Independently editable shot and its locally owned media layers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Shot {
    pub id: String,
    pub title: String,
    pub narration: String,
    pub duration: f64,
    #[serde(default)]
    pub direction: Direction,
    #[serde(default)]
    pub background: Option<PathBuf>,
    #[serde(default)]
    pub character: Option<PathBuf>,
    #[serde(default)]
    pub voice: Option<PathBuf>,
    #[serde(default)]
    pub music: Option<PathBuf>,
    #[serde(default)]
    pub generated: Option<PathBuf>,
    #[serde(default)]
    pub preview: Option<PathBuf>,
    #[serde(default)]
    pub source_note: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub layers: Vec<VisualLayer>,
    #[serde(default)]
    pub asset_notes: String,
    #[serde(default)]
    pub thumbnail: Option<PathBuf>,
}
/// An independently positioned visual layer. Coordinates are frame percentages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct VisualLayer {
    pub path: PathBuf,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub opacity: f64,
}
/// Parameters that can be applied per shot or reused as a template.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Direction {
    pub style: String,
    pub setting: String,
    pub camera: String,
    pub motion: String,
    pub lighting: String,
    pub expression: String,
    pub sound: String,
    pub prompt: String,
}
impl Default for Direction {
    fn default() -> Self {
        Self {
            style: "2D".into(),
            setting: String::new(),
            camera: "Toàn cảnh".into(),
            motion: "Đứng yên".into(),
            lighting: "Theo bối cảnh".into(),
            expression: "Tự nhiên".into(),
            sound: String::new(),
            prompt: String::new(),
        }
    }
}
impl Default for Storyboard {
    fn default() -> Self {
        Self {
            version: 1,
            title: "Phim mới".into(),
            shots: vec![],
            output: None,
        }
    }
}
impl Shot {
    /// New stable identity; never derived from row position.
    pub fn new(narration: &str, duration: f64) -> Self {
        Self {
            id: format!("{:032x}", rand::random::<u128>()),
            title: "Cảnh mới".into(),
            narration: narration.into(),
            duration,
            direction: Direction::default(),
            background: None,
            character: None,
            voice: None,
            music: None,
            generated: None,
            preview: None,
            source_note: String::new(),
            caption: String::new(),
            layers: vec![],
            asset_notes: String::new(),
            thumbnail: None,
        }
    }
    /// Provider prompt from the same visible shot fields used in the editor.
    pub fn visual_prompt(&self) -> String {
        let d = &self.direction;
        format!(
            "Style: {}. Setting: {}. Framing: {}. Motion: {}. Lighting: {}. Character expression: {}. {}",
            d.style, d.setting, d.camera, d.motion, d.lighting, d.expression, d.prompt
        )
    }
    /// Missing files/narration, invalid duration and short voice estimates.
    pub fn warnings(&self) -> Vec<String> {
        let mut issues = vec![];
        if !self.duration.is_finite() || !(0.5..=600.0).contains(&self.duration) {
            issues.push("Thời lượng phải từ 0,5 đến 600 giây".into());
        }
        if self
            .generated
            .as_ref()
            .or(self.background.as_ref())
            .is_none()
        {
            issues.push("Chưa gắn hình hoặc video".into());
        }
        for (label, path) in [
            ("Hình/video", &self.background),
            ("Nhân vật", &self.character),
            ("Giọng đọc", &self.voice),
            ("Nhạc", &self.music),
            ("Video tạo", &self.generated),
        ] {
            if path.as_ref().is_some_and(|p| !p.is_file()) {
                issues.push(format!("Không tìm thấy file {label}"));
            }
        }
        for (i, layer) in self.layers.iter().enumerate() {
            if !layer.path.is_file() {
                issues.push(format!("Không tìm thấy lớp hình {}", i + 1));
            }
        }
        if !self.narration.trim().is_empty() && self.voice.is_none() {
            issues.push("Có lời dẫn nhưng chưa gắn/tạo giọng đọc".into());
        }
        if self.narration.split_whitespace().count() as f64 / 3.0 > self.duration + 1.0 {
            issues.push("Lời dẫn có thể dài hơn cảnh; đo giọng hoặc tăng số giây".into());
        }
        issues
    }
}
impl Storyboard {
    /// Bound persisted/untrusted AI data before exposing it to the UI or renderer.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.shots.len() > 300 || self.title.len() > 1024 {
            return Err("Tệp phân cảnh không đúng phiên bản hoặc quá lớn".into());
        }
        let mut ids = HashSet::new();
        for s in &self.shots {
            if s.id.is_empty()
                || s.id.len() > 128
                || !ids.insert(&s.id)
                || !s.duration.is_finite()
                || !(0.5..=600.0).contains(&s.duration)
                || s.narration.len() > 16000
                || s.title.len() > 1024
                || s.layers.len() > 16
                || s.asset_notes.len() > 16000
                || s.layers.iter().any(|l| {
                    !l.x.is_finite()
                        || !l.y.is_finite()
                        || !l.width.is_finite()
                        || !l.opacity.is_finite()
                        || !(-100.0..=100.0).contains(&l.x)
                        || !(-100.0..=100.0).contains(&l.y)
                        || !(1.0..=200.0).contains(&l.width)
                        || !(0.0..=1.0).contains(&l.opacity)
                })
                || serde_json::to_vec(&s.direction)
                    .map_err(|_| "Không đọc được chỉ đạo")?
                    .len()
                    > 32768
            {
                return Err("Cảnh bị trùng ID, sai thời lượng hoặc vượt giới hạn nội dung".into());
            }
        }
        if self.shots.iter().map(|s| s.duration).sum::<f64>() > 14400.0 {
            return Err("Tổng thời lượng vượt 4 giờ".into());
        }
        Ok(())
    }
    /// Convert user-authored paragraphs into editable scenes without inventing AI output.
    pub fn from_script(script: &str, total_seconds: f64) -> Result<Self, String> {
        let paragraphs: Vec<_> = script
            .split('\n')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if paragraphs.is_empty() || paragraphs.len() > 300 {
            return Err("Nhập kịch bản, mỗi đoạn một cảnh (tối đa 300)".into());
        }
        let duration = (total_seconds / paragraphs.len() as f64).clamp(0.5, 600.0);
        let board = Self {
            shots: paragraphs
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    let mut s = Shot::new(text, duration);
                    s.title = format!("Cảnh {}", i + 1);
                    s
                })
                .collect(),
            ..Self::default()
        };
        board.validate()?;
        Ok(board)
    }
    /// Accept only an AI shot proposal, never file paths or completion states.
    pub fn from_ai(text: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Proposal {
            title: String,
            shots: Vec<ProposedShot>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct ProposedShot {
            title: String,
            narration: String,
            duration: f64,
            #[serde(default)]
            direction: Direction,
        }
        if text.len() > 512 * 1024 {
            return Err("Kịch bản AI vượt giới hạn".into());
        }
        let text = text
            .trim()
            .strip_prefix("```json")
            .or_else(|| text.trim().strip_prefix("```"))
            .unwrap_or(text.trim())
            .trim()
            .trim_end_matches("```")
            .trim();
        let p: Proposal = serde_json::from_str(text)
            .map_err(|_| "AI chưa trả đúng cấu trúc cảnh; bản hiện có vẫn được giữ")?;
        let board = Self {
            version: 1,
            title: p.title,
            shots: p
                .shots
                .into_iter()
                .map(|s| {
                    let mut shot = Shot::new(&s.narration, s.duration);
                    shot.title = s.title;
                    shot.direction = s.direction;
                    shot
                })
                .collect(),
            output: None,
        };
        if board.shots.is_empty() {
            return Err("AI trả danh sách cảnh rỗng".into());
        }
        board.validate()?;
        Ok(board)
    }
    /// Subtitle timing and render duration share a single source of truth.
    pub fn srt(&self) -> Result<String, String> {
        self.validate()?;
        let mut start = 0.0;
        let mut index = 0;
        let mut result = String::new();
        for shot in &self.shots {
            let text = if shot.caption.trim().is_empty() {
                shot.narration.trim()
            } else {
                shot.caption.trim()
            };
            if !text.is_empty() {
                index += 1;
                result.push_str(&format!(
                    "{index}\n{} --> {}\n{}\n\n",
                    timestamp(start),
                    timestamp(start + shot.duration),
                    text.replace("-->", "→")
                ));
            }
            start += shot.duration;
        }
        Ok(result)
    }
}
fn timestamp(seconds: f64) -> String {
    let t = (seconds * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        t / 3600000,
        t / 60000 % 60,
        t / 1000 % 60,
        t % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silent_shots_preserve_subtitle_gaps_and_layers_are_bounded() {
        let mut board = Storyboard::from_script("Một\nHai", 4.0).unwrap();
        board.shots[0].narration.clear();
        assert!(
            board
                .srt()
                .unwrap()
                .contains("00:00:02,000 --> 00:00:04,000")
        );
        board.shots[0].layers.push(VisualLayer {
            path: "image.png".into(),
            x: 0.0,
            y: 0.0,
            width: 30.0,
            opacity: 1.1,
        });
        assert!(board.validate().is_err());
    }
    #[test]
    fn reorder_keeps_ids_and_srt_tracks_new_durations() {
        let mut b = Storyboard::from_script("Xin chào\nSản phẩm mới", 12.0).unwrap();
        let id = b.shots[1].id.clone();
        b.shots.swap(0, 1);
        b.shots[0].duration = 2.25;
        assert_eq!(b.shots[0].id, id);
        assert!(b.srt().unwrap().contains("00:00:02,250 --> 00:00:08,250"));
        b.shots[1].id = id;
        assert!(b.validate().is_err());
    }
    #[test]
    fn ai_cannot_fabricate_local_artifacts() {
        assert!(Storyboard::from_ai(r#"{"title":"AI","shots":[{"title":"a","narration":"b","duration":5,"generated":"C:/fake.mp4"}]}"#).is_err());
        let b = Storyboard::from_ai(
            r#"{"title":"AI","shots":[{"title":"a","narration":"b","duration":5}]}"#,
        )
        .unwrap();
        assert!(b.shots[0].generated.is_none());
        assert_eq!(b.shots[0].warnings().len(), 2);
    }
}
