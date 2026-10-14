# 设备文件共享 —— 现状与 Agent 交接

- **最近更新**：2026-10-14（首次快照 2026-10-13）。
- **性质**：进度快照 + **Agent 交接文档**，供接手 P4/P5/P7 的 agent 快速进入上下文。需求与决策见 §1/§2；已落地代码位置与不变量见 §3；下一步任务清单见 §5；gate 与硬约束见 §6/§7。
- **一句话状态**：P1 安全底座已迁到 14-crate 基线并可消费（Phase 0）；`remote-share/1` 控制面 **holder 侧**（wire codec + serve 端口 + serve 逻辑 + iroh 适配器 + 引擎装配）已实现、单测/集成测全绿、已推到 `Killea/Engine` fork 分支。剩余：出站 client（P5）、传输编排（P4）、daemon 路由（P5）、前端（P7）、两 peer 端到端（推送 + rev-pin 后）。

## 1. 原始需求

在设备界面上点击一台 **在线** 设备，若该设备在线则出现一个面板，展示该机器的硬盘目录树；凡有权限的目录都允许拖放文件进去，并支持删除、重命名、拷贝等文件操作；拖放不可用时提供上传按钮，支持上传文件夹与文件。目的：方便几台开发机之间互传文件。附带要求设计该面板与既有界面如何配合（弹窗 / 新 tab / 其他）。

追加提出的两个问题：本机有多个网卡，应支持用户设置走哪条 LAN，或自动确定最快的 LAN 线路。

**设计修正（2026-10-14）**：交互形态回归规格原始设计——点击设备 → 面板展示该机的 **授权目录树** → 把 **本地文件** 拖放（HTML5 DnD）或经上传按钮（原生文件/文件夹选择器）送入某目录。上传来源是 **本地文件**，不是剪贴板条目（上一版"把剪贴板内容物化成 .txt/.url 再发"的胶水已移除，属过度解读）。P0–P5 管线不受此修正影响，仅 P7 前端形态与"上传来源"随之明确。

## 2. 定稿的需求决策

| 决策点 | 定稿 | 理由 |
| --- | --- | --- |
| 定位 | 正式产品功能，非开发者自留工具 | 用户选定 |
| 传输层 | **P2P 新 ALPN**（`remote-share/1`），LAN-only 用现成 `allow_relay_fallback=false` | LAN HTTP 兼容通道是明文 HTTP + 可重放静态 Basic + 无路径约束原语，承载整盘访问会破 VISION 传输保护底线，且受 ADR-005 边界约束 |
| 暴露范围 | **显式授权白名单**，逐根独立 read/write/delete 开关，默认空 | `VISION.md:61` 锁定"单 MasterKey 扁平加密、无法可靠撤销已泄露设备"，被盗时损失上界必须只是用户主动共享的目录 |
| 白名单落盘 | **Engine 加密存储**（Space 子密钥），不进 settings | `settings.json` 整文件明文落盘，且已有同类字段 `file_sync.auto_save_dir` 存绝对路径明文、无书面论证。不沿用该先例 |
| 路径沙箱 | **引入 `cap-std`** | 手写三平台 `openat2`/`O_RESOLVE_BENEATH`/`NtCreateFile` 风险更高；纯字符串校验挡不住符号链接与 TOCTOU |
| UI 形态 | **设备详情面板内新增「文件」Tab**（方案 A） | 侧栏设备行现在无 `onClick`；详情面板本就是"按设备切换的上下文"，`DeviceList` 天然成为设备切换器，零新导航 |
| 共享管理入口 | **设置页新增「共享」UI 分组**，对端 Tab 内不放跳转链接 | 每台设备只管自己的暴露面；避免"我的"与"别人的"两套语义混在一屏 |
| 范围 | 全量一次做完（浏览 + 上传 + 下载+mkdir/重命名/删除/设备内拷贝 + 路径可见性） | 用户选定 |
| 多网卡 | 自动选路 **已存在**（iroh 内置 `BiasedRttPathSelector`）；本次只做"显示走的是哪张网卡"；**手动钉选为非目标** | `Builder::path_selector()` 在 iroh 1.3 位于 `unstable-custom-transports` 之后 |

## 3. 已落地（Engine 仓，分支 `feat/device-file-share-p1-port`，已推 `fork`）

基线 `origin/main` = `0e25f418`（14-crate 布局）。5 个 commit，全部已推到 `github.com/Killea/Engine` 的 `feat/device-file-share-p1-port`（新分支，未覆盖；PR 可建：https://github.com/Killea/Engine/pull/new/feat/device-file-share-p1-port）。**工作树 = git worktree `/home/hank/Documents/Engine-share-port`**（主仓 `/home/hank/Documents/Engine` 保持未动）。

| commit | 内容 |
| --- | --- |
| `ed3116be` | **Phase 0**：P1 安全底座移植到 14-crate 基线（原压在落后 36 commit 的 7-crate 旧基线；首次快照 §4.1 的"方向错误"已纠正） |
| `0cfeb69d` | **Phase 1 A**：`remote-share/1` wire 协议 codec（11 单测） |
| `54c6544d` | **B1**：serve 端口 + domain 类型 + `ShareRootId` wire 字节助手 |
| `033e3b59` | **B2**：serve 逻辑（authorize→confine→read AND，fail-closed）+ 13 单测 |
| `33aa08b8` | **B3**：iroh serve 适配器 + `install_remote_share` + bind-time ALPN + 引擎装配 + 5 条端点级判定 |

### 3.1 各件代码位置（14-crate 布局，均在 Engine 仓 worktree）

| 件 | 位置 |
| --- | --- |
| 共享根 domain + grant + 归一化 | `crates/uc-core/src/ports/remote_share/model.rs`（含 `ShareRootId::as_bytes`/`from_wire_bytes`） |
| 存储端口 / 约束端口 / serve 端口 | `crates/uc-core/src/ports/remote_share/{store,confinement,serve}.rs`（`RemoteShareServePort`、payload-free `ShareServeError`、redacting-Debug 的 `ShareRootDescriptor`/`ShareDirectoryPage`/`ShareAvailability`） |
| 加密共享根清单（Space 子密钥 AEAD，原子替换、截断式读取） | `crates/uc-infra-storage/src/remote_share/store.rs`（`EncryptedShareStore`） |
| cap-std 路径约束内核 + 19 条逃逸探针 | `crates/uc-infra-local/src/fs/share_confinement.rs` + `crates/uc-infra-local/tests/share_confinement_escape_probe.rs` |
| fail-closed 授权器 + 门面 | `crates/uc-application/src/remote_share/{authorizer,facade}.rs`（`ShareAuthorizer`、`RemoteShareFacade`；ADR-018 暴露） |
| serve 逻辑（AND 门） | `crates/uc-application/src/remote_share/serve_handler.rs`（`RemoteShareServe`/`RemoteShareServeDeps`；`spawn_blocking` 读 fs；13 单测，mock 双向验证：仅 authorize 或仅 resolve 都拒绝） |
| wire codec | `crates/uc-sync-protocol/src/remote_share.rs`（`REMOTE_SHARE_ALPN = b"uniclipboard/remote-share/1"`；10 字节头 magic `UCRS`/version/kind/body_len(4BE) + postcard，cap-before-allocate；64KiB 请求 / 2MiB 响应上限；30s IO deadline；Hello/ListRoots/List/Stat；11 单测） |
| iroh serve 适配器 + ALPN 装配 | `crates/uc-infra-p2p/src/network/iroh/remote_share/{serve_adapter,mod}.rs` + `crates/uc-infra-p2p/src/network/iroh/node.rs`（`IrohSessionBuilder::install_remote_share`、bind-time ALPN 数组；`IrohRemoteShareServeHandler` 经 `InboundPeerGate` 放行后分发到 `RemoteShareServePort`；5 端点级判定：member 各请求被服务 / unknown peer 丢弃 / 错误映射 deny code） |
| 引擎装配 | `crates/uc-engine/src/assembly/{deps.rs, wire/mod.rs, sync_engine.rs}`（`SyncEngineDeps.remote_share` 携带 confinement；`prepare_sync_session` 安装点用 `space_access.share_store` + confinement 构造 `RemoteShareServe`，`builder.finish()` 前调 `install_remote_share`） |
| 观测协议枚举 | `crates/uc-observability-contract/src/diagnostics/connectivity/inbound_peer.rs`（`InboundPeerProtocol::RemoteShare`） |
| ADR | `docs/design-docs/decisions/034-share-path-confinement-cap-std.md`、`035-share-store-space-subkey.md`（原 ADR-030/031 因与 baseline 自身 030/031 撞号而重编号） |

### 3.2 必须遵守的核心不变量（接手 P4/P5 时别破坏）

1. **AND 义务（authorize + confine）**：serve 侧每个读请求必须先 `ShareAuthorizer::authorize`（fail-closed，返回 `Option<SharePermit>`）**再** `confinement.resolve`/`resolve_root`（cap-std 约束），**两者都成功才读**；`resolve` 单独不构成授权。见 `serve_handler.rs`。
2. **wire 脱敏**：`real_path`（holder 本地绝对路径）**绝不** 上 wire、进日志。wire DTO 只带不透明 `root_id` + `label` + 相对条目名；`ShareRootDescriptor`/`ShareDirectoryEntry` 的 `Debug` 实现脱敏 `label`/`name`。
3. **持久化默认密文**：共享根清单（白名单）经 Space 子密钥 AEAD 加密落盘，**不明文** 进 DB/日志/索引（见 VISION「安全与隐私底线」）。
4. **控制面只传元数据**：`remote-share/1` 只跑 Hello/ListRoots/List/Stat；**字节体走 P4 传输管线**（iroh-blobs），不经本 ALPN。
5. **失败封闭**：任何授权/约束/存储失败一律 deny（不给对端泄露具体原因），映射到类型化 deny code。

## 4. 仍未闭合的安全保证（P4/P5 开工前置决定）

P1/P2 无破坏性消费者，故当前可接受；**P4（落盘）/P5（变更 RPC）不得在其上建破坏性操作**，下列三项需在开工前裁定：

1. **`real_path` 是快照，不是能力。** 约束内核从不打开末段，交回的绝对路径由调用方用环境权限重开 —— 正是 ADR-034 决定 2 排除的"先校验字符串、再按绝对路径重开"两步式。彻底修法：端口交出 **不透明句柄** 并提供 **相对已保留 `Dir` 的操作**。列为 **P4 开工前置决定**。
2. **根生命周期检测不全。** 实测：改名挪走且不重植 ⇒ 可检测（`is_dir()` false → `RootGone`）；**改名后在同路径重植** ⇒ 检测不到，`real_path` 指向未校验的新树；**删除后重建** ⇒ 检测不到。修法：登记时从 **已打开的句柄** 取身份（Linux `dev`/`ino`，Windows file index），存活检查里复核，不匹配 fail-closed。
3. **注册期 `canonicalize` 与 `open_ambient_dir` 不互证。** 两次独立环境解析之间可被换掉，使 `dir` 与 `path` 指向不同树。身份复核与第 2 条同一批修。

## 5. 下一步任务清单（P4 → P5 → P7，建议顺序）

### P4 传输编排（Engine 仓）

- StageUpload / CommitUpload / AbortUpload 两阶段（iroh-blobs publish/fetch）+ 原子落盘 + 进度/取消。
- **前置决定（spec §19）**：blob 按 digest 寻址、任何持 digest 的 Space 成员可拉 → 需定（盐化 digest / 短命 digest 白名单 / ADR-015 接受并文档化）。
- 落盘目标必须经 P1 约束内核 `resolve` 到 **已授权根内**，不落白名单外；复用现有「发送到设备」(resend) 的 iroh-blobs 字节层。

### P5 daemon 路由 + 出站 client（本仓 + Engine 仓）

- **出站 client 端口**：dial 对端 `remote-share/1` 的 `ListRoots`/`List`/`Stat`（列出某台远程设备的目录树）。**本轮 holder 侧无出站 client，明确延后到 P5（无 consumer，YAGNI）**。
- daemon 路由：`POST /peers/{id}/share/upload|download|...` + WS `remote-share.*`；OpenAPI/SDK 再生成（`crates/uc-webserver`/`uc-daemon-contract`/`Cargo.toml` rev bump）。
- 到这一步才有用户可感知能力 + 能端到端验证。

### P7 前端（本仓）

- 设备详情面板「文件」Tab：渲染对端授权目录树（消费 P2 的 ListRoots/List/Stat）。
- **上传入口**：目录树内拖放 **本地文件**（HTML5 DnD，跨平台 best-effort）+ 上传按钮（原生文件/文件夹选择器，兜底，跨平台保证）。上传来源是 **本地文件**，非剪贴板物化。
- 六语 i18n。

### 收尾

- 两 peer 端到端验证（推送 + rev-pin 后；三台开发机两两组合为真实测试床，见 §9）。
- 合并与发布：需明确授权（推 `github.com/UniClipboard/Engine` + 发发布标签 + 本仓 rev-pin；见 §7-7）。

## 6. 验证与 gate（诚实清单）

### 6.1 命令

- Engine 仓（worktree `/home/hank/Documents/Engine-share-port`）：`cargo test --workspace --locked`、`cargo fmt --all -- --check`、`node scripts/architecture/check-rust-style.mjs`、`node scripts/architecture/check-engine-repository.mjs`、`node scripts/architecture/check-observability-privacy.mjs`。
- 本改动相关 crate 快验：`cargo test -p uc-infra-p2p remote_share`（serve-adapter 5 判定 + pull_serve 7）、`cargo test -p uc-application remote_share`（serve-handler 13）、`cargo check -p uc-engine`。

### 6.2 本机构 `cargo test --workspace` 的 3 条环境性失败（**非本改动回归**，2026-10-14 逐条 A/B/环境诊断）

1. `space_leave::repeated_leaves_do_not_accumulate_tasks_or_descriptors`（uc-engine）—— 负载敏感的资源计数 canary，`--workspace` 高并发下 flaky。冷却 A/B：baseline 6/6 绿 vs 带 B3 3/3 绿（一致）；隔离/冷却下稳定通过。
2. `peer_address_read_diagnostics`（uc-infra-profile，**非本改动 crate**）—— backtrace 符号化测试，需 `UC_EXPECT_DIAGNOSTIC_STACK=available`（CI `pr-check.yml` 已设）。本地裸跑默认 `captured` 会 `unresolved` 失败；带该 env 即通过。
3. `uc-upgrade-matrix` `d1::*`（13 条整组 `anchor-host-missing → skip`，**非本改动 crate**）—— 需 CI `build-upgrade-anchors.sh` 预构建的旧版 Engine anchor 二进制，本地 worktree 没有。

**结论**：本机跑全 workspace gate 时加 `UC_EXPECT_DIAGNOSTIC_STACK=available`，且以上 3 条环境性失败 **不作为本改动回归判断依据**；权威 gate 是 CI（构建 anchor、设 env、干净环境）。

### 6.3 仍不能声称

- **Windows / macOS 全程未执行**（ADR-034 实测表那两列"跳过"，非"通过"）。P9 端到端验收才覆盖。
- "锁定 ⇒ 清单不可读"仅手写 stub 验证，未对真实 Space 会话验证（等 P5 路由）。

## 7. 硬约束（接手 agent 必读）

1. **推送 gate**：Engine 工作 **绝不推共享 `origin`**（`github.com/UniClipboard/Engine`）；只推 `fork`（`Killea/Engine`），且需明确授权（已完成 Phase 0+1 的推送）。
2. **wire 脱敏**（§3.2-2）：`real_path` 永不上 wire/日志。
3. **持久化默认密文**（§3.2-3）：业务负载默认 MasterKey AEAD，不明文落库/日志；例外见 VISION（内容类型枚举、文件内容本体、受管缓存中的清理后入站文件名）。
4. **代码注释英文；项目文档中文**；commit message / PR 标题英文；一个 commit 一个意图。
5. **层边界**：`uc-core`（domain/ports）**不依赖** `uc-sync-protocol`（wire）；domain↔wire 翻译在 iroh 适配器（infra 本地镜像，与 core 类型分离，同 `active_clipboard_state` 的先例）。
6. **日志宏 `uc_info!`/`uc_warn!` 等用 `name = value` 字段语法**（非 tracing 的 `name,` 简写）。
7. **本仓硬 gate**：`check-engine-repository.mjs` 要求 Engine 包只能来自 pinned rev（禁本地路径）→ 本仓要 **运行** Engine 工作需先推 `UniClipboard/Engine` + 发发布标签 + 本仓 rev-pin（§5 收尾，需授权）。

## 8. 关键资产位置

| 内容 | 路径 |
| --- | --- |
| 规格（权威） | 本仓 `docs/specs/2026-10-12-device-file-share-spec.md` |
| 执行计划（Phase 1，含交付订正层） | Engine 仓 `docs/exec-plans/active/2026-10-12-device-file-share-phase1.md` |
| ADR-034 / ADR-035 | Engine 仓 `docs/design-docs/decisions/034-…`、`035-…` |
| **38 条裁定 + 逐任务评审记录（唯一载体）** | Engine 仓 `.superpowers/sdd/2026-10-12-device-file-share-phase1/progress.md`（**不要删**） |
| 本交接文档 | 本仓 `.planning/2026-10-13-device-file-share-status.md` |
| 线框稿（UI 方案对比） | 本仓 `.superpowers/brainstorm/1190761-1791828190/content/` |

## 9. 环境事实

- 三台开发机同交换机：Linux 本机 `192.168.99.1`；Windows(Legion) `192.168.99.2`（`ssh windows`，用户 `hankw`，远程 shell 是 cmd，多命令用 `&` 连接）；Mac mini `192.168.99.3`（`ssh mac-mini`，用户 `hank`；交换机断线备用 `ssh mac-mini-wifi` → `192.168.50.170`）。三平台两两组合是 P9 端到端验收的真实测试床。
- 本机工具链：`go` 不在 PATH，需 `/home/hank/sdk/go1.27.1/bin/go`（`apps/gui-go/go.mod` 要 `go 1.26` + toolchain `1.27.1`）；**Wails CLI 未安装**，故出不了完整应用包；`apps/gui-go/build.sh` 在非 Darwin 直接 `exit 2`，仓库无 Linux 打包路径。
- Rust 工具链 `1.95.0-x86_64-unknown-linux-gnu`。磁盘紧张（约 1.1T，长期 97% 满）；`cargo test --workspace` 前留意 `target/` 占用，必要时清本仓或主仓的 `target/`。
