# 设备文件共享 —— 现状、问题与进度记录

- **日期**：2026-10-13。
- **性质**：进度与问题快照，供恢复上下文用。不是规格（规格见下），不是执行计划。
- **一句话状态**：需求已定稿，规格已评审通过，P1（安全底座）已在 Engine 实现并通过逐任务评审；但 **P1 落在了落后上游 36 个 commit、且 crate 布局已被上游废弃的基线上**，且 **P1 没有任何调用方**，所以"编译即可用"目前达不到。

## 1. 原始需求

在设备界面上点击一台 **在线** 设备，若该设备在线则出现一个面板，展示该机器的硬盘目录树；凡有权限的目录都允许拖放文件进去，并支持删除、重命名、拷贝等文件操作；拖放不可用时提供上传按钮，支持上传文件夹与文件。目的：方便几台开发机之间互传文件。附带要求设计该面板与既有界面如何配合（弹窗 / 新 tab / 其他）。

追加提出的两个问题：本机有多个网卡，应支持用户设置走哪条 LAN，或自动确定最快的 LAN 线路。

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

## 3. 已完成

### 3.1 文档（产品仓 `UniClipboard`，分支 `hank_dev`，已提交未推送）

- `docs/specs/2026-10-12-device-file-share-spec.md` —— 规格，19 节，含 9 条完成定义、8+1 条安全不变量（I1–I9）、错误码枚举、阶段切分 P1–P9、三份待写 ADR。**已自审并修正 13 处 + 后续 6 处**。
- 相关提交：`2eee0abbf`、`a2dd53acf`、`d9359b44f`、`e53304a0a`、`102e317ae`、`c697b65dd`、`aee0d7197`、`4b2a9da2d`。
- `hank_dev` 领先 `own/hank_dev` **11 个 commit，未推送**。

### 3.2 P1 实现（Engine 仓，分支 `fix/capture-image-from-file-member`，**全部未推送**）

30 个 commit，`dc6cc71b..5bc29597`。交付内容：

| 件 | 位置（P1 当时所用布局） |
| --- | --- |
| `cap-std` 包含强度实测闸门 + 24 条逃逸语料 | `uc-infra/tests/share_confinement_escape_probe.rs` |
| 共享根领域类型 + grant 矩阵 + 宿主无关归一化 | `uc-core/src/ports/remote_share/model.rs` |
| 存储端口 + 约束端口（含根生命周期） | 同目录 `store.rs`、`confinement.rs` |
| 加密共享根清单（Space 子密钥 AEAD、原子替换、截断式读取） | `uc-infra/src/remote_share/store.rs` |
| 路径约束内核（逐段拒绝符号链接、`resolve_root`） | `uc-infra/src/remote_share/confinement.rs` |
| fail-closed 授权器（`Option` 语义、许可密封、日志脱敏） | `uc-application/src/remote_share/authorizer.rs` |
| 装配接入 | `uc-application/src/deps.rs` + `uc-engine/src/assembly/wire/` |
| ADR-030（`cap-std` 选型）、ADR-031（子密钥加密） | `docs/design-docs/decisions/` |
| 执行计划（含 41 处"交付订正"层） | `docs/exec-plans/active/2026-10-12-device-file-share-phase1.md` |

P1 的验收判据逐条映射到了具体测试，见计划末尾"完成判据"表。

### 3.3 本机构建（Linux）

`cargo build --locked -p uc-daemon` 通过（1m14s）；CLI、GUI 宿主、前端 dist 均产出。**但这次构建不含任何 P1 工作**（见 §4.1、§4.3）。

## 4. 阻塞问题

### 4.1【最严重，我的错误】Engine 基线方向搞反

时间线：

1. 第一轮研究派出的 agent 读产品仓固定 rev `0e25f418` 的 cargo 缓存，报告布局为 `uc-infra-p2p` / `uc-infra-local` / `uc-infra-storage` / `uc-infra-crypto` / `uc-infra-content` / `uc-infra-profile` / `uc-infra-security` / `uc-sync-protocol` 等 **14 个 crate**。—— **这是对的**。
2. 我后来发现本地克隆 `/home/hank/Documents/Engine` 只有 **7 个 crate**（`uc-infra` 单个合并 crate，无 `uc-sync-protocol`），据此断定"上游已把 14 个 crate 合并成 7 个，固定 rev 是合并 **前** 的旧布局，实施必须基于主线"，并据此 **改写** 了规格 §2/§4 与计划的全部 `ENGINE:` 路径。
3. 实测：**方向相反**。`origin/main` = `0e25f418` = **14-crate 拆分布局**（更新）；本地 `main` = `0292e147` = **7-crate**，**落后 `origin/main` 36 个 commit、领先 0**。上游在那 36 步里把 `uc-infra` **拆开** 了。本地克隆只是陈旧。

后果：

- P1 的 30 个 commit 全部压在落后 36 步的旧基线上。
- `crates/uc-infra/src/remote_share/` 这条路径在上游 **不存在**，需搬到 `uc-infra-local`（`uc-infra-local/src/fs` 上游确实存在，落点是对的）；`cap-std` 依赖要跟着搬；模块声明要重连。
- 规格 §2 表格、§4 分层表、计划的文件结构表里所有 `ENGINE:` 路径 **方向错误**，是我"订正"进去的。
- P2 的 ALPN 落点应是上游的 `uc-infra-p2p` + `uc-sync-protocol`，即 **第一轮研究的原始结论本来就对**。

### 4.2 P1 无调用方，"编译即可用"从 P1 到不了

P1 只有安全底座：没有 `remote-share/1` ALPN、没有 daemon 路由、没有 UI。即使合并、发版、换 rev 全部做对，`uniclipd` 与 GUI 的行为与现在 **逐字节等价**。要达到"能用"，缺 P2–P7（九段里的后七段）。

### 4.3 产品仓无法消费本地 Engine 工作

`scripts/architecture/check-engine-repository.mjs` 硬性要求：

- `:144-148` 任何包对 Engine 用本地路径 → 报错 `uses a local path for …`
- `:155` / `:178` / `:188` / `:234` 所有 Engine 包必须钉在 **同一个 40 位 rev**，且该 rev 须能从 Git 远端解析
- `crates/AGENTS.md` 反模式清单明列"对 `UniClipboard/Engine` 拥有的包使用本地路径、分支或非发布标签"
- Engine `AGENTS.md` 另规定"交付只通过带校验信息的 GitHub Release"

结论：要让产品构建包含 Engine 的工作，必须先 **推到 `github.com/UniClipboard/Engine` 并发发布标签**。推共享远端、尤其推上游 `main`，属于需明确授权的动作。

### 4.4 分支归属（Ruling 4 的可预期代价）

按用户决定，P1 直接落在 `fix/capture-image-from-file-member` 上，未另开分支。该分支底部压着一个与 remote-share 无关的在途 commit `dc6cc71b fix(capture): keep derived image-from-file reps out of the file-set manifest`（**未进 `origin/main`**）。

- 好处：不碰脏工作树，构建缓存是热的。
- 代价：P1 与该 capture 修复绑在同一分支；该分支若先合并或被废弃，P1 随之走。
- Engine 工作树里还有两人的在途文件全程未动：`uc-infra/src/db/repositories/entry_receive_attempt_repo.rs`（已修改）、`uc-infra/src/clipboard/entry_availability.rs`（未跟踪，**且无 `mod` 声明，是孤儿文件，其 12 个测试从未运行**）。

## 5. 三条已明写"未闭合"的安全保证

P1 无调用方，故当前可接受；**P2 不得在其上建破坏性操作**。

1. **`real_path` 是快照，不是能力。** 约束内核从不打开末段，交回的绝对路径由调用方用环境权限重开 —— 正是 ADR-030 决定 2 排除的"先校验字符串、再按绝对路径重开"两步式。彻底修法：端口交出 **不透明句柄** 并提供 **相对已保留 `Dir` 的操作**。列为 **P2 开工前置决定**。
2. **根生命周期检测不全。** 实测：改名挪走且不重植 ⇒ 可检测（`is_dir()` false → `RootGone`）；**改名后在同路径重植** ⇒ 检测不到，`real_path` 指向未校验的新树；**删除后重建** ⇒ 检测不到。修法：登记时从 **已打开的句柄** 取身份（Linux `dev`/`ino`，Windows file index），存活检查里复核，不匹配 fail-closed。
3. **注册期 `canonicalize` 与 `open_ambient_dir` 不互证。** 两次独立环境解析之间可被换掉，使 `dir` 与 `path` 指向不同树。文档已软化到实际保证，身份复核与第 2 条同一批修。

## 6. 评审留下、P2 开工前必须做的四项

| # | 内容 | 为何要紧 |
| --- | --- | --- |
| R34 | 规格 §16 把"含 I9 的 `List` 过滤用例"列在 **P1** 验收行下，但 P1 不交付 `List`，该测试不存在 | **修正波自己引入的过度声称**，会被 P2 当验收清单照抄。改规格一行 |
| R35 | `resolve_root` 交回 `relative` 为空的 `ConfinedTarget`，结构上就是"拼接基座"；没有任何文字禁止调用方 `real_path.join(对端名字)` | 那样会同时重开刚关掉的三件事（宿主无关筛查、逐段符号链接拒绝、绝对 join 顶掉根）。`List` 就是第一个消费者 |
| R36 | 根级枚举第一次可能让 `.uniclipboard-trash-*` 被对端看见；"I9 要求 `List` 在序列化边界过滤该前缀"这句话在 Engine 仓零命中 | 之前根无法寻址，遗漏无害；现在必须记 |
| R37 | 计划把 `is_windows_reserved_device_name` 记在 Task 3 的 Produces 下，代码却定义在 Task 2 的 `model.rs`；Task 2 的 `model.rs` 草图里没有它、`mod.rs` 草图却从 `model` 再导出 | **照计划文本写会编译不过**。以代码为准改计划 |

另有两条 Nitpick 已裁定接受不改（R38）：ADR-030"库的设备名过滤在这条路径一次都不执行"应限定到末段；探针里 `""`/`"."` 断言排在 `unregister_root` 之后。

## 7. 验证状态（诚实清单）

**跑过并通过**：`uc-core remote_share` 16、逃逸探针 19、`uc-infra remote_share` 15、`uc-application remote_share` 9、`uc-engine dependency_firewall` 34、`cargo check --workspace --all-targets --locked`、`cargo test --workspace --locked --no-run`、`cargo fmt --all -- --check`、`check-rust-style.mjs`、`check-engine-repository.mjs`、`check-observability-privacy.mjs`、`git diff --check`。

**没跑过 / 不能声称**：

- `cargo test --workspace --locked` 的 **运行时执行** 从未跑过（只有 `--no-run` 构建全通 + 四个 crate 的套件）。合并条件应是 CI 的 `tests` job 绿，不是 `checks` 绿。
- **Windows 与 macOS 全程未执行。** ADR-030 实测表那两列是"跳过"，不是"通过"。P1 没有运行期路径碰文件系统（装配只建空表），故今天不会让它们回归。
- "锁定 ⇒ 清单不可读"只用手写 stub 验证过，未对真实 Space 会话验证（要等 P5 的路由）。
- 一次性探针测得的若干行为（尤其"库跟随根内符号链接"）无 committed 断言，P2 继承的是无法从树内复推的结论。
- 7 条 `dead_code` + 1 条 `unused_imports` 告警被 **有意接受**（P1 无调用方的诚实标记，CI 无 `-D warnings`）。条件：P2 经 `facade` 消费；若 P2 改授权器形状则重新分诊，不得 `#[allow]`。

## 8. 关键资产位置

| 内容 | 路径 |
| --- | --- |
| 规格（权威） | `UniClipboard:docs/specs/2026-10-12-device-file-share-spec.md` |
| 执行计划（含交付订正层） | `Engine:docs/exec-plans/active/2026-10-12-device-file-share-phase1.md` |
| ADR-030 / ADR-031 | `Engine:docs/design-docs/decisions/030-…`、`031-…` |
| **38 条裁定 + 逐任务评审记录（唯一载体）** | `Engine:.superpowers/sdd/2026-10-12-device-file-share-phase1/progress.md`（252 行） |
| 各任务实现与评审报告 | 同目录 `task-N-report.md`、`final-fix-report.md` |
| 线框稿（UI 方案对比） | `UniClipboard:.superpowers/brainstorm/1190761-1791828190/content/` |

`progress.md` 那份 ledger **不要删** —— R34–R37 只有它承载。

## 9. 下一步（建议顺序）

1. **纠正规格与计划里的 Engine 布局基线**（方向反了的那条），便宜，且不改会把错误带进 P2 与后续所有移植。
2. **把 P1 重新落到当前 `origin/main`（`0e25f418`）上**：跨 crate 搬文件（`uc-infra` → `uc-infra-local`）、重连模块声明、重跑全套门。这是移植工作，不是 `git rebase`。
3. 顺带清掉 §6 的 R34–R37。
4. **P2**：`remote-share/1` ALPN + `Hello`/`ListRoots`/`List`/`Stat` + 能力通告。到这一步才有用户可感知的能力。
5. P3–P7（变更 RPC、传输编排、daemon API、Go 宿主拖放、前端 Tab + 六语）。
6. 合并与推送：需明确授权（涉及推 `github.com/UniClipboard/Engine` 与发发布标签）。

## 10. 环境事实

- 三台开发机同交换机：Linux 本机 `192.168.99.1`；Windows(Legion) `192.168.99.2`（`ssh windows`，用户 `hankw`，远程 shell 是 cmd，多命令用 `&` 连接）；Mac mini `192.168.99.3`（`ssh mac-mini`，用户 `hank`；交换机断线备用 `ssh mac-mini-wifi` → `192.168.50.170`）。三平台两两组合是 P9 端到端验收的真实测试床。
- 本机工具链：`go` 不在 PATH，需 `/home/hank/sdk/go1.27.1/bin/go`（`apps/gui-go/go.mod` 要 `go 1.26` + toolchain `1.27.1`）；**Wails CLI 未安装**，故出不了完整应用包；`apps/gui-go/build.sh` 在非 Darwin 直接 `exit 2`，仓库无 Linux 打包路径。
- 产品仓另有未跟踪的 `squashfs-root/`（376 MB，解包的 AppImage，不应入库）与 `.superpowers/`（我建线框的工作区，未加 `.gitignore`）。
