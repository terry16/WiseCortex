import type { MessageKey } from "./en";

// 日本語
export const ja = {
  // generic
  "common.done": "完了",
  "common.add": "追加",
  "common.remove": "削除",
  "common.itemCount": "{n} 件",

  // nav / shell
  "brand.sub": "セルフホスト型 AI エージェント",
  "nav.newTask": "新規タスク",
  "nav.chat": "チャット",
  "nav.tasks": "スケジュール",
  "nav.jobs": "バックグラウンド",
  "nav.skills": "スキル",
  "nav.channels": "通知チャンネル",
  "nav.settings": "設定",

  // theme
  "theme.toggle": "テーマ切替（ライト / ダーク）",

  // footer (rail)
  "foot.modelLabel": "現在のモデル",
  "foot.costLabel": "本日のコスト",
  "foot.model.unset": "未設定",
  "foot.access.locked": "access_key ロック中",
  "foot.access.public": "公開アクセス",
  "foot.versionTitle": "サーバーのバージョン",

  // offline
  "offline.banner": "接続が切断されました — 再接続中…",

  // login gate
  "login.sub": "このインスタンスは保護されています。アクセスキーを入力してください。",
  "login.placeholder": "アクセスキー",
  "login.enter": "入る",
  "login.err.wrong": "キーが違います。もう一度お試しください",
  "login.err.unreachable": "サーバーに接続できません。後でもう一度お試しください",
  "auth.prompt": "この WiseCortex はアクセスキーが必要です。入力してください：",

  // attachments
  "attach.removeHint": "クリックで削除",

  // composer
  "composer.placeholder": "WiseCortex にタスクを指示…（Enter で送信、Shift+Enter で改行）",
  "composer.more": "操作を展開",
  "composer.stop": "停止",
  "composer.cwd.label": "作業ディレクトリ",
  "composer.cwd.defaultDir": "既定のディレクトリ",
  "composer.cwd.task": "タスクの作業ディレクトリ：{dir}",
  "composer.cwd.locked": "（タスク開始済み、ロック中）",
  "composer.cwd.global": "作業ディレクトリ（グローバル）：{dir}",
  "composer.cwd.pickPrompt": "タスクの作業ディレクトリ（空＝グローバルのワークスペースを使用）：",
  "composer.model.title": "このタスクのモデル（既定はグローバルに従う）",
  "composer.effort.title": "このチャットの思考強度（モデルが非対応なら無視）",
  "composer.effort.opt.default": "思考：既定",
  "composer.effort.opt.off": "思考：オフ",
  "composer.effort.opt.low": "思考：low",
  "composer.effort.opt.medium": "思考：medium",
  "composer.effort.opt.high": "思考：high",
  "composer.effort.opt.xhigh": "思考：xhigh",
  "composer.effort.opt.max": "思考：max",
  "composer.skills.pinned": "このタスクのスキル（{n}）：{list}",
  "composer.skills.empty": "このタスクのスキル（既定は自動選択、クリックでピン留め）",
  "composer.perm.title": "ツール操作の権限",
  "composer.perm.default": "既定の権限",
  "composer.perm.solo": "solo（自動承認）",
  "composer.perm.strict": "厳格（各ステップで確認）",
  "composer.plan.on":
    "プランモード：オン（読み取り専用の調査＋計画、変更不可）。承認後はここをオフにして「実行」と指示してください",
  "composer.plan.off": "プランモード：読み取り専用の調査＋計画、変更不可。クリックで有効化",
  "composer.kb.pinned": "このチャットのナレッジベース（{n}）：\n{list}",
  "composer.kb.empty": "ナレッジベース（このチャット、クリックでフォルダ/ファイルを追加）",
  "composer.mem.title": "メモリ（AI が覚えている内容の確認・編集）",

  // skills picker
  "skillsPicker.title": "このタスクのスキル",
  "skillsPicker.note":
    "0 個ピン留め＝AI が全スキルから自動選択。いくつかピン留め＝このタスクはそれらのみ使用（安定し、誤選択なし）。",
  "skillsPicker.empty": "利用できるスキルがありません。",
  "skillsPicker.projectBadge": "プロジェクト",
  "skillsPicker.projectBadge.title": "このタスクの作業ディレクトリ skills/ から",

  // knowledge base
  "kb.title": "ナレッジベース（このチャット）",
  "mem.title": "メモリ",
  "mem.note":
    "AI が remember ツールで自ら記録し、毎ターン注入されます。ここで実際に何を覚えているかを確認し、誤りを直せます。<b>Lessons</b> の項目は「繰り返してはいけない失敗」で、AI は最優先で従うよう指示され、容量超過時も最後まで残ります。",
  "mem.project": "プロジェクトメモリ — {dir}",
  "mem.projectHint":
    "この作業ディレクトリの<b>全セッションで共有</b>され、新しい会話にも引き継がれます。常に守ってほしい決まりはここへ。",
  "mem.session": "この会話のみ",
  "mem.sessionHint": "現在の会話だけに属し、会話を削除すると一緒に消えます。",
  "mem.empty": "（空）",
  "kb.note":
    "このチャット用にフォルダやファイルをナレッジベースとしてマウントします。質問時に AI が knowledge_search で検索して回答します（ローカルのみ、このチャットのみ有効）。",
  "kb.addLabel": "パスを追加（フォルダまたはファイル、絶対パス）",
  "kb.input.placeholder": "例：D:\\kb または ~/wisecortex/workspace/kb/faq.md",
  "kb.pick.title": "フォルダを選択",
  "kb.empty": "まだナレッジベースのパスがありません。",
  "kb.pickPrompt": "ナレッジベースのフォルダの絶対パス：",

  // chat lifecycle (used by ws-dispatcher)
  "chat.done": "完了（{n} ターン、{cost}）",
  "chat.done.duration": " · {duration}s",
  "chat.done.cache": "キャッシュヒット {rate}%（{hits}/{total}、{tokens}）",
  "chat.interrupted": "中断しました",
  "chat.queued": "⏳ キューに追加しました — 現在のターン終了後に自動処理します",
  "chat.retry": "再試行",
  "chat.retrying": "再試行中…",
  "error.insufficient_credit": "残高不足",
  "error.insufficient_credit.action": "チャージ",

  // artifacts
  "artifact.rendered": "生成済み · クリックで右側にプレビュー",
  "artifact.source": "生成済み · クリックでソースを表示",

  // hero (empty state)
  "greet.night": "夜更かしですね",
  "greet.morning": "おはようございます",
  "greet.afternoon": "こんにちは",
  "greet.evening": "こんばんは",
  "hero.title": "{greet}、何をお手伝いしましょう？",
  "hero.sub":
    "WiseCortex はコードの作成、スクリプト実行、オンライン調査、スキル呼び出しを行い、結果を IM に送信できます。",
  "suggest.landing.title": "製品ランディングページを作る",
  "suggest.landing.sub": "概要からレスポンシブな 1 ページを生成",
  "suggest.research.title": "テーマをオンライン調査",
  "suggest.research.sub": "資料を集めて要点を要約",
  "suggest.debug.title": "バグの原因を突き止めて",
  "suggest.debug.sub": "根本原因を特定して修正",
  "suggest.chart.title": "データをグラフにする",
  "suggest.chart.sub": "CSV / 表 → 可視化",

  // settings — language
  "settings.language.title": "表示言語",
  "settings.language.sub": "切り替えると即時に反映されます（ページが再読み込みされます）。",
  "settings.language.label": "言語",

  // settings — generic
  "common.cancel": "キャンセル",
  "common.save": "保存",
  "settings.pick": "選択…",
  "settings.keySetPlaceholder": "設定済み（空欄=変更しない）",

  // settings — model access
  "settings.models.title": "モデル接続",
  "settings.models.sub": "BYOK で自分の API キーを使用。モデルごとに価格を設定してコスト計算に利用",
  "settings.models.add": "モデルを追加",
  "settings.models.default": "既定モデル",
  "settings.models.empty": "モデルがまだありません。「モデルを追加」から設定してください。",
  "settings.models.badge.default": "既定",
  "settings.models.badge.vision": "ビジョン",
  "settings.models.badge.visionTitle": "画像入力に対応",
  "settings.models.priceIn": "入力",
  "settings.models.priceOut": "出力",
  "settings.models.priceUnit": "¥ / 百万トークン",
  "settings.models.keySet": "キー設定済み",
  "settings.models.keyUnset": "未設定",
  "settings.models.subscription": "サブスク枠",
  "settings.models.subscriptionTitle":
    "このモデルはサブスクリプションログイン（OAuth）の枠を使うため、API キーは不要です",
  "settings.models.configure": "設定",
  "settings.models.delete": "削除",
  "settings.models.optionLabel": "{model}（{provider}）",

  // settings — workspace
  "settings.workspace.title": "作業ディレクトリ",
  "settings.workspace.sub":
    "AI 生成スクリプト / ナレッジベース / 自己学習のグローバルな拠点。各チャットでは入力欄から一時ディレクトリを個別指定できます",
  "settings.workspace.label": "グローバル作業ディレクトリ",
  "settings.workspace.placeholder": "空欄=システム既定を使用",
  "settings.workspace.hintTauri":
    "「選択…」でシステムダイアログからディレクトリを選択。パスを直接編集することもできます。",
  "settings.workspace.hintWeb": "WebUI：サーバー上の絶対パスを入力してください。",
  "settings.workspace.pickPrompt": "グローバル作業ディレクトリ（空欄=システム既定）：",

  // settings — proxy
  "settings.proxy.title": "ネットワークプロキシ",
  "settings.proxy.sub":
    "設定すると、すべての送信アクセス（LLM、スキル/マーケット、web ツール）がプロキシ経由になります。空欄=直接接続",
  "settings.proxy.label": "プロキシアドレス",
  "settings.proxy.placeholder": "http://host:port または socks5://host:port",
  "settings.proxy.hint":
    "http/https/socks5 に対応。変更後すぐ反映（自動ホットリロード）。環境変数 WC_PROXY でも上書きできます。",

  // settings — Claude subscription OAuth
  "settings.claudeOauth.title": "Claude サブスクリプションログイン",
  "settings.claudeOauth.sub":
    "Claude サブスクリプション（Pro/Max）枠でローカル推論を実行し、モデルを横並びで比較できます。ログイン後、「モデル管理」で「Claude サブスクリプション」にチェックを入れたモデルを追加すると使えます。",
  "settings.claudeOauth.step1":
    "ブラウザが自動で開かない場合（デスクトップシェルでよくあります）、「開く」または「リンクをコピー」で認可ページを手動で開いてください：",
  "settings.claudeOauth.step2":
    "ログイン後、ページに表示される code をコピーして下に貼り付けて完了：",
  "settings.claudeOauth.codePlaceholder": "認可 code を貼り付け（code#state の形式）",
  "settings.claudeOauth.warn":
    "⚠️ Claude Code の OAuth クライアントを流用しています。第三者利用はグレーゾーンで、個人のローカル利用のみ。公式ポリシーで制限されたり、いつでも失効する可能性があります。",
  "settings.claudeOauth.loggedIn": "✓ Claude サブスクリプションにログイン済み",

  // settings — ChatGPT subscription OAuth
  "settings.chatgptOauth.title": "ChatGPT サブスクリプションログイン",
  "settings.chatgptOauth.sub":
    "ChatGPT（Plus/Pro）サブスクリプション枠で Codex モデルをローカル実行し、比較できます。ログイン後、「モデル管理」で「ChatGPT サブスクリプション」にチェックを入れたモデルを追加すると使えます。",
  "settings.chatgptOauth.step1":
    "「開く」または「リンクをコピー」でブラウザの認可ページを開いてログインしてください：",
  "settings.chatgptOauth.step2":
    "認可後、ブラウザは <code>localhost:1455</code> にリダイレクトします（たいてい開けませんが正常です）。<b>アドレスバーの URL 全体</b>、またはその中の code を下に貼り付けてください：",
  "settings.chatgptOauth.codePlaceholder":
    "code または localhost:1455/auth/callback?code=... のアドレス全体を貼り付け",
  "settings.chatgptOauth.warn":
    "⚠️ Codex の OAuth クライアントを流用しています。第三者利用はグレーゾーンで、個人のローカル利用のみ。公式ポリシーで制限されたり、いつでも失効する可能性があります。プロキシが必要です。",
  "settings.chatgptOauth.loggedIn": "✓ ChatGPT サブスクリプションにログイン済み",
  "settings.grokOauth.title": "Grok サブスクリプションログイン",
  "settings.grokOauth.sub":
    "X Premium / SuperGrok サブスクリプション枠で Grok モデルを実行（デバイスコードログイン、ローカル/サーバー配備どちらでも可）。ログイン後「モデル管理」で「Grok サブスクリプション」にチェックしたモデルを追加してください。",
  "settings.grokOauth.step1": "「開く」を押して、任意のデバイスのブラウザで認可ページを開きます：",
  "settings.grokOauth.step2":
    "認可ページで以下のコードを入力すると、完了後このページは自動的にログインします：",
  "settings.grokOauth.warn":
    "⚠️ xAI 公式 Grok-CLI の OAuth クライアントを流用しています。第三者利用はグレーゾーンで、個人利用のみ。公式ポリシーで制限されたり、いつでも失効する可能性があります。",
  "settings.grokOauth.loggedIn": "✓ Grok サブスクリプションにログイン済み",
  "settings.grokOauth.waiting": "認可待ち…ブラウザで完了するとこのページは自動更新されます",
  "settings.grokOauth.expired":
    "デバイスコードの期限が切れました。もう一度「ログイン」を押してください",

  // settings — OAuth shared
  "settings.oauth.checking": "確認中…",
  "settings.oauth.login": "ログイン",
  "settings.oauth.logout": "ログアウト",
  "settings.oauth.open": "開く",
  "settings.oauth.copyLink": "リンクをコピー",
  "settings.oauth.notLoggedIn": "未ログイン",
  "settings.oauth.relogin": "再ログイン",
  "settings.oauth.unknown": "状態不明",
  "settings.oauth.openedCode": "認可ページを開きました。ログイン後に code を貼り付けてください",
  "settings.oauth.openedUrl":
    "認可ページを開きました。ログイン後にアドレスバーの URL を貼り付けてください",
  "settings.oauth.manualOpen": "「開く」または「リンクをコピー」で認可ページを手動で開いてください",
  "settings.oauth.waitingBrowser":
    "ブラウザで認可ページを開きました。ログインを完了すると自動的に検出します…",
  "settings.oauth.loopbackCopied":
    "認可リンクをコピーしました。ブラウザで開いてログインを完了してください。自動的に検出します…",
  "settings.oauth.loopbackTimeout": "認可待ちがタイムアウトしました。もう一度ログインしてください",
  "settings.oauth.loginFailed": "ログイン失敗：{e}",
  "settings.oauth.copied": "リンクをコピーしました。ブラウザで開いてください",
  "settings.oauth.redeeming": "交換中…",
  "settings.oauth.failed": "失敗：{e}",
  "settings.oauth.unknownError": "不明なエラー",

  // settings — iteration limits
  "settings.maxiter.title": "ツール呼び出しターン上限",
  "settings.maxiter.sub":
    "1 タスクで連続してツールを呼び出せる最大ターン数（暴走防止）。対話チャットと無人タスクで別々に設定します。",
  "settings.maxiter.interactive": "対話チャット上限",
  "settings.maxiter.interactivePlaceholder": "空欄=既定 1000",
  "settings.maxiter.interactiveHint":
    "デスクトップ/Web で自分が操作するチャットに適用——高めに設定。開発ではほぼ到達せず、いつでも手動で停止できます。**バックグラウンド長時間タスク（task_start）もこの枠を使います。**環境変数 WC_MAX_ITERATIONS_INTERACTIVE が優先。",
  "settings.maxiter.unattended": "無人タスク上限",
  "settings.maxiter.unattendedPlaceholder": "空欄=既定 50",
  "settings.maxiter.unattendedHint":
    "スケジュールタスク / Feishu·WeCom トリガーのタスクに適用（無人、暴走コスト防止）。複数ファイル生成＋デプロイのような長いタスクでは高めに。環境変数 WC_MAX_ITERATIONS が優先。",
  "settings.maxiter.subagent": "サブタスク（サブ agent）の回数上限",
  "settings.maxiter.subagentPlaceholder": "空欄=既定 100",
  "settings.maxiter.subagentHint":
    "チャットの <b>task</b> ツールが派生するサブ agent に適用されます。以前は 15 固定で低すぎ、差分レビューやログ解析ではファイルを数個読むだけで使い切り、サブタスクはほぼ必ず「回数上限に到達」で終わっていました。メイン会話と別項目なのは、サブタスクが並列で複数走ることが多くコストが乗算になるためです。環境変数 WC_SUBAGENT_MAX_ITERATIONS が優先されます。",

  // settings — log cleanup
  "settings.logclean.title": "スケジュールタスクのログ整理",
  "settings.logclean.sub":
    "スケジュールタスクの実行ログはサイズで整理します。上限を超えたファイルだけを古い「実行 1 回分」単位で削り、上限内のログには一切触れません。1 時間ごとに確認します。",
  "settings.logclean.autoLabel": "ログを自動整理",
  "settings.logclean.autoSub": "オフにするとログは永久保持され、手動で整理します。",
  "settings.logclean.maxLabel": "タスクごとのログ上限（MB）",
  "settings.logclean.placeholder": "空欄=既定 10",
  "settings.logclean.hint":
    "上限を超えると古い<b>実行 1 回分</b>から削り、直近 1 回は必ず残します——「何回も実行したのにログが空」にはなりません。変更は即時反映、再起動不要。",

  // settings — web search
  "settings.websearch.title": "ウェブ検索",
  "settings.websearch.sub":
    "web_search ツールのプロバイダー。API キーを入力するか SearXNG をセルフホスト。空欄=DuckDuckGo（キー不要）",
  "settings.websearch.provider": "プロバイダー",
  "settings.websearch.ddg": "DuckDuckGo（既定、キー不要）",
  "settings.websearch.searxng": "SearXNG（セルフホスト）",
  "settings.websearch.brave": "Brave Search（API キー）",
  "settings.websearch.tavily": "Tavily（API キー）",
  "settings.websearch.searxngUrl": "SearXNG アドレス",

  // settings — reasoning effort
  "settings.effort.title": "推論強度（extended thinking）",
  "settings.effort.sub":
    "回答前にモデルをより深く考えさせます——複雑なタスクや難しいバグで安定しますが、所要時間とコストが増えます。プロバイダーごとに自動変換：Anthropic は thinking、OpenAI/Gemini は reasoning_effort、Qwen/Hunyuan は enable_thinking。DeepSeek など思考が組み込まれたモデルはパラメータを送りません。",
  "settings.effort.label": "強度",
  "settings.effort.off": "オフ（既定）",
  "settings.effort.xhigh": "xhigh（コーディングの最適点）",
  "settings.effort.hint":
    "変更後すぐ反映（自動ホットリロード）。環境変数 WC_REASONING_EFFORT でも上書きできます。",

  // settings — access control
  "settings.access.title": "アクセス制御",
  "settings.access.sub": "ローカルの WS/REST を保護し、危険な操作を制御します",
  "settings.access.enableKey": "access_key を有効化",
  "settings.access.enableKeySub":
    "オンにすると、すべての接続でキーが必要になります（変更後はサーバー再起動で反映）。<b>外部アクセスには必須</b>——サーバーは既定でローカルのみにバインドします。外部公開には nginx リバースプロキシを使ってください。",
  "settings.access.keyPlaceholder": "アクセスキーを設定",
  "settings.access.envManaged":
    "環境変数 WC_ACCESS_KEY で設定されています —— サーバー側で変更してください（ここでは編集できません）。",
  "settings.access.confirm": "危険な操作の前に確認",
  "settings.access.confirmSub":
    "オンにすると、ファイル書き込み / shell などの操作の前に同意を求めます（オフ=無人で全自動）",
  "settings.access.automem": "自動メモリ",
  "settings.access.automemSub":
    "オンにすると、長い会話では数ターンごとにバックグラウンドで要点を抽出してセッションメモリに書き込みます（追加の LLM 呼び出しが発生。必要に応じて有効化。変更後はサーバー再起動で反映）",
  "settings.access.autotrim": "コンテキスト自動最適化",
  "settings.access.autotrimSub":
    "画像は 1 回だけ送信し、以降のターンでは省略。圧縮時も画像を破棄します",
  "settings.access.autotrimHelp":
    "画像はコンテキストで最も高価です——スクリーンショット 1 枚で軽く 1000 トークン、しかも既定では毎ターンそのまま再送されます。オンにすると、画像は送信したターンだけコンテキストに入り、以降は 1 行のプレースホルダに置き換えられ、履歴圧縮時にも破棄されます。UI 修正やテスト実行では一度見れば十分です。同じ画像を繰り返し見比べたいときだけオフにしてください。",

  // settings — MCP
  "settings.mcp.title": "MCP サーバー",
  "settings.mcp.sub":
    "MCP サーバー（stdio または Streamable HTTP）に接続。その tools/resources/prompts は <code>mcp__server__tool</code> として AI に公開されます。変更後すぐ再接続",
  "settings.mcp.label":
    "サーバー設定（JSON：名前 → stdio {command,args,env} または HTTP {url,headers}）",
  "settings.mcp.hint":
    "resources/prompts は <code>list_resources</code>/<code>read_resource</code>/<code>list_prompts</code>/<code>get_prompt</code> ツールを自動合成します。sampling/roots の逆方向リクエストは自動処理（stdio のみ）。",
  "settings.mcp.save": "保存して再接続",
  "settings.mcp.discovered": "検出されたツール",
  "settings.mcp.noTools": "（接続済みツールはまだありません）",
  "settings.mcp.savedReconnecting": "保存しました。バックグラウンドで再接続中…",

  // settings — hooks
  "settings.hooks.title": "イベントフック（hooks）",
  "settings.hooks.sub":
    "イベント時点でコマンドを実行してガードレール/副作用を行います：PreToolUse はツールを遮断、PostToolUse はコンテキストを追加、SessionStart/Stop/UserPromptSubmit",
  "settings.hooks.label":
    "フック設定（JSON：イベント → [{matcher, command, timeout_ms}]；matcher はツール名の正規表現で、Pre/PostToolUse のみ使用）",
  "settings.hooks.save": "保存",
  "settings.hooks.hint":
    'コマンドは stdin でイベント JSON を受け取ります。遮断：JSON <code>{"decision":"block","reason":"…"}</code> または非 0 終了コード（理由は stderr から）。コンテキスト追加：<code>{"additionalContext":"…"}</code>。',

  // settings — statuses
  "settings.status.loading": "読み込み中…",
  "settings.status.loadFailed": "読み込み失敗：{e}",
  "settings.status.workspaceUpdated": "作業ディレクトリを更新しました",
  "settings.status.workspaceReset": "既定の作業ディレクトリに戻しました",
  "settings.status.websearchUpdated": "ウェブ検索を更新しました",
  "settings.status.jsonParseFailed": "JSON 解析に失敗：{e}",
  "settings.status.saving": "保存中…",
  "settings.status.saved": "保存しました",
  "settings.status.effortUpdated": "推論強度を更新しました",
  "settings.status.proxySet": "プロキシを設定しました（ホットリロード済み）",
  "settings.status.proxyOff": "プロキシをオフにしました（直接接続）",
  "settings.status.maxiterSet": "無人タスク上限を {v} に設定しました（ホットリロード済み）",
  "settings.status.maxiterReset": "既定の上限 50 に戻しました（ホットリロード済み）",
  "settings.status.maxiterInteractiveSet":
    "対話チャット上限を {v} に設定しました（ホットリロード済み）",
  "settings.status.maxiterInteractiveReset": "既定の上限 1000 に戻しました（ホットリロード済み）",
  "settings.status.maxiterSubagentSet": "サブタスク上限を {v} に設定しました（ホットリロード済み）",
  "settings.status.maxiterSubagentReset":
    "サブタスクの既定上限 100 に戻しました（ホットリロード済み）",
  "settings.status.logcleanDefault": "既定の上限 10MB",
  "settings.status.logcleanOn": "ログの自動整理を有効にしました",
  "settings.status.logcleanOff": "ログの自動整理を無効にしました（永久保持）",
  "settings.status.logcleanMb": "上限 {v}MB",
  "settings.status.logcleanUpdated": "ログ整理を更新しました：{msg}",
  "settings.status.accessKeyOff": "アクセスキーを無効化しました（サーバー再起動で反映）",
  "settings.status.accessKeySet": "アクセスキーを設定しました（サーバー再起動で反映）",
  "settings.status.updated": "更新しました",
  "settings.status.automemOn": "自動メモリを有効化しました（サーバー再起動で反映）",
  "settings.status.autotrimOn": "コンテキスト自動最適化をオンにしました（画像は 1 回だけ）",
  "settings.status.autotrimOff": "コンテキスト自動最適化をオフにしました（画像を毎ターン再送）",
  "settings.status.automemOff": "自動メモリを無効化しました（サーバー再起動で反映）",

  // settings — model modal
  "settings.modal.editTitle": "モデルを設定",
  "settings.modal.addTitle": "モデルを追加",
  "settings.modal.provider": "プロバイダー",
  "settings.modal.endpoint": "API エンドポイント",
  "settings.modal.useClaudeOauth": "Claude サブスクリプションを使用（API キー不要）",
  "settings.modal.useClaudeOauthHint":
    "チェックすると、このモデルはローカルでログイン済みの Claude サブスクリプション枠を使用します（Anthropic 公式エンドポイント固定）。先に「Claude サブスクリプションログイン」でログインしてください。上の「モデル」を Claude のモデル id（例：claude-sonnet-4-6）に設定します。",
  "settings.modal.useChatgptOauth": "ChatGPT サブスクリプションを使用（API キー不要）",
  "settings.modal.useChatgptOauthHint":
    "チェックすると、このモデルはローカルでログイン済みの ChatGPT サブスクリプション枠を使用します（Codex バックエンド + Responses トランスポート固定）。先に「ChatGPT サブスクリプションログイン」でログインしてください。上の「モデル」を Codex のモデル id（例：gpt-5-codex）に設定します。",
  "settings.modal.useGrokOauth": "Grok サブスクリプションを使用（API キー不要）",
  "settings.modal.useGrokOauthHint":
    "チェックすると、このモデルはログイン済みの Grok サブスクリプション枠を使用します（api.x.ai、OpenAI 互換トランスポート）。先に「Grok サブスクリプションログイン」でログインしてください。モデル例：grok-code-fast-1 / grok-4-1。",
  "settings.modal.useGeminiOauth": "Gemini サブスクリプションを使用（API キー不要）",
  "settings.modal.useGeminiOauthHint":
    "チェックすると、このモデルはログイン済みの Gemini サブスクリプション枠を使用します（Code Assist バックエンド、ネイティブ Gemini トランスポート）。先に「Gemini サブスクリプションログイン」でログインしてください。モデル例：gemini-2.5-pro / gemini-2.5-flash。",
  "settings.geminiOauth.title": "Gemini サブスクリプションログイン",
  "settings.geminiOauth.sub":
    "個人の Google アカウント（Gemini Code Assist の無料/有料枠）で Gemini モデルを実行します。ログイン後「モデル管理」で「Gemini サブスクリプション」にチェックしたモデルを追加してください。",
  "settings.geminiOauth.step1":
    "「開く」をクリックして Google アカウントでログインします。認可ページにコードが表示されます：",
  "settings.geminiOauth.step2":
    "codeassist.google.com/authcode ページに表示されたコードをここに貼り付けてください：",
  "settings.geminiOauth.warn":
    "⚠️ gemini-cli の公式 OAuth クライアントを流用しています。第三者利用はグレーゾーンで、個人利用のみ。無料枠は Google のポリシーに従い、制限されたりいつでも使えなくなる可能性があります。",
  "settings.geminiOauth.loggedIn": "✓ Gemini サブスクリプションにログイン済み",
  "settings.geminiOauth.loggedInAs": "✓ Gemini サブスクリプションにログイン済み（{email}）",
  "settings.geminiOauth.codePlaceholder":
    "コードを貼り付け（または codeassist.google.com/authcode?code=... の URL 全体）",
  "settings.modal.vision": "画像入力に対応（vision）",
  "settings.modal.visionHint":
    "このモデルが画像を「見られる」かどうか。モデル選択後に内蔵リストから自動チェックされ、手動でも変更できます。<b>オフにすると送信前に画像を自動的に取り除きます</b>（deepseek などのテキスト専用モデルは必ずオフに。さもないとスクリーンショットが 4xx で弾かれます）。",
  "settings.modal.apiKeyHint":
    "ローカルバックエンドにのみ保存され、アップロードされません。「Claude サブスクリプションを使用」にチェックした場合は空欄でも可。",
  "settings.modal.priceLabel": "価格（¥ / 百万トークン、任意）",
  "settings.modal.priceIn": "¥ 入力",
  "settings.modal.priceInPlaceholder": "入力価格",
  "settings.modal.priceOut": "¥ 出力",
  "settings.modal.priceOutPlaceholder": "出力価格",
  "settings.modal.priceCache": "¥ キャッシュ",
  "settings.modal.priceCachePlaceholder": "キャッシュ読み取り価格（空欄=入力価格×0.1）",
  "settings.modal.priceHint":
    "キャッシュ読み取り価格が空欄の場合は入力価格の 1/10 で見積もります。キャッシュが多い場面では正確に入力しないとコストが過大になります。",
  "settings.modal.maxtokLabel": "1 回の最大出力トークン（max_tokens、任意）",
  "settings.modal.maxtokPlaceholder": "空欄=グローバル既定 32768 を使用",
  "settings.modal.maxtokHint":
    "小さすぎると大きなファイル書き込みのツール引数が切り詰められてエラーになります。大きなファイル/長い出力では高めに。大きすぎるとモデルの実上限で拒否（400）されることがあるため、モデルの実際の出力上限に合わせて設定してください。",
  "settings.modal.effortLabel": "推論強度（thinking）",
  "settings.modal.effortDefault": "（グローバル既定に従う）",
  "settings.modal.effortOff": "オフ",
  "settings.modal.effortHint":
    "このモデルのみに適用され、グローバルを上書きします。プロバイダーごとに自動変換（Anthropic→thinking、OpenAI/Gemini→reasoning_effort、Qwen/Hunyuan→enable_thinking。DeepSeek など思考が組み込まれたモデルはパラメータを送らないため無効）。「オフ」=このモデルは思考しない。「グローバル既定に従う」=上のグローバル推論強度を使用。",
  "settings.modal.test": "接続テスト",
  "settings.modal.baseHintPreset": "プリセットエンドポイント（自動入力）",
  "settings.modal.baseHintCompat":
    "OpenAI 互換エンドポイント。自分で入力してください。例：http://localhost:8000/v1",
  "settings.modal.modelHintPreset":
    "プロバイダーのプリセット。使用するモデル ID を選択してください。",
  "settings.modal.modelHintCompat": "OpenAI 互換 API。モデル ID を手動で入力してください。",
  "settings.modal.needEndpoint": "Endpoint を入力してください",
  "settings.modal.needModel": "Model ID を選択 / 入力してください",
  "settings.modal.testing": "テスト中…",
  "settings.modal.testOk": "✓ 接続正常",
  "settings.modal.testFail": "✗ 失敗：{e}",

  // chat / sidebar / jobs / artifact / platform
  "chat.thinking": "✻ 思考 · {preview}",
  "chat.processing": "処理中…",
  "sidebar.currentTask": "現在のタスク",
  "sidebar.running": "実行中…",
  "sidebar.deleteTask": "タスクを削除",
  "sessions.deleteConfirm.title": "タスクを削除",
  "sessions.deleteConfirm.message": "タスク「{name}」を削除しますか？この操作は取り消せません。",
  "sidebar.renameTask": "タスク名を変更",
  "sidebar.section.tasks": "タスク",
  "sidebar.section.workspace": "ワークスペース",
  "sidebar.newSessionInDir": "このディレクトリで新しいセッションを作成",
  "jobs.secAgo": "{n}秒前",
  "jobs.minAgo": "{n}分前",
  "jobs.hourAgo": "{n}時間前",
  "jobs.dayAgo": "{n}日前",
  "jobs.sub":
    "AI が task_start で起動した長時間のバックグラウンドタスク（非同期で実行、確認・停止可能）",
  "jobs.empty": "バックグラウンドタスクはありません。AI が task_start を呼ぶとここに表示されます。",
  "jobs.running": "実行中",
  "jobs.completed": "完了",
  "artifact.tab.preview": "プレビュー",
  "artifact.tab.code": "コード",
  "common.refresh": "更新",
  "common.close": "閉じる",
  "artifact.readFailed": "読み込み失敗：{e}",
  "platform.cwdPrompt": "作業ディレクトリ（絶対パス、空欄=既定に戻す）：",

  // generic actions
  "common.edit": "編集",
  "common.delete": "削除",

  // scheduled tasks
  "tasks.desc": "スケジュールに従って prompt を実行し、結果をチャンネルに送信",
  "tasks.add": "＋ 新規タスク",
  "tasks.stat.running": "実行中",
  "tasks.stat.totalRuns": "累計実行",
  "tasks.stat.disabled": "無効",
  "tasks.stat.successRate": "前回の成功率",
  "tasks.status.ok": "成功",
  "tasks.status.notRun": "未実行",
  "tasks.status.maxIter": "未完了・ターン上限",
  "tasks.status.llmError": "モデル呼び出し失敗",
  "tasks.status.workdirNotFound": "作業ディレクトリがありません",
  "tasks.status.notifyErr": "通知失敗",
  "tasks.empty": "タスクがまだありません。「＋ 新規タスク」で作成してください。",
  "tasks.run": "▶ 実行",
  "tasks.runTitle": "予定を待たず、今すぐ 1 回実行して結果を確認",
  "tasks.logs": "ログ",
  "tasks.runsCount": "{n} 回実行",
  "tasks.lastDuration": "前回 {s}s",
  "tasks.modelTitle": "使用モデル",
  "tasks.runningBtn": "実行中…",
  "tasks.runFailed": "実行失敗：{e}",
  "tasks.logsTitle": "{name} · 実行ログ",
  "tasks.noLogs": "ログはまだありません",
  "tasks.interval.days": "{n} 日ごと",
  "tasks.interval.hours": "{n} 時間ごと",
  "tasks.interval.minutes": "{n} 分ごと",
  "tasks.interval.seconds": "{n} 秒ごと",
  "tasks.form.editTitle": "タスクを編集",
  "tasks.form.addTitle": "新規タスク",
  "tasks.form.name": "タスク名",
  "tasks.form.namePlaceholder": "例：「毎日のニュース要約」",
  "tasks.form.prompt": "タスク内容（prompt）",
  "tasks.form.promptPlaceholder": "エージェントに渡す指示",
  "tasks.form.schedule": "スケジュール方法",
  "tasks.form.byInterval": "間隔で",
  "tasks.form.byCron": "Cron 式",
  "tasks.form.cronPlaceholder": "0 30 8 * * *  （秒 分 時 日 月 曜日；5 フィールドも可）",
  "tasks.form.cronHint": "ローカルタイムゾーン。例：毎日 8:30 → <code>30 8 * * *</code>",
  "tasks.form.channel": "送信チャンネル",
  "tasks.form.model": "モデル（任意）",
  "tasks.form.modelHint":
    "空欄=グローバル既定モデルを使用。このタスク専用のモデルを指定できます（高頻度タスクには安価なモデルなど）。",
  "tasks.form.workdir": "作業ディレクトリ（任意）",
  "tasks.form.workdirPlaceholder":
    "空欄=グローバル作業ディレクトリ。絶対パスを入れるとこのタスクはそのディレクトリで隔離実行されます",
  "tasks.form.workdirHint":
    "指定するとこのタスクはそのディレクトリで実行され、その <code>&lt;dir&gt;/skills</code> プロジェクト級スキルを読み込みます。グローバルを汚しません。",
  "tasks.form.create": "作成",
  "tasks.form.noChannel": "（通知しない）",
  "tasks.form.defaultModel": "既定モデル",
  "tasks.form.defaultModelNamed": "既定モデル（{name}）",
  "tasks.form.required": "タスク名 / 内容 / スケジュールは必須です",

  // skills
  "skills.src.builtin": "組み込み",
  "skills.src.installed": "ローカル / Git",
  "skills.src.workdir": "プロジェクト",
  "skills.group.builtin": "組み込みスキル",
  "skills.group.workdir": "プロジェクトのスキル",
  "skills.group.installed": "インストール済みスキル",
  "skills.filter.all": "すべて",
  "skills.filter.builtin": "組み込み",
  "skills.filter.workdir": "プロジェクト",
  "skills.filter.installed": "インストール済み",
  "skills.searchMine": "名前や説明で検索…",
  "skills.clearSearch": "検索をクリア",
  "skills.empty.noMatch":
    "「{q}」に一致するスキルはありません。短いキーワードを試すか、分類を切り替えてください。",
  "skills.desc":
    "SKILL.md は書けばすぐ使えます。組み込みはプリインストール済み、マーケットでソースを切り替えてインストール、openclaw からの移行も可能",
  "skills.migrate": "openclaw から移行",
  "skills.import": "Git からインポート",
  "skills.create": "スキルを作成",
  "skills.tab.mine": "マイスキル",
  "skills.tab.market": "マーケット",
  "skills.source": "ソース",
  "skills.searchPlaceholder": "マーケットのスキルを検索…",
  "skills.toggle.on": "有効（クリックで無効化）",
  "skills.toggle.off": "無効（クリックで有効化）",
  "skills.noDesc": "（説明なし）",
  "skills.viewSkillMd": "SKILL.md を表示",
  "skills.uninstall": "アンインストール",
  "skills.status.enabling": "{name} を有効化中…",
  "skills.status.disabling": "{name} を無効化中…",
  "skills.status.uninstalling": "{name} をアンインストール中…",
  "skills.installed": "インストール済み",
  "skills.install": "インストール",
  "skills.status.installing": "{name} をインストール中…",
  "skills.status.installed": "{name} をインストールしました",
  "skills.status.installFailed": "インストール失敗：{e}",
  "skills.empty.mine":
    "スキルがまだありません。「マーケット」からインストール、「Git からインポート」、「openclaw から移行」、または「スキルを作成」してください。",
  "skills.empty.market":
    "このソースにインストール可能なスキルがありません（到達不可、または検索結果なし）。別のソースに切り替えるか検索をクリアしてください。",
  "skills.customSource": "カスタムソース…",
  "skills.loadingMarket": "マーケットを読み込み中…",
  "skills.marketLoadFailed": "マーケットの読み込みに失敗：{e}",
  "skills.switchingSource": "ソースを切り替え中…",
  "skills.customSourcePrompt": "カスタム registry ソース URL（静的 JSON）：",
  "skills.cannotRead": "（読み取れません）",
  "skills.import.title": "Git からスキルをインポート",
  "skills.import.urlLabel": "Git リポジトリ URL",
  "skills.import.subLabel": "サブディレクトリ（任意）",
  "skills.import.subPlaceholder": "既定：skills",
  "skills.import.note":
    "リポジトリ全体または単一スキルのインポートに対応。<code>@file</code> 参照は自動でインライン化されます。",
  "skills.import.ok": "インポート",
  "skills.import.needUrl": "Git の URL を入力してください",
  "skills.import.cloning": "クローンしてインポート中…",
  "skills.migrate.title": "openclaw からスキルを移行",
  "skills.migrate.note":
    "openclaw の一般的なスキルの場所（~/.config/openclaw、.agents/skills など）を自動検出しました。チェックして WiseCortex にインポートします。",
  "skills.migrate.scanning": "検出中…",
  "skills.migrate.importSelected": "選択をインポート",
  "skills.migrate.empty":
    "openclaw のスキルが見つかりません。「Git からインポート」をお試しください。",
  "skills.migrate.exists": "既に存在",
  "skills.migrate.needOne": "少なくとも 1 つ選択してください",
  "skills.migrate.importing": "インポート中…",
  "skills.migrate.done": "{n}/{total} 個のスキルを移行しました",
  "skills.creator.step.basic": "基本情報",
  "skills.creator.step.trigger": "トリガー",
  "skills.creator.step.body": "スキル本文",
  "skills.creator.step.tools": "ツール",
  "skills.creator.step.preview": "プレビューと保存",
  "skills.creator.prev": "戻る",
  "skills.creator.next": "次へ",
  "skills.creator.slugLabel": "スキル slug（invoke 用、kebab-case）",
  "skills.creator.descLabel": "一言の説明",
  "skills.creator.descPlaceholder": "git のコミットと issue を集約して週報を出力",
  "skills.creator.triggerLabel": "いつ使うか（トリガー、自然言語）",
  "skills.creator.triggerPlaceholder": "週報 / 業務まとめに言及したとき",
  "skills.creator.bodyLabel": "スキル本文（Markdown、@path でファイル参照可）",
  "skills.creator.bodyPlaceholder": "# 手順\n1. …",
  "skills.creator.toolsLabel": "このスキルが使うツール",
  "skills.creator.mdWhenUse": "## いつ使うか",
  "skills.creator.mdTools": "## 利用可能なツール",
  "skills.creator.previewLabel": "SKILL.md のプレビュー：",
  "skills.creator.save": "スキルを保存",
  "skills.creator.needSlug": "slug を入力してください",

  // channels
  "common.copy": "コピー",
  "channels.desc": "IM を接続して双方向チャット、または送信プッシュ先を設定",
  "channels.name": "名前",
  "channels.saveFailed": "保存に失敗",
  "channels.platform.feishu.name": "Feishu（飛書）",
  "channels.platform.feishu.desc": "イベント購読 · グループ/DM 双方向",
  "channels.platform.wecom.name": "WeCom（企業微信）",
  "channels.platform.wecom.desc": "暗号化コールバック · アプリメッセージ",
  "channels.platform.onebot.name": "QQ",
  "channels.platform.onebot.desc": "OneBot / NapCat · グループと DM",
  "channels.platform.email.name": "メール",
  "channels.platform.email.desc": "SMTP 送信 · 送信通知",
  "channels.platform.webhook.name": "汎用 Webhook",
  "channels.platform.webhook.desc": "送信のみ · 任意の HTTP エンドポイントへ",
  "channels.callback.feishuSummary": "公開デプロイ（上級）：イベント購読コールバック URL",
  "channels.callback.feishuNote":
    "サーバーに公開アドレスがある場合のみ。ローカルでは下の「ロング接続」（公開不要）を使ってください。",
  "channels.callback.wecomHint": "（公開 HTTPS が必要。WeCom アプリの「メッセージ受信」に入力）",
  "channels.callback.genericHint": "（各プラットフォームの管理画面に入力）",
  "channels.callback.label": "受信コールバック URL",
  "channels.appPushTarget": "アプリプッシュ → {target}",
  "channels.notConfigured": "未設定。",
  "channels.notConfigured.webhook": "送信 webhook の宛先を追加してください。",
  "channels.notConfigured.generic": "接続するとプッシュでき、双方向チャットにも対応します。",
  "channels.badge.configured": "設定済み",
  "channels.badge.notConnected": "未接続",
  "channels.feishu.scan": "QR で接続",
  "channels.feishu.lcOn": "ロング接続：オン（公開不要）",
  "channels.feishu.lcOff": "ロング接続：オフ",
  "channels.feishu.lcTitle":
    "ロング接続（WebSocket、公開コールバック不要）——メッセージはこちらで受信。トグルは即時反映、再起動不要",
  "channels.feishu.lcOnStatus": "ロング接続を有効化（数秒で自動接続）",
  "channels.feishu.lcOffStatus": "ロング接続を無効化（数秒で自動切断）",
  "channels.feishu.appPush": "アプリを送信に再利用",
  "channels.feishu.appPushTitle":
    "QR 接続済みの Feishu アプリ Bot を送信プッシュに利用（スケジュールタスクで選択可）。カスタム Bot を作る必要はありません",
  "channels.feishu.configOutbound": "送信プッシュを設定",
  "channels.wecom.credsSet": "受信資格情報：設定済み",
  "channels.wecom.credsConfig": "受信資格情報を設定",
  "channels.addTarget": "宛先を追加",
  "channels.connect": "{name} に接続",
  "channels.delete": "{name} を削除",
  "channels.copyOk": "コールバック URL をコピーしました",
  "channels.copyFail": "コピーに失敗。手動でテキストを選択してコピーしてください",
  "channels.faPush.recentHint":
    "下に Bot へ最近メッセージした会話が表示されます。1 つ選んでください。",
  "channels.faPush.noRecentHint":
    "最近の会話がまだありません——まず Feishu で Bot にメッセージを送り（グループで @ するか DM）、戻って更新してください。chat_id を直接貼り付けることもできます。",
  "channels.faPush.title": "Feishu アプリプッシュ（QR 接続を再利用）",
  "channels.faPush.namePlaceholder": "例：「開発グループ プッシュ」",
  "channels.faPush.chatLabel": "対象の chat_id",
  "channels.faPush.chatPlaceholder": "oc_…（グループ） / 最近の会話を選択",
  "channels.faPush.required": "名前と対象会話は必須です",
  "channels.config.smtpHost": "SMTP サーバー (host:port)",
  "channels.config.onebotBase": "OneBot HTTP ベース URL",
  "channels.config.smtpPlaceholder": "例：smtp.example.com:465",
  "channels.config.recipients": "宛先（カンマ区切り）",
  "channels.config.username": "ユーザー名",
  "channels.config.usernamePlaceholder": "SMTP ログイン、通常は送信メール",
  "channels.config.password": "パスワード / アプリパスワード",
  "channels.config.passwordPlaceholder": "SMTP パスワードまたはアプリパスワード",
  "channels.config.from": "差出人（空欄=ユーザー名）",
  "channels.config.emailNote":
    "ポート 465=暗黙 TLS、587=STARTTLS。資格情報はローカルバックエンドにのみ保存されます。",
  "channels.config.title": "{name} を設定",
  "channels.config.namePlaceholder": "例：「開発グループ」",
  "channels.config.groupLabel": "グループ番号（target）",
  "channels.config.groupPlaceholder": "グループ番号、任意",
  "channels.config.callbackNote":
    "双方向チャット：上のコールバック URL を {name} の管理画面に入力し、CLI でアプリ資格情報を設定します。",
  "channels.config.required": "名前とアドレスは必須です",
  "channels.scan.title": "Feishu を QR で接続",
  "channels.scan.generating": "QR コードを生成中…",
  "channels.scan.wait": "お待ちください…",
  "channels.scan.note":
    "<strong>Feishu / Lark App</strong> でスキャンして認可しアプリを作成します。成功すると app_id / app_secret が自動入力されます。<br/>スキャンは<strong>アプリ作成と資格情報の取得のみ</strong>。メッセージを受信するには、Feishu 開発者コンソールでさらに：① 権限管理に <code>im:message</code> を追加；② イベント購読で「ロング接続」を選び「メッセージ受信」を購読；③ バージョンを公開。その後このページで「ロング接続」を有効化しサーバーを再起動してください。",
  "channels.scan.failed": "QR フローを開始できません：{e}",
  "channels.scan.prompt": "Feishu / Lark App でスキャンして認可…",
  "channels.scan.connected": "接続しました！app_id={id}",
  "channels.scan.denied": "認可が拒否されました。閉じて再試行してください。",
  "channels.scan.expired": "QR コードが期限切れです。閉じて再スキャンしてください。",
  "channels.scan.error": "エラー：{e}",
  "channels.wecom.title": "WeCom · 受信資格情報",
  "channels.wecom.corpId": "企業 ID（corp_id）",
  "channels.wecom.secret": "アプリ Secret（corp_secret）",
  "channels.wecom.secretPlaceholder": "空欄=変更しない",
  "channels.wecom.agentId": "アプリ AgentId（agent_id）",
  "channels.wecom.agentIdPlaceholder": "例：1000002",
  "channels.wecom.token": "コールバック Token（callback_token）",
  "channels.wecom.tokenPlaceholder": "管理画面「メッセージ受信」内の Token",
  "channels.wecom.aesKey": "コールバック EncodingAESKey",
  "channels.wecom.aesPlaceholder": "43 桁、空欄=変更しない",
  "channels.wecom.note":
    'WeCom 管理画面 → アプリ → 「メッセージ受信」で API 受信を設定：URL に <span class="mono">{cb}</span>（公開 HTTPS で到達可能なこと）、Token / EncodingAESKey はここと一致させます。資格情報はローカルバックエンドにのみ保存されます。',
  "channels.wecom.savedReady": "WeCom 資格情報を保存しました（準備完了）",
  "channels.wecom.savedIncomplete": "WeCom 資格情報を保存しました（フィールド不足）",
  "channels.platform.qqbot.name": "QQ ボット（公式）",
  "channels.platform.qqbot.desc":
    "QQ オープンプラットフォーム · AppID/シークレット · ゲートウェイ接続、公開アドレス不要",
  "channels.qq.scan": "QR で連携（推奨）",
  "channels.qq.creds": "認証情報を手入力",
  "channels.qq.credsSet": "認証情報：設定済み",
  "channels.qq.connect": "ゲートウェイ：オフ · クリックで接続",
  "channels.qq.disconnect": "ゲートウェイ：オン · クリックで切断",
  "channels.qq.toggleTitle":
    "QQ ゲートウェイ（WebSocket、公開コールバック不要）——切り替えは即時反映、再起動不要",
  "channels.qq.onStatus": "QQ ゲートウェイを有効化（数秒で接続）",
  "channels.qq.offStatus": "QQ ゲートウェイを無効化",
  "channels.qq.appId": "AppID",
  "channels.qq.scanTitle": "QR で QQ ボットを連携",
  "channels.qq.scanNote":
    "<strong>スマホの QQ アプリ</strong>でスキャンし、連携するボットを選ぶと AppID/AppSecret が自動入力されます。サーバーアドレスは不要。連携ページは Tencent がホストし、接続元は既定で「サードパーティボット」と表示されます。",
  "channels.qq.scanPrompt": "スマホの QQ でスキャンして連携…",
  "channels.qq.scanConnected": "連携しました！AppID={id}",
  "channels.qq.title": "QQ ボットの認証情報",
  "channels.qq.appSecret": "AppSecret",
  "channels.qq.appSecretPlaceholder": "空欄＝現在の値を保持",
  "channels.qq.note":
    "q.qq.com でボットを作成し、設定ページから AppID/AppSecret をコピーします。サーバーアドレスは不要——WebSocket ゲートウェイで外向きに接続します。ゲートウェイが code 4914 で切断する場合、ボットにグループ/DM メッセージ権限がありません。",
  "channels.qq.savedReady": "保存しました（認証情報の準備完了、クリックで接続）",
  "channels.qq.savedIncomplete": "保存しました（認証情報が不完全）",
  "channels.platform.clawbot.name": "WeChat ClawBot",
  "channels.platform.clawbot.desc":
    "iLink ロングポーリング · 個人アカウントの DM、公開アドレス不要",
  "channels.clawbot.botId": "Bot ID",
  "channels.clawbot.scan": "QR で接続",
  "channels.clawbot.scanTitle": "WeChat ClawBot に接続",
  "channels.clawbot.scanNote":
    "<strong>スマホの WeChat</strong> でスキャンして承認してください。1 つの WeChat アカウントにつき Bot は 1 つだけで、本人と 1:1 で紐づきます。テキストと音声（サーバー側の文字起こし）に対応、画像とファイルは未対応です。",
  "channels.clawbot.scanPrompt": "WeChat でスキャンし、スマホで承認してください…",
  "channels.clawbot.scanConnected": "接続しました！{id}",
  "channels.clawbot.connect": "ポーリング：オフ · クリックで開始",
  "channels.clawbot.disconnect": "ポーリング：オン · クリックで停止",
  "channels.clawbot.toggleTitle":
    "iLink ロングポーリング（公開コールバック不要）——切り替えは即時反映、再起動不要",
  "channels.clawbot.onStatus": "WeChat のポーリングを開始しました",
  "channels.clawbot.offStatus": "WeChat のポーリングを停止しました",
  "channels.clawbot.soloNote":
    "<strong>1 か所だけ</strong>で有効にしてください。同期カーソルは Bot ごとに共有されるため、2 台で同時にポーリングするとメッセージがランダムに振り分けられます。",
  "tasks.form.chanGroupFeishu": "Feishu アプリ",
  "tasks.form.chanGroupQq": "QQ ボット",
  "tasks.form.feishuChatOpt": "Feishu · {id}",
  "tasks.form.qqC2cOpt": "QQ DM · {id}",
  "tasks.form.qqGroupOpt": "QQ グループ · {id}",
} satisfies Record<MessageKey, string>;
