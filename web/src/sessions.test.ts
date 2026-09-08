import { beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "./i18n";
import { Sessions } from "./sessions";

function setup() {
  const messages = document.createElement("div");
  document.body.appendChild(messages);
  return { sessions: new Sessions({ messages }), messages };
}

/** 侧栏 + 两个任务（aaa11111 为活动）；从 calls 读回调结果。 */
function sidebarWithTasks() {
  const messages = document.createElement("div");
  const sidebar = document.createElement("div");
  document.body.append(messages, sidebar);
  const calls = { switched: "", deleted: "" };
  const s = new Sessions({
    messages,
    sidebar,
    onSwitch: (id) => {
      calls.switched = id;
    },
    onDelete: (id) => {
      calls.deleted = id;
    },
  });
  // bbb22222 需有名字：renderList 会过滤未命名且非活动、非运行中的空会话。
  s.setAll([{ id: "aaa11111" }, { id: "bbb22222", name: "第二个任务" }], false, 0);
  s.setActive("aaa11111");
  return { sidebar, calls };
}

/** 让「确认框 resolve → 删除回调」这条 await 链跑完（回调在微任务里，同步断言追不上）。 */
const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  document.body.innerHTML = "";
  setLocale("zh-CN"); // 断言里硬编码了中文段头/标签，固定到 zh-CN 使其与界面默认语言无关
});

describe("Sessions rendering", () => {
  it("renders assistant content as markdown", () => {
    const { sessions, messages } = setup();
    sessions.appendMsg("assistant", "**bold** and `code`");
    const html = messages.innerHTML;
    expect(html).toContain("<strong>bold</strong>");
    expect(html).toContain("<code>code</code>");
  });

  it("treats non-assistant html as trusted (pre-escaped by caller)", () => {
    const { sessions, messages } = setup();
    // dispatcher passes already-escaped html for user/error
    sessions.appendMsg("user", "&lt;script&gt;");
    expect(messages.textContent).toBe("<script>");
    expect(messages.querySelector("script")).toBeNull();
  });

  it("shows then clears a progress indicator", () => {
    const { sessions, messages } = setup();
    sessions.showProgress("thinking", "thinking", {}, null);
    expect(messages.querySelector(".progress")?.textContent).toContain("thinking");
    sessions.clearProgress();
    expect(messages.querySelector(".progress")).toBeNull();
  });

  it("suppresses the thinking indicator once the assistant starts streaming", () => {
    const { sessions, messages } = setup();
    // 服务端在流式期间仍会持续推 progress（每个 Usage 事件一条），而 assistant_delta 会
    // clearProgress——一删一建、每次重建都追加到末尾（跑到流式气泡下面），高度反复增减
    // 就是「thinking 一直刷新 + UI 上下跳」。开始吐字后就不该再出现 progress。
    sessions.showProgress("thinking", "thinking", {}, null);
    expect(messages.querySelector(".progress")).not.toBeNull();

    sessions.appendDelta("Hel"); // dispatcher 收到 delta 时会先 clearProgress
    sessions.clearProgress();
    sessions.showProgress("thinking", "thinking", {}, null); // 随后又来一条 Usage → progress
    expect(messages.querySelector(".progress")).toBeNull();

    // 定稿后 streamingEl 清空 → progress 恢复显示（下一轮工具/思考仍要有反馈）。
    sessions.appendMsg("assistant", "Hello");
    sessions.showProgress("thinking", "thinking", {}, null);
    expect(messages.querySelector(".progress")).not.toBeNull();
  });

  it("only auto-scrolls while pinned to the bottom, and resumes when scrolled back", () => {
    const { sessions, messages } = setup();
    // jsdom 不排版，手动喂尺寸：内容 1000、视口 300 → 贴底位置 scrollTop=700。
    Object.defineProperty(messages, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(messages, "clientHeight", { value: 300, configurable: true });

    // 用户手工上滚到中间去看历史 → 新增量不得把他拽回底部。
    messages.scrollTop = 200;
    sessions.appendDelta("新内容");
    expect(messages.scrollTop).toBe(200);

    // 主动滚回底部 → 下一条增量自动恢复跟随（无状态：每次 append 前现测位置）。
    messages.scrollTop = 700;
    sessions.appendDelta("更多");
    expect(messages.scrollTop).toBe(1000);
  });

  it("sent user messages force the view to the bottom without changing the normal pin rule", () => {
    const { sessions, messages } = setup();
    Object.defineProperty(messages, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(messages, "clientHeight", { value: 300, configurable: true });

    // 用户上滚查看历史后主动发送：自己的新消息必须立即可见。
    messages.scrollTop = 200;
    sessions.appendMsg("user", "我发送的消息", { forceScroll: true });
    expect(messages.scrollTop).toBe(1000);

    // 后续普通更新仍遵守原规则：不在底部时不得自动滚动。
    messages.scrollTop = 200;
    sessions.appendMsg("assistant", "普通新消息");
    expect(messages.scrollTop).toBe(200);
  });

  it("the pin rule applies to tool output too, not only deltas", () => {
    const { sessions, messages } = setup();
    Object.defineProperty(messages, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(messages, "clientHeight", { value: 300, configurable: true });
    // 贴底 → 工具输出跟随。
    messages.scrollTop = 700;
    sessions.appendToolResult("输出");
    expect(messages.scrollTop).toBe(1000);
    // 上滚看历史 → 工具输出也不得把他拽回底部。
    messages.scrollTop = 100;
    sessions.appendToolResult("更多输出");
    expect(messages.scrollTop).toBe(100);
  });

  it("appendInfo joins main and sub", () => {
    const { sessions, messages } = setup();
    sessions.appendInfo("done", "cache 90%");
    expect(messages.querySelector(".msg-info")?.textContent).toBe("done · cache 90%");
  });

  it("streams deltas into a live bubble then finalizes as markdown", () => {
    const { sessions, messages } = setup();
    sessions.appendDelta("Hel");
    sessions.appendDelta("lo **w**");
    const live = messages.querySelector(".msg-assistant.streaming") as HTMLElement;
    expect(live).not.toBeNull();
    expect(live.textContent).toBe("Hello **w**");
    // 定稿：替换为 markdown 渲染，去掉 streaming，且不新增气泡
    sessions.appendMsg("assistant", "Hello **w**");
    expect(messages.querySelectorAll(".msg-assistant").length).toBe(1);
    expect(messages.querySelector(".msg-assistant.streaming")).toBeNull();
    expect(messages.querySelector(".msg-assistant")?.innerHTML).toContain("<strong>w</strong>");
  });

  it("tool call renders a collapsed details with result nested inside", () => {
    const { sessions, messages } = setup();
    sessions.appendToolCall("shell", { command: "ls" }, "$ ls");
    sessions.appendToolResult("file1\nfile2");
    const details = messages.querySelector("details.tool") as HTMLDetailsElement;
    expect(details).not.toBeNull();
    expect(details.open).toBe(false); // 默认收起
    expect(details.querySelector("summary")?.textContent).toContain("$ ls");
    expect(details.querySelector(".tool-body .tool-result")?.textContent).toBe("file1\nfile2");
  });

  it("renderList builds clickable rows with active highlight + delete (任务 section)", async () => {
    const { sidebar, calls } = sidebarWithTasks();

    const rows = sidebar.querySelectorAll(".task-row");
    expect(rows.length).toBe(2);
    // 无工作目录的任务进「任务」段；无名任务显示「当前任务」。
    expect(sidebar.querySelector(".task-group-head")?.textContent).toBe("任务 (2)");
    expect(sidebar.querySelector(".task-row.active .task-name")?.textContent).toBe("当前任务");
    (rows[1].querySelector(".task-name") as HTMLElement).click();
    expect(calls.switched).toBe("bbb22222");

    // 删除要过二次确认：点 ✕ 只弹框，点「删除」才真的回调。
    (rows[0].querySelector(".task-del") as HTMLElement).click();
    const ok = document.querySelector<HTMLElement>(".overlay [data-ok]");
    expect(ok).toBeTruthy(); // 点 ✕ 应弹出确认框
    ok?.click();
    await flush();
    expect(calls.deleted).toBe("aaa11111");
  });

  // 确认框存在的意义就是防误删，可这条路径此前没有任何覆盖。
  it("删除确认框点「取消」不删会话", async () => {
    const { sidebar, calls } = sidebarWithTasks();
    const rows = sidebar.querySelectorAll(".task-row");

    (rows[0].querySelector(".task-del") as HTMLElement).click();
    (document.querySelector(".overlay [data-cancel]") as HTMLElement).click();
    await flush();

    expect(calls.deleted).toBe(""); // 取消 = 一个都不能删
    expect(document.querySelector(".overlay")).toBeNull(); // 确认框应已关闭
  });

  it("rename button swaps in an input; Enter commits via onRename and updates the row", () => {
    const messages = document.createElement("div");
    const sidebar = document.createElement("div");
    document.body.append(messages, sidebar);
    let renamed: string[] = [];
    const s = new Sessions({
      messages,
      sidebar,
      onRename: (id, name) => {
        renamed = [id, name];
      },
    });
    s.setAll([{ id: "t1", name: "自动生成的看不出干嘛的名字" }], false, 0);
    s.renderList();

    (sidebar.querySelector(".task-ren") as HTMLElement).click();
    const input = sidebar.querySelector(".task-name-input") as HTMLInputElement;
    expect(input).not.toBeNull();
    expect(input.value).toBe("自动生成的看不出干嘛的名字");

    input.value = "  修 MUD 战斗  ";
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(renamed).toEqual(["t1", "修 MUD 战斗"]); // 去除首尾空白后提交
    // 行恢复文本显示，名字已本地更新（不等服务端广播）。
    expect(sidebar.querySelector(".task-name-input")).toBeNull();
    expect(sidebar.querySelector(".task-name")?.textContent).toBe("修 MUD 战斗");
  });

  it("rename input Escape or unchanged name cancels without firing onRename", () => {
    const messages = document.createElement("div");
    const sidebar = document.createElement("div");
    document.body.append(messages, sidebar);
    let fired = 0;
    const s = new Sessions({
      messages,
      sidebar,
      onRename: () => {
        fired++;
      },
    });
    s.setAll([{ id: "t1", name: "原名" }], false, 0);
    s.renderList();

    // Escape 取消
    (sidebar.querySelector(".task-ren") as HTMLElement).click();
    let input = sidebar.querySelector(".task-name-input") as HTMLInputElement;
    input.value = "改了但按了 Esc";
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(fired).toBe(0);
    expect(sidebar.querySelector(".task-name")?.textContent).toBe("原名");

    // 名字没变 / 清空 → 不提交
    (sidebar.querySelector(".task-ren") as HTMLElement).click();
    input = sidebar.querySelector(".task-name-input") as HTMLInputElement;
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(fired).toBe(0);
    (sidebar.querySelector(".task-ren") as HTMLElement).click();
    input = sidebar.querySelector(".task-name-input") as HTMLInputElement;
    input.value = "   ";
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(fired).toBe(0);
    expect(sidebar.querySelector(".task-name")?.textContent).toBe("原名");
  });

  it("clicking + on a workspace folder creates a new session in that working directory", () => {
    const messages = document.createElement("div");
    const sidebar = document.createElement("div");
    document.body.append(messages, sidebar);
    let newSessionWorkdir: string | undefined;
    const s = new Sessions({
      messages,
      sidebar,
      onNewSession: (workingDir) => {
        newSessionWorkdir = workingDir;
      },
    });
    s.setAll([{ id: "t1", name: "修 MUD", working_dir: "D:/work/mud" }], false, 0);
    s.renderList();

    // 每个工作空间目录的组头右侧都有一个 + 按钮。
    const btn = sidebar.querySelector(".ws-folder-head .ws-new-session") as HTMLElement;
    expect(btn).not.toBeNull();
    btn.click();
    expect(newSessionWorkdir).toBe("D:/work/mud");
  });

  it("renderList groups tasks with a working_dir under 工作空间 by folder leaf", () => {
    const messages = document.createElement("div");
    const sidebar = document.createElement("div");
    document.body.append(messages, sidebar);
    const s = new Sessions({ messages, sidebar });
    s.setAll(
      [
        { id: "t1", name: "随便聊" }, // 无工作目录 → 任务段
        { id: "t2", name: "做个 mud", working_dir: "D:/work/mud", status: "working" },
        { id: "t3", name: "另一个 mud 任务", working_dir: "D:/work/mud" },
        { id: "t4", name: "狼人杀", working_dir: "D:/games/AIWolfKill" },
      ],
      false,
      0,
    );
    s.renderList();

    const heads = [...sidebar.querySelectorAll(".task-group-head")].map((h) => h.textContent);
    expect(heads).toContain("任务 (1)");
    expect(heads).toContain("工作空间 (2)"); // 两个不同目录
    const folders = [...sidebar.querySelectorAll(".ws-folder-head .ws-name")].map(
      (n) => n.textContent,
    );
    expect(folders).toEqual(["mud", "AIWolfKill"]);
    // 运行中的任务在行右侧显示 loading 转圈。
    expect(sidebar.querySelectorAll(".task-spin").length).toBe(1);
  });

  it("renderHistory replays user/assistant/tool messages", () => {
    const { sessions, messages } = setup();
    sessions.renderHistory([
      { role: "user", content: "q" },
      { role: "assistant", content: "**a**", tool_calls: [{ name: "shell", arguments: "{}" }] },
      { role: "tool", content: "out" },
    ]);
    expect(messages.querySelector(".msg-user")?.textContent).toBe("q");
    expect(messages.querySelector(".msg-assistant")?.innerHTML).toContain("<strong>a</strong>");
    expect(messages.querySelector("details.tool")).not.toBeNull();
  });

  it("renderHistory replays user image attachments as thumbnails", () => {
    const { sessions, messages } = setup();
    sessions.renderHistory([
      { role: "user", content: "look", images: ["data:image/png;base64,AAAA"] },
    ]);
    const img = messages.querySelector(".msg-user img.attach-thumb") as HTMLImageElement | null;
    expect(img).not.toBeNull();
    expect(img?.getAttribute("src")).toBe("data:image/png;base64,AAAA");
    // 文本仍在
    expect(messages.querySelector(".msg-user")?.textContent).toContain("look");
  });

  it("takePendingMessage returns once then clears", () => {
    const { sessions } = setup();
    sessions.setPendingMessage("s1", "hi");
    expect(sessions.takePendingMessage()).toEqual({ session_id: "s1", content: "hi" });
    expect(sessions.takePendingMessage()).toBeNull();
  });
});
