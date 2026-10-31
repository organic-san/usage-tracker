import { Menu } from "@tauri-apps/api/menu";

let current = null;

/**
 * 在游標位置彈出原生選單。
 * 用原生選單而不是 HTML:HTML 元素一定會被蓋在官方頁面(子 webview)底下。
 * items 格式見 @tauri-apps/api/menu,例如 { text, action }、{ text, checked, action }、{ item: "Separator" }。
 */
export async function popup(items) {
  // 上一個選單的資源留到這裡才釋放:popup() 返回時,點選事件可能還沒送達
  const prev = current;
  current = null;
  prev?.close().catch(() => {});
  current = await Menu.new({ items });
  await current.popup();
}
