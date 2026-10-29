/**
 * SLive TikTok Shop Auto-Pin Helper Content Script
 * Injected into TikTok Shop Streamer Dashboard
 */

(function () {
  console.log("%c[SLive Auto-Pin v1.0.4] Script initialized on TikTok Shop Dashboard (Tự động kiểm tra, ghim & ghim lại mỗi 30s)", "color: #00f0ff; font-weight: bold;");

  let SERVER_URL = "http://127.0.0.1:8010";
  const CANDIDATE_PORTS = [8010, 5000];
  let lastResolvedTime = 0;

  async function resolveServerUrl() {
    const now = Date.now();
    if (now - lastResolvedTime < 5000) return SERVER_URL;
    lastResolvedTime = now;
    for (const port of CANDIDATE_PORTS) {
      try {
        const controller = new AbortController();
        const tid = setTimeout(() => controller.abort(), 1200);
        const res = await fetch(`http://127.0.0.1:${port}/api/tiktok/status`, {
          method: "GET",
          signal: controller.signal
        });
        clearTimeout(tid);
        if (res.ok) {
          SERVER_URL = `http://127.0.0.1:${port}`;
          return SERVER_URL;
        }
      } catch (e) {}
    }
    return SERVER_URL;
  }

  const POLL_INTERVAL = 800; // 0.8s responsive polling
  let isPolling = false;
  let hudElement = null;
  let lastActionId = null;
  let currentPinnedInfo = { index: null, title: null };
  let autoSyncTimer1 = null;
  let autoSyncTimer2 = null;
  let lastReportedPinnedTitle = null;

  // Auto-repin 30s state
  let activeProductFromDesktop = null;
  let autoPinTiktokEnabled = true;
  let autoRepin30sEnabled = true;
  let recheckPinIntervalSeconds = 30;
  let lastPeriodicPinCheckTime = Date.now();
  let isRepinning = false;

  // 1. Create Floating HUD with Sync Cart button
  function createFloatingHUD() {
    if (document.getElementById("slive-pin-hud")) return;

    const savedPos = JSON.parse(localStorage.getItem("slive_hud_pos") || 'null');
    const hud = document.createElement("div");
    hud.id = "slive-pin-hud";
    hud.style.cssText = `
      position: fixed;
      ${savedPos ? `top: ${savedPos.top}px; left: ${savedPos.left}px;` : 'bottom: 16px; left: 16px;'}
      z-index: 999999;
      background: rgba(10, 16, 32, 0.94);
      border: 1px solid #00f0ff;
      border-radius: 10px;
      padding: 8px 12px;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
      color: #cbd5e1;
      font-size: 12px;
      box-shadow: 0 8px 28px rgba(0, 240, 255, 0.25);
      backdrop-filter: blur(10px);
      min-width: 230px;
      max-width: 300px;
      user-select: none;
    `;

    hud.innerHTML = `
      <div id="slive-hud-header" style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 5px; border-bottom: 1px solid rgba(0,240,255,0.2); padding-bottom: 4px; cursor: move;">
        <div style="display: flex; align-items: center; gap: 5px;">
          <span style="color: #64748b; font-size: 11px; cursor: move;" title="Kéo thả di chuyển">⋮⋮</span>
          <span id="slive-status-dot" style="width: 8px; height: 8px; border-radius: 50%; background: #ffaa00; display: inline-block; box-shadow: 0 0 6px #ffaa00;"></span>
          <strong style="color: #00f0ff; font-size: 12px;">SLive Auto-Pin <small style="font-size: 9px; color: #10b981; font-weight: normal;">v1.0.4</small></strong>
        </div>
        <div style="display: flex; align-items: center; gap: 5px;">
          <span id="slive-hud-badge" style="background: rgba(0,240,255,0.15); color: #00f0ff; font-size: 9.5px; padding: 1px 4px; border-radius: 4px;">Đang kết nối</span>
          <button id="slive-hud-toggle-btn" title="Thu nhỏ/Mở rộng" style="background: transparent; border: none; color: #94a3b8; cursor: pointer; font-size: 13px; padding: 0 2px;">−</button>
        </div>
      </div>
      <div id="slive-hud-body">
        <div id="slive-hud-content" style="font-size: 11px; line-height: 1.35;">
          <div>Trạng thái: <span id="slive-conn-text" style="color: #ffaa00;">Đang tìm app...</span></div>
          <div style="margin-top: 2px;">Đang ghim: <span id="slive-pinned-text" style="color: #fff; font-weight: bold;">Chưa có</span></div>
        </div>
        <div style="margin-top: 6px; padding-top: 5px; border-top: 1px solid rgba(255,255,255,0.08); display: flex; gap: 4px;">
          <button id="slive-btn-sync-cart" style="flex: 1; background: #00f0ff; color: #060913; border: none; padding: 4px 6px; border-radius: 4px; font-weight: 700; font-size: 10.5px; cursor: pointer; display: flex; align-items: center; justify-content: center; gap: 4px;">
            <span>📥</span> Đồng bộ giỏ sang App
          </button>
        </div>
      </div>
    `;

    document.body.appendChild(hud);
    hudElement = hud;

    // Draggable Logic
    const header = document.getElementById("slive-hud-header");
    if (header) {
      let isDragging = false;
      let startX, startY, initialLeft, initialTop;

      header.addEventListener("mousedown", (e) => {
        if (e.target.id === "slive-hud-toggle-btn") return;
        isDragging = true;
        startX = e.clientX;
        startY = e.clientY;
        const rect = hud.getBoundingClientRect();
        initialLeft = rect.left;
        initialTop = rect.top;
        hud.style.bottom = "auto";
        hud.style.right = "auto";
        hud.style.left = `${initialLeft}px`;
        hud.style.top = `${initialTop}px`;
      });

      window.addEventListener("mousemove", (e) => {
        if (!isDragging) return;
        const dx = e.clientX - startX;
        const dy = e.clientY - startY;
        const newLeft = Math.max(10, Math.min(window.innerWidth - hud.offsetWidth - 10, initialLeft + dx));
        const newTop = Math.max(10, Math.min(window.innerHeight - hud.offsetHeight - 10, initialTop + dy));
        hud.style.left = `${newLeft}px`;
        hud.style.top = `${newTop}px`;
      });

      window.addEventListener("mouseup", () => {
        if (isDragging) {
          isDragging = false;
          const rect = hud.getBoundingClientRect();
          localStorage.setItem("slive_hud_pos", JSON.stringify({ left: rect.left, top: rect.top }));
        }
      });
    }

    const toggleBtn = document.getElementById("slive-hud-toggle-btn");
    const hudBody = document.getElementById("slive-hud-body");
    if (toggleBtn && hudBody) {
      let isCollapsed = false;
      toggleBtn.addEventListener("click", () => {
        isCollapsed = !isCollapsed;
        hudBody.style.display = isCollapsed ? "none" : "block";
        toggleBtn.innerText = isCollapsed ? "+" : "−";
      });
    }

    const btnSync = document.getElementById("slive-btn-sync-cart");
    if (btnSync) {
      btnSync.addEventListener("click", () => {
        sendCartToDesktop(true);
      });
    }
  }

  function updateHUD(connected, message, pinnedTitle) {
    const dot = document.getElementById("slive-status-dot");
    const badge = document.getElementById("slive-hud-badge");
    const connText = document.getElementById("slive-conn-text");
    const pinnedText = document.getElementById("slive-pinned-text");

    if (dot && badge && connText && pinnedText) {
      if (connected) {
        dot.style.background = "#10b981";
        dot.style.boxShadow = "0 0 8px #10b981";
        badge.innerText = "ONLINE";
        badge.style.color = "#10b981";
        badge.style.background = "rgba(16,185,129,0.15)";
        connText.innerText = message || "Đã kết nối SLive Desktop";
        connText.style.color = "#10b981";
      } else {
        dot.style.background = "#ff0055";
        dot.style.boxShadow = "0 0 8px #ff0055";
        badge.innerText = "OFFLINE";
        badge.style.color = "#ff0055";
        badge.style.background = "rgba(255,0,85,0.15)";
        connText.innerText = message || "Không kết nối được SLive Desktop";
        connText.style.color = "#ff0055";
      }

      if (pinnedTitle) {
        pinnedText.innerText = pinnedTitle;
      }
    }
  }

  // 3. Helper: Find all genuine eCommerce product rows in the table
  function getAllProductRowsFromDOM() {
    // Strategy 1: Find all container rows containing product metadata
    const allDivs = Array.from(document.querySelectorAll('div, tr, li, section')).filter(el => {
      const txt = (el.innerText || '').toLowerCase();
      // Block static table headers, creation buttons, and promotional mini-games
      if (txt.includes("orders over") || txt.includes("product list in this live") || txt.includes("from topgia") || txt.includes("give free prizes") || txt.includes("tạo voucher") || txt.includes("lucky draw")) {
        return false;
      }

      // Check if this is a standalone pure voucher banner (without actual eCommerce goods/stock)
      const isPureVoucherBanner = (txt.includes("phiếu giảm giá") || txt.includes("mã giảm giá") || txt.includes("voucher") || txt.includes("coupon")) &&
                                  !txt.includes("in stock") && !txt.includes("tồn kho") && !txt.includes("requested demo") && !txt.includes("clicks");
      if (isPureVoucherBanner) {
        // User instruction: Không được ghim và đọc voucher độc lập, không để voucher làm lệch thứ tự sản phẩm bán hàng
        return false;
      }

      const hasStock = txt.includes("in stock") || txt.includes("tồn kho") || txt.includes("requested demo") || txt.includes("clicks") || txt.includes("hot deals");
      const hasPrice = /(?:\d{1,3}(?:[.,]\d{3})+)|(?:\d+\s*(?:đ|₫|k|vnd))/i.test(txt);
      const hasImg = el.querySelector('img');
      return hasStock && hasPrice && Boolean(hasImg) && txt.length < 500;
    });

    // Strategy 2: Filter to innermost unique containers (exact product row cards)
    let uniqueRows = allDivs.filter(row => {
      return !allDivs.some(other => other !== row && row.contains(other));
    });

    // Fallback Strategy 3: If uniqueRows is empty, find by thumbnail images
    if (uniqueRows.length === 0) {
      const imgs = Array.from(document.querySelectorAll('img')).filter(img => {
        const src = img.src || '';
        if (src.includes('avatar') || src.includes('icon') || src.includes('emoji')) return false;
        const rect = img.getBoundingClientRect();
        return (rect.width >= 20 && rect.width <= 250) || (img.naturalWidth >= 40);
      });

      imgs.forEach(img => {
        let p = img.parentElement;
        let d = 0;
        while (p && p !== document.body && d < 6) {
          const txt = (p.innerText || '').toLowerCase();
          if (txt.includes("orders over") || txt.includes("product list in this live") || txt.includes("from topgia") || txt.includes("tạo voucher")) break;
          const hasPrice = /(?:\d{1,3}(?:[.,]\d{3})+)|(?:\d+\s*(?:đ|₫|k|vnd))/i.test(txt);
          if (hasPrice && txt.length >= 10 && txt.length <= 400) {
            if (!uniqueRows.includes(p)) uniqueRows.push(p);
            break;
          }
          p = p.parentElement;
          d++;
        }
      });
    }

    // Sort strictly top-to-bottom
    uniqueRows.sort((a, b) => {
      return (a.getBoundingClientRect().top + window.scrollY) - (b.getBoundingClientRect().top + window.scrollY);
    });

    return uniqueRows;
  }

  // 4. Scrape all products from TikTok Shop Live DOM
  function scrapeCartProductsFromDOM() {
    const rows = getAllProductRowsFromDOM();
    const products = [];
    const seenTitles = new Set();

    rows.forEach((row, i) => {
      try {
        const text = row.innerText.trim();
        const lines = text.split('\n').map(l => l.trim()).filter(Boolean);

        // Price: e.g. 149.000đ, 128.999đ, 120.000đ, 87.000đ, 119.000đ
        let price = '';
        const priceMatch = text.match(/(?:₫|\$|VND)?\s*\d{1,3}(?:[.,]\d{3})+\s*(?:đ|₫|k|VND)?/i) || text.match(/\d+[\.,]?\d*\s*(?:k|đ|₫|VND)/i);
        if (priceMatch) price = priceMatch[0].trim();

        // Title
        let title = '';
        const bracketLine = lines.find(l => (l.includes('[') && l.includes(']')) || (l.startsWith('(') && l.includes(')')) || l.includes('【'));
        if (bracketLine) {
          title = bracketLine;
        } else {
          const clean = lines.filter(l => !/^\d{1,3}$/.test(l) && !/in stock|tồn kho|requested|clicks|attr|hot deals|pin|ghim|unpin|bỏ ghim/i.test(l) && (!price || !l.includes(price)));
          title = clean.reduce((a, b) => a.length >= b.length ? a : b, `Sản phẩm #${i + 1}`);
        }

        const lowerTitle = title.toLowerCase();
        // Ignore static headers, creation buttons, or pure non-product lines
        if (lowerTitle.includes("product list in this live") || lowerTitle.includes("tạo voucher") || lowerTitle.includes("give free prizes") || lowerTitle.includes("giveaway")) {
          return;
        }
        if (/^(?:voucher|mã giảm giá|phiếu giảm giá|coupon)$/i.test(title.trim())) {
          return;
        }

        const dedupeKey = title.substring(0, 25).toLowerCase();
        if (seenTitles.has(dedupeKey)) return;
        seenTitles.add(dedupeKey);

        const img = row.querySelector('img');

        // Robust pinned status check (English and Vietnamese)
        const rowLower = text.toLowerCase();
        const hasUnpinBtn = Array.from(row.querySelectorAll('button, a, [role="button"]')).some(b => {
          const bt = (b.innerText || b.textContent || '').toLowerCase().trim();
          const ba = (b.getAttribute('aria-label') || '').toLowerCase();
          const bt2 = (b.getAttribute('title') || '').toLowerCase();
          return bt === 'unpin' || bt === 'bỏ ghim' || bt === 'hủy ghim' || bt.includes('unpin') || bt.includes('bỏ ghim') || bt.includes('hủy ghim') ||
                 ba.includes('unpin') || ba.includes('bỏ ghim') || ba.includes('hủy ghim') ||
                 bt2.includes('unpin') || bt2.includes('bỏ ghim') || bt2.includes('hủy ghim');
        });
        const isPinned = hasUnpinBtn ||
          rowLower.includes('unpin') ||
          rowLower.includes('bỏ ghim') ||
          rowLower.includes('hủy ghim') ||
          rowLower.includes('đang ghim') ||
          rowLower.includes('pinned');

        // Extract product id from data attributes or links if available
        let extractedProdId = '';
        const idAttr = row.getAttribute('data-product-id') || row.getAttribute('data-id') || row.getAttribute('data-item-id');
        if (idAttr) {
          extractedProdId = idAttr;
        } else {
          const anyLink = row.querySelector('a[href*="product"], a[href*="goods"]');
          if (anyLink) {
            const m = anyLink.href.match(/(?:product|goods|id)[\/=_](\d{10,})/i);
            if (m) extractedProdId = m[1];
          }
        }

        // Sequential index assignment
        let cardIndex = products.length + 1;
        const numBadge = lines.find(l => /^#?\d{1,2}$/.test(l));
        if (numBadge) {
          const parsed = parseInt(numBadge.replace('#', ''), 10);
          if (parsed >= 1 && parsed <= 99) cardIndex = parsed;
        }

        products.push({
          index: cardIndex,
          title: title.substring(0, 120),
          price: price || 'Ưu đãi',
          image: img ? img.src : '',
          link: '',
          tiktok_product_id: extractedProdId,
          is_pinned: isPinned,
          auto_pin: true,
          enabled: true,
          _cardElement: row
        });
      } catch (e) {
        console.warn("[SLive Auto-Pin] Lỗi parse row:", e);
      }
    });

    // Ensure currently pinned product is placed at index 0 (slot #1 in live cart)
    products.sort((a, b) => (b.is_pinned ? 1 : 0) - (a.is_pinned ? 1 : 0));
    // Re-index cleanly 1, 2, 3...
    products.forEach((p, idx) => {
      p.index = idx + 1;
    });

    console.log(`[SLive Auto-Pin] Quét được ${products.length} sản phẩm thực từ TikTok Live (đã loại trừ voucher độc lập)`, products);
    return products;
  }

  // Floating Glass Toast on TikTok Shop
  function showGlassToast(msg, type = "info", icon = "✨") {
    let toast = document.getElementById("slive-glass-toast");
    if (!toast) {
      toast = document.createElement("div");
      toast.id = "slive-glass-toast";
      toast.style.cssText = `
        position: fixed;
        top: 24px;
        left: 50%;
        transform: translateX(-50%);
        z-index: 1000000;
        background: rgba(10, 20, 38, 0.96);
        border: 1px solid #00f0ff;
        border-radius: 30px;
        padding: 8px 20px;
        color: #fff;
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
        font-size: 13px;
        font-weight: 600;
        box-shadow: 0 8px 30px rgba(0, 240, 255, 0.4);
        backdrop-filter: blur(12px);
        display: flex;
        align-items: center;
        gap: 8px;
        pointer-events: none;
        transition: all 0.3s cubic-bezier(0.16, 1, 0.3, 1);
        opacity: 0;
      `;
      document.body.appendChild(toast);
    }

    if (type === "success") {
      toast.style.borderColor = "#10b981";
      toast.style.boxShadow = "0 8px 30px rgba(16, 185, 129, 0.4)";
    } else if (type === "error") {
      toast.style.borderColor = "#ff0055";
      toast.style.boxShadow = "0 8px 30px rgba(255, 0, 85, 0.4)";
    } else {
      toast.style.borderColor = "#00f0ff";
      toast.style.boxShadow = "0 8px 30px rgba(0, 240, 255, 0.4)";
    }

    toast.innerHTML = `<span style="font-size: 15px;">${icon}</span> <span>${msg}</span>`;
    toast.style.opacity = "1";
    toast.style.top = "28px";

    setTimeout(() => {
      toast.style.opacity = "0";
      toast.style.top = "18px";
    }, 2800);
  }

  // 4. Send Cart to Desktop
  async function sendCartToDesktop(manualClick = false, isAutoUpdateAfterPin = false) {
    const rawProducts = scrapeCartProductsFromDOM();

    if (rawProducts.length === 0) {
      console.warn("[SLive Auto-Pin] Không có sản phẩm nào trong giỏ hàng TikTok.");
      // If it's a background auto update, do not send empty list to prevent accidental wipe
      if (!manualClick && !isAutoUpdateAfterPin) {
        return;
      }
      try {
        await fetch(`${SERVER_URL}/api/tiktok/sync_cart`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            products: [],
            total: 0,
            allow_empty: manualClick, // only allow wiping if user explicitly requested manual sync
            timestamp: Date.now()
          })
        });
      } catch (e) { }
      updateHUD(true, "Giỏ hàng TikTok đang trống (0 SP)", "Chưa có");
      if (manualClick) {
        showGlassToast("Giỏ hàng trên TikTok hiện đang trống (0 sản phẩm).", "info", "🛒");
      }
      return;
    }

    // Clean up DOM references before sending JSON
    const products = rawProducts.map(p => ({
      index: p.index,
      title: p.title,
      price: p.price,
      image: p.image,
      link: p.link,
      tiktok_product_id: p.tiktok_product_id,
      is_pinned: p.is_pinned,
      auto_pin: p.auto_pin,
      enabled: p.enabled
    }));

    try {
      const res = await fetch(`${SERVER_URL}/api/tiktok/sync_cart`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          products: products,
          total: products.length,
          allow_empty: false,
          timestamp: Date.now()
        })
      });

      if (res.ok) {
        const pinnedProd = products.find(p => p.is_pinned);
        const pinnedTitle = pinnedProd ? `#${pinnedProd.index}: ${pinnedProd.title}` : (currentPinnedInfo.title || 'Chưa ghim');
        console.log(`[SLive Auto-Pin] Đã gửi thành công ${products.length} sản phẩm sang SLive Desktop! (Ghim: ${pinnedTitle})`);
        updateHUD(true, `Đã đồng bộ ${products.length} SP`, pinnedTitle);
        if (manualClick) {
          showGlassToast(`Đã đồng bộ thành công ${products.length} sản phẩm sang App!`, "success", "✅");
        } else if (isAutoUpdateAfterPin) {
          showGlassToast(`Đã cập nhật lại vị trí giỏ hàng (${products.length} SP)`, "success", "🔄");
        }
      }
    } catch (e) {
      console.error("[SLive Auto-Pin] Lỗi gửi giỏ hàng:", e);
      if (manualClick) {
        showGlassToast("Không thể kết nối SLive Desktop (127.0.0.1:8010)", "error", "⚠️");
      }
    }
  }

  // Helper: Schedule automatic cart re-sync after a product is pinned / pin switches
  function scheduleAutoSyncCartAfterPin(reason = "") {
    if (autoSyncTimer1) clearTimeout(autoSyncTimer1);
    if (autoSyncTimer2) clearTimeout(autoSyncTimer2);

    console.log(`[SLive Auto-Pin] Lên lịch tự động cập nhật giỏ hàng: ${reason}`);

    // First attempt at 1500ms (TikTok takes ~1-1.5s to confirm modal and re-render DOM)
    autoSyncTimer1 = setTimeout(async () => {
      console.log("[SLive Auto-Pin] Tự động cập nhật danh sách giỏ hàng lần 1 (1.5s)...");
      await sendCartToDesktop(false, true);
    }, 1500);

    // Second check at 3000ms for slow networks
    autoSyncTimer2 = setTimeout(async () => {
      console.log("[SLive Auto-Pin] Tự động cập nhật danh sách giỏ hàng lần 2 (3.0s)...");
      await sendCartToDesktop(false, false);
    }, 3000);
  }

  // 5. Helper: Click Pin button on genuine product row
  function clickPinButton(btn, rowElement, targetIndex, targetTitle) {
    if (!btn) return false;

    // Safety check: Ensure btn does NOT belong to table headers, voucher buttons, or pure standalone voucher cards
    let checkParent = btn;
    for (let i = 0; i < 5; i++) {
      if (!checkParent) break;
      const txt = (checkParent.innerText || '').toLowerCase();
      if (txt.includes("product list in this live") || txt.includes("from topgia") || txt.includes("tạo voucher")) {
        console.error("[SLive Auto-Pin] TỪ CHỐI BẤM: Nút này thuộc Tiêu đề bảng hoặc nút Tạo voucher!", txt.substring(0, 40));
        return false;
      }
      // Pure standalone voucher banner check (no stock, no product metrics)
      const isPureVoucher = (txt.includes("phiếu giảm giá") || txt.includes("mã giảm giá") || txt.includes("voucher") || txt.includes("coupon")) &&
                            !txt.includes("in stock") && !txt.includes("tồn kho") && !txt.includes("requested demo") && !txt.includes("clicks");
      if (isPureVoucher) {
        console.warn("[SLive Auto-Pin] TỪ CHỐI BẤM: Thẻ voucher độc lập (không ghim voucher theo cấu hình).");
        return false;
      }
      checkParent = checkParent.parentElement;
    }

    // Check if already pinned (Unpin / Bỏ ghim text)
    const text = (btn.innerText || '').toLowerCase().trim();
    if (text === "unpin" || text === "bỏ ghim" || text === "hủy ghim" || text.includes("unpin") || text.includes("bỏ ghim")) {
      console.log(`[SLive Auto-Pin] Sản phẩm "${targetTitle}" ĐÃ ĐƯỢC GHIM SẴN.`);
      currentPinnedInfo = { index: targetIndex, title: targetTitle || 'Sản phẩm' };
      updateHUD(true, `Đang ghim: ${targetTitle}`, currentPinnedInfo.title);
      scheduleAutoSyncCartAfterPin(`Sản phẩm #${targetIndex} đã ghim sẵn, đồng bộ lại thứ tự`);
      return true;
    }

    // Scroll smoothly into view
    if (rowElement) {
      rowElement.scrollIntoView({ behavior: 'smooth', block: 'center' });
    } else {
      btn.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }

    console.log(`[SLive Auto-Pin] BẤM NÚT GHIM CHUẨN XÁC cho sản phẩm #${targetIndex}...`, btn);
    btn.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    btn.click();

    setTimeout(handleConfirmModal, 300);
    setTimeout(handleConfirmModal, 800);

    currentPinnedInfo = { index: targetIndex, title: targetTitle || `Sản phẩm #${targetIndex}` };
    updateHUD(true, `Đã ghim #${targetIndex}`, currentPinnedInfo.title);

    // Automatically re-sync cart after pinning!
    scheduleAutoSyncCartAfterPin(`Đã bấm ghim SP #${targetIndex}`);
    return true;
  }

  // 6. Auto-click confirmation modals if shown
  function handleConfirmModal() {
    const modals = Array.from(document.querySelectorAll('.arco-modal, .tiktok-modal, div[role="dialog"]'));
    for (let modal of modals) {
      if (modal.offsetParent !== null) {
        const confirmButtons = Array.from(modal.querySelectorAll('button'));
        for (let btn of confirmButtons) {
          const text = btn.innerText.toLowerCase().trim();
          if (text === "xác nhận" || text === "đồng ý" || text === "confirm" || text === "ok" || text.includes("xác nhận")) {
            console.log("[SLive Auto-Pin] Tự động bấm Xác nhận đổi ghim trên modal...", btn);
            btn.click();
            return;
          }
        }
      }
    }
  }

  // Helpers for intelligent product matching
  function normalizeTitle(text) {
    if (!text) return "";
    return text
      .toLowerCase()
      .replace(/\[[^\]]*\]/g, " ")
      .replace(/\([^\)]*\)/g, " ")
      .replace(/【[^】]*】/g, " ")
      .replace(/[^\p{L}\p{N}\s]/gu, " ")
      .replace(/\s+/g, " ")
      .trim();
  }

  function getTitleKeywords(text) {
    const clean = normalizeTitle(text);
    const stopWords = new Set([
      "chính", "hãng", "cao", "cấp", "loại", "mới", "nhất", "siêu", "đẹp", "tốt",
      "hàng", "ngày", "combo", "deal", "giá", "rẻ", "hot", "sản", "phẩm", "sp", "số",
      "chỉ", "còn", "ưu", "đãi", "quà", "tặng", "chất", "lượng", "auth"
    ]);
    return clean.split(" ").filter(w => w.length >= 2 && !stopWords.has(w));
  }

  function findProductRow(rows, targetTitle, targetIndex, targetProdId) {
    const normTarget = normalizeTitle(targetTitle);
    const targetKeywords = getTitleKeywords(targetTitle);

    let bestRow = null;
    let bestScore = 0;
    let bestReason = "";

    for (let i = 0; i < rows.length; i++) {
      const row = rows[i];
      const rowText = (row.innerText || "").toLowerCase();
      const normRowText = normalizeTitle(row.innerText || "");
      let score = 0;
      let reason = "";

      // 1. Direct Product ID match (Highest priority)
      if (targetProdId && targetProdId.length >= 5) {
        const hasIdInAttr = Array.from(row.attributes || []).some(a => (a.value || "").includes(targetProdId));
        if (rowText.includes(targetProdId) || hasIdInAttr) {
          console.log(`[SLive Auto-Pin] Khớp tuyệt đối theo Product ID: ${targetProdId}`);
          return { row, score: 1000, reason: "Product ID match" };
        }
      }

      // 2. Exact normalized title substring match
      if (normTarget.length >= 6 && normRowText.includes(normTarget)) {
        score += 350;
        reason = "Khớp trọn vẹn tiêu đề sản phẩm";
      } else if (normTarget.length >= 15 && normRowText.includes(normTarget.substring(0, 25))) {
        score += 250;
        reason = "Khớp phần đầu tiêu đề sản phẩm";
      }

      // 3. Keyword / Token overlap match
      if (targetKeywords.length > 0) {
        let matchedCount = 0;
        for (const kw of targetKeywords) {
          if (normRowText.includes(kw)) {
            matchedCount++;
          }
        }
        const ratio = matchedCount / targetKeywords.length;
        if (ratio >= 0.35) {
          score += ratio * 200;
          if (!reason) reason = `Trùng khớp ${matchedCount}/${targetKeywords.length} từ khóa (${Math.round(ratio * 100)}%)`;
        }
      }

      // 4. Cart serial badge match (e.g. badge #3 or exact number 3)
      if (targetIndex >= 1) {
        const lines = (row.innerText || "").split("\n").map(l => l.trim());
        const hasExactBadge = lines.some(l => l === String(targetIndex) || l === `#${targetIndex}` || l === `sp${targetIndex}`);
        if (hasExactBadge) {
          score += 70;
          if (!reason) reason += ` + STT #${targetIndex}`;
        }
      }

      if (score > bestScore) {
        bestScore = score;
        bestRow = row;
        bestReason = reason;
      }
    }

    if (bestRow && bestScore >= 45) {
      console.log(`[SLive Auto-Pin] Tìm thấy dòng khớp nhất (Điểm: ${Math.round(bestScore)}, Lý do: ${bestReason})`);
      return { row: bestRow, score: bestScore, reason: bestReason };
    }

    // Fallback: Nếu không tìm thấy theo tên và có targetIndex
    if (targetIndex >= 1 && targetIndex <= rows.length) {
      console.warn(`[SLive Auto-Pin] Không khớp tên, sử dụng fallback dòng vị trí #${targetIndex}`);
      return { row: rows[targetIndex - 1], score: 10, reason: "Fallback index" };
    }

    return { row: null, score: 0, reason: "Không tìm thấy" };
  }

  // Helper: Tìm nút Bỏ ghim / Unpin trên một dòng sản phẩm
  function findUnpinButton(rowElement) {
    if (!rowElement) return null;
    const buttons = Array.from(rowElement.querySelectorAll('button, a, div[role="button"], span[role="button"], .arco-btn, [class*="pin"], [class*="Pin"]'));
    for (let b of buttons) {
      const t = (b.innerText || b.textContent || '').toLowerCase().trim();
      const aria = (b.getAttribute('aria-label') || '').toLowerCase().trim();
      const titleAttr = (b.getAttribute('title') || '').toLowerCase().trim();
      if (t === "unpin" || t === "bỏ ghim" || t === "hủy ghim" || t.includes("unpin") || t.includes("bỏ ghim") || t.includes("hủy ghim") ||
          aria.includes("unpin") || aria.includes("bỏ ghim") || aria.includes("hủy ghim") ||
          titleAttr.includes("unpin") || titleAttr.includes("bỏ ghim") || titleAttr.includes("hủy ghim")) {
        return b;
      }
    }
    return null;
  }

  // Helper: Tìm nút Ghim / Pin trên một dòng sản phẩm (bỏ qua nút bỏ ghim)
  function findPinButton(rowElement) {
    if (!rowElement) return null;
    const buttons = Array.from(rowElement.querySelectorAll('button, a, div[role="button"], span[role="button"], .arco-btn, [class*="pin"], [class*="Pin"]'));
    for (let b of buttons) {
      const t = (b.innerText || b.textContent || '').toLowerCase().trim();
      const aria = (b.getAttribute('aria-label') || '').toLowerCase().trim();
      const titleAttr = (b.getAttribute('title') || '').toLowerCase().trim();
      if (t.includes("unpin") || t.includes("bỏ ghim") || t.includes("hủy ghim") || aria.includes("unpin") || titleAttr.includes("bỏ ghim")) {
        continue;
      }
      if (t === "pin" || t === "ghim" || t.includes("pin to top") || t.includes("ghim lên đầu") || aria.includes("pin") || aria.includes("ghim") || titleAttr.includes("ghim") || titleAttr.includes("pin")) {
        return b;
      }
    }
    for (let b of buttons) {
      const t = (b.innerText || b.textContent || '').toLowerCase().trim();
      if (t.includes("xóa") || t.includes("sửa") || t.includes("chi tiết") || t.includes("copy") || t.includes("share") || t.includes("unpin") || t.includes("bỏ ghim")) {
        continue;
      }
      if (t.includes("ghim") || t.includes("pin")) {
        return b;
      }
      if (b.querySelector('svg') && (b.className.toLowerCase().includes('pin') || b.innerHTML.toLowerCase().includes('pin'))) {
        return b;
      }
    }
    return null;
  }

  // Helper: Kiểm tra dòng sản phẩm có đang ở trạng thái đã ghim hay không
  function isRowPinned(rowElement) {
    if (!rowElement) return false;
    return Boolean(findUnpinButton(rowElement));
  }

  // 6C. Thực hiện BỎ GHIM và GHIM LẠI (Unpin & Re-pin) sản phẩm đang nói sau mỗi 30s
  async function performUnpinAndRepin(actionData) {
    if (isRepinning) {
      console.log("[SLive Auto-Pin] Đang trong quá trình ghim lại, bỏ qua yêu cầu trùng.");
      return false;
    }
    isRepinning = true;
    lastPeriodicPinCheckTime = Date.now();

    try {
      const targetIndex = parseInt(actionData.index, 10);
      const targetTitle = (actionData.title || "").trim();
      const targetProdId = (actionData.product_id || "").trim();

      console.log(`[SLive Auto-Pin] [Chu kỳ ${recheckPinIntervalSeconds}s] BẮT ĐẦU BỎ GHIM & GHIM LẠI SP #${targetIndex}: "${targetTitle}"`);
      showGlassToast(`Đang làm mới ghim SP #${targetIndex}...`, "info", "🔄");

      const rows = getAllProductRowsFromDOM();
      if (rows.length === 0) {
        console.warn("[SLive Auto-Pin] Không tìm thấy danh sách sản phẩm trên DOM.");
        return false;
      }

      const matchResult = findProductRow(rows, targetTitle, targetIndex, targetProdId);
      let targetRow = matchResult.row;
      if (!targetRow) {
        console.warn(`[SLive Auto-Pin] Không tìm thấy dòng sản phẩm để ghim lại: "${targetTitle}"`);
        return false;
      }

      // Bước 1: Tìm và bấm nút BỎ GHIM (Unpin)
      const unpinBtn = findUnpinButton(targetRow);
      if (unpinBtn) {
        console.log(`[SLive Auto-Pin] Bấm nút BỎ GHIM SP #${targetIndex}...`, unpinBtn);
        unpinBtn.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
        unpinBtn.click();
        setTimeout(handleConfirmModal, 300);
        setTimeout(handleConfirmModal, 700);
      } else {
        console.log(`[SLive Auto-Pin] SP #${targetIndex} chưa có nút Bỏ ghim, chuyển sang ghim trực tiếp.`);
      }

      // Bước 2: Chờ TikTok DOM cập nhật nút từ "Bỏ ghim" sang "Ghim" (polling tối đa 3s)
      let pinBtn = null;
      const startTime = Date.now();
      while (Date.now() - startTime < 3000) {
        await new Promise(r => setTimeout(r, 350));
        const updatedRows = getAllProductRowsFromDOM();
        const updatedMatch = findProductRow(updatedRows, targetTitle, targetIndex, targetProdId);
        if (updatedMatch.row) {
          targetRow = updatedMatch.row;
          pinBtn = findPinButton(targetRow);
          if (pinBtn) break;
        }
      }

      // Bước 3: Bấm GHIM LẠI (Re-pin)
      if (pinBtn) {
        console.log(`[SLive Auto-Pin] BẤM GHIM LẠI SP #${targetIndex} thành công!`, pinBtn);
        pinBtn.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
        pinBtn.click();
        setTimeout(handleConfirmModal, 300);
        setTimeout(handleConfirmModal, 800);
        setTimeout(handleConfirmModal, 1500);

        currentPinnedInfo = { index: targetIndex, title: targetTitle || `Sản phẩm #${targetIndex}` };
        updateHUD(true, `Đã ghim lại #${targetIndex}`, currentPinnedInfo.title);
        showGlassToast(`Đã ghim lại SP #${targetIndex} theo kịch bản!`, "success", "📌");
        scheduleAutoSyncCartAfterPin(`Đã ghim lại SP #${targetIndex} (chu kỳ ${recheckPinIntervalSeconds}s)`);
        return true;
      } else {
        console.warn(`[SLive Auto-Pin] Không tìm thấy nút Ghim để ghim lại SP #${targetIndex}`);
        showGlassToast(`Chưa thấy nút Ghim lại cho SP #${targetIndex}`, "info", "⚠️");
        return false;
      }
    } catch (err) {
      console.error("[SLive Auto-Pin] Lỗi trong quá trình bỏ ghim & ghim lại:", err);
      return false;
    } finally {
      setTimeout(() => {
        isRepinning = false;
      }, 2000);
    }
  }

  // 7. Pin Action Router
  function performPin(actionData) {
    if (actionData.action === "REPIN" || actionData.action === "UNPIN_AND_REPIN" || actionData.forceRepin) {
      return performUnpinAndRepin(actionData);
    }

    const targetIndex = parseInt(actionData.index, 10);
    const targetTitle = (actionData.title || "").trim();
    const targetProdId = (actionData.product_id || "").trim();

    console.log(`[SLive Auto-Pin] Nhận lệnh ghim: #${targetIndex} - "${targetTitle}" (ID: ${targetProdId || 'none'})`);

    const rows = getAllProductRowsFromDOM();
    console.log(`[SLive Auto-Pin] Tổng số dòng sản phẩm trong DOM: ${rows.length}`);

    if (rows.length === 0) {
      console.warn("[SLive Auto-Pin] Không tìm thấy dòng sản phẩm nào trên DOM");
      showGlassToast(`Không tìm thấy sản phẩm trên TikTok Live`, "error", "⚠️");
      return false;
    }

    // Tìm dòng sản phẩm thông minh theo TIÊU ĐỀ và MÃ SP trước
    const matchResult = findProductRow(rows, targetTitle, targetIndex, targetProdId);
    let targetRow = matchResult.row;

    if (!targetRow) {
      console.warn(`[SLive Auto-Pin] Không tìm thấy dòng sản phẩm: "${targetTitle}"`);
      showGlassToast(`Không tìm thấy "${targetTitle.substring(0, 25)}..."`, "error", "⚠️");
      return false;
    }

    // Scroll into view smoothly
    targetRow.scrollIntoView({ behavior: 'smooth', block: 'center' });

    // Look for Pin buttons inside targetRow
    const buttons = Array.from(targetRow.querySelectorAll('button, a, div[role="button"], span[role="button"], [class*="pin"], [class*="Pin"]'));
    let pinBtn = null;

    for (let b of buttons) {
      const t = (b.innerText || '').toLowerCase().trim();
      const aria = (b.getAttribute('aria-label') || '').toLowerCase().trim();
      const titleAttr = (b.getAttribute('title') || '').toLowerCase().trim();
      if (t === "pin" || t === "ghim" || t.includes("pin to top") || aria.includes("pin") || aria.includes("ghim") || titleAttr.includes("ghim") || titleAttr.includes("pin")) {
        pinBtn = b;
        break;
      }
      if (t === "unpin" || t === "bỏ ghim" || t.includes("unpin") || aria.includes("unpin") || titleAttr.includes("bỏ ghim")) {
        console.log(`[SLive Auto-Pin] Sản phẩm "${targetTitle}" ĐÃ ĐƯỢC GHIM SẴN.`);
        currentPinnedInfo = { index: targetIndex, title: targetTitle || 'Sản phẩm' };
        updateHUD(true, `Đang ghim: ${targetTitle}`, currentPinnedInfo.title);
        showGlassToast(`Đang ghim: "${(targetTitle || 'Sản phẩm').substring(0, 25)}..."`, "info", "📌");
        return true;
      }
    }

    if (!pinBtn) {
      const allButtonsInRow = Array.from(targetRow.querySelectorAll('button, [role="button"]'));
      for (let b of allButtonsInRow) {
        const text = (b.innerText || '').toLowerCase().trim();
        if (text.includes("xóa") || text.includes("sửa") || text.includes("chi tiết") || text.includes("copy") || text.includes("share")) {
          continue;
        }
        if (text.includes("ghim") || text.includes("pin")) {
          pinBtn = b;
          break;
        }
        if (b.querySelector('svg') && (b.className.toLowerCase().includes('pin') || b.innerHTML.toLowerCase().includes('pin'))) {
          pinBtn = b;
          break;
        }
      }
    }

    if (!pinBtn) {
      const svg = targetRow.querySelector('svg');
      if (svg) pinBtn = svg.closest('button, [role="button"], div');
    }

    if (pinBtn) {
      return clickPinButton(pinBtn, targetRow, targetIndex, targetTitle);
    } else {
      console.warn(`[SLive Auto-Pin] Không tìm thấy nút Ghim trên dòng: "${targetTitle}"`);
      showGlassToast(`Chưa thấy nút Ghim cho "${targetTitle.substring(0, 20)}..."`, "info", "📌");
      return false;
    }
  }

  // 8. Polling loop connecting to SLive Desktop
  async function pollServer() {
    try {
      // Check currently pinned product from DOM
      const prods = scrapeCartProductsFromDOM();
      const pinnedProd = prods.find(p => p.is_pinned);
      if (pinnedProd) {
        if (currentPinnedInfo.title !== pinnedProd.title) {
          console.log(`[SLive Auto-Pin] Trạng thái ghim thay đổi trên TikTok: #${pinnedProd.index} - ${pinnedProd.title}`);
          if (lastReportedPinnedTitle !== null && lastReportedPinnedTitle !== pinnedProd.title) {
            scheduleAutoSyncCartAfterPin("Phát hiện sản phẩm ghim trên TikTok thay đổi");
          }
          lastReportedPinnedTitle = pinnedProd.title;
        }
        currentPinnedInfo = { index: pinnedProd.index, title: pinnedProd.title };
      } else {
        if (!isRepinning) {
          if (currentPinnedInfo.title !== null) {
            console.log(`[SLive Auto-Pin] Không còn sản phẩm nào ghim trên TikTok (hết 30s hoặc đã bị hạ ghim)`);
            lastReportedPinnedTitle = null;
          }
          currentPinnedInfo = { index: null, title: null };
        }
      }

      const reportRes = await fetch(`${SERVER_URL}/api/tiktok/report_status`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          version: "1.0.4",
          url: window.location.href,
          current_pinned_index: currentPinnedInfo.index,
          current_pinned_title: currentPinnedInfo.title,
          timestamp: Date.now()
        })
      });

      if (reportRes.ok) {
        const reportData = await reportRes.json();
        if (reportData.active_product !== undefined) {
          activeProductFromDesktop = reportData.active_product;
        }
        if (reportData.auto_pin_tiktok !== undefined) {
          autoPinTiktokEnabled = Boolean(reportData.auto_pin_tiktok);
        }
        if (reportData.auto_repin_30s !== undefined) {
          autoRepin30sEnabled = Boolean(reportData.auto_repin_30s);
        }
        if (reportData.recheck_pin_interval !== undefined) {
          recheckPinIntervalSeconds = parseInt(reportData.recheck_pin_interval, 10) || 30;
        }
      } else {
        throw new Error(`HTTP ${reportRes.status}`);
      }

      const actionsRes = await fetch(`${SERVER_URL}/api/tiktok/pending_actions`);
      if (actionsRes.ok) {
        const data = await actionsRes.json();
        const actions = data.actions || [];

        if (data.active_product !== undefined) {
          activeProductFromDesktop = data.active_product;
        }
        if (data.auto_pin_tiktok !== undefined) {
          autoPinTiktokEnabled = Boolean(data.auto_pin_tiktok);
        }
        if (data.auto_repin_30s !== undefined) {
          autoRepin30sEnabled = Boolean(data.auto_repin_30s);
        }
        if (data.recheck_pin_interval !== undefined) {
          recheckPinIntervalSeconds = parseInt(data.recheck_pin_interval, 10) || 30;
        }

        for (let action of actions) {
          if (action.action_id && action.action_id !== lastActionId) {
            lastActionId = action.action_id;
            console.log("[SLive Auto-Pin] Nhận lệnh từ SLive Desktop:", action);

            if (action.action === "PIN") {
              performPin(action);
            } else if (action.action === "REPIN" || action.action === "UNPIN_AND_REPIN") {
              performUnpinAndRepin(action);
            } else if (action.action === "SYNC_CART") {
              sendCartToDesktop(true);
            }
          }
        }
        updateHUD(true, `Đã kết nối SLive (${SERVER_URL})`, currentPinnedInfo.title);
      }
    } catch (err) {
      await resolveServerUrl();
      updateHUD(false, `Không kết nối được app SLive (${SERVER_URL})`, null);
    }
  }

  // 8B. Periodic 30s check:
  // - Nếu chưa ghim: ghim sản phẩm đang nói theo kịch bản
  // - Nếu đã ghim rồi: bỏ ghim và ghim lại sản phẩm đang nói sau mỗi 30s
  // - Đảm bảo ghim đúng sản phẩm với kịch bản
  function checkAndRepinActiveProductIfNeeded() {
    if (!autoPinTiktokEnabled || !autoRepin30sEnabled || !activeProductFromDesktop) {
      return;
    }
    if (activeProductFromDesktop.auto_pin === false) {
      return;
    }
    if (isRepinning) {
      return;
    }

    const intervalMs = Math.max(10, recheckPinIntervalSeconds) * 1000;
    const now = Date.now();
    if (now - lastPeriodicPinCheckTime < intervalMs) {
      return;
    }

    lastPeriodicPinCheckTime = now;

    console.log(`[SLive Auto-Pin] [Định kỳ ${recheckPinIntervalSeconds}s] Kiểm tra sản phẩm đang nói theo kịch bản: #${activeProductFromDesktop.index} "${activeProductFromDesktop.title}"...`);

    const rows = getAllProductRowsFromDOM();
    if (rows.length === 0) return;

    const matchResult = findProductRow(rows, activeProductFromDesktop.title, activeProductFromDesktop.index, activeProductFromDesktop.tiktok_product_id);
    const targetRow = matchResult.row;
    if (!targetRow) {
      console.warn(`[SLive Auto-Pin] [Định kỳ ${recheckPinIntervalSeconds}s] Không tìm thấy dòng sản phẩm: "${activeProductFromDesktop.title}"`);
      return;
    }

    const pinnedAlready = isRowPinned(targetRow);

    if (!pinnedAlready) {
      console.log(`[SLive Auto-Pin] [Định kỳ ${recheckPinIntervalSeconds}s] Sản phẩm đang nói #${activeProductFromDesktop.index} CHƯA ĐƯỢC GHIM. Đang tiến hành ghim...`);
      showGlassToast(`Đang ghim SP #${activeProductFromDesktop.index} theo kịch bản`, "success", "📌");
      performPin({
        index: activeProductFromDesktop.index,
        title: activeProductFromDesktop.title,
        product_id: activeProductFromDesktop.tiktok_product_id
      });
    } else {
      console.log(`[SLive Auto-Pin] [Định kỳ ${recheckPinIntervalSeconds}s] Sản phẩm đang nói #${activeProductFromDesktop.index} ĐÃ ĐƯỢC GHIM. Đang thực hiện BỎ GHIM & GHIM LẠI...`);
      performUnpinAndRepin({
        index: activeProductFromDesktop.index,
        title: activeProductFromDesktop.title,
        product_id: activeProductFromDesktop.tiktok_product_id
      });
    }
  }

  // 9. Listen for manual Pin / Unpin clicks directly on TikTok Shop page
  document.addEventListener("click", (e) => {
    const target = e.target;
    if (!target) return;
    const btn = target.closest('button, a, div[role="button"], span[role="button"], .arco-btn');
    if (!btn) return;
    if (btn.closest('#slive-pin-hud') || btn.closest('#slive-glass-toast')) return;

    const btnText = (btn.innerText || btn.textContent || '').toLowerCase().trim();
    const ariaLabel = (btn.getAttribute('aria-label') || '').toLowerCase();
    const titleAttr = (btn.getAttribute('title') || '').toLowerCase();

    const isPinClick = 
      btnText === "pin" || btnText === "ghim" || btnText.includes("pin to top") || btnText.includes("ghim lên đầu") ||
      btnText === "unpin" || btnText === "bỏ ghim" || btnText.includes("hủy ghim") ||
      ariaLabel.includes("pin") || ariaLabel.includes("ghim") ||
      titleAttr.includes("pin") || titleAttr.includes("ghim");

    const isConfirmClick = 
      (btnText === "xác nhận" || btnText === "đồng ý" || btnText === "confirm" || btnText === "ok") &&
      Boolean(btn.closest('.arco-modal, .tiktok-modal, div[role="dialog"]'));

    if (isPinClick || isConfirmClick) {
      console.log("[SLive Auto-Pin] Phát hiện thao tác ghim / bỏ ghim trên TikTok:", btnText || ariaLabel);
      scheduleAutoSyncCartAfterPin("Thao tác người dùng trên TikTok");
    }
  }, true);

  function init() {
    createFloatingHUD();
    setInterval(pollServer, POLL_INTERVAL);
    setInterval(checkAndRepinActiveProductIfNeeded, 2000);
    pollServer();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
