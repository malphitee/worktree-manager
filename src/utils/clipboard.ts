// 复制文本到剪贴板（设计 002）：navigator.clipboard 优先，失败降级到隐藏 textarea + execCommand。
// 纯前端能力，不新增后端命令、不占用操作锁；两种方式都失败时 reject。

export async function copyText(text: string): Promise<void> {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
      await navigator.clipboard.writeText(text);
      return;
    }
  } catch {
    // 权限被拒或不可用 → 走降级路径
  }
  if (!fallbackCopy(text)) {
    throw new Error("复制失败，请手动选择文本");
  }
}

function fallbackCopy(text: string): boolean {
  if (typeof document === "undefined") {
    return false;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  textarea.style.top = "0";
  document.body.appendChild(textarea);
  textarea.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  } finally {
    document.body.removeChild(textarea);
  }
  return ok;
}
