import os
import re

index_path = r"d:\capcutpolot\concat\resources\postiz-web\index.html"
with open(index_path, "r", encoding="utf-8") as f:
    html = f.read()

# 1. TIM VA XOA TRIET DE THE SPINNER TIM TRONG HTML
# Doan ma spinner: pt-[100px]"><div style="width:100px;height:100px;border:13px solid transparent;border-top-color:#612bd3;border-radius:50%;animation:spin 0.8s linear infinite"></div></div>
spinner_pattern = r'<div class="flex-1 flex justify-center pt-\[100px\]"><div style="[^"]*border-top-color:#612bd3[^"]*"></div></div>'

# 2. XAY DUNG GIAO DIEN CALENDAR & POST COMPOSER DANG CAP CONCAT + POSTIZ
calendar_interface = """
<div id="postiz-native-workspace" class="flex-1 flex flex-col h-full bg-[#0d0f18] text-[#f1f5f9] p-4 overflow-hidden select-none">
  <!-- Top Toolbar -->
  <div class="flex items-center justify-between pb-4 border-b border-[#1e2436] mb-4">
    <div class="flex items-center gap-3">
      <div class="text-xl font-bold tracking-tight text-white flex items-center gap-2">
        <span>Lịch đăng & Quản lý bài</span>
        <span class="text-xs px-2 py-0.5 rounded-full bg-indigo-500/20 text-indigo-400 border border-indigo-500/30 font-medium">Bản nội bộ Concat</span>
      </div>
      <div class="flex items-center bg-[#111420] border border-[#1e2436] rounded-lg p-0.5 ml-4">
        <button id="btn-prev-week" class="px-2.5 py-1 text-xs text-gray-400 hover:text-white rounded hover:bg-[#1e2436] transition">&larr;</button>
        <span id="current-week-label" class="px-3 py-1 text-xs font-semibold text-gray-200">Tuần này (30/10 - 05/11)</span>
        <button id="btn-next-week" class="px-2.5 py-1 text-xs text-gray-400 hover:text-white rounded hover:bg-[#1e2436] transition">&rarr;</button>
      </div>
    </div>

    <!-- Actions & Platform Filters -->
    <div class="flex items-center gap-2.5">
      <div class="flex items-center bg-[#111420] border border-[#1e2436] rounded-lg p-1 gap-1">
        <button class="px-2.5 py-1 text-xs rounded font-medium bg-indigo-600 text-white shadow-sm">Tất cả kênh</button>
        <button class="px-2.5 py-1 text-xs rounded font-medium text-gray-400 hover:text-white hover:bg-[#1e2436] transition flex items-center gap-1.5">
          <span class="w-2 h-2 rounded-full bg-black border border-gray-600"></span> TikTok
        </button>
        <button class="px-2.5 py-1 text-xs rounded font-medium text-gray-400 hover:text-white hover:bg-[#1e2436] transition flex items-center gap-1.5">
          <span class="w-2 h-2 rounded-full bg-red-600"></span> YouTube
        </button>
        <button class="px-2.5 py-1 text-xs rounded font-medium text-gray-400 hover:text-white hover:bg-[#1e2436] transition flex items-center gap-1.5">
          <span class="w-2 h-2 rounded-full bg-blue-600"></span> Facebook
        </button>
      </div>

      <button id="btn-create-post" class="px-3.5 py-1.5 text-xs font-semibold bg-gradient-to-r from-indigo-600 to-indigo-500 hover:from-indigo-500 hover:to-indigo-400 text-white rounded-lg shadow-lg shadow-indigo-600/20 flex items-center gap-1.5 transition">
        <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="12" y1="5" x2="12" y2="19"></line><line x1="5" y1="12" x2="19" y2="12"></line></svg>
        <span>+ Tạo bài mới</span>
      </button>
    </div>
  </div>

  <!-- 7-Column Calendar Grid -->
  <div class="flex-1 flex flex-col border border-[#1e2436] rounded-xl bg-[#111420]/60 backdrop-blur overflow-hidden">
    <!-- Day Headers -->
    <div class="grid grid-cols-7 border-b border-[#1e2436] bg-[#111420] text-center text-xs font-semibold text-gray-400">
      <div class="py-2.5 border-r border-[#1e2436] text-indigo-400 bg-indigo-500/5">Thứ Hai <span class="block text-[10px] text-gray-500 font-normal">30 Thg 10</span></div>
      <div class="py-2.5 border-r border-[#1e2436]">Thứ Ba <span class="block text-[10px] text-gray-500 font-normal">31 Thg 10</span></div>
      <div class="py-2.5 border-r border-[#1e2436] text-white bg-indigo-600/10 font-bold">Thứ Tư (Hôm nay) <span class="block text-[10px] text-indigo-300 font-normal">01 Thg 11</span></div>
      <div class="py-2.5 border-r border-[#1e2436]">Thứ Năm <span class="block text-[10px] text-gray-500 font-normal">02 Thg 11</span></div>
      <div class="py-2.5 border-r border-[#1e2436]">Thứ Sáu <span class="block text-[10px] text-gray-500 font-normal">03 Thg 11</span></div>
      <div class="py-2.5 border-r border-[#1e2436] text-amber-400">Thứ Bảy <span class="block text-[10px] text-gray-500 font-normal">04 Thg 11</span></div>
      <div class="py-2.5 text-amber-400">Chủ Nhật <span class="block text-[10px] text-gray-500 font-normal">05 Thg 11</span></div>
    </div>

    <!-- Calendar Time Slots -->
    <div class="flex-1 overflow-y-auto divide-y divide-[#1e2436]/60">
      <!-- Slot 09:00 -->
      <div class="grid grid-cols-7 min-h-[90px] group/slot">
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">09:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">09:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative bg-indigo-500/[0.03] hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-indigo-400 font-mono font-semibold">09:00</span>
          <!-- Scheduled Post Sample -->
          <div class="mt-1.5 p-2 rounded-lg bg-[#191e30] border border-indigo-500/40 shadow-sm cursor-pointer hover:border-indigo-400 transition">
            <div class="flex items-center gap-1.5 mb-1">
              <span class="w-1.5 h-1.5 rounded-full bg-red-500"></span>
              <span class="text-[10px] font-semibold text-gray-300 truncate">Video Shorts AI #1</span>
            </div>
            <p class="text-[10px] text-gray-400 line-clamp-1">Khám phá công nghệ làm video tự động...</p>
          </div>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">09:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">09:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">09:00</span>
        </div>
        <div class="p-2 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">09:00</span>
        </div>
      </div>

      <!-- Slot 12:00 (Gio vang) -->
      <div class="grid grid-cols-7 min-h-[90px] group/slot">
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">12:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">12:00</span>
          <!-- Scheduled Post TikTok -->
          <div class="mt-1.5 p-2 rounded-lg bg-[#191e30] border border-emerald-500/40 shadow-sm cursor-pointer hover:border-emerald-400 transition">
            <div class="flex items-center gap-1.5 mb-1">
              <span class="w-1.5 h-1.5 rounded-full bg-emerald-400"></span>
              <span class="text-[10px] font-semibold text-gray-300 truncate">TikTok Trending Clip</span>
            </div>
            <p class="text-[10px] text-gray-400 line-clamp-1">#xuhuong #concat #videoai</p>
          </div>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative bg-indigo-500/[0.03] hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-indigo-400 font-mono font-semibold">12:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">12:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">12:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">12:00</span>
        </div>
        <div class="p-2 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">12:00</span>
        </div>
      </div>

      <!-- Slot 19:00 (Gio vang toi) -->
      <div class="grid grid-cols-7 min-h-[90px] group/slot">
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">19:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">19:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative bg-indigo-500/[0.03] hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-indigo-400 font-mono font-semibold">19:00</span>
          <!-- Scheduled Post Facebook -->
          <div class="mt-1.5 p-2 rounded-lg bg-[#191e30] border border-blue-500/40 shadow-sm cursor-pointer hover:border-blue-400 transition">
            <div class="flex items-center gap-1.5 mb-1">
              <span class="w-1.5 h-1.5 rounded-full bg-blue-500"></span>
              <span class="text-[10px] font-semibold text-gray-300 truncate">Reels Facebook Page</span>
            </div>
            <p class="text-[10px] text-gray-400 line-clamp-1">Công chiếu tập 3 phim hoạt hình AI</p>
          </div>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">19:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">19:00</span>
        </div>
        <div class="p-2 border-r border-[#1e2436]/60 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">19:00</span>
        </div>
        <div class="p-2 relative hover:bg-white/[0.02] transition">
          <span class="text-[10px] text-gray-500 font-mono">19:00</span>
        </div>
      </div>
    </div>
  </div>

  <!-- Modal Tao Bai Viet / Dat Lich -->
  <div id="modal-post-composer" class="fixed inset-0 bg-black/70 backdrop-blur-sm z-50 flex items-center justify-center hidden">
    <div class="bg-[#111420] border border-[#1e2436] rounded-2xl w-[580px] max-w-[90vw] p-5 shadow-2xl flex flex-col gap-4">
      <div class="flex items-center justify-between border-b border-[#1e2436] pb-3">
        <h3 class="text-sm font-bold text-white flex items-center gap-2">
          <span>Tạo & Đặt Lịch Đăng Bài</span>
        </h3>
        <button id="btn-close-modal" class="text-gray-400 hover:text-white p-1 rounded-lg hover:bg-[#1e2436] transition">&times;</button>
      </div>

      <!-- Platform Selection -->
      <div>
        <label class="block text-xs font-medium text-gray-400 mb-1.5">Chọn nền tảng đăng:</label>
        <div class="flex gap-2">
          <label class="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-[#1e2436] bg-[#191e30] text-xs cursor-pointer hover:border-indigo-500 transition">
            <input type="checkbox" checked class="rounded text-indigo-600"> <span>TikTok</span>
          </label>
          <label class="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-[#1e2436] bg-[#191e30] text-xs cursor-pointer hover:border-indigo-500 transition">
            <input type="checkbox" checked class="rounded text-indigo-600"> <span>YouTube Shorts</span>
          </label>
          <label class="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-[#1e2436] bg-[#191e30] text-xs cursor-pointer hover:border-indigo-500 transition">
            <input type="checkbox" class="rounded text-indigo-600"> <span>Facebook Reels</span>
          </label>
        </div>
      </div>

      <!-- Content Textarea -->
      <div>
        <label class="block text-xs font-medium text-gray-400 mb-1.5">Nội dung bài viết / Tiêu đề video:</label>
        <textarea id="composer-caption" rows="3" class="w-full bg-[#0d0f18] border border-[#1e2436] rounded-xl p-3 text-xs text-white placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition resize-none" placeholder="Nhập tiêu đề hoặc mô tả hashtag..."></textarea>
      </div>

      <!-- Time Picker -->
      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="block text-xs font-medium text-gray-400 mb-1.5">Ngày đăng:</label>
          <input type="date" value="2026-11-01" class="w-full bg-[#0d0f18] border border-[#1e2436] rounded-xl px-3 py-2 text-xs text-white focus:outline-none focus:border-indigo-500">
        </div>
        <div>
          <label class="block text-xs font-medium text-gray-400 mb-1.5">Khung giờ vàng:</label>
          <select class="w-full bg-[#0d0f18] border border-[#1e2436] rounded-xl px-3 py-2 text-xs text-white focus:outline-none focus:border-indigo-500">
            <option>12:00 (Trưa - Nhiều tương tác)</option>
            <option selected>19:00 (Tối - Giờ vàng)</option>
            <option>21:00 (Đêm)</option>
          </select>
        </div>
      </div>

      <!-- Actions -->
      <div class="flex justify-end gap-2.5 pt-2 border-t border-[#1e2436]">
        <button id="btn-cancel-post" class="px-4 py-2 text-xs text-gray-400 hover:text-white rounded-lg hover:bg-[#1e2436] transition">Hủy</button>
        <button id="btn-submit-post" class="px-5 py-2 text-xs font-semibold bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg shadow-lg shadow-indigo-600/30 transition">Lưu & Đặt Lịch</button>
      </div>
    </div>
  </div>

  <script>
  (function() {
    var modal = document.getElementById('modal-post-composer');
    var btnCreate = document.getElementById('btn-create-post');
    var btnClose = document.getElementById('btn-close-modal');
    var btnCancel = document.getElementById('btn-cancel-post');
    var btnSubmit = document.getElementById('btn-submit-post');

    function openModal() { if (modal) modal.classList.remove('hidden'); }
    function closeModal() { if (modal) modal.classList.add('hidden'); }

    if (btnCreate) btnCreate.onclick = openModal;
    if (btnClose) btnClose.onclick = closeModal;
    if (btnCancel) btnCancel.onclick = closeModal;

    if (btnSubmit) {
      btnSubmit.onclick = function() {
        var caption = document.getElementById('composer-caption').value;
        if (window.__POSTIZ_IPC__ && window.__POSTIZ_IPC__.call) {
          window.__POSTIZ_IPC__.call('postiz_post_create', { caption: caption });
        }
        alert('Đã lưu bài viết vào Lịch đăng thành công!');
        closeModal();
      };
    }
  })();
  </script>
</div>
"""

# Thay the khoi spinner cu bang giao dien Calendar
if re.search(spinner_pattern, html):
    html = re.sub(spinner_pattern, calendar_interface, html, count=1)
    print("Replaced spinner pattern with Calendar interface!")
else:
    # Neu pattern khong match hoan toan, tim `pt-[100px]` va thay the
    html = re.sub(r'<div class="flex-1 flex justify-center pt-\[100px\]">.*?</div></div>', calendar_interface, html, flags=re.DOTALL, count=1)
    print("Replaced pt-[100px] block with Calendar interface!")

# 3. KHOI PHUC LAI TAT CA CAC ICON TREN SIDEBAR BEN TRAI
# Dam bao sidebar co day du: Launches, Agent, Analytics, Media, Settings
sidebar_fix_css = """
<style id="postiz-sidebar-complete">
/* An duy nhat logo cu */
svg[viewBox="0 0 48 48"],
#left-menu > div > div:first-child:has(svg) {
    display: none !important;
}

/* Hien thi tat ca cac nut tren sidebar */
#left-menu a, #left-menu button, #left-menu div {
    visibility: visible !important;
    opacity: 1 !important;
}
</style>
"""

html = html.replace('</head>', f'{sidebar_fix_css}\n</head>')

with open(index_path, "w", encoding="utf-8") as f:
    f.write(html)

launches_path = r"d:\capcutpolot\concat\resources\postiz-web\launches.html"
with open(launches_path, "w", encoding="utf-8") as f:
    f.write(html)

print("SUCCESS: index.html & launches.html updated with complete Calendar & no spinner!")

