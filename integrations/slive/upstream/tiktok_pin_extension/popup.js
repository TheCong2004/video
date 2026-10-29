document.addEventListener('DOMContentLoaded', async () => {
  const statusBadge = document.getElementById('status-badge');
  const serverStatus = document.getElementById('server-status');
  const pinnedProduct = document.getElementById('pinned-product');
  const btnOpen = document.getElementById('btn-open-dashboard');
  const btnPing = document.getElementById('btn-test-ping');

  async function checkServer() {
    const ports = [8010, 5000];
    let connected = false;
    for (const port of ports) {
      try {
        const res = await fetch(`http://127.0.0.1:${port}/api/tiktok/status`);
        if (res.ok) {
          const data = await res.json();
          statusBadge.textContent = 'ONLINE';
          statusBadge.className = 'status-badge status-online';
          serverStatus.textContent = `Đã kết nối (127.0.0.1:${port})`;

          if (data.runtime && data.runtime.active_product_title) {
            pinnedProduct.textContent = `#${data.runtime.active_product_index}: ${data.runtime.active_product_title}`;
          } else {
            pinnedProduct.textContent = 'Chưa chọn sản phẩm';
          }
          connected = true;
          break;
        }
      } catch (e) {
        // Continue to probe next port
      }
    }
    if (!connected) {
      statusBadge.textContent = 'OFFLINE';
      statusBadge.className = 'status-badge status-offline';
      serverStatus.textContent = 'Không kết nối được SLive App';
      pinnedProduct.textContent = '-';
    }
  }

  btnOpen.addEventListener('click', () => {
    chrome.tabs.create({ url: 'https://shop.tiktok.com/streamer/live/product/dashboard' });
  });

  btnPing.addEventListener('click', () => {
    statusBadge.textContent = 'Đang kiểm tra...';
    checkServer();
  });

  checkServer();
});
