# QQ 接入（OneBot / NapCat）

把 WiseCortex 接进 QQ 群和私聊。适合做群客服、群助手。

> ⚠️ **这条路是非官方的。** NapCat 登录的是**个人 QQ 账号**，不是腾讯开放平台的机器人，
> 有违反 QQ 用户协议、账号被封的风险。用小号，风险自负。
> 想合规就走「QQ 机器人（官方）」那条通道——代价是只能收 @ 消息、有 5 分钟被动回复窗口和频控。

## 它是怎么连起来的

```
手机 QQ 扫码  →  NapCat（持有 QQ 登录态）  ←─ HTTP ─→  WiseCortex
```

**NapCat 不是 WiseCortex 的一部分**，是个独立的第三方进程，你得自己装、自己跑。
WiseCortex 只跟它说 HTTP，压根不知道你用的是哪个 QQ 号——所以 WiseCortex 界面上
**没有**「填 QQ 号」的地方，那不是漏了。

### 为什么 NapCat 需要装 QQ 客户端

NapCat **不重新实现 QQ 协议**（上一代的 go-cqhttp、mirai 才是那样，已基本被风控团灭）。
它直接调用官方 QQ 客户端自带的协议引擎（NT 架构的 Node 模块），只是不启动图形界面。

所以「无头」= 不开窗口，**不等于**不需要 QQ。你要装 QQ，是因为 NapCat 需要它那套引擎文件；
但你**不需要打开 QQ，也不需要在 QQ 里登录**，NapCat 会自己把引擎拉起来。

好处是发出去的流量和真人用 QQ 没有区别——**协议层伪装得很好**。
但行为层照样暴露：秒回、24 小时在线、消息模式规律，一样会进风控。

---

## 一、安装 NapCat

### Linux（推荐，Ubuntu 20+ / Debian 10+）

```bash
curl -o napcat.sh https://nclatest.znin.net/NapNeko/NapCat-Installer/main/script/install.sh
sudo bash napcat.sh --tui
```

TUI 里选 **Shell (Rootless)**。脚本会把 linuxqq 一并装上。

装之前先看内存，linuxqq + NapCat 大概吃 300MB–1GB：

```bash
free -h
```

### Windows

1. 先装好 **Windows QQ 客户端**（必须已安装且是新版）。
2. 从 [Releases](https://github.com/NapNeko/NapCatQQ/releases) 下 `NapCat.Shell.zip`，解压。
3. **Windows 10 双击 `launcher-win10.bat`；Windows 11 双击 `launcher.bat`。** 别点错。
4. 以后快速登录：`launcher-win10.bat <你的QQ号>`，免扫码。

---

## 二、扫码登录

启动后控制台会直接打出二维码，**用手机 QQ 扫**——扫的是哪个号，机器人就是哪个号。

乱码或不显示时走 WebUI：日志里有一行 `http://127.0.0.1:6099/webui?token=XXXXX`。
token 是**自动生成**的（不是固定值），在启动日志或 `config/webui.json` 里。
6099 被占用时会自动 +1。

### 服务器上怎么扫（无图形界面）

**别把 6099 对公网开** —— WebUI 只有一个 token 保护，暴露出去等于把 QQ 号送人。
在**你自己电脑上**开 SSH 隧道：

```bash
ssh -L 6099:127.0.0.1:6099 <用户名>@<服务器IP>
```

保持窗口开着，本地浏览器访问 `http://127.0.0.1:6099/webui`，在里面扫码。
扫完就能关隧道，登录态留在服务器上。

> **异地登录风控**：QQ 从机房 IP 登录是明确的风险信号，比挂机器人本身更容易触发验证。
> 缓解办法只有一个——**选定一台就别来回切**，反复换登录地比一直待在机房更糟。

---

## 三、配置网络（NapCat WebUI → 网络配置）

建**两条**，缺一不可：

**① HTTP 服务器** —— WiseCortex 调它发消息

| 项 | 值 |
|---|---|
| 主机 | `127.0.0.1` |
| 端口 | `3000` |
| Token | **留空** |

> Token 必须留空：WiseCortex 的 `notify::post_json` 不发 `Authorization` 头，
> NapCat 一开校验就 401，**回复全部静默失败**——WiseCortex 里看得到输出，群里一个字没有。

**② HTTP 客户端** —— 它把消息推给 WiseCortex

| 项 | 值 |
|---|---|
| URL | `http://127.0.0.1:7070/api/im/onebot` |
| 上报自身消息 | **关闭** |
| 消息格式 | `string` |

> 两者不在同一台机器时，把 `127.0.0.1` 各自换成对方的内网 IP。同机部署则一个端口都不用对外开。

---

## 四、配置 WiseCortex

建一个 onebot 通道（**漏了这步收得到消息但回不出去**）：

```bash
wisecortex channel add --name qq --kind onebot --url http://127.0.0.1:3000 --target <群号>
```

或走界面：通道页 →「QQ · OneBot / NapCat」→ 填 HTTP 基地址和群号。

没配的表现是：消息收得到、agent 也跑了，但群里一片安静。这种情况会往 `error.log` 写一条。

---

## 五、用

| 场景 | 怎么发 |
|---|---|
| 群聊 | `wc <你的问题>` —— **`wc` 必须是整条消息的开头** |
| 私聊 | 直接说，不用前缀 |

**别 @ 机器人。** @ 在 `raw_message` 里是 `[CQ:at,qq=...]`，会挡在 `wc` 前面导致前缀匹配失败。

### 会话与知识库

每个群、每个私聊**各绑各的会话**：

| 来源 | 会话 id |
|---|---|
| 群 999 | `onebot-g999` |
| QQ 12345 的私聊 | `onebot-u12345` |

**绑定互不影响**——你在私聊里 `/switch`，群里不会跟着变。要配某个群，就得**在那个群里**发命令。

知识库、模型、技能、工作目录都是**跟着会话**走的。所以典型用法是：

1. 在 WiseCortex 网页上建好会话，各自绑各自的知识库
2. 在目标群里发 `wc /sessions` 看序号
3. `wc /switch 2` → 这个群从此跑在那个会话上，带着它的知识库和模型

可用命令：`/sessions` `/switch <序号>` `/model` `/modellist` `/setworkdir` `/workdir` `/reset` `/help`。
（QQ 里只能**用**知识库，改绑定要回网页。）

### 命令的两条安全规则

**1. 命令结果私聊回，群里不回显。**
`/sessions` 会列出你全部会话的名字，打进客服群等于把手上所有活儿念给客户听。
所以在群里发命令，群里一片安静，结果私聊送到你手上——**绑定的仍是那个群**。

**2. 白名单。** 配置项 `onebot_admins`（QQ 号列表），或环境变量优先：

```bash
WC_ONEBOT_ADMINS=123456,789012
```

名单外的人发 `/` 命令会被**静默忽略**（不回「你没权限」——那等于当众宣布这里有命令可打），
普通提问照常回答。

- **留空 = 不限制**，自用群不受影响。
- 环境变量填了却一个 QQ 号都解析不出来时，**按「禁止所有人执行命令」处理**并记一条日志。
  安全开关配错了必须立刻显形，不能悄悄退回「谁都能执行」。

> 未知的 `/xxx` 不算命令，会照常交给模型——「/etc/hosts 怎么改」这类提问不受白名单影响。

---

## 开机自启（systemd）

安装脚本给的是 `screen` 启动法，**服务器一重启 NapCat 就没了**，且是静默失联。生产环境换成 systemd。

先停掉 screen 实例——两个实例同时登同一个 QQ 号会互相踢下线，症状时好时坏很难查：

```bash
screen -S napcat -X quit
screen -ls                       # 应为 No Sockets found
```

拿到 QQ 号（配置文件名里那串数字）与 xvfb-run 路径：

```bash
ls /root/Napcat/opt/QQ/resources/app/app_launcher/napcat/config/ | grep onebot11
which xvfb-run
```

`/etc/systemd/system/napcat.service`：

```ini
[Unit]
Description=NapCat (QQ OneBot)
After=network-online.target
Wants=network-online.target
# 重启节流：5 分钟内连续失败 3 次就停手。QQ 登录态过期时它一直起不来，
# 没有这个限制会无限重启、刷爆日志而你毫不知情。
StartLimitIntervalSec=300
StartLimitBurst=3

[Service]
Type=simple
User=root
ExecStart=/usr/bin/xvfb-run -a /root/Napcat/opt/QQ/qq --no-sandbox -q <QQ号>
Restart=always
RestartSec=30
# xvfb-run 派生 Xvfb 和 QQ 两个子进程，必须整组回收，否则停服务后残留进程占着端口。
KillMode=control-group

[Install]
WantedBy=multi-user.target
```

```bash
systemctl daemon-reload
systemctl enable --now napcat
systemctl status napcat --no-pager
```

换成 systemd 后 **`screen -r napcat` 不再可用**，日志看 `journalctl -u napcat -f`。

### QQ 登录态过期了怎么办

登录态几周到几个月会过期。届时 `-q` 快速登录失败、服务重启 3 次后停住，机器人静默失联
（`systemctl status napcat` 显示 `failed`）。重新扫码：

```bash
systemctl stop napcat
screen -dmS napcat bash -c "xvfb-run -a /root/Napcat/opt/QQ/qq --no-sandbox"
screen -r napcat                 # 扫码，完了 Ctrl+A 然后 D
screen -S napcat -X quit
systemctl start napcat
```

---

## 排查

| 现象 | 多半是 |
|---|---|
| 群里没反应 | 没带 `wc` 前缀，或 @ 了机器人 |
| agent 跑了但群里没回复 | 没建 onebot 通道，或 NapCat 的 HTTP 服务器配了 token |
| 私聊自问自答 | NapCat 的「上报自身消息」没关（代码里已挡一层，但白费一轮往返） |
| 命令没反应 | 不在 `onebot_admins` 白名单里 |
| 每句话都失忆 | 跑的还是旧版本，重启 WiseCortex |

日志在 `~/.local/share/wisecortex/logs/error.log`（`buglog` scope 为 `onebot`）。
注意 **error.log 只收 `buglog::record`，不捕获 stderr**——「日志里没报错」证明不了任何事。

## 参考

- [NapCat 原理](https://napneko.github.io/guide/napcat)
- [NapCat-Installer](https://github.com/NapNeko/NapCat-Installer)
- [NapCat WebUI 配置](https://napneko.github.io/config/basic)
