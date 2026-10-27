# AGENTS.md

native-layer（crate、CLI、JS 模块都叫 `nlayer`）是一个 TSX 应用在 web、iOS、Android 上跑的运行时。机制从 [nsh](https://github.com/longbridge/nsh) 抽进新 crate，不保证跑现有 nsh 应用，也不从零重写渲染器。来源和 Apache-2.0 归属在 `NOTICE`。抽代码的起点是 nsh `ca59368b`。

这份文件是范围。和它冲突的实现改范围，不改文件来迁就实现。

## v1 完成的标准

同一个 TSX 应用，用 `nlayer init` / `build` / `run --target web|ios|android` 在三端跑起来。`nlayer test` 在无窗口引擎里跑 TSX，断言 mount 树、文本和布局。验收屏至少覆盖：纵向布局、一段 grid、嵌套 `Text` span、打包图片、双轴 `Scroll`、单行受控 `Input`、`onPress`、一处静态 transform、一次 `fetch` JSON、safe area 的 `env()`。

## 不要做

v1 不做的分三档。三档都不要为了让拷来的代码编译而搬进来，也不要加一层空 API「先留着」。

### 以后要做

v1 不做。做到那一层再搬，不留钩子。

- 宿主：Nav、List、Canvas、Video、WebView、动画图、手势系统、长按
- 深色模式、无障碍 API、SVG、GIF、多行输入、文本选择、链接、行内视图、IME 事件回 JS
- 阴影、渐变。`AnimationProp` 跟动画图走，v1 不暴露
- Tailwind utility 表。`class` / `classList` 仍并进同一套样式 IR；v1 解析器为空，未知 `class` 报错且不生效
- `crypto.subtle`、WebSocket、storage。v1 里 `crypto.subtle` 是缺的，不是空桩
- `resource`。滚动的刷新、缩放、分页。`fetch` 的 cookie
- 嵌入 ABI、`nlayer package`、HMR、`nsh-dev` 协议
- 三端 UI 自动化、设备像素对齐的 headless 断言

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

拷来的 IR 可以残留白名单外的名字，避免重写 apply 路径。残留不是实现许可，也不是后期承诺。阴影、渐变、`AnimationProp` 是上面的「以后要做」；其余残留（filter、backdrop、mask、blend、outline、glass、transition、内置 keyframes，以及别的未点名 key）不预承诺。

## 怎么搬

抽选中的子系统进新 crate。不是 fork 整棵 nsh 再删，也不是按同一设计重写。

搬：reactive 图、JSX lowering（`element` / `insert` / `spread` / `cond` / `each`）、mount batch、引擎线程、Taffy 布局（含 grid）和 nsh/RN 默认值、web / iOS / Android 上 `View` / `Text` / `Image` / `Scroll` / 单行 `Input` 的宿主路径、静态 transform、WinterTC 最小集加 `fetch`。

不搬：`nsh-base`、Tailwind 表、桌面后端、GPUI、Node catalog、HMR、签名、嵌入 ABI、Rust app 模板、shader、canvas、list、动画图，以及 web 的 fetch 宿主桥。这是现在的裁剪，不是上面三档的替代。拷的时候就裁，不要整文件进来再删。`ops.rs` 按 v1 协议裁。`nsh-macros` 只在元素解码真正需要时搬 `PropEnum`。`assets`、`native_module`、`#[component]` 不搬，也不是后期目标。

每搬一个文件，在 `NOTICE` 写上来源路径。拷来的文件保留 Apache-2.0 声明；新仓库也是 Apache-2.0，不另起许可证。

缺的依赖是缺的。用最小的真实类型把已拷文件编译过去可以，用整子系统去填一个字段不行。`todo!()`、空函数、未接入的桩不算搬完。

## 已定的产品面

- 宿主：`View`、`Text`、`Image`、`Scroll`、`Input`。`onPress`。`If` / `Each`。图：`signal`、`computed`、`effect`、`batch`、`untracked`、`cleanup`、`context`，外加 `ErrorBoundary`。v1 无 `resource`（以后要做，不留）。函数名沿用 nsh，不是 `createSignal`。
- 文本：嵌套 `Text` 是同一段的 span（颜色、字重、字号、斜体），继承父级文本样式。
- 输入：单行受控 `value` / `onChange` / `placeholder` / `disabled`，加 `secureTextEntry` 和 `keyboardType`（`text` / `email` / `number` / `phone`）。
- 样式：Taffy。未写 `display` 为纵向 flex，`flexShrink` 为 0，无 `min-width: auto`。公开子集是白名单：盒模型、flex、relative/absolute、尺寸、margin/padding（含 `env()`）、纯色背景、border、radius、opacity、overflow、zIndex、列出的文本样式、`objectFit`、静态 transform（`transform` / `translate` / `rotate` / `scale` / `transformOrigin`）、grid（`display: "grid"`、`gridTemplateColumns` / `gridTemplateRows`、`gridAutoColumns` / `gridAutoRows`、`gridAutoFlow`、`gridColumn` / `gridRow` 及其 `Start` / `End`，含 line names）。`activeStyle` / `hoverStyle` / `focusStyle` 进协议，由平台应用。白名单外的 key，不管 IR 里有没有、apply 接没接，都在终端报错且不生效，不进 mount 协议或 JS 模块。任何 `class` 同样报错且不生效，不拆引擎。
- 滚动：原生，两轴。v1 无刷新、缩放、分页（以后要做，不留）。
- 图片：打包 `import` 加 `http(s)`。v1 是 PNG / JPEG / WebP（SVG、GIF 以后要做）。系统字体（自定义字体不预承诺）。
- 平台 API：`console`、timers、`queueMicrotask`、`URL` / `URLSearchParams`、`TextEncoder` / `TextDecoder`、`AbortController`、`atob` / `btoa`、`performance.now`、`crypto.getRandomValues`、`fetch` / `Request` / `Response` / `Headers`。body 只有 text / json / arrayBuffer / blob。`Blob` 只作为这个结果类型。`crypto.subtle` 是缺的，不是空桩（以后要做）。`fetch` 任意 URL，跟随重定向，认 `AbortSignal`。v1 不带 cookie（以后要做）。无 capability grant（永久）。QuickJS 宿主（iOS、Android、`nlayer test`）由引擎发请求。Web 上应用在页面里跑，`fetch` 就是浏览器的，不进 wasm
- UI 环境留在 `@nlayer/ui`：`platform.os` 为 `web` | `ios` | `android`，safe area，keyboard inset。只通过 `env()` 进入 padding/margin。
- 运行：iOS、Android 和 `nlayer test` 用 QuickJS。Web 不把 QuickJS 打进 wasm：页面执行应用的 ES module，wasm 只跑 reactive 图、布局和 mount batch，DOM 由页面挂。应用的 `fetch` 留在页面，不经引擎。引擎线程产出 batch，平台主线程挂原生视图。`run` 监视 TSX 和资源，重建后重启引擎，状态丢掉。编译失败只打在终端，上一份成功的引擎继续跑。v1 无 HMR（以后要做）；监视重建、重启、丢状态仍是 v1。
- 测试：headless 跑 TSX。文本度量用确定性替身。v1 不对齐真机像素；像素对齐断言和三端 UI 自动化以后要做。
- 底线：iOS 15+，Android API 28+。`run` 用模拟器或浏览器，不重装 App。构建不需要 Node。`build` 写出 bundle、类型和 source map。

名字：仓库 `native-layer`，二进制 `nlayer`，crate 前缀 `nlayer-`。应用源码不写裸 `@nlayer`。公开两个原生模块，互不重导出：`@nlayer/runtime`（`signal`、`computed`、`effect`、`batch`、`untracked`、`cleanup`、`context`、`mount`、`ErrorBoundary`）和 `@nlayer/ui`（`View`、`Text`、`Image`、`Scroll`、`Input`，以及 `platform`）。`@nlayer/jsx-runtime` 只有类型，没有实现：`jsx` 为 `react-jsx`，`jsxImportSource` 为 `@nlayer`，这个裸 scope 只写在 tsconfig。编译后的 JSX 导入内部模块 `nlayer:rt`，应用不写它，也不写 `@nlayer/jsx-runtime`。v1 没有 `@nlayer/jsx-dev-runtime`。

## 仓库

- 计划中的 crate：`nlayer-core`、`nlayer-js`、`nlayer-engine`、`nlayer-bundler`、`nlayer-platform-web`、`nlayer-platform-ios`、`nlayer-platform-android`、`nlayer-cli`、`nlayer-testing`。没有 `nlayer-dev`，没有 `nlayer-package`，没有 `nlayer-base`。`nlayer package` 和 `nsh-dev` 协议以后要做，v1 不为此加 crate。

已经在 `nlayer-core` 里的：reactive 图、几何单位、样式 IR、attributed text。还没有阴影树、mount batch、JS 宿主、引擎、打包器、三端宿主和 CLI。不要在这些还不存在时写调用它们的桩。

## 改代码时

- 先读这份文件，再读要改的模块的 `//!`。
- 一个提交是一个做完的切片：行为能跑，测试是绿的，没有 `todo!()`、空实现或「先留着」的桩。红的树不提交。
- 现在的验证是 `cargo test -p nlayer-core`。`nlayer test` 落地之后，动到 TSX、引擎或宿主的切片还要它是绿的。不要用空测试代替。
- 拷来的文件保留行为。改名和裁掉 v1 不用的入口可以，重写算法不行。
