// SPDX-License-Identifier: AGPL-3.0-or-later
//! CSV decoding and row mapping for the native batch-import workflow.
//!
//! This module deliberately stops before page lookup or queue insertion: those
//! require the migrated native page catalog and production queue contracts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One parsed row using the legacy batch-import field contract.
pub struct BulkImportRow {
    /// One-based source record number, including the header record.
    pub row_index: usize,
    /// Stable content-page identifier, not a display name.
    pub page_id: String,
    /// Optional source-image path or URL.
    pub source_image: Option<String>,
    /// Prompt used to create the still image.
    pub image_prompt: Option<String>,
    /// Optional prompt for extending an image to a 9:16 frame.
    pub expand_916_prompt: Option<String>,
    /// Prompt used to animate the source image.
    pub video_prompt: Option<String>,
    /// Optional publication title.
    pub title: Option<String>,
    /// Optional publication caption.
    pub caption: Option<String>,
    /// Parsed hashtags, with leading `#` preserved if supplied.
    pub hashtags: Vec<String>,
    /// Lowercase destination platforms.
    pub platforms: Vec<String>,
    /// Optional `auto` or `review` publication mode.
    pub post_mode: Option<String>,
    /// Optional scheduled publication time as supplied by the CSV.
    pub post_time: Option<String>,
    /// Optional per-row output destination override.
    pub output_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// CSV syntax error with the one-based record where parsing failed.
pub struct CsvError {
    /// One-based CSV record number (not physical line number).
    pub record: usize,
    /// Stable, non-localized parse description.
    pub message: &'static str,
}

/// Parses comma, tab, or semicolon separated records with RFC-4180 quoting,
/// including escaped quotes and embedded newlines in quoted fields.
pub fn parse_csv(input: &str) -> Result<Vec<Vec<String>>, CsvError> {
    let delimiter = detect_delimiter(input);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = input.chars().peekable();
    let mut quoted = false;
    let mut after_quote = false;
    let mut record = 1;

    while let Some(ch) = chars.next() {
        if quoted {
            match ch {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => {
                    quoted = false;
                    after_quote = true;
                }
                _ => field.push(ch),
            }
            continue;
        }

        match ch {
            '"' if field.is_empty() && !after_quote => quoted = true,
            '"' => {
                return Err(CsvError {
                    record,
                    message: "unexpected quote in an unquoted field",
                });
            }
            c if c == delimiter => {
                row.push(std::mem::take(&mut field));
                after_quote = false;
            }
            '\n' | '\r' => {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                if !row.iter().all(|cell| cell.trim().is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                after_quote = false;
                record += 1;
            }
            c if after_quote && !c.is_whitespace() => {
                return Err(CsvError {
                    record,
                    message: "unexpected text after a closing quote",
                });
            }
            c if after_quote => {}
            c => field.push(c),
        }
    }

    if quoted {
        return Err(CsvError {
            record,
            message: "unterminated quoted field",
        });
    }
    if !field.is_empty() || !row.is_empty() || after_quote {
        row.push(field);
        if !row.iter().all(|cell| cell.trim().is_empty()) {
            rows.push(row);
        }
    }
    Ok(rows)
}

/// Converts a header matrix into the stable import-row schema used by ArtCraft.
pub fn map_rows(matrix: &[Vec<String>]) -> Vec<BulkImportRow> {
    let Some(header) = matrix.first() else {
        return Vec::new();
    };
    let normalized: Vec<String> = header
        .iter()
        .enumerate()
        .map(|(index, value)| {
            normalize_header(if index == 0 {
                value.trim_start_matches('\u{feff}')
            } else {
                value
            })
        })
        .collect();
    let find = |aliases: &[&str]| {
        normalized
            .iter()
            .position(|name| aliases.iter().any(|alias| name == alias))
    };
    let columns = Columns {
        page: find(&["page", "pageid", "pagename"]),
        source: find(&["sourceimage", "imageurl", "imagepath", "source"]),
        image: find(&["imageprompt", "promptimage", "prompt"]),
        expand: find(&["expand916prompt", "expandprompt", "916"]),
        video: find(&["videoprompt", "promptvideo", "motionprompt"]),
        title: find(&["title", "headline", "posttitle"]),
        caption: find(&["caption", "description", "content"]),
        hashtags: find(&["hashtags", "tags", "hashtag"]),
        platforms: find(&["platforms", "platform", "social"]),
        mode: find(&["postmode", "mode"]),
        time: find(&["posttime", "slot", "schedule"]),
        output: find(&["outputoverride", "output", "destination"]),
    };

    matrix
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(index, values)| {
            if values.iter().all(|value| value.trim().is_empty()) {
                return None;
            }
            let get = |column: Option<usize>| {
                column
                    .and_then(|i| values.get(i))
                    .map(|v| v.trim())
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned)
            };
            let split = |value: Option<String>, separators: &[char]| {
                value
                    .map(|value| {
                        value
                            .split(separators)
                            .map(str::trim)
                            .filter(|part| !part.is_empty())
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            Some(BulkImportRow {
                row_index: index + 1,
                page_id: get(columns.page).unwrap_or_default(),
                source_image: get(columns.source),
                image_prompt: get(columns.image),
                expand_916_prompt: get(columns.expand),
                video_prompt: get(columns.video),
                title: get(columns.title),
                caption: get(columns.caption),
                hashtags: split(get(columns.hashtags), &[',', ' ', ';']),
                platforms: split(get(columns.platforms), &[',', ';', '|'])
                    .into_iter()
                    .map(|p| p.to_ascii_lowercase())
                    .collect(),
                post_mode: get(columns.mode).map(|v| v.to_ascii_lowercase()),
                post_time: get(columns.time),
                output_override: get(columns.output),
            })
        })
        .collect()
}

/// Performs row-level checks that don't need a live page catalog or backend.
pub fn validate_rows(rows: &[BulkImportRow]) -> Vec<(usize, &'static str)> {
    let mut errors = Vec::new();
    for row in rows {
        if row.page_id.trim().is_empty() {
            errors.push((row.row_index, "page_id is required"));
        }
        if row
            .image_prompt
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
            && row
                .video_prompt
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            && row
                .source_image
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            errors.push((
                row.row_index,
                "provide an image prompt, video prompt, or source image",
            ));
        }
        if row
            .platforms
            .iter()
            .any(|p| !matches!(p.as_str(), "facebook" | "tiktok" | "youtube"))
        {
            errors.push((
                row.row_index,
                "platform must be facebook, tiktok, or youtube",
            ));
        }
        if row
            .post_mode
            .as_deref()
            .is_some_and(|mode| !matches!(mode, "auto" | "review"))
        {
            errors.push((row.row_index, "post mode must be auto or review"));
        }
    }
    errors
}

fn normalize_header(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn detect_delimiter(input: &str) -> char {
    let first = input.lines().next().unwrap_or_default();
    let mut in_quotes = false;
    let mut counts = [(',', 0usize), ('\t', 0), (';', 0)];
    let mut chars = first.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            if in_quotes && chars.peek() == Some(&'"') {
                chars.next();
            } else {
                in_quotes = !in_quotes;
            }
        } else if !in_quotes {
            if let Some((_, count)) = counts.iter_mut().find(|(delimiter, _)| *delimiter == ch) {
                *count += 1;
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| *count > 0)
        .map(|(delimiter, _)| delimiter)
        .unwrap_or(',')
}

struct Columns {
    page: Option<usize>,
    source: Option<usize>,
    image: Option<usize>,
    expand: Option<usize>,
    video: Option<usize>,
    title: Option<usize>,
    caption: Option<usize>,
    hashtags: Option<usize>,
    platforms: Option<usize>,
    mode: Option<usize>,
    time: Option<usize>,
    output: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_delimiters_escaped_quotes_and_multiline_fields() {
        let parsed = parse_csv(
            "page_id,image_prompt,caption\r\np1,\"girl, neon\",\"She said \"\"hi\"\"\nagain\"\r\n",
        )
        .expect("valid CSV");
        assert_eq!(parsed[1], ["p1", "girl, neon", "She said \"hi\"\nagain"]);
    }

    #[test]
    fn detects_semicolon_and_tab_delimiters() {
        assert_eq!(
            parse_csv("page_id;video_prompt\np1;move").expect("CSV")[1],
            ["p1", "move"]
        );
        assert_eq!(
            parse_csv("page_id\tvideo_prompt\np1\tmove").expect("TSV")[1],
            ["p1", "move"]
        );
    }

    #[test]
    fn accepts_utf8_bom_on_the_first_header() {
        let matrix = parse_csv("\u{feff}page_id,image_prompt\np1,Hero").expect("CSV");
        assert_eq!(map_rows(&matrix)[0].page_id, "p1");
    }

    #[test]
    fn reports_malformed_quote_instead_of_silently_truncating() {
        assert_eq!(
            parse_csv("page_id,prompt\np1,\"unfinished")
                .expect_err("unclosed quote")
                .record,
            2
        );
        assert!(parse_csv("page_id,prompt\np1,un\"expected").is_err());
    }

    #[test]
    fn maps_aliases_and_validates_rows_without_backend_assumptions() {
        let matrix = parse_csv(
            "Page Name;Image Prompt;Platforms;Post Mode\nDemo;Hero;TikTok|YouTube;review",
        )
        .expect("CSV");
        let rows = map_rows(&matrix);
        assert_eq!(rows[0].page_id, "Demo");
        assert_eq!(rows[0].platforms, ["tiktok", "youtube"]);
        assert!(validate_rows(&rows).is_empty());
    }

    #[test]
    fn rejects_missing_source_and_unsupported_platform() {
        let matrix = parse_csv("page_id,platforms\np1,instagram").expect("CSV");
        let rows = map_rows(&matrix);
        assert_eq!(validate_rows(&rows).len(), 2);
    }
}
