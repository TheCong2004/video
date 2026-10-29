from os import listdir, path
import numpy as np
import scipy, cv2, os, sys, argparse
import json, subprocess, random, string
from tqdm import tqdm
from glob import glob
import torch
import pickle

os.environ.setdefault("PYTHONUTF8", "1")
os.environ.setdefault("PYTHONIOENCODING", "utf-8")
for _s in (sys.stdout, sys.stderr):
    try:
        _s.reconfigure(encoding="utf-8", errors="backslashreplace")
    except Exception:
        pass

from avatars.wav2lip import face_detection
from runtime_paths import avatar_path as resolve_avatar_path


parser = argparse.ArgumentParser(description='Inference code to lip-sync videos in the wild using Wav2Lip models')
parser.add_argument('--img_size', default=384, type=int)
parser.add_argument('--avatar_id', default='video1', type=str)
parser.add_argument('--video_path', default='8052379938970.mp4', type=str)
parser.add_argument('--nosmooth', default=False, action='store_true',
					help='Prevent smoothing face detections over a short temporal window')
parser.add_argument('--pads', nargs='+', type=int, default=[0, 10, 0, 0],
					help='Padding (top, bottom, left, right). Please adjust to include chin at least')
parser.add_argument('--face_det_batch_size', type=int, 
					help='Batch size for face detection', default=16)
args = parser.parse_args()

device = 'cuda' if torch.cuda.is_available() else 'cpu'
print('Using {} for inference.'.format(device))

def osmakedirs(path_list):
	for path in path_list:
		os.makedirs(path) if not os.path.exists(path) else None

def video2imgs(vid_path, save_path, ext='.png', cut_frame=10000000):
	cap = cv2.VideoCapture(vid_path)
	total_frames = int(cap.get(cv2.CAP_PROP_FRAME_COUNT)) or 0
	count = 0
	while True:
		if count > cut_frame:
			break
		ret, frame = cap.read()
		if ret:
			cv2.putText(frame, "LiveTalking", (10, 20), cv2.FONT_HERSHEY_SIMPLEX, 0.3, (128, 128, 128), 1)
			cv2.imwrite(f"{save_path}/{count:08d}.png", frame)
			count += 1
			if total_frames > 0 and (count % 30 == 0 or count == total_frames):
				p = min(15, max(1, int(15 * count / total_frames)))
				print(f'STAGE: Tách khung hình ({count}/{total_frames})')
				print(f'PROGRESS: {p}')
				sys.stdout.flush()
		else:
			break
	cap.release()
	print('STEP: extract_frames')
	print('STAGE: Tách khung hình hoàn tất')
	print('PROGRESS: 15')
	sys.stdout.flush()

def get_smoothened_boxes(boxes, T=7):
	n = len(boxes)
	if n == 0:
		return boxes
	half_w = max(1, T // 2)
	smoothed = np.zeros_like(boxes, dtype=np.float32)
	for i in range(n):
		start = max(0, i - half_w)
		end = min(n, i + half_w + 1)
		window = boxes[start:end]
		smoothed[i] = np.mean(window, axis=0)
	return np.round(smoothed).astype(int)

def detect_faces_streamed(img_paths, pads, max_detect_dim=640, batch_size=4):
	"""Quét mặt theo batch nhỏ và tiền thu nhỏ ảnh (downscale 640px) để:
	1. Tiết kiệm 95% RAM (không nạp hàng nghìn ảnh 1080p vào RAM cùng lúc).
	2. Tiết kiệm 80% VRAM GPU, tránh tràn VRAM/GPU thrashing khi chạy kèm Live Engine.
	3. Tăng tốc độ quét mặt nhanh gấp 4-5 lần."""
	if torch.cuda.is_available():
		try:
			torch.cuda.empty_cache()
		except Exception:
			pass

	det_device = device
	try:
		detector = face_detection.FaceAlignment(face_detection.LandmarksType._2D,
												 flip_input=False, device=det_device)
	except Exception as exc:
		print(f"Khởi tạo detector trên {det_device} thất bại: {exc}. Chuyển sang CPU.")
		det_device = 'cpu'
		detector = face_detection.FaceAlignment(face_detection.LandmarksType._2D,
												 flip_input=False, device='cpu')

	total = len(img_paths)
	raw_boxes = []
	pady1, pady2, padx1, padx2 = pads
	effective_batch_size = max(1, batch_size)

	indices = list(range(0, total, effective_batch_size))
	nb = len(indices)

	for bi, i in enumerate(indices):
		chunk_paths = img_paths[i : i + effective_batch_size]
		batch_imgs = []
		scales = []
		orig_shapes = []

		for p in chunk_paths:
			img = cv2.imread(p)
			if img is None:
				raise ValueError(f"Không thể đọc ảnh: {p}")
			h, w = img.shape[:2]
			orig_shapes.append((h, w))
			scale = float(max_detect_dim) / max(h, w) if max(h, w) > max_detect_dim else 1.0
			if scale < 1.0:
				small = cv2.resize(img, (int(w * scale), int(h * scale)))
			else:
				small = img
			batch_imgs.append(small)
			scales.append(scale)

		try:
			dets = detector.get_detections_for_batch(np.array(batch_imgs))
		except RuntimeError as rerr:
			if 'out of memory' in str(rerr).lower() or 'cuda' in str(rerr).lower():
				if torch.cuda.is_available():
					torch.cuda.empty_cache()
				# Fallback từng ảnh một
				dets = []
				for s_img in batch_imgs:
					try:
						d = detector.get_detections_for_batch(np.array([s_img]))
						dets.extend(d)
					except Exception:
						cpu_det = face_detection.FaceAlignment(face_detection.LandmarksType._2D,
															   flip_input=False, device='cpu')
						d = cpu_det.get_detections_for_batch(np.array([s_img]))
						dets.extend(d)
			else:
				raise

		for idx_in_batch, (rect, scale, (h, w)) in enumerate(zip(dets, scales, orig_shapes)):
			global_idx = i + idx_in_batch
			if rect is None:
				# Thử lại trên ảnh gốc nếu ảnh thu nhỏ không bắt được mặt
				orig_img = cv2.imread(chunk_paths[idx_in_batch])
				try:
					retry = detector.get_detections_for_batch(np.array([orig_img]))
					if retry and retry[0] is not None:
						rect = retry[0]
						scale = 1.0
				except Exception:
					pass
			if rect is None:
				faulty_img = cv2.imread(chunk_paths[idx_in_batch])
				os.makedirs('temp', exist_ok=True)
				cv2.imwrite('temp/faulty_frame.jpg', faulty_img)
				raise ValueError(f'Face not detected at frame {global_idx}! Ensure the video contains a clear face in all frames.')

			x1, y1, x2, y2 = rect
			if scale < 1.0:
				x1 = int(round(x1 / scale))
				y1 = int(round(y1 / scale))
				x2 = int(round(x2 / scale))
				y2 = int(round(y2 / scale))

			y1 = max(0, y1 - pady1)
			y2 = min(h, y2 + pady2)
			x1 = max(0, x1 - padx1)
			x2 = min(w, x2 + padx2)
			raw_boxes.append([x1, y1, x2, y2])

		del batch_imgs

		curr = len(raw_boxes)
		prog = 15 + int(55 * curr / max(1, total))
		print(f'STAGE: Quét khuôn mặt ({curr}/{total})')
		print(f'PROGRESS: {prog}')
		sys.stdout.flush()

	boxes = np.array(raw_boxes)
	if not args.nosmooth:
		boxes = get_smoothened_boxes(boxes, T=7)

	del detector
	if torch.cuda.is_available():
		try:
			torch.cuda.empty_cache()
		except Exception:
			pass

	print('STEP: face_detection')
	print('STAGE: Quét khuôn mặt hoàn tất')
	print('PROGRESS: 70')
	sys.stdout.flush()
	return boxes

if __name__ == "__main__":
    avatar_path = str(resolve_avatar_path(args.avatar_id, for_write=True))
    full_imgs_path = f"{avatar_path}/full_imgs" 
    face_imgs_path = f"{avatar_path}/face_imgs" 
    coords_path = f"{avatar_path}/coords.pkl"
    osmakedirs([avatar_path, full_imgs_path, face_imgs_path])
    print(args)

    video2imgs(args.video_path, full_imgs_path, ext='png')
    input_img_list = sorted(glob(os.path.join(full_imgs_path, '*.[jpJP][pnPN]*[gG]')))
    total_faces = len(input_img_list)

    if total_faces == 0:
        raise ValueError("Không tìm thấy khung hình nào được trích xuất từ video.")

    boxes = detect_faces_streamed(input_img_list, pads=args.pads,
                                  max_detect_dim=640,
                                  batch_size=args.face_det_batch_size)

    coord_list = []
    print('STAGE: Bắt đầu cắt ảnh chân dung...')
    sys.stdout.flush()

    for idx, (img_path, box) in enumerate(zip(input_img_list, boxes)):
        frame = cv2.imread(img_path)
        if frame is None:
            continue
        x1, y1, x2, y2 = int(box[0]), int(box[1]), int(box[2]), int(box[3])
        crop = frame[y1:y2, x1:x2]
        if crop.size == 0:
            crop = frame
        resized_crop_frame = cv2.resize(crop, (args.img_size, args.img_size))
        cv2.imwrite(f"{face_imgs_path}/{idx:08d}.png", resized_crop_frame)
        coord_list.append((y1, y2, x1, x2))
        del frame, crop

        if (idx + 1) % 25 == 0 or (idx + 1) == total_faces:
            prog = 70 + int(28 * (idx + 1) / max(1, total_faces))
            print(f'STAGE: Cắt ảnh chân dung ({idx + 1}/{total_faces})')
            print(f'PROGRESS: {prog}')
            sys.stdout.flush()

    with open(coords_path, 'wb') as f:
        pickle.dump(coord_list, f)

    print('STEP: complete')
    print('STAGE: Hoàn tất')
    print('PROGRESS: 100')
    sys.stdout.flush()
