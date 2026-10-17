# 设备文件共享 —— 现状与 Agent 交接

- **最近更新**：2026-10-16（首次快照 2026-10-13）。
- **性质**：进度快照 + **Agent 交接文档**，供接手 P7/Go 宿主的 agent 快速进入上下文。需求与决策见 §1/§2；已落地代码位置与不变量见 §3；下一步任务清单见 §5；gate 与硬约束见 §6/§7。
- **一句话状态**：Engine 侧 holder 面、出站 client 面、Engine op 暴露与 **本仓 daemon HTTP/WS 面（P5）均已落地**：16 条 session-scoped 路由 + 2101–2121→稳定码 + 423 锁定语义 + `remote-share.*` 事件改名（daemon 登记表判别）+ OpenAPI/TS client 已重生成，本仓检查全绿（除按预期红的 provenance gate）。Engine 暂经本地 path 依赖消费。剩余：Go 宿主（拖放/`stat_local_paths`/选择器）、前端（P7）、三平台/双 peer 端到端（P9），以及合并前的 upstream 推送授权 + repin。

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

## 3. 已落地（Engine 仓，分支 `feat/device-file-share-p1-port`）

基线 `origin/main` = `0e25f418`（14-crate 布局）。**工作树 = git worktree `/home/hank/Documents/Engine-share-port`**（主仓 `/home/hank/Documents/Engine` 保持未动）。**全部 7 个 commit 已推到 `github.com/Killea/Engine`**（用户 2026-10-15 明确授权直接同步三台机器）；`UniClipboard/Engine` upstream 与产品仓 rev-pin 未动，需 §7 的授权流程。

| commit | 内容 |
| --- | --- |
| `ed3116be` | **Phase 0**：P1 安全底座移植到 14-crate 基线（原压在落后 36 commit 的 7-crate 旧基线；首次快照 §4.1 的"方向错误"已纠正） |
| `0cfeb69d` | **Phase 1 A**：`remote-share/1` wire 协议 codec（11 单测） |
| `54c6544d` | **B1**：serve 端口 + domain 类型 + `ShareRootId` wire 字节助手 |
| `033e3b59` | **B2**：serve 逻辑（authorize→confine→read AND，fail-closed）+ 13 单测 |
| `33aa08b8` | **B3**：iroh serve 适配器 + `install_remote_share` + bind-time ALPN + 引擎装配 + 5 条端点级判定 |
| `90141f9e` | **P2–P4 收尾**：句柄化约束内核 + 显式 opcode 线格式 + DenyCode 全集 + 变更 RPC（mkdir/rename/delete/copy_within）+ 回收站 + 两阶段上传（StageUpload/Commit/Abort，bounded fetch + transfer_id 进度）+ 导出（publish_reader → ticket）+ §5.3 审计记录 + `ShareRootsRegistry` 启动 reconcile / `replace_doc` / `revoke_all` + ADR-036 |
| `b3e2e792` | **P5 Engine op 暴露**（spec §9.1）：12 个 `Operation`/`OperationResult`/`OperationKind` 变体（QueryRemoteShareState、ReplaceRemoteShareRoots、SetRemoteShareSuspended、QueryPeerShareState、ListPeerShareRoots、ListPeerShareDirectory、StatPeerShareEntry、ListPeerShareTrash、MutatePeerShare、UploadPeerShare、DownloadPeerShare、CancelPeerShareTransfer）+ `contract/remote_share.rs` DTO + dispatch → `SessionSupervisor` accessor → `RemoteShareClient`/`ShareRootsRegistry` + caller-issued `transfer_id`（进度/终态事件与取消共用关联键）+ `RemoteShareClient` 在途表/终态 StatusChanged/上传 commit 点取消语义（TooLate）/下载 shutdown_inflight_fetch + `EngineErrorCategory::Cancelled`（含 uniffi 绑定映射）+ deny 17 码 → 数字块 2101–2117 分类映射，16 条 ops 单测钉死映射表 |
| `d0096330` | **P5 出站 client**：`RemoteShareClientPort`（`uc-core`，ShareClientDeny 17 码领域镜像 + Unreachable/Refused/UnexpectedReply/Io 分类）+ `IrohRemoteShareClientAdapter`（地址解析 → staggered-retry dial → 单 bi-stream 请求/应答 → 主动 close）+ `RemoteShareClient` 编排（枚举 → publish+ 签票 → 48KiB 预算分片 stage → commit，`TooLate` 视为已落盘；export→`fetch_to_path`；会话拆除 `release_inflight` 清算在途上传租约）+ `install_remote_share` 改返回 `RemoteShareHandlers{client}` + 会话装配持有。无对应 `Operation` 变体——daemon 路由落地时一并加 Engine op 面 |

### 3.1 各件代码位置（14-crate 布局，均在 Engine 仓 worktree）

| 件 | 位置 |
| --- | --- |
| 共享根 domain + grant + 归一化 | `crates/uc-core/src/ports/remote_share/model.rs`（含 `ShareRootId::as_bytes`/`from_wire_bytes`、保留名筛查 `is_reserved_share_component`） |
| 存储端口 / 约束端口 / serve 端口 | `crates/uc-core/src/ports/remote_share/{store,confinement,serve}.rs`（`RemoteShareServePort` 全 15 动词、payload-free `ShareServeError`、不透明 `ConfinedTarget` 能力句柄） |
| 加密共享根清单（Space 子密钥 AEAD，原子替换、截断式读取） | `crates/uc-infra-storage/src/remote_share/store.rs`（`EncryptedShareStore`） |
| cap-std 路径约束内核 + 逃逸探针 | `crates/uc-infra-local/src/fs/share_confinement.rs`（登记时句柄/路径身份互证 + 保留命名空间扫描 + 末段 no-follow）+ `crates/uc-infra-local/tests/share_confinement_escape_probe.rs`（25 测） |
| fail-closed 授权器 + 门面 | `crates/uc-application/src/remote_share/{authorizer,facade}.rs`（`ShareAuthorizer`、`RemoteShareFacade`；ADR-018 暴露） |
| serve 逻辑（AND 门） | `crates/uc-application/src/remote_share/serve_handler.rs`（`RemoteShareServe`/`RemoteShareServeDeps`：share_store + confinement + blobs + progress） |
| 传输编排 | `crates/uc-application/src/remote_share/transfer.rs`（`ShareTransferCoordinator`：staging 会话 + fetch worker `MAX_CONCURRENT_FETCHES=4` + `transfer_id` 进度上报 + commit 墓碑/取消 + 导出 lease） |
| 根注册责任人 | `crates/uc-application/src/remote_share/registry.rs`（`ShareRootsRegistry`：`reconcile_startup`/`replace_doc`/`revoke_all`；清单唯一写路径） |
| wire codec | `crates/uc-sync-protocol/src/remote_share.rs`（显式 `u32 opcode` + per-op postcard payload；`DenyCode` 全 17 码；`WireStageEntry.ticket`；`StageUpload.transfer_id`；`Exported{ticket,size}`；golden 向量钉字节） |
| iroh serve 适配器 + ALPN 装配 + §5.3 审计 | `crates/uc-infra-p2p/src/network/iroh/remote_share/serve_adapter.rs`（全部 op 分发 + DenyCode 全量映射 + `audit_served`；`crates/uc-infra-p2p/src/network/iroh/blobs.rs` 有 `publish_reader` 流式发布） |
| 审计记录契约 | `crates/uc-observability-contract/src/diagnostics/connectivity/remote_share.rs`（`remote_share.serve.op` 封闭记录：op/root_id/blake3(rel) 截断/结果码，peer 经 `peer_context` 匿名 `peer_ref`） |
| 引擎装配 | `crates/uc-engine/src/assembly/sync_engine.rs`（`install_remote_share` 后 `ShareRootsRegistry::reconcile_startup`，`builder.finish()` 前；`SyncSessionAssembly::remote_share_registry` 会话级持有，`shutdown` 首步 `revoke_all`） |
| 观测协议枚举 | `crates/uc-observability-contract/src/diagnostics/connectivity/inbound_peer.rs`（`InboundPeerProtocol::RemoteShare`） |
| ADR | `docs/design-docs/decisions/034-share-path-confinement-cap-std.md`、`035-share-store-space-subkey.md`、`036-remote-share-alpn-blob-addressing.md`（线格式约定 + §19 残余风险接受） |
| 执行计划（已归档） | Engine 仓 `docs/exec-plans/completed/2026-10-14-device-file-share-p3-p4.md`（含 T9 gate 证据与稳定结论） |

### 3.2 必须遵守的核心不变量（接手 P5/P7 时别破坏）

1. **AND 义务（authorize + confine）**：serve 侧每个请求必须先 `ShareAuthorizer::authorize`（fail-closed，返回 `Option<SharePermit>`）**再** 走 confinement 动词，**两者都成功才执行**；`resolve` 单独不构成授权。见 `serve_handler.rs`/`transfer.rs`。
2. **wire 脱敏**：holder 本地绝对路径与对端身份 **绝不** 上 wire、进日志。wire DTO 只带不透明 `root_id` + `label` + 相对条目名；`label`/`name`/`path`/`ticket` 类字段 `Debug` 一律脱敏；§5.3 审计里 peer → 匿名 `peer_ref`、rel → `blake3` 截断指纹。
3. **持久化默认密文**：共享根清单（白名单）经 Space 子密钥 AEAD 加密落盘，**不明文** 进 DB/日志/索引（见 VISION「安全与隐私底线」）。
4. **控制面只传元数据**：`remote-share/1` 传 opcode 帧；**字节体走 iroh-blobs**（`BlobTicket` 含地址+hash），不经本 ALPN。
5. **失败封闭**：任何授权/约束/存储失败一律 deny（不给对端泄露具体原因），映射到类型化 deny code；`StagingNotFound`→`NotFound`、`IsADirectory`/`NotADirectory`→`Denied` 两处塌缩是有意的。
6. **句柄化能力**：`ConfinedTarget` 是句柄不是路径快照——撤销/重登记/根替换后旧 token 一切 op 得 `RootGone`（代次 + 身份复核）。客户端路径首段撞上 `.uniclipboard-trash-`/`.uniclipboard-staging-` 保留前缀一律拒绝。
7. **清单唯一写路径**：`ShareRootsRegistry`（`reconcile_startup`/`replace_doc`/`revoke_all`）——装配/未来写路径不得绕过它直接 `register_root`。
8. **暂存原子落位**：`CommitUpload` 等 fetch 收敛再逐条 rename；staging_id commit 后进墓碑，重复 commit/迟到 abort 得 `TooLate`；`AbortUpload` 幂等且撕在途 fetch。

## 4. 已闭合的前置项（2026-10-15，随 `90141f9e`）

原 §4 三项未闭合保证已全部按裁定方案落地：

1. **`real_path` 快照 → 不透明句柄**。`ConfinedTarget` 内嵌已验证父目录 `Dir` fd + 末段名 + 登记代次；`ShareConfinementPort` 全动词相对保留句柄操作，检查对象与被操作对象是同一个 fd；末段一律 `cap-fs-ext` no-follow 打开。
2. **根生命周期**。登记时从已打开句柄取 `dev`/`ino`（Windows `volume_serial_number`/`file_index`）作 `RootIdentity`；存活检查比对登记路径 stat 身份——挪走、替换、删除重建全部 fail-closed 成 `RootGone`（探针 25 测覆盖）。
3. **`canonicalize` 与 `open_ambient_dir` 互证**。同一次登记比对定稿路径 stat 与句柄 stat 身份，不一致即拒绝。

§19 裁定：**接受内容寻址残余风险**（持 digest 者可拉取同字节属既有属性，残余为存在性预言机 + 带宽占用），不加盐、不建白名单，写入 ADR-036。

## 4.5 P5 daemon/API（本仓，hank_dev 工作树，未提交）

2026-10-16：daemon HTTP/WS 面已按 spec §9/§12 落地并全绿。Engine 消费方式从 fork rev-pin 改为 **本地 path 依赖**（`Cargo.toml` → `../Engine-share-port/crates/uc-engine`），省掉每次 Engine 迭代的 push/rev 仪式；**合并前必须换回 `UniClipboard/Engine` 固定 rev**（`check-engine-repository.mjs` 当前按预期红）。

| 件 | 位置 / 内容 |
| --- | --- |
| 路由/WS/错误码常量 | `crates/uc-daemon-contract/src/constants.rs`：16 条 `http_route` 常量 + `remote-share.progress`/`remote-share.status_changed`/`remote-share.availability_changed` + `remote_share_code` 稳定字符串块 |
| DTO | `crates/uc-daemon-contract/src/api/dto/remote_share.rs`：全 camelCase；可选字段 `skip_serializing_if`；`PeerShareListResponse`/`PeerShareMutationResultDto` 为 `kind` 标记联合；9 个 envelope alias 注册在 `envelope.rs` |
| handler | `crates/uc-webserver/src/api/remote_share.rs`（约 1440 行）：16 条路由全 session-scoped；Engine 数字码 2101–2121 → 稳定字符串码；`1103`/`2121` → `423 session_locked`；`GET /remote-share/state` 锁定态返回 200 无路径形态；upload/download 走 `spawn_transfer`（daemon 签发 UUID `transfer_id`，立即 202，后台跑 Engine op，execute 前置失败时合成终态事件）；cancel → `cancelled`/`not_inflight`/`too_late` |
| 事件归属 | `RemoteShareTracker`（同文件）：inflight/finished 集合 + per-peer availability 基线。`entry_id` 不可用作判别式（outbound 翻译器把 `transfer_id` 填进 `entry_id`，mobile-lan 也有 `entry_id=None`），必须以登记表为准；`StatusChanged` 对 remote-share 只发终态（Engine `emit_terminal`），重复终态折叠、迟到丢弃 |
| 事件转发 | `apps/daemon/src/daemon/engine_events.rs`：`forward_transfer_event` 拦 `TransferProgress`/`TransferStatusChanged`，登记 id 改名 `remote-share.*` 后转发 `file-transfer` topic；availability 变化由 state/error 路径触发 `publish_availability` |
| 接线 | `routes.rs`（`router_l2_plus` merge）、`server.rs`（`DaemonApiState.remote_share`）、`api/mod.rs`、`openapi.rs`（16 op + 33 schema 注册，冻结断言 114 路径 / 126 op）|
| 门禁修正 | `content_lock.rs`：`/remote-share/` 归类 Open（handler 自回 423/锁定形态），`/peers/*` 已 Open；顺手删掉已移除的 `/analytics/capture` OPEN_ROUTES 残项。openapi.rs 里 99/111 的冻结断言本就是 stale（98287e52 删 analytics 没改断言，gen-openapi.rs 的 98/110 才是真的）——已一并改正 |

验证：`cargo check --workspace --all-targets` 绿；`cargo test -p uc-webserver`（196 lib 测，含 remote_share 5 测）/`-p uc-daemon-contract`（111）/`-p uc-daemon` 绿；`cargo fmt --all -- --check`、`git diff --check` 绿；`bun run gen:api` 已重生成（363 schema / 114 路径 / 126 op，TS client 同步）。`check-engine-repository.mjs` 按预期红（path 依赖期）。

## 5. 下一步任务清单（P5 → P7 → P9，建议顺序）

### ~~P4 传输编排（Engine 仓）~~ 已完成（`90141f9e`，未推送）

holder 侧全部落地：两阶段上传（StageUpload 含 ticket 清单 + `transfer_id` → 有界 fetch → CommitUpload 原子落位/墓碑 → AbortUpload 幂等撕取）、Export（`publish_reader` 流式发布 + `RemoteShareExport` 30min lease）、变更 RPC 与回收站全线打通、§5.3 审计记录、`ShareRootsRegistry` 启动收敛。**上传方（peer 侧）的 publish + ticket 发放 + 上传循环属于 P5 出站 client 一并实现**——当前只有 holder 侧 serve 面。

### ~~P5 daemon 路由（本仓）~~ 已完成（2026-10-16，见 §4.5）

- ~~出站 client 端口~~：`RemoteShareClientPort` + `IrohRemoteShareClientAdapter` + `RemoteShareClient` 已落地并经会话装配持有（`SyncSessionAssembly.remote_share_client`），测试：p2p loopback 9 判定 + application 编排 7 判定。
- ~~Engine op 暴露~~：`b3e2e792` 已落地——12 个 op 变体 + dispatch + supervisor accessor + caller-issued `transfer_id` + `Cancelled` 类别 + deny 数字块映射，spec §9.1 定稿。
- ~~daemon 路由~~：16 条路由 + WS `remote-share.*` 改名 + OpenAPI/SDK 再生成全部落地；Engine 暂经本地 path 依赖消费（合并前换回 pinned rev）。
- 下一步是 Go 宿主（拖放/`stat_local_paths`/选择器）与 P7 前端，之后才有用户可感知能力 + 端到端验证。

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
- 本改动相关 crate 快验：`cargo test -p uc-infra-p2p remote_share`（serve-adapter 7 判定）、`cargo test -p uc-application remote_share`（serve-handler 20 + transfer 编排 + registry 5）、`cargo test -p uc-infra-local --test share_confinement_escape_probe`（25 探针）、`cargo test -p uc-sync-protocol remote_share`（codec + golden 向量）、`cargo check -p uc-engine`。

### 6.2 本机构 `cargo test --workspace` 的环境性失败（**非本改动回归**）

1. `space_leave::repeated_leaves_do_not_accumulate_tasks_or_descriptors`（uc-engine）—— 负载敏感的资源计数 canary，高并发下 flaky；单独重跑即绿（2026-10-15 复验）。
2. `peer_address_read_diagnostics`（uc-infra-profile，**非本改动 crate**）—— backtrace 符号化测试，需 `UC_EXPECT_DIAGNOSTIC_STACK=available`（CI `pr-check.yml` 已设）。本地裸跑默认 `captured` 会 `unresolved` 失败；带该 env 即通过。
3. `uc-upgrade-matrix` `d*::*`（整组 `anchor-host-missing → skip`，**非本改动 crate**）—— 需 CI `build-upgrade-anchors.sh` 预构建的旧版 Engine anchor 二进制，本地 worktree 没有（2026-10-15 复验：66 cell 全为此因）。

**结论**：本机跑全 workspace gate 时加 `UC_EXPECT_DIAGNOSTIC_STACK=available`，且以上环境性失败 **不作为本改动回归判断依据**；权威 gate 是 CI（构建 anchor、设 env、干净环境）。

### 6.3 仍不能声称

- **Windows / macOS 全程未执行**（ADR-034 实测表那两列"跳过"，非"通过"；句柄身份字段取的是 `cap-std::fs::MetadataExt` 的 windows 侧 `volume_serial_number`/`file_index`，真机未验）。P9 端到端验收才覆盖。
- "锁定 ⇒ 清单不可读"仅手写 stub 验证，未对真实 Space 会话验证（等 P5 路由）。
- **导出/上传全路径未做双 peer 真机验证**（`publish_reader`/`fetch_to_path` 经 blobs 实例的栈均以 stub 或单测覆盖；真机流量验证随 daemon 路由 + 端到端验收做）。
- ~~出站 client 尚无宿主内消费者~~——`b3e2e792` 起 `Engine::execute` 已可经 12 个 remote-share `Operation` 变体触达 client/registry。

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
| 执行计划（P3/P4，已完结归档） | Engine 仓 `docs/exec-plans/completed/2026-10-14-device-file-share-p3-p4.md` |
| ADR-034 / ADR-035 / ADR-036 | Engine 仓 `docs/design-docs/decisions/034-…`、`035-…`、`036-…` |
| **38 条裁定 + 逐任务评审记录（唯一载体）** | Engine 仓 `.superpowers/sdd/2026-10-12-device-file-share-phase1/progress.md`（**不要删**） |
| 本交接文档 | 本仓 `.planning/2026-10-13-device-file-share-status.md` |
| 线框稿（UI 方案对比） | 本仓 `.superpowers/brainstorm/1190761-1791828190/content/` |

## 9. 环境事实

- 三台开发机同交换机：Linux 本机 `192.168.99.1`；Windows(Legion) `192.168.99.2`（`ssh windows`，用户 `hankw`，远程 shell 是 cmd，多命令用 `&` 连接）；Mac mini `192.168.99.3`（`ssh mac-mini`，用户 `hank`；交换机断线备用 `ssh mac-mini-wifi` → `192.168.50.170`）。三平台两两组合是 P9 端到端验收的真实测试床。
- 本机工具链：`go` 不在 PATH，需 `/home/hank/sdk/go1.27.1/bin/go`（`apps/gui-go/go.mod` 要 `go 1.26` + toolchain `1.27.1`）；**Wails CLI 未安装**，故出不了完整应用包；`apps/gui-go/build.sh` 在非 Darwin 直接 `exit 2`，仓库无 Linux 打包路径。
- Rust 工具链 `1.95.0-x86_64-unknown-linux-gnu`。磁盘紧张（约 1.1T，长期 97% 满）；`cargo test --workspace` 前留意 `target/` 占用，必要时清本仓或主仓的 `target/`。
