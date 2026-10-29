# v1 缺口审计

本文不是产品范围。范围仍以 `docs/v1.md` 和 `AGENTS.md` 为准；本文只记录当前实现与 v1 契约之间的差距、验证证据和验收方式。

- 审计基线：`main`，`25152b4`，2026-10-29
- 审计范围：`crates/*`、`packages/*`、`examples/gallery`、`scripts/*`
- 验证环境：macOS 27.0.0 arm64；Xcode iOS 27.1 模拟器（iPhone Duo）；Android emulator-5554；web target 使用本机 Brave（Chromium）
- 维护约定：每修掉一项，删除对应条目或在“状态”里标成已关闭；不要让本文变成第二份产品范围

## 结论

v1 的主功能面已经实现：reactive 图、JSX lowering、引擎线程、bundle、五个宿主组件、三端平台 API、CLI 和类型包都有真实代码，没有 `todo!()` / 空实现。当前剩余工作集中在四类：

1. 明确的契约缺口（绘制）：P0-4 已关闭。
2. 宿主语义偏差（onScroll 节流已关闭；Android transformOrigin 百分比仍开）
3. 验收与回归覆盖（三端 target 测试、gallery 断言、If/Each/ErrorBoundary）
4. 需要定稿的文档歧义（单实例双轴 Scroll、web 上的浏览器全局）

## 验证基线

| 检查 | 结果 |
|---|---|
| `cargo test --workspace` | 全绿；286 个 `#[test]` 函数 |
| `nlayer test examples/gallery/App.test.tsx` | 12/12 |
| `nlayer test examples/gallery/App.test.tsx --target web` | 12/12 |
| `nlayer test examples/gallery/App.test.tsx --target ios` | 12/12 |
| `nlayer test examples/gallery/App.test.tsx --target android` | 12/12 |
| `nlayer build examples/gallery --out /tmp/nlayer-build-check` | 写出 `bundle.js`、`bundle.js.map`、`mark.png` |
| `nlayer init /tmp/nlayer-init-check` | 写出 `App.tsx`、`App.test.tsx`、`tsconfig.json`、`package.json`、`.gitignore` |
| `scripts/test-types` | 五个类型包打包并在临时工程中 `tsc` 通过 |

三端 12/12 是手工运行结果；仓库里没有对应的自动化 target 测试（见 P2-1）。

## P0：明确的契约缺口

| ID | 缺口 | 证据 | 影响 |
|---|---|---|---|
| P0-4 | 已关闭。web 图片 `border-radius: inherit` 加 `overflow: clip`；Android `ImageBox` 按 `Decor` 圆角路径裁剪。树上报 `radius`（point，四角）。web target 断言 16px 圆角图片的计算半径 | 同上 | 三端圆角图片裁到半径 |
| P0-10 | 已关闭。焦点变化沿父链重绘；`focusStyle` 在自身或子孙 field 聚焦时生效。touch mode 下普通 Box 只 `setFocusable`，点击不会聚焦它（与 `HostView` 一致）；键盘或方向键可以。祖先样式不依赖那条路径 | `Mounter.focusChanged` / `chainFocused` | 聚焦输入时祖先 `focusStyle` 亮；树上报 `focused` |

### P0 验收方式


- P0-4：已关闭。`nlayer test --target web` 断言 `radius` 为 `[16, 16, 16, 16]`。Android 树的 `radius` 来自 `onDraw` 使用的 fitted 半径。
- P0-10：已关闭。touch mode 下普通 Box 的 `focusStyle` 不能靠点击点亮，靠内部 field 或非触摸焦点。

## P1：宿主语义与契约定稿

| ID | 项 | 现状 | 证据 |
|---|---|---|---|
| P1-1 | Scroll 是否单实例双轴 | 文档写“两轴”，实现是 `horizontal: bool` 单轴；gallery 用两个 Scroll 分别覆盖两轴并通过 | `crates/nlayer-core/src/mount.rs:27`、`ops.rs:452`；web `base.css:25-32`；Android `ScrollBox.java:15-17,206-217` |
| P1-2 | 已关闭。web 用 `requestAnimationFrame`；iOS / Android 把同一帧的偏移合成一次，结束、拖动和惯性先发出等待中的 `onScroll` | `scroll_frame.rs`；`ScrollBox.queueScroll`；web `queueScroll` | `onScroll` 至多每帧一次 |
| P1-3 | Android `transformOrigin` 百分比在 resize 后陈旧 | pivot 在样式应用时缓存，尺寸变化不重算；只有 Text/分数 transform 会重新发样式 | `crates/nlayer-core/src/core/mod.rs:1486-1494` |
| P1-4 | Android 安装判定只看 APK mtime | 应用被卸载或清数据后不会重装，后续 `run-as` push 失败；iOS 有容器探测 | `crates/nlayer-cli/src/android.rs:658-669`；iOS `ios.rs:281-291` |
| P1-5 | Node 内置模块拒绝不完整 | 只拒绝 `node:*` 和 `events/assert/util/string_decoder/fs/path/os`；`buffer`、`process`、`streams`、`net`、`zlib` 等会落到 rolldown 通用解析错误 | `crates/nlayer-bundler/src/plugin.rs:196-210` |
| P1-6 | 应用源码可写 `nlayer:rt` | 只拒绝 `@nlayer/jsx-runtime`；手写 `nlayer:rt` 会被保留为外部模块 | `crates/nlayer-bundler/src/plugin.rs:160-179`；文档约定见 `AGENTS.md` |
| P1-7 | Web 上 `crypto.subtle` 存在 | QuickJS 宿主中缺失；web target 在浏览器页面执行应用，浏览器全局仍在 | 契约 `docs/v1.md:17`；web 运行模型 `docs/v1.md:19` |

### P1 处理建议

- P1-1：先定稿。若保留单实例单轴，把 `docs/v1.md` 的“两轴”改成“每实例一轴，横竖各一个 Scroll”；若要单实例双轴，需要改 `ScrollConfig`、三个宿主编码器、`scroll()` 测试步骤。
- P1-2：已关闭。同一帧多次偏移只收到最后一次 `onScroll`。
- P1-3：尺寸变化后重发 transform 样式，或在宿主 apply frame 时重算 pivot。
- P1-4：安装前用 `pm path` 或等价探测确认包存在，再决定是否安装。
- P1-5：补全拒绝名单，或至少覆盖 AGENTS 明确列出的 `buffer` / `process` / `path` / `net` / `os` / `zlib` / streams。
- P1-6：区分编译器生成的 `nlayer:rt` 和应用源码手写的 `nlayer:rt`，后者报错。
- P1-7：先定稿 web 是否受 v1 “crypto.subtle 缺失”约束。若受约束，需要在页面启动时遮蔽；若不受，改文档说明该条只适用于 QuickJS 宿主。

## P2：测试与验收覆盖

| ID | 缺口 | 证据 | 影响 |
|---|---|---|---|
| P2-1 | 没有自动化 iOS/Android target 测试 | `crates/nlayer-cli/src/ios_test.rs`、`android_test.rs` 无集成测试；`crates/nlayer-cli/tests/` 只有 `web_target.rs` | 三端 12/12 只是手工结果，回归无保护 |
| P2-2 | gallery 断言偏弱 | `examples/gallery/App.test.tsx` | 水平 Scroll 从未实际滚动；六个滚动回调只触发 `onScroll`；active/hover/focus 只断言基础布局；safe area 只断言 top/left；`platform.os` 接受任意值 |
| P2-3 | `If` 无测试；`Each` 只测默认 key；`ErrorBoundary` 只测组件体抛错 | `crates/nlayer-js/tests/renderer.rs` | lowering 或运行时回归不会被发现 |
| P2-4 | CLI 路径未测 | `crates/nlayer-cli/tests/` | `--filter`、真实 stdin 步骤、`run --target ios|android` 的 watch/rebuild/失败回退、类型版本警告退出码都未覆盖 |
| P2-5 | Web target 输入保真度不足 | `web/host.js:381-439` | press/type/scroll 是合成 DOM 事件，不是 CDP Input；键盘 Return、拖拽、momentum 未被真实浏览器 E2E 覆盖 |
| P2-6 | Android Java 层无测试 | `crates/nlayer-platform-android/java/` | `Mounter`、`ScrollBox`、`InputBox`、`ImageBox` 的设备行为只靠手工验证 |
| P2-7 | iOS 宿主模块被 `cfg(target_os = "ios")` 排除出 `cargo test` | `crates/nlayer-platform-ios/src/lib.rs:24-45` | 设备层只有 15 个纯逻辑测试；mount/host/driver/text 不跑 |

### P2 建议的最小验收集

1. 增加 `ios_target.rs`、`android_target.rs`，用与 `web_target.rs` 相同的最小应用覆盖 press/type/scroll/tree/safe-area。
2. gallery 增加：水平 Scroll 实际滚动、其余五个滚动回调触发、`max(env(a),env(b))`、bottom/right safe area、target 对应的 `platform.os`。
3. 增加 `<If>` 的 when/else/函数 child/分支切换；`<Each key={fn}>`、`key="index"`、乱序重排。
4. `ErrorBoundary` 覆盖 effect/listener 抛错、`reset`、fallback 再抛。
5. CLI 覆盖 `--version`、`--filter`、stdin 步骤、类型版本警告退出码 0。

## P3：清理项

| ID | 项 | 证据 |
|---|---|---|
| P3-1 | `PointerEvents` 解析进样式 IR，但 hit-test 从不读取 | `crates/nlayer-core/src/props.rs`、`style.rs:1044` |
| P3-2 | 未接线的通用 reactive API | `crates/nlayer-core/src/reactive.rs`：`owned_signal`、`set_equals`、`remove_cleanup`、`collecting_effects`、`with_root_under` |
| P3-3 | web 有两份 `ErrorBoundary`，第一份被覆盖 | `crates/nlayer-platform-web/web/browser/runtime.js:106-121` vs `:305-323` |
| P3-5 | test bundle 导出 `finish`，但 `@nlayer/test` 类型未声明 | `crates/nlayer-bundler/src/test_api.rs:9`；`crates/nlayer-bundler/src/types.rs:452-536` |
| P3-6 | web 不绘制 `visibility: hidden` | `crates/nlayer-core/src/style.rs:3618`；web `encode.rs:562-614` |
| P3-7 | Android 丢弃 `skewX` / `skewY` | `crates/nlayer-platform-android/src/encode.rs:323-326`；v1 未点名 skew，属残留 |

## 验收矩阵

`docs/v1.md:7` 列出的验收屏共 13 项。`examples/gallery/App.tsx` 都有对应实现，四端 12 个测试用例通过；下表标出断言的薄弱处。

| 验收项 | 应用位置 | 现有断言 | 剩余风险 |
|---|---|---|---|
| 纵向布局 | `App.tsx:40-63` | 相等 x、递增 y；row 相等 y | 未断言 `flexShrink: 0` / 无 `min-width: auto` |
| grid | `App.tsx:64-84` | 2×2 单元 x/y | 未覆盖 line names、auto flow |
| 嵌套 Text span | `App.tsx:194-199` | 一段文本、字重/斜体/字号/颜色 | gallery 未覆盖 span `testID` / span `onPress` |
| 打包图片 | `App.tsx:3,207` | `asset:/mark.png`、cover、32×32 | 只读挂载元数据，未断言解码/绘制 |
| 双轴 Scroll | `App.tsx:219-232` | 垂直溢出 + 水平内容宽度 | 水平 Scroll 从未实际滚动 |
| 滚动回调 | `App.tsx:245-271` | 六个初始 `"none"`；只触发 `onScroll` | 其余五个回调未触发 |
| active/hover/focus | `App.tsx:129-178` | 基础宽高 + 文案 | 未断言实际绘制；无 hover 步骤 |
| 受控 Input | `App.tsx:290-354` | 输入、`Name: ada`、disabled | 未覆盖 Return、`Keyboard.dismiss`、placeholder 选择器 |
| onPress | `App.tsx:85-96,129,144` | 文案变化 | 未覆盖嵌套 span press |
| 静态 transform | `App.tsx:97-107` | transform 步骤列表 | 未断言绘制或 origin 几何 |
| fetch JSON | `App.tsx:359-375` | 等到 `Gallery: 5` | 只有成功路径；未覆盖 abort/重定向/错误 |
| safe-area `env()` | `App.tsx:108-116,398-402` | top=22、left=10 | bottom/right、keyboard inset、`max(env(a),env(b))` 未断言 |
| `platform.os` | `App.tsx:382-388` | 值属于三端之一 | 未断言当前 target 对应值 |

## 附件：已复现的 API 缺口

临时应用 `/tmp/nlayer-probe`，用 `nlayer test` 无窗口运行。结果 `3 passed, 4 failed`。`TextDecoder` 的两项和 `Blob` 构造器已关闭。

| 测试 | 结果 | 说明 |
|---|---|---|---|
| `crypto.subtle is absent` | ok | QuickJS 宿主符合 v1 |
| `URLSearchParams exposes entries` | fail | 运行时没有 `entries`；类型也未声明，属标准完整性而非类型漂移 |
| `Headers exposes entries` | ok | 运行时存在，类型未声明 |
| `Response has no clone` | ok | 与 v1 最小面一致 |


## 建议切片顺序

1. **CLI/API 契约**：P0-1、P0-5、P0-8、P0-9 已关闭。
2. **键盘/选择器**：P0-2、P0-3、P0-6、P0-7 已关闭。
3. **绘制与宿主语义**：P0-4、P0-10、P1-2 已关闭。剩余 P1-3。
4. **Scroll 定稿**：P1-1；先改文档或改实现，不要两份契约并存。
5. **验收自动化**：P2-1 到 P2-3；把 gallery 和三端 target 测试接入 `cargo test` / CI。
