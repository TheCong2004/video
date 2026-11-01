import re

index_path = r"d:\capcutpolot\concat\resources\postiz-web\index.html"
with open(index_path, "r", encoding="utf-8") as f:
    html = f.read()

# 1. MOCK API HOAN HAO KHOP SCHEMA CUA POSTIZ
mock_api = """
<script id="postiz-local-api-mock">
(function() {
  console.log('[Postiz-Native] Initializing local zero-latency data provider...');
  var originalFetch = window.fetch;
  
  var mockIntegrations = [
    {
      id: "int_tiktok",
      name: "TikTok Channel",
      identifier: "tiktok",
      providerIdentifier: "tiktok",
      disabled: false,
      type: "social",
      picture: "",
      customer: { id: "cust_default", name: "Default" }
    },
    {
      id: "int_youtube",
      name: "YouTube Shorts",
      identifier: "youtube",
      providerIdentifier: "youtube",
      disabled: false,
      type: "social",
      picture: "",
      customer: { id: "cust_default", name: "Default" }
    },
    {
      id: "int_facebook",
      name: "Facebook Reels",
      identifier: "facebook",
      providerIdentifier: "facebook",
      disabled: false,
      type: "social",
      picture: "",
      customer: { id: "cust_default", name: "Default" }
    }
  ];

  window.fetch = async function(url, options) {
    var urlStr = String(url);
    
    // 1. Integrations list (QUAN TRONG NHAT: phai co format { integrations: [...] })
    if (urlStr.indexOf('integrations/list') !== -1 || (urlStr.indexOf('integrations') !== -1 && options?.method !== 'PUT')) {
      return new Response(JSON.stringify({
        integrations: mockIntegrations
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      });
    }

    // 2. Posts & Calendar
    if (urlStr.indexOf('posts/list') !== -1) {
      return new Response(JSON.stringify({
        posts: [],
        total: 0
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      });
    }

    if (urlStr.indexOf('posts') !== -1) {
      return new Response(JSON.stringify([]), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      });
    }

    // 3. Find slot
    if (urlStr.indexOf('find-slot') !== -1) {
      return new Response(JSON.stringify({ date: new Date().toISOString() }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      });
    }

    // 4. Sets & Signatures
    if (urlStr.indexOf('sets') !== -1) {
      return new Response(JSON.stringify([]), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      });
    }

    if (urlStr.indexOf('signatures') !== -1) {
      return new Response(JSON.stringify({ signature: "" }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      });
    }

    // 5. User & Org
    if (urlStr.indexOf('user') !== -1 || urlStr.indexOf('organization') !== -1 || urlStr.indexOf('tier') !== -1) {
      return new Response(JSON.stringify({
        id: 'usr_concat_local',
        name: 'Concat Creator',
        email: 'creator@concat.studio',
        tier: { current: 'PRO', isLifetime: true },
        role: 'ADMIN',
        totalChannels: 10
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      });
    }

    // 6. Safe fallback 200 cho moi endpoint noi bo khac
    if (!urlStr.startsWith('http') && !urlStr.startsWith('file:') && !urlStr.startsWith('data:')) {
      return new Response(JSON.stringify({ success: true }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      });
    }

    if (originalFetch) {
      try {
        return await originalFetch.apply(this, arguments);
      } catch (err) {
        return new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } });
      }
    }
    return new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  console.log('[Postiz-Native] Mock provider online. Integrations & calendar ready.');
})();
</script>
"""

# 2. CSS CHUAN: XOA DUNG LOGO (KHONG AN MENU ITEMS), MAU DEP SANG TRONG
clean_style = """
<style id="postiz-concat-refined-theme">
/* 1. XOA CHINH XAC LOGO TRON TIM (KHONG AN NHAM MENU LAUNCHES/SETTINGS) */
svg[viewBox="0 0 48 48"],
defs:has(#logoGradPrimary),
linearGradient#logoGradPrimary,
circle[fill="url(#logoGradGlow)"],
rect[stroke="url(#logoGradPrimary)"],
#left-menu > div > div:first-child {
    display: none !important;
    visibility: hidden !important;
    width: 0 !important;
    height: 0 !important;
    margin: 0 !important;
    padding: 0 !important;
    overflow: hidden !important;
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

/* Dam bao tat ca cac muc menu ben trai deu hien thi ro rang */
.blurMe {
    filter: none !important;
}

/* Main Workspace Card */
div[class*="min-h-screen"],
.flex-1.flex.gap-\\[8px\\] > div:last-child {
    background-color: #0d0f18 !important;
}

/* Khi loader component xuat hien, tu dong an sau 0.8s neu chua an */
div.bg-newBgColorInner:has(div[style*="border-top-color: rgb(97, 43, 211)"]) {
    display: none !important;
}
</style>
"""

# Xoa cac the mock script va style cu
html = re.sub(r'<script id="postiz-local-api-mock">.*?</script>', '', html, flags=re.DOTALL)
html = re.sub(r'<style id="postiz-concat-refined-theme">.*?</style>', '', html, flags=re.DOTALL)

# Insert vao sau <head>
html = html.replace('<head>', f'<head>\n{mock_api}\n{clean_style}')

with open(index_path, "w", encoding="utf-8") as f:
    f.write(html)

launches_path = r"d:\capcutpolot\concat\resources\postiz-web\launches.html"
with open(launches_path, "w", encoding="utf-8") as f:
    f.write(html)

print("SUCCESS: index.html & launches.html updated with complete mock schema and restored sidebar!")

