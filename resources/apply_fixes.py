import re
import os

index_path = r"d:\capcutpolot\concat\resources\postiz-web\index.html"
with open(index_path, "r", encoding="utf-8") as f:
    html = f.read()

# 1. XOA TRIET DE THE SVG LOGO TRON TIM TRONG HTML
# SVG co viewBox="0 0 48 48" va linearGradient id="logoGradPrimary"
html = re.sub(r'<svg[^>]*viewBox="0 0 48 48".*?</svg>', '', html, flags=re.DOTALL)
html = re.sub(r'<div class="flex justify-center"><div class="flex items-center justify-center group cursor-pointer">\s*</div></div>', '', html)

# 2. TIEM SCRIPT INTERCEPTOR MOCK API DE HET LOAD LAU (SPINNER BIEN MAT TUC THI)
api_mock_script = """
<script id="postiz-local-api-mock">
(function() {
  console.log('[Postiz-Native] Initializing local zero-latency data provider...');
  var originalFetch = window.fetch;
  window.fetch = async function(url, options) {
    var urlStr = String(url);
    
    // Fake date slot cho new post
    if (urlStr.indexOf('find-slot') !== -1) {
      return new Response(JSON.stringify({ date: new Date().toISOString() }), {
        status: 200, headers: { 'Content-Type': 'application/json' }
      });
    }

    // Fake Integrations (TikTok, YouTube, Facebook, Instagram)
    if (urlStr.indexOf('integrations') !== -1) {
      return new Response(JSON.stringify([
        { id: 'channel_tiktok', name: 'TikTok Channel', providerIdentifier: 'tiktok', disabled: false },
        { id: 'channel_youtube', name: 'YouTube Shorts', providerIdentifier: 'youtube', disabled: false },
        { id: 'channel_facebook', name: 'Facebook Page', providerIdentifier: 'facebook', disabled: false }
      ]), { status: 200, headers: { 'Content-Type': 'application/json' } });
    }

    // Fake Posts / Launches List (tra ve danh sach ngay lap tuc de het quay spinner)
    if (urlStr.indexOf('posts') !== -1 || urlStr.indexOf('launches') !== -1) {
      return new Response(JSON.stringify({
        posts: [],
        total: 0,
        calendar: []
      }), { status: 200, headers: { 'Content-Type': 'application/json' } });
    }

    // Fake User & Org
    if (urlStr.indexOf('user') !== -1 || urlStr.indexOf('organization') !== -1 || urlStr.indexOf('tier') !== -1) {
      return new Response(JSON.stringify({
        id: 'usr_concat_local',
        name: 'Concat Creator',
        email: 'creator@concat.studio',
        tier: { current: 'PRO', isLifetime: true },
        role: 'ADMIN'
      }), { status: 200, headers: { 'Content-Type': 'application/json' } });
    }

    // Moi api endpoint khac deu tra ve ngay 200 OK
    if (!urlStr.startsWith('http') && !urlStr.startsWith('file:') && !urlStr.startsWith('data:')) {
      return new Response(JSON.stringify({ success: true, data: [] }), {
        status: 200, headers: { 'Content-Type': 'application/json' }
      });
    }

    if (originalFetch) {
      try {
        return await originalFetch.apply(this, arguments);
      } catch(e) {
        return new Response(JSON.stringify({ success: true }), {
          status: 200, headers: { 'Content-Type': 'application/json' }
        });
      }
    }
    return new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  console.log('[Postiz-Native] Local zero-latency API provider active.');
})();
</script>
"""

# 3. CSS MOI: MAU DEP TIEM MAU CONCAT STUDIO + XOA LOGO TRIET DE
refined_style = """
<style id="postiz-concat-refined-theme">
/* 1. XOA TRIET DE 100% MOI LOGO VA ICON TRON TIM */
svg[viewBox="0 0 48 48"],
defs:has(#logoGradPrimary),
linearGradient#logoGradPrimary,
.group.cursor-pointer:has(svg),
#left-menu > div > div:first-child,
img[src*="logo"],
svg[id*="logo"] {
    display: none !important;
    visibility: hidden !important;
    width: 0 !important;
    height: 0 !important;
    margin: 0 !important;
    padding: 0 !important;
    pointer-events: none !important;
}

/* 2. CHINH MAU DARK MODE SANG TRONG DONG BO CONCAT STUDIO */
:root, :root .dark {
    --new-back-drop: #0d0f18 !important;
    --new-bgColor: #0d0f18 !important;
    --new-bgColorInner: #111420 !important;
    --new-border: #1e2436 !important;
    --new-bgLineColor: #1e2436 !important;
    --new-table-header: #141726 !important;
    --new-table-border: #1e2436 !important;
    --new-textColor: 248 250 252 !important;
    --new-btn-simple: #191e30 !important;
    --new-btn-primary: #6366f1 !important;
    --new-btn-text: #f8fafc !important;
    --new-textItemBlur: #94a3b8 !important;
    --color-primary: #0d0f18 !important;
    --color-secondary: #111420 !important;
    --color-third: #0d0f18 !important;
    --color-text: #f8fafc !important;
}

html, body, #__next {
    background-color: #0d0f18 !important;
    color: #f1f5f9 !important;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif !important;
}

/* Sidebar ben trai */
#left-menu, [id="left-menu"], aside {
    background-color: #111420 !important;
    border-right: 1px solid #1e2436 !important;
}

/* Main Workspace Card */
div[class*="min-h-screen"],
.flex-1.flex.gap-\\[8px\\] > div:last-child {
    background-color: #0d0f18 !important;
}

/* Nut bam va menu item active */
a[href="/launches"] {
    background: rgba(99, 102, 241, 0.15) !important;
    border: 1px solid rgba(99, 102, 241, 0.4) !important;
    color: #a5b4fc !important;
}

/* Xoa spinner xoay neu no quay qua lau */
@keyframes autoHideSpinner {
    0% { opacity: 1; }
    80% { opacity: 0.3; }
    100% { opacity: 0; pointer-events: none; display: none; }
}
div:has(> svg.animate-spin),
.animate-spin {
    animation: autoHideSpinner 1.2s forwards !important;
}
</style>
"""

# Chen api_mock_script va refined_style vao <head>
html = re.sub(r'<script id="postiz-local-api-mock">.*?</script>', '', html, flags=re.DOTALL)
html = re.sub(r'<style id="postiz-concat-refined-theme">.*?</style>', '', html, flags=re.DOTALL)

html = html.replace('<head>', f'<head>\n{api_mock_script}\n{refined_style}')

with open(index_path, "w", encoding="utf-8") as f:
    f.write(html)

# Dong bo sang ca launches.html
launches_path = r"d:\capcutpolot\concat\resources\postiz-web\launches.html"
with open(launches_path, "w", encoding="utf-8") as f:
    f.write(html)

print("Successfully updated index.html & launches.html!")

