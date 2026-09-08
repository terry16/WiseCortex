// ── 图标集（细线 SVG，替代花哨的 emoji）─────────────────────────────────────
// 内联 SVG，无外部图标依赖。
// icon(name, size) 返回内联 SVG 字符串；颜色用 currentColor，由 CSS 控制。

const PATHS: Record<string, string> = {
  // 品牌标记 = 中心枢纽 + 六向节点（与 public/icon.svg 同构）。
  cortex:
    '<circle cx="12" cy="12" r="2.5" fill="currentColor" stroke="none"/><circle cx="12" cy="6.2" r="1.5" fill="currentColor" stroke="none"/><circle cx="17" cy="9.1" r="1.15" fill="currentColor" stroke="none"/><circle cx="17" cy="14.9" r="1.5" fill="currentColor" stroke="none"/><circle cx="12" cy="17.8" r="1.15" fill="currentColor" stroke="none"/><circle cx="7" cy="14.9" r="1.5" fill="currentColor" stroke="none"/><circle cx="7" cy="9.1" r="1.15" fill="currentColor" stroke="none"/><path d="M12 12 12 6.6M12 12l4.7-2.7M12 12l4.7 2.7M12 12v5.4M12 12l-4.7 2.7M12 12L7.3 9.3"/>',
  chat: '<path d="M4 5h16v11H8l-4 4V5Z"/>',
  // 记忆 = 大脑轮廓 + 中缝。比灯泡/书签更直指「记住的东西」，且不与知识库(书)撞。
  brain:
    '<path d="M12 5.2a3 3 0 0 0-5.5 1.5A2.9 2.9 0 0 0 4.6 9.5a2.9 2.9 0 0 0 .9 2.1 2.9 2.9 0 0 0 1.3 4.6A2.8 2.8 0 0 0 12 17.6Z"/><path d="M12 5.2a3 3 0 0 1 5.5 1.5 2.9 2.9 0 0 1 1.9 2.8 2.9 2.9 0 0 1-.9 2.1 2.9 2.9 0 0 1-1.3 4.6A2.8 2.8 0 0 1 12 17.6Z"/><path d="M12 5.2v12.4"/>',
  clock: '<circle cx="12" cy="12" r="8.2"/><path d="M12 7.5V12l3 1.8"/>',
  // 技能 = 能力/魔法感的「闪光」，比单颗五角星更达意（星常被读作收藏/评分）。
  skill:
    '<path d="M12 3.5c.45 3.35 1.7 4.6 5 5-3.3.4-4.55 1.65-5 5-.45-3.35-1.7-4.6-5-5 3.3-.4 4.55-1.65 5-5Z"/><path d="M18.5 13.5c.2 1.7.85 2.35 2.5 2.6-1.65.25-2.3.9-2.5 2.6-.2-1.7-.85-2.35-2.5-2.6 1.65-.25 2.3-.9 2.5-2.6Z"/>',
  // 知识库 = 摊开的书（原来用通用文档图标，与「计划」撞且不达意）。
  book: '<path d="M12 6.2C10.3 5.1 8.3 4.6 6 4.7v12c2.3-.1 4.3.4 6 1.6 1.7-1.2 3.7-1.7 6-1.6v-12c-2.3-.1-4.3.4-6 1.5Z"/><path d="M12 6.2V18.3"/>',
  // 计划 = 带勾选的剪贴板（清单/方案）。
  clipboard:
    '<rect x="5.5" y="5" width="13" height="15.5" rx="2"/><rect x="9" y="3" width="6" height="3.2" rx="1.1"/><path d="M8.8 11.4l1.4 1.4 3-3.2"/><path d="M8.8 15.6h6.4"/>',
  bell: '<path d="M6 9a6 6 0 0 1 12 0c0 5 2 6 2 6H4s2-1 2-6Z"/><path d="M10 19a2 2 0 0 0 4 0"/>',
  gear: '<circle cx="12" cy="12" r="3"/><path d="M12 2.5v3M12 18.5v3M21.5 12h-3M5.5 12h-3M18.5 5.5l-2 2M7.5 16.5l-2 2M18.5 18.5l-2-2M7.5 7.5l-2-2"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  send: '<path d="M5 12l14-7-5 14-2.5-5.5L5 12Z"/>',
  menu: '<path d="M4 7h16M4 12h16M4 17h16"/>',
  x: '<path d="M6 6l12 12M18 6L6 18"/>',
  // 帮助：圆圈问号。用于设置项旁的「这是什么」提示。
  help: '<circle cx="12" cy="12" r="8.6"/><path d="M9.6 9.4a2.5 2.5 0 1 1 3.3 2.4c-.6.2-.9.7-.9 1.3v.5"/><path d="M12 16.6v.01"/>',
  check: '<path d="M5 12.5l4.5 4.5L19 7"/>',
  refresh:
    '<path d="M4 12a8 8 0 0 1 13.5-5.7L20 8M20 4v4h-4"/><path d="M20 12a8 8 0 0 1-13.5 5.7L4 16M4 20v-4h4"/>',
  trash: '<path d="M5 7h14M10 7V5h4v2M6 7l1 13h10l1-13"/>',
  bolt: '<path d="M13 3L5 13h6l-1 8 8-10h-6l1-8Z"/>',
  plug: '<path d="M9 2v6M15 2v6M7 8h10v3a5 5 0 0 1-10 0V8ZM12 16v6"/>',
  shield: '<path d="M12 3l7 3v5c0 5-3 7.5-7 9-4-1.5-7-4-7-9V6l7-3Z"/>',
  key: '<circle cx="8" cy="14" r="3.3"/><path d="M10.4 11.6L20 2m-3 2l2 2m-4 0l2 2"/>',
  coin: '<ellipse cx="12" cy="6.5" rx="7" ry="3"/><path d="M5 6.5v11c0 1.7 3.1 3 7 3s7-1.3 7-3v-11M5 12c0 1.7 3.1 3 7 3s7-1.3 7-3"/>',
  folder:
    '<path d="M3 7a2 2 0 0 1 2-2h3.5l2 2H19a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z"/>',
  image:
    '<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="8.5" cy="9.5" r="1.6"/><path d="M21 16l-5-5L5 20"/>',
  doc: '<path d="M6 3h8l4 4v14H6V3Z"/><path d="M14 3v4h4"/>',
  code: '<path d="M9 8l-4 4 4 4M15 8l4 4-4 4"/>',
  globe:
    '<circle cx="12" cy="12" r="8.2"/><path d="M3.8 12h16.4M12 3.8c2.4 2.3 2.4 14.1 0 16.4M12 3.8c-2.4 2.3-2.4 14.1 0 16.4"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2"/>',
  pencil:
    '<path d="M4.5 19.5l.9-3.6L16.6 4.7a2 2 0 0 1 2.8 2.8L8.2 18.7l-3.7.8Z"/><path d="M14.8 6.5l2.8 2.8"/>',
  chevR: '<path d="M9 6l6 6-6 6"/>',
  search: '<circle cx="11" cy="11" r="6.4"/><path d="M15.6 15.6L20 20"/>',
  // 主题切换：暗色模式显示月亮，浅色模式显示太阳。
  moon: '<path d="M20 14.5A8.5 8.5 0 1 1 9.5 4a6.8 6.8 0 0 0 10.5 10.5Z"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2.2M12 19.3v2.2M2.5 12h2.2M19.3 12h2.2M5.3 5.3l1.6 1.6M17.1 17.1l1.6 1.6M18.7 5.3l-1.6 1.6M6.9 17.1l-1.6 1.6"/>',
};

/** 返回内联 SVG 字符串。stroke=currentColor，便于用 CSS 上色。 */
export function icon(name: keyof typeof PATHS | string, size = 18): string {
  const body = PATHS[name] ?? "";
  return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
}

export type IconName = keyof typeof PATHS;
