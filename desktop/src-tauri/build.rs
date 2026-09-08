fn main() {
    // tauri-build 只为 tauri.conf.json / capabilities / resources 声明了 rerun-if-changed，
    // **唯独没有图标**。后果很隐蔽：换了 icons/icon.ico 之后，cargo 认为 build.rs 的输入
    // 没变、不重新执行它，于是 exe 里嵌的还是旧图标资源——图换了，构建系统却不知道，
    // 只有 cargo clean 或恰好改了 tauri.conf.json 时才会碰巧刷新。
    // 显式把三个平台的图标列进来，让改图必然触发重嵌。
    println!("cargo:rerun-if-changed=icons/icon.ico"); // Windows：嵌进 exe 的窗口/文件图标
    println!("cargo:rerun-if-changed=icons/icon.icns"); // macOS：.app 包图标
    println!("cargo:rerun-if-changed=icons/icon.png"); // Linux/通用
    tauri_build::build();
}
