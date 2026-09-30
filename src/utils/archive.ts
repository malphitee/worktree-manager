// 归档对话框的纯逻辑（设计 008 §5.16）：确认按钮可用性 = 确认文本严格相等 且（干净 或 已勾选强制）
export function canArchive(
  clean: boolean,
  confirmation: string,
  confirmationText: string,
  force: boolean,
): boolean {
  if (confirmation !== confirmationText) {
    return false;
  }
  return clean || force;
}
