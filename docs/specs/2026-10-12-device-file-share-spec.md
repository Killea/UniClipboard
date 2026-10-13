# 设备文件共享规格

- **状态**：草案，待评审；本文不代表已完成设计批准或实施。
- **日期**：2026-10-12。
- **范围**：桌面端在已配对设备之间浏览被显式授权的目录树，并进行上传、下载、新建、重命名、删除与设备内拷贝；附带把当前网络路径的本地地址透出到设备详情。
- **约束来源**：[产品原则](../../VISION.md)、[架构规则](../agent/architecture-rules.md)、[端口规则](../architecture/ports.md)、[Rust 规则](../agent/rust-rules.md)、[前端 UI 规则](../agent/frontend-ui-rules.md)、[Go 宿主规则](../../apps/gui-go/AGENTS.md)。
- **跨仓说明**：协议、端口、领域状态与加密存储属 `UniClipboard/Engine` 仓，本仓通过固定发布标签消费；下文以 `ENGINE:` 前缀标注该仓路径，其余为本仓相对路径。
- **Engine 布局基线**：`ENGINE:` 路径按 Engine **当前主线布局** 书写（`uc-core` / `uc-application` / `uc-infra` / `uc-engine` 共 7 个 crate）。本仓当前固定的 `0e25f41` 仍是合并前的旧布局（`uc-sync-protocol`、`uc-infra-p2p`、`uc-infra-local`、`uc-infra-storage`、`uc-infra-crypto` 等独立 crate），因此 **实施必须先基于 Engine 主线**，不能就着固定 rev 的路径动手。线格式代码现在的归属是 `ENGINE:crates/uc-infra/src/network/iroh/<feature>/*_wire.rs`，不再有独立的协议 crate。

## 1. 完成定义

以下条件必须同时满足：

1. 在设备页选中一台在线设备，「文件」Tab 能列出该设备 **显式授权** 的根目录，展开可逐层浏览其下的目录与文件，无需离开该页即可完成一次上传和一次下载。
2. 未被授权的目录，无论客户端如何构造路径参数，服务端一律拒绝，且不存在任何一条可绕过约束内核的路径输入形式（见 §13 不变量 I3）。
3. 对端未解锁（MasterKey 未派生）时，本功能明确报告为"对端未解锁"，不表现为离线、不表现为空目录、不静默失败。
4. 共享白名单中的绝对路径在任何持久化介质上都不以明文出现，包括数据库列、磁盘缓存、搜索索引与日志。
5. 上传一个含多级子目录的文件夹，中途取消、对端掉线、磁盘写满三种情况下，目标目录内都不残留部分写入的结果。
6. 拖放与"上传文件夹"按钮两条入口走同一条数据通路；拖放不可用时功能不降级。
7. 设备详情能看到本次连接实际使用的本地地址与远端地址；多网卡主机上默认由传输层自动竞速选路，无需用户配置。
8. 六份语言包（`en-US`、`zh-CN`、`zh-TW`、`ja-JP`、`ru-RU`、`pt-BR`）键完全对齐，i18n 门禁通过。
9. Linux ↔ Windows ↔ macOS 三平台两两组合完成一次真实的双向文件传输与一次删除/重命名验收。

## 2. 当前事实

本规格基于以下核对快照。实施时须固定通过验证的新 Engine 版本并同步 `Cargo.toml` 与 `Cargo.lock`。

| 位置 | 已有能力 | 尚缺能力 |
| --- | --- | --- |
| `ENGINE:crates/uc-infra/src/network/iroh/` | iroh 1.3 单栈、按功能注册 ALPN（预注册数组 `node.rs:971`）、`InboundPeerGate` 身份解析 | 无通用 RPC-over-P2P；无按请求授权 |
| `ENGINE:crates/uc-infra/src/network/iroh/active_clipboard/pull_wire.rs` | 请求→响应线格式模板：magic → 长度前缀 → postcard，读取前先验分配上限（`:182`） | 该模板仅承载固定上限的内存内信封 |
| `ENGINE:crates/uc-infra/src/network/iroh/blobs.rs` | `publish_path`、`fetch_to_path(digest, target)`（`ExportMode::TryReference`）、`_temp_tag` 抗 GC | 无面向任意远端目录的读写语义 |
| `ENGINE:crates/uc-infra/src/security/profile_storage_upgrade/persistence.rs` | 小型加密文档落盘的 **最贴近模板**：`seal_profile_payload`/`open_profile_payload` + 64 KiB 上限 + 自有 `write_atomically` | 不可直接复用，需按同形态新写 |
| `ENGINE:crates/uc-infra/src/security/profile_content_key_vault/persistence.rs` | 同形态的另一实现，含 `take(cap+1)` 截断与篡改 fail-closed 测试 | **整模块 `pub(super)`，外部不可调用** |
| `ENGINE:crates/uc-core/src/ports/space/access.rs:206` | `DeriveSpaceSubkeyPort::derive_subkey(salt, info) -> [u8; 32]`，锁定时返回类型化 `NotUnlocked` | — |
| `apps/daemon/src/daemon/mobile_lan_lifecycle.rs` | LAN HTTP 兼容通道 | 明文 HTTP、静态 Basic 无重放防护、无路径约束原语；范围受 ADR-005 约束 |
| `crates/uc-webserver/src/api/clipboard.rs:97` | `POST /clipboard/dispatch-file` 立即返回 + `entry_id` 即传输号 | 无通用作业登记表；`dispatch_file` 无并发信号量 |
| `ENGINE:crates/uc-infra/src/db/schema.rs` | `file_transfer` 表持久化绑定与状态 | 无 `completed_bytes` 列；启动即在途批量失败，不支持续传 |
| `apps/gui/src/components/history/sidebar/HistorySidebar.tsx:324` | 设备行渲染图标/名称/在线点 | 无 `onClick`，点击不产生任何行为 |
| `apps/gui/src/pages/DevicesPage.tsx:94` | 侧栏 + 设备列表 + 按设备切换的详情面板 | 详情面板无分页能力 |
| `apps/gui-go/host_commands_files.go:82` | `chooseDirectory`（Linux 走 zenity）、`chooseSaveFile`、`copyPathIntoDir` 递归遍历 | 无任何 OS 级文件拖放接线 |
| `ENGINE:crates/uc-infra/src/settings/repository.rs` | settings 读写 | **整个 `settings.json` 明文落盘**，无 AEAD 调用点 |

Engine 侧另有两处必须在实现时避开的既有缺陷：`pull_wire.rs:131` 的 `Io(std::io::Error)` 未标 `#[source]`，丢失了错误来源链，与 `ENGINE:docs/design-docs/error-handling.md` 相悖，新代码不得照抄；magic byte 唯一性 **只在单个 ALPN 内成立**（`0xC2` 已同时被 `transfer_progress_wire.rs` 与 `pull_wire.rs` 使用），不得按全局唯一来记文档。

### 2.1 已排除的传输方案

LAN HTTP 兼容通道被排除，理由不是性能而是底线：`crates/uc-webserver/src/mobile_lan/server.rs:78` 为明文 `axum::serve`，认证为可重放的静态 Basic，`file_staging.rs:47` 明确注明"adapter 不做路径白名单"。用它承载整盘访问会直接违反 `VISION.md` 「传输保护：统一通过带 Ed25519 身份认证的 QUIC 加密通道」，并扩张 ADR-005 划定的通道边界。

### 2.2 存量偏差记录

`file_sync.auto_save_dir` 存储用户绝对目录路径且明文（Engine settings 模型内，字段名稳定、行号随布局变动，实施时按符号定位），未检索到任何书面非敏感性论证。本功能不沿用该先例。该存量偏差的处置另立工作项，不在本规格范围内。

## 3. 范围与非目标

### 本期必须覆盖

- 共享根的增删、启停、逐根读/写/删除授权。
- 远端根列表、逐层目录列举、单项探测。
- 上传（单文件与多级文件夹）、下载/另存、设备内拷贝、新建目录、重命名、删除，以及回收目录的列举、还原与清空。
- 传输进度、取消、失败重传；跨平台拖放。
- 对端能力与解锁状态的探测与呈现。
- 本地地址透出到设备详情。

### 非目标

- **断点续传**。持久层无字节偏移，启动对账主动将在途传输批量置失败；本期按"失败重传"设计。
- **手动钉选网卡**。iroh 的 `Builder::path_selector()` 位于 `unstable-custom-transports` 特性之后，开启需自维护选路逻辑。
- **跨 Space 或跨信任组共享**。仅限同一 Space 内已配对设备。
- **中心存储与离线投递**。共享仅在双方同时在线时可用。
- **远端文件内容预览**。只列元数据；预览沿用既有剪贴板文件链路。
- **移动端**。本期桌面专属；协议不得写入只有桌面能实现的语义而不留扩展位。
- **对整盘或家目录的递归全盘索引**。列举按层、分页、限深。

## 4. 总体架构与归属

```text
对端设备 B（被浏览方）                          本机设备 A（浏览方）
┌──────────────────────────────┐            ┌──────────────────────────┐
│ remote-share/1 serve_adapter │◄──元数据──►│ remote-share/1 client    │
│  └ Authorizer → Confine → fs │            │  └ RemoteShareClientPort │
│ 共享根存储（MasterKey 密文）  │            │ 共享根仅本机自管         │
└──────────────────────────────┘            └──────────────────────────┘
           ▲     iroh-blobs：文件字节      ▲
           └───────────────────────────────┘
                        ▲
                本机 daemon（HTTP/WS）
                        ▲
        Go 宿主（仅拖放路径） + React「文件」Tab
```

| 层 | 仓 | 新增 |
| --- | --- | --- |
| 线格式 | `ENGINE:crates/uc-infra/src/network/iroh/remote_share/wire.rs` | 帧编解码 + magic + 分配上限 + `REMOTE_SHARE_ALPN` |
| 端口 | `ENGINE:crates/uc-core/src/ports/remote_share/`（`mod.rs` 只声明与再导出） | `RemoteShareServePort`、`RemoteShareClientPort`、`SharedRootStorePort`、`ShareAuthorizer` |
| P2P 适配器 | `ENGINE:crates/uc-infra/src/network/iroh/remote_share/{serve_adapter,client_adapter}.rs` | `node.rs` 两个注册点（ALPN 数组 + `install_remote_share`） |
| 用例 | `ENGINE:crates/uc-application/src/remote_share/` | 授权、约束、执行、传输编排；`deps.rs` 注入 |
| 存储 | `ENGINE:crates/uc-infra/src/remote_share/` | 加密 `ShareStore` 文档仓库（仿 profile_content_key_vault 形态） |
| 约束内核 | `ENGINE:crates/uc-infra/src/fs/share_confinement.rs` | `cap-std` 封装 + 保留前缀拒绝 |
| daemon 契约与路由 | `crates/uc-daemon-contract/`、`crates/uc-webserver/src/api/remote_share.rs` | DTO、路由、事件投影 |
| 宿主 | `apps/gui-go/` | `EnableFileDrop` + 拖放事件转发 + 一次本机路径探测 |
| 前端 | `apps/gui/src/` | 「文件」Tab、目录树、队列、共享管理 UI 分组 |

装配点为 `ENGINE:crates/uc-engine/src/assembly/sync_engine.rs`；观测枚举为 `ENGINE:crates/uc-observability-contract/src/diagnostics/connectivity/inbound_peer.rs` 的 `InboundPeerProtocol` 变体，并须同步其字符串合同测试 `tests/inbound_peer_rejection_contract.rs`。

**归属原则**：授权判定、路径约束、文件系统操作、字节传输全部在 Engine；本仓只做传输适配与呈现。宿主命令处理器与平台适配器内不得写业务判定。

## 5. 共享根与授权模型

### 5.1 数据模型

```text
ShareStore（单个加密文档，MasterKey AEAD）
  suspended: bool                       # 一键暂停
  roots: [SharedRoot]

SharedRoot
  id: Uuid                              # 对外仅暴露此不透明 id
  path: String                          # 绝对路径，仅存在于密文内
  label: String                         # 用户可读名，同样密文
  grants: { read: bool, write: bool, delete: bool }
  enabled: bool
  added_at_ms: i64
```

**不新增 settings 数据模型分区**（即不扩展 `Settings` 结构体）。总开关语义即"清单为空"，暂停标记与路径同处密文文档内，因此本功能对 `settings.json` 零改动，既避开其既有明文落盘问题，也避开新增 settings 分区的改动面。前端设置页的那个 UI 分组只是本密文文档的编辑器，见 §11.5。

**实现形态：现成先例都在，但都不可直接复用，需按同一形态新写。** Engine 里有两类相近实现：`uc-infra/src/security/profile_storage_upgrade/persistence.rs`（`seal_profile_payload`/`open_profile_payload` + 64 KiB 小上限 + 自有 `write_atomically`）是 **最贴近的模板**；`profile_content_key_vault/persistence.rs` 形态相近但 **整个模块是 `pub(super)`，不可外部复用**，且它走的是 `SecureStorageAccess` + 包装主密钥，不是子密钥派生。二者都不得当作可直接调用的设施写进计划。

密钥材料经 `DeriveSpaceSubkeyPort::derive_subkey(&self, salt: &[u8], info: &[u8]) -> Result<[u8; 32], SpaceAccessError>` 派生（`ENGINE:crates/uc-core/src/ports/space/access.rs:206-213`，HKDF-SHA256、IKM 为 MasterKey），使用独立的 `salt`/`info` 与其它子密钥域分隔。该端口在依赖图中的位置是 `ApplicationDeps.security.space_access_ports.derive_subkey`。

文件位置沿用 vault 目录布局：`AppPaths::from_app_dirs(...).vault_dir`（= `app_data_root/vault`），新增一个 `share-store-v1.json` 常量与 `VaultLayout::share_store_path()` 访问器，与既有 4 个 dotfile 的 `*_FILE` + `*_PURPOSE` 成对写法一致。

推论：`derive_subkey` 在锁定时返回类型化的 `NotUnlocked`，因此解锁前 `ShareStore` 必然不可读、共享能力必然不可用。这是被接受的设计约束而非缺陷，且与既有做法一致——`NotUnlocked` 正是映射到线格式 `Locked` 状态的路径。本机侧呈现见 §9 与 §11.5，对端侧见 §11.4。

### 5.2 授权判定

服务端每次操作按序判定，任一步失败即拒绝且 **fail closed**（对齐 `ENGINE:crates/uc-application/src/clipboard/sync/receive_gate.rs` 的既有做法）：

1. 连接方身份经 `InboundPeerGate` 解析为 `DeviceId`；`IdentityUnresolved`/`Ambiguous`/`LedgerDenied` 直接拒绝。
2. 会话代次匹配当前 Space；重配对设备不得复用进行中的授权。
3. `suspended == false`。
4. `root_id` 存在且 `enabled`。
5. 该根对应本次操作所需的 grant。
6. 路径经 §6 约束成功。

grant 组合规则：覆盖写入要求 `write && delete`；设备内拷贝目标要求 `write`；重命名要求 `write + delete`。

前 1–2 步不是新代码：serve 适配器应在进入业务前复用既有的 `ContentExchangeGatePort`（`ENGINE:crates/uc-core/src/membership/ports.rs:314`）与 `PeerAdmissionPort`，与 `pull_serve_adapter.rs:126` 的检查位置保持一致，拒绝经 `record_inbound_peer_rejection` 记录。本功能新增的只有第 3–6 步。

新增的授权器应照搬 `MemberReceiveGate` 的 fail-closed 形状：`authorize(...) -> Option<Permit>` 而非 `Result`，读取存储失败时记一条固定原因的 `warn!` 后返回 `None`（见 `receive_gate.rs:100-107` 的成员查询失败分支）。隐私边界在无法验证时不得放行，这条理由同样适用于共享白名单。

### 5.3 审计

每次服务端操作记录 `peer_device_id`、`op`、`root_id`、`blake3(relative_path)` 截断值、结果码。**不记录明文路径**（`VISION.md` 日志脱敏）。默认采样常开，不引入新遥测事件名。

## 6. 路径约束内核

本功能唯一真正新增的安全部件，独立可测、独立评审。

采用 `cap-std` 的能力式模型：根目录在启用时 `open` 为目录 fd 并持有；所有操作从该 fd 出发，库层保证无法逃出。客户端参数只含 `root_id` 与相对路径，**协议上不存在绝对路径字段**。

现状核对：`cap-std` 在 Engine 全工作区 **不存在**（所有 `Cargo.toml` 与 `Cargo.lock` 零命中），属新依赖引入；已在用的 OS crate 只有 `libc` 与 `windows-sys`，无 `rustix`/`nix`/`windows`。`ENGINE:crates/uc-infra/src/fs/` 下唯一的路径处理是 `inbound_target.rs:182` 的 `sanitize_basename`（仅取 basename、中和 `/`、`..`、NUL），它做的是 **文件名清洗而非目录约束**，不可当约束器复用；`vault_layout.rs` 只是根 + 固定文件名的布局，无包含性检查。因此本内核是全新代码，不复用上述任一项。

接口形态：

```rust
pub fn resolve(root: &Dir, client_rel: &str) -> Result<CapPath, ConfineDenied>;
```

`ConfineDenied` 为不透明错误，不回显任何路径片段。

**`cap-std` 实际提供的保证（P1 Task 1 已实测订正）。** 本段原先断言"`Dir` 会解析并跟随指向根外的符号链接"，**该断言已被实测推翻**：`cap-std` 4.0.3 在 Linux 上对经指向根外的符号链接打开文件返回 `PermissionDenied`（`"a path led outside of the filesystem"`），即它提供的是 **目录树包含**，不只是路径字符串包含。机制为 `openat2` 的 `RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS`，`EXDEV` 映射到该错误；`openat2` 不可用时退回逐段手工打开。

两点仍要求本层自行加严，不得因"库已挡住"而省略：

- **根内符号链接仍被跟随**（`inside -> public` 成功）。逐段拒绝符号链接因此是有效的策略加严——它防的是"共享子树被一根链接悄悄外扩"，与逃出根是两回事。该前提目前仅由 ADR-030 记录，Task 5 须把它变成 committed 断言。
- **Windows 与 macOS 未实测**，ADR-030 记为"跳过"；跨平台结论在真机执行前不得当作已保证。`cap-primitives` 还显式 **不对 Android 尝试 `openat2`**（seccomp），Android 恒走手工逐段打开路径。

必须覆盖并被测试固定的逃逸形式：`..` 与嵌套 `..`、符号链接链与指向根外的链接、绝对路径、Windows 盘相对路径 `C:foo`、UNC `\\server\share`、备用数据流 `file:stream`、`\\?\` 长路径前缀、大小写不敏感导致的前缀判断失效、**Windows 保留设备名**（`CON`、`PRN`、`AUX`、`NUL`、`CLOCK$`、`COM1`–`COM9`、`LPT1`–`LPT9`，见 §7）、超长与含控制字符的文件名。

**两类形式的覆盖时点是 P2，不是 P1**（P1 终评订正；先前把这两类与上表并列写为"必须覆盖并被测试固定"，高于 P1 实际交付的保证）：

| 逃逸形式 | P1 交付状态 | 关闭它的机制与阶段 |
| --- | --- | --- |
| 两次调用之间的 TOCTOU 竞态 | **未覆盖**。约束内核从不打开末段，`ConfinedTarget::real_path` 是解析那一刻的字符串快照，随后的读写与改名走调用方的环境权限；P1 也没有并发改名的压力用例。逐段行走只把窗口 **收窄**（判定与下行都相对同一个父句柄），没有闭合。 | P2 的前置决定：端口交出 **对外不透明的句柄** 并提供 **相对该句柄（相对已保留的 `Dir`）的操作**，使"检查过的对象"与"被改的对象"在类型上是同一个 fd。在该重画定稿前，P2/P3 不得在 `real_path` 上直接建破坏性操作。 |
| 根目录被替换为指向他处的链接（登记后根被挪走、删除后在同路径重建） | **未覆盖**。根的身份没有从已持有的句柄复核过。`RootGone` 有多个否定来源（登记表未命中、登记路径的 `is_dir()` 存活检查、`register_root` 打不开根），它 **不** 区分"目录消失"与"根从未登记"，后者要另经 `is_registered` 判定；正因如此，根被挪走的三种形态里只有"删除且不重建"可检测并有 committed 断言，而"改名挪走并在同路径重建"与"删除后重建"都检测不到。 | P2：登记时从 **已打开的句柄** 取身份（Linux `dev`/`ino`，Windows file index），在存活检查里复核，不匹配即 fail-closed 成 `RootGone`。 |

这两类的可达前提都是能对根路径的父目录写入（本机用户或同机进程）；对端经协议既删不掉也改不了共享根本身（`..`、绝对路径一律被拒）。因此它们是范围决定，不是对端可达的新通道，但 **不得** 在 P2 之前把它当作已闭合的保证。

根目录 `open` 失败（被移走、权限不足）时该根进入 `unavailable` 态并向对端报告 `root_gone`，不降级为全盘可见。

### 6.1 回收目录前缀保留

每个共享根在 **根内** 有一个回收目录，真实名为 `.uniclipboard-trash-<root_id>`（`root_id` 为该根的 UUID）。

约束层额外施加一条规则：**任何客户端提供的相对路径，其首段匹配 `.uniclipboard-trash-*` 一律拒绝**，返回 `denied`。因此该目录物理位于根内（保证删除是同盘 `rename`，O(1)），但在协议上不可寻址——对端既不能列举、也不能读写、也不能把它改名或删掉。`List` 在序列化边界就过滤掉该前缀，不依赖 UI 约定。

启用某个根时须检查根内是否已存在匹配该模式的条目；存在则拒绝启用并报 `reserved_name`，使该不变量由强制检查保证，而非依赖 UUID 碰撞概率极低这一假设。

回收目录自身的内容只能经 §7 的 `ListTrash`/`Restore`/`EmptyTrash` 操作。

## 7. `remote-share/1` 协议

ALPN：`uniclipboard/remote-share/1`。线格式落在 `ENGINE:crates/uc-infra/src/network/iroh/remote_share/wire.rs`，沿用 active-clipboard pull 的模板（`active_clipboard/pull_wire.rs`）：magic → 大端长度前缀 → postcard 正文，读取时 **先校验长度再分配**，写入时再校验一次。magic 只需在本 ALPN 内唯一，不与全局共享。请求/响应的消息类型就定义在该线格式文件与对应 port 文件中，不放进 `uc-core/src/network/protocol/`（该目录只承载剪贴板领域载荷）。

**只承载元数据，绝不承载文件字节**。单请求上限 64 KiB，单响应上限 2 MiB；列举超出即分页。

请求类型：

| 请求 | 用途 | 所需 grant |
| --- | --- | --- |
| `Hello` | 能力与状态探测 | 无（仅需身份） |
| `ListRoots` | 列出可见根（id + label + grants） | read |
| `List { root, rel, cursor }` | 单层列举，返回分页条目 | read |
| `Stat { root, rel }` | 单项探测 | read |
| `Mkdir { root, rel }` | 新建目录 | write |
| `Rename { root, from, to }` | 重命名/移动 | write + delete |
| `Delete { root, rel }` | 移入该根的回收目录（同盘 `rename`） | delete |
| `ListTrash { root, cursor }` | 列举回收条目，返回原相对路径、删除时间、大小 | delete |
| `Restore { root, trash_id, to }` | 从回收目录还原到树内某处 | write + delete |
| `EmptyTrash { root }` | 永久清空该根的回收目录（唯一的永久删除入口） | delete |
| `CopyWithin { root, from, to }` | 设备内拷贝 | write |
| `StageUpload { root, dir, entries[] }` | 开暂存区并登记一批清单，返回 `staging_id`；可多次调用累积 | write |
| `CommitUpload { staging_ids[] }` | 校验并原子落位这批暂存 | write |
| `AbortUpload { staging_ids[] }` | 丢弃暂存 | write |
| `Export { root, rel }` | 请求对端发布 blob，返回 digest + size | read |

响应统一 `status byte + envelope`。`Hello` 的 status 区分四种对端态：`ok` / `locked` / `unsupported` / `suspended`，前端据此决定 Tab 呈现（§11.4）。

列举条目：`name`、`is_dir`、`size`、`mtime_ms`、`kind`。`name` 为 basename，是协议中唯一的文件名字段。

**相对路径的线文法（宿主无关）。** 所有 `rel`/`from`/`to`/`dir` 字段中的相对路径一律定义为 `/` 分隔的一段段名字，并 **禁止**：空段、`.`、`..`、**任何纯点或纯空格段**（`"..."`、`" "`，以及 `".. "`、`".../x"`、`". /x"` 这类带结尾点/空格的变体）、前导 `/`、反斜杠 `\`、冒号 `:`、**Windows 保留设备名**（`CON`、`PRN`、`AUX`、`NUL`、`CLOCK$`、`COM1`–`COM9`、`LPT1`–`LPT9`；按 **第一个 `.` 之前的基础名** 判定并先做同一条归一化，故 `nul.log`、`CON.txt`、`"COM1 .txt"` 同样禁止），以及任何控制字符（含 NUL）。纯点/纯空格段必须单列禁止项：Windows 在解析前会吃掉段尾的点与空格，于是 `".. "` 折成 `..`（父目录上跳）、`"..."` 与 `" "` 折成非法空名，而只比对字面 `.`/`..` 的筛查会同时放行它们。保留设备名同样必须单列禁止项，理由与上跳不同：它的危险是 **命名空间切换**——Win32 名称解析把 `<root>\CON` 交给设备，共享树整个被绕过；而约束内核从不打开末段（§7 的 `Mkdir`/`StageUpload` 要求目标尚不存在时也能解析），`cap-std` 自己的设备名过滤只作用于库的 open，在这条路径上一次都不会执行，所以这道筛查是 Windows 上唯一的保证。判定统一经一个宿主无关的归一化（ASCII 小写 + 去段尾 `.`/空格）后再比对，与保留前缀判定共用同一条“这个段究竟指向哪个名字”的定义。因此该文法在所有平台上给出同一判定，不随宿主的 `Path` 解析器改变——这一点对 §6 的第二道边界至关重要，因为 `cap-std` 在 Windows 与 macOS 上尚未实测。

**根自身如何被寻址（P1 终评订正）。** 上面的文法禁掉了 `""` 与 `"."` 两种写法，于是 §7 的 `List { root, rel }` **无法** 表达"列举一个根的顶层"。定稿的形态不是哨兵而是端口上的显式方法：`ShareConfinementPort` 提供 `resolve_root(root) -> ConfinedTarget`，交回 `relative` 为空、绝对路径即登记时定稿的根目录的目标；`resolve` 则始终拒绝空串与纯点段。选它而不用"`rel` 传空串代表根"这条哨兵写法，是因为哨兵要么要求适配层在调用内核 **之前** 特判 `rel == ""`（忘记特判的那一路把整道筛查与逐段行走一起绕掉），要么往内核里塞一条不经筛查的旁路——两者都是工期压力下最典型的 bypass 写法。据此 **P2 的线格式必须给出一个能表达"根自身"的写法**（例如把 `rel` 定义为可选字段，缺省即根），并在适配层把它映射到 `resolve_root`；**不得** 把空串或 `"."` 送进 `resolve`。

代价被明确接受：在 Linux 与 macOS 上合法的 `notes:backup.txt` 一类含冒号的文件名、含反斜杠的文件名、**纯点或纯空格命名的段**（Linux 上 `...` 与 `" "` 都是合法文件名），以及 **`CON`、`nul`、`COM1` 一类在 Linux/macOS 上完全合法、只是 Windows 保留的名字**，**无法经本协议访问**。这是为可预测性换来的收窄，不是遗漏；UI 在列举时应对这类条目给出"不可经共享访问"的提示，而不是静默隐藏。

列举为 **尽力而为的一致性快照，不做目录版本化**。游标是不透明值，服务端不为其保留目录状态；列举期间目录被并发改动，同一条目可能重复出现或漏出。客户端按 `name` 去重合并，并在响应带 `truncated` 时提示用户手动刷新。此语义明确写入协议文档，不伪装成原子快照。

注册点：`node.rs` 的 ALPN 预注册数组（`:971`）与 `install_remote_share()`（对齐 `:1312` 的既有 install 形态），加 `uc-engine/src/assembly/sync_engine.rs` 装配调用，以及 `uc-observability-contract` 的 `InboundPeerProtocol` 变体与其字符串合同测试。线格式测试按仓库习惯内联在线格式文件底部的 `#[cfg(test)] mod tests`，用 `tokio::io::duplex` 跑双向编解码。

## 8. 数据传输

### 8.1 上传（A → B，两阶段）

1. A 侧 daemon 把本地路径展开为清单，相对路径复用 `ENGINE:crates/uc-core/src/clipboard/file_set.rs` 的 `FileSetMemberLocation` 语义（ADR-010）。
2. A 对每项 `publish_path` 得到 digest。
3. A 发 `StageUpload{root, dir, entries:[{digest, rel, size}]}`；B 校验 write、在 **根内** 建暂存目录、返回 `staging_id`。清单按 §7 的 64 KiB 请求上限分片，一个上传任务可对应多个 `staging_id`；父任务在全部清单登记完成前不得进入提交。
4. B 逐项 `fetch_to_path(digest, staging/rel)`。此步是标准 blob pull，因此既有 `transfer-progress/1` 反向帧可直接给 A 提供字节级进度，无需新协议。
5. A 发 `CommitUpload`；B 校验 size 与 digest 后原子移动到目标目录。

失败语义：第 4/5 步任何失败均触发暂存丢弃，目标目录不受影响。取消晚于提交点返回 `too_late`（对齐 `crates/uc-webserver/src/api/clipboard.rs:211` 的既有先例）。

重名策略默认自动改名，复用 `apps/gui-go/host_commands_files.go:135` 的 `uniqueTargetPath` 语义；"覆盖"需 `write && delete`。

### 8.2 下载（B → A）

1. A 发 `Export{root, rel}`；B 校验 read、`publish_path` 该文件、返回 digest + size。
2. A 本地 `fetch_to_path(digest, 用户选定路径)`。
3. 进度由 A 侧本地报告器产生（B 不生成反向帧），走同一 WS topic。
4. B 侧临时 blob 由既有 `_temp_tag` 机制抗 GC，完成后解标回收。

### 8.3 任务模型

- 一次传输一个 `transfer_id`，POST 立即返回，进度与终态走 WS，**不轮询**。
- 文件夹 = 父任务 + N 子任务，父进度按字节聚合。
- 并发：blob 拉取复用 `MAX_CONCURRENT_BLOB_PULLS = 4` 的信号量模式；每 peer 并发 RPC 上限取 8，超限排队而非拒绝。
- 跨进程重启不续传：A 侧启动对账把遗留任务标为 `interrupted`，UI 提供一键重传。

## 9. daemon API 与事件

HTTP（全部 L2+，继承既有速率与鉴权中间件）：

```text
GET    /remote-share/state                         本机共享态：suspended / 根数 / 是否锁定
PUT    /remote-share/roots                         本机白名单整体写入
POST   /remote-share/suspend                       一键暂停 / 恢复
GET    /peers/{peerId}/share/state                 转发 Hello
GET    /peers/{peerId}/share/list                  浏览
POST   /peers/{peerId}/share/upload                返回 transferId[]
POST   /peers/{peerId}/share/download              返回 transferId
POST   /peers/{peerId}/share/mkdir|rename|delete|copy|empty-trash
GET    /peers/{peerId}/share/trash                   ListTrash，分页
POST   /peers/{peerId}/share/trash/restore           Restore
POST   /remote-share/transfers/{id}/cancel
```

WS 复用既有 `file-transfer` topic（其 `build_snapshot_event` 返回 `None`，无需做快照），新增事件名 `remote-share.progress`、`remote-share.status_changed`、`remote-share.availability_changed`。事件字段一律 camelCase。

两条写路由 `PUT /remote-share/roots`、`POST /remote-share/suspend` 读写 §5.1 的密文文档，因此 **未解锁时返回 `423 session_locked`**，与内容路由的既有行为一致；不得为渲染设置页而把该文档放进 `content_lock.rs:460` 的锁屏放行清单。`GET /remote-share/state` 只读，锁屏下返回 `200 {locked: true, rootCount: null, suspended: null}`，不含任何路径，使设置页能如实呈现"解锁后方可管理"。

改动面约 10–14 个文件，含 `uc-daemon-contract` 常量与 DTO、`uc-webserver` 路由与 OpenAPI 注册（须同步递增其冻结的基数断言）、`schema/openapi.json` 重生成与 hey-api SDK 重生成。

## 10. Go 宿主职责

刻意保持极薄。宿主 **不参与** 字节传输——本机文件由 daemon 直接读盘。

1. 主窗口开 `EnableFileDrop`；前端以 `data-file-drop-target` 标记投放区。
2. 监听 `events.Common.WindowFilesDropped`，把 `{paths[], targetId}` 转成宿主事件发给前端。
3. 新增一条宿主命令 `stat_local_paths`：对拖入路径返回 `size`/`is_dir`/存在性，供确认条展示。该调用即时返回，不受 `host_invoke.go:121` 的 30 秒命令超时影响。
4. 上传文件夹复用既有 `chooseDirectory`；另存复用 `chooseSaveFile`。
5. 同步手改 `apps/gui/src/lib/ipc-bindings.generated.ts`，并确认 `apps/gui-go/e2e/command-coverage.sh` 两侧对齐。

Linux 上任何外部程序派生须走 `host_helper_env_linux.go` 的清理路径。

## 11. 前端交互

### 11.1 入口

`HistorySidebar.tsx:324` 设备行补 `onClick` → 导航至 `/devices` 并 `setSelection({kind:'peer', id})`，同时把详情面板切到「文件」Tab。设备列表行沿用 `DeviceList.tsx:198` 既有选中行为。

### 11.2 面板位置

`PeerDetailPanel.tsx` 包成 `ui/tabs.tsx` 的「概览 | 文件」。不新增顶级路由，不新增窗口。

### 11.3 内部结构

顶部根选择器 + 面包屑 + 工具条（上传文件 / 上传文件夹 / 新建 / 刷新）；左侧懒加载目录树；右侧列表（多选、行内 `⋯` 与右键菜单）+ 投放区；底部传输条，可展开为队列表。

列举参数：逐层拉取、cursor 分页；**深度上限 8 与每页 2000 项均为客户端约束**（服务端不感知树深度，只受 §7 的 2 MiB 响应上限与 `truncated` 标志约束），超限提示改用列表分页浏览，不静默截断。

### 11.4 Tab 可见性与状态

| 对端态 | 呈现 |
| --- | --- |
| 离线 | Tab 不渲染 |
| `unsupported` | Tab 不渲染（对端版本无此 ALPN） |
| `locked` | Tab 渲染并显示"对端未解锁，共享暂不可用" + 重试 |
| `suspended` | 显示"对端已暂停全部共享" |
| 对端根为空 | 显示"对端尚未开放任何目录"，并提示需在 **那台设备** 上配置；不链到本机设置页，避免误导成"改我自己的共享" |

本机自身未开放任何根时，Tab 仍可正常浏览对端（两件事互不依赖）；"去设置 → 共享配置本机"的引导链接只出现在 §11.5 的设置页与本机设备详情中。

### 11.5 共享管理入口

**设置页新增「共享」UI 分组**（本机自管；注意这是 §9 路由的前端编辑器，不是 `Settings` 结构体的新分区）：逐根路径、label、三个 grant 开关、启停，顶部一键暂停。选择该入口的理由是归属清晰——每台设备只管理自己的暴露面，与"我在对别人开放什么"的心智一致。对端的「文件」Tab **不** 内嵌任何本机共享的编辑入口或跳转链接（理由见 §11.4），避免出现"我的"与"别人的"两套语义混在同一屏。

### 11.6 拖放确认

拖放落地前必须弹确认条："上传 3 项 / 共 128 MB 到 Legion:Downloads/2026-Q3？"。误拖成本不为零。投放到非投放区为无操作。

### 11.7 删除语义

删除经 `Delete` 移入该根的回收目录（§6.1），在树上作为一个 **虚拟节点**「回收站」呈现——它不是路径，由 `ListTrash`/`Restore`/`EmptyTrash` 驱动，因此不受根内真实目录同名条目的干扰。

保留 `TRASH_RETENTION_DAYS = 7`（Engine 侧常量，非用户设置），到期由服务端后台清理。**不提供单项永久删除**；唯一的永久删除入口是「清空回收站」，需 `delete` grant 且二次确认。还原（`Restore`）保留原相对路径作为默认目标，冲突时按 §8.1 的重名策略处理。

### 11.8 状态与持久化

远端目录树与所有远端路径 **只存在于 Redux 内存态，不落任何持久层**。这同时满足密文底线与避免陈旧树。服务端状态数据经 `store/api.ts` 的 RTK Query 端点承载并按 tag 失效；不建立"组件本地缓存 + Redux"双份来源。

## 12. 错误码

稳定枚举，消息中不出现绝对路径：

`locked` · `unsupported` · `unsupported_request` · `suspended` · `revoked` · `denied` · `reserved_name` · `not_found` · `conflict` · `root_gone` · `quota` · `too_large` · `too_many_entries` · `io` · `cancelled` · `too_late` · `interrupted`

## 13. 安全不变量

- **I1** 文件字节只经 iroh-blobs 传输，应用层 AEAD 与 Ed25519 身份认证不得被任何路径绕过。
- **I2** 协议字段中不存在绝对路径；服务端只接受 `root_id` + 相对路径。
- **I3** 不存在任何客户端可构造的路径输入能逃出已授权的根。该不变量由 §6 的测试语料固定。
- **I4** 共享根的路径与 label 只存在于 MasterKey 密文文档内；数据库、缓存、搜索索引、日志、崩溃报告均不得出现明文。
- **I5** 授权判定 fail closed；任何解析、身份、会话、存储异常一律拒绝而非放行。
- **I6** 本功能不新增任何中心存储、离线投递或 store-and-forward。
- **I7** 本功能不使用 LAN HTTP 兼容通道，也不改变其边界。
- **I8** 遥测事件名与既有分析 ID 隔离规则不变；本功能不得新增可关联真实身份的字段。
- **I9** 回收目录前缀对客户端不可寻址：任何以 `.uniclipboard-trash-*` 为首段的路径一律在约束层拒绝，且 `List` 永不返回该前缀。回收条目只能经 §7 的专用 RPC 操作。

## 14. 网络路径可见性

`ENGINE:crates/uc-infra-p2p/src/network/iroh/observed_connections.rs:160` 当前在解构 `PathEvent` 时丢弃 `local_addr`。改为保留，并在 `ConnectionPath` 增加 `local_address`，投影到 `SpaceMember`，在「概览」Tab 展示 `本地地址 → 远端地址 · 直连/中继`。

RTT 是否可在不引入 `unstable` 接口的前提下取到，P8 开工前须以现有 Engine 版本实测确认；取不到则不显示该项，本规格不把 RTT 写入验收条件。

多网卡选路沿用 iroh 内置 `BiasedRttPathSelector`（RTT 竞速、IPv6 优先、粘滞防抖），本规格不改变其行为，也不提供钉选。既有 `addr_filter.rs:184` 的局域网发卡地址过滤继续生效。

## 15. 兼容性与升级

- 旧版本对端无 `remote-share/1`：ALPN 协商失败，`Hello` 得 `unsupported`，Tab 不渲染。不产生错误弹窗，不重试风暴。
- 协议版本后缀 `/1` 起即预留演进位。**双向兼容要求**：任一侧收到自己不认识的操作码，必须回 `unsupported_request` 状态字节，不得解码崩溃或挂断连接让对端超时。新增操作码因此总是向后兼容的；破坏性变更只能靠新增 ALPN 版本号承载。
- `ShareStore` 为新增文档，无历史数据需迁移；但其内部自带 `schema_version`，后续演进须按"未知字段忽略 + 版本递增"读取，使旧版本 daemon 仍能读新版本写入的文档。
- Engine 变更须先发发布标签，本仓只改 `Cargo.toml` 唯一固定标签并同步 `Cargo.lock`。

## 16. 测试与验收

| 层 | 手段 |
| --- | --- |
| 约束内核 | §6 逃逸语料穷举固定为具名单元用例（仓库既有风格），并逐条固定"由哪一道边界挡下"与"错误变体只能是 `Denied`/`ReservedName`"；含 I9 的回收前缀拒绝与 `List` 过滤用例；含共享根自身可寻址（`resolve_root`，见 §7）与 Windows 保留设备名两条。**TOCTOU 竞态用例随 P2 的句柄重画一并交付**（P1 终评订正）：末段从不被本层打开，交回的是解析期字符串快照，因此对着它写的"竞态用例"只能断言本层已经拒绝的那些形状，无法证明"检查的对象与使用的对象是同一个"。该用例的前置条件就是 §6 表格里那条不透明句柄 + 相对保留 fd 操作的机制，先于机制写它等于把结论写进测试名里 |
| 编解码 | 线格式文件内联 `mod tests` 中的双向 round-trip；长度上限在读取侧与写入侧各一条负向用例 |
| 授权 | 全部 grant 组合（2³ = 8）× §7 各请求类型的判定矩阵；fail closed 路径逐条覆盖 |
| 传输 | 两节点回环 iroh 集成测试；上传中途取消/掉线/写满三场景验证目标目录无残留 |
| daemon | 路由级测试 + OpenAPI 冻结基数递增 + 事件 camelCase 断言 |
| 前端 | Vitest：树懒加载、分页超限、投放确认、队列状态机、i18n 六语键对齐 |
| 端到端 | Linux ↔ Windows ↔ macOS 三平台两两组合，真实双向传输 + 删除/重命名各一次，保存可复跑工件 |

约束内核是安全关键件，随机化属性测试对它确有额外价值，但 **Engine 全工作区当前没有 `proptest`/`quickcheck`，也不存在任何属性测试**（`Cargo.lock` 里的 `arbitrary` 只是 `zip` 的传递依赖）。因此基线是穷举具名用例；是否引入 `proptest` 作为 dev-dependency 在 P1 计划中单列一项决定，不与功能实现混在同一次改动里。测试替身按仓库习惯手写内联 stub：`mockall` 虽在 `uc-infra`、`uc-application`、`compatibility/uc-mobile-lan` 的 dev-deps 中声明（且是 iroh 的传递依赖），但 `crates/uc-core/` 下 **零使用**，本功能不改变这一现状。磁盘相关的单测直接用 `tempfile`（`uc-infra` 已声明 `tempfile = "3"`，并有 `test-util` 特性门控）；`uc-testkit::TempDirLease` 的构造器是 `pub(crate)`，从 `uc-infra` 单测中不可达，不要试图引它。

按 `apps/gui-go/AGENTS.md`，宿主侧不写单元测试，验收走真实 WebView → HTTP/WS → daemon 路径；自动化须使用独立沙箱、唯一 `UC_PROFILE`、`UC_GUI_GO_ISOLATED=1`、`UC_DISABLE_SYSTEM_CLIPBOARD=1`。会改变前台窗口或覆盖系统剪贴板的脚本只在专用测试主机运行。

## 17. 实施阶段

一份规格，按依赖排序切段，每段可独立验收：

**实施计划按阶段切成 4 份，不写成一份。** 每份须独立产出可验证软件；把 P1–P9 压进单一执行文档会导致大量凭猜测写出的签名与占位步骤。P1–P4 属 Engine，其计划落在 `ENGINE:docs/exec-plans/active/`（日期命名式，并须登记进同目录 `index.md`）；P5–P9 属本仓。

1. **P1 约束内核 + 加密共享根存储 + 授权模型**（纯 Engine，含逃逸语料与授权矩阵）。最高风险先做。
2. **P2 `remote-share/1` ALPN + `Hello`/`ListRoots`/`List`/`Stat`** 与能力通告。
3. **P3 变更 RPC**：`Mkdir`/`Rename`/`Delete`/`CopyWithin` + 回收目录（`ListTrash`/`Restore`/`EmptyTrash`）与 §6.1 前缀保留。
4. **P4 传输编排**：`StageUpload`/`CommitUpload`/`AbortUpload`/`Export`、下载、进度与取消。
5. **P5 daemon API** + OpenAPI 重生成 + SDK 重生成。
6. **P6 Go 宿主拖放** + `stat_local_paths` + 契约手改。
7. **P7 前端**：侧栏 `onClick`、Tab、树、列表、队列、设置「共享」UI 分组、六语。
8. **P8 本地地址透出**。
9. **P9 三平台端到端验收**。

P1–P4 完成即可用 CLI 与集成测试验收，不依赖 UI。

## 18. 需要新增的 ADR

- **ADR-013 用 `cap-std` 作为共享路径约束原语**：能力式模型对比自研三平台 `openat2`/`O_RESOLVE_BENEATH`/`NtCreateFile` 与纯字符串校验三选项。须明确 `cap-std` 是 Engine 工作区的 **全新依赖**（当前零命中，OS 侧仅有 `libc` 与 `windows-sys`），并给出其 Windows 支持度的核查证据。
- **ADR-014 共享根清单存于 MasterKey 密文文档而非 settings**：记录"解锁前共享不可用"这一被接受的约束，并引用 §2.2 说明不沿用 `auto_save_dir` 明文先例。
- **ADR-015 新增 `remote-share/1` ALPN**：记录排除 LAN HTTP 通道的底线论证，以及元数据走 ALPN、字节走 iroh-blobs 的分工。

## 19. 待确认项

- **内容寻址导致的跨会话可读性**（P4 前必须定稿）。`ENGINE:crates/uc-infra-p2p/src/network/iroh/blobs.rs` 的 blob 服务当前只按 ALPN 与会话门控，**不按请求授权**：任何已准入的 Space 成员只要知道 digest 即可拉取。本规格的 digest 只在授权后的 `Export` 响应中于两设备间传递，正常路径下第三方无从得知；但内容寻址意味着 **相同字节即相同 digest**，若某文件已通过剪贴板历史等其它途径让第三方持有过同一 digest，它即可绕过共享授权直接拉取本功能导出的同一内容。这是既有属性，本功能会放大其影响面。可选处置：为导出内容加一次性随机盐后再寻址（破坏去重收益）、在 blob 服务侧引入按 digest 的短期授权登记表、或接受并在 ADR-015 中书面记录该残余风险。
