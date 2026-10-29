// SPDX-License-Identifier: AGPL-3.0-or-later
//! Presentation/preflight helpers; never fabricate a candidate or change durations.

pub(crate) fn source_problem(duration: f64, minimum: f64) -> Option<String> {
    if !duration.is_finite() || duration <= 0.0 {
        return Some("Không đọc được thời lượng video. Chọn lại video nguồn.".into());
    }
    if duration < 4.0 {
        return Some(format!(
            "Video chỉ dài {duration:.1} giây. Chọn video dài ít nhất 4 giây; tính năng này không kéo dài video."
        ));
    }
    (duration < minimum).then(|| format!("Video dài {duration:.1} giây, ngắn hơn đoạn tối thiểu {minimum:.0} giây. Chọn video dài hơn hoặc giảm độ dài tối thiểu."))
}

pub(crate) fn result_summary(count: usize) -> String {
    if count == 0 {
        "Chưa tìm được đoạn phù hợp. Thử giảm độ dài tối thiểu hoặc chọn video dài hơn; chưa tạo video mới.".into()
    } else {
        format!("Tìm được {count} đoạn. Xem lại, chọn đoạn muốn giữ rồi xuất thành video riêng.")
    }
}

pub(crate) fn analysis_limits(warnings: &[String]) -> String {
    let mut notes = Vec::new();
    if warnings
        .iter()
        .any(|w| w.contains("No installed Whisper model"))
    {
        notes.push("Chưa có bộ nhận giọng nói: chưa chọn đoạn theo lời thoại. Có thể nhập phụ đề để phân tích lời nói.");
    }
    if warnings.iter().any(|w| w.contains("Center-crop fallback")) {
        notes.push("Khung dọc đang cắt giữa ảnh; xem lại để tránh mất người trong khung.");
    }
    if notes.is_empty() && !warnings.is_empty() {
        notes.push("Có lưu ý khi phân tích; xem Thông tin kiểm tra trước khi xuất.");
    }
    notes.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_second_source_cannot_satisfy_fifteen_second_clip() {
        let problem = source_problem(5.0, 15.0).unwrap();
        assert!(problem.contains("5.0 giây"));
        assert!(problem.contains("15 giây"));
        assert!(problem.contains("giảm độ dài tối thiểu"));
        assert!(source_problem(5.0, 4.0).is_none());
        assert!(source_problem(15.0, 15.0).is_none());
    }

    #[test]
    fn invalid_or_unsupported_source_has_actionable_failure() {
        for duration in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            assert!(source_problem(duration, 15.0).unwrap().contains("Chọn lại"));
        }
        assert!(
            source_problem(3.9, 15.0)
                .unwrap()
                .contains("ít nhất 4 giây")
        );
    }

    #[test]
    fn zero_results_never_pretend_to_have_created_videos() {
        assert!(result_summary(0).contains("chưa tạo video mới"));
        assert!(result_summary(3).contains("Tìm được 3 đoạn"));
        assert!(!result_summary(3).contains("Đã xuất"));
    }

    #[test]
    fn technical_details_stay_separate_but_relevant_limits_are_visible() {
        let warnings = vec![
            "No installed Whisper model: visual/audio-only analysis".into(),
            "Center-crop fallback: face/subject tracking is not enabled".into(),
        ];
        let original = warnings.clone();
        let message = analysis_limits(&warnings);
        assert!(message.contains("chưa chọn đoạn theo lời thoại"));
        assert!(message.contains("cắt giữa ảnh"));
        assert!(!message.contains("Whisper"));
        assert_eq!(
            warnings, original,
            "original diagnostics must remain available"
        );
        assert!(analysis_limits(&[]).is_empty());
    }
}
