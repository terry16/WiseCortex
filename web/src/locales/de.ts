import type { MessageKey } from "./en";

// Deutsch
export const de = {
  // generic
  "common.done": "Fertig",
  "common.add": "Hinzufügen",
  "common.remove": "Entfernen",
  "common.itemCount": "{n} Einträge",

  // nav / shell
  "brand.sub": "Selbst gehosteter KI-Agent",
  "nav.newTask": "Neue Aufgabe",
  "nav.chat": "Chat",
  "nav.tasks": "Geplant",
  "nav.jobs": "Hintergrund",
  "nav.skills": "Fähigkeiten",
  "nav.channels": "Kanäle",
  "nav.settings": "Einstellungen",

  // theme
  "theme.toggle": "Design wechseln (hell / dunkel)",

  // footer (rail)
  "foot.modelLabel": "Aktuelles Modell",
  "foot.costLabel": "Kosten heute",
  "foot.model.unset": "Nicht konfiguriert",
  "foot.access.locked": "access_key gesperrt",
  "foot.access.public": "Öffentlicher Zugriff",
  "foot.versionTitle": "Server-Version",

  // offline
  "offline.banner": "Verbindung verloren – neu verbinden…",

  // login gate
  "login.sub": "Diese Instanz ist geschützt. Bitte Zugriffsschlüssel eingeben.",
  "login.placeholder": "Zugriffsschlüssel",
  "login.enter": "Eintreten",
  "login.err.wrong": "Falscher Schlüssel, bitte erneut versuchen",
  "login.err.unreachable": "Server nicht erreichbar, bitte später erneut versuchen",
  "auth.prompt": "Dieses WiseCortex benötigt einen Zugriffsschlüssel, bitte eingeben:",

  // attachments
  "attach.removeHint": "Zum Entfernen klicken",

  // composer
  "composer.placeholder":
    "WiseCortex eine Aufgabe zuweisen…  (Enter zum Senden, Shift+Enter für Zeilenumbruch)",
  "composer.more": "Aktionen einblenden",
  "composer.stop": "Stopp",
  "composer.cwd.label": "Arbeitsverzeichnis",
  "composer.cwd.defaultDir": "Standardverzeichnis",
  "composer.cwd.task": "Arbeitsverzeichnis der Aufgabe: {dir}",
  "composer.cwd.locked": " (Aufgabe gestartet, gesperrt)",
  "composer.cwd.global": "Arbeitsverzeichnis (global): {dir}",
  "composer.cwd.pickPrompt":
    "Arbeitsverzeichnis der Aufgabe (leer = globalen Arbeitsbereich nutzen):",
  "composer.model.title": "Modell für diese Aufgabe (Standard: global)",
  "composer.effort.title":
    "Denkaufwand für diesen Chat (ignoriert, wenn das Modell ihn nicht unterstützt)",
  "composer.effort.opt.default": "Denken: Standard",
  "composer.effort.opt.off": "Denken: aus",
  "composer.effort.opt.low": "Denken: low",
  "composer.effort.opt.medium": "Denken: medium",
  "composer.effort.opt.high": "Denken: high",
  "composer.effort.opt.xhigh": "Denken: xhigh",
  "composer.effort.opt.max": "Denken: max",
  "composer.skills.pinned": "Fähigkeiten dieser Aufgabe ({n}): {list}",
  "composer.skills.empty":
    "Fähigkeiten dieser Aufgabe (standardmäßig automatisch, zum Anheften klicken)",
  "composer.perm.title": "Berechtigung für Tool-Aktionen",
  "composer.perm.default": "Standardberechtigung",
  "composer.perm.solo": "solo (automatisch genehmigen)",
  "composer.perm.strict": "Streng (jeden Schritt bestätigen)",
  "composer.plan.on":
    "Planmodus: AN (nur lesendes Erkunden + Plan, keine Änderungen). Nach Freigabe hier ausschalten und „ausführen“ sagen.",
  "composer.plan.off":
    "Planmodus: nur lesendes Erkunden + Plan, keine Änderungen. Zum Aktivieren klicken.",
  "composer.kb.pinned": "Wissensbasis dieses Chats ({n}):\n{list}",
  "composer.kb.empty":
    "Wissensbasis (dieser Chat – zum Hinzufügen eines Ordners/einer Datei klicken)",
  "composer.mem.title": "Gedächtnis (ansehen / bearbeiten)",

  // skills picker
  "skillsPicker.title": "Fähigkeiten dieser Aufgabe",
  "skillsPicker.note":
    "0 angeheftet = die KI wählt automatisch aus allen Fähigkeiten; einige angeheftet = diese Aufgabe nutzt nur diese (stabiler, keine Fehlauswahl).",
  "skillsPicker.empty": "Keine Fähigkeiten verfügbar.",
  "skillsPicker.projectBadge": "Projekt",
  "skillsPicker.projectBadge.title": "Aus dem Arbeitsverzeichnis dieser Aufgabe skills/",

  // knowledge base
  "kb.title": "Wissensbasis (dieser Chat)",
  "mem.title": "Gedächtnis",
  "mem.note":
    "Die KI schreibt diese Einträge selbst über das remember-Tool; sie werden in jeder Runde eingespielt. Hier siehst du, was sie sich tatsächlich gemerkt hat, und kannst Falsches korrigieren. Einträge unter <b>Lessons</b> sind Fehler, die sich nicht wiederholen dürfen — die KI soll sie vorrangig befolgen, und sie werden bei Platzmangel zuletzt verworfen.",
  "mem.project": "Projektgedächtnis — {dir}",
  "mem.projectHint":
    "Wird von <b>allen Sitzungen</b> in diesem Arbeitsverzeichnis geteilt, auch von neuen Unterhaltungen. Regeln, die immer gelten sollen, gehören hierher.",
  "mem.session": "Nur diese Unterhaltung",
  "mem.sessionHint": "Gehört allein zu diesem Chat; mit dem Chat wird es gelöscht.",
  "mem.empty": "(leer)",
  "kb.note":
    "Ordner oder Dateien als Wissensbasis für diesen Chat einbinden; bei Fragen ruft die KI vor der Antwort knowledge_search auf (nur lokal, nur dieser Chat).",
  "kb.addLabel": "Pfad hinzufügen (Ordner oder Datei, absoluter Pfad)",
  "kb.input.placeholder": "z. B. D:\\kb oder ~/wisecortex/workspace/kb/faq.md",
  "kb.pick.title": "Ordner auswählen",
  "kb.empty": "Noch keine Wissensbasis-Pfade eingebunden.",
  "kb.pickPrompt": "Absoluter Pfad des Wissensbasis-Ordners:",

  // chat lifecycle (used by ws-dispatcher)
  "chat.done": "Fertig ({n} Runden, {cost})",
  "chat.done.duration": " · {duration}s",
  "chat.done.cache": "Cache-Treffer {rate}% ({hits}/{total}, {tokens})",
  "chat.interrupted": "Abgebrochen",
  "chat.queued": "⏳ In Warteschlange — läuft automatisch nach der aktuellen Runde",
  "chat.retry": "Wiederholen",
  "chat.retrying": "Wird wiederholt…",
  "error.insufficient_credit": "Guthaben unzureichend",
  "error.insufficient_credit.action": "Aufladen",

  // artifacts
  "artifact.rendered": "Erstellt · zum Vorschauen rechts klicken",
  "artifact.source": "Erstellt · zum Anzeigen des Quellcodes klicken",

  // hero (empty state)
  "greet.night": "Noch wach",
  "greet.morning": "Guten Morgen",
  "greet.afternoon": "Guten Tag",
  "greet.evening": "Guten Abend",
  "hero.title": "{greet} — was kann ich für dich tun?",
  "hero.sub":
    "WiseCortex kann Code schreiben, Skripte ausführen, online recherchieren, Fähigkeiten aufrufen und Ergebnisse an deinen Messenger senden.",
  "suggest.landing.title": "Eine Produkt-Landingpage bauen",
  "suggest.landing.sub": "Eine responsive Einzelseite aus einem Briefing",
  "suggest.research.title": "Ein Thema online recherchieren",
  "suggest.research.sub": "Quellen sammeln und Kernpunkte zusammenfassen",
  "suggest.debug.title": "Hilf mir, einen Bug zu finden",
  "suggest.debug.sub": "Ursache finden, dann beheben",
  "suggest.chart.title": "Daten in Diagramme verwandeln",
  "suggest.chart.sub": "CSV / Tabellen → Visualisierung",

  // settings — language
  "settings.language.title": "Oberflächensprache",
  "settings.language.sub": "Wird sofort wirksam (die Seite wird neu geladen).",
  "settings.language.label": "Sprache",

  // settings — generic
  "common.cancel": "Abbrechen",
  "common.save": "Speichern",
  "settings.pick": "Wählen…",
  "settings.keySetPlaceholder": "Bereits gesetzt (leer = unverändert)",

  // settings — model access
  "settings.models.title": "Modellzugang",
  "settings.models.sub":
    "BYOK mit eigenem API-Schlüssel; pro Modell Preise für die Kostenrechnung festlegen",
  "settings.models.add": "Modell hinzufügen",
  "settings.models.default": "Standardmodell",
  "settings.models.empty":
    "Noch keine Modelle — klicke auf „Modell hinzufügen“, um eines einzurichten.",
  "settings.models.badge.default": "Standard",
  "settings.models.badge.vision": "Vision",
  "settings.models.badge.visionTitle": "Unterstützt Bildeingabe",
  "settings.models.priceIn": "Ein",
  "settings.models.priceOut": "Aus",
  "settings.models.priceUnit": "¥ / Mio. Tokens",
  "settings.models.keySet": "Schlüssel gesetzt",
  "settings.models.keyUnset": "Nicht konfiguriert",
  "settings.models.subscription": "Abo-Kontingent",
  "settings.models.subscriptionTitle":
    "Dieses Modell nutzt das Kontingent deiner Abo-Anmeldung (OAuth); kein API-Schlüssel nötig",
  "settings.models.configure": "Konfigurieren",
  "settings.models.delete": "Löschen",
  "settings.models.optionLabel": "{model} ({provider})",

  // settings — workspace
  "settings.workspace.title": "Arbeitsverzeichnis",
  "settings.workspace.sub":
    "Globaler Ort für KI-generierte Skripte / Wissensbasis / Selbstlernen; jeder Chat kann im Eingabefeld ein temporäres Verzeichnis festlegen",
  "settings.workspace.label": "Globales Arbeitsverzeichnis",
  "settings.workspace.placeholder": "Leer = Systemstandard verwenden",
  "settings.workspace.hintTauri":
    "Klicke auf „Wählen…“, um ein Verzeichnis über den Systemdialog zu wählen; du kannst den Pfad auch direkt bearbeiten.",
  "settings.workspace.hintWeb": "WebUI: Gib einen absoluten Pfad auf dem Server ein.",
  "settings.workspace.pickPrompt": "Globales Arbeitsverzeichnis (leer = Systemstandard):",

  // settings — proxy
  "settings.proxy.title": "Netzwerk-Proxy",
  "settings.proxy.sub":
    "Wenn gesetzt, läuft jeder ausgehende Zugriff (LLM, Skills/Marktplatz, Web-Tools) über den Proxy; leer = direkt",
  "settings.proxy.label": "Proxy-Adresse",
  "settings.proxy.placeholder": "http://host:port oder socks5://host:port",
  "settings.proxy.hint":
    "Unterstützt http/https/socks5. Sofort wirksam (Hot-Reload). Kann auch per Umgebungsvariable WC_PROXY überschrieben werden.",

  // settings — Claude subscription OAuth
  "settings.claudeOauth.title": "Claude-Abo-Anmeldung",
  "settings.claudeOauth.sub":
    "Führe Inferenz lokal mit deinem Claude-Abo-Kontingent (Pro/Max) aus, um Modelle direkt zu vergleichen. Füge nach dem Login unter „Modellverwaltung“ ein Modell mit aktiviertem „Claude-Abo“ hinzu.",
  "settings.claudeOauth.step1":
    "Falls der Browser nicht automatisch öffnet (in der Desktop-Shell üblich), klicke auf „Öffnen“ oder „Link kopieren“, um die Autorisierungsseite manuell zu öffnen:",
  "settings.claudeOauth.step2":
    "Kopiere nach dem Login den auf der Seite angezeigten Code und füge ihn unten ein, um abzuschließen:",
  "settings.claudeOauth.codePlaceholder": "Autorisierungscode einfügen (wie code#state)",
  "settings.claudeOauth.warn":
    "⚠️ Verwendet den OAuth-Client von Claude Code; Drittnutzung ist eine Grauzone, nur für persönliche lokale Nutzung; kann durch offizielle Richtlinien eingeschränkt werden oder jederzeit ausfallen.",
  "settings.claudeOauth.loggedIn": "✓ Beim Claude-Abo angemeldet",

  // settings — ChatGPT subscription OAuth
  "settings.chatgptOauth.title": "ChatGPT-Abo-Anmeldung",
  "settings.chatgptOauth.sub":
    "Führe Codex-Modelle lokal mit deinem ChatGPT-Abo-Kontingent (Plus/Pro) aus, um zu vergleichen. Füge nach dem Login unter „Modellverwaltung“ ein Modell mit aktiviertem „ChatGPT-Abo“ hinzu.",
  "settings.chatgptOauth.step1":
    "Klicke auf „Öffnen“ oder „Link kopieren“, um die Autorisierungsseite im Browser zu öffnen und dich anzumelden:",
  "settings.chatgptOauth.step2":
    "Nach der Autorisierung leitet der Browser auf <code>localhost:1455</code> weiter (öffnet meist nicht — das ist normal). Füge die <b>komplette URL aus der Adressleiste</b> oder nur den Code unten ein:",
  "settings.chatgptOauth.codePlaceholder":
    "Code oder die vollständige Adresse localhost:1455/auth/callback?code=... einfügen",
  "settings.chatgptOauth.warn":
    "⚠️ Verwendet den OAuth-Client von Codex; Drittnutzung ist eine Grauzone, nur für persönliche lokale Nutzung; kann durch offizielle Richtlinien eingeschränkt werden oder jederzeit ausfallen. Benötigt einen Proxy.",
  "settings.chatgptOauth.loggedIn": "✓ Beim ChatGPT-Abo angemeldet",
  "settings.grokOauth.title": "Grok-Abo-Anmeldung",
  "settings.grokOauth.sub":
    "Grok-Modelle über dein X-Premium-/SuperGrok-Abo-Kontingent nutzen (Gerätecode-Anmeldung, funktioniert lokal und auf Servern). Nach der Anmeldung unter „Modellverwaltung“ ein Modell mit angehaktem „Grok-Abo“ anlegen.",
  "settings.grokOauth.step1":
    "Klicke „Öffnen“, um die Autorisierungsseite in einem Browser auf einem beliebigen Gerät zu öffnen:",
  "settings.grokOauth.step2":
    "Gib dort den folgenden Code ein; diese Seite meldet sich danach automatisch an:",
  "settings.grokOauth.warn":
    "⚠️ Verwendet den offiziellen Grok-CLI-OAuth-Client von xAI; Drittnutzung ist eine Grauzone, nur für persönliche Nutzung; kann jederzeit eingeschränkt werden oder ausfallen.",
  "settings.grokOauth.loggedIn": "✓ Beim Grok-Abo angemeldet",
  "settings.grokOauth.waiting":
    "Warte auf Autorisierung… diese Seite aktualisiert sich automatisch, sobald du im Browser fertig bist",
  "settings.grokOauth.expired": "Gerätecode abgelaufen — bitte erneut auf „Anmelden“ klicken",

  // settings — OAuth shared
  "settings.oauth.checking": "Wird geprüft…",
  "settings.oauth.login": "Anmelden",
  "settings.oauth.logout": "Abmelden",
  "settings.oauth.open": "Öffnen",
  "settings.oauth.copyLink": "Link kopieren",
  "settings.oauth.notLoggedIn": "Nicht angemeldet",
  "settings.oauth.relogin": "Erneut anmelden",
  "settings.oauth.unknown": "Status unbekannt",
  "settings.oauth.openedCode": "Autorisierungsseite geöffnet — Code nach dem Login einfügen",
  "settings.oauth.openedUrl":
    "Autorisierungsseite geöffnet — URL aus der Adressleiste nach dem Login einfügen",
  "settings.oauth.manualOpen":
    "Klicke auf „Öffnen“ oder „Link kopieren“, um die Autorisierungsseite manuell zu öffnen",
  "settings.oauth.waitingBrowser":
    "Autorisierungsseite im Browser geöffnet — melde dich an, diese Seite erkennt es automatisch…",
  "settings.oauth.loopbackCopied":
    "Autorisierungslink kopiert — im Browser öffnen und anmelden; diese Seite erkennt es automatisch…",
  "settings.oauth.loopbackTimeout":
    "Zeitüberschreitung beim Warten auf die Autorisierung — bitte erneut anmelden",
  "settings.oauth.loginFailed": "Anmeldung fehlgeschlagen: {e}",
  "settings.oauth.copied": "Link kopiert — im Browser öffnen",
  "settings.oauth.redeeming": "Wird eingelöst…",
  "settings.oauth.failed": "Fehlgeschlagen: {e}",
  "settings.oauth.unknownError": "Unbekannter Fehler",

  // settings — iteration limits
  "settings.maxiter.title": "Limit für Tool-Aufruf-Runden",
  "settings.maxiter.sub":
    "Maximale aufeinanderfolgende Tool-Aufruf-Runden pro Aufgabe (Schutz vor Ausreißern). Interaktiver Chat und unbeaufsichtigte Aufgaben werden getrennt eingestellt.",
  "settings.maxiter.interactive": "Limit für interaktiven Chat",
  "settings.maxiter.interactivePlaceholder": "Leer = Standard 1000",
  "settings.maxiter.interactiveHint":
    "Gilt für Chats, die du selbst in der Desktop-/Web-App führst — hoch ansetzen; in der Entwicklung selten erreicht, jederzeit manuell stoppbar. **Hintergrund-Langläufer (task_start) nutzen dasselbe Limit.** Umgebungsvariable WC_MAX_ITERATIONS_INTERACTIVE hat Vorrang.",
  "settings.maxiter.unattended": "Limit für unbeaufsichtigte Aufgaben",
  "settings.maxiter.unattendedPlaceholder": "Leer = Standard 50",
  "settings.maxiter.unattendedHint":
    "Gilt für geplante / per Feishu·WeCom ausgelöste Aufgaben (unbeaufsichtigt, Schutz vor Kostenausreißern). Für lange Aufgaben wie Mehrdatei-Generierung + Deploy höher setzen. Umgebungsvariable WC_MAX_ITERATIONS hat Vorrang.",
  "settings.maxiter.subagent": "Teilaufgaben-Limit (Sub-Agent)",
  "settings.maxiter.subagentPlaceholder": "Leer = Standard 100",
  "settings.maxiter.subagentHint":
    "Gilt für Sub-Agents, die das <b>task</b>-Tool im Chat erzeugt. Zuvor fest auf 15 — viel zu niedrig: Ein Diff-Review oder eine Log-Analyse verbraucht das schon beim Lesen weniger Dateien, sodass Teilaufgaben fast immer mit „Rundenlimit erreicht“ endeten. Getrennt vom Chat-Limit, weil Teilaufgaben oft mehrfach parallel laufen und die Kosten sich multiplizieren. Umgebungsvariable WC_SUBAGENT_MAX_ITERATIONS hat Vorrang.",

  // settings — log cleanup
  "settings.logclean.title": "Log-Bereinigung geplanter Aufgaben",
  "settings.logclean.sub":
    "Ausführungslogs geplanter Aufgaben werden nach Größe bereinigt: Nur Dateien über dem Limit werden gekürzt, älteste vollständige Ausführung zuerst. Logs unter dem Limit bleiben unangetastet. Stündlich geprüft.",
  "settings.logclean.autoLabel": "Logs automatisch bereinigen",
  "settings.logclean.autoSub":
    "Aus: Logs bleiben dauerhaft erhalten und werden von Ihnen selbst bereinigt.",
  "settings.logclean.maxLabel": "Log-Limit pro Aufgabe (MB)",
  "settings.logclean.placeholder": "Leer = Standard 10",
  "settings.logclean.hint":
    "Über dem Limit werden ganze <b>Ausführungen</b> ältestzuerst entfernt, die neueste bleibt immer erhalten — eine Aufgabe kann also nie viele Läufe mit leerem Log zeigen. Gilt sofort, kein Neustart nötig.",

  // settings — web search
  "settings.websearch.title": "Websuche",
  "settings.websearch.sub":
    "Anbieter für das web_search-Tool; gib einen API-Schlüssel ein oder hoste SearXNG selbst, leer = DuckDuckGo (kein Schlüssel)",
  "settings.websearch.provider": "Anbieter",
  "settings.websearch.ddg": "DuckDuckGo (Standard, kein Schlüssel)",
  "settings.websearch.searxng": "SearXNG (selbst gehostet)",
  "settings.websearch.brave": "Brave Search (API-Schlüssel)",
  "settings.websearch.tavily": "Tavily (API-Schlüssel)",
  "settings.websearch.searxngUrl": "SearXNG-URL",

  // settings — reasoning effort
  "settings.effort.title": "Denkaufwand (extended thinking)",
  "settings.effort.sub":
    "Lässt das Modell vor der Antwort mehr nachdenken — stabiler bei komplexen Aufgaben / kniffligen Bugs; erhöht Dauer und Kosten. Automatisch je Anbieter übersetzt: Anthropic nutzt thinking, OpenAI/Gemini reasoning_effort, Qwen/Hunyuan enable_thinking; Modelle mit eingebautem Denken (z. B. DeepSeek) senden keinen Parameter.",
  "settings.effort.label": "Aufwand",
  "settings.effort.off": "Aus (Standard)",
  "settings.effort.xhigh": "xhigh (idealer Wert fürs Coden)",
  "settings.effort.hint":
    "Sofort wirksam (Hot-Reload). Kann auch per Umgebungsvariable WC_REASONING_EFFORT überschrieben werden.",

  // settings — access control
  "settings.access.title": "Zugriffssteuerung",
  "settings.access.sub": "Schützt das lokale WS/REST und steuert gefährliche Operationen",
  "settings.access.enableKey": "access_key aktivieren",
  "settings.access.enableKeySub":
    "Wenn aktiv, müssen alle Verbindungen den Schlüssel mitführen (Serverneustart zum Übernehmen). <b>Für externen Zugriff erforderlich</b> — der Server bindet standardmäßig nur an localhost; nutze für die Veröffentlichung einen nginx-Reverse-Proxy.",
  "settings.access.keyPlaceholder": "Zugriffsschlüssel festlegen",
  "settings.access.envManaged":
    "Über die Umgebungsvariable WC_ACCESS_KEY gesetzt — auf dem Server ändern (hier nicht editierbar).",
  "settings.access.confirm": "Vor gefährlichen Operationen bestätigen",
  "settings.access.confirmSub":
    "Wenn aktiv, fragen Operationen wie Dateischreiben / Shell zuerst um Zustimmung (aus = unbeaufsichtigt, vollautomatisch)",
  "settings.access.automem": "Auto-Memory",
  "settings.access.automemSub":
    "Wenn aktiv, extrahieren lange Chats alle paar Runden im Hintergrund Kernpunkte in den Sitzungsspeicher (zusätzliche LLM-Aufrufe — bei Bedarf aktivieren; Serverneustart zum Übernehmen)",
  "settings.access.autotrim": "Kontext automatisch schlank halten",
  "settings.access.autotrimSub":
    "Bilder werden nur einmal gesendet; spätere Runden lassen sie weg, das Komprimieren ebenfalls",
  "settings.access.autotrimHelp":
    "Bilder sind das Teuerste im Kontext — ein Screenshot kostet schnell tausend Tokens und wird standardmäßig in jeder Runde erneut mitgeschickt. Ist die Option an, gelangt ein Bild nur in die Runde, in der es gesendet wurde; spätere Runden ersetzen es durch einen einzeiligen Platzhalter, und beim Komprimieren des Verlaufs fallen Bilder ebenfalls weg. Bei UI-Änderungen oder Testläufen ist das Bild nach einmaligem Ansehen wertlos; schalte die Option aus, wenn du dasselbe Bild wiederholt vergleichen musst.",

  // settings — MCP
  "settings.mcp.title": "MCP-Server",
  "settings.mcp.sub":
    "Verbinde MCP-Server (stdio oder Streamable HTTP); ihre tools/resources/prompts werden der KI als <code>mcp__server__tool</code> bereitgestellt; verbindet bei Änderung sofort neu",
  "settings.mcp.label":
    "Serverkonfiguration (JSON: Name → stdio {command,args,env} oder HTTP {url,headers})",
  "settings.mcp.hint":
    "resources/prompts synthetisieren automatisch die Tools <code>list_resources</code>/<code>read_resource</code>/<code>list_prompts</code>/<code>get_prompt</code>; sampling/roots-Rückanfragen werden automatisch behandelt (nur stdio).",
  "settings.mcp.save": "Speichern und neu verbinden",
  "settings.mcp.discovered": "Entdeckte Tools",
  "settings.mcp.noTools": "(noch keine verbundenen Tools)",
  "settings.mcp.savedReconnecting": "Gespeichert, verbinde im Hintergrund neu…",

  // settings — hooks
  "settings.hooks.title": "Event-Hooks (hooks)",
  "settings.hooks.sub":
    "Führe an Event-Punkten Befehle für Leitplanken/Nebeneffekte aus: PreToolUse kann Tools abfangen, PostToolUse hängt Kontext an, SessionStart/Stop/UserPromptSubmit",
  "settings.hooks.label":
    "Hook-Konfiguration (JSON: Event → [{matcher, command, timeout_ms}]; matcher ist eine Tool-Namen-Regex, nur von Pre/PostToolUse genutzt)",
  "settings.hooks.save": "Speichern",
  "settings.hooks.hint":
    'Der Befehl erhält das Event-JSON über stdin; Blockieren: JSON <code>{"decision":"block","reason":"…"}</code> oder ein Exit-Code ungleich 0 (Grund aus stderr); Kontext anhängen: <code>{"additionalContext":"…"}</code>.',

  // settings — statuses
  "settings.status.loading": "Wird geladen…",
  "settings.status.loadFailed": "Laden fehlgeschlagen: {e}",
  "settings.status.workspaceUpdated": "Arbeitsverzeichnis aktualisiert",
  "settings.status.workspaceReset": "Standard-Arbeitsverzeichnis wiederhergestellt",
  "settings.status.websearchUpdated": "Websuche aktualisiert",
  "settings.status.jsonParseFailed": "JSON-Analyse fehlgeschlagen: {e}",
  "settings.status.saving": "Wird gespeichert…",
  "settings.status.saved": "Gespeichert",
  "settings.status.effortUpdated": "Denkaufwand aktualisiert",
  "settings.status.proxySet": "Proxy gesetzt (hot reloaded)",
  "settings.status.proxyOff": "Proxy aus (direkt)",
  "settings.status.maxiterSet":
    "Limit für unbeaufsichtigte Aufgaben auf {v} gesetzt (hot reloaded)",
  "settings.status.maxiterReset": "Standardlimit 50 wiederhergestellt (hot reloaded)",
  "settings.status.maxiterInteractiveSet":
    "Limit für interaktiven Chat auf {v} gesetzt (hot reloaded)",
  "settings.status.maxiterInteractiveReset": "Standardlimit 1000 wiederhergestellt (hot reloaded)",
  "settings.status.maxiterSubagentSet": "Teilaufgaben-Limit auf {v} gesetzt (hot reloaded)",
  "settings.status.maxiterSubagentReset":
    "Standard-Teilaufgabenlimit 100 wiederhergestellt (hot reloaded)",
  "settings.status.logcleanDefault": "Standardlimit 10 MB",
  "settings.status.logcleanOn": "Automatische Log-Bereinigung aktiviert",
  "settings.status.logcleanOff": "Automatische Log-Bereinigung deaktiviert (dauerhaft behalten)",
  "settings.status.logcleanMb": "Limit {v} MB",
  "settings.status.logcleanUpdated": "Log-Bereinigung aktualisiert: {msg}",
  "settings.status.accessKeyOff": "Zugriffsschlüssel deaktiviert (Serverneustart zum Übernehmen)",
  "settings.status.accessKeySet": "Zugriffsschlüssel gesetzt (Serverneustart zum Übernehmen)",
  "settings.status.updated": "Aktualisiert",
  "settings.status.automemOn": "Auto-Memory aktiviert (Serverneustart zum Übernehmen)",
  "settings.status.autotrimOn": "Kontext-Schlankhaltung an (jedes Bild nur einmal)",
  "settings.status.autotrimOff": "Kontext-Schlankhaltung aus (Bilder in jeder Runde erneut)",
  "settings.status.automemOff": "Auto-Memory deaktiviert (Serverneustart zum Übernehmen)",

  // settings — model modal
  "settings.modal.editTitle": "Modell konfigurieren",
  "settings.modal.addTitle": "Modell hinzufügen",
  "settings.modal.provider": "Anbieter",
  "settings.modal.endpoint": "API-Endpunkt",
  "settings.modal.useClaudeOauth": "Claude-Abo verwenden (kein API-Schlüssel nötig)",
  "settings.modal.useClaudeOauthHint":
    "Wenn aktiviert, nutzt dieses Modell das lokal angemeldete Claude-Abo-Kontingent (fester offizieller Anthropic-Endpunkt); melde dich zuerst unter „Claude-Abo-Anmeldung“ an. Setze das „Modell“ oben auf eine Claude-Modell-ID (z. B. claude-sonnet-4-6).",
  "settings.modal.useChatgptOauth": "ChatGPT-Abo verwenden (kein API-Schlüssel nötig)",
  "settings.modal.useChatgptOauthHint":
    "Wenn aktiviert, nutzt dieses Modell das lokal angemeldete ChatGPT-Abo-Kontingent (fester Codex-Backend + Responses-Transport); melde dich zuerst unter „ChatGPT-Abo-Anmeldung“ an. Setze das „Modell“ oben auf eine Codex-Modell-ID (z. B. gpt-5-codex).",
  "settings.modal.useGrokOauth": "Grok-Abo verwenden (kein API-Key nötig)",
  "settings.modal.useGrokOauthHint":
    "Wenn aktiviert, nutzt dieses Modell das angemeldete Grok-Abo-Kontingent (api.x.ai, OpenAI-kompatibler Transport); melde dich zuerst unter „Grok-Abo-Anmeldung“ an. Modelle: grok-code-fast-1, grok-4-1 usw.",
  "settings.modal.useGeminiOauth": "Gemini-Abo verwenden (kein API-Key nötig)",
  "settings.modal.useGeminiOauthHint":
    "Wenn aktiviert, nutzt dieses Modell das angemeldete Gemini-Abo-Kontingent (Code-Assist-Backend, nativer Gemini-Transport); melde dich zuerst unter „Gemini-Abo-Anmeldung“ an. Modelle: gemini-2.5-pro, gemini-2.5-flash usw.",
  "settings.geminiOauth.title": "Gemini-Abo-Anmeldung",
  "settings.geminiOauth.sub":
    "Gemini-Modelle über dein persönliches Google-Konto (Gemini Code Assist, kostenloses/bezahltes Kontingent) nutzen. Nach der Anmeldung unter „Modellverwaltung“ ein Modell mit angehaktem „Gemini-Abo“ anlegen.",
  "settings.geminiOauth.step1":
    "Klicke „Öffnen“, um dich mit deinem Google-Konto anzumelden; die Autorisierungsseite zeigt einen Code:",
  "settings.geminiOauth.step2":
    "Füge den auf der Seite codeassist.google.com/authcode angezeigten Code hier ein:",
  "settings.geminiOauth.warn":
    "⚠️ Verwendet den offiziellen gemini-cli-OAuth-Client; Drittnutzung ist eine Grauzone, nur für persönliche Nutzung; das kostenlose Kontingent unterliegt der Google-Richtlinie und kann jederzeit gedrosselt werden oder ausfallen.",
  "settings.geminiOauth.loggedIn": "✓ Beim Gemini-Abo angemeldet",
  "settings.geminiOauth.loggedInAs": "✓ Beim Gemini-Abo angemeldet ({email})",
  "settings.geminiOauth.codePlaceholder":
    "Code einfügen (oder die vollständige URL codeassist.google.com/authcode?code=...)",
  "settings.modal.vision": "Unterstützt Bildeingabe (vision)",
  "settings.modal.visionHint":
    "Ob dieses Modell Bilder „sehen“ kann. Nach der Modellwahl automatisch aus einer eingebauten Liste angekreuzt; manuell änderbar. <b>Ausgeschaltet werden Bilder vor dem Senden automatisch entfernt</b> (reine Textmodelle wie deepseek müssen es ausschalten, sonst werden Screenshots mit 4xx abgewiesen).",
  "settings.modal.apiKeyHint":
    "Wird nur im lokalen Backend gespeichert, nie hochgeladen. Kann leer bleiben, wenn „Claude-Abo verwenden“ aktiviert ist.",
  "settings.modal.priceLabel": "Preis (¥ / Mio. Tokens, optional)",
  "settings.modal.priceIn": "¥ Ein",
  "settings.modal.priceInPlaceholder": "Eingangspreis",
  "settings.modal.priceOut": "¥ Aus",
  "settings.modal.priceOutPlaceholder": "Ausgangspreis",
  "settings.modal.priceCache": "¥ Cache",
  "settings.modal.priceCachePlaceholder": "Cache-Lesepreis (leer = Eingang × 0,1)",
  "settings.modal.priceHint":
    "Ist der Cache-Lesepreis leer, wird er mit 1/10 des Eingangspreises geschätzt; bei viel Cache unbedingt genau angeben, sonst werden die Kosten überhöht.",
  "settings.modal.maxtokLabel": "Max. Ausgabe-Tokens pro Aufruf (max_tokens, optional)",
  "settings.modal.maxtokPlaceholder": "Leer = globalen Standard 32768 verwenden",
  "settings.modal.maxtokHint":
    "Zu klein schneidet Tool-Argumente beim Schreiben großer Dateien ab und verursacht Fehler; für große Dateien / lange Ausgaben höher setzen. Zu groß kann vom realen Limit des Modells abgelehnt werden (400) — auf das tatsächliche Ausgabelimit des Modells setzen.",
  "settings.modal.effortLabel": "Denkaufwand (thinking)",
  "settings.modal.effortDefault": "(globalem Standard folgen)",
  "settings.modal.effortOff": "Aus",
  "settings.modal.effortHint":
    "Gilt nur für dieses Modell und überschreibt global. Automatisch je Anbieter übersetzt (Anthropic→thinking, OpenAI/Gemini→reasoning_effort, Qwen/Hunyuan→enable_thinking; Modelle mit eingebautem Denken wie DeepSeek senden keinen Parameter, daher ohne Wirkung). „Aus“ = dieses Modell denkt nicht; „globalem Standard folgen“ = den globalen Denkaufwand oben verwenden.",
  "settings.modal.test": "Verbindung testen",
  "settings.modal.baseHintPreset": "Voreingestellter Endpunkt (automatisch ausgefüllt)",
  "settings.modal.baseHintCompat":
    "OpenAI-kompatibler Endpunkt — selbst eingeben, z. B. http://localhost:8000/v1",
  "settings.modal.modelHintPreset":
    "Vom Anbieter voreingestellt — wähle die zu verwendende Modell-ID.",
  "settings.modal.modelHintCompat": "OpenAI-kompatible API — Modell-ID manuell eingeben.",
  "settings.modal.needEndpoint": "Bitte den Endpoint eingeben",
  "settings.modal.needModel": "Bitte die Model ID wählen / eingeben",
  "settings.modal.testing": "Wird getestet…",
  "settings.modal.testOk": "✓ Verbindung OK",
  "settings.modal.testFail": "✗ Fehlgeschlagen: {e}",

  // chat / sidebar / jobs / artifact / platform
  "chat.thinking": "✻ Denken · {preview}",
  "chat.processing": "Wird verarbeitet…",
  "sidebar.currentTask": "Aktuelle Aufgabe",
  "sidebar.running": "Läuft…",
  "sidebar.deleteTask": "Aufgabe löschen",
  "sessions.deleteConfirm.title": "Aufgabe löschen",
  "sessions.deleteConfirm.message":
    "Aufgabe „{name}“ löschen? Dies kann nicht rückgängig gemacht werden.",
  "sidebar.renameTask": "Aufgabe umbenennen",
  "sidebar.section.tasks": "Aufgaben",
  "sidebar.section.workspace": "Arbeitsbereiche",
  "sidebar.newSessionInDir": "Neue Sitzung in diesem Ordner",
  "jobs.secAgo": "vor {n}s",
  "jobs.minAgo": "vor {n}min",
  "jobs.hourAgo": "vor {n}h",
  "jobs.dayAgo": "vor {n}d",
  "jobs.sub":
    "Lang laufende Hintergrundaufgaben, die die KI mit task_start startet (asynchron — einsehbar und stoppbar)",
  "jobs.empty":
    "Keine Hintergrundaufgaben. Sie erscheinen hier, nachdem die KI task_start aufruft.",
  "jobs.running": "Läuft",
  "jobs.completed": "Fertig",
  "artifact.tab.preview": "Vorschau",
  "artifact.tab.code": "Code",
  "common.refresh": "Aktualisieren",
  "common.close": "Schließen",
  "artifact.readFailed": "Lesen fehlgeschlagen: {e}",
  "platform.cwdPrompt": "Arbeitsverzeichnis (absoluter Pfad, leer = Standard wiederherstellen):",

  // generic actions
  "common.edit": "Bearbeiten",
  "common.delete": "Löschen",

  // scheduled tasks
  "tasks.desc": "Einen Prompt nach Zeitplan ausführen und das Ergebnis an einen Kanal senden",
  "tasks.add": "＋ Neue Aufgabe",
  "tasks.stat.running": "Aktiv",
  "tasks.stat.totalRuns": "Läufe gesamt",
  "tasks.stat.disabled": "Deaktiviert",
  "tasks.stat.successRate": "Letzte Erfolgsrate",
  "tasks.status.ok": "Erfolg",
  "tasks.status.notRun": "Nicht gelaufen",
  "tasks.status.maxIter": "Unfertig · Rundenlimit",
  "tasks.status.llmError": "Modellaufruf fehlgeschlagen",
  "tasks.status.workdirNotFound": "Arbeitsverzeichnis fehlt",
  "tasks.status.notifyErr": "Benachrichtigung fehlgeschlagen",
  "tasks.empty": "Noch keine Aufgaben — klicke auf „＋ Neue Aufgabe“, um eine zu erstellen.",
  "tasks.run": "▶ Ausführen",
  "tasks.runTitle":
    "Jetzt einmal ausführen und das Ergebnis ansehen, ohne auf den Zeitplan zu warten",
  "tasks.logs": "Logs",
  "tasks.runsCount": "{n} mal gelaufen",
  "tasks.lastDuration": "zuletzt {s}s",
  "tasks.modelTitle": "Verwendetes Modell",
  "tasks.runningBtn": "Läuft…",
  "tasks.runFailed": "Ausführung fehlgeschlagen: {e}",
  "tasks.logsTitle": "{name} · Ausführungslogs",
  "tasks.noLogs": "Noch keine Logs",
  "tasks.interval.days": "Alle {n} Tag(e)",
  "tasks.interval.hours": "Alle {n} Stunde(n)",
  "tasks.interval.minutes": "Alle {n} Minute(n)",
  "tasks.interval.seconds": "Alle {n} Sekunde(n)",
  "tasks.form.editTitle": "Aufgabe bearbeiten",
  "tasks.form.addTitle": "Neue Aufgabe",
  "tasks.form.name": "Aufgabenname",
  "tasks.form.namePlaceholder": "z. B. „Tägliche News-Zusammenfassung“",
  "tasks.form.prompt": "Aufgabeninhalt (prompt)",
  "tasks.form.promptPlaceholder": "Die Anweisung, die dem Agenten übergeben wird",
  "tasks.form.schedule": "Zeitplan",
  "tasks.form.byInterval": "Nach Intervall",
  "tasks.form.byCron": "Cron-Ausdruck",
  "tasks.form.cronPlaceholder": "0 30 8 * * *  (Sek Min Std Tag Monat Wochentag; 5 Felder auch OK)",
  "tasks.form.cronHint": "Lokale Zeitzone. z. B. täglich 8:30 → <code>30 8 * * *</code>",
  "tasks.form.channel": "Push-Kanal",
  "tasks.form.model": "Modell (optional)",
  "tasks.form.modelHint":
    "Leer = globales Standardmodell verwenden; du kannst dieser Aufgabe ein eigenes Modell zuweisen (z. B. ein günstigeres für häufige Aufgaben).",
  "tasks.form.workdir": "Arbeitsverzeichnis (optional)",
  "tasks.form.workdirPlaceholder":
    "Leer = globales Arbeitsverzeichnis; ein absoluter Pfad führt diese Aufgabe isoliert in diesem Verzeichnis aus",
  "tasks.form.workdirHint":
    "Wenn gesetzt, läuft diese Aufgabe in diesem Verzeichnis und lädt dessen <code>&lt;dir&gt;/skills</code> projektbezogene Skills, ohne die globalen zu beeinflussen.",
  "tasks.form.create": "Erstellen",
  "tasks.form.noChannel": "(keine Benachrichtigung)",
  "tasks.form.defaultModel": "Standardmodell",
  "tasks.form.defaultModelNamed": "Standardmodell ({name})",
  "tasks.form.required": "Aufgabenname / Inhalt / Zeitplan sind erforderlich",

  // skills
  "skills.src.builtin": "Integriert",
  "skills.src.installed": "Lokal / Git",
  "skills.src.workdir": "Projekt",
  "skills.group.builtin": "Integrierte Skills",
  "skills.group.workdir": "Projekt-Skills",
  "skills.group.installed": "Installierte Skills",
  "skills.filter.all": "Alle",
  "skills.filter.builtin": "Integriert",
  "skills.filter.workdir": "Projekt",
  "skills.filter.installed": "Installiert",
  "skills.searchMine": "Nach Name oder Beschreibung suchen…",
  "skills.clearSearch": "Suche löschen",
  "skills.empty.noMatch":
    "Kein Skill passt zu „{q}“. Kürzeres Stichwort versuchen oder Kategorie wechseln.",
  "skills.desc":
    "SKILL.md funktioniert, sobald du es schreibst; Integrierte sind vorinstalliert, der Markt erlaubt Quellenwechsel, und du kannst von openclaw migrieren",
  "skills.migrate": "Von openclaw migrieren",
  "skills.import": "Aus Git importieren",
  "skills.create": "Skill erstellen",
  "skills.tab.mine": "Meine Skills",
  "skills.tab.market": "Markt",
  "skills.source": "Quelle",
  "skills.searchPlaceholder": "Markt-Skills suchen…",
  "skills.toggle.on": "Aktiviert (zum Deaktivieren klicken)",
  "skills.toggle.off": "Deaktiviert (zum Aktivieren klicken)",
  "skills.noDesc": "(keine Beschreibung)",
  "skills.viewSkillMd": "SKILL.md ansehen",
  "skills.uninstall": "Deinstallieren",
  "skills.status.enabling": "{name} wird aktiviert…",
  "skills.status.disabling": "{name} wird deaktiviert…",
  "skills.status.uninstalling": "{name} wird deinstalliert…",
  "skills.installed": "Installiert",
  "skills.install": "Installieren",
  "skills.status.installing": "{name} wird installiert…",
  "skills.status.installed": "{name} installiert",
  "skills.status.installFailed": "Installation fehlgeschlagen: {e}",
  "skills.empty.mine":
    "Noch keine Skills. Installiere über den „Markt“, „Aus Git importieren“, „Von openclaw migrieren“ oder „Skill erstellen“.",
  "skills.empty.market":
    "Keine installierbaren Skills aus dieser Quelle (evtl. nicht erreichbar oder keine Suchergebnisse). Versuche eine andere Quelle oder leere die Suche.",
  "skills.customSource": "Eigene Quelle…",
  "skills.loadingMarket": "Markt wird geladen…",
  "skills.marketLoadFailed": "Markt konnte nicht geladen werden: {e}",
  "skills.switchingSource": "Quelle wird gewechselt…",
  "skills.customSourcePrompt": "Eigene Registry-Quell-URL (statisches JSON):",
  "skills.cannotRead": "(nicht lesbar)",
  "skills.import.title": "Skills aus Git importieren",
  "skills.import.urlLabel": "Git-Repository-URL",
  "skills.import.subLabel": "Unterverzeichnis (optional)",
  "skills.import.subPlaceholder": "Standard: skills",
  "skills.import.note":
    "Unterstützt Import ganzer Repos oder einzelner Skills; <code>@file</code>-Referenzen werden automatisch eingebettet.",
  "skills.import.ok": "Importieren",
  "skills.import.needUrl": "Bitte die Git-URL eingeben",
  "skills.import.cloning": "Klonen und importieren…",
  "skills.migrate.title": "Skills von openclaw migrieren",
  "skills.migrate.note":
    "Übliche openclaw-Skill-Orte automatisch erkannt (~/.config/openclaw, .agents/skills usw.). Auswählen, um nach WiseCortex zu importieren.",
  "skills.migrate.scanning": "Wird gescannt…",
  "skills.migrate.importSelected": "Auswahl importieren",
  "skills.migrate.empty":
    "Keine openclaw-Skills erkannt. Versuche stattdessen „Aus Git importieren“.",
  "skills.migrate.exists": "Bereits vorhanden",
  "skills.migrate.needOne": "Bitte mindestens eines auswählen",
  "skills.migrate.importing": "Wird importiert…",
  "skills.migrate.done": "{n}/{total} Skills migriert",
  "skills.creator.step.basic": "Grundlagen",
  "skills.creator.step.trigger": "Auslöser",
  "skills.creator.step.body": "Skill-Inhalt",
  "skills.creator.step.tools": "Tools",
  "skills.creator.step.preview": "Vorschau & speichern",
  "skills.creator.prev": "Zurück",
  "skills.creator.next": "Weiter",
  "skills.creator.slugLabel": "Skill-Slug (für invoke, kebab-case)",
  "skills.creator.descLabel": "Einzeilige Beschreibung",
  "skills.creator.descPlaceholder": "git-Commits und Issues zu einem Wochenbericht aggregieren",
  "skills.creator.triggerLabel": "Wann verwenden (Auslöser, natürliche Sprache)",
  "skills.creator.triggerPlaceholder":
    "Wenn ein Wochenbericht / eine Arbeitszusammenfassung erwähnt wird",
  "skills.creator.bodyLabel": "Skill-Inhalt (Markdown, @path referenziert Dateien)",
  "skills.creator.bodyPlaceholder": "# Schritte\n1. …",
  "skills.creator.toolsLabel": "Tools, die dieser Skill nutzt",
  "skills.creator.mdWhenUse": "## Wann verwenden",
  "skills.creator.mdTools": "## Verfügbare Tools",
  "skills.creator.previewLabel": "SKILL.md-Vorschau:",
  "skills.creator.save": "Skill speichern",
  "skills.creator.needSlug": "Bitte einen Slug eingeben",

  // channels
  "common.copy": "Kopieren",
  "channels.desc": "Einen IM für Zwei-Wege-Chat verbinden oder ausgehende Push-Ziele konfigurieren",
  "channels.name": "Name",
  "channels.saveFailed": "Speichern fehlgeschlagen",
  "channels.platform.feishu.name": "Feishu",
  "channels.platform.feishu.desc": "Ereignis-Abo · Gruppe/DM zweiseitig",
  "channels.platform.wecom.name": "WeCom",
  "channels.platform.wecom.desc": "Verschlüsselter Callback · App-Nachrichten",
  "channels.platform.onebot.name": "QQ",
  "channels.platform.onebot.desc": "OneBot / NapCat · Gruppen und DMs",
  "channels.platform.email.name": "E-Mail",
  "channels.platform.email.desc": "SMTP-Versand · ausgehende Benachrichtigungen",
  "channels.platform.webhook.name": "Generischer Webhook",
  "channels.platform.webhook.desc": "Nur ausgehend · Push an beliebigen HTTP-Endpunkt",
  "channels.callback.feishuSummary":
    "Öffentliches Deployment (erweitert): Callback-URL für das Ereignis-Abo",
  "channels.callback.feishuNote":
    "Nur wenn der Server eine öffentliche Adresse hat. Auf einer lokalen Maschine die „Long Connection“ unten nutzen (keine öffentliche Adresse nötig).",
  "channels.callback.wecomHint":
    " (öffentliches HTTPS erforderlich; in „Nachrichten empfangen“ der WeCom-App eintragen)",
  "channels.callback.genericHint": " (in der Konsole der Plattform eintragen)",
  "channels.callback.label": "Eingehende Callback-URL",
  "channels.appPushTarget": "App-Push → {target}",
  "channels.notConfigured": "Nicht konfiguriert. ",
  "channels.notConfigured.webhook": "Ein ausgehendes Webhook-Ziel hinzufügen.",
  "channels.notConfigured.generic":
    "Nach dem Verbinden kannst du pushen und Zwei-Wege-Chat führen.",
  "channels.badge.configured": "Konfiguriert",
  "channels.badge.notConnected": "Nicht verbunden",
  "channels.feishu.scan": "Per QR verbinden",
  "channels.feishu.lcOn": "Long Conn: an (keine öffentliche Adresse)",
  "channels.feishu.lcOff": "Long Conn: aus",
  "channels.feishu.lcTitle":
    "Long Connection (WebSocket, kein öffentlicher Callback) — Nachrichten kommen hierüber; der Schalter wirkt sofort, kein Neustart",
  "channels.feishu.lcOnStatus": "Long Connection aktiviert (verbindet sich in wenigen Sekunden)",
  "channels.feishu.lcOffStatus": "Long Connection deaktiviert (trennt sich in wenigen Sekunden)",
  "channels.feishu.appPush": "App für Push wiederverwenden",
  "channels.feishu.appPushTitle":
    "Den per QR verbundenen Feishu-App-Bot für ausgehenden Push nutzen (in geplanten Aufgaben wählbar); kein eigener Bot nötig",
  "channels.feishu.configOutbound": "Ausgehenden Push konfigurieren",
  "channels.wecom.credsSet": "Empfangs-Zugangsdaten: gesetzt",
  "channels.wecom.credsConfig": "Empfangs-Zugangsdaten konfigurieren",
  "channels.addTarget": "Ziel hinzufügen",
  "channels.connect": "{name} verbinden",
  "channels.delete": "{name} löschen",
  "channels.copyOk": "Callback-URL kopiert",
  "channels.copyFail": "Kopieren fehlgeschlagen, bitte den Text manuell markieren und kopieren",
  "channels.faPush.recentHint":
    "Unten stehen aktuelle Chats, die dem Bot geschrieben haben — wähle einfach einen.",
  "channels.faPush.noRecentHint":
    "Noch keine aktuellen Chats — schreibe dem Bot zuerst in Feishu (in einer Gruppe @ oder per DM), dann komm zurück und aktualisiere. Du kannst auch eine chat_id direkt einfügen.",
  "channels.faPush.title": "Feishu-App-Push (QR-Verbindung wiederverwenden)",
  "channels.faPush.namePlaceholder": "z. B. „Dev-Gruppe Push“",
  "channels.faPush.chatLabel": "Ziel-chat_id",
  "channels.faPush.chatPlaceholder": "oc_… (Gruppe) / aktuellen Chat wählen",
  "channels.faPush.required": "Name und Zielchat sind erforderlich",
  "channels.config.smtpHost": "SMTP-Server (host:port)",
  "channels.config.onebotBase": "OneBot-HTTP-Basis-URL",
  "channels.config.smtpPlaceholder": "z. B. smtp.example.com:465",
  "channels.config.recipients": "Empfänger (kommagetrennt)",
  "channels.config.username": "Benutzername",
  "channels.config.usernamePlaceholder": "SMTP-Login, meist die Absender-E-Mail",
  "channels.config.password": "Passwort / App-Passwort",
  "channels.config.passwordPlaceholder": "SMTP-Passwort oder App-Passwort",
  "channels.config.from": "Absender (leer = Benutzername)",
  "channels.config.emailNote":
    "Port 465 = implizites TLS, 587 = STARTTLS. Zugangsdaten werden nur im lokalen Backend gespeichert.",
  "channels.config.title": "{name} konfigurieren",
  "channels.config.namePlaceholder": "z. B. „Dev-Gruppe“",
  "channels.config.groupLabel": "Gruppennummer (target)",
  "channels.config.groupPlaceholder": "Gruppennummer, optional",
  "channels.config.callbackNote":
    "Zwei-Wege-Chat: Trage die obige Callback-URL in die Konsole von {name} ein und konfiguriere die App-Zugangsdaten per CLI.",
  "channels.config.required": "Name und Adresse sind erforderlich",
  "channels.scan.title": "Feishu per QR verbinden",
  "channels.scan.generating": "QR-Code wird erstellt…",
  "channels.scan.wait": "Bitte warten…",
  "channels.scan.note":
    "Mit der <strong>Feishu / Lark App</strong> scannen, um zu autorisieren und die App zu erstellen; app_id / app_secret werden bei Erfolg automatisch ausgefüllt.<br/>Das Scannen <strong>erstellt nur die App und holt Zugangsdaten</strong>. Zum Empfangen von Nachrichten zusätzlich in der Feishu-Entwicklerkonsole: ① <code>im:message</code> unter Berechtigungen hinzufügen; ② beim Ereignis-Abo „Long Connection“ wählen und „Nachrichten empfangen“ abonnieren; ③ eine Version veröffentlichen. Dann auf dieser Seite „Long Connection“ aktivieren und den Server neu starten.",
  "channels.scan.failed": "QR-Ablauf kann nicht gestartet werden: {e}",
  "channels.scan.prompt": "Mit der Feishu / Lark App scannen, um zu autorisieren…",
  "channels.scan.connected": "Verbunden! app_id={id}",
  "channels.scan.denied": "Autorisierung verweigert; schließen und erneut versuchen.",
  "channels.scan.expired": "QR-Code abgelaufen; schließen und erneut scannen.",
  "channels.scan.error": "Fehler: {e}",
  "channels.wecom.title": "WeCom · Empfangs-Zugangsdaten",
  "channels.wecom.corpId": "Corp-ID (corp_id)",
  "channels.wecom.secret": "App Secret (corp_secret)",
  "channels.wecom.secretPlaceholder": "leer = unverändert",
  "channels.wecom.agentId": "App AgentId (agent_id)",
  "channels.wecom.agentIdPlaceholder": "z. B. 1000002",
  "channels.wecom.token": "Callback-Token (callback_token)",
  "channels.wecom.tokenPlaceholder": "der Token in „Nachrichten empfangen“ der Konsole",
  "channels.wecom.aesKey": "Callback-EncodingAESKey",
  "channels.wecom.aesPlaceholder": "43 Zeichen, leer = unverändert",
  "channels.wecom.note":
    'In der WeCom-Konsole → App → „Nachrichten empfangen“ den API-Empfang einstellen: URL = <span class="mono">{cb}</span> (muss über öffentliches HTTPS erreichbar sein), Token / EncodingAESKey wie hier. Zugangsdaten werden nur im lokalen Backend gespeichert.',
  "channels.wecom.savedReady": "WeCom-Zugangsdaten gespeichert (bereit)",
  "channels.wecom.savedIncomplete": "WeCom-Zugangsdaten gespeichert (noch fehlende Felder)",
  "channels.platform.qqbot.name": "QQ-Bot (offiziell)",
  "channels.platform.qqbot.desc":
    "QQ Open Platform · AppID/Secret · Gateway-Verbindung, keine öffentliche Adresse",
  "channels.qq.scan": "Per QR verbinden (empfohlen)",
  "channels.qq.creds": "Zugangsdaten manuell eingeben",
  "channels.qq.credsSet": "Zugangsdaten: gesetzt",
  "channels.qq.connect": "Gateway: aus · zum Verbinden klicken",
  "channels.qq.disconnect": "Gateway: an · zum Trennen klicken",
  "channels.qq.toggleTitle":
    "QQ-Gateway (WebSocket, kein öffentlicher Callback) — Umschalten wirkt sofort, kein Neustart",
  "channels.qq.onStatus": "QQ-Gateway aktiviert (verbindet in wenigen Sekunden)",
  "channels.qq.offStatus": "QQ-Gateway deaktiviert",
  "channels.qq.appId": "AppID",
  "channels.qq.scanTitle": "QQ-Bot per QR verbinden",
  "channels.qq.scanNote":
    "Mit der <strong>QQ-App auf dem Handy</strong> scannen und den zu verbindenden Bot wählen — AppID/AppSecret werden automatisch ausgefüllt; keine Serveradresse nötig. Die Verbindungsseite wird von Tencent gehostet und zeigt den Integrator standardmäßig als „Drittanbieter-Bot“.",
  "channels.qq.scanPrompt": "Mit dem Handy-QQ scannen, um zu verbinden…",
  "channels.qq.scanConnected": "Verbunden! AppID={id}",
  "channels.qq.title": "QQ-Bot-Zugangsdaten",
  "channels.qq.appSecret": "AppSecret",
  "channels.qq.appSecretPlaceholder": "leer = aktuellen Wert behalten",
  "channels.qq.note":
    "Erstelle einen Bot auf q.qq.com und kopiere AppID/AppSecret von der Einstellungsseite. Keine Serveradresse nötig — die Verbindung läuft ausgehend über ein WebSocket-Gateway. Schließt das Gateway mit Code 4914, fehlt dem Bot die Berechtigung für Gruppen-/DM-Nachrichten auf der Plattform.",
  "channels.qq.savedReady": "Gespeichert (Zugangsdaten bereit, zum Verbinden klicken)",
  "channels.qq.savedIncomplete": "Gespeichert (Zugangsdaten unvollständig)",
  "channels.platform.clawbot.name": "WeChat ClawBot",
  "channels.platform.clawbot.desc": "iLink Long-Poll · privater Chat, keine öffentliche Adresse",
  "channels.clawbot.botId": "Bot-ID",
  "channels.clawbot.scan": "Per QR verbinden",
  "channels.clawbot.scanTitle": "WeChat ClawBot verbinden",
  "channels.clawbot.scanNote":
    "Mit der <strong>WeChat-App auf dem Handy</strong> scannen und bestätigen. Pro WeChat-Konto gibt es genau einen Bot, 1:1 an dich gebunden. Text und Sprache (serverseitige Transkription) funktionieren; Bilder und Dateien noch nicht.",
  "channels.clawbot.scanPrompt": "Mit WeChat scannen und am Handy bestätigen…",
  "channels.clawbot.scanConnected": "Verbunden! {id}",
  "channels.clawbot.connect": "Polling: aus · zum Starten klicken",
  "channels.clawbot.disconnect": "Polling: an · zum Stoppen klicken",
  "channels.clawbot.toggleTitle":
    "iLink Long-Poll (kein öffentlicher Callback) — wirkt sofort, kein Neustart nötig",
  "channels.clawbot.onStatus": "WeChat-Polling gestartet",
  "channels.clawbot.offStatus": "WeChat-Polling gestoppt",
  "channels.clawbot.soloNote":
    "Nur an <strong>einer Stelle</strong> aktivieren. Der Sync-Cursor gilt pro Bot — pollen zwei Rechner gleichzeitig, werden deine Nachrichten zufällig zwischen ihnen aufgeteilt.",
  "tasks.form.chanGroupFeishu": "Feishu-App",
  "tasks.form.chanGroupQq": "QQ-Bot",
  "tasks.form.feishuChatOpt": "Feishu · {id}",
  "tasks.form.qqC2cOpt": "QQ-DM · {id}",
  "tasks.form.qqGroupOpt": "QQ-Gruppe · {id}",
} satisfies Record<MessageKey, string>;
