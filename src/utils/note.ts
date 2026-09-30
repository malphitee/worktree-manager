// 迭代备注前端预检（设计 009：≤50 字符、单行；空串＝清除）
// 注意：这里只是前端预检，后端会重新校验（前端不可信）。

export interface NoteValidation {
  ok: boolean;
  /** 通过时：去除首尾空白后的值；空串规范化为 null（＝清除备注） */
  value: string | null;
  /** 不通过时的中文原因 */
  message: string | null;
}

export const NOTE_MAX_LENGTH = 50;

export function validateNote(raw: string): NoteValidation {
  const value = raw.trim();
  if (value.length === 0) {
    return { ok: true, value: null, message: null };
  }
  if (value.includes("\n") || value.includes("\r")) {
    return { ok: false, value: null, message: "备注不能换行" };
  }
  // 按 Unicode 标量计长度（与后端一致）
  if (Array.from(value).length > NOTE_MAX_LENGTH) {
    return { ok: false, value: null, message: `备注不能超过 ${NOTE_MAX_LENGTH} 字符` };
  }
  return { ok: true, value, message: null };
}
