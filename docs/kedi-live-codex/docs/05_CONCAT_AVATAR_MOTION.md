# 05 — Concat integration, Avatar Clone Studio, Motion/Emotion/Face–Hand

## Tư duy

Concat tạo asset/timeline theo kiểu offline và cung cấp JSON-RPC/MCP. OpenTalking tạo khuôn mặt nói/lip-sync realtime. Không phát mỗi frame qua `Concat export.run()` — export là đường dựng file. Thay vào đó xuất asset manifests có version từ project Concat; runtime sử dụng file media đã chuẩn bị cùng overlay scene. Nếu bản Kedi đã có playback/compositor realtime khác, reuse.

## Asset pipeline

`source footage with consent → probe/normalize → segment/tag → face/landmark/mask metadata (if model supports) → motion clips + posters + thumbnails → Concat scene template → export manifest → runtime prefetch → playout`.

**Bộ footage quay:** camera cố định hoặc metadata chuyển camera, ánh sáng ổn định, 1080p source khi có, nhiều clip 2–8s `idle`, `talk_neutral`, `smile`, `laugh_soft`, `listening`, `point_left`, `point_right`, `product_present`, `hand_detail`, `look_down`, `hold_product`, `transition_cover`; tránh quá nhiều cử động làm hỏng face crop. Quyền hình ảnh/giọng của người mẫu phải có consent, thời hạn và phạm vi sử dụng.

**Avatar manifest example** (proposed, không trùng định dạng OT):

```json
{
  "schema_version": 1,
  "avatar_id": "host_01",
  "asset_revision": 4,
  "identity_consent_id": "consent_record_reference",
  "base_video": "assets/avatar/host_01/base_idle.mp4",
  "voice_profile_id": "voice_01",
  "face_backend": "opentalking_quicktalk",
  "motion_library": "manifests/host_01_motion.json",
  "scene_templates": ["product_focus", "face_hand_split"],
  "rights": {"commercial": true, "expires_at": "2027-10-01T00:00:00Z"}
}
```

Asset loader validate file existence, checksum/content hash, format, codecs, dimensions, resolution ratio, disk/VRAM availability; avoid path traversal and unsigned remote manifest. All files reside in project-specific allowed root; never arbitrary path from live comment.

## MotionClip metadata

`clip_id`, `sha256`, `duration_ms`, `fps`, `codec`, `width/height`, `gesture`, `emotion`, `entry_pose_tag`, `exit_pose_tag`, `loopable`, `transition_in/out_ms`, `mouth_region_visible`, `occlusion_quality`, `preferred_scene`, `product_binding?`, `render_cost_hint`, `consent_ref`. Motion tags deterministic enum; LLM selects from whitelist. Missing tag -> `idle/attentive`.

## Scene Controller

- `prepare(scene, product_revision)` loads assets/prewarms overlays asynchronously.
- `commit(scene_revision, start_pts)` atomic switch at safe PTS; event `SCENE_APPLIED` includes actually rendered revision.
- `rollback_to_idle()` on invalid asset, GPU load, or mismatch; no black frame.
- Keep scene state separated from product pin state; image overlay can switch without platform pin action.
- Preload product image with supported codecs; use alpha pre-multiplied compositing, handle resolution/safe-area templates for 9:16, 16:9.

## Video-based clone (mô hình chuẩn theo phép đo)

1. Face detection + landmark/crop + mask from authorized video; ensure source face consistently visible, head pose within backend limits.
2. Preserve natural body/hands from footage; drive changed mouth/face region using OpenTalking runtime.
3. Face pasteback/fusion should respect light/exposure, color histogram, occlusion mask (hand over face), feathering, temporal jitter filter.
4. Compare frame-to-frame and audio-driven lip-sync; **không khẳng định DEEPCORE tự sinh cử chỉ toàn thân AI** khi code chỉ chứng minh footage/face–hand.
5. Treat motion model JoyVASA/FasterLivePortrait as experiment; benchmarking before default. If choosing another model, verify exact license and GPU profile.

## Scene switching logic

- `intent=GREETING` -> `smile/listening` clip; `PRICE_QUERY` -> face close-up + product card; `PRODUCT_DEMO` -> `hand_detail/product_present`; `COMPARE` -> split/product board; `UNKNOWN` -> idle.
- Gesture selection not at per-token frequency; debounce motion 2–5 sec according to clip, avoid random hand flapping. Prefer cutaway/closeup to conceal hard avatar frame transitions.
- When product changes, stage image and CTA text; commit only with trusted revision, crossfade 120–300ms tested visually; no numeric claims without verified facts.
- When source video mouth sync degraded, fallback to face closeup or product B-roll, retain voice; do not hard switch avatar model live without prewarm.

## UI integration — modify existing app, don't replace

Add Live Studio panel within existing Concat/Tauri/Slint architecture (after audit) with tabs: `Session`, `Avatar/Motion`, `Script`, `Products`, `Comments`, `Audio/Output`, `Health`. Status: actual avatar engine, GPU, queue ms, media FPS, connection state, active/featured/pinned SKU, latest safe reply, pending approvals. Operator controls: start/stop/mute/takeover/manual speak/pin request override, with confirmation. Visual color not the source of truth; read backend session state.

## Codex acceptance tests

- Invalid asset path rejected; clip with wrong codec -> fallback, no crash.
- Switching from face to hand while audio continuous, no blank > defined frame budget; screenshot/video fixture if runtime accessible.
- Unknown gesture safely maps to idle; manually selected motion overrides AI.
- Source audio/copyright/identity rights manifest expired -> cannot publish new avatar session.
- Scene revision matches product revision before numerical price overlay; image decode failure doesn't stop output.
- Existing Concat edit/export regression tests unchanged.
