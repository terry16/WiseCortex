// ── 访问密钥（前端侧）────────────────────────────────────────────────────────
// 服务端配置了 access_key 时，WS 用 query、REST 用 x-access-key 头携带；
// 握手被拒(1006 且 auth 未通过)时弹框让用户输入，存 localStorage 后重连。

const STORAGE_KEY = "wisecortex_access_key";

export function getKey(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export function setKey(key: string): void {
  try {
    localStorage.setItem(STORAGE_KEY, key);
  } catch {
    /* 忽略（隐私模式等） */
  }
}

export function clearKey(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* 忽略 */
  }
}

/** REST fetch 用的鉴权头（无 key 时为空对象）。 */
export function authHeaders(): Record<string, string> {
  const k = getKey();
  return k ? { "x-access-key": k } : {};
}

/**
 * ws.ts 需要的 Auth 适配器。
 * `passed` 在成功连上后置 true；1006 关闭且未 passed 时，ws.ts 调 reset()+check()。
 */
export function createWsAuth(onNeedKey: () => void) {
  let passed = false;
  return {
    get passed() {
      return passed;
    },
    markPassed() {
      passed = true;
    },
    getKey,
    reset() {
      passed = false;
      clearKey();
    },
    check() {
      onNeedKey();
    },
  };
}
