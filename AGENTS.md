# AGENTS.md

native-layer（crate、CLI、JS 模块都叫 `nlayer`）是一个 TSX 应用在 web、iOS、Android 上跑的运行时。机制从 [nsh](https://github.com/longbridge/nsh) 抽进新 crate，不保证跑现有 nsh 应用，也不从零重写渲染器。来源和 Apache-2.0 归属在 `NOTICE`。抽代码的起点是 nsh `ca59368b`。

本文件是不做的事和搬运纪律。产品范围在 `docs/v1.md`（验收、产品面、公开模块导出）。两份冲突时，不做的事以本文件为准，产品行为以 `docs/v1.md` 为准。`docs/v1.md` 不能把本文件禁止的东西写成 v1。不要把 `docs/v1.md` `@` 进本文件。

## 不要做

v1 不做的分三档。三档都不要为了让拷来的代码编译而搬进来，也不要加一层空 API「先留着」。

### 以后要做

v1 不做。做到那一层再搬，不留钩子。

- 宿主：Nav、List、Canvas、Video、WebView、动画图、手势系统、长按
- 深色模式、无障碍 API、SVG、GIF、多行输入、文本选择、链接、行内视图、IME 事件回 JS。`testID` 不是无障碍 API
- 阴影、渐变的绘制。`AnimationProp` 跟动画图走，v1 不暴露
- Tailwind utility 表。`class` / `classList` 仍并进同一套样式 IR；v1 解析器为空，未知 `class` 警告且不生效
- `crypto.subtle`、WebSocket、storage。v1 里 `crypto.subtle` 是缺的，不是空桩
- `resource`。滚动的刷新、缩放、分页。`fetch` 的 cookie
- 嵌入 ABI、`nlayer package`、HMR、`nsh-dev` 协议
- 设备像素对齐的 headless 断言

### 永久不做

- Rust 写 app，以及 `#[component]`。应用只写 TSX。它不是应用模板，也不是写原生组件的宏
- Shader
- 不提供 Node 内建，也不提供 CJS / `require` 兼容层：`buffer` / `fs` / `process` / `path` / `net` / `os` / `zlib`、streams。不为了跑 npm 包加 shim
- 内置 tsc。构建不需要 Node
- capability grant。`fetch` 任意 URL，不经 grant。nsh 的 web `fetch` 经 wasm 交回页面再发，是为了这条 grant；这里没有 grant，不要把那条桥搬回来

### 不预承诺

v1 不做。不写后期做不做，也不写后期方案。

- 公开扩展钩子。v1 不留。不写闭合 tag 集，也不写开放注册
- 桌面后端。不写 GPUI，也不写 AppKit / Win32 / GTK
- 自定义字体。v1 只用系统字体
- 签名包
- `nlayer check`

拷来的 IR 可以残留宿主不画的名字，避免重写 apply 路径。残留不是绘制承诺，也不是后期承诺。不认识的样式名警告且不生效，不另设拒绝白名单。阴影、渐变的绘制、`AnimationProp` 是上面的「以后要做」；其余残留（filter、backdrop、mask、blend、outline、glass、transition、内置 keyframes，以及别的未点名 key）不预承诺。

## 怎么搬

抽选中的子系统进新 crate。不是 fork 整棵 nsh 再删，也不是按同一设计重写。

搬：reactive 图、JSX lowering（`element` / `insert` / `spread` / `cond` / `each`）、mount batch、引擎线程、Taffy 布局（含 grid）和 nsh/RN 默认值、web / iOS / Android 上 `View` / `Text` / `Image` / `Scroll` / 单行 `Input` 的宿主路径、静态 transform、WinterTC 最小集加 `fetch`。

不搬：`nsh-base`、Tailwind 表、桌面后端、GPUI、Node catalog、HMR、签名、嵌入 ABI、Rust app 模板、shader、canvas、list、动画图，以及 web 的 fetch 宿主桥。这是现在的裁剪，不是上面三档的替代。拷的时候就裁，不要整文件进来再删。`ops.rs` 按 v1 协议裁。`nsh-macros` 只在元素解码真正需要时搬 `PropEnum`。`assets`、`native_module`、`#[component]` 不搬，也不是后期目标。

每搬一个文件，在 `NOTICE` 写上来源路径。拷来的文件保留 Apache-2.0 声明；新仓库也是 Apache-2.0，不另起许可证。

缺的依赖是缺的。用最小的真实类型把已拷文件编译过去可以，用整子系统去填一个字段不行。`todo!()`、空函数、未接入的桩不算搬完。

## 名字

仓库 `native-layer`，二进制 `nlayer`，crate 前缀 `nlayer-`。应用源码不写裸 `@nlayer`。公开两个原生模块，互不重导出：`@nlayer/runtime` 和 `@nlayer/ui`。导出名单在 `docs/v1.md`。`@nlayer/jsx-runtime` 只有类型，没有实现：`jsx` 为 `react-jsx`，`jsxImportSource` 为 `@nlayer`，这个裸 scope 只写在 tsconfig。编译后的 JSX 导入内部模块 `nlayer:rt`，应用不写它，也不写 `@nlayer/jsx-runtime`。v1 没有 `@nlayer/jsx-dev-runtime`。

## 仓库

只有这些 crate：`nlayer-core`、`nlayer-js`、`nlayer-engine`、`nlayer-bundler`、`nlayer-platform-web`、`nlayer-platform-ios`、`nlayer-platform-android`、`nlayer-cli`、`nlayer-testing`。没有 `nlayer-dev`，没有 `nlayer-package`，没有 `nlayer-base`。`nlayer package` 和 `nsh-dev` 协议以后要做，v1 不为此加 crate。

## 改代码时

- 先读本文件，再读要改的模块的 `//!`。动到产品行为（宿主、样式、平台 API、模块导出、验收）先读 `docs/v1.md`。
- 产品范围要变：先改 `docs/v1.md`，或与实现在同一切片里改。文件写目标契约，不写做到哪了。
- 交给子代理的任务写明相关的不做事项，以及 `docs/v1.md` 里相关的产品范围。子代理不加载 `AGENTS.md`。
- 提交前跑 `scripts/fmt`。非 Rust 源码（JS、TS、TSX、CSS、HTML、JSON、TOML、Java）必须已格式化。Markdown 不进这套：`bun fmt` 会改写行内代码。
- 一个提交是一个做完的切片：行为能跑，测试是绿的，没有 `todo!()`、空实现或「先留着」的桩。红的树不提交。
- 改过的 crate 跑 `cargo test -p`。动到 TSX、引擎或宿主的切片还要 `nlayer test` 是绿的。不要用空测试代替。
- 拷来的文件保留行为。改名和裁掉 v1 不用的入口可以，重写算法不行。
