# AGENTS.md

native-layer（crate、CLI、JS 模块都叫 `nlayer`）是一个 TSX 应用在 web、iOS、Android 上跑的运行时。机制从 [nsh](https://github.com/longbridge/nsh) 抽进新 crate，不保证跑现有 nsh 应用，也不从零重写渲染器。来源和 Apache-2.0 归属在 `NOTICE`。抽代码的起点是 nsh `ca59368b`。

这份文件是范围。和它冲突的实现改范围，不改文件来迁就实现。

## v1 完成的标准

同一个 TSX 应用，用 `nlayer init` / `build` / `run --target web|ios|android` 在三端跑起来。`nlayer test` 在无窗口引擎里跑 TSX，断言 mount 树、文本和布局。验收屏至少覆盖：纵向布局、嵌套 `Text` span、打包图片、双轴 `Scroll`、单行受控 `Input`、`onPress`、一次 `fetch` JSON、safe area 的 `env()`。

## 不要做

这些不是「以后再补的钩子」，是 v1 明确排除的。不要为了让拷来的代码编译而把它们搬进来，也不要加一层空 API「先留着」。

- Rust 写 app、`#[component]` 应用模板、扩展注册、嵌入 ABI、签名包、`nlayer package`、HMR、`nsh-dev` 协议
- 桌面后端。以后若做桌面，只加 GPUI，吃同一套 mount batch。不建 AppKit / Win32 / GTK
- Nav、List、Canvas、Shader、Video、WebView、动画图、手势系统、长按
- Tailwind utility 表。`class` / `classList` 只并进同一套样式 IR；v1 解析器为空
- Node 兼容：`buffer` / `fs` / `process` / `path` / `net` / `os` / `zlib`，以及 streams、`SubtleCrypto`、WebSocket、storage
- 深色模式、自定义字体、无障碍 API、SVG、GIF、多行输入、文本选择、链接、行内视图、IME 事件回 JS
- `nlayer check`、内置 tsc、三端 UI 自动化、设备像素对齐的 headless 断言
- 公开扩展钩子。以后扩展靠封闭 tag 集上的可加注册，v1 不留

拷来的 IR 里可以残留 grid、阴影、渐变、transform、`AnimationProp` 名字。那是为了不重写 apply 路径。不要据此去实现这些能力，也不要在公开样式、mount 协议或 JS 模块里暴露它们。

## 怎么搬

抽选中的子系统进新 crate。不是 fork 整棵 nsh 再删，也不是按同一设计重写。

搬：reactive 图、JSX lowering（`element` / `insert` / `spread` / `cond` / `each`）、mount batch、引擎线程、Taffy 布局和 nsh/RN 默认值、web / iOS / Android 上 `View` / `Text` / `Image` / `Scroll` / 单行 `Input` 的宿主路径、WinterTC 最小集加 `fetch`。

不搬：`nsh-base`、Tailwind 表、桌面后端、GPUI、Node catalog、HMR、签名、嵌入 ABI、Rust app 模板、shader、canvas、list、动画图。拷的时候就裁，不要整文件进来再删。`ops.rs` 按 v1 协议裁。`nsh-macros` 只在元素解码真正需要时搬 `PropEnum`，不搬 `assets` / `native_module`。

每搬一个文件，在 `NOTICE` 写上来源路径。拷来的文件保留 Apache-2.0 声明；新仓库也是 Apache-2.0，不另起许可证。

缺的依赖是缺的。用最小的真实类型把已拷文件编译过去可以，用整子系统去填一个字段不行。`todo!()`、空函数、未接入的桩不算搬完。

## 已定的产品面

- 宿主：`View`、`Text`、`Image`、`Scroll`、`Input`。`onPress`。`If` / `Each`。图：`signal`、`computed`、`effect`、`batch`、`untracked`、`cleanup`、`context`，外加 `ErrorBoundary`。无 `resource`。函数名沿用 nsh，不是 `createSignal`。
- 文本：嵌套 `Text` 是同一段的 span（颜色、字重、字号、斜体），继承父级文本样式。
- 输入：单行受控 `value` / `onChange` / `placeholder` / `disabled`，加 `secureTextEntry` 和 `keyboardType`（`text` / `email` / `number` / `phone`）。
- 样式：Taffy。未写 `display` 为纵向 flex，`flexShrink` 为 0，无 `min-width: auto`。公开子集是盒模型、flex、relative/absolute、尺寸、margin/padding（含 `env()`）、纯色背景、border、radius、opacity、overflow、zIndex、列出的文本样式、`objectFit`。`activeStyle` / `hoverStyle` / `focusStyle` 进协议，由平台应用。未实现的 key 和任何 `class` 在终端报错且不生效，不拆引擎。
- 滚动：原生，两轴。无刷新、缩放、分页。
- 图片：打包 `import` 加 `http(s)`，PNG / JPEG / WebP。系统字体。
- 平台 API：`console`、timers、`queueMicrotask`、`URL` / `URLSearchParams`、`TextEncoder` / `TextDecoder`、`AbortController`、`atob` / `btoa`、`performance.now`、`crypto.getRandomValues`、`fetch` / `Request` / `Response` / `Headers`。body 只有 text / json / arrayBuffer / blob。`Blob` 只作为这个结果类型。`crypto.subtle` 是缺的，不是空桩。`fetch` 任意 URL，跟随重定向，不带 cookie，认 `AbortSignal`。无 capability grant。
- UI 环境留在 `@nlayer`：`platform.os` 为 `web` | `ios` | `android`，safe area，keyboard inset。只通过 `env()` 进入 padding/margin。
- 运行：QuickJS。Web 是同一套 WASM 引擎加 DOM，不是第二条 JS 运行时。引擎线程产出 batch，平台主线程挂原生视图。`run` 监视 TSX 和资源，重建后重启引擎，状态丢掉。编译失败只打在终端，上一份成功的引擎继续跑。无 HMR。
- 测试：headless 跑 TSX。文本度量用确定性替身，不对齐真机像素。
- 底线：iOS 15+，Android API 28+。`run` 用模拟器或浏览器，不重装 App。构建不需要 Node。`build` 写出 bundle、类型和 source map。

名字：仓库 `native-layer`，包和 import `@nlayer`，JSX runtime `@nlayer/jsx-runtime`，内部模块 `nlayer:rt`，二进制 `nlayer`，crate 前缀 `nlayer-`。

## 仓库

计划中的 crate：`nlayer-core`、`nlayer-js`、`nlayer-engine`、`nlayer-bundler`、`nlayer-platform-web`、`nlayer-platform-ios`、`nlayer-platform-android`、`nlayer-cli`、`nlayer-testing`。没有 `nlayer-dev`，没有 `nlayer-package`，没有 `nlayer-base`。

已经在 `nlayer-core` 里的：reactive 图、几何单位、样式 IR、attributed text。还没有阴影树、mount batch、JS 宿主、引擎、打包器、三端宿主和 CLI。不要在这些还不存在时写调用它们的桩。

## 改代码时

- 先读这份文件，再读要改的模块的 `//!`。
- 一个提交是一个做完的切片：行为能跑，测试是绿的，没有 `todo!()`、空实现或「先留着」的桩。红的树不提交。
- 现在的验证是 `cargo test -p nlayer-core`。`nlayer test` 落地之后，动到 TSX、引擎或宿主的切片还要它是绿的。不要用空测试代替。
- 拷来的文件保留行为。改名和裁掉 v1 不用的入口可以，重写算法不行。
