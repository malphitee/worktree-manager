// 基分支归一化「预览」（设计 013 §3.3：`previewNormalized`）（设计 013；仅用于界面展示与前端预检，不做安全判定）。
// 规则（requirements.md 判定口径 12）：空串＝origin/master；<remote>/<branch> 首段命中已配置 remote 则原样；
// 裸分支名优先 origin，没有 origin 且只有一个 remote 时用该 remote；否则报错。
// 后端 base_ref.rs 会重新归一化并校验，前端结果不可信。

export interface BaseRefPreview {
  /** 归一化结果的展示值（出错时原样回显输入） */
  value: string;
  /** 出错原因（中文），null 表示可继续 */
  error: string | null;
}

export function previewNormalized(input: string, remotes: string[]): BaseRefPreview {
  const raw = input.trim();
  if (raw.length === 0) {
    return { value: "origin/master", error: null };
  }
  const slash = raw.indexOf("/");
  if (slash > 0) {
    const head = raw.slice(0, slash);
    const rest = raw.slice(slash + 1);
    if (remotes.includes(head) && rest.length > 0) {
      return { value: raw, error: null };
    }
    return { value: raw, error: `remote「${head}」不在已配置 remote 列表中` };
  }
  if (remotes.includes("origin")) {
    return { value: `origin/${raw}`, error: null };
  }
  if (remotes.length === 1) {
    return { value: `${remotes[0]}/${raw}`, error: null };
  }
  return { value: raw, error: "仓库没有 origin remote，请填写 <remote>/<branch>" };
}
