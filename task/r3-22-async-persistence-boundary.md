# R3.22 运行期异步持久化边界原子切换执行任务文档

## 0. 执行控制台

| 字段 | 值 |
| --- | --- |
| 生成时间 | 2026-08-17 |
| 最后更新 | 2026-08-18 |
| 证据基线 | 2026-08-17 |
| 分支或提交 | `dev/r3-direct-execution-protocol-quality` / 本文所在唯一 R3.22 HEAD 提交（`refactor(database): establish async persistence boundary`；精确 hash 由提交后 Git 事实记录，避免 commit 内容自引用） |
| 核心证据范围 | `task/roadmap.md` 的 R3.22；`server/src/database` 的同步 Diesel/r2d2/global connection 边界；`server/src/config.rs`；`server/src/service/app_state.rs`；`server/src/service/infra.rs`；`server/src/service/catalog`；`server/src/service/admin`；`server/src/service/runtime/api_key_governance`；`server/src/proxy/{auth,logging}.rs`；Manager/System/Metrics/Record/Stat Controllers；`server/src/main.rs`；`server/Cargo.toml`；现有 SQLite/PostgreSQL 测试夹具与用户已确认的实施决定 |
| 计划过期触发 | R3.22 Roadmap 边界、`server/src/database/mod.rs` 的连接所有权、`FinalConfig` 数据库字段、`AppState` 构造、LogManager 队列合同、API Key Governance 基线/结算、R4.1/R4.2 所有权、Diesel/diesel-async/bb8 版本或“单一原子提交”决定发生实质变化；R4.1 在本文完成前开始实现；当前分支出现另一套异步数据库迁移 |
| 文档模式 | migration-plan |
| 模式选择原因 | 本项一次性替换全部监听后数据库执行、依赖注入、测试夹具和持久化 worker 生命周期，同时保留既有 Schema、领域行为和公共合同，属于运行期持久化兼容边界迁移 |
| 执行权限 | 可执行 |
| 待确认事项 | 无 |
| 当前状态 | 已完成 |
| 状态说明 | 任务 1—12、C1—C4、全部最终门禁与完成情况检查均已闭合；用户确认无未裁定严重问题，R3.22 由本文所在唯一原子提交完整交付 |
| 总体进度 | 100% |
| 当前检查点 | C4（已完成） |
| 下一检查点 | 无 |
| 任务量汇总 | S 0 / M 3 / L 9 |
| 上下文风险 | 高 |
| 下一步任务 | 无；R4.1 需按其独立领域合同另行启动 |

## 0.1 状态口径

| 类别 | 口径 |
| --- | --- |
| 状态 | 未开始 / 进行中 / 部分完成 / 阻塞 / 已完成 / 不再执行；“当前状态”和任务“状态”只能写这些值，解释写入状态说明或后续备注 |
| 进度 | 总体进度按任务量加权计算：S=1、M=2、L=3；公式为 `round(sum(任务权重 * 任务进度百分比) / sum(任务权重))`；“不再执行”的任务在记录原因后从分母排除；分母为 0 时，只有当前状态为“已完成”才写 100%，否则写 0% |
| 任务量 | S：局部小改；M：单模块或相邻模块；L：跨模块、核心路径或迁移类任务 |
| 上下文风险 | 低：局部即可；中：需多文件或历史决定；高：核心路径、跨模块、迁移、排障或大量证据 |
| 检查点 | C1—C4 仅控制同一个工作区变更集的执行顺序与验证，不是提交或部署边界；所有检查点完成前不得创建 R3.22 实现提交 |

## 0.2 全局执行规则

- 本文是 R3.22 的唯一实施指南。后续执行默认只读取本文与当前工作区，不依赖生成本文的 skill 或前序对话。
- R3.22 只有一个原子交付：任务 1—12、C1—C4、必跑门禁和完成情况检查全部闭合后，才允许形成唯一实现提交。检查点完成、局部编译通过或单个后端通过均不得提交、部署或宣称 R3.22 部分交付。
- 指定任务执行：用户要求“完成任务 X”或“完成任务X”时，只执行该任务；执行前检查任务状态、前置任务、阻塞条件、解除条件和当前工作区证据。存在未解除阻塞时先说明阻塞，不得绕过。
- 检查点执行只推进同一个未提交工作区。用户要求“完成 C1”时允许工作区暂时同时包含旧实现与新基础，但旧实现不得被包装成新的兼容池、fallback 或双写合同，且不得提交该状态。
- 单个任务实施过程中允许短暂编译失败；任务标记“已完成”时必须恢复该任务规定的构建与专项测试。任何检查点结束时 `cargo check -p cyder-api` 必须通过。
- 任务完成后更新任务状态、实际完成、验证记录、后续备注、当前状态、总体进度、当前/下一检查点、下一步任务和文档更新记录。验证记录只能写实际运行结果。
- 状态输出：结合本文和工作区证据先列检查点，再使用 `任务 / 检查点 / 标题 / 文档状态 / 实际状态判断 / 阻塞项 / 下一步动作` 输出全部任务；过期、不一致或无法验证的状态必须明确标记。
- 完成情况检查：先对照本文与工作区列出未完成、错误实现、缺失验证、计划偏差、文档过期和优化项；结果先交用户确认。未经确认不得写入完成情况检查记录、追加任务或创建最终提交。
- 任务编号不得重排；新增任务追加到末尾；不再执行的任务保留编号和原因。实现偏离已确认合同必须先把相关任务标记为阻塞并请求用户裁定。
- 实现命令使用原生 Cargo/npm/Docker，并以 `rtk` 作为外层命令运行器；不得使用 `just`。

完成目标口径：

| 用户请求 | 执行范围 | 停止条件 |
| --- | --- | --- |
| 完成任务 X | 只执行任务 X | 任务前置要求、阻塞条件或当前证据不满足 |
| 完成 C1 / 完成C1 | 只执行 C1 内尚未完成的任务 | C1 前置要求或任务门禁不满足；完成后不得提交 |
| 完成到 C2 / 完成到C2 | 从当前状态顺序执行到 C2 结束，包含更早检查点内尚未完成的任务 | 遇到未解除阻塞、必跑验证失败或计划过期触发；完成后不得提交 |
| 全部完成 / 一次性完成 | 按任务顺序执行全部未完成任务，通过 C4 后先进行完成情况检查 | 遇到未解除阻塞、必跑验证失败、计划过期触发或完成情况检查尚未获用户确认 |

## 0.3 全局验证规则

- 每个实现任务结束至少运行 `rtk cargo fmt --check`、`rtk cargo check -p cyder-api` 与任务指定的专项测试；临时红态只允许存在于任务内部，不允许写入“已完成”。
- 数据库行为变更必须使用生产同路径 `DatabaseRuntime` 验证；不得用同步测试 Repository 或 task-local/global 数据库替代。
- SQLite 是默认全量自动化后端；真实 PostgreSQL 17 只执行本文裁定的边界级门禁，不扩展成全领域终态认证。
- 涉及 Proxy/Auth/Governance/Logging 的任务必须运行对应集成回归；涉及 Manager 的任务必须运行对应 Controller/Admin Service 测试；涉及 Metrics/Request Log 的任务必须运行摄取、校准、查询和 drain 测试。
- 任务 9 后每个检查点都必须运行新的 persistence boundary lint；发现 `get_connection()`、运行期 r2d2、数据库模块外 raw Diesel 执行或测试隐式数据库选择即失败。
- 最终门禁包含格式、编译、全量后端测试、Release 构建、Log Lint、Persistence Boundary Lint、Transform Quick Quality Gate、真实 PostgreSQL 17 边界 Smoke、依赖树与 diff check。
- 前端、OpenAPI 和双数据库 Schema 没有目标变更；最终审计以“无相关 diff”为验收。出现相关 diff 时先标记计划偏差，不得以顺手兼容或清理为由保留。

## 0.4 全局禁止项

- 不得把 R3.22 拆成多个实现提交，不得在中间提交临时同步池、双运行时、双写、fallback 或分层半迁移状态。
- 不得引入 SQLx、deadpool-diesel、tokio-rusqlite、第二套 ORM、自研 SQLite worker/channel 协议或直接 `libsqlite3-sys` unsafe interrupt。
- 不得保留全局 `DATABASE`、无参数 `get_connection()`、Service Locator、task-local/thread-local 当前数据库或通用 Storage 单例。
- 不得在基础持久化层自动重试 operation；LogManager 现有三次领域重试是唯一保留的当前数据库重试合同。
- 不得在数据库事务内等待 HTTP、Redis、文件 I/O、sleep、channel、缓存锁或其他非同连接数据库操作。
- 不得让 Controller、Proxy、Service 或 worker 取得原始连接、调用 raw SQL 或直接使用 Diesel RunQueryDsl。
- 不得通过丢弃已开始的 Future 实现写入超时或取消；已开始 operation 必须被 `DatabaseRuntime` 跟踪到最终结果并参与 drain。
- 不得新增公共错误码、Retry-After、Manager/Proxy API 字段、OpenAPI 路径、前端入口、数据库表/列/Migration、Prometheus Endpoint 或持久化 telemetry 表。
- 不得改造 LogManager 的队列容量、重试次数、批处理、drop policy 或磁盘缓冲；这些继续由 R4.2 所有。
- 不得把数据库 Readiness 变成全局 Proxy 准入门禁，不得以过期缓存或数据库错误伪装 Not Found。
- 不得修改 SQLite `synchronous`、`wal_autocheckpoint`、`temp_store` 或 cache size；本项只固定 WAL、foreign keys 和 busy timeout。
- 不得把全部历史 PostgreSQL ignored tests、完整四协议 Matrix、长时间 soak、容量 SLO 或双数据库终态认证纳入 R3.22；这些保留给 R9.13/R9.14。

## 1. 背景、目标与范围

- 背景：当前 `server/src/database/mod.rs` 以 `OnceLock<DbPool>`、Diesel r2d2、`DbConnection` 枚举和无参数 `get_connection()` 提供同步数据库访问。`db_execute!` 将同一同步 block 分别放入 PostgreSQL/SQLite Schema 与 Model 环境编译。async Controller、Catalog、Proxy Auth、Governance、LogManager、Metrics 和后台 worker 会在 Tokio 运行路径直接取得同步连接。
- 适用前提：R3.21 已完成；R4.1 尚未开始；产品继续是单管理员、单实例网关；SQLite 与 PostgreSQL 都是正式支持的数据库；配置仍是启动期 YAML。
- 最终目标：一次性把监听后的所有持久化消费者切到显式注入的 async `DatabaseRuntime`，PostgreSQL 使用真正异步驱动，SQLite 同步驱动只存在于有界、可观察的异步包装内；随后删除全部运行期同步连接所有权与测试捷径。
- 非目标：数据库 Schema/数据迁移、公共 API/UI 变化、R4 Evidence/Artifact、R4.2 日志队列产品化、R8 指标产品、R9 通用任务监督和终态双数据库认证、多实例协议、Retry/Fallback、Request Bundle、通用 Storage Port。
- 成功标准：所有监听后 Controller/Service/Repository/worker 外部 I/O 为 async；业务层不按数据库分叉；`get_connection()` 和数据库 r2d2 不存在；SQLite 不阻塞 Tokio 调度线程；PostgreSQL 使用 `AsyncPgConnection`；取消、超期、饱和、断连、恢复与关闭都有确定合同和自动化证据。
- 影响范围：后端数据库基础、全部 Repository 签名、AppState/Service 构造、Proxy Auth/Governance/Logging、Manager 控制面、运营读模型、Metrics、Readiness、后台任务、启动流程、配置、测试夹具、Cargo 依赖和静态边界检查。
- 明确边界：本文内部任务是同一原子提交的工作分解。实现期间旧代码只作为尚未删除的当前实现存在，不得被重新命名、包装或稳定为兼容层。

## 2. 决策摘要

- 已确认需求：
  - Roadmap 只保留一个 R3.22；R3.23/R3.24 已删除；阶段三为 21 项，总计 109 项。
  - R3.22 使用一份实施文档和一次原子提交完成 Gateway、Manager、运营查询、后台任务、测试与旧边界删除。
  - 业务 Repository 对外统一 async；普通双后端查询保留统一 `db_execute(...).await` 体验；方言差异显式分支。
- 已确认实现约束：
  - PostgreSQL 使用 `diesel-async 0.9.2`、bb8 和 `AsyncPgConnection`；SQLite 使用 bb8、`SyncConnectionWrapper<SqliteConnection>` 和外围有界 admission；不自建 worker/channel。
  - `AppState` 唯一持有 `Arc<DatabaseRuntime>`；Repository 显式接收 `&DatabaseRuntime`；不建设每领域 Repository trait object。
  - 原始连接、`db_execute!`、`db_transaction!` 和 Diesel query execution 只在 `server/src/database/**` 内可见。
  - 监听前使用显式单一同步 `StartupDatabaseConnection` 执行 Migration 与 Secret 准备，用完销毁；不存在同步池。
  - `database_io` 固定为 `max_waiters=64`、`queue_wait_timeout_seconds=1`、`operation_deadline_seconds=30`、`sqlite_busy_timeout_seconds=5`；`db_pool_size=5` 保持现有默认并新增 `1..=128` 校验。
  - Foreground/Background 是仅有的 workload；Readiness 是无排队探测。Background 最大并发为 `db_pool_size > 1` 时 `db_pool_size - 1`，否则为 1；Foreground waiter 存在时 Background 不开始下一 operation。
  - 排队/取得连接阶段允许调用方取消；连接取得且 operation 开始后由 `DatabaseRuntime` 跟踪到最终结果，持有 permit/连接并参与 shutdown drain。
  - 基础层不自动重试；事务内不允许非数据库 await；一次 Repository operation 取得一次连接，事务内只调用 connection-scoped helper。
  - PostgreSQL 单 statement 使用服务端 `statement_timeout`，悬挂事务使用 `idle_in_transaction_session_timeout`；SQLite 以 busy timeout 硬限制锁等待，普通执行超期只观测和降级，不强制中断。
  - SQLite 固定 WAL、foreign keys、busy timeout，不改变 synchronous；启动 foreign key check 非空即失败。
  - 初始数据库连接失败则拒绝启动；运行中断连则 Readiness 降级并由池恢复。暖缓存 Proxy 流量不受全局健康门禁影响，实际数据库依赖点 fail-closed。
  - 内部错误细分，公共 Manager/Proxy 错误合同不变；数据库容量不足不返回 Retry-After。
  - 观测只使用内存 snapshot、低基数结构化状态事件与兼容 `/ready`；无公共观测 API。
  - 关闭时停止生产者、等待已开始 operation 与 LogManager drain；R3.22 不设置进程内强杀截止。
  - 测试全部使用显式生产同路径 async runtime；真实 PostgreSQL 17 只跑边界级强制门禁。
- 已纳入当前计划：依赖与配置迁移、启动单连接、DatabaseRuntime/admission/telemetry/lifecycle、全部 Repository/消费者迁移、测试夹具替换、后台 drain、Readiness、同步边界删除、静态 lint、SQLite 全量与 PostgreSQL 边界验证。
- 明确不纳入当前计划：Schema/Migration、前端/OpenAPI、日志队列新策略、公共 telemetry、全领域 PostgreSQL 终态门禁、R4/R8/R9 产品能力。
- 被拒绝方案及仍有效原因：
  - 三个 Roadmap 提交及容量 1 的临时同步池：会把待删除的不合理结构变成提交边界。
  - SQLx/第二 ORM、自研 SQLite worker：破坏现有 Diesel Schema/Query 复用并扩大迁移面。
  - 全局 async database singleton：延续隐式依赖与测试隔离问题。
  - 通用 operation retry：无法证明写入幂等，存在重复副作用。
  - `sqlite3_interrupt`：Diesel 未暴露安全所有权边界，且超时竞态不能单独证明提交结果。
  - 单一 FIFO 工作负载：后台维护会占满数据库容量；Foreground/Background 派生限制提供最小隔离。
  - 全局 DB 健康准入：会改变暖缓存网关可用性与既有 fail-closed 点。
  - R3.22 全量 PostgreSQL 领域验收：超出异步边界证明范围，终态认证仍由 R9.13/R9.14 完成。
- 待确认事项：无

## 3. 证据摘要

| 证据来源 | 关键事实 | 支撑的决定或任务 |
| --- | --- | --- |
| `task/roadmap.md` R3.22 | R4.1 前必须一次原子切换全部运行期持久化消费者；监听前配置/Migration/Secret 保持同步；公共产品范围不得扩张 | 全局原子边界、任务 2—12 |
| `server/src/database/mod.rs` | `DbPool`/`DbConnection` 同时包装 r2d2 PostgreSQL/SQLite；`get_connection()` 依赖全局 OnceLock；`db_execute!` 同步展开两套 Schema/Model；测试依赖 task-local/thread-local pool | `DatabaseRuntime`、启动单连接、测试替换、任务 2/3/9 |
| `server/Cargo.toml`、`Cargo.lock` | 迁移前 Diesel 2.3.6 开启 r2d2，直接依赖 r2d2 0.8.10；任务 2 解析证明 diesel-async 0.9.2 要求 Diesel ~2.3.9，用户已确认把基线升级到 2.3.12；项目已有 bb8 0.9.1 与 tokio-util | 依赖选择与清理、任务 2/9 |
| `server/src/config.rs`、`.cyder/dev/config/config.default.yaml` | 现有数据库运行配置只有顶层 `db_pool_size: 5`；配置采取启动期默认与校验模式 | 保留旧字段并新增 `database_io`、任务 2 |
| `server/src/service/app_state.rs` | AppState 不拥有数据库；Metrics reconciliation 与 Manager session cleanup 通过未保存的 JoinHandle 启动；session cleanup 同步访问 Repository | 显式注入与 worker 生命周期、任务 7/8 |
| `server/src/controller/system.rs` | async `/ready` 直接调用同步 `get_connection()`；响应只含 `status/database/redis` | 无排队 readiness probe 且 JSON Schema 不变、任务 7 |
| `server/src/service/catalog/service.rs`、`server/src/proxy/auth.rs` | async cache miss 调用同步 API Key/Provider/Model/Source/Patch/Cost Repository；无效 Key 分类再次同步查库 | Gateway 纵向迁移与按依赖点 fail-closed、任务 4 |
| `server/src/service/runtime/api_key_governance/service.rs` | 当前日/月 runtime baseline 未初始化时同步读取日/月 rollup；初始化后主要使用内存/Redis store | 暖缓存继续、跨 bucket 缺基线 fail-closed、任务 4/10 |
| `server/src/proxy/logging.rs` | LogManager 使用容量 100 mpsc、三次 DB retry、Flush command 和未保存 worker；请求完成先更新治理 runtime，再 enqueue Request Log；插入成功后触发即时 Metrics sink | 保留队列产品合同，只迁移 DB/lifecycle、任务 6/8 |
| `server/src/database/metrics.rs` | 普通查询可通过双分支复用；raw SQL 已因 `$1` 与 `?` 占位符显式区分 | 统一 async db_execute 与方言特例、任务 3/6 |
| `server/src/main.rs`、`server/src/service/infra.rs` | Axum graceful shutdown 后只 flush proxy logs 并固定 sleep 1 秒；通用 spawn helper 返回但调用方不保存 JoinHandle | 持久化专用 stop/drain、任务 8 |
| `server/src/database/mod.rs` SQLite 初始化 | WAL/busy timeout 只在测试应用，生产连接未统一；生产与测试锁行为不一致 | 统一 SQLite connection setup、任务 2/3/10 |
| `diesel-async 0.9.2` 官方 API | `AsyncPgConnection` 提供真正异步 PostgreSQL；`SyncConnectionWrapper` 为同步 Diesel Connection 提供 AsyncConnection，并允许同一代码支持多后端；取消事务 Future 不保证关闭事务 | 后端选择、统一 query syntax、受跟踪 operation 与禁止丢弃事务 Future、任务 3 |

- 事实：监听后同步 DB 调用跨越 Proxy、Manager、Metrics、Readiness、后台任务与测试；迁移不能只替换连接池而不改变依赖注入与 Repository 签名。
- 推断：仅分阶段提交 Gateway/Manager/运营路径会要求临时同步兼容池或双运行边界；该结构与用户要求的原子切换冲突。
- 设计决定：内部使用四个执行检查点降低上下文风险，但全部检查点属于同一个未提交变更集，最终只产生一个 R3.22 实现提交。

## 4. 目标方案与实施边界

- 目标行为或交付物：
  - 新建 `server/src/database/{startup,runtime,error}.rs`、测试专用 `server/src/database/test_support.rs` 和 `server/src/bin/persistence_boundary_lint.rs`。
  - `DatabaseRuntime` 以 backend enum 持有 PostgreSQL/SQLite bb8 pool、全局 active/waiting admission、Background 限制、telemetry、operation tracker、shutdown state 和 readiness probe。
  - `StartupDatabaseConnection` 只持有一个同步 PostgreSQL 或 SQLite 连接，负责 Migration/Secret startup，运行期无法构造或访问。
  - `db_execute!`/`db_transaction!` 在 database 模块内把同一 async block 分别放入 PostgreSQL/SQLite Schema/Model 环境；raw SQL 方言差异继续显式 match。
- 所有权与入口：
  - `main.rs`：加载/校验配置 → 创建 startup connection → Migration/Secret 准备 → 销毁 startup connection → 建立/验证 DatabaseRuntime → 构造 AppState → 启动 workers → bind listener。
  - `AppState`：唯一持有 `Arc<DatabaseRuntime>`，并把 clone 注入 Catalog、Admin、Metrics、LogManager、Governance 和其他持久化消费者。
  - `database/*.rs`：保留领域 Repository 所有权；所有运行期公开方法为 async 且显式接收 DatabaseRuntime；事务内 helper 接收具体 connection。
- 数据、状态、接口与配置：
  - Schema 与数据不变，不新增 Migration。
  - `db_pool_size` 默认 5，范围 `1..=128`。
  - 新增 `database_io.max_waiters` 默认 64、范围 `0..=4096`；`queue_wait_timeout_seconds` 默认 1、范围 `1..=60`；`operation_deadline_seconds` 默认 30、范围 `1..=300`；`sqlite_busy_timeout_seconds` 默认 5、范围 `1..=60` 且不得大于 operation deadline。
  - Manager/Proxy/OpenAPI/前端 DTO 与协议错误不变；内部 PersistenceError 在边界映射到既有合同。
- 可观测性、调试与运维：
  - 内存 snapshot 保存 active/waiting、queue rejected、acquire timeout、operation deadline exceeded、execution error、最近成功/错误/恢复时间；仅按 backend/workload/outcome 使用低基数维度。
  - 队列满、等待超时、执行超期、依赖失败与恢复产生限频结构化事件；不记录 SQL、bind、DB URL、Secret 或正文。
  - `/ready` 使用 `try_acquire`/无排队连接探测，保持现有 JSON Schema；饱和或连接失败返回 503。
- 用户、操作或协作指导：
  - 现有 YAML 不需要重命名 `db_url`/`db_pool_size`；生成默认配置新增完整 `database_io`。
  - 初始数据库不可用时进程失败退出；运行中断连后 `/ready` 降级，恢复后自动回到 200。
  - 回滚只需回退唯一实现提交并重启；没有 Schema/Data 逆迁移。旧版本会忽略新增 `database_io` 顶层字段，旧 `db_url`/`db_pool_size` 仍存在。
- 实施策略：先冻结行为与边界，再引入 config/startup/runtime/test foundation；按 Gateway、Manager、运营路径纵向迁移；随后完成 AppState、Readiness、workers、drain 和旧边界删除；最后用 SQLite 全量与边界级 PG17 验证收口。
- 范围裁定：所有当前运行期数据库消费者、测试夹具和后台持久化任务均在本项迁移；没有“后续再迁”的 sync allowlist。
- 保留边界：既有 Repository 领域拆分、双 Schema/Model、同步 startup Migration/Secret、LogManager 容量/重试、公共错误/DTO、暖缓存行为、单实例治理和 R3 Direct Execution 合同保持。
- 替换或废弃边界：全局 OnceLock DB pool、DbPool/DbConnection r2d2 运行时、无参数 get_connection、同步 runtime Repository、task-local/thread-local TestDbContext、未跟踪持久化 worker、固定 shutdown sleep 全部删除。

## 5. 风险与验证

- 主要风险：
  - async PG 与 sync-wrapped SQLite 在事务、取消和 statement timeout 上语义不同；统一业务结果必须建立在后端安全终止能力上。
  - 大量 Repository 签名迁移会产生跨模块编译波；检查点结束必须恢复编译，且中间状态不得提交。
  - SQLite background job 在调用方取消后仍会执行；operation tracker 必须持续持有连接与 permit，防止提前复用。
  - Manager 多表事务和 Cache invalidation 顺序错误会造成已提交数据与 runtime cache 不一致。
  - LogManager/metrics/session worker 未正确停止会让 DatabaseRuntime drain 永不结束。
  - PostgreSQL 只跑代表性边界门禁；全领域终态差异继续由 R9.13/R9.14 审计，本文不得误报为已认证。
- 阻塞条件：
  - `diesel-async 0.9.2` 无法与当前 Rust 1.89+/已确认 Diesel 基线编译，且不能通过锁文件正常解析；解除条件是用户确认新的依赖方案并更新本文。任务 2 已证明旧 2.3.6 基线不兼容，用户随后确认升级到 2.3.12，该次阻塞已解除。
  - 当前分支在任务 1 基线即存在与 R3.22 无关的失败门禁；解除条件是记录失败证据并由用户决定先修复或明确新的基线。
  - 任务 11 无专用 PostgreSQL 17 URL或目标不是可破坏的临时数据库；解除条件是提供专用环境并验证数据库身份。
  - 发现 R3.22 必须改变 Schema、公共 API、日志队列产品合同或引入第二个实现提交；解除条件是停止执行并获得用户对 Roadmap/本文的重新裁定。
- 回归风险：认证错误被误映射为无效 Key、治理 baseline/settlement 漏计、Secret transaction 失去原子性、Manager cache 提前失效、Request Log/Metrics 顺序变化、Readiness 阻塞、SQLite FK/WAL 暴露历史数据问题、关闭阶段丢记录或永久挂起。
- 不验证的内容及原因：完整四协议 PG Matrix、所有历史 PG ignored tests、长时间 soak、容量 SLO、R4 Artifact、R8 UI/指标和 R9 终态双数据库认证不属于 async 持久化边界的最小证明。

| 验证项 | 命令或方法 | 必跑 | 当前结果 |
| --- | --- | --- | --- |
| Rust 格式 | `rtk cargo fmt --check` | 是 | 最终门禁通过（2026-08-18） |
| Rust 编译 | `rtk cargo check -p cyder-api` | 是 | 最终生产编译 0 errors/32 warnings，tests 编译 0 errors/50 warnings（2026-08-18） |
| 后端全量测试（SQLite 默认） | `rtk cargo test -p cyder-api` | 是 | 最终门禁 1407 passed，15 ignored，7 suites，179.80s（2026-08-18） |
| Release 构建 | `rtk cargo build -p cyder-api --release` | 是 | 最终门禁通过：0 errors/32 warnings，108 crates（2026-08-18） |
| 后端日志门禁 | `rtk cargo run -p cyder-api --bin log_lint` | 是 | 最终门禁通过（2026-08-18） |
| 持久化边界静态门禁 | `rtk cargo run -p cyder-api --bin persistence_boundary_lint` | 是 | 最终门禁通过；lint 单测 4 passed（2026-08-18） |
| Transform 快速质量门禁 | `rtk cargo run -p cyder-api --bin transform_quality_gate -- --quick` | 是 | 最终门禁通过：replay passed，contract 11/11，accounting closed，benchmark 7/7（2026-08-18） |
| 真实 PostgreSQL 17 边界门禁 | 在已导出 `CYDER_R322_POSTGRES_SMOKE_URL` 且确认其指向专用临时 PG17 数据库的 shell 中运行 `rtk cargo test -p cyder-api r3_22_postgres_async_boundary -- --ignored --test-threads=1 --nocapture` | 是 | 最终门禁通过：1 passed，1421 filtered out，PG 17.0.10 / 专用身份已核验；最终一次性容器与数据已清理（2026-08-18） |
| 依赖树审计 | `rtk cargo tree -p cyder-api`，确认 diesel-async/bb8 存在且数据库路径不再包含 r2d2 | 是 | 最终审计通过：diesel-async 0.9.2、bb8 0.9.1；r2d2 absent（2026-08-18） |
| Diff 格式 | `rtk git diff --check` | 是 | 最终门禁通过（2026-08-18） |
| 范围审计 | `rtk git diff -- server/migrations front docs/openapi docs/protocol-compatibility.yaml docs/protocol-compatibility.md`，期望无实现差异 | 是 | 最终审计零差异（2026-08-18） |

## 6. 最终结论

R3.22 已由一个未拆分原子提交完成全部监听后持久化边界切换：PostgreSQL 真异步、SQLite 有界异步包装、显式 DatabaseRuntime、完整 async Repository、受跟踪 operation/worker/drain，以及同步全局边界和测试捷径的彻底删除。任务 1—12、C1—C4、最终格式/编译/全量/Release/Log/Persistence/Transform/PG17/依赖/diff/范围门禁与完成情况检查全部闭合；HTTP graceful shutdown 后按 periodic workers stop/join → LogManager close/drain（含 Metrics sink）→ DatabaseRuntime reject/drain/close 的顺序确定关闭，同步运行时所有权、隐式测试数据库选择和数据库 r2d2 已删除。用户确认无未裁定严重问题；32 个生产 warnings/50 个 tests warnings 主要为双后端宏 unused imports 与既有 dead code，记录为非阻塞优化项，不追加 R3.22 范围任务。R4.1 需按独立领域合同另行启动。

## 7. 维护记录

### 7.1 文档更新记录

| 日期 | 更新类型 | 说明 | 验证 |
| --- | --- | --- | --- |
| 2026-08-17 | 基线 | 方案已确认，任务已细化为单一 R3.22 原子提交的可执行清单 | 文档 diff 待检查；实现验证未运行 |
| 2026-08-17 | 任务 1 | 冻结同步消费者、方言特例、worker 与公共行为基线；补齐 Readiness、Manager BaseError、LogManager 固定产品合同测试 | 格式、编译、1377 个默认测试、Log Lint、Transform Quick Gate 均通过 |
| 2026-08-17 | 任务 2 阻塞 | 已实现 database_io 与 startup 单连接主体；Cargo 证明 diesel-async 0.9.2 不能与 Diesel 2.3.6 同时解析，按预设阻塞条件停止 | 2.3.12 解析下 Cargo check 通过；强制 2.3.6 解析失败，等待用户裁定 |
| 2026-08-17 | 任务 2 完成 | 用户确认升级 Diesel 2.3.12；完成依赖、database_io、StartupDatabaseConnection、Secret 显式连接、默认/示例配置与启动顺序 | 格式、编译、配置、startup、clean migration、Secret startup/rollback 专项门禁全部通过 |
| 2026-08-17 | 任务 3 / C1 完成 | 建立双后端 DatabaseRuntime、Foreground/Background admission、tracked operation、deadline/telemetry/readiness/shutdown 与显式 TestDatabase | 格式、编译、配置 1、startup 2、runtime/admission/test support 12 条专项测试通过 |
| 2026-08-17 | 任务 4 完成 | Gateway 的 API Key/ACL/Rollup、Provider/Source/Provider Key、Model/Binding、Patch/Cost 读取与 Catalog/Auth/Governance 消费链切到显式 async DatabaseRuntime | 格式、编译、Catalog 9、Auth 9、Governance 28、Provider Key 14、串行 Direct Execution 252 条全部通过 |
| 2026-08-18 | 任务 5 完成 | Manager Auth、Secret governance 与 API Key/Provider/Source/Provider Key/Model/Binding/Patch/Cost 全部控制面读写切到显式 async Repository；事务后固定 cache invalidation 再 audit | 格式、生产与 tests 编译、Request Patch 1、Admin Service 60、Manager/管理 Controller 101、Secret 17 条全部通过；2 条既有 ignored 未冒充通过 |
| 2026-08-18 | 任务 6 / C2 完成 | Request Log、Metrics marker/rollup/ingest/reconciliation、Stat/Dashboard/Usage 与 Provider Runtime 全部生产消费者切到显式 async DatabaseRuntime；查询为 Foreground，日志写入与摄取/校准为 Background；保留既有分页、批量、聚合与 raw SQL 方言合同 | 格式、生产/tests 编译、Request Log 4、Metrics DB 4、Metrics Service 9、Stat 11、Provider Runtime 13、Record/Metrics/Stat Controllers 17、Logging 4 条通过；C2 复跑 Catalog 9、Auth 9、Governance 28、Admin 60、Controllers 101 条通过，1 条既有 ignored 未冒充通过 |
| 2026-08-18 | 任务 7 完成 | `main` 在 startup connection 销毁后显式创建并首次探测唯一 DatabaseRuntime，再注入 AppState 后 bind；`/ready` 改为 State 同实例无 waiter probe，保留原 JSON/HTTP/Redis 组合合同，饱和时 503、释放后恢复 200，暖 Catalog 与 liveness 不受全局门禁 | 格式、生产/tests 编译、Runtime 13、AppState 5、System Readiness 3、startup 顺序 1、Catalog cache 9 条通过；diff check 与唯一生产 runtime/static readiness 审计通过 |
| 2026-08-18 | 任务 8 完成 | periodic Metrics/session workers 使用 cancellation + 已保存 JoinHandle；LogManager 保存 worker、close sender 后排空已接受 command/sink 并 join；AppState 固定 workers → logs → runtime 关闭顺序，main 删除固定 sleep；所有 overdue 仅记录后继续等待 | 格式、生产/tests 编译、Logging 6、AppState lifecycle 7、Metrics 9、Runtime cancel/queue/deadline/drain 13、successful direct log 1、main shutdown 顺序 1 条通过；diff check 通过 |
| 2026-08-18 | 任务 9 / C3 完成 | 删除 DbPool/DbConnection/global get_connection、Diesel r2d2、隐式 TestDbContext selector 与测试同步 Repository；全部测试改用显式 TestDatabase + DatabaseRuntime；Secret startup 与 PG fixture raw SQL 收敛进 database 窄边界；新增 Persistence Boundary Lint | 格式、生产/tests 编译、1403 个默认测试、Log Lint、Persistence Boundary Lint 及其 3 条单测、依赖树与 diff check 全部通过；两组 `rg` 零命中同步/隐式边界和 database 外 raw query |
| 2026-08-18 | 任务 10 完成 | 补齐 SQLite 硬矩阵：自定义阻塞 SQL function + ticker 证明 Tokio 非阻塞；暂停时钟越过 deadline 后分别验证最终 commit/rollback；精确 waiter 上限/queue timeout；真实 busy lock 失败恢复；文件路径故障恢复；沿用 startup FK check 与跨层 Proxy/Manager/Log/Metrics 证据 | Runtime 16、startup 2、Persistence Lint 4、Proxy cache/fail-closed 2、Manager rollback 1、Logging 6、Metrics reconciliation 3、Readiness 3 条专项通过；全量 1407 passed/14 ignored，格式、编译、Persistence Lint 与 diff check 通过 |
| 2026-08-18 | 任务 11 完成 | 新增单一 ignored `r3_22_postgres_async_boundary` suite 与 PG17 身份安全检查；覆盖 clean/upgrade/startup Secret、async read/write/rollback、session timeout、idle 断连/池恢复、pool saturation、Proxy cache miss/governance、Manager rotation/cache、Request Log/即时 Metrics、workers/log/runtime drain | 本地一次性 `postgres:17`（17.0.10）专用数据库 `cyder_r322_async_boundary`：suite 1 passed；tests 编译 0 errors；格式、Persistence Lint、diff check 通过；容器及临时数据已清理，未运行全领域 PG suite |
| 2026-08-18 | 任务 12 / C4 / R3.22 完成 | 执行最终格式、生产/tests 编译、SQLite 全量、Release、Log/Persistence Lint、Transform Quick、全新专用 PG17、依赖树、同步边界零命中、diff/range、单提交基线与完成情况检查；用户确认后形成唯一原子提交 | 全部技术门禁通过；1407 passed/15 ignored；PG 1 passed；排除范围零 diff；开始至交付无中间实现提交；唯一提交信息为 `refactor(database): establish async persistence boundary` |

### 7.2 完成情况检查记录

| 日期 | 关键检查证据范围 | 检查结论 | 用户确认 | 追加任务 |
| --- | --- | --- | --- | --- |
| 2026-08-17 | 未进行 | 未运行 | 不适用 | 无 |
| 2026-08-18 | 任务 1—12、C1—C4；SQLite/PG17 runtime、Gateway/Manager/运营路径、worker/drain、静态边界、依赖树、全量/Release/Quality/Lint 与排除范围 diff | 无偏差完成，无未裁定严重问题；可优化项仅为非阻塞 unused import/dead code warnings，未追加范围任务 | 用户回复“确定” | 无 |

## 8. 任务清单

### 8.1 检查点规划

| 检查点 | 目标阶段 | 覆盖任务 | 前置要求 | 完成条件 | 必跑验证 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| C1 | 基线、配置、启动与 Runtime 基础 | 1—3 | 无（可独立执行） | 行为基线固定；新配置、StartupDatabaseConnection、DatabaseRuntime、async 测试 runtime 编译并通过基础专项测试；旧运行边界尚未提交 | `rtk cargo fmt --check`；`rtk cargo check -p cyder-api`；配置/startup/runtime 专项测试 | 已完成 |
| C2 | 全部领域 Repository 与消费者迁移 | 4—6 | C1 已完成 | Gateway、Manager、Request Log/Metrics/运营路径都只调用显式 async Repository；检查点结束项目可编译但仍未提交 | `rtk cargo check -p cyder-api`；Gateway/Manager/Metrics/Record 专项测试 | 已完成 |
| C3 | 应用装配、Readiness、worker drain 与旧边界退役 | 7—9 | C2 已完成 | 生产启动只建立一个 DatabaseRuntime；worker 可停止排空；get_connection/r2d2/global/test shortcut 已删除；静态 lint 通过 | `rtk cargo test -p cyder-api`；Log Lint；Persistence Boundary Lint | 已完成 |
| C4 | 双后端边界验证、全量回归与原子交付审计 | 10—12 | C3 已完成；具备专用 PostgreSQL 17 环境 | SQLite 全量、PG17 边界 Smoke、Release/Quality/Lint/diff 全部通过；完成情况检查获用户确认；形成唯一实现提交 | 第 5 节全部必跑门禁 | 已完成 |

### 8.2 任务概览

| 任务 | 检查点 | 标题 | 优先级 | 任务量 | 风险 | 阶段 | 主要验证 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | C1 | 冻结迁移基线与行为护栏 | P0 | M | 高 | 基线 | 当前全量/专项测试、同步调用清单、公共合同快照 |
| 2 | C1 | 落地依赖、配置与启动单连接边界 | P0 | L | 高 | 共享基础 | 配置校验、Migration/Secret startup、Cargo check |
| 3 | C1 | 建立 DatabaseRuntime、admission、telemetry 与 async 测试 runtime | P0 | L | 高 | 共享基础 | runtime/admission/transaction/cancel 基础测试 |
| 4 | C2 | 迁移 Gateway 执行面 Repository 与消费者 | P0 | L | 高 | 核心实现 | Auth/Governance/Catalog/Direct Execution 回归 |
| 5 | C2 | 迁移 Manager 控制面 Repository 与消费者 | P0 | L | 高 | 核心实现 | Admin/Controller/Secret/transaction/cache 测试 |
| 6 | C2 | 迁移 Request Log、Metrics 与运营读模型 | P0 | L | 高 | 核心实现 | Record/Metrics/Stat/Provider Runtime 测试 |
| 7 | C3 | 完成 AppState、启动生命周期与 Readiness 装配 | P0 | M | 高 | 集成切换 | AppState/startup/readiness/cache-degrade 测试 |
| 8 | C3 | 收敛持久化 worker、LogManager 与关闭排空 | P0 | L | 高 | 生命周期 | worker stop、log flush、metrics sink、drain 测试 |
| 9 | C3 | 删除同步运行时边界并建立静态门禁 | P0 | L | 高 | 清理门禁 | 全量测试、Persistence Boundary Lint、依赖树 |
| 10 | C4 | 完成 SQLite 并发、饱和、超期、取消与恢复矩阵 | P0 | L | 高 | 测试验证 | SQLite DatabaseRuntime/Proxy/Manager/worker 全矩阵 |
| 11 | C4 | 完成最小真实 PostgreSQL 17 异步边界门禁 | P0 | L | 高 | 测试验证 | `r3_22_postgres_async_boundary` ignored suite |
| 12 | C4 | 执行全量门禁、范围审计与唯一原子提交 | P0 | M | 高 | 文档审计 | 第 5 节全部命令、用户确认完成情况检查 |

### 8.3 任务详情

#### 1. 冻结迁移基线与行为护栏

- 状态：已完成
- 进度：100%
- 检查点：C1
- 优先级：P0
- 难度：高
- 任务量：M
- 上下文风险：高
- 目标：在跨仓迁移前固定同步消费者、方言特例、公共行为和当前绿色门禁，防止 async 改造混入行为重设计。
- 任务边界裁定：
  - 纳入：枚举所有 `get_connection()`/DbPool/r2d2、数据库模块外 raw Diesel、async 函数内同步 Repository、task-local TestDbContext、worker spawn/flush；固定 `/ready` JSON、Manager BaseError、四协议 ServerError、暖缓存/治理 baseline、LogManager 容量 100/重试 3 的回归证据。
  - 不纳入：实现 async runtime、修改产品行为、创建兼容抽象。
  - 依赖或顺序约束：无；当前基线失败会阻塞后续任务。
- 实施入口：`server/src/database/mod.rs`、`server/src/controller/system.rs`、`server/src/service/{app_state,infra}.rs`、`server/src/proxy/{auth,logging}.rs`、`server/src/service/catalog`、`server/src/service/runtime/api_key_governance`、现有测试模块。
- 涉及范围：证据清单、现有测试补强和本文实际完成记录；不修改 Schema/API。
- 证据引用：第 3 节全部本地代码证据。
- 预期结果：后续任务拥有可核验的迁移清单；被迁移行为具备现有或新增测试；当前失败与 R3.22 引入失败可区分。
- 阻塞条件：现有必跑基线存在与 R3.22 无关的失败。
- 解除条件：记录完整失败命令/输出并由用户确认先修复或更新本文基线。
- 任务特有禁止项：继承全局禁止项；不得把规划清单写成新的实现层或额外任务文档。
- 任务特有执行要求：继承全局执行规则；记录精确文件/函数与对应迁移任务编号。
- 验收标准：
  1. 同步运行路径、测试捷径、方言 raw SQL、后台 worker 和公共合同清单完整。
  2. `/ready`、外部错误、缓存降级、治理与 LogManager 当前语义有可执行回归证据。
  3. 当前 `cargo check`、全量测试、Log Lint、Transform Quick Gate 结果已记录。
- 构建与验证检查点：`rtk cargo fmt --check`、`rtk cargo check -p cyder-api`、`rtk cargo test -p cyder-api`、`rtk cargo run -p cyder-api --bin log_lint`、`rtk cargo run -p cyder-api --bin transform_quality_gate -- --quick`；任务结束不允许临时红态。
- 执行后文档更新要求：记录同步消费者清单摘要、补充的基线测试和真实命令结果。
- 实际完成：
  1. 冻结全局连接与测试捷径：`server/src/database/mod.rs` 的 `DB_POOL`/`global_db_pool`/`get_connection`、`DbPool`/`DbConnection`、`ACTIVE_TEST_DB_POOL`、`TEST_DB_SCOPE_STACK`、`DEFAULT_TEST_DB_POOL` 与 `TestDbContext::{run_sync,run_async,spawn}`，归属任务 2、3、9。
  2. 冻结 Gateway 同步消费者：API Key/ACL/Rollup、Provider/Source/Provider Key、Model/Binding、Request Patch、Cost Repository，以及 `service/catalog/service.rs`、`proxy/auth.rs`、`service/runtime/api_key_governance/service.rs` 和 Proxy direct-execution 消费链，归属任务 4。
  3. 冻结 Manager 同步消费者：Manager Credential/Auth Instance/TOTP Recovery Repository、共享管理 Repository 的 admin 方法、`service/admin/**` 与 Manager Controllers，归属任务 5。
  4. 冻结运营持久化消费者与方言特例：`database/{request_log,metrics,stat,provider_runtime}.rs` 及对应 Service/Controller；`metrics.rs`、`stat.rs` 的 PostgreSQL `$1` 与 SQLite `?` raw SQL 分支归属任务 6。
  5. 冻结生命周期边界：`controller/system.rs::ready_handler`、`service/app_state.rs` 的 Metrics reconciliation/Manager session cleanup、`service/infra.rs::spawn_background_task`、`proxy/logging.rs::LogManager`、`main.rs` 的 flush + 固定 sleep，归属任务 7、8。
  6. 冻结数据库模块外测试 raw Diesel：`controller/auth.rs`、`proxy/{router,direct_execution_regression}.rs`、`service/admin/{api_key,auth}.rs`、`service/app_state.rs`，归属任务 9；生产代码的数据库 raw query 当前集中在 `server/src/database/**`。
  7. 新增 `system_health_and_ready_responses_preserve_the_public_json_contract`，直接锁定 `/health` 与 `/ready` 的 HTTP/JSON 字段合同；新增 `manager_base_error_status_and_code_contract_is_stable` 覆盖全部 Manager `BaseError` status/code 映射。
  8. 将 LogManager 队列容量与插入尝试次数提取为私有常量且不改变行为，新增 `log_manager_queue_and_retry_product_contract_is_stable` 固定容量 100、最多 3 次尝试；四协议错误继续由 `protocol_error_contracts_cover_all_108_proxy_and_8_router_combinations` 覆盖，缓存过期/清空回源与治理基线继续由 Catalog/Governance 现有专项测试覆盖。
- 验证记录：
  1. `rtk cargo fmt --check`：通过。
  2. 三条新增专项测试分别运行并通过：每条 1 passed、1390 filtered out。
  3. `rtk cargo check -p cyder-api`：通过，0 errors；保留 13 个迁移前既有 warnings。
  4. `rtk cargo test -p cyder-api`：通过，1377 passed、14 ignored、6 suites，167.93s。
  5. `rtk cargo run -p cyder-api --bin log_lint`：通过，`Log lint passed`。
  6. `rtk cargo run -p cyder-api --bin transform_quality_gate -- --quick`：通过，replay passed、contract 11/11、accounting closed、benchmark 7/7。
- 后续备注：
  1. 新增 LogManager 常量后首次专项编译暴露 `usize` 到 `Duration::from_millis(u64)` 类型不匹配；已将尝试次数常量改为 `u64` 并重新执行全部任务门禁，最终无红态。
  2. 未发现与 R3.22 无关的基线失败；任务 2 无基线阻塞项。

#### 2. 落地依赖、配置与启动单连接边界

- 状态：已完成
- 进度：100%
- 检查点：C1
- 优先级：P0
- 难度：高
- 任务量：L
- 上下文风险：高
- 目标：固定 diesel-async/bb8 依赖与 `database_io` 配置，并把 Migration/Secret startup 从全局池拆为显式单同步连接。
- 任务边界裁定：
  - 纳入：新增 diesel-async 0.9.2 features `postgres/sqlite/bb8`；新增 DatabaseIoConfig 与确定的默认/范围/关系校验；新建 `database/startup.rs`；让 Migration 与 Secret 准备显式消费同一 startup connection；SQLite startup 切 WAL、运行 migration、foreign key check；生成默认 YAML。
  - 不纳入：运行期 pool、Repository async 迁移、Schema Migration、配置字段重命名、环境变量/UI。
  - 依赖或顺序约束：任务 1 完成；旧 r2d2 依赖只在未完成的工作区迁移阶段保留，任务 9 必须删除，任何中间状态不得提交。
- 实施入口：`server/Cargo.toml`、`Cargo.lock`、`server/src/config.rs`、生成默认配置逻辑、`server/src/database/mod.rs`、新 `server/src/database/startup.rs`、`server/src/service/secret_encryption.rs`、`server/src/main.rs`、startup/migration tests。
- 涉及范围：依赖、配置、启动数据库生命周期、Secret 测试；无运行期行为切换。
- 证据引用：`server/Cargo.toml`；`FinalConfig`；`init_sqlite_pool/init_pg_pool`；`prepare_secrets_before_startup`。
- 预期结果：启动数据库工作无需同步池和 global get；配置完整可校验；startup connection 用完销毁。
- 阻塞条件：diesel-async 版本无法与当前 Diesel/Rust 编译；WAL 或 foreign key check 暴露现有数据库不一致。
- 解除条件：依赖不兼容时请求用户更新依赖决定；数据不一致时保存证据并请求独立数据处置，不在本项静默修复。
- 任务特有禁止项：不得使用 async connection 执行 startup Migration；不得为旧配置创建隐式别名或重命名 db_url/db_pool_size。
- 任务特有执行要求：Migration 与 Secret preparation 的 transaction/rollback 行为必须保持；启动日志不得输出 DB URL/Secret。
- 验收标准：
  1. DatabaseIoConfig 默认、边界、0/null、关系约束和生成 YAML 测试通过。
  2. SQLite startup 确认 WAL、foreign_keys、foreign_key_check；不修改 synchronous。
  3. PostgreSQL/SQLite startup connection 在 runtime pool 创建前销毁。
  4. 现有 Migration 与 Secret rotation/rollback 测试保持。
- 构建与验证检查点：格式、Cargo check、配置测试、SQLite migration smoke、Secret startup tests；任务结束不允许临时红态。
- 执行后文档更新要求：记录依赖解析版本、配置字段、startup 测试结果和偏差。
- 实际完成：
  1. 已在 `server/Cargo.toml` 增加 `diesel-async 0.9.2` 的 `postgres/sqlite/bb8` features；按用户确认将 Diesel 基线从 2.3.6 升级到 2.3.12，`Cargo.lock` 与依赖树解析为唯一 Diesel 2.3.12。
  2. 已新增 `DatabaseIoConfig`，固定 `max_waiters=64`、`queue_wait_timeout_seconds=1`、`operation_deadline_seconds=30`、`sqlite_busy_timeout_seconds=5`；实现 `db_pool_size 1..=128`、全部字段边界、0/null 与 busy timeout 不大于 operation deadline 校验，并纳入生成默认 YAML。
  3. 已新增 `database/startup.rs::StartupDatabaseConnection`：PostgreSQL/SQLite 各持有一个同步连接并运行既有 clean/upgrade migration；SQLite 固定 WAL、foreign keys、busy timeout，在 migration 后执行 `foreign_key_check`，不设置 `synchronous`。
  4. 已把 `prepare_secrets_before_startup` 改为显式接收同一 startup connection；`main.rs` 当前顺序为建立并迁移 startup connection、执行 Secret transaction、销毁 startup connection、构造 AppState。旧 r2d2 pool 的 migration ownership 已移出生产 pool 构造，测试 pool bootstrap 暂经显式 startup connection，等待任务 3/9 完全替换。
  5. 已将 Secret startup 测试改为显式生产 startup connection，并增加 main 顺序、SQLite WAL/FK/busy/migration/保留默认 `synchronous=FULL` 与非空 foreign key check 拒绝启动测试；startup 建立失败只向进程输出低基数错误类别，不输出 DB URL 或 Secret。
  6. 已把 `db_pool_size` 与完整 `database_io` 写入受跟踪 `config.sample.yaml`，并由配置测试同时核对程序默认、生成默认 YAML、示例 YAML、边界、0/null 与字段关系。
- 验证记录：
  1. `rtk cargo fmt --check`：在获批依赖组合和最终实现上通过。
  2. `rtk cargo check -p cyder-api`：Diesel 2.3.12 + diesel-async 0.9.2 下通过，0 errors、13 个既有 warnings。
  3. `database_io_uses_generated_defaults_and_validates_all_boundaries`：1 passed；`sqlite_clean_upgrade_chain_from_empty`：1 passed。
  4. `database::startup::tests::`：最终 2 passed；覆盖 pragma/migration/default synchronous 与 foreign key violation。首次 foreign key fixture 使用无效 ACL scope 导致 CHECK violation，改用合法 rollup 行制造单一 FK violation 后重跑通过。
  5. `service::secret_encryption::startup_tests::`：最终 10 passed、1 ignored；首次 main 源码顺序断言因调用换行失配，改为分别断言 startup establish/secret/drop/AppState 顺序后重跑通过。
  6. `rtk cargo update -p diesel --precise 2.3.6`：失败；Cargo 报告 diesel-async 0.9.2 要求 `diesel = "~2.3.9"`，候选 2.3.6 不匹配。
  7. `rtk cargo tree -p cyder-api -i diesel-async` 与 `-i diesel`：确认 diesel-async 0.9.2、唯一 Diesel 2.3.12；`rtk git diff --check` 通过。
- 后续备注：
  1. 曾命中任务 2 预设阻塞条件“diesel-async 0.9.2 无法与 Diesel 2.3.6 通过锁文件正常解析”；用户已于后续回复“确定”，批准升级到 2.3.12，阻塞已解除并完成重验。
  2. 未改变 Schema/Migration、公共 API、前端或 Secret transaction/rollback 行为；旧 r2d2 仅作为同一未提交工作区的待迁移边界保留至任务 9。

#### 3. 建立 DatabaseRuntime、admission、telemetry 与 async 测试 runtime

- 状态：已完成
- 进度：100%
- 检查点：C1
- 优先级：P0
- 难度：高
- 任务量：L
- 上下文风险：高
- 目标：建立双后端统一 async 执行基础，并在任何领域迁移前证明 admission、transaction、取消和 shutdown 核心语义。
- 任务边界裁定：
  - 纳入：新 `database/runtime.rs`、`database/error.rs`、`database/test_support.rs`；AsyncPgConnection/SyncConnectionWrapper bb8 pools；Foreground/Background admission；typed PersistenceError；memory telemetry；tracked operation；readiness try probe；shutdown state；crate-private async db_execute/db_transaction；生产同路径 TestDatabase。
  - 不纳入：领域 Repository 方法、公共 telemetry API、通用任务 supervisor、自研 SQLite worker。
  - 依赖或顺序约束：任务 2 完成；DatabaseRuntime 对旧运行路径暂时无消费者，该状态只存在于未提交工作区。
- 实施入口：新数据库基础文件、`server/src/database/mod.rs`、`server/src/schema/{postgres,sqlite}.rs`、双 backend row model macros、测试支持。
- 涉及范围：连接所有权、admission、operation lifecycle、backend session setup、错误/观测、测试 runtime。
- 证据引用：当前 DbPool/DbConnection/db_execute/TestDbContext；diesel-async SyncConnectionWrapper/AsyncConnection 合同。
- 预期结果：同一个 async query block 在双后端编译；每次 operation 一次连接；事务持有同一 permit/connection；取消方不能提前释放已开始资源。
- 阻塞条件：SyncConnectionWrapper 无法通过 bb8 正确建立/验证连接；operation tracking 无法在调用方 drop 后保留连接所有权。
- 解除条件：提供最小复现并请求用户重新裁定后端方案；不得退回裸 spawn_blocking 或自研 worker。
- 任务特有禁止项：事务 closure 不得捕获 AppState 或执行非 DB await；基础层不得 retry；不得暴露 raw connection。
- 任务特有执行要求：
  1. queue full 立即 QueueFull；等待超过 1 秒为 AcquireTimeout；Background 限制和前台 waiter 规则精确实现。
  2. PostgreSQL 设置 statement_timeout/idle_in_transaction_session_timeout；SQLite connection setup 设置 foreign_keys/busy_timeout 并验证 WAL。
  3. operation 开始后由 runtime-owned tracked task 持有 permit/connection；shutdown 先拒绝新 operation，再等待 active=0。
- 验收标准：
  1. pool/admission 上限、max_waiters=0、wait timeout、Foreground/Background、公平边界测试通过。
  2. transaction commit/rollback、禁止嵌套 acquisition、调用方 cancellation、runtime shutdown/drain 测试通过。
  3. telemetry 计数、状态事件限频、无 SQL/bind/URL 信息测试通过。
  4. TestDatabase 无 task-local/thread-local/global 选择且使用生产同类型 DatabaseRuntime。
- 构建与验证检查点：格式、Cargo check、database runtime/error/admission/test_support 专项测试；C1 结束执行 C1 表中门禁。
- 执行后文档更新要求：记录目标类型/方法、专项测试与 C1 结果。
- 实际完成：
  1. 新增 `database/error.rs` 的 typed `DatabaseRuntimeInitError`/`PersistenceError`，以及 `database/runtime.rs` 的 `DatabaseRuntime`、`DatabaseBackendKind`、`DatabaseWorkload`、`RuntimeConnection` 和内存 `DatabaseRuntimeSnapshot`。
  2. PostgreSQL runtime 使用 `AsyncPgConnection` + diesel-async bb8，连接建立时设置 `statement_timeout`/`idle_in_transaction_session_timeout`；SQLite 使用 bb8 + `SyncConnectionWrapper<SqliteConnection>`，每连接设置 WAL、foreign keys 与 busy timeout。pool 固定 `min_idle=1`、禁用连接建立自动重试，并在构造返回前建立首连接。
  3. admission 以 pool-size capacity、`max_waiters` 和 Background semaphore 实现：queue full 立即拒绝，等待超时返回 AcquireTimeout；Background 并发为 `pool_size>1 ? pool_size-1 : 1`，Foreground waiter 存在时 Background 不取得下一 operation。
  4. operation 在连接取得后由 runtime-owned Tokio task 持有连接与 permit；调用方 drop 不会取消已开始 Future。deadline 到达只记录并继续等待确定结果，不通过丢弃 Future 实现超时；shutdown 拒绝新 operation 并等待 active=0。
  5. 增加无排队 `readiness_probe`：只使用 capacity `try_acquire`，饱和立即失败且不进入 waiter；连接断连由 bb8 后续建立恢复。实现 queue rejected、acquire timeout、deadline、execution error 与最近成功/错误/恢复时间 snapshot，状态事件只含 backend/workload/outcome 且 60 秒限频。
  6. 增加 crate-private async `runtime::db_execute!`/`db_transaction!` 双后端编译环境；保留旧 root 同步宏供未迁移代码，任务 9 删除旧边界后收敛命名。用 operation task-local marker 仅检测并拒绝嵌套 acquisition，不保存或选择数据库实例。
  7. 新增 `database/test_support.rs::TestDatabase`：每个 fixture 显式持有生产同类型 `Arc<DatabaseRuntime>` 与独立临时 SQLite 文件，无 task-local/thread-local/global 数据库选择。
  8. 修正实现自检发现的 `Notify::notify_waiters` 丢唤醒窗口：admission/drain 等待 Future 先 `enable()` 注册，再检查状态/permit，确保 capacity release 与 active-zero 通知不会在检查和 await 之间丢失。
- 验证记录：
  1. `rtk cargo fmt --check`：通过。
  2. `rtk cargo check -p cyder-api`：通过，0 errors；当前 28 warnings 中 15 个为任务 3 foundation 尚未被任务 4—6 消费产生，既有 13 个 warnings 不变。
  3. `database::runtime::tests::`：12 passed，覆盖 async query、双后端 transaction block 的 SQLite commit/rollback、max_waiters=0、queue timeout、排队取消、Foreground 优先、Background capacity reservation、nested acquisition、started caller cancellation + drain、deadline 不丢 Future、telemetry rate-limit/recovery/safe snapshot、显式隔离 TestDatabase 与 SQLite runtime session pragma。
  4. C1 `database_io_uses_generated_defaults_and_validates_all_boundaries`：1 passed；`database::startup::tests::`：2 passed；再次运行 Runtime 12 条均通过。
  5. 实现过程中首次 transaction/runtime 专项 10 条通过；新增测试时曾因测试模块全局导入 `diesel_async::RunQueryDsl` 与 Atomic `load` 方法解析冲突导致编译失败，已把 trait import缩到 SQL closure 内并在最终门禁重跑通过。
- 后续备注：
  1. C1 已闭合但仍是同一个未提交工作区；DatabaseRuntime 目前无生产领域消费者是检查点设计的预期中间状态，不得提交。
  2. 任务 4—6 迁移消费者后 foundation unused warnings 应自然消失；任务 9 最终删除旧同步宏与隐式测试边界。

#### 4. 迁移 Gateway 执行面 Repository 与消费者

- 状态：已完成
- 进度：100%
- 检查点：C2
- 优先级：P0
- 难度：高
- 任务量：L
- 上下文风险：高
- 目标：把 Proxy 请求进入、认证、治理、Catalog/Provider Key/Source 选择和请求完成结算全部切到显式 async Repository。
- 任务边界裁定：
  - 纳入：API Key/ACL/rollup、Provider/Source/Model/Binding/Patch/Cost runtime reads；Catalog cache load/reload；Auth cache miss 与 inactive classification；Governance baseline/settlement；Provider Key runtime selection；Proxy pipeline/executor 的持久化调用；请求完成治理结算。
  - 不纳入：Manager CRUD、Request Log DB insert/query、Metrics/Stat、公共错误变化、全局 DB health gate。
  - 依赖或顺序约束：C1 完成；共享 Repository 文件只迁移本项消费的方法，剩余 admin/ops 方法由任务 5/6 同一工作区继续迁移。
- 实施入口：`database/{api_key,api_key_acl_rule,api_key_rollup,provider,upstream_source,model,model_source_binding,request_patch,cost}.rs`、`service/catalog`、`proxy/auth.rs`、`service/runtime/api_key_governance`、provider key selection、Proxy pipeline/executor。
- 涉及范围：四协议 Proxy 的发送前 DB 依赖与完成结算；Cache/Secret/Direct Execution 合同保持。
- 证据引用：Catalog get_or_load 同步 closure、Auth classify_missing_active_api_key、Governance rollup baseline、completion recording。
- 预期结果：四协议 Gateway 监听后不再调用同步 DB；暖缓存继续，无实际 DB 依赖时不受 readiness 全局阻断。
- 阻塞条件：迁移要求新增公共 Proxy code/stage、改变治理口径或引入重试/fallback。
- 解除条件：停止并请求用户更新 R3.22/Roadmap；不得自行扩大合同。
- 任务特有禁止项：DB unavailable 不得映射 invalid key/Not Found；过期 positive cache 不得兜底；治理 baseline 缺失必须 fail-closed。
- 任务特有执行要求：每个 Repository operation 显式标记 Foreground；完成结算开始后遵守确定结果与 tracked operation 规则。
- 验收标准：
  1. Cache hit/miss、disabled/expired/deleted Key、DB unavailable 与 recovery 测试保持既有协议结果。
  2. 当前 bucket 暖治理继续；跨 bucket baseline 失败关闭；成功/错误/取消结算口径不变。
  3. Source/Provider Key/Model/Patch/Cost cache miss 与 direct execution 单次调用回归通过。
  4. 目标调用链没有同步 Repository 或 raw Diesel。
- 构建与验证检查点：格式、Cargo check、Catalog/Auth/Governance/Provider Key/Direct Execution 专项测试；任务结束不允许临时红态。
- 执行后文档更新要求：记录迁移方法、缓存/治理回归结果与偏差。
- 实际完成：
  1. 为 `api_key`/ACL/rollup、Provider/Source/Provider Key、Model/Binding、Request Patch 与 Cost 增加显式接收 `&DatabaseRuntime` 的 async read operation；全部标记 `Foreground` 并在 `server/src/database/**` 内使用 diesel-async query execution。Provider aggregate 的 Provider + Source 读取通过 connection-scoped helper 复用同一 operation/connection，没有嵌套 acquisition。
  2. `CatalogService` 现显式持有注入的 `Arc<DatabaseRuntime>`；reload、API Key cache miss/invalidation、Models Catalog、Provider、Model、Provider Key snapshot、Patch 与 Cost loader 全部 await async Repository。`CacheModel::from_db_with_bindings` 接收已异步读取的 Binding，Gateway 路径不再从 cache type 隐式取同步连接。
  3. Auth inactive classification 改为通过 `app_state.database` 异步读取完整 API Key；不存在、disabled、expired、deleted 与数据库故障继续保持不同协议结果，数据库错误不会写 negative cache 或伪装 invalid key/Not Found。暖 positive cache 不加全局 readiness 门禁。
  4. `ApiKeyGovernanceService` 现显式持有同一 DatabaseRuntime，日/月 baseline 通过 async rollup Repository 加载；当前 bucket 已有 runtime snapshot 时继续无需 DB，跨 bucket baseline 依赖失败保持 fail-closed。completion 的 Redis/内存口径与 lease 释放顺序未改变。
  5. `AppState` 在当前原子工作区已把同一 runtime 注入 Catalog 与 RuntimeStateBackend/Governance；Provider Key cache refresh 和 selector 保持 trusted/fail-closed snapshot 合同。任务 7 继续负责最终启动、Readiness 与 lifecycle 装配，任务 9 删除仍供未迁移 Manager/Ops 使用的同步边界。
  6. 新增 `async_auth_cache_and_database_failure_contracts_are_stable`，覆盖 cold miss、warm hit、disabled/expired/deleted、数据库不可用和恢复；新增 `warm_bucket_continues_but_cross_bucket_baseline_failure_is_closed`，覆盖治理暖 bucket 与跨 bucket 失败关闭。故障测试只用同步 fixture 改变表可用性，所有被验收的读取和错误路径均经过生产同类型 DatabaseRuntime。
- 验证记录：
  1. `rtk cargo fmt --check`：通过。
  2. `rtk cargo check -p cyder-api`：通过，0 errors、19 warnings；其中 13 个为既有 warnings，6 个为 `db_transaction` 尚待任务 5 使用、旧 provider selection 同步方法待清理及 runtime test/foundation 可见性产生的迁移 warnings。
  3. `service::catalog::service::tests`：9 passed；`proxy::auth`：9 passed；`service::runtime::api_key_governance`：28 passed；`service::runtime::provider_key_selection`：14 passed。
  4. `proxy::direct_execution_regression` 首次并行运行 251 passed、1 failed；失败项 `responses_target_precommit_client_cancellation_returns_499_and_releases_once` 在组内负载下得到 504，隔离重跑 1 passed。随后使用 `--test-threads=1` 串行重跑完整组：252 passed、1156 filtered out，895.94s。
  5. 对 `service/catalog/service.rs`、`proxy/auth.rs`、Governance 与 Provider Key runtime consumer 执行目标调用链静态检索：生产路径无 `get_connection()`、旧同步 Repository 调用或 raw Diesel；命中的 `get_connection()` 仅位于新增 SQLite 故障 fixture。
- 后续备注：
  1. 未新增公共 Proxy code/stage、Retry-After、API/OpenAPI/前端/Schema 变更；未增加数据库 retry、fallback 或全局 DB health gate。
  2. 首次 Direct Execution 并行失败已由隔离与完整串行结果证明为负载时序波动；没有放宽断言、延长产品 timeout 或修改取消行为。
  3. C2 尚未闭合；任务 5、6 继续在同一未提交工作区迁移 Manager 与运营持久化消费者。

#### 5. 迁移 Manager 控制面 Repository 与消费者

- 状态：已完成
- 进度：100%
- 检查点：C2
- 优先级：P0
- 难度：高
- 任务量：L
- 上下文风险：高
- 目标：一次性迁移全部 Manager Auth 与配置治理读写，保持事务、安全、Secret 和 Cache 提交顺序。
- 任务边界裁定：
  - 纳入：Manager Credential/Auth Instance/TOTP Recovery、API Key/ACL、Provider/Source/Provider Key、Model/Binding、Request Patch、Cost Catalog 的全部 admin query/command/transaction；Admin Service 与 Controllers；Manager session cleanup Repository 方法。
  - 不纳入：公共 DTO/OpenAPI/前端、认证产品逻辑、Secret 格式、审计产品化。
  - 依赖或顺序约束：任务 4 完成；共享 Repository 的 admin 方法在本项完成后全部 async；worker 调度留给任务 8。
- 实施入口：`database/{manager_credential,manager_auth_instance,manager_totp_recovery_code,api_key,api_key_acl_rule,provider,upstream_source,model,model_source_binding,request_patch,cost}.rs`、`service/admin`、Manager Controllers。
- 涉及范围：Manager Auth、Secret governance、所有配置 CRUD 与 Cache invalidation。
- 证据引用：当前 manager_credential 已含 PG/SQLite 专用事务 helper；Admin Service async 方法内部调用同步 Repository；BaseError 公共映射。
- 预期结果：Manager async Handler 只通过 typed async Repository；事务内只含数据库 await；commit 后才 invalidation/log。
- 阻塞条件：任何命令无法在既有 Schema/公共 API 下保持原子性或 fail-closed。
- 解除条件：保存最小失败证据并请求用户重新裁定；不得增加 Schema/API 兼容层。
- 任务特有禁止项：不得在 transaction 内加解密、访问 Redis/HTTP、发送 channel 或持有 Cache lock；不得把数据库错误细节扩大到新公共字段。
- 任务特有执行要求：预计算/验证在 transaction 前，connection-scoped helper 负责 DB 语句，Cache invalidation 与管理功能事件在 commit 后。
- 验收标准：
  1. Manager 登录/刷新/撤销/TOTP/Secret fail-closed 测试通过。
  2. 全部管理聚合 CRUD、冲突、软删除/关联保护、事务回滚与 cache immediate invalidation 通过。
  3. SQLite/PostgreSQL query block 均编译；公共响应、OpenAPI 和前端类型无 diff。
  4. Controller/Service 无 raw connection/Diesel。
- 构建与验证检查点：格式、Cargo check、Admin Service/Manager Controller/Auth/Secret 专项测试；任务结束不允许临时红态。
- 执行后文档更新要求：记录事务边界、Cache 顺序、测试结果和偏差。
- 实际完成：
  1. `ManagerAuthService` 与全部 `AdminServices` 现在显式持有同一 `Arc<DatabaseRuntime>`。Manager Credential、Auth Instance、TOTP Recovery 的 bootstrap、password rotation、TOTP install/disable/recovery、refresh rotation、revoke/logout-all、active session load 与 cleanup 均改为 typed async Repository；Manager session cleanup 的调度与 drain 仍按任务 8 边界保留。
  2. API Key/ACL 管理聚合的 create、metadata+ACL update、rotate、soft delete、reveal metadata/stored secret/detail/summary 全部改为 async operation；Secret 生成、加密、格式解析和解密均在事务外完成。Provider/Source/Provider Key、Model/Binding、Request Patch 与 Cost Catalog/Version/Component/template import 的管理 CRUD、关联校验、冲突协调与复制事务也全部迁移到 async Repository。
  3. PostgreSQL 与 SQLite 的事务各在单一 runtime operation/connection 上执行；事务 closure 只等待同连接 Diesel query。Request Patch owner/variant 校验、Provider/Model/Source 关联保护、Cost enabled-version reconcile 与 Manager Auth CAS/恢复码更新均保持原子性；SQLite 需要的多行插入使用事务内逐行 async insert，未引入后端行为分叉或新 retry。
  4. `AdminMutationRunner` 在数据库 operation 已提交后执行副作用，并固定先 cache invalidation、后管理 audit；Provider/Model/Patch/Cost/API Key 的 immediate invalidation、失败报告与既有管理事件顺序保持。Manager Auth 的内存 epoch/session snapshot 也只在确定数据库结果后更新，存储不可用继续 fail-closed。
  5. Manager 与管理 Controllers 只 await Service/typed async Repository；生产区静态检索未发现 `get_connection()`、`DbConnection`、raw Diesel 或同步 Repository 调用。测试中的故障注入仍暂用任务 9 待删除的旧同步 fixture，但被测生产路径使用显式 runtime；迁移中曾临时加入的 test task-local DatabaseRuntime 已删除，测试 helper 现在显式从各自 `TestDbContext` 构造并传入 runtime。
  6. Controller handler 的请求/响应类型与 utoipa annotations 未改变；`front`、Schema/Migration、OpenAPI artifact 和协议文档均无任务 5 diff。Cost version duplicate 的 async 事务已归属 `CostCatalogVersion`，未留下错误的 Catalog 公共入口。
- 验证记录：
  1. `rtk cargo fmt --check`：通过；`rtk cargo check -p cyder-api`：通过，0 errors、36 warnings；`rtk cargo check -p cyder-api --tests`：通过，0 errors、51 warnings。warnings 为同一原子迁移中待任务 6/9 消费或删除的旧同步方法、宏导入和既有 dead-code，不存在编译红态。
  2. 窄范围 `service::admin::request_patch::tests::`：1 passed、1407 filtered out，验证 Preview 不写入、CRUD 提交后 invalidation/audit 与规则值不进入 audit。
  3. `service::admin::`：60 passed、1 ignored、1347 filtered out，47.44s；覆盖 Manager Auth/TOTP/refresh/revoke/session、API Key、Provider/Model/Patch/Cost 聚合、冲突、软删除/关联保护、事务回滚与 cache invalidation。
  4. `controller::`：101 passed、1307 filtered out，42.38s；覆盖 Manager HTTP/Auth/Secret governance 与全部管理 Controller 合同。
  5. `service::secret_encryption::`：17 passed、1 ignored、1390 filtered out，8.19s；ignored 为既有环境依赖项，未计入通过数。
  6. `rtk git diff --check`：通过；静态检索确认 Manager Admin Service/Controller 的 raw connection/Diesel 命中仅在 `#[cfg(test)]` 故障 fixture，`current_test_database_runtime`/`ACTIVE_TEST_DATABASE_RUNTIME` 为零；前端文件无 diff，Controller diff 未改变 utoipa path/response/request-body 或公共 DTO 定义。
- 后续备注：
  1. C2 尚未闭合；任务 6 继续在同一未提交工作区迁移 Request Log、Metrics、Stat、Dashboard/Usage 与 Provider Runtime，之后执行 C2 门禁。
  2. 旧同步 Repository 方法与 TestDbContext 的同步选择边界仅为同一原子工作区尚未迁移/清理的测试消费者保留，任务 9 必须全部删除；本项未将其包装成生产兼容层。

#### 6. 迁移 Request Log、Metrics 与运营读模型

- 状态：已完成
- 进度：100%
- 检查点：C2
- 优先级：P0
- 难度：高
- 任务量：L
- 上下文风险：高
- 目标：迁移剩余持久化领域，确保 Request Log、Metrics、Dashboard/Usage/Provider Runtime/Stat 的 query/aggregate 结果不变。
- 任务边界裁定：
  - 纳入：Request Log insert/list/detail/search；Metrics marker/rollup/ingest/reconciliation；Stat/Dashboard/Usage；Provider Runtime；相关 Services/Controllers；显式 PostgreSQL/SQLite raw SQL 方言分支。
  - 不纳入：LogManager queue lifecycle（任务 8）、R4 RequestRecord/Evidence、R8 新指标/UI、Schema/Migration。
  - 依赖或顺序约束：任务 5 完成；本项结束时所有领域 Repository 运行期方法均已 async。
- 实施入口：`database/{request_log,metrics,stat,provider_runtime}.rs`、`service/metrics`、`controller/{request_log,metrics,stat,provider_runtime}.rs`、Dashboard/Usage query owners。
- 涉及范围：运营查询、日志记录、指标派生与校准；前端 API shape 不变。
- 证据引用：metrics.rs 已存在 `$1`/`?` explicit branches；RequestLog insert 后 Metrics persisted sink 顺序；Provider Runtime/Stat 同步 Repository。
- 预期结果：所有运营读写使用 async runtime，方言特例清楚，既有统计结果与分页/过滤不变。
- 阻塞条件：异步迁移要求改变现有 rollup/schema、Request Log 完整性或 R4.2 队列合同。
- 解除条件：停止执行并请求用户更新范围；不得提前实现后续阶段。
- 任务特有禁止项：不得把 sync wrapper 返回的结果改造成新的全量无界查询；不得把 telemetry 写入业务 Metrics 表。
- 任务特有执行要求：Manager query 使用 Foreground；Request Log insert、Metrics ingest/reconciliation 使用 Background；原有 batch/page limit 保持。
- 验收标准：
  1. Request Log 成功/失败/取消终态写入与查询测试通过。
  2. Metrics ingest marker、即时摄取、重复摄取、校准、Dashboard/Usage/Provider Runtime 结果测试通过。
  3. PostgreSQL/SQLite raw SQL 分支显式且双边编译。
  4. C2 结束全部 Repository 运行期公开方法为 async 并显式接收 DatabaseRuntime。
- 构建与验证检查点：格式、Cargo check、Request Log/Metrics/Stat/Provider Runtime 专项测试；C2 结束执行 C2 表中门禁。
- 执行后文档更新要求：记录方言特例清单、统计 parity 结果和 C2 状态。
- 实际完成：
  1. `RequestLog::{insert_runtime,get_by_id_runtime,list_runtime}` 显式接收 `DatabaseRuntime`；写入及 cost catalog version freeze 保持同一 Background transaction，Manager 列表/详情/精确 identity 搜索保持 Foreground、原分页/过滤/count 语义不变；Metrics 内部按 id 回读使用独立 Background 入口。
  2. `LogManager` 不再通过 global/test service locator 解析数据库，构造时持有 AppState 同一 `Arc<DatabaseRuntime>`；原容量 100、三次领域重试、成功写入后触发 Metrics sink 的顺序均未改变，queue lifecycle 与 shutdown drain 继续留给任务 8。
  3. Metrics marker、request/status/cost rollup 原子摄取、范围删除、pending/count/list/cursor 校准与全部 window/minute 查询均新增显式 async Repository；摄取、校准、repair 为 Background，Manager status/query 为 Foreground，原 reconciliation batch limit、dry-run、幂等 marker 与 repair bucket 扩展合同不变。
  4. `MetricsService` 持有显式 database runtime，摄取、即时 sink、校准 worker、repair、Dashboard/Usage query 与 Provider Runtime 聚合链全部 async；`AppState`、相关 Controllers 和测试构造统一注入同一 runtime，没有新增缓存 fallback、公共字段或业务指标表。
  5. Stat 的 system/dashboard overview、today、top model/top cost 和 Usage 聚合，以及 Provider Runtime request-log fallback、Provider/Model/Provider Key metadata 查询均迁到 Foreground async Repository；数据库侧 aggregate/order/limit、masked API key、fallback 触发和输出排序保持原行为。
  6. 方言特例保持显式：Metrics pending/list/cursor，Stat today cost、dashboard today/top model/top cost 与 Usage base/cost 分别保留 PostgreSQL `$n` 和 SQLite `?` 分支；Request Log、普通 rollup 查询与 Provider Runtime 使用双后端共同 Diesel query。生产 Service/Controller/worker 范围静态搜索未发现 `get_connection`、`DbConnection`、raw Diesel 或同步 RequestLog 调用。
  7. 将 Request Log 与 Metrics 核心数据库测试改为生产同路径 async runtime，并新增三种 Request Log 终态及 Metrics 即时摄取/重复摄取/校准闭环；新增 Stat runtime parity 覆盖 overview、today、top request/top cost 与 Usage。尚存的同步 Repository 定义及依赖它们的历史测试只作为未完成工作区旧边界存在，由任务 9 统一删除，未形成兼容层或提交边界。
- 验证记录：
  1. `rtk cargo fmt --check`：通过；`rtk cargo check -p cyder-api`：通过，0 errors、36 warnings；`rtk cargo check -p cyder-api --tests`：通过，0 errors、51 warnings。
  2. `rtk cargo test -p cyder-api database::request_log::tests::`：4 passed，覆盖 async insert/list/detail/search 与 SUCCESS/ERROR/CANCELLED 终态。
  3. `rtk cargo test -p cyder-api database::metrics::tests::`：4 passed；`rtk cargo test -p cyder-api service::metrics::`：9 passed，覆盖 marker/rollup、重复即时摄取、pending preview、实际校准与聚合闭环。
  4. `rtk cargo test -p cyder-api database::stat::tests::`：11 passed，async runtime parity 覆盖 system/dashboard overview、today、top model/top cost、Usage 及 SQLite raw aggregate；`rtk cargo test -p cyder-api provider_runtime::`：13 passed。
  5. Controller/Logging 专项通过：Request Log 3、Metrics 3、Stat 11、`proxy::logging::tests::` 4；Task 6 后完整 `controller::` 回归 101 passed。
  6. C2 Gateway/Manager 复跑通过：Catalog 9、Proxy Auth 9、API Key Governance 28、Admin Service 60 passed/1 ignored、Controllers 101；既有 ignored 未冒充通过。
  7. PostgreSQL/SQLite 分支由生产与 tests Cargo check 双边编译；`git diff --check` 通过；Task 6 生产消费范围的同步连接/raw Diesel/同步 RequestLog 静态搜索为零匹配。
- 后续备注：
  1. 首轮 Stat runtime parity 只平移 `request_received_at`，正确触发既有 `chk_request_log_timing_contract`；测试种子已改为整体平移 request/upstream/completed/created/updated 时间线，随后 11 条 Stat 专项全部通过，未放宽约束或改变生产 SQL。
  2. C2 已闭合但仍属于同一个未提交工作区；任务 7—12、C3—C4 与完成情况检查获用户确认前不得提交。

#### 7. 完成 AppState、启动生命周期与 Readiness 装配

- 状态：已完成
- 进度：100%
- 检查点：C3
- 优先级：P0
- 难度：高
- 任务量：M
- 上下文风险：高
- 目标：把 DatabaseRuntime 变成唯一生产运行期数据库所有者，并完成首次连接、运行中降级/恢复和无排队 Readiness。
- 任务边界裁定：
  - 纳入：main/AppState/Service constructors 显式注入；运行期 pool 在 startup connection 销毁后建立；至少一个连接验证成功后才 bind；`/ready` stateful no-wait probe；warm-cache no global gate。
  - 不纳入：worker stop/drain（任务 8）、旧 pool 删除（任务 9）、公共 readiness schema。
  - 依赖或顺序约束：C2 完成；所有服务构造必须接入同一 Arc<DatabaseRuntime>。
- 实施入口：`server/src/main.rs`、`server/src/service/app_state.rs`、Service/Admin/Catalog/Metrics constructors、`server/src/controller/system.rs`、Router state wiring。
- 涉及范围：应用启动、依赖注入、Readiness、运行期恢复。
- 证据引用：当前 AppState 无 DB；ready_handler 无 State 并同步 get；main 在 create_app_state 后 bind。
- 预期结果：生产只有一个 DatabaseRuntime；初始 DB 失败退出；运行中错误 503、恢复后 200；响应 JSON 不变。
- 阻塞条件：任何服务仍只能通过 global/test-local DB 构造。
- 解除条件：迁移对应构造器和测试；不得在 AppState 外再建 runtime。
- 任务特有禁止项：不得在 `/ready` 排队；不得因 in-memory degraded flag 全局拒绝 Proxy；不得把 detailed telemetry 加入响应。
- 任务特有执行要求：Readiness 饱和、断连、恢复和 Redis 组合状态均有测试。
- 验收标准：
  1. startup connection → runtime → AppState → bind 顺序确定。
  2. 初始连接失败无 listener；运行中断连/恢复不要求重启。
  3. `/health` 与 `/ready` JSON/HTTP 合同保持，DB probe 不进入 waiter queue。
  4. 全部 Service 使用同一个 runtime 实例。
- 构建与验证检查点：格式、Cargo check、AppState/startup/system router/readiness/cache degrade 专项测试；任务结束不允许临时红态。
- 执行后文档更新要求：记录启动/恢复/Readiness 证据和偏差。
- 实际完成：
  1. `main` 固定执行 `StartupDatabaseConnection` 建立/迁移/Secret 准备 → 显式 drop startup connection → `DatabaseRuntime::connect` → 首次 `readiness_probe` → `create_app_state(database)` → listener bind/serve；runtime 初始化或首次连接验证失败会在 listener 创建前终止，错误不包含 DB URL。
  2. `AppState` 不再自行读取配置并构造生产 runtime；生产 `AppState::new` 与 `create_app_state` 必须接收显式 `Arc<DatabaseRuntime>`，测试构造也先取得 runtime 再走同一装配函数。生产静态搜索只剩 `main` 一个 `DatabaseRuntime::connect`，没有 AppState 外的第二个运行时所有者。
  3. Catalog、Admin/Manager Auth、Metrics、API Key Governance、Provider Key Selector 与 LogManager 均从组合根获得同一个 runtime；AppState 测试以 pointer identity 覆盖 Catalog、Metrics、Governance 与 Manager Auth，构造链覆盖其余间接 owner。
  4. `/ready` 改为 `State<Arc<AppState>>` 并调用 `app_state.database.readiness_probe()`；probe 先以 `try_acquire` 检查 admission capacity，饱和时立即返回 503 且 `waiting` 保持 0，连接 checkout 继续由 bb8 的 `test_on_check_out` 验证；释放 capacity 后同一 AppState 恢复 200。
  5. `/health` 与 `/ready` 的 HTTP/JSON schema 保持不变；Redis `None/ok/error` 与 database `ok/error` 组合映射已固定。数据库 readiness 降级不写入 Proxy 全局准入状态，饱和期间 liveness 仍为 200，已预热 Catalog 读取仍成功。
  6. Runtime 测试新增 SQLite 初始连接失败证明，并保留同一 runtime 的 execution-error → success recovery telemetry 覆盖；真实 PostgreSQL 断连/池重建仍按计划由任务 11 的专用 PG17 边界门禁验证，没有在任务 7 冒充双后端认证。
- 验证记录：
  1. `rtk cargo fmt --check`：通过；`rtk cargo check -p cyder-api`：通过，0 errors、36 warnings；`rtk cargo check -p cyder-api --tests`：通过，0 errors、51 warnings。
  2. `rtk cargo test -p cyder-api database::runtime::tests::`：13 passed，包含初始连接失败、无 waiter readiness、execution error/recovery、隔离与 lifecycle 基础合同。
  3. `rtk cargo test -p cyder-api service::app_state::tests::`：5 passed，包含服务同 runtime pointer identity、memory runtime backend、静态配置与 session cleanup。
  4. `rtk cargo test -p cyder-api controller::system::tests::`：3 passed，覆盖公共 JSON/HTTP、DB/Redis 组合、饱和零排队 503 → 释放后 200、liveness 与 warm Catalog 不受全局 gate。
  5. `rtk cargo test -p cyder-api main_prepares_state_and_rotates_secrets_before_binding_or_serving`：1 passed，静态锁定 startup/drop/runtime/probe/AppState/bind/serve 顺序。
  6. `rtk cargo test -p cyder-api service::catalog::`：9 passed，确认缓存读写/回源行为未因 readiness 装配回归；`git diff --check` 通过。
- 后续备注：
  1. SQLite 进程内没有等价于远端网络断连的自然故障面；本任务以真实初始连接失败、运行时饱和 503/恢复 200、runtime execution-error/recovery 覆盖装配合同，真实 PostgreSQL 17 断连与恢复证据严格留给任务 11。
  2. Worker stop/join、LogManager close-and-drain、main 固定 sleep 删除和 `DatabaseRuntime::close_and_drain` 调用属于任务 8，本任务未越界提前改变关闭顺序。

#### 8. 收敛持久化 worker、LogManager 与关闭排空

- 状态：已完成
- 进度：100%
- 检查点：C3
- 优先级：P0
- 难度：高
- 任务量：L
- 上下文风险：高
- 目标：使所有持久化后台任务有明确 owner、stop、Join 与 drain 顺序，并删除固定 sleep。
- 任务边界裁定：
  - 纳入：LogManager 注入 runtime、Background workload、保存 JoinHandle、close-and-drain；Metrics reconciliation/session cleanup cancellation + Join；AppState persistence worker lifecycle；main graceful shutdown 顺序；DatabaseRuntime close/drain。
  - 不纳入：LogManager 容量/重试/drop/batch/磁盘缓冲、R9 通用 supervisor、进程内 hard kill timeout。
  - 依赖或顺序约束：任务 7 完成；HTTP graceful drain 后再停止 periodic producer 与日志 producer，最后关闭 runtime。
- 实施入口：`server/src/proxy/logging.rs`、`server/src/service/{infra,app_state}.rs`、`server/src/service/metrics`、Manager session cleanup、`server/src/main.rs`。
- 涉及范围：Request Log/instant Metrics、periodic Metrics、session cleanup、shutdown telemetry。
- 证据引用：LogManager mpsc100/retry3/Flush；AppInfra spawn_background_task；main flush + sleep1；AppState 无限 loop workers。
- 预期结果：worker 不再 untracked；停止后不领取新 batch；已开始 operation 与日志队列全部完成后 pool 才关闭。
- 阻塞条件：某 worker 持有不可停止的外部任务或 drain 需要改变 R4.2 产品合同。
- 解除条件：只重构 owner/lifecycle；产品合同变化必须请求用户重新裁定。
- 任务特有禁止项：不得 abort 已开始 DB operation；不得添加 shutdown timeout；不得把 LogManager send/backpressure 改成 drop。
- 任务特有执行要求：
  1. shutdown 顺序为 HTTP handlers → periodic workers stop/join → LogManager close/drain（含 Metrics sink）→ DatabaseRuntime reject/wait active zero/close。
  2. 超过 operation deadline 时记录 drain overdue，但继续等待确定结果。
- 验收标准：
  1. LogManager capacity100、retry3、顺序 sink 与公共行为保持。
  2. worker stop 不启动下一 tick，已开始 tick 完成；所有 JoinHandle 被等待。
  3. DB unavailable、queue full、caller cancellation、shutdown during write 测试无提前释放或丢失已接受 command。
  4. `sleep(1s)` 删除，shutdown complete 只在 drain 完成后记录。
- 构建与验证检查点：格式、Cargo check、Logging/Metrics/AppState worker/shutdown 专项测试；任务结束不允许临时红态。
- 执行后文档更新要求：记录 worker owner、关闭顺序、drain 结果和偏差。
- 实际完成：
  1. AppState 新增唯一 `PersistenceWorkers` owner，以 `CancellationToken`、`started` guard 和保存的 `JoinHandle` 管理 Metrics reconciliation 与 Manager session cleanup；启动幂等，stop 使用 biased cancellation，不领取下一 tick，已进入的 tick 不被 abort 并在完成后 join。
  2. `LogManager` 保留容量 100、`send().await` backpressure、三次领域重试和成功 insert 后顺序 Metrics sink；sender 与 worker handle 由 manager 持有，`close_and_drain` 先封闭新生产者，再让 receiver 排空全部已接受 command/sink，最后等待 worker 退出。关闭后新 command 明确进入既有 enqueue failure 记账，不会静默丢弃。
  3. AppInfra 暴露窄 `close_and_drain_proxy_logs`；AppState 的 `shutdown_persistence` 固定执行 periodic workers stop/join → Request Log/Metrics sink close/drain → `DatabaseRuntime::close_and_drain`，并为每一阶段记录开始/完成里程碑。
  4. `DatabaseRuntime::close_and_drain` 先切换 closing、拒绝新 admission，再等待 tracked active operation 清零后 closed；排空超过 operation deadline 只发 `database.shutdown_drain_overdue` 后继续等待。Workers 与 LogManager 同样只发低基数 overdue 事件，不设置 hard timeout、不 abort 已开始 Future。
  5. `main` 只在 Axum graceful shutdown 完成后调用 `shutdown_persistence`，随后才发 shutdown complete；删除固定 `sleep(1s)`。HTTP handlers 与 transport producers 已结束后才停止 periodic producers，符合已确认顺序。
  6. 生命周期测试覆盖：worker 已开始 tick 时 stop 不提前返回且 cancellation 后不领取下一 tick；完整 AppState shutdown 后 handles 为零、LogManager 已 join、runtime readiness 为 ShuttingDown；LogManager 在 DB 写等待期间 close 不提前返回，释放后完成既有重试/失败记账，DB 已关闭时已接受 command 仍会处理完毕。
- 验证记录：
  1. `rtk cargo fmt --check`：通过；`rtk cargo check -p cyder-api`：通过，0 errors、36 warnings；`rtk cargo check -p cyder-api --tests`：通过，0 errors、51 warnings。
  2. `rtk cargo test -p cyder-api proxy::logging::tests::`：6 passed，覆盖 capacity/retry 固定合同、shutdown-during-write、关闭后拒绝与 DB unavailable drain；两条失败路径均证明 2 retries/1 terminal DB failure/1 processed/pending 归零。
  3. `rtk cargo test -p cyder-api service::app_state::tests::`：7 passed，覆盖已开始 tick 完成、停止后不领取下一 tick、所有 JoinHandle 等待，以及 workers/logs/database 完整关闭顺序。
  4. `rtk cargo test -p cyder-api database::runtime::tests::`：13 passed，包含 queue full、queued caller cancellation、started caller cancellation 后仍 tracked、shutdown 等待、operation deadline 不丢 Future 与 recovery。
  5. `rtk cargo test -p cyder-api service::metrics::`：9 passed；`rtk cargo test -p cyder-api request_patch_query_value_reaches_upstream_but_not_request_log`：1 passed，确认 periodic logic 与成功 Request Log/flush 路径未回归。
  6. `rtk cargo test -p cyder-api main_drains_persistence_after_http_shutdown_without_fixed_sleep`：1 passed，锁定 HTTP graceful → persistence drain → shutdown complete 顺序并证明固定 1 秒 sleep 已删除；`git diff --check` 通过。
- 后续备注：
  1. Task 8 只改变 owner/stop/join/drain，不改变 LogManager queue/drop/batch/retry 产品合同；磁盘缓冲等继续属于 R4.2。
  2. 仍存在的同步 Repository 与 TestDbContext selector 是任务 9 明确删除对象；本任务未把它们包装成生命周期 fallback。

#### 9. 删除同步运行时边界并建立静态门禁

- 状态：已完成
- 进度：100%
- 检查点：C3
- 优先级：P0
- 难度：高
- 任务量：L
- 上下文风险：高
- 目标：删除所有过渡残留，并用自动化 lint 防止同步数据库访问重新进入监听后代码或测试。
- 任务边界裁定：
  - 纳入：删除 DbPool/DbConnection/global OnceLock/get_connection、Diesel r2d2 feature/direct crate、task-local/thread-local TestDbContext、LogManagerRuntime DB 选择、Controller/test raw connection；新增 persistence_boundary_lint。
  - 不纳入：删除同步 Diesel postgres/sqlite features；startup/migration/test fixture 的显式同步单连接继续保留。
  - 依赖或顺序约束：任务 8 完成；删除前所有消费者必须已迁移。
- 实施入口：`server/src/database/mod.rs`、所有 `get_connection` 命中、`server/Cargo.toml`/lock、测试模块、新 `server/src/bin/persistence_boundary_lint.rs`。
- 涉及范围：全后端代码与测试、依赖树、静态边界。
- 证据引用：任务 1 同步消费者清单；当前 Cargo r2d2；TestDbContext globals。
- 预期结果：监听后不存在同步 DB escape hatch；测试使用显式 runtime；依赖树无 DB r2d2。
- 阻塞条件：仍有未迁移消费者或测试只能依赖隐式当前 DB。
- 解除条件：返回任务 4—8 完成对应迁移；不得为通过 lint 增加 allowlist。
- 任务特有禁止项：lint allowlist 只包含 `database/startup.rs`、`database/runtime.rs` 中 SQLite wrapper 类型建立、`database/test_support.rs` fixture bootstrap 与 migration smoke；不得允许 Service/Controller/Proxy。
- 任务特有执行要求：lint 至少拒绝无参数 get_connection、r2d2、database 目录外 RunQueryDsl/sql_query/Connection establish、task-local/thread-local DB selector 和裸 spawn_blocking SQLite query。
- 验收标准：
  1. `rg` 与 lint 均证明 get_connection/r2d2/global DB/test selector 为零。
  2. 原始 query execution 只在 database 模块；startup sync 只在窄 allowlist。
  3. Cargo tree 包含 diesel-async/bb8 且数据库路径不含 r2d2。
  4. 全量默认测试通过；C3 完整门禁通过。
- 构建与验证检查点：格式、Cargo check、全量 cargo test、Log Lint、Persistence Boundary Lint、Cargo tree；C3 结束不允许红态。
- 执行后文档更新要求：记录删除清单、lint 规则、依赖树与 C3 状态。
- 实际完成：
  1. 删除 `DbPool`、`DbConnection`、global `OnceLock`/无参数 `get_connection()`、同步 `db_execute!`、数据库 r2d2 feature/direct dependency，以及 task/thread-local TestDbContext 与 LogManager runtime 数据库选择；运行期只保留显式注入的 `DatabaseRuntime`。
  2. 全部后端测试改用显式 `TestDatabase` + 生产同路径 `DatabaseRuntime`；删除测试 identity `run_async`/`spawn` 包装、Controller/Service/Proxy raw connection 与重复同步 Repository，补足必要的 async 测试 helper。
  3. 将 startup Secret 的 Diesel transaction/SQL 收敛到 `database/startup.rs` 的窄 `StartupSecretTransaction`；Service 只保留加解密和轮换领域编排。将 dedicated PostgreSQL fixture reset/query 收敛到 `database/test_support.rs`。
  4. 新增 `persistence_boundary_lint`，拒绝 get_connection/DbPool/DbConnection/r2d2、隐式测试数据库 selector、database 目录外 raw Diesel execution/connection establish，以及裸 `spawn_blocking` 数据库工作；lint 不为 Service/Controller/Proxy 建 allowlist。
  5. Cargo 依赖树保留 `diesel-async 0.9.2` 与 `bb8 0.9.1`，移除 r2d2；C3 结束时全量默认测试与静态门禁均为绿色。
- 验证记录：
  1. `rtk cargo fmt --check`、`rtk cargo check -p cyder-api`、`rtk cargo check -p cyder-api --tests`：通过，0 errors；当前保留未阻塞 warnings。
  2. `rtk cargo test -p cyder-api`：1403 passed，14 ignored；Secret 专项 22 passed/1 ignored，Provider 专项 7 passed。
  3. `rtk cargo run -p cyder-api --bin log_lint`：通过；`rtk cargo run -p cyder-api --bin persistence_boundary_lint`：通过；lint 单元测试 3 passed。
  4. `rtk cargo tree -p cyder-api`：确认 diesel-async 0.9.2、bb8 0.9.1 存在；`rtk cargo tree -p cyder-api -i r2d2` 无匹配 package。
  5. 两组 `rtk rg` 审计分别对同步/global/test selector 与 database 外 raw Diesel 零命中；`git diff --check` 通过。
- 后续备注：
  1. Startup migration/Secret 与 test fixture bootstrap 继续使用任务裁定允许的显式同步单连接窄边界；它们不构成监听后 escape hatch。
  2. async 双后端宏展开仍产生 unused import warnings，不影响 C3 合同；不为消除 warning 恢复同步 API 或扩大 lint allowlist。

#### 10. 完成 SQLite 并发、饱和、超期、取消与恢复矩阵

- 状态：已完成
- 进度：100%
- 检查点：C4
- 优先级：P0
- 难度：高
- 任务量：L
- 上下文风险：高
- 目标：以生产同路径 SQLite runtime 证明 Tokio 非阻塞、admission、超期、事务、缓存降级和 drain 合同。
- 任务边界裁定：
  - 纳入：确定性锁/屏障/测试 hook；WAL/FK/busy timeout；pool/queue 边界；Foreground/Background；readiness no-wait；caller drop；tracked completion；transaction；disconnect/file failure；recovery；Proxy/Manager/Log/Metrics 集成。
  - 不纳入：性能 benchmark/SLO、长时间 soak、unsafe interrupt、改变 synchronous。
  - 依赖或顺序约束：C3 完成；测试必须使用 TestDatabase + DatabaseRuntime，不得创建同步 Repository shortcut。
- 实施入口：`database/test_support.rs`、runtime/admission tests、Proxy direct execution tests、Manager/Log/Metrics/AppState tests。
- 涉及范围：SQLite 默认全量测试与并发故障注入。
- 证据引用：任务 3/4/5/6/8 目标合同；当前 test-only WAL/busy 行为。
- 预期结果：所有关键 SQLite 行为可重复、无长 sleep、无 Tokio scheduler 阻塞和无资源提前归还。
- 阻塞条件：测试只能通过 timing race 或全局数据库状态复现。
- 解除条件：增加 database::test_support 的确定性受控接缝；不得降低断言或延长 sleep 掩盖竞态。
- 任务特有禁止项：不得暴露 test hook 到非 test build；不得以串行全套测试掩盖隔离问题。
- 任务特有执行要求：至少以 paused/barrier/ticker 证明同步 SQLite query 运行时 Tokio 任务继续调度；测试写超期后最终 commit/rollback 可确定。
- 验收标准：
  1. active/waiting/max_waiters/queue timeout/background reservation/readiness saturation 精确断言。
  2. caller cancellation 后 operation 仍持有资源到结果，shutdown 等待 active zero。
  3. WAL/FK/busy/foreign_key_check 与 connection recovery 断言通过。
  4. Proxy warm cache/fail-closed、Manager transaction/cache、Log retry/drain、Metrics reconciliation 测试通过。
- 构建与验证检查点：SQLite runtime 专项测试、相关 Proxy/Manager/worker 测试、`rtk cargo test -p cyder-api`、Persistence Boundary Lint。
- 执行后文档更新要求：记录矩阵用例、确定性接缝、全量结果和偏差。
- 实际完成：
  1. 保留并复核既有 admission 矩阵：active/waiting/waiting_foreground 精确 snapshot、`max_waiters=0/1` queue full、暂停时钟精确 queue timeout、Foreground 优先、Background `pool_size - 1` reservation、Readiness no-wait 饱和/恢复。
  2. 新增仅测试使用的 SQLite blocking SQL function，以 Condvar 作为确定释放屏障；函数已在真实 SQLite query 内阻塞时，独立 Tokio ticker 完成 32 次调度且 query 仍未结束，证明 `SyncConnectionWrapper` 没有阻塞 Tokio scheduler。
  3. 使用暂停时钟跨过 1 秒 operation deadline，并在 deadline telemetry/active 已确定后释放 SQL function；一条事务最终 commit、一条最终 rollback，调用方都得到既定 `OperationDeadlineExceeded`，最终表中只有 commit 行，证明已开始写不会被丢弃或提前归还。
  4. 使用独立 startup fixture 持有 SQLite write lock，生产 runtime 写在 1 秒 busy timeout 后失败且未误判为 operation deadline；释放锁后同 runtime 成功写入并记录 recovery。文件父路径故障拒绝连接，修复临时路径后新 runtime readiness 成功。
  5. 复核 startup WAL/foreign keys/busy timeout/保留 synchronous 与非空 `foreign_key_check` 拒绝；复跑 Proxy 暖缓存/冷查 fail-closed 与轮换缓存、Manager 多写事务回滚、Log retry/drain、Metrics reconciliation、Readiness saturation 跨层链路。
  6. Persistence Boundary Lint 的 bare blocking 检查收窄为显式 `tokio::task::spawn_blocking`/`tokio::spawn_blocking`，并增加一条允许 diesel-async `SyncConnectionWrapper::spawn_blocking` 自身受管执行的回归测试；未增加业务层 allowlist。
- 验证记录：
  1. `rtk cargo test -p cyder-api database::runtime::tests:: -- --nocapture`：16 passed；覆盖真实 SQLite query scheduler、admission、deadline commit/rollback、cancel/drain、busy/file recovery 与 PRAGMA。
  2. `rtk cargo test -p cyder-api sqlite_startup_`：2 passed；WAL/FK/busy/synchronous/foreign_key_check 正常与违规拒绝均通过。`rtk cargo test -p cyder-api --bin persistence_boundary_lint`：4 passed；运行 lint 通过。
  3. Proxy cache/fail-closed 2 条、Manager API Key transaction rollback 1 条、Logging 6 条、Metrics reconciliation 3 条、System Readiness 3 条专项全部通过。
  4. `rtk cargo test -p cyder-api`：1407 passed，14 ignored，7 suites，180.10s。
  5. `rtk cargo fmt --check`、`rtk cargo check -p cyder-api`、`rtk cargo run -p cyder-api --bin persistence_boundary_lint`、`git diff --check`：组合门禁成功退出。
- 后续备注：
  1. SQLite busy timeout 的一次真实锁等待使用配置最小值 1 秒；其余并发/超期断言采用 Notify、Condvar 或 Tokio paused time，不以延长 sleep 掩盖竞态。
  2. 故障注入只位于 `#[cfg(test)]` database 模块或 TestDatabase startup fixture；所有被测 Repository/Proxy/Manager/Log/Metrics 操作仍走生产 `DatabaseRuntime`，没有同步 Repository shortcut。

#### 11. 完成最小真实 PostgreSQL 17 异步边界门禁

- 状态：已完成
- 进度：100%
- 检查点：C4
- 优先级：P0
- 难度：高
- 任务量：L
- 上下文风险：高
- 目标：使用专用 PostgreSQL 17 证明真 async runtime 与代表性全链路，不扩大为全领域终态认证。
- 任务边界裁定：
  - 纳入：单一 ignored suite `r3_22_postgres_async_boundary`，覆盖 clean+upgrade/startup Secret/runtime pool、代表性 read/write/rollback、statement/idle transaction timeout、pool saturation、断连恢复、一条 Proxy 链、一条 Manager transaction/cache 链、Request Log/instant Metrics、worker drain。
  - 不纳入：全部历史 ignored tests、每个 Manager CRUD parity、完整四协议 Matrix、soak/SLO、R9.13/R9.14 终态认证。
  - 依赖或顺序约束：任务 10 完成；环境变量必须指向可破坏的专用 PG17 数据库；串行执行。
- 实施入口：database test support 与新 PG boundary test module、现有 migration smoke helper、代表性 Proxy/Manager/Log/Metrics fixture。
- 涉及范围：真实 PG17 的最小 async 边界证明。
- 证据引用：用户确认的缩减 PG 门禁；现有 ignored dedicated PG 测试模式。
- 预期结果：R3.22 的 PG 驱动、session timeout、transaction、恢复和跨层 wiring 得到真实运行证据；其他 PG 测试至少编译。
- 阻塞条件：缺少专用 PG17 URL、数据库身份无法确认、网络/权限禁止连接或目标含不可删除数据。
- 解除条件：启动专用临时 PostgreSQL 17 并提供 URL；测试前查询数据库身份，测试后清理。
- 任务特有禁止项：不得指向共享/生产 DB；不得为完成本项运行和修复全部历史 ignored suite；不得以 SQLite 结果替代。
- 任务特有执行要求：suite 内部重建专用 schema，所有测试串行；断连/恢复使用可控连接或服务接缝，不污染其他测试。
- 验收标准：
  1. startup sync → async runtime 和代表性 query/transaction 全部通过。
  2. statement timeout、idle transaction timeout、pool saturation、disconnect/recovery 有确定断言。
  3. Proxy cache miss/governance/log/metrics 与 Manager atomic mutation/cache invalidation 各一条真实 PG 链通过。
  4. worker stop/drain/close 通过；未运行的全领域 PG 终态范围明确保留。
- 构建与验证检查点：第 5 节 PG17 命令；随后 Cargo check 确认全部 ignored tests 编译。
- 执行后文档更新要求：记录 PG 版本/专用数据库身份、安全清理、用例数、结果和明确未跑范围。
- 实际完成：
  1. 新增唯一 ignored suite `r3_22_postgres_async_boundary`；环境变量固定为 `CYDER_R322_POSTGRES_SMOKE_URL`，测试在任何 destructive reset 前同步查询并严格断言数据库名 `cyder_r322_async_boundary`、用户 `cyder_r322` 与 major version 17。
  2. TestDatabase 增加显式 `new_postgres_with_config` 与窄 PG17 identity/reset helper；suite 在 clean public schema 上运行全量 migration，随后再次建立 StartupDatabaseConnection 证明 upgrade no-op 与 Secret preparation 成功，再创建真 `AsyncPgConnection`/bb8 runtime。
  3. 边界级 runtime 覆盖代表性 create/insert/read/transaction rollback；验证连接 custom setup 将 `statement_timeout` 和 `idle_in_transaction_session_timeout` 均设为 1000ms，100ms server statement timeout 先于 runtime deadline 确定失败。
  4. 在 Diesel async transaction 内触发 100ms idle-in-transaction server disconnect，随后新 operation 成功且 session timeout 恢复为 custom setup 值，证明 broken connection 被池替换；双连接同时 active 时 Readiness no-wait 失败且 `max_waiters=0` 请求 QueueFull，释放后 active 归零。
  5. 真实 PG 跨层链覆盖：Manager 创建/轮换 API Key，Proxy cold cache auth 与 concurrency governance fail-closed，rotation 后旧 hash 失效、新 hash 生效；Provider bootstrap 原子事务建立有效 FK 上下文；LogManager 持久化 Request Log 后 Metrics sink 立即写 marker、pending reconciliation 为零。
  6. 启动 periodic workers 后执行完整 `shutdown_persistence`，确认 workers stop/join、LogManager drain、DatabaseRuntime close 后 active/waiting 为零。测试只运行这一个缩减 suite，没有扩展到历史 ignored tests 或全领域 PG parity。
  7. 本地使用已有 `postgres:17` 镜像创建 `--rm` 一次性容器；实测 `server_version_num=170010`（PostgreSQL 17.0.10）。门禁后 stop 并确认容器不存在，临时测试数据随容器删除。
- 验证记录：
  1. `CYDER_R322_POSTGRES_SMOKE_URL=... rtk cargo test -p cyder-api r3_22_postgres_async_boundary -- --ignored --test-threads=1 --nocapture`：1 passed，1421 filtered out，6 suites，0.78s；仅连接本地专用 PG17。
  2. `rtk cargo check -p cyder-api --tests`：通过，0 errors、50 warnings，证明其他 ignored PostgreSQL tests 继续编译但未冒充运行。
  3. `rtk cargo fmt --check`、tests 编译、`rtk cargo run -p cyder-api --bin persistence_boundary_lint`、`git diff --check`：组合门禁成功退出。
  4. `rtk docker exec ... SELECT current_setting('server_version_num'), current_database(), current_user`：`170010 / cyder_r322_async_boundary / cyder_r322`；测试结束 `rtk docker ps -a --filter name=cyder-r322-pg17` 零输出。
- 后续备注：
  1. 完整四协议 PG Matrix、全部 Manager CRUD parity、历史 ignored migration/Secret suites、soak/SLO 与双数据库终态认证未运行，继续归 R9.13/R9.14；本任务只证明 R3.22 最小异步持久化边界。
  2. 首次 sandbox 内运行因本机端口网络隔离出现空 `BadConnection`；按执行环境规则在授权的 sandbox 外运行后真实测试通过，这不是产品代码连接失败。

#### 12. 执行全量门禁、范围审计与唯一原子提交

- 状态：已完成
- 进度：100%
- 检查点：C4
- 优先级：P0
- 难度：高
- 任务量：M
- 上下文风险：高
- 目标：证明单一变更集满足 R3.22 全部合同，清除过渡残留，经用户确认完成情况检查后形成唯一原子提交。
- 任务边界裁定：
  - 纳入：第 5 节全部门禁、git diff/range/audit、依赖/静态边界、任务状态与验证记录、完成情况检查、唯一 commit。
  - 不纳入：实施新能力、修复范围外问题、启动 R4.1。
  - 依赖或顺序约束：任务 1—11/C1—C3 完成，PG17 门禁通过；完成情况检查必须先展示给用户并获得确认。
- 实施入口：全工作区 diff、本文、Roadmap R3.22、Cargo/测试/lint/quality gate、git。
- 涉及范围：最终原子交付与执行文档维护。
- 证据引用：本文全部决定、任务实际完成和验证结果。
- 预期结果：工作区不存在临时兼容层、未裁定 diff 或未运行必跑门禁；R3.22 由一个提交完整表达。
- 阻塞条件：任何必跑门禁失败、存在 schema/front/OpenAPI/protocol diff、同步边界残留、完成情况检查未获用户确认或发现计划过期触发。
- 解除条件：回到负责该失败的任务修正并重跑；范围变化必须先获用户裁定。
- 任务特有禁止项：不得跳过/伪造门禁；不得把失败标记 ignored；不得在完成检查确认前写 audit record、追加 follow-up 或提交。
- 任务特有执行要求：最终提交信息使用 `refactor(database): establish async persistence boundary`；提交前确认本项实现只有一个待提交变更集且无此前中间提交。
- 验收标准：
  1. 第 5 节全部必跑命令真实通过并记录。
  2. `get_connection`、r2d2、隐式 TestDbContext、数据库模块外 raw query、untracked persistence worker 为零。
  3. Schema/Migration、front、OpenAPI、Protocol Matrix 无实现 diff；公共行为回归通过。
  4. 用户确认完成情况检查，无未裁定严重问题；完成检查记录已写入。
  5. 唯一原子提交创建，本文状态、进度、检查点、任务记录与提交事实一致。
- 构建与验证检查点：第 5 节全部必跑门禁；`rtk git diff --check`；提交后 `rtk git status --short` 确认无实现残留。
- 执行后文档更新要求：记录最终命令结果、完成检查用户决定、commit hash、偏差与最终状态。
- 实际完成：
  1. 第 5 节全部最终技术门禁已真实运行并通过；SQLite 默认全量、Release、两个 lint、Transform Quick 与全新专用 PG17 boundary 均为绿色。
  2. 依赖树确认 diesel-async/bb8 且无 r2d2；两组静态 `rg` 分别确认 legacy/global/test selector 与 database 外 raw query 零命中；Persistence Boundary Lint 同时通过。
  3. `server/migrations`、`front`、OpenAPI 与 Protocol Matrix 范围 diff 为零；`git diff --check` 通过。HEAD 仍为任务开始基线 `ac6919e`，证明没有中间实现提交。
  4. 完成情况检查已向用户展示；用户回复“确定”，确认无未裁定严重问题并授权写入记录与唯一原子提交。检查记录已写入 7.2，未追加范围任务。
  5. 任务文档随全部 scoped 实现 force-add 到唯一提交，提交信息固定为 `refactor(database): establish async persistence boundary`；精确 commit hash 由提交后 `git rev-parse --short HEAD` 与最终交付记录取证，不在 commit 自身内容中制造不可实现的哈希自引用。
- 验证记录：
  1. 格式通过；生产编译 0 errors/32 warnings；tests 编译 0 errors/50 warnings。
  2. 默认全量：1407 passed、15 ignored、7 suites、179.80s；Release：0 errors/32 warnings、108 crates。
  3. Log Lint、Persistence Boundary Lint、Transform Quick（replay passed、contract 11/11、accounting closed、benchmark 7/7）均通过。
  4. 最终全新 `postgres:17` 专用容器：身份 `170010 / cyder_r322_async_boundary / cyder_r322`；PG boundary 1 passed/1421 filtered out/0.80s；容器随后清理。
  5. Cargo tree：diesel-async 0.9.2、bb8 0.9.1、r2d2 absent；legacy boundary/raw query 两组 `rg` 零命中；excluded scope diff、diff check 均通过。
- 后续备注：
  1. 生产编译 32 个 warnings、tests 编译 50 个 warnings 主要来自双后端宏展开的 unused imports 与既有 dead code；它们不影响 R3.22 合同和门禁，作为后续常规维护优化候选，不追加到本原子任务。
  2. 完整四协议 PostgreSQL Matrix、全领域 PG parity、soak/SLO 仍归 R9.13/R9.14；R3.22 未误报覆盖这些明确排除范围。
