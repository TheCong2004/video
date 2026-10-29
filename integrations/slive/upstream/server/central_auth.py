import agency_runtime
"""Local, single-user auth/billing proxy for the Live Face desktop UI.

The browser never receives the central JWT. Only the token is persisted; the
user password is deliberately not stored.
"""
import asyncio
import json
import os
import urllib.error
import urllib.request
import time
from pathlib import Path

from aiohttp import web
from runtime_config import CENTRAL_API_URL, load_config
from runtime_paths import data_dir


SERVICE_TYPE = "live_face"
PUBLIC_PATHS = {"/login.html", "/favicon.png", "/api/auth/login", "/api/auth/register", "/api/auth/verify-registration"}
PROTECTED_RUNTIME_PATHS = {
    "/offer", "/human", "/humanaudio", "/set_audiotype", "/record",
    "/interrupt_talk", "/is_speaking", "/speech_status", "/speech_settings", "/chat_messages", "/event_message",
    "/event_messages", "/tiktok_connect", "/select_avatar", "/upload_avatar"
}


class CentralAuth:
    def __init__(self, app_dir: str):
        self.app_dir = Path(app_dir)
        runtime_config = self._read_config()
        self.central_url = runtime_config.get("runtime_central_api_url") or CENTRAL_API_URL
        local_root = data_dir()
        local_root.mkdir(parents=True, exist_ok=True)
        self.auth_path = local_root / "auth.json"
        self.token = self._load_token()
        self.billing_session_id = None
        self.profile_cache = None
        self.profile_cache_at = 0

    def _read_config(self):
        return load_config(self.app_dir)

    def _load_token(self):
        try:
            return json.loads(self.auth_path.read_text(encoding="utf-8")).get("token") or None
        except Exception:
            return None

    def _save_token(self, token):
        self.token = token or None
        self.profile_cache = None
        self.profile_cache_at = 0
        if self.token:
            self.auth_path.write_text(json.dumps({"token": self.token}), encoding="utf-8")
        elif self.auth_path.exists():
            self.auth_path.unlink()

    def _sync_request(self, endpoint, method="GET", data=None, token=None):
        if endpoint.split("?")[0].endswith("/session/start"):
            data = {**(data or {}), "deviceId": agency_runtime.device_id()}
            if agency_runtime.manifest():
                data["appVersion"] = agency_runtime.installed_version()
        import ssl
        body = json.dumps(data).encode("utf-8") if data is not None else None
        ssl_ctx = ssl.create_default_context()
        request = urllib.request.Request(self.central_url + endpoint, data=body, method=method)
        request.add_header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
        request.add_header("Accept-Language", "vi-VN,vi;q=0.9,en-US;q=0.8,en;q=0.7")
        request.add_header("Content-Type", "application/json")
        request.add_header("Accept", "application/json, text/plain, */*")
        request.add_header("Origin", self.central_url)
        request.add_header("Referer", f"{self.central_url}/")
        opener = urllib.request.build_opener(
            urllib.request.HTTPSHandler(context=ssl_ctx), agency_runtime.SafeRedirect()
        )
        if agency_runtime.manifest():
            try: agency_runtime.site_config()
            except Exception as error:
                if endpoint.split('?')[0] == "/api/auth/login" and "AGENCY_MISMATCH" in str(error):
                    return {"success": False, "code": "INVALID_CREDENTIALS", "error": "Tên tài khoản hoặc mật khẩu không chính xác."}, 400
                return {"success": False, "error": "Dịch vụ hiện tạm thời không khả dụng. Vui lòng thử lại sau."}, 503
        if token:
            request.add_header("Authorization", f"Bearer {token}")
        
        max_attempts = 3
        last_error = None
        for attempt in range(max_attempts):
            try:
                with opener.open(request, timeout=15) as response:
                    return json.loads(response.read().decode("utf-8")), response.status
            except urllib.error.HTTPError as error:
                body_content = error.read().decode("utf-8") if hasattr(error, 'read') else str(error)
                try:
                    parsed = json.loads(body_content)
                    return parsed, error.code
                except Exception:
                    if "1010" in body_content or "Cloudflare" in body_content:
                        err_msg = "Kết nối bị tường lửa (Cloudflare 1010) chặn. Vui lòng thử lại hoặc kiểm tra VPN/mạng."
                    else:
                        err_msg = f"HTTP Error {error.code}: {error.reason}"
                    return {"success": False, "error": err_msg}, error.code
            except Exception as e:
                last_error = e
                if attempt < max_attempts - 1:
                    time.sleep(0.5)
                    continue
        return {"success": False, "error": f"Không kết nối được backend trung tâm: {last_error}"}, 503

    async def central(self, endpoint, method="GET", data=None, token=None):
        return await asyncio.to_thread(self._sync_request, endpoint, method, data, token)

    async def profile(self, force=False):
        if not self.token:
            return {"success": False, "error": "Yêu cầu đăng nhập."}, 401
        if not force and self.profile_cache and time.monotonic() - self.profile_cache_at < 1:
            return self.profile_cache, 200
        payload, status = await self.central(f"/api/user/profile?serviceType={SERVICE_TYPE}", token=self.token)
        if status in (401, 403) or payload.get("action") == "force_logout":
            self._save_token(None)
            self.billing_session_id = None
        elif status == 200 and payload.get("success"):
            self.profile_cache = payload
            self.profile_cache_at = time.monotonic()
            active_session = payload.get("activeSession") or {}
            if active_session.get("status") == "active" and active_session.get("serviceType") == SERVICE_TYPE:
                self.billing_session_id = active_session.get("sessionId") or active_session.get("_id")
        return payload, status

    @web.middleware
    async def middleware(self, request, handler):
        path = request.path
        if path in PUBLIC_PATHS or path.startswith("/api/auth/"):
            return await handler(request)
        requires_login = (path in ("/", "/index.html", "/dashboard.html", "/avatar.html", "/avatar-live.html", "/resources.html")
                          or path.startswith("/api/resources/") or path.startswith("/api/billing/")
                          or path.startswith("/api/invoice/") or path.startswith("/api/invoices") or path.startswith("/api/sessions")
                          or path.startswith("/api/avatar/") or path in PROTECTED_RUNTIME_PATHS)
        if requires_login:
            profile, status = await self.profile()
            if status != 200 or not profile.get("success"):
                if path in ("/", "/index.html", "/dashboard.html", "/avatar-live.html", "/resources.html"):
                    login_url = "/login.html?next=/index.html" if path == "/index.html" else "/login.html"
                    raise web.HTTPFound(login_url)
                return web.json_response(profile, status=status)
        return await handler(request)

    async def auth_state(self, request):
        payload, status = await self.profile(force=True)
        return web.json_response(payload, status=status)

    async def login(self, request):
        data = await request.json()
        payload, status = await self.central("/api/auth/login", "POST", {
            "username": str(data.get("username") or "").strip(),
            "password": str(data.get("password") or ""),
            "appType": str(data.get("appType") or SERVICE_TYPE)
        })
        if status == 200 and payload.get("success"):
            self._save_token(payload.get("token"))
            payload.pop("token", None)
        return web.json_response(payload, status=status)

    async def register(self, request):
        payload, status = await self.central("/api/auth/register", "POST", await request.json())
        payload.pop("token", None)
        return web.json_response(payload, status=status)

    async def verify_registration(self, request):
        payload, status = await self.central("/api/auth/verify-registration", "POST", await request.json())
        payload.pop("token", None)
        return web.json_response(payload, status=status)

    async def logout(self, request):
        if self.billing_session_id and self.token:
            await self.central("/api/billing/session/end", "POST", {"sessionId": self.billing_session_id}, self.token)
        self.billing_session_id = None
        self._save_token(None)
        return web.json_response({"success": True})

    async def update_tiktok_channel(self, channel: str = "", action: str = None, session_id: str = None):
        """Update active TikTok channel on central backend."""
        if not self.token:
            return None, 401
        body = {
            "tiktokChannel": str(channel or "").replace("@", "").strip(),
            "channel": str(channel or "").replace("@", "").strip(),
            "sessionId": session_id or self.billing_session_id
        }
        if action:
            body["action"] = action
        payload, status = await self.central("/api/billing/tiktok-channel", "POST", body, self.token)
        self.profile_cache = None
        return payload, status

    async def _execute_billing_operation(self, operation, request):
        mapping = {
            "profile": (f"/api/user/profile?serviceType={SERVICE_TYPE}", "GET"),
            "presence/heartbeat": ("/api/user/presence/heartbeat", "POST"),
            "presence/disconnect": ("/api/user/presence/disconnect", "POST"),
            "session/start": ("/api/billing/session/start", "POST"),
            "session/heartbeat": ("/api/billing/session/heartbeat", "POST"),
            "session/end": ("/api/billing/session/end", "POST"),
            "tiktok-channel": ("/api/billing/tiktok-channel", "POST"),
            "invoice/create": ("/api/billing/invoice/create", "POST"),
            "invoices": (f"/api/billing/invoices?serviceType={SERVICE_TYPE}", "GET"),
            "sessions": (f"/api/billing/sessions?serviceType={SERVICE_TYPE}", "GET")
        }
        if operation not in mapping:
            return web.json_response({"success": False, "error": "API không tồn tại."}, status=404)
        endpoint, method = mapping[operation]
        data = await request.json() if method == "POST" and request.can_read_body else {}
        if operation == "presence/heartbeat":
            data["appType"] = SERVICE_TYPE
        if operation in ("session/start", "invoice/create"):
            data["serviceType"] = SERVICE_TYPE
        if operation == "tiktok-channel":
            if not data.get("sessionId") and self.billing_session_id:
                data["sessionId"] = self.billing_session_id
        if operation in ("session/heartbeat", "session/end"):
            if not self.billing_session_id:
                # Attempt to recover billing_session_id from profile
                await self.profile(force=True)
            data["sessionId"] = data.get("sessionId") or self.billing_session_id
        payload, status = await self.central(endpoint, method, data if method == "POST" else None, self.token)
        if operation == "session/start" and status == 200 and payload.get("success"):
            self.billing_session_id = payload.get("sessionId")
        if operation == "session/end" or payload.get("action") in ("stop_live", "force_logout"):
            self.billing_session_id = None
        if operation in ("session/start", "session/heartbeat", "session/end", "invoice/create", "tiktok-channel"):
            self.profile_cache = None
        if payload.get("action") == "force_logout":
            self._save_token(None)
        return web.json_response(payload, status=status)

    async def billing_proxy(self, request):
        operation = request.match_info["operation"]
        return await self._execute_billing_operation(operation, request)

    async def alias_invoices(self, request):
        return await self._execute_billing_operation("invoices", request)

    async def alias_sessions(self, request):
        return await self._execute_billing_operation("sessions", request)

    async def alias_create_invoice(self, request):
        return await self._execute_billing_operation("invoice/create", request)

    async def invoice_action(self, request):
        invoice_id = request.match_info["invoice_id"]
        action = request.match_info["action"]
        if action == "status":
            endpoint, method = f"/api/billing/invoice/status/{invoice_id}", "GET"
        elif action == "cancel":
            endpoint, method = f"/api/billing/invoice/cancel/{invoice_id}", "POST"
        else:
            return web.json_response({"success": False, "error": "API không tồn tại."}, status=404)
        payload, status = await self.central(endpoint, method, {} if method == "POST" else None, self.token)
        return web.json_response(payload, status=status)

    async def agency_site(self, request):
        try: return web.json_response(await asyncio.to_thread(agency_runtime.site_config))
        except Exception: return web.json_response({"error": "Dịch vụ hiện tạm thời không khả dụng."}, status=503)

    def setup_routes(self, app):
        app.router.add_get("/api/agency/site", self.agency_site)
        app.router.add_get("/api/auth/state", self.auth_state)
        app.router.add_post("/api/auth/login", self.login)
        app.router.add_post("/api/auth/register", self.register)
        app.router.add_post("/api/auth/verify-registration", self.verify_registration)
        app.router.add_post("/api/auth/logout", self.logout)
        app.router.add_get("/api/invoices", self.alias_invoices)
        app.router.add_get("/api/sessions", self.alias_sessions)
        app.router.add_post("/api/invoice/create", self.alias_create_invoice)
        app.router.add_route("*", "/api/billing/{operation:.+}", self.billing_proxy)
        app.router.add_route("*", "/api/invoice/{action}/{invoice_id}", self.invoice_action)
