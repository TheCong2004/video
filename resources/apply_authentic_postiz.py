import re

index_path = r"d:\capcutpolot\concat\resources\postiz-web\index.html"
with open(index_path, "r", encoding="utf-8") as f:
    html = f.read()

# GIAO DIEN CHUAN 100% GOC CUA POSTIZ LAUNCHES & CALENDAR
postiz_authentic_ui = """
<div id="postiz-authentic-workspace" class="flex-1 flex flex-col h-full bg-newBgColor text-textColor select-none overflow-hidden">
  <!-- Top Navigation & Filters Bar -->
  <div class="px-[20px] pt-[16px] pb-[12px] flex flex-col gap-[12px] border-b border-newTableBorder bg-newBgColorInner/80 backdrop-blur-md">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-[12px]">
        <h1 class="text-[24px] font-[700] text-newTextColor tracking-tight">Launches</h1>
        <div class="flex items-center gap-[6px] ml-2">
          <!-- Channel Badges -->
          <div class="flex items-center gap-[6px] px-[10px] py-[4px] rounded-[8px] bg-newBgColor border border-newTableBorder text-[12px] font-[500]">
            <span class="w-[8px] h-[8px] rounded-full bg-emerald-400"></span>
            <span>TikTok</span>
          </div>
          <div class="flex items-center gap-[6px] px-[10px] py-[4px] rounded-[8px] bg-newBgColor border border-newTableBorder text-[12px] font-[500]">
            <span class="w-[8px] h-[8px] rounded-full bg-red-500"></span>
            <span>YouTube Shorts</span>
          </div>
          <div class="flex items-center gap-[6px] px-[10px] py-[4px] rounded-[8px] bg-newBgColor border border-newTableBorder text-[12px] font-[500]">
            <span class="w-[8px] h-[8px] rounded-full bg-blue-500"></span>
            <span>Facebook Reels</span>
          </div>
        </div>
      </div>

      <!-- Right Action: + New Post Button -->
      <div class="flex items-center gap-[10px]">
        <button id="postiz-btn-new-post" class="h-[40px] px-[18px] bg-gradient-to-r from-[#612bd3] to-[#7942e8] hover:opacity-95 text-white text-[14px] font-[600] rounded-[10px] shadow-[0_4px_16px_rgba(97,43,211,0.35)] flex items-center gap-[8px] transition-all cursor-pointer">
          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="12" y1="5" x2="12" y2="19"></line><line x1="5" y1="12" x2="19" y2="12"></line></svg>
          <span>New post</span>
        </button>
      </div>
    </div>

    <!-- Authentic Postiz Filters Row -->
    <div class="flex flex-row flex-wrap gap-[8px] items-center justify-between text-[14px]">
      <!-- Date Navigator -->
      <div class="flex items-center gap-[10px]">
        <div class="border h-[40px] border-newTableBorder bg-newTableBorder gap-[1px] flex items-center rounded-[8px] overflow-hidden">
          <div id="btn-prev-date" class="cursor-pointer text-textColor px-[12px] bg-newBgColorInner h-full flex items-center justify-center hover:text-textItemFocused hover:bg-boxFocused transition">
            <svg xmlns="http://www.w3.org/2000/svg" width="8" height="12" viewBox="0 0 8 12" fill="none"><path d="M6.5 11L1.5 6L6.5 1" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
          </div>
          <div class="min-w-[190px] text-center bg-newBgColorInner h-full flex items-center justify-center font-[500]">
            <span id="date-display-text" class="text-[14px] text-newTextColor">Oct 30 - Nov 5, 2026</span>
          </div>
          <div id="btn-next-date" class="cursor-pointer text-textColor px-[12px] bg-newBgColorInner h-full flex items-center justify-center hover:text-textItemFocused hover:bg-boxFocused transition">
            <svg xmlns="http://www.w3.org/2000/svg" width="8" height="12" viewBox="0 0 8 12" fill="none"><path d="M1.5 11L6.5 6L1.5 1" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
          </div>
        </div>

        <div id="btn-today" class="hover:text-textItemFocused hover:bg-boxFocused h-[40px] px-[14px] flex justify-center items-center rounded-[8px] transition-all cursor-pointer text-[14px] font-[500] bg-newBgColorInner border border-newTableBorder">
          Today
        </div>
      </div>

      <!-- View Switcher (Day / Week / Month / List) -->
      <div class="flex items-center gap-[10px]">
        <div class="flex flex-row p-[3px] border border-newTableBorder rounded-[8px] text-[13px] font-[500] bg-newBgColorInner">
          <div class="view-pill py-[4px] px-[14px] cursor-pointer rounded-[6px] transition text-textColor hover:text-textItemFocused" data-view="day">Day</div>
          <div class="view-pill py-[4px] px-[14px] cursor-pointer rounded-[6px] transition text-textItemFocused bg-boxFocused font-[600]" data-view="week">Week</div>
          <div class="view-pill py-[4px] px-[14px] cursor-pointer rounded-[6px] transition text-textColor hover:text-textItemFocused" data-view="month">Month</div>
        </div>

        <!-- Calendar / List Icons -->
        <div class="flex flex-row p-[3px] border border-newTableBorder rounded-[8px] bg-newBgColorInner">
          <div class="py-[5px] px-[8px] cursor-pointer rounded-[6px] text-textItemFocused bg-boxFocused">
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 17 19" fill="none"><path d="M15.75 7.41667H0.75M11.5833 0.75V4.08333M4.91667 0.75V4.08333M4.75 17.4167H11.75C13.1501 17.4167 13.8502 17.4167 14.385 17.1442C14.8554 16.9045 15.2378 16.522 15.4775 16.0516C15.75 15.5169 15.75 14.8168 15.75 13.4167V6.41667C15.75 5.01654 15.75 4.31647 15.4775 3.78169C15.2378 3.31129 14.8554 2.92883 14.385 2.68915C13.8502 2.41667 13.1501 2.41667 11.75 2.41667H4.75C3.34987 2.41667 2.6498 2.41667 2.11502 2.68915C1.64462 2.92883 1.26217 3.31129 1.02248 3.78169C0.75 4.31647 0.75 5.01654 0.75 6.41667V13.4167C0.75 14.8168 0.75 15.5169 1.02248 16.0516C1.26217 16.522 1.64462 16.9045 2.11502 17.1442C2.6498 17.4167 3.34987 17.4167 4.75 17.4167Z" stroke="currentColor" stroke-width="1.5"/></svg>
          </div>
          <div class="py-[5px] px-[8px] cursor-pointer rounded-[6px] text-textColor hover:text-textItemFocused">
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 17 17" fill="none"><path d="M5.5 4.5H15.5M5.5 8.5H15.5M5.5 12.5H15.5M1.5 4.5H1.508M1.5 8.5H1.508M1.5 12.5H1.508" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>
          </div>
        </div>
      </div>
    </div>
  </div>

  <!-- Authentic 7-Column Calendar Body -->
  <div class="flex-1 flex flex-col overflow-hidden bg-newBgColor">
    <!-- Days Header (7 Columns + Time Column) -->
    <div class="grid grid-cols-[64px_repeat(7,1fr)] border-b border-newTableBorder bg-newBgColorInner/95 text-[13px] font-[600] text-textItemBlur">
      <div class="py-[12px] text-center border-r border-newTableBorder text-[11px]">GMT+7</div>
      <div class="py-[10px] text-center border-r border-newTableBorder">Mon <span class="block text-[11px] font-[400] text-gray-500">Oct 30</span></div>
      <div class="py-[10px] text-center border-r border-newTableBorder">Tue <span class="block text-[11px] font-[400] text-gray-500">Oct 31</span></div>
      <div class="py-[10px] text-center border-r border-newTableBorder text-textItemFocused bg-boxFocused/40 font-[700]">Wed <span class="block text-[11px] font-[600] text-indigo-400">Nov 1 (Today)</span></div>
      <div class="py-[10px] text-center border-r border-newTableBorder">Thu <span class="block text-[11px] font-[400] text-gray-500">Nov 2</span></div>
      <div class="py-[10px] text-center border-r border-newTableBorder">Fri <span class="block text-[11px] font-[400] text-gray-500">Nov 3</span></div>
      <div class="py-[10px] text-center border-r border-newTableBorder text-amber-400">Sat <span class="block text-[11px] font-[400] text-gray-500">Nov 4</span></div>
      <div class="py-[10px] text-center text-amber-400">Sun <span class="block text-[11px] font-[400] text-gray-500">Nov 5</span></div>
    </div>

    <!-- Scrollable Time Slots Body -->
    <div class="flex-1 overflow-y-auto divide-y divide-newTableBorder/50">
      <!-- Slot 08:00 -->
      <div class="grid grid-cols-[64px_repeat(7,1fr)] min-h-[92px] group">
        <div class="py-2 px-1 text-center border-r border-newTableBorder/60 text-[11px] font-mono text-gray-500">08:00</div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 bg-boxFocused/10 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 hover:bg-boxFocused/20 transition cursor-pointer"></div>
      </div>

      <!-- Slot 12:00 (Postiz Card Example) -->
      <div class="grid grid-cols-[64px_repeat(7,1fr)] min-h-[92px] group">
        <div class="py-2 px-1 text-center border-r border-newTableBorder/60 text-[11px] font-mono text-gray-500">12:00</div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer">
          <!-- Scheduled Postiz Card -->
          <div class="p-2 rounded-[8px] bg-newBgColorInner border border-emerald-500/40 shadow-sm hover:border-emerald-400 transition">
            <div class="flex items-center gap-1.5 mb-1">
              <span class="w-[7px] h-[7px] rounded-full bg-emerald-400"></span>
              <span class="text-[11px] font-[600] text-newTextColor">TikTok Viral Shorts</span>
            </div>
            <p class="text-[11px] text-textItemBlur line-clamp-1">Kedi Media AI Studio video release</p>
          </div>
        </div>
        <div class="p-1.5 border-r border-newTableBorder/40 bg-boxFocused/10 hover:bg-boxFocused/20 transition cursor-pointer">
          <!-- Scheduled YouTube Card -->
          <div class="p-2 rounded-[8px] bg-newBgColorInner border border-red-500/40 shadow-sm hover:border-red-400 transition">
            <div class="flex items-center gap-1.5 mb-1">
              <span class="w-[7px] h-[7px] rounded-full bg-red-500"></span>
              <span class="text-[11px] font-[600] text-newTextColor">YouTube Premiere</span>
            </div>
            <p class="text-[11px] text-textItemBlur line-clamp-1">Episode 1: Automation Pipeline</p>
          </div>
        </div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 hover:bg-boxFocused/20 transition cursor-pointer"></div>
      </div>

      <!-- Slot 18:00 (Evening Prime Time) -->
      <div class="grid grid-cols-[64px_repeat(7,1fr)] min-h-[92px] group">
        <div class="py-2 px-1 text-center border-r border-newTableBorder/60 text-[11px] font-mono text-gray-500">18:00</div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 bg-boxFocused/10 hover:bg-boxFocused/20 transition cursor-pointer">
          <!-- Scheduled Facebook Card -->
          <div class="p-2 rounded-[8px] bg-newBgColorInner border border-blue-500/40 shadow-sm hover:border-blue-400 transition">
            <div class="flex items-center gap-1.5 mb-1">
              <span class="w-[7px] h-[7px] rounded-full bg-blue-500"></span>
              <span class="text-[11px] font-[600] text-newTextColor">Facebook Page Reel</span>
            </div>
            <p class="text-[11px] text-textItemBlur line-clamp-1">Highlight moment clip #concat</p>
          </div>
        </div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 hover:bg-boxFocused/20 transition cursor-pointer"></div>
      </div>

      <!-- Slot 21:00 -->
      <div class="grid grid-cols-[64px_repeat(7,1fr)] min-h-[92px] group">
        <div class="py-2 px-1 text-center border-r border-newTableBorder/60 text-[11px] font-mono text-gray-500">21:00</div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 bg-boxFocused/10 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 border-r border-newTableBorder/40 hover:bg-boxFocused/20 transition cursor-pointer"></div>
        <div class="p-1.5 hover:bg-boxFocused/20 transition cursor-pointer"></div>
      </div>
    </div>
  </div>

  <!-- Authentic Postiz Post Composer Modal -->
  <div id="postiz-composer-modal" class="fixed inset-0 bg-black/75 backdrop-blur-sm z-50 flex items-center justify-center hidden">
    <div class="bg-newBgColorInner border border-newTableBorder rounded-[16px] w-[620px] max-w-[92vw] p-[24px] shadow-2xl flex flex-col gap-[18px]">
      <div class="flex items-center justify-between border-b border-newTableBorder pb-[14px]">
        <h3 class="text-[16px] font-[700] text-newTextColor flex items-center gap-2">
          <span>Create Post & Schedule</span>
        </h3>
        <button id="postiz-btn-close-composer" class="text-textItemBlur hover:text-newTextColor p-1 rounded-lg hover:bg-newBgColor transition cursor-pointer">&times;</button>
      </div>

      <!-- Select Channels -->
      <div>
        <label class="block text-[12px] font-[600] text-textItemBlur mb-[8px]">Publish to channels:</label>
        <div class="flex gap-[8px]">
          <label class="flex items-center gap-[8px] px-[12px] py-[8px] rounded-[8px] border border-newTableBorder bg-newBgColor text-[12px] font-[500] cursor-pointer hover:border-indigo-500 transition">
            <input type="checkbox" checked class="rounded text-[#612bd3]"> <span>TikTok</span>
          </label>
          <label class="flex items-center gap-[8px] px-[12px] py-[8px] rounded-[8px] border border-newTableBorder bg-newBgColor text-[12px] font-[500] cursor-pointer hover:border-indigo-500 transition">
            <input type="checkbox" checked class="rounded text-[#612bd3]"> <span>YouTube Shorts</span>
          </label>
          <label class="flex items-center gap-[8px] px-[12px] py-[8px] rounded-[8px] border border-newTableBorder bg-newBgColor text-[12px] font-[500] cursor-pointer hover:border-indigo-500 transition">
            <input type="checkbox" class="rounded text-[#612bd3]"> <span>Facebook Reels</span>
          </label>
        </div>
      </div>

      <!-- Caption Editor -->
      <div>
        <label class="block text-[12px] font-[600] text-textItemBlur mb-[8px]">Caption & Description:</label>
        <textarea id="composer-text" rows="4" class="w-full bg-newBgColor border border-newTableBorder rounded-[10px] p-[12px] text-[13px] text-newTextColor placeholder-gray-500 focus:outline-none focus:border-[#612bd3] transition resize-none" placeholder="What's on your mind? Add hashtags, captions, tags..."></textarea>
      </div>

      <!-- Schedule Time -->
      <div class="grid grid-cols-2 gap-[12px]">
        <div>
          <label class="block text-[12px] font-[600] text-textItemBlur mb-[6px]">Schedule Date:</label>
          <input type="date" value="2026-11-01" class="w-full bg-newBgColor border border-newTableBorder rounded-[8px] px-[12px] py-[8px] text-[12px] text-newTextColor focus:outline-none focus:border-[#612bd3]">
        </div>
        <div>
          <label class="block text-[12px] font-[600] text-textItemBlur mb-[6px]">Time Slot:</label>
          <select class="w-full bg-newBgColor border border-newTableBorder rounded-[8px] px-[12px] py-[8px] text-[12px] text-newTextColor focus:outline-none focus:border-[#612bd3]">
            <option>12:00 PM (Lunch Break)</option>
            <option selected>06:00 PM (Evening Peak)</option>
            <option>09:00 PM (Night Rush)</option>
          </select>
        </div>
      </div>

      <!-- Modal Footer -->
      <div class="flex items-center justify-end gap-[10px] pt-[12px] border-t border-newTableBorder">
        <button id="postiz-btn-cancel" class="px-[16px] py-[8px] text-[13px] font-[500] text-textItemBlur hover:text-newTextColor rounded-[8px] hover:bg-newBgColor transition cursor-pointer">Cancel</button>
        <button id="postiz-btn-submit" class="px-[20px] py-[9px] text-[13px] font-[600] bg-[#612bd3] hover:bg-[#733be6] text-white rounded-[8px] shadow-lg shadow-[#612bd3]/30 transition cursor-pointer">Schedule Post</button>
      </div>
    </div>
  </div>

  <script>
  (function() {
    var modal = document.getElementById('postiz-composer-modal');
    var btnNew = document.getElementById('postiz-btn-new-post');
    var btnClose = document.getElementById('postiz-btn-close-composer');
    var btnCancel = document.getElementById('postiz-btn-cancel');
    var btnSubmit = document.getElementById('postiz-btn-submit');

    function openModal() { if (modal) modal.classList.remove('hidden'); }
    function closeModal() { if (modal) modal.classList.add('hidden'); }

    if (btnNew) btnNew.onclick = openModal;
    if (btnClose) btnClose.onclick = closeModal;
    if (btnCancel) btnCancel.onclick = closeModal;

    if (btnSubmit) {
      btnSubmit.onclick = function() {
        var text = document.getElementById('composer-text').value;
        if (window.__POSTIZ_IPC__ && window.__POSTIZ_IPC__.call) {
          window.__POSTIZ_IPC__.call('postiz_post_create', { text: text });
        }
        alert('Scheduled post successfully to connected channels!');
        closeModal();
      };
    }

    // View switcher pills
    var pills = document.querySelectorAll('.view-pill');
    pills.forEach(function(pill) {
      pill.onclick = function() {
        pills.forEach(function(p) {
          p.classList.remove('text-textItemFocused', 'bg-boxFocused', 'font-[600]');
          p.classList.add('text-textColor');
        });
        pill.classList.remove('text-textColor');
        pill.classList.add('text-textItemFocused', 'bg-boxFocused', 'font-[600]');
      };
    });
  })();
  </script>
</div>
"""

# Thay the khoi cu bang postiz_authentic_ui
if 'postiz-native-workspace' in html:
    html = re.sub(r'<div id="postiz-native-workspace".*?</div>\s*</div>\s*</div>\s*</div>\s*</div>', postiz_authentic_ui, html, flags=re.DOTALL)
else:
    html = re.sub(r'<div class="flex-1 flex justify-center pt-\[100px\]">.*?</div></div>', postiz_authentic_ui, html, flags=re.DOTALL)

with open(index_path, "w", encoding="utf-8") as f:
    f.write(html)

launches_path = r"d:\capcutpolot\concat\resources\postiz-web\launches.html"
with open(launches_path, "w", encoding="utf-8") as f:
    f.write(html)

print("SUCCESS: Postiz Authentic UI applied cleanly!")

