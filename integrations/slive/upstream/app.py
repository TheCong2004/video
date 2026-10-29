###############################################################################
#  Copyright (C) 2024 LiveTalking@lipku https://github.com/lipku/LiveTalking
#  email: lipku@foxmail.com
# 
#  Licensed under the Apache License, Version 2.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at
#  
#       http://www.apache.org/licenses/LICENSE-2.0
# 
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
###############################################################################

import sys
from pathlib import Path
try:
    _app_base = Path(sys.executable).parent if getattr(sys, "frozen", False) else Path(__file__).resolve().parent
    if str(_app_base) not in sys.path:
        sys.path.insert(0, str(_app_base))
    from importlib.machinery import PathFinder
    _current_meta = [f for f in sys.meta_path if f is not PathFinder]
    sys.meta_path = [PathFinder] + _current_meta
except Exception:
    pass

# server.py
from flask import Flask, render_template,send_from_directory,request, jsonify
try:
    from flask_sockets import Sockets
except Exception:
    Sockets = None
import base64
import json
#import gevent
#from gevent import pywsgi
#from geventwebsocket.handler import WebSocketHandler
import re
import os
import numpy as np
from threading import Thread,Event
#import multiprocessing
import torch.multiprocessing as mp

from aiohttp import web
import aiohttp
import aiohttp_cors
from aiortc import RTCPeerConnection, RTCSessionDescription,RTCIceServer,RTCConfiguration
from aiortc.rtcrtpsender import RTCRtpSender
from server.webrtc import HumanPlayer
from avatars.base_avatar import BaseAvatar
from llm import llm_response, llm_continuous
import registry
from server.routes import setup_routes
from server.rtc_manager import RTCManager
from server.session_manager import session_manager
from server.central_auth import CentralAuth

import argparse
import random
import shutil
import asyncio
import torch
from io import BytesIO
from typing import Dict
from utils.logger import logger
import copy
import gc
import sys

app = Flask(__name__)
#sockets = Sockets(app)
opt = None
model = None
global_avatars = {} # avatar_id: payload
central_auth = None
        

#####webrtc###############################
# rtc_manager replaces the old pcs set and duplicate offer handlers.
rtc_manager = None

def randN(N)->int:
    '''Tạo số ngẫu nhiên có độ dài N '''
    min = pow(10, N - 1)
    max = pow(10, N)
    return random.randint(min, max - 1)

def build_avatar_session(sessionid:str, params:dict)->BaseAvatar:
    opt_this = copy.deepcopy(opt)
    opt_this.sessionid = sessionid

    avatar_id = params.get('avatar', opt.avatar_id)
    opt_this.avatar_id = avatar_id
    if 'camera_index' in params:
        try:
            opt_this.camera_index = int(params['camera_index'])
        except (ValueError, TypeError):
            pass
    elif avatar_id and str(avatar_id).lower().startswith('webcam'):
        import re
        m = re.search(r'webcam[_\-:]?(\d+)', str(avatar_id).lower())
        if m:
            opt_this.camera_index = int(m.group(1))
    ref_audio = params.get('refaudio','') # giọng nói
    ref_text = params.get('reftext','')
    if (avatar_id and avatar_id != opt.avatar_id):
        # Avoid reloading if already cached globally
        if avatar_id not in global_avatars:
            global_avatars[avatar_id] = load_avatar(avatar_id)
        avatar_this = global_avatars[avatar_id]
    else:
        # Default avatar loaded at startup
        avatar_this = global_avatars.get(opt.avatar_id)
    if ref_audio: # tham số yêu cầu đã cấu hình âm thanh tham chiếu
        opt_this.REF_FILE = ref_audio
        opt_this.REF_TEXT = ref_text
    custom_config=params.get('custom_config','') # cấu hình dàn dựng hành động
    if custom_config:
        opt_this.customopt = json.loads(custom_config)

    avatar_session = registry.create("avatar", opt.model, opt=opt_this, model=model, avatar=avatar_this)
    return avatar_session

async def offer(request):
    return await rtc_manager.handle_offer(request)

async def on_shutdown(app):
    await rtc_manager.shutdown()
    auth = app.get('central_auth')
    if auth and auth.billing_session_id and auth.token:
        await auth.central('/api/billing/session/end', 'POST', {'sessionId': auth.billing_session_id}, auth.token)



def main():
    global rtc_manager, opt, model,load_avatar,central_auth
    # Phân tích tham số dòng lệnh
    from config import parse_args
    opt = parse_args()

    # ─── Tải plugin avatar (kích hoạt đăng ký @register) ──────────────────────
    _avatar_modules = {
        'musetalk':   'avatars.musetalk_avatar',
        'wav2lip':    'avatars.wav2lip_avatar',
        'ultralight': 'avatars.ultralight_avatar',
    }
    import importlib
    avatar_mod = importlib.import_module(_avatar_modules[opt.model])
    load_model = avatar_mod.load_model
    load_avatar = avatar_mod.load_avatar
    warm_up = avatar_mod.warm_up
    logger.info(opt)

    if opt.model == 'musetalk':
        model = load_model()
        global_avatars[opt.avatar_id] = load_avatar(opt.avatar_id) 
        warm_up(opt.batch_size,model)      
    elif opt.model == 'wav2lip':
        from runtime_paths import model_path
        model = load_model(str(model_path()))
        global_avatars[opt.avatar_id] = load_avatar(opt.avatar_id)
        warm_up(opt.batch_size,model,256)
    elif opt.model == 'ultralight':
        model = load_model(opt)
        global_avatars[opt.avatar_id] = load_avatar(opt.avatar_id)
        warm_up(opt.batch_size,global_avatars[opt.avatar_id],160)

    # init rtc manager
    session_manager.init_builder(build_avatar_session)
    rtc_manager = RTCManager(opt)
    # share avatar_sessions (RTCManager handles it but routes.py expects it)
    
    if opt.transport=='virtualcam' or opt.transport=='rtmp':
        thread_quit = Event()
        params = {}
        # session 0 for virtualcam
        session_manager.add_session('0', build_avatar_session('0', params))
        rendthrd = Thread(target=session_manager.get_session('0').render,args=(thread_quit,))
        rendthrd.start()

    #############################################################################
    # Allow larger uploads for avatar video files (increase from 100MB to 500MB)
    central_auth = CentralAuth(os.getcwd())
    appasync = web.Application(client_max_size=1024**2*500, middlewares=[central_auth.middleware])
    appasync['central_auth'] = central_auth
    appasync["llm_response"] = llm_response
    appasync["llm_continuous"] = llm_continuous

    appasync.on_shutdown.append(on_shutdown)
    appasync.router.add_post("/offer", offer)
    central_auth.setup_routes(appasync)

    # Expose the same resource status contract in the installed runtime so the
    # dashboard header/modal behaves identically before and after installation.
    from pathlib import Path
    from resource_manager import ResourceManager
    runtime_root = Path(os.getcwd())
    runtime_manifest = runtime_root / "customer-resource-manifest.json"
    if not runtime_manifest.is_file():
        runtime_manifest = runtime_root / "resource-manifest.json"
    from runtime_paths import resources_dir
    runtime_resources = ResourceManager(runtime_root, install_root=resources_dir(), manifest_path=runtime_manifest)

    async def resource_status(_request):
        status = runtime_resources.status()
        status["mode"] = "runtime"
        return web.json_response(status)

    async def resource_install(request):
        bundle_id = None
        try:
            body = await request.json()
            bundle_id = body.get("bundle_id")
        except Exception:
            pass
        runtime_resources.start(bundle_id)
        status = runtime_resources.status()
        status["mode"] = "runtime"
        return web.json_response(status)

    async def resource_cancel(_request):
        runtime_resources.cancel()
        status = runtime_resources.status()
        status["mode"] = "runtime"
        return web.json_response(status)

    async def resource_launch(_request):
        return web.json_response({"success": True})

    async def app_exit(_request):
        import asyncio
        asyncio.get_event_loop().call_later(0.3, lambda: os._exit(0))
        return web.json_response({"success": True})

    appasync.router.add_get("/api/resources/status", resource_status)
    appasync.router.add_post("/api/resources/install", resource_install)
    appasync.router.add_post("/api/resources/cancel", resource_cancel)
    appasync.router.add_post("/api/resources/launch", resource_launch)
    appasync.router.add_post("/api/app/exit", app_exit)
    
    # Đăng ký các tuyến API dùng chung trong server/routes.py
    setup_routes(appasync) 

    # Configure default CORS settings.
    cors = aiohttp_cors.setup(appasync, defaults={
            "*": aiohttp_cors.ResourceOptions(
                allow_credentials=True,
                expose_headers="*",
                allow_headers="*",
            )
        })
    # Configure CORS on all routes.
    for route in list(appasync.router.routes()):
        try:
            cors.add(route)
        except ValueError:
            # aiohttp-cors cannot attach an OPTIONS handler to resources that
            # already expose a wildcard route (billing/auth proxy). Those
            # endpoints are same-origin in the desktop app, so skipping them
            # is both safe and required for startup.
            pass

    pagename='webrtcapi.html'
    if opt.transport=='rtmp':
        pagename='rtmpapi.html'
    elif opt.transport=='rtcpush':
        pagename='rtcpushapi.html'
    logger.info('start http server; http://<serverip>:'+str(opt.listenport)+'/'+pagename)
    logger.info('Nếu dùng WebRTC, nên truy cập giao diện frontend tích hợp tại: http://<serverip>:'+str(opt.listenport)+'/dashboard.html')
    def run_server(runner):
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        loop.run_until_complete(runner.setup())
        site = web.TCPSite(runner, '0.0.0.0', opt.listenport)
        loop.run_until_complete(site.start())
        if opt.transport=='rtcpush':
            for k in range(opt.max_session):
                push_url = opt.push_url
                if k!=0:
                    push_url = opt.push_url+str(k)
                loop.run_until_complete(rtc_manager.handle_rtcpush(push_url, str(k)))
        loop.run_forever()    
    #Thread(target=run_server, args=(web.AppRunner(appasync),)).start()
    run_server(web.AppRunner(appasync))

    #app.on_shutdown.append(on_shutdown)
    #app.router.add_post("/offer", offer)

    # print('start websocket server')
    # server = pywsgi.WSGIServer(('0.0.0.0', 8000), app, handler_class=WebSocketHandler)
    # server.serve_forever()


# os.environ['MKL_SERVICE_FORCE_INTEL'] = '1'
# os.environ['MULTIPROCESSING_METHOD'] = 'forkserver'                                                    
if __name__ == '__main__':
    mp.set_start_method('spawn')
    main()
    
    
    
