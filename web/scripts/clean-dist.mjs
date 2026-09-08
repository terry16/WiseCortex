// 构建前清空 dist —— 但保留宝塔(BT 面板)在站点根目录生成的 `.user.ini`。
//
// 宝塔会往站点目录写一个 `.user.ini` 并加不可变锁(`chattr +i`)，vite 默认的
// emptyOutDir 删不掉被锁文件，构建会以 `ENOTDIR .../dist/.user.ini` 失败。
// 配合 vite.config 的 `build.emptyOutDir: false`：由本脚本负责清理（跳过 .user.ini），
// vite 只管写产物、不再去动那个文件。本地无 .user.ini 时本脚本行为不变。
import { existsSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";

const dir = "dist";
if (existsSync(dir)) {
  for (const name of readdirSync(dir)) {
    if (name === ".user.ini") continue; // 宝塔锁定文件，保留
    try {
      rmSync(join(dir, name), { recursive: true, force: true });
    } catch (e) {
      console.warn(`clean-dist: 跳过 ${name}: ${e.message}`);
    }
  }
}
