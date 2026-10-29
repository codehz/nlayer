# v1 缺口审计

本文不是产品范围。范围仍以 `docs/v1.md` 和 `AGENTS.md` 为准；本文只记录当前实现与 v1 契约之间的差距、验证证据和验收方式。

- 审计基线：`main`，`25152b4`，2026-10-29
- 审计范围：`crates/*`、`packages/*`、`examples/gallery`、`scripts/*`
- 验证环境：macOS 27.0.0 arm64；Xcode iOS 27.1 模拟器（iPhone Duo）；Android emulator-5554；web target 使用本机 Brave（Chromium）
- 维护约定：每修掉一项，删除对应条目或在“状态”里标成已关闭；不要让本文变成第二份产品范围

## 结论

v1 的主功能面已经实现：reactive 图、JSX lowering、引擎线程、bundle、五个宿主组件、三端平台 API、CLI 和类型包都有真实代码，没有 `todo!()` / 空实现。当前剩余工作集中在四类：

1. 明确的契约缺口（绘制）：P0-4 已关闭。
2. 宿主语义偏差：本切片的 focusStyle、onScroll、transformOrigin 已关闭。
3. 验收与回归覆盖：P2-1 到 P2-3 已关闭。其余见 P2-4 起
4. 清理项：P3。

P1-7 已关闭：web 的浏览器全局不遮蔽，缺失契约只约束 QuickJS 宿主。

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

`scripts/test-targets`（`cargo test -p nlayer-cli --test targets`）跑 gallery 四端和三端 press/type/scroll/safe-area。CI 是 `.github/workflows/targets.yml`。

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
| P1-1 | 已关闭。每实例一轴。产品句改为「每实例一轴，横竖各一个 Scroll」；验收屏的「双轴 Scroll」仍指两个实例。iOS 纵向假宽只收原生可滑范围，事件和树仍报引擎尺寸 | `scroll_extent.rs`；iOS `settle_content_sizes` | 单实例不是双轴 |
| P1-2 | 已关闭。web 用 `requestAnimationFrame`；iOS / Android 把同一帧的偏移合成一次，结束、拖动和惯性先发出等待中的 `onScroll` | `scroll_frame.rs`；`ScrollBox.queueScroll`；web `queueScroll` | `onScroll` 至多每帧一次 |
| P1-3 | 已关闭。`frame` 布局后按当前宽高重算 pivot；百分比跟尺寸走，绝对长度不重算 | `Mounter.frame`；`Decor.applyPivot` | resize 后 `transformOrigin` 百分比不再陈旧 |
| P1-4 | 已关闭。stamp 新鲜时仍用 `pm path` 确认包在，并用 `run-as` 确认数据目录可进。卸载或数据目录不可进时重装 | `install_if_needed`；`host_present` | `run-as` push 前包和数据目录都在 |
| P1-5 | 已关闭。不移植 Node 兼容。打包器不写 Node 拒绝。rolldown 1.2.13 不打 shim，内建留成外部 import；QuickJS 加载时抛 `could not load module` | `plugin.rs` resolve_id | 不补拒绝名单，也不留 shim 闸 |
| P1-6 | 已关闭。编译器插入 `nlayer:rt` 之前，源码里的 import、re-export、`import()`、`require` 和 `new URL` 报错。编译器生成的导入仍是外部模块 | `compile.rs` `handwritten_rt` | 应用源码不能写 `nlayer:rt` |
| P1-7 | 已关闭。Web 不受「`crypto.subtle` 缺失」约束：应用在页面里跑，浏览器全局原样保留，v1 不遮蔽。该条只约束 QuickJS 宿主（iOS、Android、无窗口 `nlayer test`） | `docs/v1.md:17` | web 上浏览器自带的 `crypto.subtle` 可用；不属于 v1 API，也不进类型 |

### P1 处理建议

- P1-1：已关闭。文档与实现都是每实例一轴。
- P1-2：已关闭。同一帧多次偏移只收到最后一次 `onScroll`。
- P1-3：已关闭。尺寸变化后宿主重算 pivot，不依赖引擎重发样式。
- P1-4：已关闭。包不在或 `run-as` 进不了数据目录时 `adb install -r`。
- P1-5：已关闭。不补拒绝名单，打包器不拒绝 Node 内建。
- P1-6：已关闭。手写 `nlayer:rt` 报错；编译器生成的导入留下。
- P1-7：已关闭。契约按宿主分档，web 不遮蔽浏览器全局；仅改文档，无运行时代码改动。

## P2：测试与验收覆盖

| ID | 缺口 | 证据 | 影响 |
|---|---|---|---|
| P2-1 | 已关闭。`tests/targets.rs` 覆盖 web/iOS/Android 的 press、type、scroll、树和 safe area，并跑 gallery 四端。`scripts/test-targets` 与 `.github/workflows/targets.yml` 是入口 | `crates/nlayer-cli/tests/targets.rs` | 回归进 `cargo test` |
| P2-2 | 已关闭。水平 Scroll 滚到 `{x:40}`；六个滚动回调都等到同一 metric；safe area 断言 top/right/bottom/left 和 `max(env, keyboard)`；`platform.os` 等于当前 target | `examples/gallery/App.test.tsx` | 回归有测试。active/hover/focus 绘制仍见验收矩阵 |
| P2-3 | 已关闭。`<If>` 覆盖 when/else、函数 child 和分支切换；`<Each>` 覆盖 `key={fn}`、`key="index"` 和乱序重排；`ErrorBoundary` 覆盖 effect、listener、`reset` 和 fallback 再抛 | `crates/nlayer-js/tests/renderer.rs` | lowering 或运行时回归有测试 |
| P2-4 | CLI 路径未测 | `crates/nlayer-cli/tests/` | `--filter`、真实 stdin 步骤、`run --target ios|android` 的 watch/rebuild/失败回退、类型版本警告退出码都未覆盖 |
| P2-5 | Web target 输入保真度不足 | `web/host.js:381-439` | press/type/scroll 是合成 DOM 事件，不是 CDP Input；键盘 Return、拖拽、momentum 未被真实浏览器 E2E 覆盖 |
| P2-6 | Android Java 层无测试 | `crates/nlayer-platform-android/java/` | `Mounter`、`ScrollBox`、`InputBox`、`ImageBox` 的设备行为只靠手工验证 |
| P2-7 | iOS 宿主模块被 `cfg(target_os = "ios")` 排除出 `cargo test` | `crates/nlayer-platform-ios/src/lib.rs:24-45` | 设备层只有 15 个纯逻辑测试；mount/host/driver/text 不跑 |

### P2 建议的最小验收集

1. 已关闭。`ios`/`android`/`web` target 测试与 gallery 四端在 `cargo test -p nlayer-cli --test targets`。
2. 已关闭。gallery 覆盖水平 Scroll 实际滚动、六个滚动回调、`max(env(a),env(b))`、四边 safe area、target 对应的 `platform.os`。
3. 已关闭。`<If>` when/else/函数 child/分支切换；`<Each key={fn}>`、`key="index"`、乱序重排。
4. 已关闭。`ErrorBoundary` 覆盖 effect/listener 抛错、`reset`、fallback 再抛。
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
| 双轴 Scroll | `App.tsx:219-232` | 垂直溢出 + 水平滚到 x=40 | 未断言绘制 |
| 滚动回调 | `App.tsx:247-275` | 六个回调都等到同一 metric | 无 |
| active/hover/focus | `App.tsx:129-178` | 基础宽高 + 文案 | 未断言实际绘制；无 hover 步骤 |
| 受控 Input | `App.tsx:290-354` | 输入、`Name: ada`、disabled | 未覆盖 Return、`Keyboard.dismiss`、placeholder 选择器 |
| onPress | `App.tsx:85-96,129,144` | 文案变化 | 未覆盖嵌套 span press |
| 静态 transform | `App.tsx:97-107` | transform 步骤列表 | 未断言绘制或 origin 几何 |
| fetch JSON | `App.tsx:359-375` | 等到 `Gallery: 5` | 只有成功路径；未覆盖 abort/重定向/错误 |
| safe-area `env()` | `App.tsx:108-116,397-405` | top/left、right/bottom、`max(safe, keyboard)` | 无 |
| `platform.os` | `App.tsx:382-388` | 等于当前 target 的 `__nlayerOs` | 无 |

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
3. **绘制与宿主语义**：P0-4、P0-10、P1-2、P1-3、P1-4 已关闭。
4. **Scroll 定稿**：P1-1 已关闭。
5. **验收自动化**：P2-1 到 P2-3 已关闭。
