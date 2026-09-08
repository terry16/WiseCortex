import { describe, expect, it } from "vitest";
import { backendHost, httpBase, isTauri, wsBase } from "./backend";

const browser = { hostname: "203.0.113.10", protocol: "http:", host: "203.0.113.10" };
const browserTls = { hostname: "app.example.com", protocol: "https:", host: "app.example.com" };
const tauriWin = { hostname: "tauri.localhost", protocol: "http:" };
const tauriProto = { hostname: "", protocol: "tauri:" };

describe("isTauri", () => {
  it("true for tauri.localhost / 空 host / 非 http(s) 协议", () => {
    expect(isTauri(tauriWin)).toBe(true);
    expect(isTauri(tauriProto)).toBe(true);
    expect(isTauri({ hostname: "anything", protocol: "tauri:" })).toBe(true);
  });
  it("false for 正常浏览器 http(s)", () => {
    expect(isTauri(browser)).toBe(false);
    expect(isTauri(browserTls)).toBe(false);
  });
});

describe("backendHost", () => {
  it("uses location.hostname for normal browser http(s)", () => {
    expect(backendHost({ hostname: "localhost", protocol: "http:" })).toBe("localhost");
    expect(backendHost({ hostname: "192.168.1.10", protocol: "https:" })).toBe("192.168.1.10");
  });
  it("falls back to 127.0.0.1 in Tauri-like environments", () => {
    expect(backendHost(tauriWin)).toBe("127.0.0.1");
    expect(backendHost(tauriProto)).toBe("127.0.0.1");
  });
});

describe("httpBase", () => {
  it("浏览器：空串 → REST 走相对路径（同源，经 nginx 反代）", () => {
    expect(httpBase(browser)).toBe("");
    expect(httpBase(browserTls)).toBe("");
  });
  it("Tauri：指向内嵌后端 127.0.0.1:7070", () => {
    expect(httpBase(tauriWin)).toBe("http://127.0.0.1:7070");
    expect(httpBase(tauriProto)).toBe("http://127.0.0.1:7070");
  });
});

describe("wsBase", () => {
  it("浏览器：页面同源，scheme 随 http/https 切 ws/wss", () => {
    expect(wsBase(browser)).toBe("ws://203.0.113.10");
    expect(wsBase(browserTls)).toBe("wss://app.example.com");
  });
  it("Tauri：指向内嵌后端 127.0.0.1:7070", () => {
    expect(wsBase(tauriWin)).toBe("ws://127.0.0.1:7070");
    expect(wsBase(tauriProto)).toBe("ws://127.0.0.1:7070");
  });
});
