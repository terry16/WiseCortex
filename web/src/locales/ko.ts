import type { MessageKey } from "./en";

// 한국어
export const ko = {
  // generic
  "common.done": "완료",
  "common.add": "추가",
  "common.remove": "제거",
  "common.itemCount": "{n}개",

  // nav / shell
  "brand.sub": "셀프 호스팅 AI 에이전트",
  "nav.newTask": "새 작업",
  "nav.chat": "채팅",
  "nav.tasks": "예약 작업",
  "nav.jobs": "백그라운드",
  "nav.skills": "스킬",
  "nav.channels": "알림 채널",
  "nav.settings": "설정",

  // theme
  "theme.toggle": "테마 전환(라이트 / 다크)",

  // footer (rail)
  "foot.modelLabel": "현재 모델",
  "foot.costLabel": "오늘 비용",
  "foot.model.unset": "미설정",
  "foot.access.locked": "access_key 잠김",
  "foot.access.public": "공개 접근",
  "foot.versionTitle": "서버 버전",

  // offline
  "offline.banner": "연결이 끊겼습니다 — 다시 연결 중…",

  // login gate
  "login.sub": "이 인스턴스는 보호되어 있습니다. 접근 키를 입력하세요.",
  "login.placeholder": "접근 키",
  "login.enter": "들어가기",
  "login.err.wrong": "키가 올바르지 않습니다. 다시 시도하세요",
  "login.err.unreachable": "서버에 연결할 수 없습니다. 잠시 후 다시 시도하세요",
  "auth.prompt": "이 WiseCortex에는 접근 키가 필요합니다. 입력하세요:",

  // attachments
  "attach.removeHint": "클릭하여 제거",

  // composer
  "composer.placeholder": "WiseCortex에 작업 지시…  (Enter로 전송, Shift+Enter로 줄바꿈)",
  "composer.more": "작업 펼치기",
  "composer.stop": "중지",
  "composer.cwd.label": "작업 디렉터리",
  "composer.cwd.defaultDir": "기본 디렉터리",
  "composer.cwd.task": "작업 디렉터리: {dir}",
  "composer.cwd.locked": " (작업 시작됨, 잠김)",
  "composer.cwd.global": "작업 디렉터리(전역): {dir}",
  "composer.cwd.pickPrompt": "작업 디렉터리 (비우면 전역 워크스페이스 사용):",
  "composer.model.title": "이 작업의 모델 (기본값은 전역을 따름)",
  "composer.effort.title": "이 채팅의 사고 강도 (모델이 미지원이면 무시)",
  "composer.effort.opt.default": "사고: 기본",
  "composer.effort.opt.off": "사고: 끔",
  "composer.effort.opt.low": "사고: low",
  "composer.effort.opt.medium": "사고: medium",
  "composer.effort.opt.high": "사고: high",
  "composer.effort.opt.xhigh": "사고: xhigh",
  "composer.effort.opt.max": "사고: max",
  "composer.skills.pinned": "이 작업의 스킬 ({n}): {list}",
  "composer.skills.empty": "이 작업의 스킬 (기본값 자동 선택, 클릭하여 고정)",
  "composer.perm.title": "도구 작업 권한",
  "composer.perm.default": "기본 권한",
  "composer.perm.solo": "solo (자동 승인)",
  "composer.perm.strict": "엄격 (단계마다 확인)",
  "composer.plan.on":
    "계획 모드: 켬 (읽기 전용 탐색 + 계획, 변경 금지). 승인 후 여기를 꺼서 끄고 “실행”이라고 말하세요.",
  "composer.plan.off": "계획 모드: 읽기 전용 탐색 + 계획, 변경 금지. 클릭하여 켜기.",
  "composer.kb.pinned": "이 채팅의 지식 베이스 ({n}):\n{list}",
  "composer.kb.empty": "지식 베이스 (이 채팅, 클릭하여 폴더/파일 추가)",
  "composer.mem.title": "메모리(AI가 기억한 내용 보기/편집)",

  // skills picker
  "skillsPicker.title": "이 작업의 스킬",
  "skillsPicker.note":
    "0개 고정 = AI가 전체 스킬에서 자동 선택, 일부 고정 = 이 작업은 해당 스킬만 사용 (더 안정적, 오선택 없음).",
  "skillsPicker.empty": "사용 가능한 스킬이 없습니다.",
  "skillsPicker.projectBadge": "프로젝트",
  "skillsPicker.projectBadge.title": "이 작업의 작업 디렉터리 skills/에서",

  // knowledge base
  "kb.title": "지식 베이스 (이 채팅)",
  "mem.title": "메모리",
  "mem.note":
    "AI가 remember 도구로 직접 기록하며 매 턴 주입됩니다. 여기서 실제로 무엇을 기억하는지 확인하고 잘못된 것을 고칠 수 있습니다. <b>Lessons</b> 항목은 반복하면 안 되는 실수로, AI가 우선 따르도록 지시받으며 용량 초과 시에도 가장 마지막에 삭제됩니다.",
  "mem.project": "프로젝트 메모리 — {dir}",
  "mem.projectHint":
    "이 작업 디렉터리의 <b>모든 세션이 공유</b>하며 새 대화에도 이어집니다. 항상 지켰으면 하는 규칙을 여기에 적으세요.",
  "mem.session": "이 대화 전용",
  "mem.sessionHint": "현재 대화에만 속하며 대화를 삭제하면 함께 삭제됩니다.",
  "mem.empty": "(비어 있음)",
  "kb.note":
    "이 채팅의 지식 베이스로 폴더나 파일을 마운트합니다. 질문하면 AI가 knowledge_search로 검색한 뒤 답변합니다 (로컬 전용, 이 채팅에서만 유효).",
  "kb.addLabel": "경로 추가 (폴더 또는 파일, 절대 경로)",
  "kb.input.placeholder": "예: D:\\kb 또는 ~/wisecortex/workspace/kb/faq.md",
  "kb.pick.title": "폴더 선택",
  "kb.empty": "아직 마운트된 지식 베이스 경로가 없습니다.",
  "kb.pickPrompt": "지식 베이스 폴더 절대 경로:",

  // chat lifecycle (used by ws-dispatcher)
  "chat.done": "완료 ({n}턴, {cost})",
  "chat.done.duration": " · {duration}s",
  "chat.done.cache": "캐시 적중 {rate}% ({hits}/{total}, {tokens})",
  "chat.interrupted": "중단됨",
  "chat.queued": "⏳ 대기열에 추가됨 — 현재 턴이 끝나면 자동 처리됩니다",
  "chat.retry": "재시도",
  "chat.retrying": "재시도 중…",
  "error.insufficient_credit": "잔액 부족",
  "error.insufficient_credit.action": "충전",

  // artifacts
  "artifact.rendered": "생성됨 · 클릭하면 오른쪽에서 미리보기",
  "artifact.source": "생성됨 · 클릭하면 소스 보기",

  // hero (empty state)
  "greet.night": "늦은 시간이네요",
  "greet.morning": "좋은 아침이에요",
  "greet.afternoon": "안녕하세요",
  "greet.evening": "좋은 저녁이에요",
  "hero.title": "{greet}, 무엇을 도와드릴까요?",
  "hero.sub":
    "WiseCortex는 코드 작성, 스크립트 실행, 온라인 조사, 스킬 호출을 하고 결과를 메신저로 보낼 수 있습니다.",
  "suggest.landing.title": "제품 랜딩 페이지 만들기",
  "suggest.landing.sub": "개요로 반응형 단일 페이지 생성",
  "suggest.research.title": "주제를 온라인으로 조사",
  "suggest.research.sub": "자료를 모아 핵심 정리",
  "suggest.debug.title": "버그를 찾도록 도와줘",
  "suggest.debug.sub": "근본 원인을 찾고 수정",
  "suggest.chart.title": "데이터를 차트로",
  "suggest.chart.sub": "CSV / 표 → 시각화",

  // settings — language
  "settings.language.title": "인터페이스 언어",
  "settings.language.sub": "전환하면 즉시 적용됩니다 (페이지가 새로고침됩니다).",
  "settings.language.label": "언어",

  // settings — generic
  "common.cancel": "취소",
  "common.save": "저장",
  "settings.pick": "선택…",
  "settings.keySetPlaceholder": "설정됨 (비우면 = 변경 안 함)",

  // settings — model access
  "settings.models.title": "모델 연결",
  "settings.models.sub": "BYOK로 자체 API 키 사용; 모델별 가격을 설정해 비용 정산에 활용",
  "settings.models.add": "모델 추가",
  "settings.models.default": "기본 모델",
  "settings.models.empty": "아직 모델이 없습니다. “모델 추가”를 눌러 설정하세요.",
  "settings.models.badge.default": "기본",
  "settings.models.badge.vision": "비전",
  "settings.models.badge.visionTitle": "이미지 입력 지원",
  "settings.models.priceIn": "입력",
  "settings.models.priceOut": "출력",
  "settings.models.priceUnit": "¥ / 백만 토큰",
  "settings.models.keySet": "키 설정됨",
  "settings.models.keyUnset": "미설정",
  "settings.models.subscription": "구독 할당량",
  "settings.models.subscriptionTitle":
    "이 모델은 구독 로그인(OAuth) 할당량을 사용하므로 API 키가 필요 없습니다",
  "settings.models.configure": "설정",
  "settings.models.delete": "삭제",
  "settings.models.optionLabel": "{model} ({provider})",

  // settings — workspace
  "settings.workspace.title": "작업 디렉터리",
  "settings.workspace.sub":
    "AI 생성 스크립트 / 지식 베이스 / 자가 학습의 전역 거점. 각 채팅에서 입력창으로 임시 디렉터리를 따로 지정할 수 있습니다",
  "settings.workspace.label": "전역 작업 디렉터리",
  "settings.workspace.placeholder": "비우면 = 시스템 기본값 사용",
  "settings.workspace.hintTauri":
    "“선택…”으로 시스템 대화상자에서 디렉터리를 선택하세요. 경로를 직접 편집할 수도 있습니다.",
  "settings.workspace.hintWeb": "WebUI: 서버의 절대 경로를 입력하세요.",
  "settings.workspace.pickPrompt": "전역 작업 디렉터리 (비우면 = 시스템 기본값):",

  // settings — proxy
  "settings.proxy.title": "네트워크 프록시",
  "settings.proxy.sub":
    "설정하면 모든 아웃바운드 접근(LLM, 스킬/마켓, web 도구)이 프록시를 경유합니다. 비우면 = 직접 연결",
  "settings.proxy.label": "프록시 주소",
  "settings.proxy.placeholder": "http://host:port 또는 socks5://host:port",
  "settings.proxy.hint":
    "http/https/socks5 지원. 변경 즉시 적용(자동 핫 리로드). 환경 변수 WC_PROXY로도 재정의할 수 있습니다.",

  // settings — Claude subscription OAuth
  "settings.claudeOauth.title": "Claude 구독 로그인",
  "settings.claudeOauth.sub":
    "Claude 구독(Pro/Max) 한도로 로컬에서 추론을 실행해 모델을 나란히 비교합니다. 로그인 후 「모델 관리」에서 「Claude 구독」을 체크한 모델을 추가하면 사용할 수 있습니다.",
  "settings.claudeOauth.step1":
    "브라우저가 자동으로 열리지 않으면(데스크톱 셸에서 흔함) “열기” 또는 “링크 복사”로 인증 페이지를 수동으로 여세요:",
  "settings.claudeOauth.step2":
    "로그인 후 페이지에 표시된 code를 복사해 아래에 붙여 넣어 완료하세요:",
  "settings.claudeOauth.codePlaceholder": "인증 code 붙여넣기 (code#state 형식)",
  "settings.claudeOauth.warn":
    "⚠️ Claude Code의 OAuth 클라이언트를 재사용합니다. 제3자 사용은 회색 지대이며 개인 로컬 사용 전용입니다. 공식 정책으로 제한되거나 언제든 중단될 수 있습니다.",
  "settings.claudeOauth.loggedIn": "✓ Claude 구독에 로그인됨",

  // settings — ChatGPT subscription OAuth
  "settings.chatgptOauth.title": "ChatGPT 구독 로그인",
  "settings.chatgptOauth.sub":
    "ChatGPT(Plus/Pro) 구독 한도로 Codex 모델을 로컬에서 실행해 비교합니다. 로그인 후 「모델 관리」에서 「ChatGPT 구독」을 체크한 모델을 추가하면 사용할 수 있습니다.",
  "settings.chatgptOauth.step1":
    "“열기” 또는 “링크 복사”로 브라우저에서 인증 페이지를 열어 로그인하세요:",
  "settings.chatgptOauth.step2":
    "인증 후 브라우저가 <code>localhost:1455</code>로 리디렉션됩니다(대개 열리지 않으며 정상입니다). <b>주소창의 전체 URL</b> 또는 그 안의 code를 아래에 붙여 넣으세요:",
  "settings.chatgptOauth.codePlaceholder":
    "code 또는 localhost:1455/auth/callback?code=... 주소 전체 붙여넣기",
  "settings.chatgptOauth.warn":
    "⚠️ Codex의 OAuth 클라이언트를 재사용합니다. 제3자 사용은 회색 지대이며 개인 로컬 사용 전용입니다. 공식 정책으로 제한되거나 언제든 중단될 수 있습니다. 프록시가 필요합니다.",
  "settings.chatgptOauth.loggedIn": "✓ ChatGPT 구독에 로그인됨",
  "settings.grokOauth.title": "Grok 구독 로그인",
  "settings.grokOauth.sub":
    "X Premium / SuperGrok 구독 한도로 Grok 모델을 실행합니다(디바이스 코드 로그인, 로컬/서버 배포 모두 가능). 로그인 후 「모델 관리」에서 「Grok 구독」을 체크한 모델을 추가하세요.",
  "settings.grokOauth.step1": "「열기」를 눌러 아무 기기의 브라우저에서 인증 페이지를 여세요:",
  "settings.grokOauth.step2":
    "인증 페이지에 아래 코드를 입력하면 완료 후 이 페이지가 자동으로 로그인됩니다:",
  "settings.grokOauth.warn":
    "⚠️ xAI 공식 Grok-CLI OAuth 클라이언트를 재사용합니다. 제3자 사용은 회색 지대이며 개인 사용 전용입니다. 공식 정책으로 제한되거나 언제든 중단될 수 있습니다.",
  "settings.grokOauth.loggedIn": "✓ Grok 구독에 로그인됨",
  "settings.grokOauth.waiting": "인증 대기 중… 브라우저에서 완료하면 이 페이지가 자동 갱신됩니다",
  "settings.grokOauth.expired": "디바이스 코드가 만료되었습니다. 「로그인」을 다시 누르세요",

  // settings — OAuth shared
  "settings.oauth.checking": "확인 중…",
  "settings.oauth.login": "로그인",
  "settings.oauth.logout": "로그아웃",
  "settings.oauth.open": "열기",
  "settings.oauth.copyLink": "링크 복사",
  "settings.oauth.notLoggedIn": "로그인 안 됨",
  "settings.oauth.relogin": "다시 로그인",
  "settings.oauth.unknown": "상태 알 수 없음",
  "settings.oauth.openedCode": "인증 페이지를 열었습니다. 로그인 후 code를 붙여 넣으세요",
  "settings.oauth.openedUrl": "인증 페이지를 열었습니다. 로그인 후 주소창 URL을 붙여 넣으세요",
  "settings.oauth.manualOpen": "“열기” 또는 “링크 복사”로 인증 페이지를 수동으로 여세요",
  "settings.oauth.waitingBrowser":
    "브라우저에서 인증 페이지를 열었습니다. 로그인을 완료하면 자동으로 감지합니다…",
  "settings.oauth.loopbackCopied":
    "인증 링크를 복사했습니다. 브라우저에서 열어 로그인을 완료하세요. 자동으로 감지합니다…",
  "settings.oauth.loopbackTimeout": "인증 대기 시간이 초과되었습니다. 다시 로그인하세요",
  "settings.oauth.loginFailed": "로그인 실패: {e}",
  "settings.oauth.copied": "링크를 복사했습니다. 브라우저에서 여세요",
  "settings.oauth.redeeming": "교환 중…",
  "settings.oauth.failed": "실패: {e}",
  "settings.oauth.unknownError": "알 수 없는 오류",

  // settings — iteration limits
  "settings.maxiter.title": "도구 호출 턴 상한",
  "settings.maxiter.sub":
    "작업당 연속 도구 호출 최대 턴 수(폭주 방지). 대화 채팅과 무인 작업을 따로 설정합니다.",
  "settings.maxiter.interactive": "대화 채팅 상한",
  "settings.maxiter.interactivePlaceholder": "비우면 = 기본값 1000",
  "settings.maxiter.interactiveHint":
    "데스크톱/웹에서 직접 진행하는 채팅에 적용 — 높게 설정. 개발에서는 거의 도달하지 않고 언제든 수동으로 멈출 수 있습니다. **백그라운드 장시간 작업(task_start)도 이 한도를 사용합니다.** 환경 변수 WC_MAX_ITERATIONS_INTERACTIVE가 우선합니다.",
  "settings.maxiter.unattended": "무인 작업 상한",
  "settings.maxiter.unattendedPlaceholder": "비우면 = 기본값 50",
  "settings.maxiter.unattendedHint":
    "예약 작업 / 페이슈·위컴 트리거 작업에 적용(무인, 폭주 비용 방지). 다중 파일 생성 + 배포 같은 긴 작업은 높여도 됩니다. 환경 변수 WC_MAX_ITERATIONS가 우선합니다.",
  "settings.maxiter.subagent": "하위 작업(하위 agent) 반복 상한",
  "settings.maxiter.subagentPlaceholder": "비우면 = 기본값 100",
  "settings.maxiter.subagentHint":
    "채팅의 <b>task</b> 도구가 파생하는 하위 agent에 적용됩니다. 이전에는 15로 하드코딩되어 너무 낮았습니다 — 변경 사항 검토나 로그 분석은 파일 몇 개만 읽어도 다 써버려서 하위 작업이 거의 항상 '반복 상한 도달'로 끝났습니다. 메인 세션과 별도 항목인 이유는 하위 작업이 여러 개 병렬로 도는 경우가 많아 비용이 곱해지기 때문입니다. 환경 변수 WC_SUBAGENT_MAX_ITERATIONS가 우선합니다.",

  // settings — log cleanup
  "settings.logclean.title": "예약 작업 로그 정리",
  "settings.logclean.sub":
    "예약 작업 실행 로그는 크기 기준으로 정리합니다. 상한을 넘은 파일만 오래된 '실행 1회' 단위로 잘라내며, 상한 이내 로그는 건드리지 않습니다. 매시간 점검합니다.",
  "settings.logclean.autoLabel": "로그 자동 정리",
  "settings.logclean.autoSub": "끄면 로그를 영구 보존하며 직접 정리합니다.",
  "settings.logclean.maxLabel": "작업당 로그 상한(MB)",
  "settings.logclean.placeholder": "비우면 = 기본값 10",
  "settings.logclean.hint":
    "상한을 넘으면 오래된 <b>실행 단위</b>로 잘라내고 최근 1회는 항상 남깁니다 — '여러 번 실행했는데 로그가 비어 있음'이 생기지 않습니다. 변경 즉시 적용되며 재시작이 필요 없습니다.",

  // settings — web search
  "settings.websearch.title": "웹 검색",
  "settings.websearch.sub":
    "web_search 도구의 제공자. API 키를 입력하거나 SearXNG를 셀프 호스트하세요. 비우면 = DuckDuckGo(키 불필요)",
  "settings.websearch.provider": "제공자",
  "settings.websearch.ddg": "DuckDuckGo(기본, 키 불필요)",
  "settings.websearch.searxng": "SearXNG(셀프 호스트)",
  "settings.websearch.brave": "Brave Search(API 키)",
  "settings.websearch.tavily": "Tavily(API 키)",
  "settings.websearch.searxngUrl": "SearXNG 주소",

  // settings — reasoning effort
  "settings.effort.title": "추론 강도(extended thinking)",
  "settings.effort.sub":
    "답하기 전에 모델이 더 생각하게 합니다 — 복잡한 작업/까다로운 버그에서 더 안정적이지만 시간과 비용이 늘어납니다. 제공자별 자동 변환: Anthropic은 thinking, OpenAI/Gemini는 reasoning_effort, Qwen/Hunyuan은 enable_thinking; DeepSeek 등 사고가 내장된 모델은 매개변수를 보내지 않습니다.",
  "settings.effort.label": "강도",
  "settings.effort.off": "끔(기본)",
  "settings.effort.xhigh": "xhigh(코딩 최적점)",
  "settings.effort.hint":
    "변경 즉시 적용(자동 핫 리로드). 환경 변수 WC_REASONING_EFFORT로도 재정의할 수 있습니다.",

  // settings — access control
  "settings.access.title": "접근 제어",
  "settings.access.sub": "로컬 WS/REST를 보호하고 위험한 작업을 제어합니다",
  "settings.access.enableKey": "access_key 활성화",
  "settings.access.enableKeySub":
    "켜면 모든 연결이 키를 지녀야 합니다(변경 후 서버 재시작 시 적용). <b>외부 접근에는 필수</b> — 서버는 기본적으로 로컬에만 바인딩하므로 외부 공개에는 nginx 리버스 프록시를 사용하세요.",
  "settings.access.keyPlaceholder": "접근 키 설정",
  "settings.access.envManaged":
    "환경 변수 WC_ACCESS_KEY로 설정됨 —— 서버에서 변경하세요(여기서는 편집할 수 없습니다).",
  "settings.access.confirm": "위험한 작업 전 확인",
  "settings.access.confirmSub":
    "켜면 파일 쓰기 / shell 등 작업 전에 동의를 구합니다(끄면 = 무인 전자동)",
  "settings.access.automem": "자동 메모리",
  "settings.access.automemSub":
    "켜면 긴 대화에서 몇 턴마다 백그라운드로 대화에서 요점을 추출해 세션 메모리에 기록합니다(추가 LLM 호출 발생 — 필요에 따라 켜기; 변경 후 서버 재시작 시 적용)",
  "settings.access.autotrim": "컨텍스트 자동 최적화",
  "settings.access.autotrimSub":
    "이미지는 한 번만 전송하고 이후 턴에서는 생략하며, 압축 시에도 버립니다",
  "settings.access.autotrimHelp":
    "이미지는 컨텍스트에서 가장 비싼 요소입니다 — 스크린샷 한 장이 쉽게 1000 토큰이며, 기본값에서는 매 턴마다 그대로 다시 전송됩니다. 켜면 이미지는 보낸 턴에만 컨텍스트에 포함되고 이후에는 한 줄 자리표시자로 대체되며, 기록 압축 시에도 버려집니다. UI 수정이나 테스트 실행에서는 한 번 보면 충분합니다. 같은 이미지를 반복해서 비교해야 할 때만 끄세요.",

  // settings — MCP
  "settings.mcp.title": "MCP 서버",
  "settings.mcp.sub":
    "MCP 서버(stdio 또는 Streamable HTTP)에 연결합니다. 그 tools/resources/prompts는 <code>mcp__server__tool</code>로 AI에 노출됩니다. 변경 즉시 재연결",
  "settings.mcp.label": "서버 설정(JSON: 이름 → stdio {command,args,env} 또는 HTTP {url,headers})",
  "settings.mcp.hint":
    "resources/prompts는 <code>list_resources</code>/<code>read_resource</code>/<code>list_prompts</code>/<code>get_prompt</code> 도구를 자동 합성합니다. sampling/roots 역방향 요청은 자동 처리됩니다(stdio만).",
  "settings.mcp.save": "저장하고 재연결",
  "settings.mcp.discovered": "발견된 도구",
  "settings.mcp.noTools": "(아직 연결된 도구 없음)",
  "settings.mcp.savedReconnecting": "저장했습니다. 백그라운드에서 재연결 중…",

  // settings — hooks
  "settings.hooks.title": "이벤트 훅(hooks)",
  "settings.hooks.sub":
    "이벤트 시점에 명령을 실행해 가드레일/부수 효과를 적용합니다: PreToolUse는 도구를 가로채고, PostToolUse는 컨텍스트를 덧붙이며, SessionStart/Stop/UserPromptSubmit",
  "settings.hooks.label":
    "훅 설정(JSON: 이벤트 → [{matcher, command, timeout_ms}]; matcher는 도구명 정규식, Pre/PostToolUse에서만 사용)",
  "settings.hooks.save": "저장",
  "settings.hooks.hint":
    '명령은 stdin으로 이벤트 JSON을 받습니다. 차단: JSON <code>{"decision":"block","reason":"…"}</code> 또는 0이 아닌 종료 코드(이유는 stderr에서); 컨텍스트 추가: <code>{"additionalContext":"…"}</code>.',

  // settings — statuses
  "settings.status.loading": "불러오는 중…",
  "settings.status.loadFailed": "불러오기 실패: {e}",
  "settings.status.workspaceUpdated": "작업 디렉터리를 업데이트했습니다",
  "settings.status.workspaceReset": "기본 작업 디렉터리로 되돌렸습니다",
  "settings.status.websearchUpdated": "웹 검색을 업데이트했습니다",
  "settings.status.jsonParseFailed": "JSON 파싱 실패: {e}",
  "settings.status.saving": "저장 중…",
  "settings.status.saved": "저장됨",
  "settings.status.effortUpdated": "추론 강도를 업데이트했습니다",
  "settings.status.proxySet": "프록시 설정됨(핫 리로드됨)",
  "settings.status.proxyOff": "프록시 끔(직접 연결)",
  "settings.status.maxiterSet": "무인 작업 상한을 {v}로 설정했습니다(핫 리로드됨)",
  "settings.status.maxiterReset": "기본 상한 50으로 되돌렸습니다(핫 리로드됨)",
  "settings.status.maxiterInteractiveSet": "대화 채팅 상한을 {v}로 설정했습니다(핫 리로드됨)",
  "settings.status.maxiterInteractiveReset": "기본 상한 1000으로 되돌렸습니다(핫 리로드됨)",
  "settings.status.maxiterSubagentSet": "하위 작업 상한을 {v}(으)로 설정했습니다(핫 리로드됨)",
  "settings.status.maxiterSubagentReset": "하위 작업 기본 상한 100으로 되돌렸습니다(핫 리로드됨)",
  "settings.status.logcleanDefault": "기본 상한 10MB",
  "settings.status.logcleanOn": "로그 자동 정리를 켰습니다",
  "settings.status.logcleanOff": "로그 자동 정리를 껐습니다(영구 보존)",
  "settings.status.logcleanMb": "상한 {v}MB",
  "settings.status.logcleanUpdated": "로그 정리를 업데이트했습니다: {msg}",
  "settings.status.accessKeyOff": "접근 키를 비활성화했습니다(서버 재시작 시 적용)",
  "settings.status.accessKeySet": "접근 키를 설정했습니다(서버 재시작 시 적용)",
  "settings.status.updated": "업데이트됨",
  "settings.status.automemOn": "자동 메모리를 켰습니다(서버 재시작 시 적용)",
  "settings.status.autotrimOn": "컨텍스트 자동 최적화 켬(이미지 1회만 전송)",
  "settings.status.autotrimOff": "컨텍스트 자동 최적화 끔(이미지 매 턴 재전송)",
  "settings.status.automemOff": "자동 메모리를 껐습니다(서버 재시작 시 적용)",

  // settings — model modal
  "settings.modal.editTitle": "모델 설정",
  "settings.modal.addTitle": "모델 추가",
  "settings.modal.provider": "제공자",
  "settings.modal.endpoint": "API 엔드포인트",
  "settings.modal.useClaudeOauth": "Claude 구독 사용(API 키 불필요)",
  "settings.modal.useClaudeOauthHint":
    "체크하면 이 모델은 로컬에 로그인된 Claude 구독 한도를 사용합니다(고정 Anthropic 공식 엔드포인트). 먼저 「Claude 구독 로그인」에서 로그인하세요. 위의 「모델」을 Claude 모델 id(예: claude-sonnet-4-6)로 설정하세요.",
  "settings.modal.useChatgptOauth": "ChatGPT 구독 사용(API 키 불필요)",
  "settings.modal.useChatgptOauthHint":
    "체크하면 이 모델은 로컬에 로그인된 ChatGPT 구독 한도를 사용합니다(고정 Codex 백엔드 + Responses 전송). 먼저 「ChatGPT 구독 로그인」에서 로그인하세요. 위의 「모델」을 Codex 모델 id(예: gpt-5-codex)로 설정하세요.",
  "settings.modal.useGrokOauth": "Grok 구독 사용(API 키 불필요)",
  "settings.modal.useGrokOauthHint":
    "체크하면 이 모델은 로그인된 Grok 구독 한도를 사용합니다(api.x.ai, OpenAI 호환 전송). 먼저 「Grok 구독 로그인」에서 로그인하세요. 모델 예: grok-code-fast-1 / grok-4-1.",
  "settings.modal.useGeminiOauth": "Gemini 구독 사용 (API 키 불필요)",
  "settings.modal.useGeminiOauthHint":
    "체크하면 이 모델은 로그인된 Gemini 구독 한도를 사용합니다(Code Assist 백엔드, 네이티브 Gemini 전송). 먼저 「Gemini 구독 로그인」에서 로그인하세요. 모델 예: gemini-2.5-pro / gemini-2.5-flash.",
  "settings.geminiOauth.title": "Gemini 구독 로그인",
  "settings.geminiOauth.sub":
    "개인 Google 계정(Gemini Code Assist 무료/유료 한도)으로 Gemini 모델을 실행합니다. 로그인 후 「모델 관리」에서 「Gemini 구독」을 체크한 모델을 추가하세요.",
  "settings.geminiOauth.step1":
    "「열기」를 클릭해 Google 계정으로 로그인하세요. 인증 페이지에 코드가 표시됩니다:",
  "settings.geminiOauth.step2":
    "codeassist.google.com/authcode 페이지에 표시된 코드를 여기에 붙여넣으세요:",
  "settings.geminiOauth.warn":
    "⚠️ gemini-cli의 공식 OAuth 클라이언트를 재사용합니다. 제3자 사용은 회색지대이며 개인용으로만 사용하세요. 무료 한도는 Google 정책에 따라 제한되거나 언제든 중단될 수 있습니다.",
  "settings.geminiOauth.loggedIn": "✓ Gemini 구독에 로그인됨",
  "settings.geminiOauth.loggedInAs": "✓ Gemini 구독에 로그인됨 ({email})",
  "settings.geminiOauth.codePlaceholder":
    "코드 붙여넣기 (또는 codeassist.google.com/authcode?code=... 전체 URL)",
  "settings.modal.vision": "이미지 입력 지원(vision)",
  "settings.modal.visionHint":
    "이 모델이 이미지를 “볼” 수 있는지 여부. 모델 선택 후 내장 목록에 따라 자동 체크되며 수동으로 바꿀 수 있습니다. <b>끄면 전송 전에 이미지를 자동으로 제거합니다</b>(deepseek 등 텍스트 전용 모델은 반드시 꺼야 하며, 그렇지 않으면 스크린샷이 4xx로 반려됩니다).",
  "settings.modal.apiKeyHint":
    "로컬 백엔드에만 저장되며 업로드되지 않습니다. 「Claude 구독 사용」을 체크하면 비워도 됩니다.",
  "settings.modal.priceLabel": "가격(¥ / 백만 토큰, 선택)",
  "settings.modal.priceIn": "¥ 입력",
  "settings.modal.priceInPlaceholder": "입력 가격",
  "settings.modal.priceOut": "¥ 출력",
  "settings.modal.priceOutPlaceholder": "출력 가격",
  "settings.modal.priceCache": "¥ 캐시",
  "settings.modal.priceCachePlaceholder": "캐시 읽기 가격(비우면 = 입력가×0.1)",
  "settings.modal.priceHint":
    "캐시 읽기 가격을 비우면 입력 가격의 1/10로 추정합니다. 캐시가 많은 경우 정확히 입력하지 않으면 비용이 과대 계상됩니다.",
  "settings.modal.maxtokLabel": "호출당 최대 출력 토큰(max_tokens, 선택)",
  "settings.modal.maxtokPlaceholder": "비우면 = 전역 기본값 32768 사용",
  "settings.modal.maxtokHint":
    "너무 작으면 큰 파일 쓰기 도구 인자가 잘려 오류가 납니다. 큰 파일/긴 출력에는 높이세요. 너무 크면 모델 실제 상한에 의해 거부될 수 있습니다(400). 모델의 실제 출력 상한에 맞춰 설정하세요.",
  "settings.modal.effortLabel": "추론 강도(thinking)",
  "settings.modal.effortDefault": "(전역 기본값 따름)",
  "settings.modal.effortOff": "끔",
  "settings.modal.effortHint":
    "이 모델에만 적용되어 전역을 재정의합니다. 제공자별 자동 변환(Anthropic→thinking, OpenAI/Gemini→reasoning_effort, Qwen/Hunyuan→enable_thinking; DeepSeek 등 사고 내장 모델은 매개변수를 보내지 않아 무효). “끔” = 이 모델은 사고하지 않음; “전역 기본값 따름” = 위의 전역 추론 강도 사용.",
  "settings.modal.test": "연결 테스트",
  "settings.modal.baseHintPreset":
    "프리셋 엔드포인트가 입력됨 — 프록시나 자체 게이트웨이 주소로 바꿀 수 있습니다",
  "settings.modal.baseHintCompat":
    "OpenAI 호환 엔드포인트 — 직접 입력하세요, 예: http://localhost:8000/v1",
  "settings.modal.modelHintPreset":
    "원하는 모델 ID를 직접 입력할 수 있습니다. 목록은 이 제공자의 주요 모델 참고용입니다.",
  "settings.modal.modelHintCompat": "OpenAI 호환 API — 모델 ID를 수동으로 입력하세요.",
  "settings.modal.needEndpoint": "Endpoint를 입력하세요",
  "settings.modal.needModel": "Model ID를 선택 / 입력하세요",
  "settings.modal.testing": "테스트 중…",
  "settings.modal.testOk": "✓ 연결 정상",
  "settings.modal.testFail": "✗ 실패: {e}",

  // chat / sidebar / jobs / artifact / platform
  "chat.thinking": "✻ 사고 · {preview}",
  "chat.processing": "처리 중…",
  "sidebar.currentTask": "현재 작업",
  "sidebar.running": "실행 중…",
  "sidebar.deleteTask": "작업 삭제",
  "sessions.deleteConfirm.title": "작업 삭제",
  "sessions.deleteConfirm.message":
    "작업 「{name}」을(를) 삭제하시겠습니까? 이 작업은 되돌릴 수 없습니다.",
  "sidebar.renameTask": "작업 이름 바꾸기",
  "sidebar.section.tasks": "작업",
  "sidebar.section.workspace": "워크스페이스",
  "sidebar.newSessionInDir": "이 폴더에서 새 세션 만들기",
  "jobs.secAgo": "{n}초 전",
  "jobs.minAgo": "{n}분 전",
  "jobs.hourAgo": "{n}시간 전",
  "jobs.dayAgo": "{n}일 전",
  "jobs.sub": "AI가 task_start로 시작한 장시간 백그라운드 작업(비동기 실행, 조회·중지 가능)",
  "jobs.empty": "백그라운드 작업이 없습니다. AI가 task_start를 호출하면 여기에 표시됩니다.",
  "jobs.running": "실행 중",
  "jobs.completed": "완료",
  "artifact.tab.preview": "미리보기",
  "artifact.tab.code": "코드",
  "common.refresh": "새로고침",
  "common.close": "닫기",
  "artifact.readFailed": "읽기 실패: {e}",
  "platform.cwdPrompt": "작업 디렉터리(절대 경로, 비우면 = 기본값 복원):",

  // generic actions
  "common.edit": "편집",
  "common.delete": "삭제",

  // scheduled tasks
  "tasks.desc": "일정에 따라 prompt를 실행하고 결과를 채널로 전송",
  "tasks.add": "＋ 새 작업",
  "tasks.stat.running": "실행 중",
  "tasks.stat.totalRuns": "누적 실행",
  "tasks.stat.disabled": "비활성",
  "tasks.stat.successRate": "지난 성공률",
  "tasks.status.ok": "성공",
  "tasks.status.notRun": "미실행",
  "tasks.status.maxIter": "미완료 · 턴 상한",
  "tasks.status.llmError": "모델 호출 실패",
  "tasks.status.workdirNotFound": "작업 디렉터리 없음",
  "tasks.status.notifyErr": "알림 실패",
  "tasks.empty": "아직 작업이 없습니다. “＋ 새 작업”을 눌러 만드세요.",
  "tasks.run": "▶ 실행",
  "tasks.runTitle": "일정을 기다리지 않고 지금 한 번 실행해 결과 확인",
  "tasks.logs": "로그",
  "tasks.runsCount": "{n}회 실행",
  "tasks.lastDuration": "지난번 {s}s",
  "tasks.modelTitle": "사용 모델",
  "tasks.runningBtn": "실행 중…",
  "tasks.runFailed": "실행 실패: {e}",
  "tasks.logsTitle": "{name} · 실행 로그",
  "tasks.noLogs": "로그가 아직 없습니다",
  "tasks.interval.days": "{n}일마다",
  "tasks.interval.hours": "{n}시간마다",
  "tasks.interval.minutes": "{n}분마다",
  "tasks.interval.seconds": "{n}초마다",
  "tasks.form.editTitle": "작업 편집",
  "tasks.form.addTitle": "새 작업",
  "tasks.form.name": "작업 이름",
  "tasks.form.namePlaceholder": "예: “매일 뉴스 브리핑”",
  "tasks.form.prompt": "작업 내용(prompt)",
  "tasks.form.promptPlaceholder": "에이전트에 전달할 지시",
  "tasks.form.schedule": "스케줄 방식",
  "tasks.form.byInterval": "간격으로",
  "tasks.form.byCron": "Cron 식",
  "tasks.form.cronPlaceholder": "0 30 8 * * *  (초 분 시 일 월 요일; 5개 필드도 가능)",
  "tasks.form.cronHint": "로컬 시간대. 예: 매일 8:30 → <code>30 8 * * *</code>",
  "tasks.form.channel": "푸시 채널",
  "tasks.form.model": "모델(선택)",
  "tasks.form.modelHint":
    "비우면 = 전역 기본 모델 사용; 이 작업 전용 모델을 지정할 수 있습니다(예: 고빈도 작업에 더 저렴한 모델).",
  "tasks.form.workdir": "작업 디렉터리(선택)",
  "tasks.form.workdirPlaceholder":
    "비우면 = 전역 작업 디렉터리; 절대 경로를 넣으면 이 작업이 해당 디렉터리에서 격리 실행됩니다",
  "tasks.form.workdirHint":
    "지정하면 이 작업은 해당 디렉터리에서 실행되고 그 <code>&lt;dir&gt;/skills</code> 프로젝트 스킬을 불러옵니다. 전역을 오염시키지 않습니다.",
  "tasks.form.create": "생성",
  "tasks.form.noChannel": "(알림 안 함)",
  "tasks.form.defaultModel": "기본 모델",
  "tasks.form.defaultModelNamed": "기본 모델({name})",
  "tasks.form.required": "작업 이름 / 내용 / 스케줄은 필수입니다",

  // skills
  "skills.src.builtin": "내장",
  "skills.src.installed": "로컬 / Git",
  "skills.src.workdir": "프로젝트",
  "skills.group.builtin": "내장 스킬",
  "skills.group.workdir": "프로젝트 스킬",
  "skills.group.installed": "설치된 스킬",
  "skills.filter.all": "전체",
  "skills.filter.builtin": "내장",
  "skills.filter.workdir": "프로젝트",
  "skills.filter.installed": "설치됨",
  "skills.searchMine": "이름 또는 설명으로 검색…",
  "skills.clearSearch": "검색 지우기",
  "skills.empty.noMatch":
    "“{q}”와 일치하는 스킬이 없습니다. 더 짧은 키워드를 쓰거나 분류를 바꿔 보세요.",
  "skills.desc":
    "SKILL.md는 작성하면 바로 사용; 내장은 기본 제공, 마켓에서 소스 전환 설치, openclaw에서 마이그레이션도 가능",
  "skills.migrate": "openclaw에서 마이그레이션",
  "skills.import": "Git에서 가져오기",
  "skills.create": "스킬 만들기",
  "skills.tab.mine": "내 스킬",
  "skills.tab.market": "마켓",
  "skills.source": "소스",
  "skills.searchPlaceholder": "마켓 스킬 검색…",
  "skills.toggle.on": "활성화됨(클릭하여 비활성화)",
  "skills.toggle.off": "비활성화됨(클릭하여 활성화)",
  "skills.noDesc": "(설명 없음)",
  "skills.viewSkillMd": "SKILL.md 보기",
  "skills.uninstall": "제거",
  "skills.status.enabling": "{name} 활성화 중…",
  "skills.status.disabling": "{name} 비활성화 중…",
  "skills.status.uninstalling": "{name} 제거 중…",
  "skills.installed": "설치됨",
  "skills.install": "설치",
  "skills.status.installing": "{name} 설치 중…",
  "skills.status.installed": "{name} 설치됨",
  "skills.status.installFailed": "설치 실패: {e}",
  "skills.empty.mine":
    "아직 스킬이 없습니다. “마켓”에서 설치, “Git에서 가져오기”, “openclaw에서 마이그레이션” 또는 “스킬 만들기”를 이용하세요.",
  "skills.empty.market":
    "이 소스에 설치 가능한 스킬이 없습니다(도달 불가 또는 검색 결과 없음). 다른 소스로 바꾸거나 검색을 지우세요.",
  "skills.customSource": "사용자 지정 소스…",
  "skills.loadingMarket": "마켓 불러오는 중…",
  "skills.marketLoadFailed": "마켓 불러오기 실패: {e}",
  "skills.switchingSource": "소스 전환 중…",
  "skills.customSourcePrompt": "사용자 지정 registry 소스 URL(정적 JSON):",
  "skills.cannotRead": "(읽을 수 없음)",
  "skills.import.title": "Git에서 스킬 가져오기",
  "skills.import.urlLabel": "Git 저장소 URL",
  "skills.import.subLabel": "하위 디렉터리(선택)",
  "skills.import.subPlaceholder": "기본값 skills",
  "skills.import.note":
    "전체 저장소 또는 단일 스킬 가져오기 지원; <code>@file</code> 참조는 자동으로 인라인됩니다.",
  "skills.import.ok": "가져오기",
  "skills.import.needUrl": "Git URL을 입력하세요",
  "skills.import.cloning": "클론 후 가져오는 중…",
  "skills.migrate.title": "openclaw에서 스킬 마이그레이션",
  "skills.migrate.note":
    "openclaw의 일반적인 스킬 위치(~/.config/openclaw, .agents/skills 등)를 자동 감지했습니다. 선택하면 WiseCortex로 가져옵니다.",
  "skills.migrate.scanning": "감지 중…",
  "skills.migrate.importSelected": "선택 가져오기",
  "skills.migrate.empty":
    "openclaw 스킬을 감지하지 못했습니다. “Git에서 가져오기”를 사용해 보세요.",
  "skills.migrate.exists": "이미 존재",
  "skills.migrate.needOne": "최소 하나 이상 선택하세요",
  "skills.migrate.importing": "가져오는 중…",
  "skills.migrate.done": "{n}/{total}개 스킬을 마이그레이션했습니다",
  "skills.creator.step.basic": "기본 정보",
  "skills.creator.step.trigger": "트리거",
  "skills.creator.step.body": "스킬 본문",
  "skills.creator.step.tools": "도구",
  "skills.creator.step.preview": "미리보기 및 저장",
  "skills.creator.prev": "이전",
  "skills.creator.next": "다음",
  "skills.creator.slugLabel": "스킬 slug(invoke용, kebab-case)",
  "skills.creator.descLabel": "한 줄 설명",
  "skills.creator.descPlaceholder": "git 커밋과 issue를 집계해 주간 보고서 출력",
  "skills.creator.triggerLabel": "사용 시점(트리거, 자연어)",
  "skills.creator.triggerPlaceholder": "주간 보고서 / 업무 요약을 언급할 때",
  "skills.creator.bodyLabel": "스킬 본문(Markdown, @path로 파일 참조 가능)",
  "skills.creator.bodyPlaceholder": "# 단계\n1. …",
  "skills.creator.toolsLabel": "이 스킬이 사용할 도구",
  "skills.creator.mdWhenUse": "## 사용 시점",
  "skills.creator.mdTools": "## 사용 가능한 도구",
  "skills.creator.previewLabel": "SKILL.md 미리보기:",
  "skills.creator.save": "스킬 저장",
  "skills.creator.needSlug": "slug를 입력하세요",

  // channels
  "common.copy": "복사",
  "channels.desc": "IM을 연결해 양방향 대화하거나 아웃바운드 푸시 대상을 설정",
  "channels.name": "이름",
  "channels.saveFailed": "저장 실패",
  "channels.platform.feishu.name": "페이슈(Feishu)",
  "channels.platform.feishu.desc": "이벤트 구독 · 그룹/DM 양방향",
  "channels.platform.wecom.name": "위챗워크(WeCom)",
  "channels.platform.wecom.desc": "암호화 콜백 · 앱 메시지",
  "channels.platform.onebot.name": "QQ",
  "channels.platform.onebot.desc": "OneBot / NapCat · 그룹과 DM",
  "channels.platform.email.name": "이메일",
  "channels.platform.email.desc": "SMTP 발송 · 아웃바운드 알림",
  "channels.platform.webhook.name": "범용 Webhook",
  "channels.platform.webhook.desc": "아웃바운드 전용 · 임의의 HTTP 엔드포인트로 푸시",
  "channels.callback.feishuSummary": "공개 배포(고급): 이벤트 구독 콜백 URL",
  "channels.callback.feishuNote":
    "서버에 공개 주소가 있을 때만 사용. 로컬에서는 아래 “롱 커넥션”(공개 불필요)을 사용하세요.",
  "channels.callback.wecomHint": "(공개 HTTPS 필요, WeCom 앱의 「메시지 수신」에 입력)",
  "channels.callback.genericHint": "(해당 플랫폼 콘솔에 입력)",
  "channels.callback.label": "인바운드 콜백 URL",
  "channels.appPushTarget": "앱 푸시 → {target}",
  "channels.notConfigured": "미설정. ",
  "channels.notConfigured.webhook": "아웃바운드 webhook 대상을 추가하세요.",
  "channels.notConfigured.generic": "연결하면 푸시할 수 있고 양방향 대화도 지원합니다.",
  "channels.badge.configured": "설정됨",
  "channels.badge.notConnected": "미연결",
  "channels.feishu.scan": "QR로 연결",
  "channels.feishu.lcOn": "롱 커넥션: 켬(공개 불필요)",
  "channels.feishu.lcOff": "롱 커넥션: 끔",
  "channels.feishu.lcTitle":
    "롱 커넥션(WebSocket, 공개 콜백 불필요) — 메시지는 이 경로로 수신; 토글은 즉시 적용, 재시작 불필요",
  "channels.feishu.lcOnStatus": "롱 커넥션을 켰습니다(몇 초 내 자동 연결)",
  "channels.feishu.lcOffStatus": "롱 커넥션을 껐습니다(몇 초 내 자동 해제)",
  "channels.feishu.appPush": "앱을 푸시에 재사용",
  "channels.feishu.appPushTitle":
    "QR로 연결된 Feishu 앱 봇을 아웃바운드 푸시에 사용(예약 작업에서 선택 가능). 커스텀 봇을 따로 만들 필요 없음",
  "channels.feishu.configOutbound": "아웃바운드 푸시 설정",
  "channels.wecom.credsSet": "수신 자격 증명: 설정됨",
  "channels.wecom.credsConfig": "수신 자격 증명 설정",
  "channels.addTarget": "대상 추가",
  "channels.connect": "{name} 연결",
  "channels.delete": "{name} 삭제",
  "channels.copyOk": "콜백 URL을 복사했습니다",
  "channels.copyFail": "복사 실패, 텍스트를 직접 선택해 복사하세요",
  "channels.faPush.recentHint":
    "아래에 봇에게 최근 메시지를 보낸 대화가 표시됩니다. 하나 선택하세요.",
  "channels.faPush.noRecentHint":
    "최근 대화가 아직 없습니다 — 먼저 Feishu에서 봇에게 메시지를 보내고(그룹에서 @ 또는 DM) 돌아와 새로고침하세요. chat_id를 직접 붙여 넣어도 됩니다.",
  "channels.faPush.title": "Feishu 앱 푸시(QR 연결 재사용)",
  "channels.faPush.namePlaceholder": "예: “개발 그룹 푸시”",
  "channels.faPush.chatLabel": "대상 chat_id",
  "channels.faPush.chatPlaceholder": "oc_…(그룹) / 최근 대화 선택",
  "channels.faPush.required": "이름과 대상 대화는 필수입니다",
  "channels.config.smtpHost": "SMTP 서버 (host:port)",
  "channels.config.onebotBase": "OneBot HTTP 기본 URL",
  "channels.config.smtpPlaceholder": "예: smtp.example.com:465",
  "channels.config.recipients": "수신자(쉼표 구분)",
  "channels.config.username": "사용자 이름",
  "channels.config.usernamePlaceholder": "SMTP 로그인, 보통 발신 이메일",
  "channels.config.password": "비밀번호 / 앱 비밀번호",
  "channels.config.passwordPlaceholder": "SMTP 비밀번호 또는 앱 비밀번호",
  "channels.config.from": "발신자(비우면=사용자 이름)",
  "channels.config.emailNote":
    "포트 465=암시적 TLS, 587=STARTTLS. 자격 증명은 로컬 백엔드에만 저장됩니다.",
  "channels.config.title": "{name} 설정",
  "channels.config.namePlaceholder": "예: “개발 그룹”",
  "channels.config.groupLabel": "그룹 번호(target)",
  "channels.config.groupPlaceholder": "그룹 번호, 선택",
  "channels.config.callbackNote":
    "양방향 대화: 위의 콜백 URL을 {name} 콘솔에 입력하고 CLI로 앱 자격 증명을 설정하세요.",
  "channels.config.required": "이름과 주소는 필수입니다",
  "channels.scan.title": "Feishu QR 연결",
  "channels.scan.generating": "QR 코드 생성 중…",
  "channels.scan.wait": "잠시만 기다려 주세요…",
  "channels.scan.note":
    "<strong>Feishu / Lark App</strong>으로 스캔해 인증하고 앱을 만듭니다. 성공하면 app_id / app_secret가 자동 입력됩니다.<br/>스캔은 <strong>앱 생성과 자격 증명 취득만</strong> 합니다. 메시지를 받으려면 Feishu 개발자 콘솔에서 추가로: ① 권한 관리에 <code>im:message</code> 추가; ② 이벤트 구독에서 “롱 커넥션” 선택 후 “메시지 수신” 구독; ③ 버전 게시. 그런 다음 이 페이지에서 “롱 커넥션”을 켜고 서버를 재시작하세요.",
  "channels.scan.failed": "QR 절차를 시작할 수 없습니다: {e}",
  "channels.scan.prompt": "Feishu / Lark App으로 스캔해 인증…",
  "channels.scan.connected": "연결됨! app_id={id}",
  "channels.scan.denied": "인증이 거부되었습니다. 닫고 다시 시도하세요.",
  "channels.scan.expired": "QR 코드가 만료되었습니다. 닫고 다시 스캔하세요.",
  "channels.scan.error": "오류: {e}",
  "channels.wecom.title": "WeCom · 수신 자격 증명",
  "channels.wecom.corpId": "기업 ID(corp_id)",
  "channels.wecom.secret": "앱 Secret(corp_secret)",
  "channels.wecom.secretPlaceholder": "비우면=변경 안 함",
  "channels.wecom.agentId": "앱 AgentId(agent_id)",
  "channels.wecom.agentIdPlaceholder": "예: 1000002",
  "channels.wecom.token": "콜백 Token(callback_token)",
  "channels.wecom.tokenPlaceholder": "콘솔 「메시지 수신」의 Token",
  "channels.wecom.aesKey": "콜백 EncodingAESKey",
  "channels.wecom.aesPlaceholder": "43자, 비우면=변경 안 함",
  "channels.wecom.note":
    'WeCom 콘솔 → 앱 → 「메시지 수신」에서 API 수신 설정: URL에 <span class="mono">{cb}</span>(공개 HTTPS로 도달 가능해야 함), Token / EncodingAESKey는 여기와 일치. 자격 증명은 로컬 백엔드에만 저장됩니다.',
  "channels.wecom.savedReady": "WeCom 자격 증명을 저장했습니다(준비됨)",
  "channels.wecom.savedIncomplete": "WeCom 자격 증명을 저장했습니다(필드 부족)",
  "channels.platform.qqbot.name": "QQ 봇(공식)",
  "channels.platform.qqbot.desc":
    "QQ 오픈 플랫폼 · AppID/시크릿 · 게이트웨이 연결, 공개 주소 불필요",
  "channels.qq.scan": "QR로 연결(권장)",
  "channels.qq.creds": "자격 증명 수동 입력",
  "channels.qq.credsSet": "자격 증명: 설정됨",
  "channels.qq.connect": "게이트웨이: 꺼짐 · 클릭하여 연결",
  "channels.qq.disconnect": "게이트웨이: 켜짐 · 클릭하여 연결 해제",
  "channels.qq.toggleTitle":
    "QQ 게이트웨이(WebSocket, 공개 콜백 불필요) — 전환 즉시 적용, 재시작 불필요",
  "channels.qq.onStatus": "QQ 게이트웨이 활성화(몇 초 내 연결)",
  "channels.qq.offStatus": "QQ 게이트웨이 비활성화",
  "channels.qq.appId": "AppID",
  "channels.qq.scanTitle": "QR로 QQ 봇 연결",
  "channels.qq.scanNote":
    "<strong>휴대폰 QQ 앱</strong>으로 스캔한 뒤 연결할 봇을 선택하면 AppID/AppSecret이 자동 입력됩니다. 서버 주소가 필요 없습니다. 연결 페이지는 Tencent가 호스팅하며 연동 주체는 기본적으로 “서드파티 봇”으로 표시됩니다.",
  "channels.qq.scanPrompt": "휴대폰 QQ로 스캔하여 연결…",
  "channels.qq.scanConnected": "연결됨! AppID={id}",
  "channels.qq.title": "QQ 봇 자격 증명",
  "channels.qq.appSecret": "AppSecret",
  "channels.qq.appSecretPlaceholder": "비워 두면 기존 값 유지",
  "channels.qq.note":
    "q.qq.com에서 봇을 만든 후 설정 페이지에서 AppID/AppSecret을 복사하세요. 서버 주소가 필요 없습니다 — WebSocket 게이트웨이로 아웃바운드 연결합니다. 게이트웨이가 code 4914로 닫히면 봇에 그룹/DM 메시지 권한이 없는 것입니다.",
  "channels.qq.savedReady": "저장됨(자격 증명 준비 완료, 클릭하여 연결)",
  "channels.qq.savedIncomplete": "저장됨(자격 증명 불완전)",
  "channels.platform.clawbot.name": "WeChat ClawBot",
  "channels.platform.clawbot.desc": "iLink 롱 폴링 · 개인 계정 DM, 공개 주소 불필요",
  "channels.clawbot.botId": "Bot ID",
  "channels.clawbot.scan": "QR로 연결",
  "channels.clawbot.scanTitle": "WeChat ClawBot 연결",
  "channels.clawbot.scanNote":
    "<strong>휴대폰 WeChat</strong>으로 스캔한 뒤 확인하세요. WeChat 계정당 봇은 하나만 만들 수 있으며 본인과 1:1로 연결됩니다. 텍스트와 음성(서버 측 전사)을 지원하며 이미지와 파일은 아직 지원하지 않습니다.",
  "channels.clawbot.scanPrompt": "WeChat으로 스캔하고 휴대폰에서 확인하세요…",
  "channels.clawbot.scanConnected": "연결됨! {id}",
  "channels.clawbot.connect": "폴링: 꺼짐 · 클릭하여 시작",
  "channels.clawbot.disconnect": "폴링: 켜짐 · 클릭하여 중지",
  "channels.clawbot.toggleTitle": "iLink 롱 폴링(공개 콜백 불필요) — 즉시 적용, 재시작 불필요",
  "channels.clawbot.onStatus": "WeChat 폴링을 시작했습니다",
  "channels.clawbot.offStatus": "WeChat 폴링을 중지했습니다",
  "channels.clawbot.soloNote":
    "<strong>한 곳에서만</strong> 켜세요. 동기화 커서는 봇 단위로 공유되므로 두 대에서 동시에 폴링하면 메시지가 무작위로 나뉩니다.",
  "tasks.form.chanGroupFeishu": "Feishu 앱",
  "tasks.form.chanGroupQq": "QQ 봇",
  "tasks.form.feishuChatOpt": "Feishu · {id}",
  "tasks.form.qqC2cOpt": "QQ DM · {id}",
  "tasks.form.qqGroupOpt": "QQ 그룹 · {id}",
} satisfies Record<MessageKey, string>;
