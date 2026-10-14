// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native controls for Bulk CSV Import workflow.

use crate::ui::{App, StudioBulk, StudioBulkRowItem};
use concat_studio::bulk_import::{map_rows, parse_csv, validate_rows};
use slint::{ComponentHandle, ModelRc, VecModel};

const CSV_SAMPLE: &str = "\
page_id,source_image,image_prompt,expand_916_prompt,video_prompt,title,caption,hashtags,platforms,post_mode
page_demo,,Cinematic portrait of a cyberpunk warrior in neon rain,,Hyper realistic camera zoom into neon reflection,Cyberpunk 2099,Amazing futuristic world #cyberpunk #neon,cyberpunk neon ai,facebook|tiktok,auto
";

pub(crate) fn bind(app: &App) {
    let weak = app.as_weak();
    app.global::<StudioBulk>().on_load_sample(move || {
        if let Some(app) = weak.upgrade() {
            let bulk = app.global::<StudioBulk>();
            bulk.set_csv_content(CSV_SAMPLE.into());
            bulk.set_status("Đã nạp mẫu CSV chuẩn.".into());
        }
    });

    let weak = app.as_weak();
    app.global::<StudioBulk>().on_validate(move || {
        let Some(app) = weak.upgrade() else { return };
        let bulk = app.global::<StudioBulk>();
        let content = bulk.get_csv_content().to_string();
        match parse_csv(&content) {
            Ok(matrix) => {
                let rows = map_rows(&matrix);
                let errors = validate_rows(&rows);
                let error_map: std::collections::HashMap<usize, &str> =
                    errors.into_iter().collect();

                let mut row_items = Vec::new();
                let mut valid_cnt = 0;
                let mut err_cnt = 0;

                for row in &rows {
                    let err = error_map.get(&row.row_index).copied().unwrap_or("");
                    let is_valid = err.is_empty();
                    if is_valid {
                        valid_cnt += 1;
                    } else {
                        err_cnt += 1;
                    }

                    row_items.push(StudioBulkRowItem {
                        row_index: row.row_index as i32,
                        page_id: row.page_id.clone().into(),
                        title: row.title.clone().unwrap_or_default().into(),
                        prompt: row
                            .video_prompt
                            .clone()
                            .or_else(|| row.image_prompt.clone())
                            .unwrap_or_default()
                            .into(),
                        platforms: row.platforms.join(", ").into(),
                        valid: is_valid,
                        error: err.into(),
                    });
                }

                bulk.set_total_rows(rows.len() as i32);
                bulk.set_valid_count(valid_cnt);
                bulk.set_error_count(err_cnt);
                bulk.set_rows(ModelRc::new(VecModel::from(row_items)));
                bulk.set_status(
                    format!(
                        "Đã kiểm tra {} dòng: {} hợp lệ, {} lỗi.",
                        rows.len(),
                        valid_cnt,
                        err_cnt
                    )
                    .into(),
                );
            }
            Err(e) => {
                bulk.set_status(
                    format!("Lỗi cú pháp CSV tại bản ghi {}: {}", e.record, e.message).into(),
                );
            }
        }
    });

    let weak = app.as_weak();
    app.global::<StudioBulk>().on_commit(move || {
        let Some(app) = weak.upgrade() else { return };
        let bulk = app.global::<StudioBulk>();
        let count = bulk.get_valid_count();
        bulk.set_status(format!("{} dòng đã kiểm tra. Chưa tạo job: chức năng gửi hàng loạt lên Promax chưa được nối; dùng Xưởng phim AI để tạo từng job.", count).into());
    });
}
