# GPU server image only. No changes to the desktop's Python/ArtCraft/Promax.
FROM pytorch/pytorch:2.0.1-cuda11.7-cudnn8-runtime
ARG MUSETALK_REV=0a89dec45a0192b824e3cf4daf96c239440c5ed8
RUN apt-get update && apt-get install -y --no-install-recommends git ffmpeg libgl1 libglib2.0-0 build-essential \
    && rm -rf /var/lib/apt/lists/*
RUN git init /opt/MuseTalk && cd /opt/MuseTalk \
    && git remote add origin https://github.com/TMElyralab/MuseTalk.git \
    && git fetch --depth 1 origin ${MUSETALK_REV} && git checkout --detach FETCH_HEAD
WORKDIR /opt/MuseTalk
RUN python -m pip install --no-cache-dir -r requirements.txt openmim \
    && mim install mmengine 'mmcv==2.0.1' 'mmdet==3.1.0' 'mmpose==1.1.0' \
    && python -m pip check
COPY server.py musetalk_engine.py download_models.py /opt/concat-lipsync/
ENV CONCAT_MUSETALK_ROOT=/opt/MuseTalk CONCAT_LIPSYNC_DATA=/data/utterances \
    HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 PYTHONUNBUFFERED=1
# Model downloads are a separate explicit command. Normal server startup is offline.
CMD ["python", "-B", "/opt/concat-lipsync/server.py"]
