// ── 访问密钥（前端侧）────────────────────────────────────────────────────────
// 服务端配置了 access_key 时，WS 用 query、REST 用 x-access-key 头携带；
// 确认确实缺密钥（见 probeNeedsKey）后弹框让用户输入，存 localStorage 后重连。

import { httpBase } from "./backend";

const STORAGE_KEY = "wisecortex_access_key";
// 改名 WiseClaw → WiseCortex 之前用的键名。只读一次并搬过来，避免老用户被迫重输密钥。
const LEGACY_STORAGE_KEY = "wiseclaw_access_key";

export function getKey(): string | null {
  try {
    const cur = localStorage.getItem(STORAGE_KEY);
    if (cur) return cur;
    // 一次性迁移：旧键还在就搬到新键，并清掉旧的。
    const legacy = localStorage.getItem(LEGACY_STORAGE_KEY);
    if (legacy) {
      localStorage.setItem(STORAGE_KEY, legacy);
      localStorage.removeItem(LEGACY_STORAGE_KEY);
      return legacy;
    }
    return null;
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
    localStorage.removeItem(LEGACY_STORAGE_KEY);
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
 * `passed` 在成功连上后置 true；1006 关闭且未 passed 时，ws.ts 先调 probeNeedsKey()
 * 确认确实缺密钥，再调 reset()+check()。
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
    /**
     * 探测「是否真的需要密钥」。
     *
     * WS 的 1006 只说明「连接异常断开」，区分不了鉴权失败与后端未启动。
     * /api/auth/status 本身不鉴权：请得通说明后端活着，此时 required &&
     * !authorized 才是真的要密钥；请求直接报错则是服务不可达，不该清密钥。
     */
    async probeNeedsKey(): Promise<boolean> {
      try {
        const r = await fetch(`${httpBase()}/api/auth/status`, {
          headers: authHeaders(),
        });
        if (!r.ok) return false;
        const st = (await r.json()) as {
          required?: boolean;
          authorized?: boolean;
        };
        return st.required === true && st.authorized !== true;
      } catch {
        return false; // 后端根本连不上——与密钥无关。
      }
    },
  };
}
