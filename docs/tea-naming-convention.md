# TEA 模块命名规范

**版本**: 2.0
**状态**: 草案
**创建日期**: 2026-08-01

---

## 一、目录结构规范

### 1.1 app_shell 目录结构

`app_shell` 是顶层协调者，包含全局消息路由、副作用执行和壳层状态管理。

#### 小规模结构（单文件）

```
app_shell/
├── mod.rs              # 模块声明
├── msg.rs              # AppMsg 全局消息枚举
├── state.rs            # ShellState 壳层状态
├── update.rs           # app_shell update() + 消息分派
├── view.rs             # 全局渲染（如 overlay）
├── intent_router.rs    # IntentRouter（单文件，所有 Intent 路由）
├── effect_runner.rs    # EffectRunner（单文件，所有 Effect 执行）
└── focus.rs            # 焦点/区域导航逻辑
```

#### 大规模结构（按功能拆分）

当 `intent_router.rs` 超过 200 行或处理超过 5 个 Feature 变体时，按功能拆分：

```
app_shell/
├── mod.rs
├── msg.rs
├── state.rs
├── update.rs
├── view.rs
├── focus.rs
├── intent_router/              # 按功能拆分的意图路由
│   ├── mod.rs                  # pub fn route(intent, shell)
│   ├── header.rs               # Header 相关 Intent 路由
│   ├── discover.rs             # Discover 相关 Intent 路由
│   ├── explorer.rs             # Explorer 相关 Intent 路由
│   ├── instance_workspace.rs   # Instance Workspace 相关 Intent 路由
│   └── sql_workspace.rs        # SQL Workspace 相关 Intent 路由
├── effect_runner/              # 按功能拆分的副作用执行
│   ├── mod.rs                  # pub fn run(effects, tx, services)
│   ├── header.rs               # Header 相关 Effect 执行
│   ├── discover.rs             # Discover 相关 Effect 执行
│   ├── explorer.rs             # Explorer 相关 Effect 执行
│   ├── instance_workspace.rs   # Instance Workspace 相关 Effect 执行
│   ├── sql_workspace.rs        # SQL Workspace 相关 Effect 执行
│   └── utils.rs                # spawn_effect 辅助函数等
```

### 1.2 app_shell 文件职责

| 文件 | 内容 | 说明 |
|---|---|---|
| `mod.rs` | 模块声明和公开接口 | re-export 关键类型 |
| `msg.rs` | `AppMsg` 枚举 | 全局消息入口，包装所有 Feature 的 Msg |
| `state.rs` | `ShellState` 结构体 | 壳层状态（焦点、全局状态等） |
| `update.rs` | `update(app_msg, shell_state)` | 顶层消息分派，调用各 Feature 的 update |
| `view.rs` | 全局渲染函数 | key_echo overlay、调试信息等 |
| `focus.rs` | 焦点/区域导航 | FocusZone 切换逻辑 |
| `intent_router.rs` / `intent_router/` | `IntentRouter` | Intent → Msg 路由 |
| `effect_runner.rs` / `effect_runner/` | `EffectRunner` | Effect 异步执行 → Msg 回传 |

### 1.3 Feature 目录结构

每个 Feature 必须使用以下标准文件结构：

```
feature_name/
├── mod.rs          # 模块声明和公开接口
├── msg.rs          # FeatureMsg 枚举定义
├── state.rs        # FeatureState 结构体定义
├── update.rs       # update() 函数实现
├── view.rs         # 渲染函数实现
├── intent.rs       # FeatureIntent 枚举定义（可选）
├── effect.rs       # FeatureEffect 枚举定义（可选）
└── sub_module/     # 子模块（可选）
    ├── mod.rs
    ├── msg.rs
    ├── state.rs
    ├── update.rs
    └── view.rs
```

### 1.4 文件存在规则

#### app_shell 文件

| 文件 | 是否必须 | 说明 |
|---|---|---|
| `mod.rs` | ✅ 必须 | 模块声明和公开 re-export |
| `msg.rs` | ✅ 必须 | 定义 `AppMsg` 枚举 |
| `state.rs` | ✅ 必须 | 定义 `ShellState` 结构体 |
| `update.rs` | ✅ 必须 | 顶层消息分派 |
| `intent_router.rs` / `intent_router/` | ✅ 必须 | Intent 路由 |
| `effect_runner.rs` / `effect_runner/` | ✅ 必须 | Effect 执行 |
| `view.rs` | ❌ 可选 | 全局 overlay 渲染 |
| `focus.rs` | ❌ 可选 | 焦点管理 |

#### Feature 文件

| 文件 | 是否必须 | 说明 |
|---|---|---|
| `mod.rs` | ✅ 必须 | 模块声明和公开 re-export |
| `msg.rs` | ✅ 必须（交互式 Feature） | 定义 `FeatureMsg` 枚举 |
| `state.rs` | ✅ 必须（有状态的 Feature） | 定义 `FeatureState` 结构体 |
| `update.rs` | ✅ 必须（有 Msg 的 Feature） | 定义 `update()` 函数 |
| `view.rs` | ✅ 必须（有渲染的 Feature） | 定义 `render()` 函数 |
| `intent.rs` | ❌ 可选 | 当 Feature 需要跨 Feature 通信时 |
| `effect.rs` | ❌ 可选 | 当 Feature 有异步操作时 |

### 1.5 特殊类型 Feature 的文件结构

#### 被动计算 Feature（如 perf_monitor）

```
perf_monitor/
├── mod.rs
├── state.rs        # PerfState 结构体
├── update.rs       # update(state, frame_instant) -> State
└── view.rs         # 渲染函数
```

**特点**: 没有 `msg.rs`、`intent.rs`、`effect.rs`，update 函数直接接收外部参数。

#### 纯显示 Feature（如 global_footer）

```
global_footer/
├── mod.rs
└── view.rs         # render(frame, area, params)
```

**特点**: 没有 `msg.rs`、`state.rs`、`update.rs`、`intent.rs`、`effect.rs`，只有渲染函数。

---

## 二、命名规范

### 2.1 文件名规范

所有文件使用**小写蛇形命名**（snake_case）：

| 文件名 | 用途 |
|---|---|
| `mod.rs` | 模块声明 |
| `msg.rs` | 消息枚举 |
| `state.rs` | 状态结构体 |
| `update.rs` | 更新函数 |
| `view.rs` | 渲染函数 |
| `intent.rs` | 意图枚举 |
| `effect.rs` | 副作用枚举 |
| `focus.rs` | 焦点管理（app_shell 专用） |
| `intent_router/` | 意图路由（目录，按功能拆分） |
| `effect_runner/` | 副作用执行（目录，按功能拆分） |

### 2.2 类型命名规范

#### 基本规则

- **枚举/结构体**: 使用 PascalCase（大驼峰）
- **函数/方法**: 使用 snake_case（小写下划线）
- **常量**: 使用 SCREAMING_SNAKE_CASE（全大写下划线）
- **trait**: 使用 PascalCase

#### app_shell 相关类型命名

| 类型 | 命名格式 | 示例 | 说明 |
|---|---|---|---|
| 全局消息枚举 | `AppMsg` | `AppMsg` | 包装所有 Feature 的 Msg |
| 壳层状态结构体 | `ShellState` | `ShellState` | app_shell 的状态 |
| 全局意图枚举 | `AppIntent`（可选） | `AppIntent` | 聚合所有 Feature 的 Intent |
| 全局副作用枚举 | `AppEffect`（可选） | `AppEffect` | 聚合所有 Feature 的 Effect |
| 焦点枚举 | `FocusZone` | `FocusZone` | 各 Feature 的焦点区域 |

**注意**: `AppIntent` 和 `AppEffect` 是可选的。也可以在 `AppMsg` 中直接使用枚举变体包装各 Feature 的 Intent/Effect，而不需要单独定义。

#### Feature 相关类型命名

| 类型 | 命名格式 | 示例 |
|---|---|---|
| 消息枚举 | `{FeatureName}Msg` | `HeaderMsg`, `ExplorerMsg` |
| 状态结构体 | `{FeatureName}State` | `HeaderState`, `ExplorerState` |
| 意图枚举 | `{FeatureName}Intent` | `HeaderIntent`, `ExplorerIntent` |
| 副作用枚举 | `{FeatureName}Effect` | `DiscoverEffect`, `SqlEffect` |
| Feature 本身 | `{FeatureName}` | `Header`, `Explorer` |

#### 子模块类型命名

| 类型 | 命名格式 | 示例 |
|---|---|---|
| 消息枚举 | `{SubModule}Msg` | `InstancesMsg`, `ConnectionsMsg` |
| 状态结构体 | `{SubModule}State` | `InstancesState`, `ConnectionsState` |
| 意图枚举 | `{SubModule}Intent` 或直接使用父 Feature 的 Intent | 通常不单独定义 |
| 副作用枚举 | `{SubModule}Effect` 或直接使用父 Feature 的 Effect | 通常不单独定义 |

**注意**: 子模块的 Intent 和 Effect 通常**冒泡到父 Feature**，所以大多数情况下不需要单独定义。

### 2.3 函数命名规范

#### app_shell 函数

| 函数类型 | 命名格式 | 示例 | 说明 |
|---|---|---|---|
| 顶层更新函数 | `update(app_msg, shell_state)` | `app_shell::update()` | 分派消息 |
| 意图路由 | `route(intent, shell_state)` | `intent_router::route()` | Intent → Msg |
| 副作用执行 | `run(effects, tx, services)` | `effect_runner::run()` | Effect → 异步任务 |
| 辅助函数 | `spawn_effect(...)` | `effect_runner::utils::spawn_effect()` | 通用辅助 |

#### Feature 函数

| 函数类型 | 命名格式 | 示例 |
|---|---|---|
| 更新函数 | `update(msg, state)` | `header::update()`, `explorer::update()` |
| 渲染函数 | `render(frame, area, ...)` 或 `view(state, ...)` | `header::render()`, `instances::view()` |
| 子模块更新 | `update(msg, state)` | `instances::update()` |
| 子模块渲染 | `view(state, ...)` | `instances::view()` |

### 2.4 模块路径命名

```rust
// app_shell 模块路径
crate::app_shell::msg::AppMsg
crate::app_shell::state::ShellState
crate::app_shell::intent_router::route
crate::app_shell::effect_runner::run

// Feature 模块路径
crate::features::header::msg::HeaderMsg
crate::features::explorer::state::ExplorerState
crate::features::explorer::instances::msg::InstancesMsg

// 正确的 import 方式
use crate::app_shell::AppMsg;
use crate::app_shell::ShellState;
use crate::features::header::HeaderMsg;
use crate::features::explorer::ExplorerState;
use crate::features::explorer::instances::InstancesMsg;
```

---

## 三、接口契约

### 3.1 app_shell 接口

#### app_shell update() 函数

```rust
/// 顶层消息分派函数
pub fn update(
    shell: &mut ShellState,
    msg: AppMsg,
) {
    match msg {
        AppMsg::Header(msg) => {
            let (intents, effects) = shell.header.update(msg);
            
            for intent in intents {
                intent_router::route(
                    FeatureIntent::Header(intent), 
                    shell
                );
            }
            
            effect_runner::run(
                effects.into_iter()
                    .map(FeatureEffect::Header)
                    .collect(),
                &shell.tx,
                &shell.services,
            );
        }
        // 其他 Feature...
    }
}
```

#### IntentRouter 接口

```rust
// 小规模：单文件
pub fn route(
    intent: FeatureIntent, 
    shell: &mut ShellState,
) {
    match intent {
        FeatureIntent::Header(HeaderIntent::OpenDiscoverModal) => {
            // 直接修改 ShellState 或触发其他 Feature 的 Msg
            shell.discover.open_modal = true;
        }
        FeatureIntent::Explorer(ExplorerIntent::InstanceSelected { idx, name }) => {
            // 产生新的 AppMsg
            // 方式 1：直接调用目标 Feature 的 update
            let (new_iw_state, iw_intents, iw_effects) = 
                shell.instance_workspace.update(
                    IwMsg::LoadInstance { idx, name }
                );
            shell.instance_workspace = new_iw_state;
            // 继续处理 iw_intents 和 iw_effects...
            
            // 方式 2：将 Msg 发送到主循环（如果在 update 外部）
            // tx.send(AppMsg::IwMsg(IwMsg::LoadInstance { idx, name })).unwrap();
        }
        // ...
    }
}

// 大规模：按功能拆分
pub mod header;
pub mod explorer;

pub fn route(intent: FeatureIntent, shell: &mut ShellState) {
    match intent {
        FeatureIntent::Header(intent) => header::route(intent, shell),
        FeatureIntent::Explorer(intent) => explorer::route(intent, shell),
        // ...
    }
}
```

#### EffectRunner 接口

```rust
// 小规模：单文件
pub fn run(
    effects: Vec<FeatureEffect>,
    tx: &Sender<AppMsg>,
    services: &Services,
) {
    for effect in effects {
        match effect {
            FeatureEffect::Explorer(ExplorerEffect::LoadInstances) => {
                let db = services.db.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let instances = db.load_instances().await;
                    tx.send(AppMsg::ExplorerMsg(
                        ExplorerMsg::InstancesLoaded { instances }
                    )).await.ok();
                });
            }
            FeatureEffect::Sql(SqlEffect::ExecuteSql { query, tab_id }) => {
                let db = services.db.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = db.execute(&query).await;
                    tx.send(AppMsg::SqlMsg(
                        SqlMsg::QueryResult { tab_id, result }
                    )).await.ok();
                });
            }
            // ...
        }
    }
}

// 大规模：按功能拆分
pub mod header;
pub mod explorer;
pub mod utils;

pub fn run(effects: Vec<FeatureEffect>, tx: &Sender<AppMsg>, services: &Services) {
    for effect in effects {
        match effect {
            FeatureEffect::Header(effect) => header::run(effect, tx, services),
            FeatureEffect::Explorer(effect) => explorer::run(effect, tx, services),
            // ...
        }
    }
}
```

### 3.2 Feature update() 函数签名

#### 交互式 Feature（有 Intent 和 Effect）

```rust
/// 有跨 Feature 通信和异步操作的 Feature
pub fn update(
    msg: FeatureMsg,
    state: &FeatureState,
) -> (FeatureState, Vec<FeatureIntent>, Vec<FeatureEffect>)
```

#### 交互式 Feature（只有 Intent，无 Effect）

```rust
/// 只有跨 Feature 通信，无异步操作的 Feature
pub fn update(
    msg: FeatureMsg,
    state: &FeatureState,
) -> (FeatureState, Vec<FeatureIntent>)
```

#### 交互式 Feature（只有 Effect，无 Intent）

```rust
/// 只有异步操作，无跨 Feature 通信的 Feature
pub fn update(
    msg: FeatureMsg,
    state: &FeatureState,
) -> (FeatureState, Vec<FeatureEffect>)
```

#### 被动计算 Feature

```rust
/// 被动计算，不需要 Msg
pub fn update(
    state: &FeatureState,
    external_param: ExternalType,  // 如 Instant, FrameInfo 等
) -> FeatureState
```

### 3.3 渲染函数签名

#### app_shell 渲染函数

```rust
/// 全局渲染（overlay、调试信息等）
pub fn render_overlay(
    frame: &mut Frame,
    shell: &ShellState,
)
```

#### Feature 渲染函数

```rust
/// 标准 Feature 渲染
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &FeatureState,
    // ... 其他上下文参数
)
```

#### 子模块渲染函数

```rust
/// 子模块渲染
pub fn view(
    frame: &mut Frame,
    area: Rect,
    state: &SubModuleState,
    // ... 其他上下文参数
)
```

**注意**: Feature 的渲染函数命名为 `render()`，子模块的渲染函数命名为 `view()`，以便区分。

### 3.4 mod.rs 公开接口

#### app_shell/mod.rs

```rust
pub mod msg;
pub mod state;
pub mod update;
pub mod view;
pub mod focus;
pub mod intent_router;
pub mod effect_runner;

pub use msg::AppMsg;
pub use state::ShellState;
pub use update::update;
pub use intent_router::route;
pub use effect_runner::run;
```

#### feature_name/mod.rs

```rust
pub mod msg;
pub mod state;
pub mod update;
pub mod view;
pub mod intent;  // 可选
pub mod effect;  // 可选
pub mod sub_module;  // 可选

pub use msg::{FeatureMsg, SubModuleMsg};
pub use state::{FeatureState, SubModuleState};
pub use update::update;
pub use view::render;
pub use intent::FeatureIntent;  // 可选
pub use effect::FeatureEffect;  // 可选

// 子模块的公开接口
pub mod instances {
    pub use super::instances::msg::InstancesMsg;
    pub use super::instances::state::InstancesState;
    pub use super::instances::update::update;
    pub use super::instances::view::view;
}
```

---

## 四、层级与通信规则

### 4.1 层级结构

```
main.rs (入口)
    ↓ 调用
app_shell (顶层协调者)
    ↓ 管理
Feature (如 explorer)
    ↓ 管理
SubModule (如 instances)
```

### 4.2 通信规则

#### 跨 Feature 通信（通过 Intent）

```
Feature A.update()
    ↓ 产生 FeatureIntent
app_shell.update() 收集 Intent
    ↓ 调用
intent_router::route(intent, shell)
    ↓ 直接修改 ShellState 或调用目标 Feature 的 update
Feature B.update()
```

**规则**: 
- Feature A 不直接引用 Feature B 的类型
- 通过 `IntentRouter` 中转
- 路由规则在 `app_shell/intent_router.rs` 或 `app_shell/intent_router/` 中定义
- `IntentRouter` 可以访问 `ShellState`，并可调用任何 Feature 的公开方法

#### Effect 异步执行

```
Feature.update()
    ↓ 产生 FeatureEffect
app_shell.update() 收集 Effect
    ↓ 调用
effect_runner::run(effects, tx, services)
    ↓ tokio::spawn 异步执行
异步任务完成
    ↓ mpsc::channel 发送
tx.send(AppMsg::FeatureMsg(FeatureMsg::Result(...)))
    ↓
主循环接收 Msg
    ↓
app_shell.update() 处理结果
```

**规则**:
- `EffectRunner` 从 `update` 循环接收 `Vec<FeatureEffect>`
- 每个 Effect 启动一个 `tokio::spawn` 异步任务
- 结果通过 `mpsc` 通道发送为新的 `AppMsg`
- 主循环读取这些消息并反馈回 `update`

#### 父子模块通信（嵌套）

```
Feature.update(FeatureMsg::SubModuleMsg(msg))
    ↓ 直接调用
sub_module::update(msg, state.sub_module)
    ↓ 返回 (SubModuleState, Vec<ParentIntent>, Vec<ParentEffect>)
Feature.update() 冒泡 Intent/Effect 到 app_shell
```

**规则**:
- 父 Feature 通过嵌套 Msg 调用子模块
- 子模块的 Intent/Effect **冒泡到父 Feature**（返回父 Feature 的 Intent/Effect 类型）
- 父 Feature 不执行子模块的 Effect，也不路由子模块的 Intent
- Intent/Effect 冒泡到 app_shell 后，由 IntentRouter 和 EffectRunner 统一处理

### 4.3 Msg 嵌套规范

```rust
// 父 Feature 的 Msg 枚举
pub enum ExplorerMsg {
    // 自身消息
    SetFocusZone(ExplorerZone),
    ToggleTree,
    
    // 嵌套子模块消息
    InstancesMsg(InstancesMsg),
    ObjectsMsg(ObjectsMsg),
}

// 子模块的 Msg 枚举
pub enum InstancesMsg {
    SelectInstance { idx: usize },
    ToggleExpand { idx: usize },
    AddConnection { instance_idx: usize },
    // ...
}
```

### 4.4 AppMsg 包装规范

```rust
// app_shell/msg.rs
pub enum AppMsg {
    // 各 Feature 的 Msg
    HeaderMsg(HeaderMsg),
    DiscoverMsg(DiscoverMsg),
    ExplorerMsg(ExplorerMsg),
    IwMsg(IwMsg),
    SqlMsg(SqlMsg),
    
    // 全局消息
    FocusChanged { zone: FocusZone },
    Tick,  // 每帧触发
    Quit,
}
```

### 4.5 Feature 枚举统一包装

```rust
// app_shell 中统一包装各 Feature 的 Intent 和 Effect
pub enum FeatureIntent {
    Header(HeaderIntent),
    Discover(DiscoverIntent),
    Explorer(ExplorerIntent),
    Iw(IwIntent),
    Sql(SqlIntent),
}

pub enum FeatureEffect {
    Header(HeaderEffect),
    Discover(DiscoverEffect),
    Explorer(ExplorerEffect),
    Iw(IwEffect),
    Sql(SqlEffect),
}
```

**注意**: `FeatureIntent` 和 `FeatureEffect` 用于在 `intent_router` 和 `effect_runner` 中统一匹配。

---

## 五、枚举值命名规范

### 5.1 消息枚举值

使用**动词开头**，描述触发动作：

```rust
pub enum ExplorerMsg {
    // 动作 + 目标
    SelectInstance { idx: usize },
    ToggleExpand { idx: usize },
    LoadConnections { instance_idx: usize },
    
    // 被动结果（由 Effect 结果产生）
    InstancesLoaded { instances: Vec<ManagedInstance> },
    InstanceConnectionsLoaded { 
        instance_idx: usize, 
        connections: Vec<...> 
    },
    
    // 嵌套子模块
    InstancesMsg(InstancesMsg),
}
```

### 5.2 意图枚举值

使用**请求式命名**，以 `Request`、`Notify`、`Open`、`Connect`、`Export` 等动词开头：

```rust
pub enum ExplorerIntent {
    // 请求另一个 Feature 执行操作
    RequestAddConnection { instance_idx: usize },
    RequestOpenWorkspace { instance_idx: usize },
    
    // 通知另一个 Feature 状态变更
    NotifyInstancesChanged,
    NotifyContextChanged { database: String, schema: String },
    
    // 直接打开/关闭
    OpenDiscoverModal,
    CloseModal,
    
    // 请求连接
    ConnectToDatabase { connection_id: ConnectionId },
    
    // 请求导出
    ExportResult { format: ExportFormat, data: QueryResult },
}
```

### 5.3 副作用枚举值

使用**动词原形**，描述要执行的操作：

```rust
pub enum ExplorerEffect {
    // 异步加载
    LoadInstances,
    LoadInstanceConnections { instance_idx: usize },
    LoadObjectsTree { database: String, schema: String },
    
    // 异步操作
    DeleteInstance { idx: usize },
    RefreshInstance { idx: usize },
    TestConnection { instance_idx: usize, connection_idx: usize },
    
    // 持久化
    SaveInstance { instance: ManagedInstance },
    SaveToFile { path: PathBuf, content: String },
    
    // 异步执行
    ExecuteSql { query: String, tab_id: TabId },
}
```

### 5.4 状态枚举值

使用**状态描述式命名**：

```rust
pub enum FormMode {
    None,           // 空闲状态
    Add,            // 新增模式
    Edit { idx: usize },  // 编辑模式（携带索引）
}

pub enum ExplorerZone {
    Instances,
    Objects,
}

pub enum FocusZone {
    Header,
    Explorer,
    InstanceWorkspace,
    SqlWorkspace,
    GlobalFooter,
}
```

---

## 六、代码示例

### 6.1 app_shell 完整结构示例

```
app_shell/
├── mod.rs
├── msg.rs              # AppMsg
├── state.rs            # ShellState
├── update.rs           # app_shell::update()
├── view.rs             # render_overlay()
├── focus.rs            # FocusZone 切换
├── intent_router.rs    # route(intent, shell) - 小规模
├── effect_runner.rs    # run(effects, tx, services) - 小规模
```

### 6.2 app_shell/msg.rs 示例

```rust
// app_shell/msg.rs

pub enum AppMsg {
    // 各 Feature 的 Msg
    HeaderMsg(HeaderMsg),
    DiscoverMsg(DiscoverMsg),
    ExplorerMsg(ExplorerMsg),
    IwMsg(IwMsg),
    SqlMsg(SqlMsg),
    
    // 全局消息
    FocusChanged { zone: FocusZone },
    Tick,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusZone {
    Header,
    Explorer,
    InstanceWorkspace,
    SqlWorkspace,
    GlobalFooter,
}
```

### 6.3 app_shell/state.rs 示例

```rust
// app_shell/state.rs

use std::sync::mpsc;

#[derive(Clone)]
pub struct ShellState {
    pub header: HeaderState,
    pub discover: DiscoverState,
    pub explorer: ExplorerState,
    pub instance_workspace: IwState,
    pub sql_workspace: SqlWorkspaceState,
    
    pub focus_zone: FocusZone,
    pub global_status: String,
    
    pub tx: mpsc::Sender<AppMsg>,
    pub services: Services,
}

pub struct Services {
    pub db: DatabaseService,
    pub storage: StorageService,
    pub scanner: ScannerService,
}
```

### 6.4 app_shell/update.rs 示例

```rust
// app_shell/update.rs

use super::msg::AppMsg;
use super::state::ShellState;
use super::intent_router;
use super::effect_runner;
use super::FeatureIntent;
use super::FeatureEffect;

pub fn update(shell: &mut ShellState, msg: AppMsg) {
    match msg {
        // 各 Feature 的消息分派
        AppMsg::HeaderMsg(header_msg) => {
            let (new_state, intents, effects) = 
                shell.header.update(header_msg);
            shell.header = new_state;
            
            for intent in intents {
                intent_router::route(
                    FeatureIntent::Header(intent), 
                    shell
                );
            }
            
            effect_runner::run(
                effects.into_iter()
                    .map(FeatureEffect::Header)
                    .collect(),
                &shell.tx,
                &shell.services,
            );
        }
        
        AppMsg::ExplorerMsg(explorer_msg) => {
            let (new_state, intents, effects) = 
                shell.explorer.update(explorer_msg);
            shell.explorer = new_state;
            
            for intent in intents {
                intent_router::route(
                    FeatureIntent::Explorer(intent), 
                    shell
                );
            }
            
            effect_runner::run(
                effects.into_iter()
                    .map(FeatureEffect::Explorer)
                    .collect(),
                &shell.tx,
                &shell.services,
            );
        }
        
        // ... 其他 Feature
        
        // 全局消息
        AppMsg::FocusChanged { zone } => {
            shell.focus_zone = zone;
        }
        
        AppMsg::Tick => {
            // 更新 perf_monitor（被动计算）
            shell.perf_monitor = perf_monitor::update(
                &shell.perf_monitor, 
                Instant::now()
            );
        }
        
        AppMsg::Quit => {
            // 退出逻辑
        }
    }
}
```

### 6.5 app_shell/intent_router.rs 示例

```rust
// app_shell/intent_router.rs

use super::state::ShellState;
use super::FeatureIntent;

pub fn route(intent: FeatureIntent, shell: &mut ShellState) {
    match intent {
        // Header Intents
        FeatureIntent::Header(HeaderIntent::OpenDiscoverModal) => {
            shell.discover.open_modal = true;
        }
        
        // Discover Intents
        FeatureIntent::Discover(DiscoverIntent::CloseModal) => {
            shell.discover.open_modal = false;
        }
        FeatureIntent::Discover(DiscoverIntent::NotifyInstancesChanged) => {
            let (new_explorer, intents, effects) = 
                shell.explorer.update(ExplorerMsg::RefreshInstances);
            shell.explorer = new_explorer;
            // 继续处理 intents 和 effects...
        }
        
        // Explorer Intents
        FeatureIntent::Explorer(ExplorerIntent::InstanceSelected { 
            instance_idx, 
            instance_name 
        }) => {
            let (new_iw, iw_intents, iw_effects) = 
                shell.instance_workspace.update(
                    IwMsg::LoadInstance { 
                        instance_idx, 
                        instance_name 
                    }
                );
            shell.instance_workspace = new_iw;
            // 继续处理 iw_intents 和 iw_effects...
        }
        
        // ... 其他 Intent
    }
}
```

### 6.6 app_shell/effect_runner.rs 示例

```rust
// app_shell/effect_runner.rs

use std::sync::mpsc;
use super::msg::AppMsg;
use super::state::Services;
use super::FeatureEffect;

pub fn run(
    effects: Vec<FeatureEffect>,
    tx: &mpsc::Sender<AppMsg>,
    services: &Services,
) {
    for effect in effects {
        match effect {
            // Explorer Effects
            FeatureEffect::Explorer(ExplorerEffect::LoadInstances) => {
                let db = services.db.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let instances = db.load_instances().await;
                    tx.send(AppMsg::ExplorerMsg(
                        ExplorerMsg::InstancesLoaded { instances }
                    )).await.ok();
                });
            }
            
            FeatureEffect::Explorer(ExplorerEffect::LoadObjectsTree { 
                database, 
                schema 
            }) => {
                let db = services.db.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let objects = db.fetch_objects(&database, &schema).await;
                    tx.send(AppMsg::ExplorerMsg(
                        ExplorerMsg::ObjectsTreeLoaded { 
                            database, 
                            objects 
                        }
                    )).await.ok();
                });
            }
            
            // SQL Workspace Effects
            FeatureEffect::Sql(SqlEffect::ExecuteSql { 
                query, 
                tab_id 
            }) => {
                let db = services.db.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = db.execute(&query).await;
                    tx.send(AppMsg::SqlMsg(
                        SqlMsg::QueryResult { tab_id, result }
                    )).await.ok();
                });
            }
            
            // ... 其他 Effect
        }
    }
}
```

### 6.7 完整的 Feature 结构示例

以 `explorer` 为例：

```
explorer/
├── mod.rs
├── msg.rs              # ExplorerMsg, InstancesMsg, ObjectsMsg
├── state.rs            # ExplorerState, InstancesState, ObjectsState
├── update.rs           # explorer::update(), instances::update(), objects::update()
├── view.rs             # explorer::render(), instances::view(), objects::view()
├── intent.rs           # ExplorerIntent
├── effect.rs           # ExplorerEffect
├── instances/          # 子模块（可选）
│   ├── mod.rs
│   ├── msg.rs          # InstancesMsg（如果独立定义）
│   ├── state.rs        # InstancesState（如果独立定义）
│   ├── update.rs       # instances::update()（如果独立定义）
│   └── view.rs         # instances::view()（如果独立定义）
└── objects/            # 子模块（可选）
    ├── mod.rs
    ├── msg.rs
    ├── state.rs
    ├── update.rs
    └── view.rs
```

---

## 七、检查清单

在创建新的 TEA 模块时，使用以下检查清单确保符合规范：

### 7.1 app_shell 检查

- [ ] 包含 `msg.rs`（`AppMsg`）、`state.rs`（`ShellState`）、`update.rs`
- [ ] 包含 `intent_router.rs` 或 `intent_router/` 目录
- [ ] 包含 `effect_runner.rs` 或 `effect_runner/` 目录
- [ ] `IntentRouter` 能访问 `ShellState` 并调用 Feature 的 update
- [ ] `EffectRunner` 使用 `tokio::spawn` 执行异步任务
- [ ] `EffectRunner` 通过 `mpsc` 通道回传 `AppMsg`

### 7.2 目录结构检查

- [ ] Feature 目录使用小写蛇形命名
- [ ] 包含 `mod.rs` 文件
- [ ] 包含 `msg.rs`、`state.rs`、`update.rs`、`view.rs`（根据类型）
- [ ] 包含 `intent.rs`、`effect.rs`（如果需要）
- [ ] 子模块有独立目录或直接内联在父模块中

### 7.3 命名检查

- [ ] 消息枚举使用 `{FeatureName}Msg` 命名
- [ ] 状态结构体使用 `{FeatureName}State` 命名
- [ ] 意图枚举使用 `{FeatureName}Intent` 命名（如果需要）
- [ ] 副作用枚举使用 `{FeatureName}Effect` 命名（如果需要）
- [ ] 枚举值使用动词开头，描述清晰

### 7.4 接口检查

- [ ] Feature 的 `update()` 返回 `(State, Vec<Intent>, Vec<Effect>)`
- [ ] Feature 的 `render()`/`view()` 函数签名符合规范
- [ ] 子模块的 Intent/Effect 冒泡到父 Feature
- [ ] `app_shell` 的 `update()` 正确分派消息

### 7.5 通信检查

- [ ] 跨 Feature 通信通过 `IntentRouter`，不直接引用其他 Feature 的 Msg
- [ ] `IntentRouter` 可以直接修改 `ShellState` 或调用目标 Feature 的 update
- [ ] `EffectRunner` 使用统一的 `mpsc` 通道回传结果
- [ ] 父子模块通信通过嵌套 Msg

---

## 八、FAQ

### Q1: `IntentRouter` 应该用单文件还是目录？

**判断标准**:
- 文件行数超过 200 行 → 拆分
- 处理超过 5 个 Feature 变体 → 拆分
- 有多个开发者同时修改 → 拆分

**拆分方式**:
```
intent_router/
├── mod.rs           # pub fn route(intent, shell)
├── header.rs        # Header 相关 Intent 路由
├── explorer.rs      # Explorer 相关 Intent 路由
├── sql_workspace.rs # SQL Workspace 相关 Intent 路由
└── ...
```

### Q2: `EffectRunner` 应该用单文件还是目录？

**判断标准同 IntentRouter**。

**拆分方式**:
```
effect_runner/
├── mod.rs           # pub fn run(effects, tx, services)
├── utils.rs         # spawn_effect 辅助函数
├── explorer.rs      # Explorer 相关 Effect 执行
├── sql_workspace.rs # SQL Workspace 相关 Effect 执行
└── ...
```

### Q3: `IntentRouter` 应该直接修改状态还是发送 Msg？

**两种方式都可以**：

**方式 A：直接修改状态**
```rust
FeatureIntent::Explorer(ExplorerIntent::InstanceSelected { idx, name }) => {
    let (new_iw, _, _) = shell.instance_workspace.update(
        IwMsg::LoadInstance { idx, name }
    );
    shell.instance_workspace = new_iw;
}
```

**方式 B：发送 Msg 到主循环**
```rust
FeatureIntent::Explorer(...) => {
    // 方式 1：通过 tx 发送
    tx.send(AppMsg::IwMsg(IwMsg::LoadInstance { idx, name })).unwrap();
    
    // 方式 2：返回 AppMsg 列表，由 app_shell 统一处理
    return vec![AppMsg::IwMsg(IwMsg::LoadInstance { idx, name })];
}
```

**选择建议**:
- 方式 A 简单直接，适合小规模应用
- 方式 B 更符合单向数据流原则，适合大规模应用
- 两种方式可以混合使用

### Q4: 子模块的 msg.rs 和 state.rs 应该放在哪里？

**方案 A（推荐）**: 放在子模块目录下
```
explorer/
├── msg.rs          # 只包含 ExplorerMsg
├── state.rs        # 只包含 ExplorerState
├── instances/
│   ├── msg.rs      # 包含 InstancesMsg
│   └── state.rs    # 包含 InstancesState
```

**方案 B**: 全部放在父 Feature 下
```
explorer/
├── msg.rs          # 包含 ExplorerMsg + InstancesMsg + ObjectsMsg
├── state.rs        # 包含 ExplorerState + InstancesState + ObjectsState
```

**选择建议**:
- 当子模块逻辑复杂、有独立的 update() 函数时，使用方案 A
- 当子模块简单、没有独立的 update() 函数时，使用方案 B

### Q5: 如何决定子模块是否需要独立的目录？

判断标准：
1. **状态复杂度**: 子模块的 State 是否超过 5 个字段
2. **交互复杂度**: 子模块是否有独立的 Msg，且 Msg 枚举值超过 5 个
3. **代码行数**: 子模块的 update() + view() 代码是否超过 100 行

如果以上任意一项满足，建议创建独立目录。

### Q6: 父子模块的 Intent/Effect 如何处理？

**规则**: 子模块的 Intent/Effect **冒泡到父 Feature**。

```rust
// 子模块的 update() 返回父 Feature 的 Intent/Effect 类型
pub fn update(
    msg: InstancesMsg,
    state: &InstancesState,
) -> (InstancesState, Vec<ExplorerIntent>, Vec<ExplorerEffect>) {
    // ...
}
```

**不推荐**: 子模块定义独立的 Intent/Effect，然后在父 Feature 中转换。这样会增加复杂度。

### Q7: 被动计算 Feature 的 update() 函数如何命名？

被动计算 Feature 的 update() 函数仍然命名为 `update()`，但参数不同：

```rust
// perf_monitor/update.rs
pub fn update(
    state: &PerfState,
    frame_instant: Instant,  // 外部参数
) -> PerfState {
    // 被动计算逻辑
}
```

### Q8: 纯显示 Feature 需要 state.rs 吗？

纯显示 Feature **不需要** state.rs，因为它没有状态。渲染函数接收所有必要的参数作为函数参数。

---

## 九、变更记录

| 版本 | 日期 | 变更内容 |
|---|---|---|
| 2.0 | 2026-08-01 | 改名为 TEA 模块命名规范，新增 app_shell 命名规范，对齐 Msg-Intent-Effect 设计指南 |
| 1.0 | 2026-08-01 | 初始版本，定义 TEA 子模块命名规范 |
