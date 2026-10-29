// SPDX-License-Identifier: AGPL-3.0-or-later
//! Human-readable labels for actual durable steps, independent of a UI toolkit.
use crate::ai_control::{Status, Step};

/// User-facing name; unknown capabilities retain their real name.
pub fn title(tool: &str) -> &str {
    match tool {
        "nexora.pages" => "Đọc hồ sơ Nexora",
        "production.generate" => "Grok tạo ảnh và video",
        "batch.expand" => "Xếp hàng sản xuất theo lô",
        "ai.plan" => "Lập kế hoạch",
        "media.inspect" => "Kiểm tra tư liệu",
        "library.list" => "Đọc thư viện video",
        "ai.script" => "Viết lời thoại",
        "ai.edit" => "Chuẩn bị thao tác chỉnh sửa",
        "editor.apply" => "Áp dụng vào dự án",
        "factory.render" => "Dựng video và âm thanh",
        "artifact.validate" => "Kiểm tra video kết quả",
        _ => tool,
    }
}

/// Keep the per-video grouping visible without exposing an internal step ID.
pub fn step_title(step: &Step) -> String {
    let suffix=step.id.rsplit("--").next().unwrap_or(&step.id);
    let number=suffix.strip_prefix('v').and_then(|s|s.split_once('-')).and_then(|(n,_)|n.parse::<usize>().ok());
    match number {Some(n) if (1..=100).contains(&n)=>format!("Video {n} · {}",title(&step.tool)),_=>title(&step.tool).into()}
}

/// Describes state without equating an uncertain dispatch with a successful edit.
pub fn state(status: Status) -> &'static str {
    match status {
        Status::Pending => "Đang xếp hàng",
        Status::Running => "Đang thực hiện",
        Status::WaitingInput => "Cần bạn bổ sung",
        Status::WaitingApproval => "Chờ bạn cho phép",
        Status::Paused => "Đã tạm dừng",
        Status::Done => "Đã hoàn thành",
        Status::Failed => "Chưa thực hiện được",
        Status::Cancelled => "Đã hủy",
        Status::Unknown => "Chưa xác định kết quả · cần kiểm tra",
    }
}

/// Explain the action, not a fabricated result.
pub fn detail(step: &Step) -> String {
    if step.status == Status::Done {
        if let Some(output) = step.output.as_ref() {
            if step.tool == "ai.script" {
                if let Some(text) = output["text"].as_str() {
                    return text.chars().take(1600).collect();
                }
            }
            if step.tool == "library.list" {
                if let Some(entries) = output["entries"].as_array() {
                    return format!("Đã đọc {} video được lưu trên máy.", entries.len());
                }
            }
            if step.tool == "factory.render" {
                if let Some(seconds) = output["duration"].as_f64() {
                    return format!(
                        "Đã dựng video dài {seconds:.1} giây. Bước tiếp theo sẽ kiểm tra tệp kết quả."
                    );
                }
            }
        }
    }
    match step.tool.as_str() {
        "nexora.pages" => "Đọc hồ sơ và preset thật từ kết nối Nexora hiện có.",
        "production.generate" => "Dùng đúng hồ sơ Nexora để tạo ảnh, mở rộng 9:16 và tạo video qua Grok; lấy kết quả về máy.",
        "batch.expand" => "Tạo các khối riêng cho từng video: nội dung, tư liệu Grok, edit, lời đọc, phụ đề và kiểm tra.",
        "ai.plan" => "Đọc yêu cầu và chia thành các bước mà công cụ hiện có thực hiện được.",
        "media.inspect" => "Đọc thông tin các ảnh, video và âm thanh bạn đã đính kèm.",
        "library.list" => "Đọc danh sách video đã lưu trên máy.",
        "ai.script" => "Nhờ AI viết lời thoại từ yêu cầu và thông tin đã cung cấp.",
        "ai.edit" => "Chuẩn bị lệnh chỉnh sửa cho đúng dự án và phiên bản đang mở.",
        "editor.apply" => "Thực hiện lệnh đã kiểm tra trong dự án; có thể hoàn tác bằng Undo.",
        "factory.render" => {
            "Ghép tư liệu, chuyển động ảnh, lời đọc và phụ đề bằng trình dựng hiện có."
        }
        "artifact.validate" => {
            "Đọc video xuất ra và kiểm tra thời lượng, hình ảnh, luồng âm thanh."
        }
        _ => "Thực hiện công cụ theo kế hoạch đã lưu.",
    }
    .into()
}

/// Preserve actionable errors, translating the common permission blocker.
pub fn error(error: Option<&str>) -> String {
    match error {
        Some("Model did not return a strict plan response") => {
            "AI đã trả lời nhưng kế hoạch thiếu trường bắt buộc hoặc sai định dạng. Chưa có thao tác nào từ kế hoạch này được chạy. Quay lại yêu cầu, bổ sung tên và thông tin sản phẩm nếu cần, rồi tạo yêu cầu mới.".into()
        }
        Some(e) if e.starts_with("[unsupported_capability]")
            || e == "Generative video / animation creation for characters is not supported by the available tools (generative video capability is missing).: generative video" => {
            "Bộ công cụ hiện tại chưa hỗ trợ yêu cầu này. Bấm ← Quay lại yêu cầu để sửa nội dung. Nếu cần hoạt hình nhân vật, phải kết nối công cụ tạo video có chuyển động; bấm Tiếp tục không bổ sung được công cụ còn thiếu.".into()
        }
        Some("Missing permission: ai.request") => {
            "Cần cho phép AI truy cập web hoặc API trước khi tiếp tục.".into()
        }
        Some(e) => e.into(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncertainty_and_waiting_are_never_presented_as_completed() {
        assert!(state(Status::Unknown).contains("Chưa xác định"));
        assert!(state(Status::WaitingInput).contains("Cần bạn"));
        assert_ne!(state(Status::Running), state(Status::Done));
    }
    #[test]
    fn permission_blocker_explains_a_user_action() {
        let message = error(Some("Missing permission: ai.request"));
        assert!(message.contains("cho phép"));
        assert!(!message.contains("ai.request"));
        assert_eq!(title("future.tool"), "future.tool");
    }
    #[test]
    fn unsupported_plan_explains_why_resume_will_not_help() {
        for message in [
            "[unsupported_capability] no tool: generative video",
            "Generative video / animation creation for characters is not supported by the available tools (generative video capability is missing).: generative video",
        ] {
            let shown = error(Some(message));
            assert!(shown.contains("Quay lại yêu cầu"));
            assert!(!shown.contains("capability"));
        }
        assert_eq!(error(Some("unrelated error")), "unrelated error");
        assert!(error(Some("Model did not return a strict plan response")).contains("thiếu trường bắt buộc"));
    }
}
