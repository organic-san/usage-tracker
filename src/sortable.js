// 以 pointer 事件實作的直向拖曳排序,側欄圖示與設定頁的服務清單共用。
// 不用 HTML5 drag & drop:Tauri 在 Windows 預設會攔截拖放事件,原生 DnD 在 webview 內無法正常運作。

const THRESHOLD = 4; // 移動超過這個距離才算拖曳,否則照常當成點擊

/**
 * @param {HTMLElement} container
 * @param {{ items: string, handle?: string, onMove: (from: number, to: number) => void }} opts
 *   items:可排序項目的選擇器;handle:只有按在這裡才能拖(省略 = 整個項目);
 *   onMove:放開時回報原位置與新位置(皆為 items 中的索引)
 */
export function sortable(container, { items, handle, onMove }) {
  let suppressClickUntil = 0;
  // 放開後瀏覽器仍可能補送 click,這段時間內吞掉,避免拖曳被當成點擊
  container.addEventListener(
    "click",
    (e) => {
      if (performance.now() < suppressClickUntil) {
        e.stopPropagation();
        e.preventDefault();
      }
    },
    true,
  );

  container.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    const grabbed = handle ? e.target.closest(handle) : e.target;
    const item = grabbed?.closest(items);
    if (!item || !container.contains(item)) return;

    const all = [...container.querySelectorAll(items)];
    const from = all.indexOf(item);
    const rects = all.map((node) => node.getBoundingClientRect());
    const step = all.length > 1 ? rects[1].top - rects[0].top : 0;
    const startY = e.clientY;
    let dragging = false;
    let to = from;

    const move = (ev) => {
      const dy = ev.clientY - startY;
      if (!dragging) {
        if (Math.abs(dy) < THRESHOLD) return;
        dragging = true;
        // 捕捉指標,游標移到官方頁面(另一個原生視窗)上方時也能繼續收到事件
        item.setPointerCapture(e.pointerId);
        item.classList.add("dragging");
        container.classList.add("sorting");
      }
      const d = Math.max(
        rects[0].top - rects[from].top,
        Math.min(rects.at(-1).top - rects[from].top, dy),
      );
      item.style.transform = `translateY(${d}px)`;

      const center = rects[from].top + rects[from].height / 2 + d;
      to = 0;
      rects.forEach((r, i) => {
        if (i !== from && center > r.top + r.height / 2) to++;
      });
      // 其他項目讓出位置,預覽放開後的順序
      all.forEach((node, i) => {
        if (i === from) return;
        const pos = i < from ? i : i - 1;
        const shift = i < from && pos >= to ? step : i > from && pos < to ? -step : 0;
        node.style.transform = shift ? `translateY(${shift}px)` : "";
      });
    };

    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      if (!dragging) return;
      suppressClickUntil = performance.now() + 300;
      all.forEach((node) => (node.style.transform = ""));
      item.classList.remove("dragging");
      container.classList.remove("sorting");
      if (to !== from) onMove(from, to);
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  });
}
