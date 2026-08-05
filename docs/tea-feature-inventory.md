# Feature 清单文档 — Nested TEA + Central Message Router 架构蓝图

**架构模式**: Nested TEA (The Elm Architecture) + Central Message Router

**目标**: 将 dbm-tui 从单体 App/AppModel 结构迁移到 Nested TEA + Central Message Router 架构
**核心原则**: 先物理重组 Feature 边界，再抽离 shared，最后移除 Deref

***

## 〇、架构模式说明

### 0.1 Nested TEA 是什么

TEA (The Elm Architecture) 是一种 Model-Update-View 模式：

- **Model (State)**: 存储当前状态
- **Update**: 接收消息 (Msg)，返回新的 Model 和副作用 (Effect)
- **View**: 根据 Model 渲染 UI

**Nested TEA** 将应用拆分为多层嵌套的 TEA 循环：

- **外层**: `app_shell` 管理全局消息路由、焦点、模态
- **中层**: 7 个 Feature 各自拥有独立的 TEA 循环（msg → state → update → view）
- **内层**: Feature 内部的子面板（如 Results、History）共享 Feature 的 TEA 循环

```
┌──────────────────────────────────────────────────────────────┐
│                     app_shell (外层 TEA)                      │
│  AppMsg → ShellState → ShellUpdate → ShellView               │
│                                                              │
│  ┌────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │  header    │  │  perf_monitor│  │  discover            │  │
│  │  (Feat 1)  │  │  (Feat 2)    │  │  (Feature 3)         │  │
│  │            │  │              │  │    ├── engine/       │  │
│  └────────────┘  └──────────────┘  │    ├── targets/      │  │
│                                    │    └── results/      │  │
│                                    └──────────────────────┘  │
│                                                              │
│  ┌──────────────┐  ┌──────────────────────┐                  │
│  │  global_     │  │  explorer            │                  │
│  │  footer      │  │  (Feature 5)         │                  │
│  │  (Feat 4)    │  │    ├── instances/    │                  │
│  └──────────────┘  │    └── objects/      │                  │
│                    └──────────────────────┘                  │
│                                                              │
│  ┌──────────────────────┐                                    │
│  │  instance_workspace  │                                    │
│  │  (Feature 6)         │                                    │
│  │    ├── overview/     │                                    │
│  │    └── connections/  │                                    │
│  └──────────────────────┘                                    │
│                                                              │
│  ┌──────────────────────────────────────────────┐            │
│  │  sql_workspace (Feature 7)                   │            │
│  │    └── sql_tab/                              │            │
│  │        ├── editor/                           │            │
│  │        │   ├── context_picker/               │            │
│  │        │   └── sql_completion/               │            │
│  │        ├── results/                          │            │
│  │        │   └── detail/                       │            │
│  │        └── history/                          │            │
│  └──────────────────────────────────────────────┘            │
└──────────────────────────────────────────────────────────────┘
```

### 0.2 Central Message Router 是什么

**Central Message Router** 是 Nested TEA 的补充机制：

- 所有消息统一经过 `app_shell` 路由
- Feature 之间**不直接通信**
- 跨 Feature 操作通过 `app_shell` 中转
- Effect（副作用）由 `app_shell` 统一执行

**消息流**:

```
用户输入
    │
    ▼
┌──────────────────────────────────────────────┐
│  app_shell::update(AppMsg)                   │
│                                              │
│  1. 判断当前焦点                               │
│  2. 将消息路由到对应 Feature                   │
│  3. 收集所有 Effect                           │
│  4. 执行 Effect（网络请求、存储等）              │
│  5. 检查是否有新的消息需要派发                   │
│                                              │
└──────────────────────────────────────────────┘
    │
    ▼ (Effect 执行后产生新的内部消息)
    │
┌──────────────────────────────────────────────┐
│  app_shell 再次路由内部消息                    │
│  (如: 查询结果到达 → 更新 Results Feature)    │
└──────────────────────────────────────────────┘
```

**与 Nested TEA 的关系**:

- Nested TEA 定义了"状态如何组织"和"视图如何分层"
- Central Message Router 定义了"消息如何流动"和"副作用如何执行"
- 两者结合确保每个 Feature 都是自包含的，同时又能通过中心协调者进行安全的交互

***

## 一、顶层目录结构

```
dbm-tui/src/
│
├── app_shell/                  # 【壳层】顶层协调者 — 管理 App 生命周期、路由、焦点、模态、调试覆盖层
│   ├── mod.rs
│   ├── msg.rs                  # AppMsg: 全局消息枚举
│   ├── state.rs                # ShellState + KeyEchoState (调试覆盖层)
│   ├── update.rs               # update(): 消息路由、焦点切换、模态管理
│   ├── debug_overlay.rs        # key_echo 调试覆盖层渲染
│   └── session.rs              # Session 持久化序列化/反序列化
│
├── features/
│   ├── header/                # 【Feature 1】顶部导航栏（应用标题、连接状态、快捷按钮）
│   ├── perf_monitor/          # 【Feature 2】性能监控（fps、冗余重绘率等）
│   ├── discover/             # 【Feature 3】实例发现/注册 Modal
│   ├── global_footer/        # 【Feature 4】底部全局快捷键提示栏（不含性能数据）
│   ├── explorer/             # 【Feature 5】连接树导航（左栏）
│   ├── instance_workspace/   # 【Feature 6】实例详情工作台
│   └── sql_workspace/        # 【Feature 7】SQL 工作台
│
├── common/                     # 【基础设施】真正跨 Feature 通用的工具
├── epoch.rs                    # 【基础设施】RenderKey 系统
├── lib.rs                      # 入口文件（不变）
└── hints.rs                    # 待拆分 → global_footer/ + 各 feature view.rs
```

***

## 二、Feature 内子模块的两种类型

在 Nested TEA 中，每个 Feature 内部可以包含两种不同粒度的子模块。**核心原则：只要子模块有自己的 state 和交互逻辑，就必须有自己的 Msg 类型，遵循标准 TEA 嵌套模式。**

### 类型 1: 完整 TEA 子模块（msg + state + update + view）

- **必须有独立的 Msg 类型**：子模块的交互是自己的状态转换，必须有独立的 Msg
- **必须有独立的 update 函数**：接收子模块自己的 Msg + State
- **父级 Msg 通过枚举嵌套转发**：`ParentMsg::ChildMsg(ChildMsg)`
- **可以独立测试**：不需要构造父级 Msg 或父级 State

```
explorer/
├── msg.rs                   # ExplorerMsg
├── state.rs                 # ExplorerState
├── update.rs                # Explorer update (解包并转发)
├── view.rs                  # Explorer 主渲染
├── instances/               # 类型 1 子模块 (独立 TEA)
│   ├── mod.rs
│   ├── msg.rs               # InstancesMsg (独立 Msg)
│   ├── state.rs             # InstancesState
│   ├── update.rs            # instances::update(InstancesMsg, InstancesState)
│   └── view.rs              # instances::view(InstancesState)
└── objects/                 # 类型 1 子模块 (独立 TEA)
    ├── mod.rs
    ├── msg.rs               # ObjectsMsg (独立 Msg)
    ├── state.rs             # ObjectsState
    ├── update.rs            # objects::update(ObjectsMsg, ObjectsState)
    └── view.rs              # objects::view(ObjectsState)
```

```rust
// explorer/msg.rs
pub enum ExplorerMsg {
    InstancesMsg(InstancesMsg),   // 嵌套子模块消息
    ObjectsMsg(ObjectsMsg),
    FocusPane { pane: ExplorerPane },
    SearchInput { text: String },
}

// explorer/update.rs
// 纯函数式按值 update：输入旧状态，move 出被改动的子状态更新后放回，
// 其余字段原样保留，避免整棵深克隆。
pub fn update(msg: ExplorerMessage, mut state: ExplorerState)
    -> (ExplorerState, Vec<ExplorerIntent>, Vec<ExplorerEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    
    match msg {
        ExplorerMessage::Instances(m) => {
            // 解包并转发给子模块（只移动 instances 这一个子状态）
            let instances::msg::InstancesMsg::Message(inner) = m;
            let instances_state = std::mem::take(&mut state.instances);
            let (s, i, e) = instances::update(inner, instances_state);
            state.instances = s;
            intents.extend(i.into_iter().map(ExplorerIntent::Instances));
            effects.extend(e.into_iter().map(ExplorerEffect::Instances));
        }
        ExplorerMessage::Objects(m) => {
            let objects_msg::ObjectsMsg::Message(inner) = m;
            let objects_state = std::mem::take(&mut state.objects);
            let (s, i, e) = objects::update(inner, objects_state);
            state.objects = s;
            intents.extend(i.into_iter().map(ExplorerIntent::Objects));
            effects.extend(e.into_iter().map(ExplorerEffect::Objects));
        }
        ExplorerMessage::FocusPane { pane } => { ... },
    }
    
    (state, intents, effects)
}

// 子模块可以独立测试
// instances/tests.rs
#[test]
fn test_expand_instance() {
    let state = InstancesState::default();
    let (new_state, effects) = instances::update(
        InstancesMsg::ExpandInstance { idx: 0 },
        &state
    );
    // 不需要构造 ExplorerMsg 或 ExplorerState
}
```

### 类型 2: 纯视图子模块（view only）

- **没有独立的 Msg 类型**：没有自己的交互逻辑
- **没有独立的 update 函数**：状态完全由父级控制
- **直接在父级 view 中绘制**：作为父级 view 的辅助函数
- **不需要独立目录结构**

```
sql_workspace/
├── msg.rs
├── state.rs
├── update.rs
├── view.rs                  // 包含类型 2 子模块的渲染函数
├── view_tab_bar.rs          // 纯视图子模块 (可选，用于代码组织)
└── sql_tab/                 // editor/results/history 的父 feature
    ├── editor/
    ├── results/
    └── history/
```

```rust
// sql_workspace/view.rs 或 view_tab_bar.rs
pub fn render_tab_bar(tabs: &[SqlTab], active_tab: usize, area: Rect) -> Paragraph {
    // 纯渲染，不需要独立的 Msg 或 update
    // 状态完全由 SqlWorkspaceState 控制
}
```

### 判断标准

```
子模块是否有自己的交互逻辑？
├── 是（如展开/折叠、滚动、选择、搜索）→ 类型 1（完整 TEA 子模块）
└── 否（如标签页、状态栏、装饰性元素）→ 类型 2（纯视图子模块）
```

| 判断项          | 类型 1 (完整 TEA 子模块) | 类型 2 (纯视图子模块)       |
| ------------ | ----------------- | ------------------- |
| 是否有独立 Msg    | ✅ 有               | ❌ 无                 |
| 是否有独立 State  | ✅ 有               | ❌ 无（或共享父级）          |
| 是否有独立 update | ✅ 有               | ❌ 无                 |
| 是否可以独立测试     | ✅ 可以              | N/A                 |
| 代码组织         | 目录结构（子目录）         | 文件结构（父级 view 的辅助函数） |

### 与类型 A/B 的对应关系（之前的命名）

之前定义的"类型 A（完整 TEA）"和"类型 B（轻量 TEA）"都是**类型 1** 的不同规模：

- 之前的"类型 A"：代码量大（> 300 行）的类型 1 子模块，使用目录结构
- 之前的"类型 B"：代码量小（< 300 行）的类型 1 子模块，使用文件结构
- 统一为：**只要是类型 1，就必须有独立的 Msg**

***

## 二·五、子模块 TEA 循环评估结果

### 评估标准

- **状态复杂度**: 状态字段数量和类型（如是否有独立光标、滚动、编辑模式等）
- **交互复杂度**: 是否有独立的键盘/鼠标处理逻辑
- **代码量**: 渲染和交互逻辑的代码行数
- **独立需求**: 是否需要独立的 Effect（如异步操作）或独立的焦点管理

### Feature 3: `discover` 评估

| 子模块         | 状态字段                                 | 交互复杂度              | 代码量       | 评估结果                                        |
| ----------- | ------------------------------------ | ------------------ | --------- | ------------------------------------------- |
| **engine**  | 2 个（`engine: Engine`、选中状态）          | 低（选择引擎，切换后需通知顶层联动扫描）      | \~30 行    | ✅ **需要独立 TEA** — 作为扫描的决策者，切换引擎需产生 Intent 通知 discover 顶层 |
| **targets** | 9 个（targets、edit\_buf、undo\_stack 等） | 中等（添加/删除/编辑/撤销/粘贴） | \~200 行交互 | ✅ **需要独立 TEA** — 复杂编辑交互、撤销/重做               |
| **results** | 7 个（items、cursor、selected、scan 等）    | 中等（光标移动、选择、过滤）     | \~80 行交互  | ✅ **需要独立 TEA** — 异步扫描、独立光标、选中集              |

### Feature 5: `explorer` 评估

| 子模块           | 状态字段                                                    | 交互复杂度                 | 代码量     | 评估结果                                  |
| ------------- | ------------------------------------------------------- | --------------------- | ------- | ------------------------------------- |
| **instances** | 3 个（instances、active\_workspace、search）                 | 低-中（展开/折叠/选择/搜索）      | \~200 行 | ✅ **需要独立 TEA** — 独立的树状态、展开状态、搜索状态     |
| **objects**   | 9 个（v\_scroll、cursor、search、expanded、bound\_instance 等） | 中（光标移动、展开/折叠、绑定实例/连接） | \~980 行 | ✅ **需要独立 TEA** — 状态复杂、独立光标/滚动/展开、绑定状态 |

### Feature 6: `instance_workspace` 评估

| 子模块             | 状态字段                                           | 交互复杂度                 | 代码量        | 评估结果                                |
| --------------- | ---------------------------------------------- | --------------------- | ---------- | ----------------------------------- |
| **overview**    | 6+ 个（status、form、test\_marks、cursor 等）         | 低-中（刷新、选择字段、删除注册）     | \~800 行渲染  | ✅ **需要独立 TEA** — 独立展示状态、选中字段、滚动状态   |
| **connections** | 8+ 个（form、test\_marks、pending\_d\_at、cursor 等） | 高（添加/编辑/删除/测试连接、表单编辑） | \~1255 行交互 | ✅ **需要独立 TEA** — 复杂表单管理、异步连接测试、编辑模式 |

### Feature 7: `sql_workspace` 评估

| 子模块                        | 状态字段                                           | 交互复杂度                      | 代码量      | 评估结果        | 层级                |
| -------------------------- | ---------------------------------------------- | -------------------------- | -------- | ----------- | ----------------- |
| **editor**                 | 5+ 个（sql\_content、cursor、search、editable、undo） | 中（光标移动、搜索、可编辑性切换）          | \~500 行  | ✅ 需要独立 TEA  | sql\_tab 子模块      |
| **editor/context\_picker** | 6+ 个（catalog、schema、cursor、search、expanded）    | 高（catalog 导航、schema 选择、搜索） | \~1387 行 | ✅ 需要独立 TEA  | editor 子模块        |
| **editor/sql\_completion** | 8+ 个（items、cursor、visible、selected、context）    | 高（触发、选择、应用、render）         | \~6849 行 | ✅ 需要独立 TEA  | editor 子模块        |
| **results**                | 10+ 个（rows、selected、sort、page、scroll 等）        | 高（滚动、排序、分页、编辑、导出）          | \~6200 行 | ✅ 需要独立 TEA  | sql\_tab 子模块      |
| **results/detail**         | 4+ 个（row\_data、edit\_mode、edit\_buf、cursor）    | 中（查看、编辑、保存）                | \~530 行  | ✅ 需要独立 TEA  | results 子模块       |
| **history**                | 5+ 个（items、cursor、selected、detail\_scroll）     | 中（选择、删除、recall、滚动）         | \~1200 行 | ✅ 需要独立 TEA  | sql\_tab 子模块      |
| **history/detail**         | 1 个（detail\_scroll）                            | 极低（仅滚动）                    | \~493 行  | ❌ 不需要独立 TEA | 合并到 history       |

**调整说明**:

- `sql_tab` 作为 `editor`、`results`、`history` 的父 feature，位于 `sql_workspace` 之下，负责标签页（Tab）管理
- `context_picker` 和 `sql_completion` 作为 `editor` 的子模块，因为它们是编辑器的辅助功能
- `detail` 作为 `results` 的子模块，因为它是 Results 表格的查看/编辑功能
- `history/detail` 不需要独立 TEA，因为它只有 1 个状态字段（scroll），且是只读详情，没有编辑功能

### 评估总结

| Feature             | 子模块             | 评估结果  | 层级                        |
| ------------------- | --------------- | ----- | ------------------------- |
| discover            | engine          | ✅ 需要  | discover 顶层               |
| discover            | targets         | ✅ 需要  | discover 顶层               |
| discover            | results         | ✅ 需要  | discover 顶层               |
| explorer            | instances       | ✅ 需要  | explorer 顶层               |
| explorer            | objects         | ✅ 需要  | explorer 顶层               |
| instance\_workspace | overview        | ✅ 需要  | instance\_workspace 顶层    |
| instance\_workspace | connections     | ✅ 需要  | instance\_workspace 顶层    |
| sql\_workspace      | editor          | ✅ 需要  | sql\_workspace 顶层         |
| sql\_workspace      | results         | ✅ 需要  | sql\_workspace 顶层         |
| sql\_workspace      | history         | ✅ 需要  | sql\_workspace 顶层         |
| editor              | context\_picker | ✅ 需要  | editor 子模块                |
| editor              | sql\_completion | ✅ 需要  | editor 子模块                |
| results             | detail          | ✅ 需要  | results 子模块               |
| history             | detail          | ❌ 不需要 | 合并到 history（仅滚动，无编辑）      |

**总计**: 14 个独立 TEA 循环（含 discover/engine，不含 history/detail 合并）

### 最终 Feature 结构图（更新）

```
features/
├── discover/             # Feature 3
│   ├── engine/          # ✅ 完整 TEA
│   ├── targets/         # ✅ 完整 TEA
│   └── results/         # ✅ 完整 TEA
├── explorer/             # Feature 5
│   └── instances/       # ✅ 完整 TEA
│   └── objects/         # ✅ 完整 TEA
├── instance_workspace/   # Feature 6
│   └── overview/        # ✅ 完整 TEA
│   └── connections/     # ✅ 完整 TEA
└── sql_workspace/        # Feature 7
    └── sql_tab/         # editor/results/history 的父 feature（标签页管理）
        ├── editor/          # ✅ 完整 TEA
        │   ├── context_picker/   # ✅ 完整 TEA (editor 子模块)
        │   └── sql_completion/   # ✅ 完整 TEA (editor 子模块)
        ├── results/         # ✅ 完整 TEA
        │   └── detail/      # ✅ 完整 TEA (results 子模块)
        └── history/         # ✅ 完整 TEA（detail 合并在内部，作为 HistoryState 的字段）
```

**嵌套层级说明**:

- 第 1 层: Feature（discover, explorer, instance\_workspace, sql\_workspace）
- 第 2 层: 父 feature（sql\_tab）
- 第 3 层: 子模块（editor, results, history, targets 等）
- 第 4 层: 子模块的子模块（context\_picker, sql\_completion, detail）
- 无需独立 TEA 的功能直接合并到父级 State 中（如 history/detail 的滚动状态）

**对比**:

- `results/detail` vs `history/detail`: 前者需要独立 TEA（有编辑功能），后者不需要（仅滚动，只读）

### 无子模块的 Feature 说明

以下 Feature 结构简单，没有需要独立 TEA 循环评估的子模块：

#### Feature 1: `header`

| 评估项           | 说明                                             |
| ------------- | ---------------------------------------------- |
| **子模块数量**     | 0 个                                            |
| **评估结论**      | 无需子模块评估                                        |
| **原因**        | 结构简单，内部只有按钮列表和导航状态，不需要拆分子模块                    |
| **完整 TEA 循环** | ✅ Feature 本身有完整的 TEA 循环（msg/state/update/view） |

**HeaderState 结构**：

```rust
pub struct HeaderState {
    pub active_button: usize,      // 当前选中按钮
    pub status_text: String,       // 状态文本
    pub width: u16,                // 渲染宽度
}
```

#### Feature 2: `perf_monitor`

| 评估项           | 说明                                             |
| ------------- | ---------------------------------------------- |
| **子模块数量**     | 0 个                                            |
| **评估结论**      | 无需子模块评估                                        |
| **原因**        | 被动计算组件，不需要 Msg/Intent/Effect，由 app\_shell 每帧调用 |
| **完整 TEA 循环** | ❌ 特殊类型（被动计算，无 Msg，update 函数签名不同）               |

**PerfState 结构**：

```rust
pub struct PerfState {
    pub fps: f64,
    pub frame_time: Duration,
    pub memory_usage: u64,
}
```

**Perf\_monitor update 函数**（特殊签名）：

```rust
pub fn update(state: &PerfState, frame_instant: Instant) -> PerfState {
    // 被动计算，不需要 Msg
}
```

#### Feature 4: `global_footer`

| 评估项           | 说明                                   |
| ------------- | ------------------------------------ |
| **子模块数量**     | 0 个                                  |
| **评估结论**      | 无需子模块评估                              |
| **原因**        | 纯显示组件，不需要 Msg/Intent/Effect，只有一个渲染函数 |
| **完整 TEA 循环** | ❌ 特殊类型（纯显示，无 Msg/State，只有 view 函数）   |

**Global\_footer 渲染函数**（特殊签名）：

```rust
pub fn render(
    frame: &mut Frame, 
    area: Rect, 
    focus_zone: &FocusZone, 
    global_status: Option<&str>
) {
    // 纯渲染，不需要 State
}
```

### 所有 Feature 子模块评估完整总结

| Feature             | 子模块             | 评估结果  | 层级                     | 说明                        |
| ------------------- | --------------- | ----- | ---------------------- | ------------------------- |
| **header**          | —               | 无子模块  | —                      | 结构简单，无需拆分                 |
| **perf\_monitor**   | —               | 无子模块  | —                      | 被动计算，无需拆分                 |
| **global\_footer**  | —               | 无子模块  | —                      | 纯显示，无需拆分                  |
| discover            | engine          | ✅ 需要  | discover 顶层            | 引擎选择，作为扫描决策者，切换时产生 Intent 通知顶层 |
| discover            | targets         | ✅ 需要  | discover 顶层            | 复杂编辑交互、撤销/重做              |
| discover            | results         | ✅ 需要  | discover 顶层            | 异步扫描、独立光标、选中集             |
| explorer            | instances       | ✅ 需要  | explorer 顶层            | 独立的树状态、展开状态、搜索状态          |
| explorer            | objects         | ✅ 需要  | explorer 顶层            | 状态复杂、独立光标/滚动/展开           |
| instance\_workspace | overview        | ✅ 需要  | instance\_workspace 顶层 | 独立展示状态、选中字段、滚动状态          |
| instance\_workspace | connections     | ✅ 需要  | instance\_workspace 顶层 | 复杂表单管理、异步连接测试             |
| sql\_workspace      | editor          | ✅ 需要  | sql\_workspace 顶层      | 编辑器核心功能                   |
| sql\_workspace      | results         | ✅ 需要  | sql\_workspace 顶层      | 结果集管理、编辑功能                |
| sql\_workspace      | history         | ✅ 需要  | sql\_workspace 顶层      | 历史记录管理                    |
| editor              | context\_picker | ✅ 需要  | editor 子模块             | Catalog 导航、schema 选择      |
| editor              | sql\_completion | ✅ 需要  | editor 子模块             | 代码补全、智能提示                 |
| results             | detail          | ✅ 需要  | results 子模块            | 详情查看/编辑功能                 |
| history             | detail          | ❌ 不需要 | 合并到 history            | 仅滚动，无编辑                   |

**总计**:

- 7 个顶层 Feature
- 13 个独立 TEA 循环
- 3 个特殊类型（无子模块或无需 TEA 循环）
- 1 个合并项（history/detail 合并到 history）

### 三种 Feature 类型完整示例

| 类型               | Feature             | 子模块数 | Msg | Intent                                 | Effect                             |
| ---------------- | ------------------- | ---- | --- | -------------------------------------- | ---------------------------------- |
| **交互式 Feature**  | header              | 0    | ✅   | ✅ (OpenDiscoverModal)                  | ❌                                  |
| <br />           | discover            | 3    | ✅   | ✅ (CloseModal, NotifyInstancesChanged) | ✅ (StartScan, RegisterInstances)   |
| <br />           | explorer            | 2    | ✅   | ✅ (InstanceSelected, ObjectSelected)   | ✅ (LoadInstances, LoadObjectsTree) |
| <br />           | instance\_workspace | 2    | ✅   | ✅ (RefreshExplorerInstances)           | ✅ (SaveConnection, TestConnection) |
| <br />           | sql\_workspace      | 3    | ✅   | ✅ (NotifyExplorerObjectChanged)        | ✅ (RunQuery, CommitResults)        |
| **被动计算 Feature** | perf\_monitor       | 0    | ❌   | ❌                                      | ❌                                  |
| **纯显示 Feature**  | global\_footer      | 0    | ❌   | ❌                                      | ❌                                  |

***

## 二·六、Msg/Intent/Effect 分析框架

### 分析目标

基于 `msg_effect_intent-instruction.md` 的定义，分析每个 Feature 或子模块需要：

- **Msg**: 组件自身的状态变更指令（内部闭环）
- **Intent**: 跨 Feature 协作请求（由全局协调器处理）
- **Effect**: 异步或外部交互（由全局执行器运行）

### 决策速查表

| 场景                  | 应该返回什么？                     | 示例                                                                   |
| ------------------- | --------------------------- | -------------------------------------------------------------------- |
| 用户按键，更新组件内部状态       | **Msg**                     | `HeaderMsg::NavigateLeft` → `header.button -= 1`                     |
| 组件需要执行数据库查询         | **Effect**                  | `DiscoverEffect::ScanTargets { targets }`                            |
| 组件需要切换到另一个全局面板      | **Intent**                  | `HeaderIntent::OpenDiscoverModal`                                    |
| 异步结果返回后更新状态         | **Msg**（异步结果先变为 Msg）        | `AppMsg::ScanResult { items }` → `DiscoverMsg::ScanResult { items }` |
| 组件完成操作后通知其他 Feature | 异步结果 → **Msg** → **Intent** | `Msg::QueryResult(data)` → `Intent::NotifyDataChanged`               |
| 被动计算（如 FPS）         | 不需要 Msg，由 app\_shell 每帧调用   | `perf_monitor::update(state, frame_instant)`                         |
| 纯显示（如 Footer）       | 不需要任何 Msg/Intent/Effect     | `global_footer::render(frame, area, focus_zone, status)`             |

### 三种 Feature 类型

| 类型               | Msg | Intent | Effect | 示例                         |
| ---------------- | --- | ------ | ------ | -------------------------- |
| **交互式 Feature**  | ✅ 有 | 可能有    | 可能有    | header, discover, explorer |
| **被动计算 Feature** | ❌ 无 | ❌ 无    | ❌ 无    | perf\_monitor              |
| **纯显示 Feature**  | ❌ 无 | ❌ 无    | ❌ 无    | global\_footer             |

### 当前三个简单 Feature 分析结果

| Feature            | Msg            | Intent                               | Effect | update 返回类型                  |
| ------------------ | -------------- | ------------------------------------ | ------ | ---------------------------- |
| **header**         | ✅ 有（HeaderMsg） | ✅ 有（HeaderIntent::OpenDiscoverModal） | ❌ 无    | `(State, Vec<HeaderIntent>)` |
| **perf\_monitor**  | ❌ 无（被动计算）      | ❌ 无                                  | ❌ 无    | `PerfState`（被动更新）            |
| **global\_footer** | ❌ 无（纯显示）       | ❌ 无                                  | ❌ 无    | N/A（纯渲染函数）                   |

### 返回值约定

**交互式 Feature**:

```rust
// 基础 TEA
pub fn update(msg: FeatureMsg, state: &FeatureState) -> FeatureState

// 有跨 Feature 通信
pub fn update(msg: FeatureMsg, state: &FeatureState) -> (FeatureState, Vec<FeatureIntent>)

// 有异步操作
pub fn update(msg: FeatureMsg, state: &FeatureState) -> (FeatureState, Vec<FeatureEffect>)

// 两者都有
pub fn update(msg: FeatureMsg, state: &FeatureState) -> (FeatureState, Vec<FeatureIntent>, Vec<FeatureEffect>)
```

**被动计算 Feature**:

```rust
// 每帧由 app_shell 调用
pub fn update(state: &PerfState, frame_instant: Instant) -> PerfState
```

**纯显示 Feature**:

```rust
// 由 app_shell 调用
pub fn render(frame: &mut Frame, area: Rect, params: &Params)
```

### EffectExecutor 和 IntentRouter 的位置

#### 设计原则

| 原则                      | 说明                                             |
| ----------------------- | ---------------------------------------------- |
| **EffectExecutor 全局唯一** | 所有 Feature 的 Effect 都由 `app_shell` 统一执行        |
| **IntentRouter 全局唯一**   | 所有 Feature 的 Intent 都由 `app_shell` 统一路由        |
| **Feature 只产生，不执行**     | Feature 只产生 Intent/Effect，不负责执行它们              |
| **父 Feature 只冒泡，不执行**   | 嵌套通信中，父 Feature 只负责冒泡子 Feature 的 Intent/Effect |

#### 职责划分

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              Feature 的职责（产生）                          │
└─────────────────────────────────────────────────────────────────────────────┘

✅ 处理 Msg → 更新 State
✅ 产生 Intent（跨 Feature 请求）
✅ 产生 Effect（副作用描述）
❌ 不执行 Effect
❌ 不路由 Intent
❌ 不管理异步任务

┌─────────────────────────────────────────────────────────────────────────────┐
│                              app_shell 的职责（执行）                        │
└─────────────────────────────────────────────────────────────────────────────┘

✅ 接收所有 Feature 的 Intent
✅ IntentRouter: 路由 Intent → 转换为目标 Feature 的 Msg
✅ 接收所有 Feature 的 Effect
✅ EffectExecutor: 执行 Effect（异步）
✅ 将 Effect 结果转换为 Msg 回传

┌─────────────────────────────────────────────────────────────────────────────┐
│                              父 Feature 的职责（嵌套转发）                    │
└─────────────────────────────────────────────────────────────────────────────┘

✅ 接收子 Feature 的 Intent/Effect
✅ 冒泡到 app_shell（或进一步处理）
❌ 不执行子 Feature 的 Effect
❌ 不路由子 Feature 的 Intent
```

#### 为什么不在 Feature 内部实现路由/执行？

| 原因       | 说明                                                                    |
| -------- | --------------------------------------------------------------------- |
| **单一职责** | `app_shell` 是顶层协调者，负责跨 Feature 通信；Feature 只需要产生 Intent/Effect，不需要自己执行 |
| **解耦性**  | Feature 不应该关心 Intent 如何被路由、Effect 如何被执行                               |
| **统一性**  | 所有 Effect 执行走同一个通道，所有 Intent 路由走同一个规则，便于调试和监控                         |
| **可测试性** | Feature 的 `update()` 是纯函数，只需测试 Msg → State/Intent/Effect 的映射          |

#### 嵌套通信中的冒泡示例

```rust
// explorer/update.rs
// 纯函数式按值 update：用 std::mem::take 只 move 出被改的子状态更新后放回，
// 其余字段原样保留，避免整棵深克隆。
pub fn update(msg: ExplorerMessage, mut state: ExplorerState)
    -> (ExplorerState, Vec<ExplorerIntent>, Vec<ExplorerEffect>) {
    
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    
    match msg {
        ExplorerMessage::Instances(m) => {
            // 0. 解包子模块消息
            let instances::msg::InstancesMsg::Message(inner) = m;
            
            // 1. 父 Feature 直接调用子 Feature 的 update()
            let instances_state = std::mem::take(&mut state.instances);
            let (s, child_intents, child_effects) = instances::update(inner, instances_state);
            
            state.instances = s;
            
            // 2. 冒泡：收集子 Feature 的 Intent/Effect
            //    这是"转发"，不是"路由"
            intents.extend(child_intents);  // 子 Feature 的 Intent 冒泡到父 Feature
            effects.extend(child_effects);  // 子 Feature 的 Effect 冒泡到父 Feature
        }
        // ...
    }
    
    // 3. 最终冒泡到 app_shell 处理
    (state, intents, effects)
}
```

**注意**：冒泡不是真正的路由。真正的路由发生在 `app_shell`：

```
Feature.update() → (State, Vec<Intent>, Vec<Effect>)
    ↓ 冒泡
app_shell 收集所有 Intent/Effect
    ↓ 真正的路由/执行
IntentRouter: Intent → 目标 Feature 的 Msg
EffectExecutor: Effect → 异步执行 → 结果 Msg
```

### 所有 Feature 的 Msg/Intent/Effect 分析导航

| Feature                 | 分析位置                                                                         | 说明                                |
| ----------------------- | ---------------------------------------------------------------------------- | --------------------------------- |
| **header**              | 本节（二·六）或 [Feature 1 详细定义](#feature-1-header--顶部导航栏)                          | 三个简单 Feature 在此节统一分析              |
| **perf\_monitor**       | 本节（二·六）或 [Feature 2 详细定义](#feature-2-perf_monitor--性能监控)                     | 三个简单 Feature 在此节统一分析              |
| **global\_footer**      | 本节（二·六）或 [Feature 4 详细定义](#feature-4-global_footer--底部全局快捷键提示栏)              | 三个简单 Feature 在此节统一分析              |
| **discover**            | [Feature 3 详细定义](#feature-3-discover--实例发现注册) 内的 Intent/Effect 结构            | 含 targets、results 子模块分析           |
| **explorer**            | [Feature 5 详细定义](#feature-5-explorer--连接树导航) 内的 Intent/Effect 结构             | 含 instances、objects 子模块分析         |
| **instance\_workspace** | [Feature 6 详细定义](#feature-6-instance_workspace--实例详情工作台) 内的 Intent/Effect 结构 | 含 overview、connections 子模块分析      |
| **sql\_workspace**      | [Feature 7 详细定义](#feature-7-sql_workspace--sql-工作台) 内的 Intent/Effect 结构      | 含 editor、results、history 及嵌套子模块分析 |

**快速跳转**：

- [Feature 3: discover 分析](#feature-3-discover--实例发现注册)
- [Feature 5: explorer 分析](#feature-5-explorer--连接树导航)
- [Feature 6: instance\_workspace 分析](#feature-6-instance_workspace--实例详情工作台)
- [Feature 7: sql\_workspace 分析](#feature-7-sql_workspace--sql-工作台)

### 跨 Feature 通信汇总

| 源 Feature               | 目标 Feature          | Intent                         | 触发场景                 |
| ----------------------- | ------------------- | ------------------------------ | -------------------- |
| **header**              | discover            | `OpenDiscoverModal`            | 用户点击按钮或按快捷键          |
| **discover**            | explorer            | `NotifyInstancesChanged`       | 注册实例后通知刷新            |
| **explorer**            | instance\_workspace | `InstanceSelected`             | 用户选择实例               |
| **explorer**            | instance\_workspace | `RequestAddConnection`         | 快捷键 'a' 添加连接         |
| **explorer**            | instance\_workspace | `RequestEditConnection`        | 快捷键 'e' 编辑连接         |
| **explorer**            | sql\_workspace      | `ObjectSelected`               | 用户选择数据库对象            |
| **explorer**            | sql\_workspace      | `ContextChanged`               | 用户切换数据库/Schema       |
| **instance\_workspace** | explorer            | `RefreshExplorerInstances`     | 保存/删除连接后刷新           |
| **instance\_workspace** | explorer            | `RefreshExplorerConnections`   | 连接变更后刷新              |
| **sql\_workspace**      | explorer            | `NotifyExplorerObjectChanged`  | 对象编辑完成后刷新            |
| **sql\_workspace**      | explorer            | `NotifyExplorerContextChanged` | Context Picker 变更后通知 |

### Effect 执行场景汇总

| Feature                 | Effect                  | 触发场景      | 异步操作           |
| ----------------------- | ----------------------- | --------- | -------------- |
| **discover**            | `StartScan`             | 用户点击扫描    | 异步网络扫描         |
| **discover**            | `CancelScan`            | 用户取消扫描    | 停止异步任务         |
| **discover**            | `RegisterInstances`     | 用户注册实例    | 持久化存储写入        |
| **explorer**            | `LoadInstances`         | 初始化/刷新    | 从持久化存储读取       |
| **explorer**            | `LoadObjectsTree`       | 展开对象树     | 从数据库获取元数据      |
| **explorer**            | `DeleteInstance`        | 用户删除实例    | 持久化存储删除        |
| **instance\_workspace** | `SaveConnection`        | 保存新增/编辑连接 | 持久化存储写入        |
| **instance\_workspace** | `TestConnection`        | 测试连接      | 异步 ping 数据库    |
| **instance\_workspace** | `DeleteConnection`      | 删除连接      | 持久化存储删除        |
| **sql\_workspace**      | `RunQuery`              | 执行 SQL 查询 | 异步数据库查询        |
| **sql\_workspace**      | `LoadHistory`           | 加载历史记录    | 从持久化存储读取       |
| **sql\_workspace**      | `CommitResults`         | 提交编辑结果    | 异步事务提交         |
| **sql\_workspace**      | `LoadContextPickerData` | 加载元数据     | 从数据库获取 catalog |
| **sql\_workspace**      | `LoadCompletionItems`   | 加载补全项     | 从数据库获取提示项      |

***

## 二·七、全局 Msg/Intent/Effect 分析总结

### 所有 Feature 分析结果汇总

| Feature                                   | Msg       | Intent                                                                      | Effect                                                                    | update 返回类型                             |
| :---------------------------------------- | :-------- | :-------------------------------------------------------------------------- | :------------------------------------------------------------------------ | :-------------------------------------- |
| **header**                                | ✅ 有       | ✅ 有（OpenDiscoverModal）                                                      | ❌ 无                                                                       | `(State, Vec<Intent>)`                  |
| **perf\_monitor**                         | ❌ 无（被动计算） | ❌ 无                                                                         | ❌ 无                                                                       | `PerfState`（被动更新）                       |
| **global\_footer**                        | ❌ 无（纯显示）  | ❌ 无                                                                         | ❌ 无                                                                       | N/A（纯渲染函数）                              |
| **discover**                              | ✅ 有       | ✅ 有（CloseModal, NotifyInstancesChanged）                                     | ✅ 有（StartScan, CancelScan, RegisterInstances）                             | `(State, Vec<Intent>, Vec<Effect>)`     |
| **discover/engine**                       | ✅ 有       | ✅ 有（EngineSelected → 通过父级）                                                 | ❌ 无（目前）                                                                   | `(State, Vec<Intent>, Vec<Effect>)`     |
| **discover/targets**                      | ✅ 有       | ❌ 无（目前）                                                                     | ❌ 无（目前）                                                                   | `(State, Vec<Intent>, Vec<Effect>)`（预留） |
| **discover/results**                      | ✅ 有       | ✅ 有（通过父级）                                                                   | ✅ 有（RegisterInstances）                                                    | `(State, Vec<Intent>, Vec<Effect>)`     |
| **explorer**                              | ✅ 有       | ✅ 有（InstanceSelected, ObjectSelected, ContextChanged, RequestOpenWorkspace） | ✅ 有（LoadInstances, RefreshInstances, DeleteInstance 等）                    | `(State, Vec<Intent>, Vec<Effect>)`     |
| **explorer/instances**                    | ✅ 有       | ✅ 有（SelectInstance → 通过父级）                                                  | ✅ 有（LoadConnections, RefreshInstance）                                     | `(State, Vec<Intent>, Vec<Effect>)`     |
| **explorer/objects**                      | ✅ 有       | ✅ 有（SelectObject, SelectDatabase, SelectSchema → 通过父级）                      | ✅ 有（LoadObjects, RefreshCurrent）                                          | `(State, Vec<Intent>, Vec<Effect>)`     |
| **instance\_workspace**                   | ✅ 有       | ✅ 有（RefreshExplorerInstances, UnregisterInstance, CloseWorkspace）           | ✅ 有（LoadInstanceData, SaveConnection, DeleteConnection, TestConnection 等） | `(State, Vec<Intent>, Vec<Effect>)`     |
| **instance\_workspace/overview**          | ✅ 有       | ✅ 有（Unregister → 通过父级）                                                      | ✅ 有（Refresh, Unregister）                                                  | `(State, Vec<Intent>, Vec<Effect>)`     |
| **instance\_workspace/connections**       | ✅ 有       | ✅ 有（保存/删除/测试完成后通知 Explorer → 通过父级）                                          | ✅ 有（SaveConnection, DeleteConnection, TestConnection）                     | `(State, Vec<Intent>, Vec<Effect>)`     |
| **sql\_workspace**                        | ✅ 有       | ✅ 有（NotifyExplorerObjectChanged, NotifyExplorerContextChanged）              | ✅ 有（RunQuery, StopQuery, CommitResults, LoadHistory 等）                    | `(State, Vec<Intent>, Vec<Effect>)`     |
| **sql\_workspace/sql\_tab/editor**                 | ✅ 有       | ✅ 有（UpdateContext → 通过父级）                                                   | ✅ 有（加载 ContextPicker 元数据、加载 Completion 项）                                 | `(State, Vec<Intent>, Vec<Effect>)`     |
| **sql\_workspace/sql\_tab/editor/context\_picker** | ✅ 有       | ✅ 有（SetContext → 通过父级）                                                      | ✅ 有（RefreshData）                                                          | `(State, Vec<Intent>, Vec<Effect>)`     |
| **sql\_workspace/sql\_tab/editor/sql\_completion** | ✅ 有       | ❌ 无                                                                         | ✅ 有（LoadCompletions）                                                      | `(State, Vec<Effect>)`                  |
| **sql\_workspace/sql\_tab/results**                | ✅ 有       | ✅ 有（CommitChanges → 通过父级）                                                   | ✅ 有（CommitChanges, RefreshPage）                                           | `(State, Vec<Intent>, Vec<Effect>)`     |
| **sql\_workspace/sql\_tab/results/detail**         | ✅ 有       | ❌ 无                                                                         | ✅ 有（SaveField, AddRow, DeleteRow）                                         | `(State, Vec<Effect>)`                  |
| **sql\_workspace/sql\_tab/history**                | ✅ 有       | ❌ 无                                                                         | ✅ 有（DeleteHistory, RecallHistory, LoadHistory, SaveHistoryEntry）          | `(State, Vec<Effect>)`                  |

### 跨 Feature 通信关系图

```
header ──Intent──→ app_shell ──路由──→ discover

discover ──Intent──→ app_shell ──路由──→ explorer (刷新实例列表)

explorer ──Intent──→ app_shell ──路由──→ sql_workspace
  ├─ InstanceSelected → SqlWorkspaceMsg::SetInstance
  ├─ ObjectSelected → SqlWorkspaceMsg::OpenObject
  └─ ContextChanged → SqlWorkspaceMsg::SetContext

instance_workspace ──Intent──→ app_shell ──路由──→ explorer
  ├─ RefreshExplorerInstances → ExplorerMsg::RefreshInstances
  └─ RefreshExplorerConnections → ExplorerMsg::LoadConnections

sql_workspace ──Intent──→ app_shell ──路由──→ explorer
  ├─ NotifyExplorerObjectChanged → ExplorerMsg::RefreshObjects
  └─ NotifyExplorerContextChanged → ExplorerMsg::LoadContextPickerData
```

### Effect 执行架构

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                                    app_shell                                  │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                         Message Router                              │    │
│  │                                                                     │    │
│  │  1. 接收 Feature 产生的 (Intent, Effect)                            │    │
│  │  2. 将 Intent 路由到目标 Feature（包装为 Msg）                        │    │
│  │  3. 将 Effect 提交给 EffectExecutor 执行                            │    │
│  │  4. Effect 执行完成后，将结果包装为 Msg 路由回源 Feature               │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                         EffectExecutor                              │    │
│  │                                                                     │    │
│  │  执行各种异步操作：                                                   │    │
│  │  - 数据库查询 (RunQuery)                                             │    │
│  │  - 持久化存储 (SaveConnection, LoadInstances)                        │    │
│  │  - 异步扫描 (StartScan)                                             │    │
│  │  - 提交事务 (CommitResults)                                         │    │
│  │  - 加载元数据 (LoadContextPickerData)                                │    │
│  │                                                                     │    │
│  │  执行完成后 → 产生 Msg → 路由回源 Feature                            │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                         IntentRouter                                │    │
│  │                                                                     │    │
│  │  处理跨 Feature 通信：                                                │    │
│  │  - 解析 Intent 目标                                                  │    │
│  │  - 将 Intent 包装为目标 Feature 的 Msg                                │    │
│  │  - 路由到目标 Feature                                                │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 统一的 Effect 执行流程

```
1. Feature.update(msg, state) → (new_state, intents, effects)
   ↓
2. app_shell 收集所有 intents 和 effects
   ↓
3. 处理 intents：
   - 解析目标 Feature
   - 包装为目标 Feature 的 Msg
   - 通过 Message Router 路由
   ↓
4. 处理 effects：
   - 提交给 EffectExecutor
   - EffectExecutor 异步执行
   - 执行完成后产生 Msg
   - 通过 Message Router 路由回源 Feature
   ↓
5. 源 Feature 收到 Msg，执行 update
   ↓
6. 循环（回到步骤 1）
```

### 三大 Feature 类型总结

| 类型               | 特征                        | 代表                                                              |
| :--------------- | :------------------------ | :-------------------------------------------------------------- |
| **交互式 Feature**  | 有 Msg，可能有 Intent 和 Effect | header, discover, explorer, instance\_workspace, sql\_workspace |
| **被动计算 Feature** | 无 Msg，由 app\_shell 每帧调用   | perf\_monitor                                                   |
| **纯显示 Feature**  | 无 Msg、Intent、Effect，纯渲染函数 | global\_footer                                                  |

***

## 三、Feature 详细定义

### Feature 1: `header` — 顶部导航栏

**职责**: 应用标题、连接状态、快捷按钮的显示和交互；Header 是独立的焦点区域，可以接收键盘和鼠标输入

**TEA 结构**:

| 文件          | 类型   | 内容                                                 | 当前来源                                                        |
| ----------- | ---- | -------------------------------------------------- | ----------------------------------------------------------- |
| `mod.rs`    | —    | 模块声明 + 公开 API                                      | 新建                                                          |
| `msg.rs`    | —    | `HeaderMsg` 枚举                                     | 从 `app.rs` 中 Header 相关交互提取                                  |
| `intent.rs` | —    | `HeaderIntent` 枚举                                  | 从 Header 按钮激活逻辑提取                                           |
| `state.rs`  | —    | `HeaderState`                                      | 来自 `components/header.rs:HeaderView` + `app.rs` 中 Header 状态 |
| `update.rs` | —    | `update(msg, state) -> (State, Vec<HeaderIntent>)` | 从 `app.rs` 中 Header 处理逻辑提取                                  |
| `view.rs`   | 类型 1 | Header 渲染函数                                        | 从 `ui.rs` 中 Header 渲染提取                                     |

**当前文件迁移映射**:

| 当前文件                   | 目标位置                          | 说明              |
| ---------------------- | ----------------------------- | --------------- |
| `components/header.rs` | `header/state.rs`             | HeaderView 状态迁移 |
| `ui.rs` → Header 渲染    | `header/view.rs`              | Header 绘制逻辑     |
| `app.rs` → Header 交互   | `header/update.rs` + `msg.rs` | Header 消息处理     |
| `app.rs` → Header 焦点管理 | `header/update.rs`            | Header 焦点逻辑     |

**状态归属**:

- `HeaderView.button`（按钮光标位置）
- `HeaderView.status`（状态文本）
- Header 按钮矩形区域（`header_button_rects`）
- Header 区域矩形（`header_rect`）

**Msg 结构**:

```rust
// header/msg.rs
pub enum HeaderMsg {
    NavigateLeft,
    NavigateRight,
    ClickButton { index: usize },
    SetStatus { text: String },
}
```

**Intent 结构**:

```rust
// header/intent.rs
pub enum HeaderIntent {
    OpenDiscoverModal,   // 请求 app_shell 打开 Discover Modal
}
```

**Update 返回类型**:

```rust
// header/update.rs
pub fn update(msg: HeaderMsg, state: &HeaderState) -> (HeaderState, Vec<HeaderIntent>) {
    let mut new_state = state.clone();
    let mut intents = Vec::new();
    
    match msg {
        HeaderMsg::NavigateLeft => {
            new_state.button = new_state.button.saturating_sub(1);
        }
        HeaderMsg::NavigateRight => {
            new_state.button = (new_state.button + 1).min(max_buttons);
        }
        HeaderMsg::ClickButton { index } => {
            new_state.button = index;
            if index == 0 {
                intents.push(HeaderIntent::OpenDiscoverModal);
            }
        }
        HeaderMsg::SetStatus { text } => {
            new_state.status = text;
        }
    }
    
    (new_state, intents)
}
```

**说明**: Header 的 update 返回 `Vec<HeaderIntent>`，`app_shell` 在收到后会处理 `OpenDiscoverModal` 意图，打开 Discover Modal。

***

### Feature 2: `perf_monitor` — 性能监控

**职责**: 显示应用运行时的性能指标（帧率、冗余重绘率等）；独立的调试功能，与业务逻辑无关

**TEA 结构**:

| 文件          | 类型   | 内容                                      | 当前来源                             |
| ----------- | ---- | --------------------------------------- | -------------------------------- |
| `mod.rs`    | —    | 模块声明 + 公开 API                           | 新建                               |
| `state.rs`  | —    | `PerfState`                             | 从 `app.rs` 中 fps/redundancy 字段提取 |
| `update.rs` | —    | `update(frame_instant, state) -> State` | 被动计算，不需要 Msg                     |
| `view.rs`   | 类型 1 | 性能指标渲染函数                                | 从 `ui.rs` 中 fps 渲染提取             |

**特殊说明**: Perf Monitor 是**被动显示 Feature**，不需要完整的 Msg/Intent/Effect 循环。它的状态更新由 `app_shell` 在每帧自动调用，而不是通过消息触发。

**当前文件迁移映射**:

| 当前文件                                      | 目标位置                     | 说明           |
| ----------------------------------------- | ------------------------ | ------------ |
| `app.rs:fps` + `redundancy_rate`          | `perf_monitor/state.rs`  | 性能状态         |
| `lib.rs` → fps 计算逻辑                       | `perf_monitor/update.rs` | 帧率更新逻辑       |
| `ui.rs` → fps 渲染 (draw\_global\_footer 中) | `perf_monitor/view.rs`   | fps/waste 渲染 |
| `app.rs:update_fps()` 方法                  | `perf_monitor/update.rs` | fps 更新函数     |

**状态归属**:

- `fps: f64` — 当前帧率
- `redundancy_rate: f64` — 冗余重绘率（0-1）
- `last_frame_instant: Option<Instant>` — 上次绘制时间（fps 计算用）

**分析结果**:

- ❌ **不需要 Msg**: 被动计算，不需要消息驱动
- ❌ **不需要 Intent**: 无跨 Feature 通信
- ❌ **不需要 Effect**: 无异步操作

**Update 函数签名**:

```rust
// perf_monitor/update.rs
// 被动更新：由 app_shell 在每帧自动调用
pub fn update(state: &PerfState, frame_instant: Instant) -> PerfState {
    let mut new_state = state.clone();
    
    // 计算 FPS
    if let Some(last) = new_state.last_frame_instant {
        let dt = frame_instant.duration_since(last).as_secs_f64();
        if dt > 0.0 {
            let inst = 1.0 / dt;
            new_state.fps = if new_state.fps <= 0.0 {
                inst
            } else {
                new_state.fps * 0.8 + inst * 0.2
            };
        }
    }
    new_state.last_frame_instant = Some(frame_instant);
    
    // 计算 redundancy_rate (由 app_shell 提供)
    new_state.redundancy_rate = compute_redundancy_rate();
    
    new_state
}
```

***

### Feature 3: `discover` — 实例发现/注册

**职责**: Modal 窗口，提供实例扫描（Engine 选择 + Target 编辑 + Results 列表）和注册功能

**TEA 结构**:

| 文件          | 类型   | 内容                                                                        | 当前来源                           |
| ----------- | ---- | ------------------------------------------------------------------------- | ------------------------------ |
| `mod.rs`    | —    | 模块声明 + 公开 API                                                             | 新建                             |
| `msg.rs`    | —    | `DiscoverMsg`（嵌套 EngineMsg + TargetsMsg + ResultsMsg + 异步结果）              | 新建                             |
| `intent.rs` | —    | `DiscoverIntent`（跨 Feature 通信）                                            | 新建                             |
| `effect.rs` | —    | `DiscoverEffect`（异步操作）                                                    | 新建                             |
| `state.rs`  | —    | `DiscoverState`（聚合三个子模块 state）                                            | 聚合 EngineState + TargetsState + ResultsState |
| `update.rs` | —    | `update(msg, state) -> (State, Vec<DiscoverIntent>, Vec<DiscoverEffect>)` | 父级调度逻辑                         |
| `view.rs`   | —    | 主渲染函数（包含 Engine 渲染）                                                       | 从 `discover_modal/draw.rs` 提取  |
| `engine/`   | 类型 1 | Engine 子模块（独立 TEA，作为扫描决策者）                                              | 见下表                            |
| `targets/`  | 类型 1 | Targets 子模块（独立 TEA）                                                       | 见下表                            |
| `results/`  | 类型 1 | Scan Results 子模块（独立 TEA）                                                  | 见下表                            |

**DiscoverMsg 结构**:

```rust
// discover/msg.rs
pub enum DiscoverMsg {
    // 子模块消息
    EngineMsg(EngineMsg),
    TargetsMsg(TargetsMsg),
    ResultsMsg(ScanResultsMsg),
    
    // 内部状态变更
    FocusPane { pane: DiscoverFocus },
    ToggleUnregisteredOnly,
    CloseConfirm,
    
    // 异步结果消息（来自 Effect 执行后的反馈）
    ScanProgress { current: usize, total: usize },
    ScanComplete { items: Vec<DiscoveredInstance> },
    ScanError { error: String },
    RegisterComplete { success_count: usize },
    RegisterError { error: String },
}
```

> **Engine 子模块**：`SelectEngine(Engine)` 已移入 `engine/msg.rs`（`EngineMsg`），切换引擎由 `engine` 子模块处理，并通过 `EngineIntent`（如 `EngineSelected`）通知 discover 顶层，供扫描流程读取当前引擎。


**DiscoverIntent 结构**:

```rust
// discover/intent.rs
pub enum DiscoverIntent {
    CloseModal,                    // 请求 app_shell 关闭 Modal
    NotifyInstancesChanged,        // 通知 Explorer 刷新实例列表
}
```

**DiscoverEffect 结构**:

```rust
// discover/effect.rs
pub enum DiscoverEffect {
    StartScan {                    // 启动异步扫描
        targets: Vec<DiscoveryTarget>,
        engine: Engine,
    },
    CancelScan,                    // 取消正在进行的扫描
    RegisterInstances {            // 批量注册实例
        instances: Vec<DiscoveredInstance>,
    },
}
```

**DiscoverState 结构**:

```rust
// discover/state.rs
pub struct DiscoverState {
    pub engine: EngineState,            // 类型 1 子模块（当前引擎选择）
    pub targets: TargetsState,          // 类型 1 子模块
    pub results: ScanResultsState,      // 类型 1 子模块
    pub focus: DiscoverFocus,
    pub close_confirm: bool,
    pub scan_progress: ScanProgress,
}
```

> **EngineState**（`engine/state.rs`）：`Engine` 是独立 TEA 子模块，持有当前选中的引擎。`StartScan` 时 discover 顶层从 `state.engine.engine`（或 `EngineState::selected()`）读取当前引擎，与 `state.targets` 一起构造 `DiscoverEffect::StartScan`。


**Update 返回类型**:

```rust
// discover/update.rs
pub fn update(
    msg: DiscoverMsg, 
    state: &DiscoverState
) -> (DiscoverState, Vec<DiscoverIntent>, Vec<DiscoverEffect>) {
    let mut new_state = state.clone();
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    
    match msg {
        DiscoverMsg::TargetsMsg(targets_msg) => {
            // 解包并转发给子模块
            let (new_targets, child_intents, child_effects) = 
                targets::update(targets_msg, &state.targets);
            new_state.targets = new_targets;
            intents.extend(child_intents);
            effects.extend(child_effects);
        }
        DiscoverMsg::ResultsMsg(results_msg) => {
            // 解包并转发给子模块
            let (new_results, child_intents, child_effects) = 
                results::update(results_msg, &state.results);
            new_state.results = new_results;
            intents.extend(child_intents);
            effects.extend(child_effects);
        }
        DiscoverMsg::StartScan => {
            // 从 state.targets 获取验证后的 targets
            let validated_targets = validate_targets(&state.targets);
            // 从 engine 子模块读取当前选中的引擎
            let engine = state.engine.selected();
            if !validated_targets.is_empty() {
                effects.push(DiscoverEffect::StartScan {
                    targets: validated_targets,
                    engine,
                });
            }
        }
        DiscoverMsg::StopScan => {
            effects.push(DiscoverEffect::CancelScan);
        }
        DiscoverMsg::RegisterInstance => {
            let selected = state.results.get_selected();
            if !selected.is_empty() {
                effects.push(DiscoverEffect::RegisterInstances {
                    instances: selected,
                });
            }
        }
        DiscoverMsg::ScanComplete { items } => {
            // 异步结果 → 更新 state
            new_state.results.items = items;
        }
        DiscoverMsg::RegisterComplete { success_count } => {
            // 异步结果 → 通知其他 Feature
            if success_count > 0 {
                intents.push(DiscoverIntent::NotifyInstancesChanged);
            }
        }
        // ... 其他消息处理
    }
    
    (new_state, intents, effects)
}
```

**说明**: Discover 的 update 返回 `(State, Vec<Intent>, Vec<Effect>)`：

- Effect 由 `app_shell` 执行（异步扫描、注册等）
- Effect 执行完成后，结果会包装为 DiscoverMsg（如 `ScanComplete`、`RegisterComplete`）
- Intent 由 `app_shell` 处理（关闭 Modal、通知其他 Feature 等）

***

**`targets/`** **子模块结构**:

| 文件          | 内容                                                                                             | 当前来源                                          |
| ----------- | ---------------------------------------------------------------------------------------------- | --------------------------------------------- |
| `mod.rs`    | 模块声明                                                                                           | 新建                                            |
| `msg.rs`    | `TargetsMsg`（AddTarget, RemoveTarget, EditTarget, PasteTargets 等）                              | 从 `discover_modal/draw.rs` + `interact.rs` 提取 |
| `state.rs`  | `TargetsState`（targets 列表、编辑模式、撤销栈等）                                                           | 从 `discover_modal/draw.rs` + `interact.rs`    |
| `update.rs` | `update(TargetsMsg, TargetsState) -> (TargetsState, Vec<DiscoverIntent>, Vec<DiscoverEffect>)` | 从 `discover_modal/interact.rs`                |
| `view.rs`   | `view(TargetsState)`                                                                           | 从 `discover_modal/draw.rs`                    |

**TargetsMsg 结构**:

```rust
// discover/targets/msg.rs
pub enum TargetsMsg {
    AddTarget { host: String, ports_spec: String },
    RemoveTarget { index: usize },
    EditField { index: usize, field: TargetCol, value: String },
    PasteTargets { text: String },
    Undo,
    Redo,
    CursorMove { direction: CursorDirection },
}
```

**Targets 分析结果**:

- ✅ **需要 Msg**: 所有编辑操作都是内部状态变更
- ❌ **不需要 Intent**: 所有操作都在 Discover Feature 内部
- ❌ **不需要 Effect**: 所有操作都是同步的（当前）

**Targets update 返回类型**: `(TargetsState, Vec<DiscoverIntent>, Vec<DiscoverEffect>)`（虽然目前没有返回值，但为未来扩展预留）

***

**`results/`** **子模块结构**:

| 文件          | 内容                                                                                                         | 当前来源                                          |
| ----------- | ---------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| `mod.rs`    | 模块声明                                                                                                       | 新建                                            |
| `msg.rs`    | `ScanResultsMsg`（SelectResult, ToggleSelection, ScrollResults 等）                                           | 从 `discover_modal/draw.rs` + `interact.rs` 提取 |
| `state.rs`  | `ScanResultsState`（扫描结果、选中项、扫描进度等）                                                                         | 从 `discover_modal/draw.rs` + `interact.rs`    |
| `update.rs` | `update(ScanResultsMsg, ScanResultsState) -> (ScanResultsState, Vec<DiscoverIntent>, Vec<DiscoverEffect>)` | 从 `discover_modal/interact.rs`                |
| `view.rs`   | `view(ScanResultsState)`                                                                                   | 从 `discover_modal/draw.rs`                    |

**ScanResultsMsg 结构**:

```rust
// discover/results/msg.rs
pub enum ScanResultsMsg {
    SelectResult { index: usize },
    ToggleSelection { index: usize },
    ToggleSelectAll,
    ToggleUnregisteredOnly,
    CursorMove { direction: CursorDirection },
    Scroll { delta: isize },
    RegisterSelected,  // ⚠️ 产生 Effect 和 Intent
}
```

**Results 分析结果**:

- ✅ **需要 Msg**: 选择、过滤、滚动等是内部状态变更
- ✅ **需要 Effect**: `RegisterSelected` 需要持久化存储
- ✅ **需要 Intent**: 注册完成后需要通知 Explorer 刷新

**Results update 返回类型**: `(ScanResultsState, Vec<DiscoverIntent>, Vec<DiscoverEffect>)`

**Results update 示例**:

```rust
// discover/results/update.rs
pub fn update(
    msg: ScanResultsMsg, 
    state: &ScanResultsState
) -> (ScanResultsState, Vec<DiscoverIntent>, Vec<DiscoverEffect>) {
    let mut new_state = state.clone();
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    
    match msg {
        ScanResultsMsg::RegisterSelected => {
            let selected = state.get_selected_instances();
            if !selected.is_empty() {
                effects.push(DiscoverEffect::RegisterInstances { instances: selected });
            }
        }
        // ... 其他消息处理
    }
    
    (new_state, intents, effects)
}
```

***

#### Discover Feature 分析总结

| Feature/子模块          | Msg | Intent                                  | Effect                                        | update 返回类型                             |
| -------------------- | --- | --------------------------------------- | --------------------------------------------- | --------------------------------------- |
| **discover**         | ✅ 有 | ✅ 有（CloseModal, NotifyInstancesChanged） | ✅ 有（StartScan, CancelScan, RegisterInstances） | `(State, Vec<Intent>, Vec<Effect>)`     |
| **discover/targets** | ✅ 有 | ❌ 无（目前）                                 | ❌ 无（目前）                                       | `(State, Vec<Intent>, Vec<Effect>)`（预留） |
| **discover/results** | ✅ 有 | ✅ 有（通过父级 Discover）                      | ✅ 有（RegisterInstances）                        | `(State, Vec<Intent>, Vec<Effect>)`     |

#### Effect 执行流程

```
1. 用户按下 "Start Scan"
   → DiscoverMsg::StartScan
   → Discover::update() 产生 DiscoverEffect::StartScan

2. app_shell 执行 Effect
   → 启动异步扫描任务
   → 定时发送 AppMsg::DiscoverMsg(ScanProgress/ScanComplete)

3. app_shell 将 Msg 路由回 Discover
   → DiscoverMsg::ScanComplete { items }
   → Discover::update() 更新 state

4. 用户选择实例并注册
   → ScanResultsMsg::RegisterSelected
   → Results::update() 产生 DiscoverEffect::RegisterInstances

5. app_shell 执行 Effect
   → 持久化存储
   → 发送 AppMsg::DiscoverMsg(RegisterComplete)

6. app_shell 将 Msg 路由回 Discover
   → DiscoverMsg::RegisterComplete { success_count }
   → Discover::update() 产生 DiscoverIntent::NotifyInstancesChanged

7. app_shell 处理 Intent
   → 通知 Explorer Feature 刷新
   → 路由 ExplorerMsg::RefreshInstances
```

**评估说明**:

- **Discover 主 Feature**: 5+ 个状态字段，包含 Engine 选择、Targets 编辑、Results 列表等多区域交互，且涉及异步扫描和注册操作，需要完整 TEA 循环 + Intent + Effect
- **Discover/Engine**: 作为扫描的决策者，持有当前引擎选择；切换引擎时产生 `EngineIntent` 通知 discover 顶层，供 `StartScan` 读取，需要独立 TEA
- **Discover/Targets**: 9 个状态字段，丰富的编辑交互（添加/删除/编辑/撤销/重做/粘贴/扫描），需要独立 TEA
- **Discover/Results**: 7 个状态字段，包含异步扫描结果处理和批量注册功能，需要独立 TEA

**当前文件迁移映射**:

| 当前文件                                        | 目标位置                                                                        | 说明                           |
| ------------------------------------------- | --------------------------------------------------------------------------- | ---------------------------- |
| `discover_modal/draw.rs`                    | `discover/view.rs` + `targets/view.rs` + `results/view.rs`                  | 拆分渲染逻辑                       |
| `discover_modal/interact.rs`                | `discover/update.rs` + `msg.rs` + `targets/update.rs` + `results/update.rs` | 交互逻辑 → update；动作枚举 → msg     |
| `discover_modal/mod.rs`                     | **删除**                                                                      | 被新 `discover/mod.rs` 替代      |
| `app.rs:DiscoverState`                      | `discover/state.rs`                                                         | 状态归属迁移                       |
| `app.rs:DiscoverScanUi`                     | `discover/state.rs`                                                         | 扫描状态迁移                       |
| `hints.rs` → `discover_*_footer_text`       | `discover/view.rs` + 子模块 `view.rs`                                          | Footer 文本归 feature 所有        |
| `components/modal.rs` (DiscoverTargetsView) | `discover/targets/state.rs`                                                 | Discover 专属 view state 归入子模块 |
| `components/modal.rs` 通用部分                  | `common/`                                                                   | 纯通用逻辑保留                      |

**状态归属**:

- `DiscoverState`（focus 切换、close\_confirm、scan\_progress）
- `EngineState`（当前引擎选择）
- `TargetsState`（targets 列表、编辑模式、撤销/重做栈、光标位置）
- `ScanResultsState`（扫描结果、选中项、过滤条件、光标位置）
- `discover_status`（反馈消息）
- `last_paste_content`

**分析结果**:

- ✅ **需要 Msg**: 丰富的内部交互（编辑、选择、滚动等）
- ✅ **需要 Intent**: CloseModal（关闭 Modal）、NotifyInstancesChanged（通知 Explorer 刷新）
- ✅ **需要 Effect**: StartScan（异步扫描）、CancelScan（取消扫描）、RegisterInstances（持久化注册）

***

### Feature 4: `global_footer` — 底部全局快捷键提示栏

**职责**: 显示当前焦点区域的快捷键提示和全局状态消息；Global Footer 是纯渲染 Feature，不接收独立的焦点输入

**TEA 结构**:

| 文件        | 类型   | 内容            | 当前来源            |
| --------- | ---- | ------------- | --------------- |
| `mod.rs`  | —    | 模块声明 + 公开 API | 新建              |
| `view.rs` | 类型 1 | Footer 渲染函数   | 从 `hints.rs` 提取 |

**特殊说明**: Global Footer 是**纯显示 Feature**，不需要完整的 Msg/Intent/Effect 循环。它只需要 View 逻辑，状态由其他 Feature 提供（通过参数传入）。

**当前文件迁移映射**:

| 当前文件                                     | 目标位置                    | 说明           |
| ---------------------------------------- | ----------------------- | ------------ |
| `hints.rs` → `global_footer_text()`      | `global_footer/view.rs` | 全局 Footer 渲染 |
| `hints.rs` → `key()`, `join()`, `keys()` | `global_footer/view.rs` | 辅助渲染函数       |
| `hints.rs` → 其他 feature footer           | 各 feature `view.rs`     | 按 feature 拆分 |

**状态归属**:

- 当前焦点区域标识（用于显示对应的快捷键提示）
- 全局状态消息（`global_status`）

**分析结果**:

- ❌ **不需要 Msg**: 纯显示，无交互
- ❌ **不需要 Intent**: 无跨 Feature 通信
- ❌ **不需要 Effect**: 无异步操作

**View 函数签名**:

```rust
// global_footer/view.rs
// 纯渲染：由 app_shell 在每帧调用
pub fn render(
    frame: &mut Frame,
    area: Rect,
    focus_zone: &FocusZone,
    global_status: Option<&str>,
) {
    // 根据当前焦点区域显示对应的快捷键提示
    // 不持有独立的 state，所有状态从参数获取
}
```

**说明**: Perf Monitor 的性能数据已迁移到 `perf_monitor` Feature，Global Footer 现在只负责显示快捷键提示和状态消息。

***

### Feature 5: `explorer` — 连接树导航

**职责**: 左栏连接树导航，包含实例树（Instances）和 Schema 对象浏览器（Objects）两个面板

**TEA 结构**:

| 文件           | 类型   | 内容                                                                        | 当前来源                             |
| ------------ | ---- | ------------------------------------------------------------------------- | -------------------------------- |
| `mod.rs`     | —    | 模块声明 + 公开 API                                                             | 新建                               |
| `msg.rs`     | —    | `ExplorerMsg`（嵌套 InstancesMsg + ObjectsMsg + 异步结果）                        | 新建                               |
| `intent.rs`  | —    | `ExplorerIntent`（跨 Feature 通信）                                            | 新建                               |
| `effect.rs`  | —    | `ExplorerEffect`（异步操作）                                                    | 新建                               |
| `state.rs`   | —    | `ExplorerState`（聚合子模块 state）                                              | 聚合 InstancesState + ObjectsState |
| `update.rs`  | —    | `update(msg, state) -> (State, Vec<ExplorerIntent>, Vec<ExplorerEffect>)` | 父级调度逻辑                           |
| `view.rs`    | —    | 主渲染函数                                                                     | 从 `ui.rs` 中 explorer 渲染提取        |
| `instances/` | 类型 1 | Instances 子模块（独立 TEA）                                                     | 见下表                              |
| `objects/`   | 类型 1 | Objects 子模块（独立 TEA）                                                       | 见下表                              |

**ExplorerMsg 结构**:

```rust
// explorer/msg.rs
pub enum ExplorerMsg {
    // 子模块消息
    InstancesMsg(InstancesMsg),
    ObjectsMsg(ObjectsMsg),
    
    // 内部状态变更
    FocusPane { pane: ExplorerPane },
    SearchInput { text: String },
    
    // 异步结果消息（来自 Effect 执行后的反馈）
    InstancesLoaded { instances: Vec<ManagedInstance> },
    InstanceConnectionsLoaded { instance_idx: usize, connections: Vec<InstanceConnection> },
    ObjectsTreeLoaded { database: String, objects: Vec<ObjectsRow> },
    InstancesRefreshComplete,
}
```

**ExplorerIntent 结构**:

```rust
// explorer/intent.rs
pub enum ExplorerIntent {
    InstanceSelected {                    // 选择实例 → 通知 SQL Workspace
        instance_idx: usize,
        instance_name: String,
    },
    ObjectSelected {                      // 选择对象 → 通知 SQL Workspace
        database: String,
        schema: Option<String>,
        object_name: String,
        kind: ObjectKind,
    },
    ContextChanged {                      // 切换数据库/Schema → 通知 SQL Workspace
        database: String,
        schema: Option<String>,
    },
    RequestOpenWorkspace {                // 请求打开 Instance Workspace
        instance_idx: usize,
    },
}
```

**ExplorerEffect 结构**:

```rust
// explorer/effect.rs
pub enum ExplorerEffect {
    LoadInstances,                        // 从持久化存储加载实例列表
    LoadInstanceConnections {             // 加载指定实例的连接列表
        instance_idx: usize,
    },
    RefreshInstances,                     // 刷新实例列表
    RefreshInstance {                     // 刷新单个实例详情
        instance_idx: usize,
    },
    LoadObjectsTree {                     // 加载对象树（从数据库获取元数据）
        database: String,
        schema: Option<String>,
    },
    RefreshObjects {                      // 刷新当前对象树
        database: String,
        schema: Option<String>,
    },
    DeleteInstance {                      // 删除实例
        instance_idx: usize,
    },
}
```

**ExplorerState 结构**:

```rust
// explorer/state.rs
pub struct ExplorerState {
    pub instances: InstancesState,        // 类型 1 子模块
    pub objects: ObjectsState,            // 类型 1 子模块
    pub active_pane: ExplorerPane,
    pub search_text: String,
}
```

**Update 返回类型**:

```rust
// explorer/update.rs
// 纯函数式按值 update：用 std::mem::take 只 move 出被改的子状态更新后放回，
// 其余字段原样保留，避免整棵深克隆。
pub fn update(
    msg: ExplorerMessage,
    mut state: ExplorerState
) -> (ExplorerState, Vec<ExplorerIntent>, Vec<ExplorerEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    
    match msg {
        ExplorerMessage::Instances(m) => {
            let instances::msg::InstancesMsg::Message(inner) = m;
            let instances_state = std::mem::take(&mut state.instances);
            let (s, child_intents, child_effects) = instances::update(inner, instances_state);
            state.instances = s;
            intents.extend(child_intents);
            effects.extend(child_effects);
        }
        ExplorerMessage::Objects(m) => {
            let objects::msg::ObjectsMsg::Message(inner) = m;
            let objects_state = std::mem::take(&mut state.objects);
            let (s, child_intents, child_effects) = objects::update(inner, objects_state);
            state.objects = s;
            intents.extend(child_intents);
            effects.extend(child_effects);
        }
        ExplorerMsg::LoadInstances => {
            effects.push(ExplorerEffect::LoadInstances);
        }
        ExplorerMessage::RefreshInstances => {
            effects.push(ExplorerEffect::RefreshInstances);
        }
        ExplorerMessage::InstancesLoaded { instances } => {
            // 异步结果 → 更新 state
            state.instances.set_instances(instances);
        }
        // ... 其他消息处理
    }
    
    (state, intents, effects)
}
```

**说明**: Explorer 的 update 返回 `(State, Vec<Intent>, Vec<Effect>)`：

- Effect 由 `app_shell` 执行（加载实例、加载对象树、刷新等）
- Effect 执行完成后，结果会包装为 ExplorerMsg（如 `InstancesLoaded`、`ObjectsTreeLoaded`）
- Intent 由 `app_shell` 处理（通知 SQL Workspace 切换上下文等）

***

**`instances/`** **子模块结构**:

| 文件          | 内容                                                                                                   | 当前来源                                   |
| ----------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------- |
| `mod.rs`    | 模块声明                                                                                                 | 新建                                     |
| `msg.rs`    | `InstancesMsg`（ExpandInstance, CollapseInstance, SelectInstance 等）                                   | 从 `app.rs` + `tree/` 提取                |
| `state.rs`  | `InstancesState`（ConnectionTreeState, TreeView 等）                                                    | 从 `app.rs` + `components/instances.rs` |
| `update.rs` | `update(InstancesMsg, InstancesState) -> (InstancesState, Vec<ExplorerIntent>, Vec<ExplorerEffect>)` | 从 `app.rs` + `tree/scroll.rs`          |
| `view.rs`   | `view(InstancesState)`                                                                               | 从 `ui.rs` + `components/instances.rs`  |

**InstancesMsg 结构**:

```rust
// explorer/instances/msg.rs
pub enum InstancesMsg {
    SelectInstance { instance_idx: usize },
    ExpandInstance { instance_idx: usize },
    CollapseInstance { instance_idx: usize },
    MoveCursor { direction: CursorDirection },
    LoadConnections { instance_idx: usize },  // ⚠️ 产生 Effect
    RefreshInstance { instance_idx: usize },    // ⚠️ 产生 Effect
    SearchInput { text: String },
    ClearSearch,
}
```

**Instances 分析结果**:

- ✅ **需要 Msg**: 选择、展开/折叠、光标移动、搜索等是内部状态变更
- ✅ **需要 Intent**: `SelectInstance` 需要通知 SQL Workspace 切换上下文
- ✅ **需要 Effect**: `LoadConnections`、`RefreshInstance` 需要持久化存储读取

**Instances update 返回类型**: `(InstancesState, Vec<ExplorerIntent>, Vec<ExplorerEffect>)`

***

**`objects/`** **子模块结构**:

| 文件          | 内容                                                                                             | 当前来源                      |
| ----------- | ---------------------------------------------------------------------------------------------- | ------------------------- |
| `mod.rs`    | 模块声明                                                                                           | 新建                        |
| `msg.rs`    | `ObjectsMsg`（NavigateUp, NavigateDown, SelectObject 等）                                         | 从 `tree/objects.rs` 提取    |
| `state.rs`  | `ObjectsState`（ObjectsView 等）                                                                  | 从 `components/objects.rs` |
| `update.rs` | `update(ObjectsMsg, ObjectsState) -> (ObjectsState, Vec<ExplorerIntent>, Vec<ExplorerEffect>)` | 从 `tree/objects.rs`       |
| `view.rs`   | `view(ObjectsState)`                                                                           | 从 `tree/objects.rs`       |

**ObjectsMsg 结构**:

```rust
// explorer/objects/msg.rs
pub enum ObjectsMsg {
    SelectDatabase { database: String },
    SelectSchema { database: String, schema: String },
    SelectObject {                                // ⚠️ 产生 Intent
        database: String,
        schema: Option<String>,
        object_name: String,
        kind: ObjectKind,
    },
    ExpandNode { node: ObjectsNode },
    CollapseNode { node: ObjectsNode },
    MoveCursor { direction: CursorDirection },
    LoadObjects {                                 // ⚠️ 产生 Effect
        database: String,
        schema: Option<String>,
    },
    RefreshCurrent,                              // ⚠️ 产生 Effect
    SearchInput { text: String },
    ClearSearch,
}
```

**Objects 分析结果**:

- ✅ **需要 Msg**: 展开/折叠、光标移动、搜索等是内部状态变更
- ✅ **需要 Intent**: `SelectObject`、`SelectDatabase`、`SelectSchema` 需要通知 SQL Workspace
- ✅ **需要 Effect**: `LoadObjects`、`RefreshCurrent` 需要从数据库获取元数据

**Objects update 返回类型**: `(ObjectsState, Vec<ExplorerIntent>, Vec<ExplorerEffect>)`

***

#### Explorer Feature 分析总结

| Feature/子模块            | Msg | Intent                                                                      | Effect                                                 | update 返回类型                         |
| ---------------------- | --- | --------------------------------------------------------------------------- | ------------------------------------------------------ | ----------------------------------- |
| **explorer**           | ✅ 有 | ✅ 有（InstanceSelected, ObjectSelected, ContextChanged, RequestOpenWorkspace） | ✅ 有（LoadInstances, RefreshInstances, DeleteInstance 等） | `(State, Vec<Intent>, Vec<Effect>)` |
| **explorer/instances** | ✅ 有 | ✅ 有（SelectInstance → 通过父级）                                                  | ✅ 有（LoadConnections, RefreshInstance）                  | `(State, Vec<Intent>, Vec<Effect>)` |
| **explorer/objects**   | ✅ 有 | ✅ 有（SelectObject, SelectDatabase, SelectSchema → 通过父级）                      | ✅ 有（LoadObjects, RefreshCurrent）                       | `(State, Vec<Intent>, Vec<Effect>)` |

#### Effect 执行流程示例

```
1. 用户选择实例
   → InstancesMsg::SelectInstance { instance_idx }
   → Instances::update() 产生 ExplorerIntent::InstanceSelected
   → Explorer::update() 将 Intent 冒泡给 app_shell
   → app_shell 处理 Intent：路由 SqlWorkspaceMsg::SetInstance

2. 用户展开实例（加载连接）
   → InstancesMsg::LoadConnections { instance_idx }
   → Instances::update() 产生 ExplorerEffect::LoadInstanceConnections
   → Explorer::update() 将 Effect 冒泡给 app_shell
   → app_shell 执行 Effect（从持久化存储读取）
   → 异步结果返回：ExplorerMsg::InstanceConnectionsLoaded
   → Explorer::update() 更新 state

3. 用户选择对象
   → ObjectsMsg::SelectObject { database, schema, ... }
   → Objects::update() 产生 ExplorerIntent::ObjectSelected
   → Explorer::update() 将 Intent 冒泡给 app_shell
   → app_shell 处理 Intent：路由 SqlWorkspaceMsg::OpenObject
```

***

**当前文件迁移映射**:

| 当前文件                                                 | 目标位置                          | 说明                |
| ---------------------------------------------------- | ----------------------------- | ----------------- |
| `tree/mod.rs` + `tree/objects.rs` + `tree/scroll.rs` | `explorer/objects/`           | Objects 子模块       |
| `components/instances.rs` (TreeSearchMut)            | `explorer/instances/`         | Instances 子模块     |
| `components/objects.rs` (ObjectsView)                | `explorer/objects/state.rs`   | Objects 状态        |
| `components/explorer.rs` (ExplorerView)              | `explorer/state.rs`           | 聚合状态              |
| `components/shared/search.rs` (PaneSearch)           | `common/` 或子模块 state          | 通用搜索组件保留在 common  |
| `app.rs:ConnectionTreeState`                         | `explorer/instances/state.rs` | Instances 核心状态    |
| `app.rs:InstanceNode`                                | `explorer/instances/state.rs` | 实例树节点结构           |
| `lib.rs:load_instance_connections()`                 | `explorer/effect.rs` + 执行器    | 从持久化加载连接 → Effect |
| `lib.rs:reload_managed_instance()`                   | `explorer/effect.rs` + 执行器    | 刷新实例详情 → Effect   |
| `hints.rs` → 树相关 footer                              | `explorer/view.rs`            | <br />            |

**状态归属**:

- `instances/state.rs`: `ConnectionTreeState`、`InstanceNode`、实例树搜索状态、树光标状态
- `objects/state.rs`: `ObjectsView`、`ObjectsRow`、Objects 搜索状态、Objects cursor/scroll/expanded
- `explorer/state.rs`: `ExplorerPane`、`search_text`

**分析结果**:

- ✅ **需要 Msg**: 丰富的内部交互（选择、展开/折叠、搜索、光标移动等）
- ✅ **需要 Intent**: InstanceSelected、ObjectSelected、ContextChanged（通知 SQL Workspace 切换上下文）
- ✅ **需要 Effect**: LoadInstances、RefreshInstances、LoadObjectsTree（从持久化存储/数据库加载数据）

***

### Feature 6: `instance_workspace` — 实例详情工作台

**职责**: 中间栏实例详情，包含 Overview（实例详情）和 Connections（连接管理）两个子面板

**TEA 结构**:

| 文件             | 类型   | 内容                                                            | 当前来源                                |
| -------------- | ---- | ------------------------------------------------------------- | ----------------------------------- |
| `mod.rs`       | —    | 模块声明 + 公开 API                                                 | 新建                                  |
| `msg.rs`       | —    | `IwMsg`（嵌套 OverviewMsg + ConnectionsMsg + 异步结果）               | 新建                                  |
| `intent.rs`    | —    | `IwIntent`（跨 Feature 通信）                                      | 新建                                  |
| `effect.rs`    | —    | `IwEffect`（异步操作）                                              | 新建                                  |
| `state.rs`     | —    | `IwState`（聚合子模块 state）                                        | 聚合 OverviewState + ConnectionsState |
| `update.rs`    | —    | `update(msg, state) -> (State, Vec<IwIntent>, Vec<IwEffect>)` | 父级调度逻辑                              |
| `view.rs`      | —    | 主渲染函数                                                         | 从 `instance_workspace/draw.rs` 提取   |
| `overview/`    | 类型 1 | Overview 子模块（独立 TEA）                                          | 见下表                                 |
| `connections/` | 类型 1 | Connections 子模块（独立 TEA）                                       | 见下表                                 |

**IwMsg 结构**:

```rust
// instance_workspace/msg.rs
pub enum IwMsg {
    // 子模块消息
    OverviewMsg(OverviewMsg),
    ConnectionsMsg(ConnectionsMsg),
    
    // 内部状态变更
    FocusPane { pane: IwPane },
    
    // 异步结果消息（来自 Effect 执行后的反馈）
    InstanceDataLoaded { instance: ManagedInstance },
    ConnectionsLoaded { connections: Vec<InstanceConnection> },
    ConnectionSaved { connection: InstanceConnection },
    ConnectionDeleted { connection_name: String },
    ConnectionTestResult { conn_idx: usize, success: bool, latency_ms: u64 },
    UnregisterComplete { success: bool },
}
```

**IwIntent 结构**:

```rust
// instance_workspace/intent.rs
pub enum IwIntent {
    RefreshExplorerInstances,        // 连接变更 → 通知 Explorer 刷新实例树
    RefreshExplorerConnections {      // 连接变更 → 通知 Explorer 刷新连接列表
        instance_idx: usize,
    },
    UnregisterInstance,               // 注销实例 → 通知 Explorer
    CloseWorkspace,                   // 请求关闭 Workspace
}
```

**IwEffect 结构**:

```rust
// instance_workspace/effect.rs
pub enum IwEffect {
    LoadInstanceData {                 // 加载实例详情
        instance_idx: usize,
    },
    LoadConnections {                  // 加载连接列表
        instance_idx: usize,
    },
    RefreshInstanceData {              // 刷新实例数据
        instance_idx: usize,
    },
    SaveConnection {                   // 保存连接（新增或更新）
        instance_idx: usize,
        connection: InstanceConnection,
        is_new: bool,
    },
    DeleteConnection {                 // 删除连接
        instance_idx: usize,
        connection_idx: usize,
    },
    TestConnection {                   // 测试连接（ping 数据库）
        instance_idx: usize,
        connection_idx: usize,
    },
    UnregisterInstance {               // 注销实例
        instance_idx: usize,
    },
}
```

**IwState 结构**:

```rust
// instance_workspace/state.rs
pub struct IwState {
    pub overview: OverviewState,         // 类型 1 子模块
    pub connections: ConnectionsState,   // 类型 1 子模块
    pub active_pane: IwPane,
    pub instance_idx: Option<usize>,    // 当前实例索引
    pub status_message: String,         // 状态反馈消息
}
```

**Update 返回类型**:

```rust
// instance_workspace/update.rs
// 纯函数式按值 update：用 std::mem::take 只 move 出被改的子状态更新后放回，
// 其余字段原样保留，避免整棵深克隆。
pub fn update(
    msg: IwMessage,
    mut state: IwState
) -> (IwState, Vec<IwIntent>, Vec<IwEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    
    match msg {
        IwMessage::Overview(m) => {
            let overview::msg::OverviewMsg::Message(inner) = m;
            let overview_state = std::mem::take(&mut state.overview);
            let (s, child_intents, child_effects) = overview::update(inner, overview_state);
            state.overview = s;
            intents.extend(child_intents);
            effects.extend(child_effects);
        }
        IwMessage::Connections(m) => {
            let connections::msg::ConnectionsMsg::Message(inner) = m;
            let connections_state = std::mem::take(&mut state.connections);
            let (s, child_intents, child_effects) = connections::update(inner, connections_state);
            state.connections = s;
            intents.extend(child_intents);
            effects.extend(child_effects);
        }
        IwMessage::RefreshInstance => {
            if let Some(idx) = state.instance_idx {
                effects.push(IwEffect::RefreshInstanceData { instance_idx: idx });
            }
        }
        IwMessage::InstanceDataLoaded { instance } => {
            // 异步结果 → 更新 state
            state.overview.set_instance(instance);
        }
        IwMessage::ConnectionSaved { connection } => {
            // 异步结果 → 通知 Explorer 刷新
            intents.push(IwIntent::RefreshExplorerInstances);
        }
        // ... 其他消息处理
    }
    
    (state, intents, effects)
}
```

**说明**: Instance Workspace 的 update 返回 `(State, Vec<Intent>, Vec<Effect>)`：

- Effect 由 `app_shell` 执行（加载数据、保存连接、测试连接等）
- Effect 执行完成后，结果会包装为 IwMsg（如 `InstanceDataLoaded`、`ConnectionSaved`）
- Intent 由 `app_shell` 处理（通知 Explorer 刷新实例树等）

***

**`overview/`** **子模块结构**:

| 文件          | 内容                                                                                    | 当前来源                                              |
| ----------- | ------------------------------------------------------------------------------------- | ------------------------------------------------- |
| `mod.rs`    | 模块声明                                                                                  | 新建                                                |
| `msg.rs`    | `OverviewMsg`（Refresh, SelectField, Unregister 等）                                     | 从 `instance_workspace/draw.rs` + `interact.rs` 提取 |
| `state.rs`  | `OverviewState`（实例详情字段、选中字段等）                                                         | 从 `instance_workspace/draw.rs` + `app.rs`         |
| `update.rs` | `update(OverviewMsg, OverviewState) -> (OverviewState, Vec<IwIntent>, Vec<IwEffect>)` | 从 `instance_workspace/interact.rs`                |
| `view.rs`   | `view(OverviewState)`                                                                 | 从 `instance_workspace/draw.rs`                    |

**OverviewMsg 结构**:

```rust
// instance_workspace/overview/msg.rs
pub enum OverviewMsg {
    Refresh,                                      // ⚠️ 产生 Effect
    SelectField { field: OverviewField },
    MoveCursor { direction: CursorDirection },
    Unregister,                                   // ⚠️ 产生 Effect + Intent
}
```

**Overview 分析结果**:

- ✅ **需要 Msg**: 选择字段、光标移动是内部状态变更
- ✅ **需要 Intent**: `Unregister` 需要通知 Explorer
- ✅ **需要 Effect**: `Refresh`、`Unregister` 需要持久化存储操作

**Overview update 返回类型**: `(OverviewState, Vec<IwIntent>, Vec<IwEffect>)`

***

**`connections/`** **子模块结构**:

| 文件          | 内容                                                                                             | 当前来源                                              |
| ----------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| `mod.rs`    | 模块声明                                                                                           | 新建                                                |
| `msg.rs`    | `ConnectionsMsg`（AddConnection, EditConnection, DeleteConnection, TestConnection 等）            | 从 `instance_workspace/draw.rs` + `interact.rs` 提取 |
| `state.rs`  | `ConnectionsState`（连接列表、表单状态、测试状态等）                                                            | 从 `instance_workspace/draw.rs` + `app.rs`         |
| `update.rs` | `update(ConnectionsMsg, ConnectionsState) -> (ConnectionsState, Vec<IwIntent>, Vec<IwEffect>)` | 从 `instance_workspace/interact.rs`                |
| `view.rs`   | `view(ConnectionsState)`                                                                       | 从 `instance_workspace/draw.rs`                    |

**ConnectionsMsg 结构**:

```rust
// instance_workspace/connections/msg.rs
pub enum ConnectionsMsg {
    SelectConnection { conn_idx: usize },
    MoveCursor { direction: CursorDirection },
    CursorUp,
    CursorDown,
    
    // 表单操作
    StartAddForm,
    StartEditForm { conn_idx: usize },
    CancelForm,
    FormFieldChanged { field: AddField, value: String },
    SubmitForm,                                   // ⚠️ 产生 Effect（新增或更新）
    DeleteConnection { conn_idx: usize },          // ⚠️ 产生 Effect
    TestConnection { conn_idx: usize },            // ⚠️ 产生 Effect（ping 数据库）
    QuickTestCurrent,                              // ⚠️ 产生 Effect
}
```

**Connections 分析结果**:

- ✅ **需要 Msg**: 选择连接、光标移动、表单编辑是内部状态变更
- ✅ **需要 Intent**: 保存/删除/测试连接完成后需要通知 Explorer 刷新
- ✅ **需要 Effect**: 所有 CRUD 操作和测试连接都是异步的

**Connections update 返回类型**: `(ConnectionsState, Vec<IwIntent>, Vec<IwEffect>)`

**Connections update 示例**:

```rust
// instance_workspace/connections/update.rs
pub fn update(
    msg: ConnectionsMsg, 
    state: &ConnectionsState
) -> (ConnectionsState, Vec<IwIntent>, Vec<IwEffect>) {
    let mut new_state = state.clone();
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    
    match msg {
        ConnectionsMsg::SubmitForm => {
            if let Some(form) = &state.form {
                if form.is_new {
                    effects.push(IwEffect::SaveConnection {
                        instance_idx: state.instance_idx.unwrap(),
                        connection: form.to_connection(),
                        is_new: true,
                    });
                } else {
                    effects.push(IwEffect::SaveConnection {
                        instance_idx: state.instance_idx.unwrap(),
                        connection: form.to_connection(),
                        is_new: false,
                    });
                }
            }
        }
        ConnectionsMsg::DeleteConnection { conn_idx } => {
            effects.push(IwEffect::DeleteConnection {
                instance_idx: state.instance_idx.unwrap(),
                connection_idx: conn_idx,
            });
        }
        ConnectionsMsg::TestConnection { conn_idx } => {
            effects.push(IwEffect::TestConnection {
                instance_idx: state.instance_idx.unwrap(),
                connection_idx: conn_idx,
            });
        }
        // ... 其他消息处理
    }
    
    (new_state, intents, effects)
}
```

**ConnectionsState 结构（含 FormMode）**:

```rust
// instance_workspace/connections/state.rs

/// 表单模式：区分三种视图状态
#[derive(Clone, Debug, PartialEq)]
pub enum FormMode {
    /// 显示连接列表
    None,
    /// 显示新增连接表单
    Add,
    /// 显示编辑连接表单（包含编辑索引）
    Edit { idx: usize },
}

/// 连接表单数据（Add 和 Edit 共用）
#[derive(Clone, Debug, Default)]
pub struct ConnectionFormData {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    /// 原始连接索引（编辑模式时使用）
    pub original_idx: Option<usize>,
}

/// 连接测试标记
#[derive(Clone, Debug)]
pub struct ConnectionTestMark {
    pub conn_idx: usize,
    pub success: bool,
    pub latency_ms: u64,
}

/// Connections 子模块状态
#[derive(Clone, Debug)]
pub struct ConnectionsState {
    /// 当前实例的连接列表
    pub connections: Vec<InstanceConnection>,
    /// 列表光标位置
    pub cursor: usize,
    /// 当前选中的连接
    pub selected_idx: Option<usize>,
    /// 表单模式（控制视图切换）
    pub form_mode: FormMode,
    /// 表单数据（Add/Edit 共用）
    pub form_data: ConnectionFormData,
    /// 测试标记列表
    pub test_marks: Vec<ConnectionTestMark>,
}
```

**FormMode 切换逻辑**:

```
                    StartAddForm
  ┌─────────┐  ─────────────────►  ┌─────────┐
  │  List   │                      │   Add   │
  │ (None)  │  ◄─────────────────  │  Form   │
  └─────────┘     CancelForm/      └─────────┘
       ▲          SubmitForm          ▲
       │                               │
       │ StartEditForm                 │ StartEditForm
       │                               │
  ┌────┴────┐  ─────────────────►  ┌─────────┐
  │  List   │                      │   Edit  │
  │ (None)  │  ◄─────────────────  │  Form   │
  └─────────┘     CancelForm/      └─────────┘
                  SubmitForm
```

**视图切换说明**:

- `FormMode::None` → 渲染连接列表视图
- `FormMode::Add` → 渲染新增连接表单（空表单）
- `FormMode::Edit { idx }` → 渲染编辑连接表单（预填充数据）

**为什么不拆分为独立子模块**:

1. **共享表单结构**: Add 和 Edit 共用 `ConnectionFormData` 和校验逻辑
2. **紧密耦合**: 列表选择触发编辑、表单提交后刷新列表
3. **状态复杂度**: 单独来看都不够复杂，合并后仍可维护
4. **符合 TEA 原则**: 通过 `form_mode` 字段切换视图，保持单一数据源

***

#### Instance Workspace Feature 分析总结

| Feature/子模块                         | Msg | Intent                                                            | Effect                                                                    | update 返回类型                         |
| ----------------------------------- | --- | ----------------------------------------------------------------- | ------------------------------------------------------------------------- | ----------------------------------- |
| **instance\_workspace**             | ✅ 有 | ✅ 有（RefreshExplorerInstances, UnregisterInstance, CloseWorkspace） | ✅ 有（LoadInstanceData, SaveConnection, DeleteConnection, TestConnection 等） | `(State, Vec<Intent>, Vec<Effect>)` |
| **instance\_workspace/overview**    | ✅ 有 | ✅ 有（Unregister → 通过父级）                                            | ✅ 有（Refresh, Unregister）                                                  | `(State, Vec<Intent>, Vec<Effect>)` |
| **instance\_workspace/connections** | ✅ 有 | ✅ 有（保存/删除/测试完成后通知 Explorer → 通过父级）                                | ✅ 有（SaveConnection, DeleteConnection, TestConnection）                     | `(State, Vec<Intent>, Vec<Effect>)` |

#### Effect 执行流程示例

```
1. 用户点击 "Test Connection"
   → ConnectionsMsg::TestConnection { conn_idx }
   → Connections::update() 产生 IwEffect::TestConnection
   → Iw::update() 将 Effect 冒泡给 app_shell
   → app_shell 执行 Effect（异步 ping 数据库）
   → 异步结果返回：IwMsg::ConnectionTestResult
   → Iw::update() 更新 state 并显示结果

2. 用户保存新连接
   → ConnectionsMsg::SubmitForm
   → Connections::update() 产生 IwEffect::SaveConnection { is_new: true }
   → Iw::update() 将 Effect 冒泡给 app_shell
   → app_shell 执行 Effect（持久化存储写入）
   → 异步结果返回：IwMsg::ConnectionSaved
   → Iw::update() 产生 IwIntent::RefreshExplorerInstances
   → app_shell 处理 Intent：路由 ExplorerMsg::RefreshInstances

3. 用户删除连接
   → ConnectionsMsg::DeleteConnection { conn_idx }
   → Connections::update() 产生 IwEffect::DeleteConnection
   → Iw::update() 将 Effect 冒泡给 app_shell
   → app_shell 执行 Effect（持久化存储删除）
   → 异步结果返回：IwMsg::ConnectionDeleted
   → Iw::update() 产生 IwIntent::RefreshExplorerConnections
   → app_shell 处理 Intent：路由 ExplorerMsg::LoadConnections
```

***

**当前文件迁移映射**:

| 当前文件                                            | 目标位置                                                                                       | 说明                   |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------ | -------------------- |
| `instance_workspace/draw.rs`                    | `instance_workspace/view.rs` + `overview/view.rs` + `connections/view.rs`                  | 拆分渲染                 |
| `instance_workspace/interact.rs`                | `instance_workspace/update.rs` + `msg.rs` + `overview/update.rs` + `connections/update.rs` | 交互 → update；动作 → msg |
| `app.rs:InstanceWorkspaceState`                 | `instance_workspace/state.rs`                                                              | 状态迁移                 |
| `app.rs:AddConnectionForm` + `FormMode`         | `instance_workspace/connections/state.rs`                                                  | 表单状态                 |
| `app.rs:ConnectionFormMode`                     | `instance_workspace/connections/state.rs`                                                  | 表单模式                 |
| `app.rs:ConnectionTestMark`                     | `instance_workspace/connections/state.rs`                                                  | 测试标记                 |
| `app.rs:InstancePane` (Overview/Connections)    | `instance_workspace/state.rs`                                                              | 面板切换                 |
| `components/manager.rs` (WorkspaceView/Manager) | `instance_workspace/state.rs`                                                              | 工作台 view state       |
| `components/overview.rs`                        | `instance_workspace/overview/view.rs`                                                      | Overview view state  |
| `lib.rs:load_instance_connections()`            | `instance_workspace/effect.rs` + 执行器                                                       | 加载连接 → Effect        |
| `lib.rs:reload_managed_instance()`              | `instance_workspace/effect.rs` + 执行器                                                       | 刷新实例 → Effect        |
| `hints.rs` → `instance_workspace_footer_text`   | `instance_workspace/view.rs`                                                               | Footer 文本            |

**状态归属**:

- `InstanceWorkspaceState`（form、status、test\_marks、form\_pending\_d\_at）
- `InstancePane`（Overview / Connections 切换）
- 连接表单状态（AddConnectionForm、FormMode、ConnectionFormMode）
- 连接测试标记（ConnectionTestMark）
- Overview 光标和滚动状态
- Connections 光标
- `add_form`、`add_connection_status`、`add_connection_status_kind`、`default_user`

**分析结果**:

- ✅ **需要 Msg**: 丰富的内部交互（选择、表单编辑、光标移动等）
- ✅ **需要 Intent**: RefreshExplorerInstances、RefreshExplorerConnections（通知 Explorer 刷新）
- ✅ **需要 Effect**: LoadInstanceData、SaveConnection、DeleteConnection、TestConnection（所有 CRUD 和测试操作都是异步的）

***

### Feature 7: `sql_workspace` — SQL 工作台

**职责**: 右栏 SQL 工作台，包含 Tab 管理、SQL 编辑器（含 Context Picker、SQL Completion）、History、Results（含 Detail 编辑）

**TEA 结构**:

| 文件          | 类型   | 内容                                                               | 当前来源                         |
| ----------- | ---- | ---------------------------------------------------------------- | ---------------------------- |
| `mod.rs`    | —    | 模块声明 + 公开 API                                                    | 新建                           |
| `msg.rs`    | —    | `SqlMsg`（嵌套 editor、results、history 消息 + 异步结果）                    | 新建                           |
| `intent.rs` | —    | `SqlIntent`（跨 Feature 通信）                                        | 新建                           |
| `effect.rs` | —    | `SqlEffect`（异步操作）                                                | 新建                           |
| `state.rs`  | —    | `SqlWorkspaceState`（聚合 editor、results、history state + SqlTab 定义） | 聚合子模块 State                  |
| `update.rs` | —    | `update(msg, state) -> (State, Vec<SqlIntent>, Vec<SqlEffect>)`  | 父级调度逻辑                       |
| `view.rs`   | —    | 主渲染函数 + Tab 栏                                                    | 从 `ui.rs` 中 sql workspace 渲染 |
| `sql_tab/`  | —    | editor/results/history 的父 feature（标签页管理）                            | 新建                           |
| `sql_tab/editor/`   | 类型 1 | SQL 编辑器子模块（含 context\_picker、sql\_completion）                    | 见下表                          |
| `sql_tab/results/`  | 类型 1 | Results 子模块（含 detail）                                            | 见下表                          |
| `sql_tab/history/`  | 类型 1 | History 子模块                                                      | 见下表                          |

**SqlMsg 结构**:

```rust
// sql_workspace/msg.rs
pub enum SqlMsg {
    // 子模块消息
    EditorMsg(EditorMsg),
    ResultsMsg(ResultsMsg),
    HistoryMsg(HistoryMsg),
    
    // Tab 管理
    OpenTab { instance: String, connection: String },
    CloseTab { tab_id: TabId },
    SwitchTab { tab_id: TabId },
    SetInstance { instance: String },
    SetContext { database: String, schema: Option<String> },
    OpenObject { database: String, schema: Option<String>, object_name: String, kind: ObjectKind },
    
    // 查询操作
    RunQuery,                                    // ⚠️ 产生 Effect
    StopQuery,                                   // ⚠️ 产生 Effect
    
    // 异步结果消息（来自 Effect 执行后的反馈）
    QueryResult { tab_id: TabId, result: QueryResultData },
    QueryError { tab_id: TabId, error: String },
    QueryStopped { tab_id: TabId },
    CommitComplete { tab_id: TabId, success: bool },
    CommitError { tab_id: TabId, error: String },
    HistorySaved { tab_id: TabId, entry_id: String },
    ObjectInsertedOrUpdated { instance: String, object_name: String },
}
```

**SqlIntent 结构**:

```rust
// sql_workspace/intent.rs
pub enum SqlIntent {
    NotifyExplorerObjectChanged {               // 对象表编辑完成 → 通知 Explorer 刷新
        instance: String,
        database: String,
        object_name: String,
    },
    NotifyExplorerContextChanged {              // Context Picker 变更 → 通知 Explorer
        database: String,
        schema: Option<String>,
    },
    RequestOpenExplorer,                        // 请求打开 Explorer 面板
}
```

**SqlEffect 结构**:

```rust
// sql_workspace/effect.rs
pub enum SqlEffect {
    RunQuery {                                  // 执行 SQL 查询
        tab_id: TabId,
        sql: String,
        instance: String,
        connection: String,
        database: String,
        schema: Option<String>,
    },
    StopQuery {                                 // 停止正在执行的查询
        tab_id: TabId,
    },
    LoadHistory {                               // 加载历史记录
        instance: String,
        connection: String,
    },
    SaveHistoryEntry {                          // 保存历史记录
        instance: String,
        connection: String,
        sql: String,
    },
    DeleteHistoryEntry {                        // 删除历史记录
        entry_id: String,
    },
    RecallHistory {                             // 重新调用历史查询
        entry_id: String,
    },
    CommitResults {                             // 提交编辑结果（INSERT/UPDATE/DELETE）
        tab_id: TabId,
        changes: Vec<RowChange>,
    },
    RollbackResults {                           // 回滚编辑
        tab_id: TabId,
    },
    LoadContextPickerData {                     // 加载 Context Picker 元数据
        instance: String,
        connection: String,
    },
    LoadCompletionItems {                       // 加载 SQL 补全项
        instance: String,
        connection: String,
        database: Option<String>,
    },
}
```

**SqlWorkspaceState 结构**:

```rust
// sql_workspace/state.rs
pub struct SqlWorkspaceState {
    pub tabs: Vec<SqlTab>,
    pub active_tab: usize,
    pub editor: EditorState,
    pub results: ResultsState,
    pub history: HistoryState,
    pub query_status: QueryStatus,
}
```

**QueryStatus 结构**:

```rust
// sql_workspace/state.rs
pub struct QueryStatus {
    pub is_running: bool,
    pub running_tab_id: Option<TabId>,
    pub last_result: Option<QueryResultData>,
    pub last_error: Option<String>,
}
```

**Update 返回类型**:

```rust
// sql_workspace/update.rs
pub fn update(
    msg: SqlMsg, 
    state: &SqlWorkspaceState
) -> (SqlWorkspaceState, Vec<SqlIntent>, Vec<SqlEffect>) {
    let mut new_state = state.clone();
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    
    match msg {
        SqlMsg::EditorMsg(editor_msg) => {
            let (new_editor, child_intents, child_effects) = 
                editor::update(editor_msg, &state.editor);
            new_state.editor = new_editor;
            intents.extend(child_intents);
            effects.extend(child_effects);
        }
        SqlMsg::ResultsMsg(results_msg) => {
            let (new_results, child_intents, child_effects) = 
                results::update(results_msg, &state.results);
            new_state.results = new_results;
            intents.extend(child_intents);
            effects.extend(child_effects);
        }
        SqlMsg::HistoryMsg(history_msg) => {
            let (new_history, child_intents, child_effects) = 
                history::update(history_msg, &state.history);
            new_state.history = new_history;
            intents.extend(child_intents);
            effects.extend(child_effects);
        }
        SqlMsg::RunQuery => {
            if let Some(tab) = state.tabs.get(state.active_tab) {
                effects.push(SqlEffect::RunQuery {
                    tab_id: tab.id,
                    sql: tab.sql_text(),
                    instance: tab.instance_name.clone(),
                    connection: tab.connection_name.clone(),
                    database: tab.database.clone(),
                    schema: tab.schema.clone(),
                });
                new_state.query_status.is_running = true;
                new_state.query_status.running_tab_id = Some(tab.id);
            }
        }
        SqlMsg::QueryResult { tab_id, result } => {
            // 异步结果 → 更新 state
            if let Some(tab) = new_state.tabs.iter_mut().find(|t| t.id == tab_id) {
                tab.set_result(result.clone());
            }
            new_state.results.set_data(result);
            new_state.query_status.is_running = false;
        }
        SqlMsg::CommitComplete { tab_id, success } => {
            if success {
                // 提交完成 → 可能需要通知 Explorer 刷新
                intents.push(SqlIntent::NotifyExplorerObjectChanged {
                    instance: // 从 tab 获取
                });
            }
        }
        // ... 其他消息处理
    }
    
    (new_state, intents, effects)
}
```

**说明**: SQL Workspace 的 update 返回 `(State, Vec<Intent>, Vec<Effect>)`：

- Effect 由 `app_shell` 执行（执行查询、保存历史、提交编辑等）
- Effect 执行完成后，结果会包装为 SqlMsg（如 `QueryResult`、`CommitComplete`）
- Intent 由 `app_shell` 处理（通知 Explorer 刷新等）

***

**`editor/`** **子模块结构**:

| 文件                | 内容                                                                                | 当前来源                             |
| ----------------- | --------------------------------------------------------------------------------- | -------------------------------- |
| `mod.rs`          | 模块声明                                                                              | 新建                               |
| `msg.rs`          | `EditorMsg`（EditSql, SearchSql, CursorMove, ContextPickerMsg, CompletionMsg 等）    | 从 `ui.rs` + `sql_search.rs` 提取   |
| `state.rs`        | `EditorState`（SQL 内容、光标位置、搜索状态、ContextPickerState、CompletionState 等）              | 从 `ui.rs` + `sql_editability.rs` |
| `update.rs`       | `update(EditorMsg, EditorState) -> (EditorState, Vec<SqlIntent>, Vec<SqlEffect>)` | 从 `app.rs`                       |
| `view.rs`         | `view(EditorState)`                                                               | 从 `ui.rs`                        |
| `context_picker/` | 子模块（独立 TEA）                                                                       | 见下表                              |
| `sql_completion/` | 子模块（独立 TEA）                                                                       | 见下表                              |

**EditorMsg 结构**:

```rust
// sql_workspace/sql_tab/editor/msg.rs
pub enum EditorMsg {
    EditSql { char: char },
    CursorMove { direction: CursorDirection },
    SearchSql { text: String },
    ContextPickerMsg(ContextPickerMsg),
    CompletionMsg(CompletionMsg),
    ToggleEditable,
    Undo,
    Redo,
    UpdateContext { database: String, schema: Option<String> },  // ⚠️ 产生 Intent
}
```

**Editor 分析结果**:

- ✅ **需要 Msg**: 编辑、光标、搜索是内部状态变更
- ✅ **需要 Intent**: `UpdateContext` 需要通知 Explorer
- ✅ **需要 Effect**: ContextPicker 加载元数据、Completion 加载补全项

***

**`sql_tab/editor/context_picker/`** **子模块结构**:

| 文件          | 内容                                                                                                     | 当前来源                                          |
| ----------- | ------------------------------------------------------------------------------------------------------ | --------------------------------------------- |
| `mod.rs`    | 模块声明                                                                                                   | 新建                                            |
| `msg.rs`    | `ContextPickerMsg`（SelectCatalog, SelectSchema, NavigateUp, NavigateDown, RefreshData 等）               | 从 `context_picker/types.rs` + `catalog.rs` 提取 |
| `state.rs`  | `ContextPickerState`（catalog、schema、cursor、search 等）                                                   | 从 `context_picker/types.rs` + `catalog.rs`    |
| `update.rs` | `update(ContextPickerMsg, ContextPickerState) -> (ContextPickerState, Vec<SqlIntent>, Vec<SqlEffect>)` | 从 `context_picker/catalog.rs`                 |
| `view.rs`   | `view(ContextPickerState)`                                                                             | 从 `context_picker/view.rs`                    |

**ContextPickerMsg 结构**:

```rust
// sql_workspace/sql_tab/editor/context_picker/msg.rs
pub enum ContextPickerMsg {
    SelectCatalog { name: String },
    SelectSchema { name: String },
    NavigateUp,
    NavigateDown,
    MoveCursor { direction: CursorDirection },
    RefreshData,                                // ⚠️ 产生 Effect
    SetContext { database: String, schema: Option<String> },  // ⚠️ 产生 Intent
}
```

**ContextPicker 分析结果**:

- ✅ **需要 Msg**: 选择、导航、光标移动是内部状态变更
- ✅ **需要 Intent**: `SetContext` 需要通知 Explorer
- ✅ **需要 Effect**: `RefreshData` 需要从数据库加载元数据

***

**`sql_tab/editor/sql_completion/`** **子模块结构**:

| 文件            | 内容                                                                            | 当前来源                         |
| ------------- | ----------------------------------------------------------------------------- | ---------------------------- |
| `mod.rs`      | 模块声明                                                                          | 新建                           |
| `msg.rs`      | `CompletionMsg`（TriggerCompletion, SelectCompletion, ApplyCompletion 等）       | 从 `sql_completion/` 提取       |
| `state.rs`    | `CompletionState`（items、cursor、visible、selected 等）                            | 从 `sql_completion/apply.rs`  |
| `update.rs`   | `update(CompletionMsg, CompletionState) -> (CompletionState, Vec<SqlEffect>)` | 从 `sql_completion/mod.rs`    |
| `view.rs`     | `view(CompletionState)`                                                       | 从 `sql_completion/render.rs` |
| `provider.rs` | 保留                                                                            | 补全项提供者                       |
| `semantic.rs` | 保留                                                                            | 语义分析                         |
| `context.rs`  | 保留                                                                            | 上下文分析                        |
| `tokens.rs`   | 保留                                                                            | 词法分析                         |
| `keywords.rs` | 保留                                                                            | 关键字列表                        |

**CompletionMsg 结构**:

```rust
// sql_workspace/sql_tab/editor/sql_completion/msg.rs
pub enum CompletionMsg {
    TriggerCompletion,                          // ⚠️ 可能产生 Effect（加载补全项）
    SelectCompletion { index: usize },
    ApplyCompletion,
    CloseCompletion,
    MoveCursor { direction: CursorDirection },
    LoadCompletions,                            // ⚠️ 产生 Effect
}
```

**Completion 分析结果**:

- ✅ **需要 Msg**: 选择、应用、关闭是内部状态变更
- ❌ **不需要 Intent**: 所有操作都在 SQL Workspace 内部
- ✅ **需要 Effect**: 加载补全项可能需要从数据库获取元数据

***

**`sql_tab/results/`** **子模块结构**:

| 文件          | 内容                                                                                   | 当前来源                              |
| ----------- | ------------------------------------------------------------------------------------ | --------------------------------- |
| `mod.rs`    | 模块声明                                                                                 | 新建                                |
| `msg.rs`    | `ResultsMsg`（ScrollResults, SortResults, ExportResults, DetailMsg, CommitChanges 等）  | 从 `results/` 提取                   |
| `state.rs`  | `ResultsState`（结果集、选中行、排序状态、DetailState 等）                                           | 从 `results/`                      |
| `update.rs` | `update(ResultsMsg, ResultsState) -> (ResultsState, Vec<SqlIntent>, Vec<SqlEffect>)` | 从 `results/`                      |
| `view.rs`   | `view(ResultsState)`                                                                 | 从 `results/` + `common/format.rs` |
| `detail/`   | 子模块（独立 TEA）                                                                          | 见下表                               |

**ResultsMsg 结构**:

```rust
// sql_workspace/sql_tab/results/msg.rs
pub enum ResultsMsg {
    ScrollResults { delta: isize },
    SortResults { column: usize },
    ExportResults,
    SelectRow { row_idx: usize },
    DetailMsg(DetailMsg),
    ToggleEditMode,
    PageChange { page: usize },
    CommitChanges,                              // ⚠️ 产生 Effect + Intent
    RollbackChanges,                            // ⚠️ 产生 Effect
    RefreshPage,                                // ⚠️ 产生 Effect
}
```

**Results 分析结果**:

- ✅ **需要 Msg**: 滚动、排序、选择是内部状态变更
- ✅ **需要 Intent**: `CommitChanges` 需要通知 Explorer 刷新
- ✅ **需要 Effect**: `CommitChanges`、`RefreshPage` 是异步操作

***

**`sql_tab/results/detail/`** **子模块结构**:

| 文件          | 内容                                                                | 当前来源                                        |
| ----------- | ----------------------------------------------------------------- | ------------------------------------------- |
| `mod.rs`    | 模块声明                                                              | 新建                                          |
| `msg.rs`    | `DetailMsg`（EditField, SaveField, CancelEdit 等）                   | 从 `results/detail.rs` + `detail_edit.rs` 提取 |
| `state.rs`  | `DetailState`（详情数据、编辑状态、编辑缓冲区等）                                   | 从 `results/detail.rs` + `detail_edit.rs`    |
| `update.rs` | `update(DetailMsg, DetailState) -> (DetailState, Vec<SqlEffect>)` | 从 `results/detail.rs` + `detail_edit.rs`    |
| `view.rs`   | `view(DetailState)`                                               | 从 `results/detail.rs` + `detail_edit.rs`    |

**DetailMsg 结构**:

```rust
// sql_workspace/sql_tab/results/detail/msg.rs
pub enum DetailMsg {
    EditField { field_name: String },
    SaveField { field_name: String, value: String },  // ⚠️ 产生 Effect
    CancelEdit,
    AddRow,                                     // ⚠️ 产生 Effect
    DeleteRow { row_idx: usize },                // ⚠️ 产生 Effect
    ToggleEditMode,
}
```

**Detail 分析结果**:

- ✅ **需要 Msg**: 编辑字段、取消编辑是内部状态变更
- ❌ **不需要 Intent**: Detail 子模块不直接跨 Feature 通信
- ✅ **需要 Effect**: 保存字段、添加行、删除行需要异步提交

***

**`sql_tab/history/`** **子模块结构**:

| 文件          | 内容                                                                       | 当前来源            |
| ----------- | ------------------------------------------------------------------------ | --------------- |
| `mod.rs`    | 模块声明                                                                     | 新建              |
| `msg.rs`    | `HistoryMsg`（SelectHistory, DeleteHistory, RecallHistory, LoadHistory 等） | 从 `history/` 提取 |
| `state.rs`  | `HistoryState`（历史列表、选中项、详情滚动等）                                           | 从 `history/`    |
| `update.rs` | `update(HistoryMsg, HistoryState) -> (HistoryState, Vec<SqlEffect>)`     | 从 `history/`    |
| `view.rs`   | `view(HistoryState)`                                                     | 从 `history/`    |

**HistoryMsg 结构**:

```rust
// sql_workspace/sql_tab/history/msg.rs
pub enum HistoryMsg {
    SelectHistory { entry_idx: usize },
    DeleteHistory { entry_id: String },         // ⚠️ 产生 Effect
    RecallHistory { entry_id: String },         // ⚠️ 产生 Effect（重新调用查询）
    LoadHistory,                                // ⚠️ 产生 Effect
    SaveHistoryEntry { sql: String },            // ⚠️ 产生 Effect
    MoveCursor { direction: CursorDirection },
    Scroll { delta: isize },
    SearchInput { text: String },
    ClearSearch,
}
```

**History 分析结果**:

- ✅ **需要 Msg**: 选择、光标、滚动、搜索是内部状态变更
- ❌ **不需要 Intent**: History 子模块不直接跨 Feature 通信
- ✅ **需要 Effect**: 删除、重新调用、加载、保存是持久化操作

***

#### SQL Workspace Feature 分析总结

| Feature/子模块                               | Msg | Intent                                                         | Effect                                                           | update 返回类型                         |
| ----------------------------------------- | --- | -------------------------------------------------------------- | ---------------------------------------------------------------- | ----------------------------------- |
| **sql\_workspace**                        | ✅ 有 | ✅ 有（NotifyExplorerObjectChanged, NotifyExplorerContextChanged） | ✅ 有（RunQuery, StopQuery, CommitResults, LoadHistory 等）           | `(State, Vec<Intent>, Vec<Effect>)` |
| **sql\_workspace/sql\_tab/editor**                 | ✅ 有 | ✅ 有（UpdateContext → 通过父级）                                      | ✅ 有（加载 ContextPicker 元数据、加载 Completion 项）                        | `(State, Vec<Intent>, Vec<Effect>)` |
| **sql\_workspace/sql\_tab/editor/context\_picker** | ✅ 有 | ✅ 有（SetContext → 通过父级）                                         | ✅ 有（RefreshData）                                                 | `(State, Vec<Intent>, Vec<Effect>)` |
| **sql\_workspace/sql\_tab/editor/sql\_completion** | ✅ 有 | ❌ 无                                                            | ✅ 有（LoadCompletions）                                             | `(State, Vec<Effect>)`              |
| **sql\_workspace/sql\_tab/results**                | ✅ 有 | ✅ 有（CommitChanges → 通过父级）                                      | ✅ 有（CommitChanges, RefreshPage）                                  | `(State, Vec<Intent>, Vec<Effect>)` |
| **sql\_workspace/sql\_tab/results/detail**         | ✅ 有 | ❌ 无                                                            | ✅ 有（SaveField, AddRow, DeleteRow）                                | `(State, Vec<Effect>)`              |
| **sql\_workspace/sql\_tab/history**                | ✅ 有 | ❌ 无                                                            | ✅ 有（DeleteHistory, RecallHistory, LoadHistory, SaveHistoryEntry） | `(State, Vec<Effect>)`              |

#### Effect 执行流程示例

```
1. 用户执行查询
   → SqlMsg::RunQuery
   → SqlWorkspace::update() 产生 SqlEffect::RunQuery
   → app_shell 执行 Effect（异步数据库查询）
   → 异步结果返回：SqlMsg::QueryResult
   → SqlWorkspace::update() 更新 state（显示结果）

2. 用户编辑结果并提交
   → DetailMsg::SaveField
   → Detail::update() 产生 SqlEffect::CommitResults
   → Results::update() 冒泡 Effect 给 SqlWorkspace
   → SqlWorkspace::update() 将 Effect 冒泡给 app_shell
   → app_shell 执行 Effect（异步提交事务）
   → 异步结果返回：SqlMsg::CommitComplete
   → SqlWorkspace::update() 产生 SqlIntent::NotifyExplorerObjectChanged
   → app_shell 处理 Intent：路由 ExplorerMsg::RefreshObjects

3. 用户从 History 重新调用查询
   → HistoryMsg::RecallHistory
   → History::update() 产生 SqlEffect::RecallHistory
   → SqlWorkspace::update() 冒泡 Effect 给 app_shell
   → app_shell 执行 Effect（加载历史 SQL 文本到编辑器 + 执行查询）
   → 异步结果返回：SqlMsg::QueryResult
   → SqlWorkspace::update() 更新 state
```

***

**当前文件迁移映射**:

| 当前文件                                              | 目标位置                                            | 说明                                |
| ------------------------------------------------- | ----------------------------------------------- | --------------------------------- |
| `app.rs:SqlTab`                                   | `sql_workspace/state.rs`                        | 核心 Tab 状态定义                       |
| `sql_search.rs`                                   | `sql_workspace/sql_tab/editor/state.rs`                 | SQL 编辑器内搜索                        |
| `sql_editability.rs`                              | `sql_workspace/sql_tab/editor/state.rs`                 | 可编辑性数据结构                          |
| `results/query.rs`                                | `sql_workspace/effect.rs` + 执行器                 | 执行查询 → Effect                     |
| `results/pagination.rs`                           | `sql_workspace/sql_tab/results/state.rs`                | 分页状态                              |
| `results/detail.rs` + `detail_edit.rs`            | `sql_workspace/sql_tab/results/detail/`                 | Detail 子模块                        |
| `results/edit.rs` + `edit_flow.rs`                | `sql_workspace/sql_tab/results/update.rs` + `effect.rs` | 提交编辑 → Effect                     |
| `results/search.rs`                               | `sql_workspace/sql_tab/results/state.rs`                | Results 搜索状态                      |
| `results/toolbar.rs`                              | `sql_workspace/sql_tab/results/view.rs`                 | Results 工具栏渲染                     |
| `history/` 全部文件                                   | `sql_workspace/sql_tab/history/`                        | History 存储和渲染                     |
| `context_picker/` 全部文件                            | `sql_workspace/sql_tab/editor/context_picker/`          | editor 子模块                        |
| `sql_completion/` 全部文件                            | `sql_workspace/sql_tab/editor/sql_completion/`          | editor 子模块                        |
| `common/format.rs` → Results 专属部分                 | `sql_workspace/sql_tab/results/view.rs`                 | `init_results_layout`、Results 常量等 |
| `common/row_change_kind.rs`                       | `sql_workspace/sql_tab/results/detail/state.rs`         | Detail 子模块类型                      |
| `common/line_numbers.rs`                          | `sql_workspace/sql_tab/editor/view.rs`                  | SQL 编辑器专属                         |
| `app_model.rs` sql 相关 pending 字段                  | `sql_workspace/state.rs`                        | pending 操作状态                      |
| `hints.rs` → sql/results/history footer           | `sql_workspace/view.rs`                         | Footer 文本                         |
| `components/workspace.rs` (WorkspaceView/SqlPane) | `sql_workspace/state.rs`                        | Workspace view state              |
| `components/interaction.rs` sql 相关 hover          | `sql_workspace/sql_tab/editor/state.rs`                 | Editor 拖拽/hover state             |

**状态归属**:

- `SqlTab`（SQL 文本、编辑器状态、Results、History 等）
- Tab 集合管理（tabs、active\_tab、next\_tab\_id）
- Results 相关 pending 状态（count\_total\_rows、page\_action、row\_limit 等）
- History 存储（sql\_history）
- `MetaCache`（context\_picker 元数据缓存）→ `editor/context_picker/state.rs`
- `refresh_in_progress`、`last_refresh_at`、`refresh_cooldown_until`、`count_cooldown_until`、`scan_cooldown_until`、`test_connection_cooldown_until`
- `results_count_in_progress`
- Query 运行状态（is\_running、running\_tab\_id、last\_result、last\_error）

**分析结果**:

- ✅ **需要 Msg**: 丰富的内部交互（编辑、滚动、搜索、选择等）
- ✅ **需要 Intent**: NotifyExplorerObjectChanged、NotifyExplorerContextChanged（通知 Explorer 刷新）
- ✅ **需要 Effect**: RunQuery、StopQuery、CommitResults、LoadHistory、SaveHistoryEntry（所有持久化和数据库操作都是异步的）
- `connection_last_tab`（实例→tab 映射）

***

## 四、`app_shell` — 壳层

**职责**: 顶层协调者，管理 App 生命周期、消息路由、焦点、模态、调试覆盖层（key\_echo）

**TEA 结构**:

| 文件                   | 内容                               | 当前来源                                     |
| -------------------- | -------------------------------- | ---------------------------------------- |
| `mod.rs`             | 模块声明                             | 新建                                       |
| `msg.rs`             | `AppMsg` 枚举（全局消息 + 跨 feature 路由） | 从 `app.rs` 的 handle\_\* 消息提取             |
| `state.rs`           | `ShellState` + `KeyEchoState`    | 从 `app.rs` + `app_model.rs` 中壳层相关字段      |
| `update.rs`          | `update(msg, state) -> State`    | 从 `app.rs` 的路由逻辑提取                       |
| `focus.rs`           | 焦点/区域导航逻辑                        | 从 `zone_nav.rs`                          |
| `debug_overlay.rs`   | key\_echo 调试覆盖层渲染                | 从 `ui.rs` 的 `draw_key_echo_overlay()` 提取 |
| `session.rs`         | Session 持久化                      | 从 `session.rs`                           |
| `intent.rs`          | `AppIntent` 枚举（统一 Intent 定义）     | 新建，聚合所有 Feature 的 Intent                 |
| `effect.rs`          | `AppEffect` 枚举（统一 Effect 定义）     | 新建，聚合所有 Feature 的 Effect                 |
| `intent_router.rs`   | `IntentRouter`（Intent → Msg 路由）  | 新建，跨 Feature 通信中转                        |
| `effect_executor.rs` | `EffectExecutor`（Effect 异步执行）    | 新建，副作用执行器                                |

**当前文件迁移映射**:

| 当前文件                                       | 目标位置                         | 说明                                  |
| ------------------------------------------ | ---------------------------- | ----------------------------------- |
| `app.rs` 路由逻辑                              | `app_shell/update.rs`        | 消息路由、焦点切换                           |
| `app.rs` App 结构体                           | `app_shell/state.rs`         | 壳层状态（session、节流器、editor\_handler 等） |
| `app.rs:key_echo_enabled` + `key_echo_log` | `app_shell/state.rs`         | 调试覆盖层状态                             |
| `app.rs:record_key_echo()`                 | `app_shell/update.rs`        | 按键记录逻辑                              |
| `app.rs:format_key_echo()`                 | `app_shell/debug_overlay.rs` | 按键格式化                               |
| `ui.rs:draw_key_echo_overlay()`            | `app_shell/debug_overlay.rs` | 覆盖层渲染                               |
| `lib.rs:key_echo 切换逻辑`                     | `app_shell/update.rs`        | F12 切换逻辑                            |
| `components/focus.rs` (FocusState)         | `app_shell/state.rs`         | 焦点状态                                |
| `zone_nav.rs`                              | `app_shell/focus.rs`         | 区域导航                                |
| `session.rs`                               | `app_shell/session.rs`       | Session 持久化                         |
| `components/interaction.rs` 通用拖拽           | `app_shell/state.rs`         | 全局拖拽状态                              |

**状态归属**:

- `FocusZone`（Header / Explorer / Workspace 切换）
- `ModalKind`（Discover 模态开关）
- `ActiveSplitter`（全局分割器拖拽）
- `FocusState`（explorer\_pane、focus）
- 全局拖拽/hover 状态
- `UiLayout`（布局缓存）
- `content_epoch`
- `KeyEchoState`（key\_echo\_enabled、key\_echo\_log）— 调试覆盖层
- Session 持久化（tree\_width、explorer\_split\_ratio、discover\_targets\_ratio 等跨会话状态）

**key\_echo 说明**:

- 独立的调试覆盖层，按 F12 切换启用/禁用
- 显示最近按下的按键历史
- 覆盖在所有 Feature 之上，不遵循 Feature 边界
- 仅用于开发调试，普通用户不需要使用

***

### app\_shell Intent/Effect 处理架构

#### 核心架构图

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                                    app_shell                                  │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                          AppShellState                             │    │
│  │                                                                     │    │
│  │  - header_state, discover_state, explorer_state                     │    │
│  │  - iw_state, sql_workspace_state                                    │    │
│  │  - focus_zone: FocusZone                                           │    │
│  │  - pending_effects: Vec<PendingEffect>                             │    │
│  │  - effect_results: Vec<EffectResult>                               │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                         IntentRouter                                │    │
│  │                                                                     │    │
│  │  接收 Feature 产生的 Intent → 转换为目标 Feature 的 Msg               │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                         EffectExecutor                              │    │
│  │                                                                     │    │
│  │  接收 Feature 产生的 Effect → 异步执行 → 产生 EffectResult           │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

#### AppShellState 扩展

```rust
// app_shell/state.rs
pub struct AppShellState {
    // 各 Feature 的 State
    pub header: HeaderState,
    pub discover: DiscoverState,
    pub explorer: ExplorerState,
    pub instance_workspace: IwState,
    pub sql_workspace: SqlWorkspaceState,
    
    // 焦点管理
    pub focus_zone: FocusZone,
    
    // Effect 执行管理
    pub pending_effects: Vec<PendingEffect>,
    pub effect_results: Vec<EffectResult>,
    
    // 全局状态
    pub global_status: String,
}

/// 待执行的 Effect（携带源 Feature 信息，用于结果回传）
pub struct PendingEffect {
    pub effect: AppEffect,
    pub source_feature: FeatureId,
}

/// Effect 执行结果（会被包装为 Msg 路由回源 Feature）
pub struct EffectResult {
    pub result: EffectResultData,
    pub source_feature: FeatureId,
}

/// Feature 标识符（用于 Intent 来源追踪和 Effect 结果回传）
#[derive(Clone, Copy, Debug)]
pub enum FeatureId {
    Header,
    Discover,
    Explorer,
    InstanceWorkspace,
    SqlWorkspace,
}
```

#### AppMsg —— 统一的消息入口

```rust
// app_shell/msg.rs
pub enum AppMsg {
    // 来自各 Feature 的消息（由 Intent 转换而来）
    HeaderMsg(HeaderMsg),
    DiscoverMsg(DiscoverMsg),
    ExplorerMsg(ExplorerMsg),
    IwMsg(IwMsg),
    SqlMsg(SqlMsg),
    
    // 焦点切换
    FocusChanged { zone: FocusZone },
    
    // Effect 执行结果（由 EffectExecutor 返回）
    EffectResult(EffectResult),
    
    // 全局消息
    Tick,  // 每帧触发（用于 perf_monitor 等）
    Quit,
}
```

#### AppEffect —— 统一的 Effect 定义

```rust
// app_shell/effect.rs
pub enum AppEffect {
    // 来自各 Feature 的 Effect
    HeaderEffect(HeaderEffect),
    DiscoverEffect(DiscoverEffect),
    ExplorerEffect(ExplorerEffect),
    IwEffect(IwEffect),
    SqlEffect(SqlEffect),
}

pub enum AppIntent {
    Header(HeaderIntent),
    Discover(DiscoverIntent),
    Explorer(ExplorerIntent),
    Iw(IwIntent),
    Sql(SqlIntent),
}

pub fn intent_source(intent: &AppIntent) -> FeatureId {
    match intent {
        AppIntent::Header(_) => FeatureId::Header,
        AppIntent::Discover(_) => FeatureId::Discover,
        AppIntent::Explorer(_) => FeatureId::Explorer,
        AppIntent::Iw(_) => FeatureId::InstanceWorkspace,
        AppIntent::Sql(_) => FeatureId::SqlWorkspace,
    }
}

pub fn effect_source(effect: &AppEffect) -> FeatureId {
    match effect {
        AppEffect::Header(_) => FeatureId::Header,
        AppEffect::Discover(_) => FeatureId::Discover,
        AppEffect::Explorer(_) => FeatureId::Explorer,
        AppEffect::Iw(_) => FeatureId::InstanceWorkspace,
        AppEffect::Sql(_) => FeatureId::SqlWorkspace,
    }
}
```

#### IntentRouter 设计

```rust
// app_shell/intent_router.rs
pub struct IntentRouter;

impl IntentRouter {
    /// 处理来自各 Feature 的 Intent，转换为 AppMsg
    pub fn route(&self, intent: AppIntent) -> Vec<AppMsg> {
        let mut msgs = Vec::new();
        
        match intent {
            // Header Intents
            AppIntent::Header(HeaderIntent::OpenDiscoverModal) => {
                msgs.push(AppMsg::DiscoverMsg(DiscoverMsg::OpenModal));
            }
            
            // Discover Intents
            AppIntent::Discover(DiscoverIntent::CloseModal) => {
                msgs.push(AppMsg::DiscoverMsg(DiscoverMsg::CloseModal));
            }
            AppIntent::Discover(DiscoverIntent::NotifyInstancesChanged) => {
                msgs.push(AppMsg::ExplorerMsg(ExplorerMsg::RefreshInstances));
            }
            
            // Explorer Intents → 跨 Feature 通信
            AppIntent::Explorer(ExplorerIntent::InstanceSelected { 
                instance_idx, instance_name 
            }) => {
                msgs.push(AppMsg::IwMsg(IwMsg::LoadInstance { 
                    instance_idx, instance_name 
                }));
            }
            AppIntent::Explorer(ExplorerIntent::ObjectSelected { 
                database, schema, object_name, kind 
            }) => {
                msgs.push(AppMsg::SqlMsg(SqlMsg::OpenObject { 
                    database, schema, object_name, kind 
                }));
            }
            AppIntent::Explorer(ExplorerIntent::ContextChanged { 
                database, schema 
            }) => {
                msgs.push(AppMsg::SqlMsg(SqlMsg::SetContext { 
                    database, schema 
                }));
            }
            // 快捷键跨 Feature：Explorer → Instance Workspace
            AppIntent::Explorer(ExplorerIntent::RequestAddConnection { 
                instance_idx 
            }) => {
                msgs.push(AppMsg::IwMsg(IwMsg::OpenAddConnection { instance_idx }));
            }
            AppIntent::Explorer(ExplorerIntent::RequestEditConnection { 
                instance_idx, connection_idx 
            }) => {
                msgs.push(AppMsg::IwMsg(IwMsg::OpenEditConnection { 
                    instance_idx, connection_idx 
                }));
            }
            // Explorer → Explorer 内部刷新
            AppIntent::Explorer(ExplorerIntent::RefreshConnections { 
                instance_idx 
            }) => {
                msgs.push(AppMsg::ExplorerMsg(ExplorerMsg::LoadConnections { 
                    instance_idx 
                }));
            }
            
            // Instance Workspace Intents → 通知 Explorer
            AppIntent::Iw(IwIntent::RefreshExplorerInstances) => {
                msgs.push(AppMsg::ExplorerMsg(ExplorerMsg::RefreshInstances));
            }
            AppIntent::Iw(IwIntent::RefreshExplorerConnections { 
                instance_idx 
            }) => {
                msgs.push(AppMsg::ExplorerMsg(ExplorerMsg::LoadConnections { 
                    instance_idx 
                }));
            }
            AppIntent::Iw(IwIntent::CloseWorkspace) => {
                msgs.push(AppMsg::IwMsg(IwMsg::ClearWorkspace));
            }
            
            // SQL Workspace Intents → 通知 Explorer
            AppIntent::Sql(SqlIntent::NotifyExplorerObjectChanged { 
                instance: _, database, object_name 
            }) => {
                msgs.push(AppMsg::ExplorerMsg(ExplorerMsg::RefreshObjects { 
                    database, object_name 
                }));
            }
            AppIntent::Sql(SqlIntent::NotifyExplorerContextChanged { 
                database, schema 
            }) => {
                msgs.push(AppMsg::ExplorerMsg(ExplorerMsg::SetContext { 
                    database, schema 
                }));
            }
        }
        
        msgs
    }
}
```

#### EffectExecutor 设计

```rust
// app_shell/effect_executor.rs
use std::sync::mpsc;

pub struct EffectExecutor {
    tx: mpsc::Sender<EffectResult>,
    rx: mpsc::Receiver<EffectResult>,
}

impl EffectExecutor {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx }
    }
    
    /// 提交 Effect 执行（异步）
    pub fn submit(&self, effect: AppEffect, source: FeatureId) {
        let tx = self.tx.clone();
        
        tokio::spawn(async move {
            let result = Self::execute_effect(effect).await;
            let _ = tx.send(EffectResult {
                result,
                source_feature: source,
            });
        });
    }
    
    /// 执行具体的 Effect 逻辑（异步）
    async fn execute_effect(effect: AppEffect) -> EffectResultData {
        match effect {
            // Discover Effects
            AppEffect::Discover(DiscoverEffect::StartScan) => {
                let items = scan_network().await;
                EffectResultData::DiscoverScanComplete(items)
            }
            AppEffect::Discover(DiscoverEffect::CancelScan) => {
                cancel_scan().await;
                EffectResultData::DiscoverScanCancelled
            }
            AppEffect::Discover(DiscoverEffect::RegisterInstances { instances }) => {
                let count = save_instances(instances).await;
                EffectResultData::InstancesRegistered(count)
            }
            
            // Explorer Effects
            AppEffect::Explorer(ExplorerEffect::LoadInstances) => {
                let instances = load_from_store().await;
                EffectResultData::InstancesLoaded(instances)
            }
            AppEffect::Explorer(ExplorerEffect::LoadInstanceConnections { 
                instance_idx 
            }) => {
                let connections = load_connections(instance_idx).await;
                EffectResultData::ConnectionsLoaded { 
                    instance_idx, connections 
                }
            }
            AppEffect::Explorer(ExplorerEffect::LoadObjectsTree { 
                database, schema 
            }) => {
                let objects = fetch_objects(database, schema).await;
                EffectResultData::ObjectsTreeLoaded { database, objects }
            }
            AppEffect::Explorer(ExplorerEffect::DeleteInstance { 
                instance_idx 
            }) => {
                delete_instance_from_store(instance_idx).await;
                EffectResultData::InstanceDeleted(instance_idx)
            }
            
            // Instance Workspace Effects
            AppEffect::Iw(IwEffect::LoadInstanceData { instance_idx }) => {
                let instance = load_instance(instance_idx).await;
                EffectResultData::InstanceDataLoaded(instance)
            }
            AppEffect::Iw(IwEffect::SaveConnection { 
                instance_idx, connection, is_new 
            }) => {
                save_connection(instance_idx, &connection, is_new).await;
                EffectResultData::ConnectionSaved(connection)
            }
            AppEffect::Iw(IwEffect::DeleteConnection { 
                instance_idx, connection_idx 
            }) => {
                delete_connection(instance_idx, connection_idx).await;
                EffectResultData::ConnectionDeleted(connection_idx)
            }
            AppEffect::Iw(IwEffect::TestConnection { 
                instance_idx, connection_idx 
            }) => {
                let (success, latency) = 
                    ping_connection(instance_idx, connection_idx).await;
                EffectResultData::ConnectionTestResult { 
                    conn_idx: connection_idx, 
                    success, 
                    latency_ms: latency 
                }
            }
            AppEffect::Iw(IwEffect::UnregisterInstance { instance_idx }) => {
                unregister_instance(instance_idx).await;
                EffectResultData::InstanceUnregistered(instance_idx)
            }
            
            // SQL Workspace Effects
            AppEffect::Sql(SqlEffect::RunQuery { 
                tab_id, sql, instance, connection, database, schema 
            }) => {
                match execute_query(sql, instance, connection, database, schema).await {
                    Ok(result) => EffectResultData::QueryResult { tab_id, result },
                    Err(error) => EffectResultData::QueryError { tab_id, error },
                }
            }
            AppEffect::Sql(SqlEffect::StopQuery { tab_id }) => {
                stop_running_query(tab_id).await;
                EffectResultData::QueryStopped { tab_id }
            }
            AppEffect::Sql(SqlEffect::LoadHistory { instance, connection }) => {
                let history = load_history(instance, connection).await;
                EffectResultData::HistoryLoaded(history)
            }
            AppEffect::Sql(SqlEffect::SaveHistoryEntry { 
                instance, connection, sql 
            }) => {
                let entry_id = save_history(instance, connection, sql).await;
                EffectResultData::HistorySaved { entry_id }
            }
            AppEffect::Sql(SqlEffect::DeleteHistoryEntry { entry_id }) => {
                delete_history(entry_id).await;
                EffectResultData::HistoryDeleted(entry_id)
            }
            AppEffect::Sql(SqlEffect::RecallHistory { entry_id }) => {
                let sql = recall_history(entry_id).await;
                EffectResultData::HistoryRecalled { entry_id, sql }
            }
            AppEffect::Sql(SqlEffect::CommitResults { tab_id, changes }) => {
                match commit_changes(tab_id, changes).await {
                    Ok(()) => EffectResultData::CommitComplete { tab_id, success: true },
                    Err(error) => EffectResultData::CommitError { tab_id, error },
                }
            }
            AppEffect::Sql(SqlEffect::LoadContextPickerData { 
                instance, connection 
            }) => {
                let data = load_catalog_data(instance, connection).await;
                EffectResultData::ContextPickerDataLoaded(data)
            }
            AppEffect::Sql(SqlEffect::LoadCompletionItems { 
                instance, connection, database 
            }) => {
                let items = load_completion_items(instance, connection, database).await;
                EffectResultData::CompletionItemsLoaded(items)
            }
        }
    }
    
    /// 尝试接收 Effect 结果（非阻塞，在每帧 Tick 中调用）
    pub fn try_recv(&self) -> Option<EffectResult> {
        self.rx.try_recv().ok()
    }
}

/// Effect 结果数据（会被转换为 Msg 路由回源 Feature）
pub enum EffectResultData {
    // Discover
    DiscoverScanComplete(Vec<ScanTarget>),
    DiscoverScanCancelled,
    InstancesRegistered(usize),
    
    // Explorer
    InstancesLoaded(Vec<ManagedInstance>),
    ConnectionsLoaded { instance_idx: usize, connections: Vec<InstanceConnection> },
    ObjectsTreeLoaded { database: String, objects: Vec<ObjectsRow> },
    InstanceDeleted(usize),
    
    // Instance Workspace
    InstanceDataLoaded(ManagedInstance),
    ConnectionSaved(InstanceConnection),
    ConnectionDeleted(usize),
    ConnectionTestResult { conn_idx: usize, success: bool, latency_ms: u64 },
    InstanceUnregistered(usize),
    
    // SQL Workspace
    QueryResult { tab_id: TabId, result: QueryResultData },
    QueryError { tab_id: TabId, error: String },
    QueryStopped { tab_id: TabId },
    HistoryLoaded(Vec<HistoryEntry>),
    HistorySaved { entry_id: String },
    HistoryDeleted(String),
    HistoryRecalled { entry_id: String, sql: String },
    CommitComplete { tab_id: TabId, success: bool },
    CommitError { tab_id: TabId, error: String },
    ContextPickerDataLoaded(CatalogData),
    CompletionItemsLoaded(Vec<CompletionItem>),
}
```

#### app\_shell update 函数

```rust
// app_shell/update.rs
pub fn update(state: &mut AppShellState, msg: AppMsg) {
    let intent_router = IntentRouter::new();
    let effect_executor = EffectExecutor::instance();
    
    // 收集本帧产生的 Intent 和 Effect
    let mut intents: Vec<AppIntent> = Vec::new();
    let mut effects: Vec<AppEffect> = Vec::new();
    
    match msg {
        // === 处理来自各 Feature 的 Msg ===
        
        // Header
        AppMsg::HeaderMsg(header_msg) => {
            let (new_state, new_intents) = header::update(header_msg, &state.header);
            state.header = new_state;
            intents.extend(new_intents.into_iter().map(AppIntent::Header));
        }
        
        // Discover
        AppMsg::DiscoverMsg(discover_msg) => {
            let (new_state, new_intents, new_effects) = 
                discover::update(discover_msg, &state.discover);
            state.discover = new_state;
            intents.extend(new_intents.into_iter().map(AppIntent::Discover));
            effects.extend(new_effects.into_iter().map(AppEffect::Discover));
        }
        
        // Explorer
        AppMsg::ExplorerMsg(explorer_msg) => {
            let (new_state, new_intents, new_effects) = 
                explorer::update(explorer_msg, &state.explorer);
            state.explorer = new_state;
            intents.extend(new_intents.into_iter().map(AppIntent::Explorer));
            effects.extend(new_effects.into_iter().map(AppEffect::Explorer));
        }
        
        // Instance Workspace
        AppMsg::IwMsg(iw_msg) => {
            let (new_state, new_intents, new_effects) = 
                instance_workspace::update(iw_msg, &state.instance_workspace);
            state.instance_workspace = new_state;
            intents.extend(new_intents.into_iter().map(AppIntent::Iw));
            effects.extend(new_effects.into_iter().map(AppEffect::Iw));
        }
        
        // SQL Workspace
        AppMsg::SqlMsg(sql_msg) => {
            let (new_state, new_intents, new_effects) = 
                sql_workspace::update(sql_msg, &state.sql_workspace);
            state.sql_workspace = new_state;
            intents.extend(new_intents.into_iter().map(AppIntent::Sql));
            effects.extend(new_effects.into_iter().map(AppEffect::Sql));
        }
        
        // === 处理 Effect 执行结果 ===
        AppMsg::EffectResult(effect_result) => {
            // 将 Effect 结果转换为源 Feature 的 Msg
            let source_msg = convert_result_to_msg(effect_result);
            // 递归处理（会路由到正确的 Feature）
            update(state, source_msg);
            return;  // 提前返回，避免重复处理 intents/effects
        }
        
        // === 处理焦点切换 ===
        AppMsg::FocusChanged { zone } => {
            state.focus_zone = zone;
        }
        
        // === 每帧 Tick ===
        AppMsg::Tick => {
            // 1. 更新 perf_monitor（被动计算，不需要 Msg）
            state.perf_monitor = perf_monitor::update(&state.perf_monitor, Instant::now());
            
            // 2. 检查 Effect 结果（非阻塞）
            while let Some(result) = effect_executor.try_recv() {
                let source_msg = convert_result_to_msg(result);
                update(state, source_msg);
            }
        }
        
        AppMsg::Quit => {
            // 退出逻辑
        }
    }
    
    // === 处理所有产生的 Intent ===
    // Intent 会被路由为新的 AppMsg，然后递归处理
    for intent in intents {
        let routed_msgs = intent_router.route(intent);
        for routed_msg in routed_msgs {
            update(state, routed_msg);
        }
    }
    
    // === 处理所有产生的 Effect ===
    // Effect 会被提交给 EffectExecutor 异步执行
    for effect in effects {
        let source = effect_source(&effect);
        effect_executor.submit(effect, source);
    }
}

/// 将 EffectResult 转换为源 Feature 的 Msg
fn convert_result_to_msg(result: EffectResult) -> AppMsg {
    use FeatureId::*;
    use EffectResultData::*;
    
    match (result.source_feature, result.result) {
        // Discover 结果
        (Discover, DiscoverScanComplete(items)) => {
            AppMsg::DiscoverMsg(DiscoverMsg::ScanComplete { items })
        }
        (Discover, DiscoverScanCancelled) => {
            AppMsg::DiscoverMsg(DiscoverMsg::ScanCancelled)
        }
        (Discover, InstancesRegistered(count)) => {
            AppMsg::DiscoverMsg(DiscoverMsg::RegisterComplete { count })
        }
        
        // Explorer 结果
        (Explorer, InstancesLoaded(instances)) => {
            AppMsg::ExplorerMsg(ExplorerMsg::InstancesLoaded { instances })
        }
        (Explorer, ConnectionsLoaded { instance_idx, connections }) => {
            AppMsg::ExplorerMsg(ExplorerMsg::InstanceConnectionsLoaded { 
                instance_idx, connections 
            })
        }
        (Explorer, ObjectsTreeLoaded { database, objects }) => {
            AppMsg::ExplorerMsg(ExplorerMsg::ObjectsTreeLoaded { database, objects })
        }
        (Explorer, InstanceDeleted(idx)) => {
            AppMsg::ExplorerMsg(ExplorerMsg::InstanceDeleted { idx })
        }
        
        // Instance Workspace 结果
        (InstanceWorkspace, InstanceDataLoaded(instance)) => {
            AppMsg::IwMsg(IwMsg::InstanceDataLoaded { instance })
        }
        (InstanceWorkspace, ConnectionSaved(connection)) => {
            AppMsg::IwMsg(IwMsg::ConnectionSaved { connection })
        }
        (InstanceWorkspace, ConnectionDeleted(conn_idx)) => {
            AppMsg::IwMsg(IwMsg::ConnectionDeleted { conn_idx })
        }
        (InstanceWorkspace, ConnectionTestResult { conn_idx, success, latency_ms }) => {
            AppMsg::IwMsg(IwMsg::ConnectionTestResult { 
                conn_idx, success, latency_ms 
            })
        }
        (InstanceWorkspace, InstanceUnregistered(idx)) => {
            AppMsg::IwMsg(IwMsg::UnregisterComplete { success: true })
        }
        
        // SQL Workspace 结果
        (SqlWorkspace, QueryResult { tab_id, result }) => {
            AppMsg::SqlMsg(SqlMsg::QueryResult { tab_id, result })
        }
        (SqlWorkspace, QueryError { tab_id, error }) => {
            AppMsg::SqlMsg(SqlMsg::QueryError { tab_id, error })
        }
        (SqlWorkspace, QueryStopped { tab_id }) => {
            AppMsg::SqlMsg(SqlMsg::QueryStopped { tab_id })
        }
        (SqlWorkspace, HistoryLoaded(history)) => {
            AppMsg::SqlMsg(SqlMsg::HistoryLoaded { history })
        }
        (SqlWorkspace, HistorySaved { entry_id }) => {
            AppMsg::SqlMsg(SqlMsg::HistorySaved { tab_id: None, entry_id })
        }
        (SqlWorkspace, CommitComplete { tab_id, success }) => {
            AppMsg::SqlMsg(SqlMsg::CommitComplete { tab_id, success })
        }
        (SqlWorkspace, CommitError { tab_id, error }) => {
            AppMsg::SqlMsg(SqlMsg::CommitError { tab_id, error })
        }
        (SqlWorkspace, ContextPickerDataLoaded(data)) => {
            AppMsg::SqlMsg(SqlMsg::ContextPickerDataLoaded { data })
        }
        (SqlWorkspace, CompletionItemsLoaded(items)) => {
            AppMsg::SqlMsg(SqlMsg::CompletionItemsLoaded { items })
        }
        
        // 默认：忽略未处理的结果
        _ => AppMsg::Tick,
    }
}
```

#### 主循环集成

```rust
// main.rs
fn main() {
    // 初始化
    let mut state = AppShellState::initial();
    let effect_executor = EffectExecutor::instance();
    
    // 初始化加载（同步加载初始数据）
    load_initial_data(&mut state);
    
    // 主循环
    loop {
        // 1. 每帧 Tick（更新 perf_monitor + 检查 Effect 结果）
        app_shell::update(&mut state, AppMsg::Tick);
        
        // 2. 处理用户输入
        if let Some(input) = read_key() {
            let msg = handle_input(input, &state.focus_zone);
            app_shell::update(&mut state, msg);
        }
        
        // 3. 渲染
        render(&mut state);
        
        // 4. 检查退出
        if should_quit(&state) {
            break;
        }
    }
}
```

#### 消息流处理完整流程

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              消息流处理流程                                  │
└─────────────────────────────────────────────────────────────────────────────┘

1. 用户操作 → Feature.update(msg, state) → (new_state, intents, effects)
    ↓
2. app_shell 收集所有 intents 和 effects
    ↓
3. 处理 Intents（同步路由）：
   ┌─────────────────────────────────────────────────────────────────────┐
   │  for intent in intents {                                            │
   │      routed_msgs = intent_router.route(intent);                     │
   │      for routed_msg in routed_msgs {                                │
   │          update(state, routed_msg);  // 递归处理                     │
   │      }                                                              │
   │  }                                                                  │
   └─────────────────────────────────────────────────────────────────────┘
    ↓
4. 处理 Effects（异步执行）：
   ┌─────────────────────────────────────────────────────────────────────┐
   │  for effect in effects {                                            │
   │      effect_executor.submit(effect, source);  // 提交异步执行       │
   │  }                                                                  │
   │                                                                     │
   │  // 在每帧 Tick 中检查结果：                                         │
   │  while let Some(result) = effect_executor.try_recv() {              │
   │      source_msg = convert_result_to_msg(result);                    │
   │      update(state, source_msg);  // 路由回源 Feature                 │
   │  }                                                                  │
   └─────────────────────────────────────────────────────────────────────┘
    ↓
5. 渲染（基于最新的 state）
    ↓
6. 循环（回到步骤 1）
```

#### 设计要点总结

| 要点             | 说明                                           |
| -------------- | -------------------------------------------- |
| **Feature 解耦** | Feature 不直接通信，通过 Intent → Msg 的转换实现解耦        |
| **Effect 异步**  | Effect 不阻塞主循环，通过 channel 回传结果                |
| **统一入口**       | 所有消息通过 `AppMsg` 进入 `app_shell::update()`     |
| **递归处理**       | Intent 转换后的 Msg 会递归调用 `update()`，支持链式 Intent |
| **结果回传**       | Effect 执行完成后，结果会被转换为源 Feature 的 Msg 并回传      |
| **非阻塞检查**      | 在每帧 Tick 中使用 `try_recv()` 非阻塞检查 Effect 结果    |

#### 与 Feature 分析的集成

| 之前分析的内容                                                       | 在 app\_shell 中的体现                                 |
| ------------------------------------------------------------- | ------------------------------------------------- |
| 所有 Feature 的 `intent.rs`                                      | 被包装为 `AppIntent`，由 `IntentRouter` 路由              |
| 所有 Feature 的 `effect.rs`                                      | 被包装为 `AppEffect`，由 `EffectExecutor` 执行            |
| 各 Feature 的 `update()` 返回 `(State, Vec<Intent>, Vec<Effect>)` | 在 app\_shell 中收集并处理                               |
| 跨 Feature 通信关系图                                               | 由 `IntentRouter` 实现具体的路由规则                        |
| Effect 执行流程                                                   | 由 `EffectExecutor` + `convert_result_to_msg()` 实现 |

#### 扩展性设计

| 扩展场景           | 修改点                                          |
| -------------- | -------------------------------------------- |
| 新增 Feature     | 在 `AppMsg`、`AppIntent`、`AppEffect` 中添加枚举变体   |
| 新增 Intent 路由   | 在 `IntentRouter::route()` 中添加匹配分支            |
| 新增 Effect 执行   | 在 `EffectExecutor::execute_effect()` 中添加匹配分支 |
| 新增 Effect 结果处理 | 在 `convert_result_to_msg()` 中添加匹配分支          |
| 新增 Effect 结果类型 | 在 `EffectResultData` 中添加枚举变体                 |

***

## 五、基础设施模块

### `common/` — 跨 Feature 纯工具库

#### 角色定位

`common` 是**基础设施层**，提供跨 Feature 复用的**纯工具函数、数据结构和系统绑定**。

| 特性                  | 说明                                 |
| ------------------- | ---------------------------------- |
| ❌ **不是 Feature**    | 没有 TEA 循环，没有 Msg/State/Update/View |
| ❌ **不属于任何 Feature** | 不承载业务逻辑，不依赖 Feature 的类型            |
| ✅ **是纯工具库**         | 提供通用的、可复用的基础设施能力                   |

#### 判断标准

| 应该放在 `common`        | 不应该放在 `common`                  |
| -------------------- | ------------------------------- |
| ✅ 无状态的纯函数            | ❌ 有业务语义的状态管理                    |
| ✅ 被 3 个以上 Feature 复用 | ❌ 只被 1-2 个 Feature 使用           |
| ✅ 不依赖任何 Feature      | ❌ 依赖 Feature 的 Msg/State/Intent |
| ✅ 与业务领域无关的通用能力       | ❌ 具体业务领域的逻辑                     |

#### 依赖规则

```
正确的依赖方向：
  Feature A → common ← Feature B
  (Feature 可以依赖 common，common 不能依赖任何 Feature)

禁止的依赖方向：
  common → Feature A  ❌
  Feature A → Feature B  ❌ (跨 Feature 直接依赖被禁止)
```

#### 文件清单

| 文件                   | 类型   | 说明                                                       | 决策依据                                 |
| -------------------- | ---- | -------------------------------------------------------- | ------------------------------------ |
| `editor.rs`          | 系统绑定 | 通用编辑器封装（edtui 绑定）                                        | 纯基础设施，被 sql\_workspace 和 discover 共用 |
| `theme.rs`           | 系统绑定 | 主题/亮度检测                                                  | 系统级绑定，跨所有 feature                    |
| `shortcuts.rs`       | 全局配置 | 全局快捷键定义                                                  | 跨所有 feature 共用                       |
| `text_width.rs`      | 纯函数  | 文本宽度计算                                                   | 纯函数，被所有 view 共用                      |
| `splitter.rs`        | 数据结构 | 分割器逻辑                                                    | 通用数据结构，被所有 splitter 共用               |
| `clipboard.rs`       | 系统绑定 | 剪贴板操作                                                    | 系统级绑定，跨 feature 通用                   |
| `pane_scrollbar.rs`  | 数据结构 | 通用滚动条                                                    | 通用组件，跨 feature 通用                    |
| `scrollable_list.rs` | 数据结构 | 通用滚动列表                                                   | 通用组件，跨 feature 通用                    |
| `overlay_clear.rs`   | 纯函数  | 通用覆盖层清除                                                  | 纯计算，跨 feature 通用                     |
| `format.rs` → 通用部分   | 纯函数  | `cell_display_width`、`truncate_cell_display`、`FIELD_SEP` | 纯函数，跨 feature 通用                     |

#### 与 Feature 的边界示例

| 场景                  | 归属                             | 原因                 |
| ------------------- | ------------------------------ | ------------------ |
| 文本宽度计算              | `common/text_width.rs`         | 纯函数，所有 view 都需要    |
| SQL 格式化（Results 专属） | `sql_workspace/view.rs`        | 只有 Results 使用      |
| 滚动条绘制               | `common/pane_scrollbar.rs`     | 通用组件，多个 Feature 使用 |
| SQL 编辑器行号           | `sql_workspace/sql_tab/editor/view.rs` | 只有编辑器使用            |
| 分割器状态               | `common/splitter.rs`           | 通用数据结构             |
| 树操作快捷键              | `explorer/msg.rs`              | 只有 Explorer 使用     |

**需从 common 移出的文件**（业务语义过重，应归属具体 Feature）:

- `format.rs` → Results 专属部分 → `sql_workspace/view_results.rs`
- `line_numbers.rs` → SQL 编辑器专属 → `sql_workspace/view_editor.rs`
- `row_change_kind.rs` → Results 专属 → `sql_workspace/view_results.rs`

### `epoch.rs` — RenderKey 系统

保持独立，不属于任何 feature。

***

## 六、AppModel 字段归属完整清单

以下是 `app_model.rs` 中所有字段的迁移目标：

| 字段                                 | 目标 Feature           | 说明                    |
| ---------------------------------- | -------------------- | --------------------- |
| `driver`                           | `app_shell`          | 全局数据库驱动               |
| `pool_manager`                     | `app_shell`          | 全局连接池管理               |
| `tree`                             | `explorer`           | ConnectionTreeState   |
| `tree_width`                       | `explorer`           | 树宽度                   |
| `tree_status`                      | `explorer`           | 树状态文本                 |
| `last_results_click`               | `sql_workspace`      | Results 交互追踪          |
| `default_results_row_limit`        | `sql_workspace`      | Results 默认行数          |
| `focus_return`                     | `app_shell`          | 焦点返回目标                |
| `recall_editor_mode`               | `sql_workspace`      | History recall 模式     |
| `mouse_select_editor`              | `sql_workspace`      | 编辑器鼠标选择模式             |
| `mouse_select_detail`              | `sql_workspace`      | Detail 鼠标选择模式         |
| `last_detail_click`                | `sql_workspace`      | Detail 交互追踪           |
| `tabs`                             | `sql_workspace`      | SqlTab 集合             |
| `active_tab`                       | `sql_workspace`      | 当前 Tab                |
| `next_tab_id`                      | `sql_workspace`      | Tab ID 计数器            |
| `discover`                         | `discover`           | DiscoverState         |
| `discover_status`                  | `discover`           | Discover 状态文本         |
| `last_paste_content`               | `discover`           | 粘贴去重                  |
| `discover_hosts`                   | `discover`           | 初始 host 列表            |
| `add_form`                         | `instance_workspace` | AddConnectionForm     |
| `add_connection_status`            | `instance_workspace` | 连接添加状态                |
| `add_connection_status_kind`       | `instance_workspace` | 连接添加状态类型              |
| `default_user`                     | `instance_workspace` | 默认用户名                 |
| `sql_history`                      | `sql_workspace`      | SQL 历史                |
| `last_history_click`               | `sql_workspace`      | History 交互追踪          |
| `last_tree_click`                  | `explorer`           | 树交互追踪                 |
| `last_objects_click`               | `explorer`           | Objects 交互追踪          |
| `pending_objects_schema_apply`     | `explorer`           | Objects pending 操作    |
| `pending_objects_table_open`       | `explorer`           | Objects pending 操作    |
| `last_instance_connection_click`   | `instance_workspace` | 连接交互追踪                |
| `last_instance_form_field_click`   | `instance_workspace` | 表单交互追踪                |
| `last_context_picker_click`        | `sql_workspace`      | ContextPicker 交互追踪    |
| `last_editor_click`                | `sql_workspace`      | 编辑器交互追踪               |
| `pending_open_context_picker`      | `sql_workspace`      | ContextPicker pending |
| `pending_apply_context_picker`     | `sql_workspace`      | ContextPicker pending |
| `pending_picker_preview_db`        | `sql_workspace`      | ContextPicker pending |
| `pending_results_page_action`      | `sql_workspace`      | Results pending       |
| `pending_results_row_limit`        | `sql_workspace`      | Results pending       |
| `pending_results_edit_commit`      | `sql_workspace`      | Results pending       |
| `pending_results_toolbar_action`   | `sql_workspace`      | Results pending       |
| `pending_count_total_rows`         | `sql_workspace`      | Results pending       |
| `pending_results_page_after_count` | `sql_workspace`      | Results pending       |
| `pending_results_ensure_row`       | `sql_workspace`      | Results pending       |
| `results_count_in_progress`        | `sql_workspace`      | Results 状态            |
| `refresh_in_progress`              | `sql_workspace`      | 刷新状态                  |
| `last_refresh_at`                  | `sql_workspace`      | 刷新时间戳                 |
| `refresh_cooldown_until`           | `sql_workspace`      | 冷却时间                  |
| `count_cooldown_until`             | `sql_workspace`      | 冷却时间                  |
| `scan_cooldown_until`              | `discover`           | 扫描冷却时间                |
| `scan_progress_rendered`           | `discover`           | 扫描进度                  |
| `test_connection_cooldown_until`   | `instance_workspace` | 测试连接冷却                |
| `connection_last_tab`              | `sql_workspace`      | 实例→Tab 映射             |
| `meta_cache`                       | `sql_workspace`      | 元数据缓存                 |
| `pending_objects_fetches`          | `explorer`           | Objects pending       |
| `pending_completion_refresh`       | `sql_workspace`      | Completion pending    |
| `content_epoch`                    | `app_shell`          | 全局内容 epoch            |
| `fps`                              | `perf_monitor`       | 帧率                    |
| `redundancy_rate`                  | `perf_monitor`       | 冗余重绘率                 |
| `key_echo_enabled`                 | `app_shell`          | 调试覆盖层开关               |
| `key_echo_log`                     | `app_shell`          | 按键历史日志                |

***

## 七、Nested TEA + Central Message Router 通信规则

本章节详细说明 **Central Message Router** 如何协调 **Nested TEA** 架构中的消息流动。

### 7.1 消息路由流程

```
用户输入事件
    │
    ▼
┌──────────────────────────────────────────────┐
│  Central Message Router (app_shell)          │
│                                              │
│  ① 接收 AppMsg                               │
│  ② 判断当前焦点 Feature                      │
│  ③ 路由 Msg 到目标 Feature                   │
│  ④ 收集所有 Effect                            │
│  ⑤ 执行 Effect (网络/IO/存储)                │
│  ⑥ 处理 Effect 返回的新 Msg                  │
│  ⑦ 更新全局状态 (焦点/模态/节流)             │
└──────────────────────────────────────────────┘
    │
    ▼ (路由到焦点 Feature)
    │
    ├── header::update(HeaderMsg) → (State, Vec<Effect>)
    ├── perf_monitor::update(PerfMsg) → (State, Vec<Effect>)
    ├── global_footer::update(FooterMsg) → (State, Vec<Effect>)
    ├── discover::update(DiscoverMsg) → (State, Vec<Effect>)
    ├── explorer::update(ExplorerMsg) → (State, Vec<Effect>)
    ├── instance_workspace::update(IwMsg) → (State, Vec<Effect>)
    └── sql_workspace::update(SqlMsg) → (State, Vec<Effect>)
```

### 7.2 跨 Feature 通信模式

```
模式 1: Effect 驱动的跨 Feature 更新
───────────────────────────────────
用户点击实例树中的实例
    │
    ▼
ExplorerFeature::update(ExplorerMsg::SelectInstance)
    │
    ├── 返回新的 ExplorerState (高亮选中实例)
    └── 返回 Effect::LoadInstance(instance_id)
    │
    ▼ (Central Message Router 执行 Effect)
    │
app_shell 执行 LoadInstance:
    │
    ├── 查询实例详情
    └── 生成 IwMsg::InstanceLoaded(instance_data)
    │
    ▼ (路由到 InstanceWorkspaceFeature)
    │
InstanceWorkspaceFeature::update(IwMsg::InstanceLoaded)
    │
    └── 更新 IwState 并触发渲染
```

```
模式 2: 直接 Msg 路由 (同一 Feature 内部)
─────────────────────────────────────────
SQL 编辑器内用户按下 Ctrl+Enter
    │
    ▼
SqlWorkspaceFeature::update(SqlMsg::RunQuery)
    │
    ├── 更新 SqlTab 状态 (标记为 running)
    └── 返回 Effect::ExecuteSql(sql, tab_id)
    │
    ▼ (Central Message Router 执行 Effect)
    │
app_shell 执行 ExecuteSql:
    │
    ├── 发送 SQL 到数据库
    └── 生成 SqlMsg::QueryCompleted(results, tab_id)
    │
    ▼ (路由回 SqlWorkspaceFeature)
    │
SqlWorkspaceFeature::update(SqlMsg::QueryCompleted)
    │
    └── 更新 SqlTab.Results 并触发渲染
```

### 7.3 TEA 接口契约

**每个 Feature 必须实现**:

```rust
// 消息类型
pub enum FeatureMsg {
    // 用户交互消息
    UserAction1,
    UserAction2 { param: Type },
    // 内部事件消息 (由 Effect 执行后产生)
    InternalEvent1,
    InternalEvent2 { data: Type },
}

// Intent 类型（跨 Feature 请求）
pub enum FeatureIntent {
    CrossFeatureRequest1 { param: Type },
    CrossFeatureRequest2,
}

// Effect 类型（副作用描述）
pub enum FeatureEffect {
    AsyncOperation1 { param: Type },
    AsyncOperation2,
}

// 状态类型
pub struct FeatureState {
    // 业务数据
    field1: Type1,
    field2: Type2,
    // 视图状态
    cursor: usize,
    scroll: usize,
}

// Update 函数: Msg + State → (NewState, Vec<Intent>, Vec<Effect>)
pub fn update(
    msg: FeatureMsg, 
    state: &FeatureState
) -> (FeatureState, Vec<FeatureIntent>, Vec<FeatureEffect>) {
    // 纯函数: 不执行任何副作用
    // 返回新的状态 + 跨 Feature 请求列表 + 副作用描述列表
}

// View 函数: &State + Rect → 渲染
pub fn view(state: &FeatureState, area: Rect) {
    // 渲染 UI
}
```

**核心规则**:

1. **跨 Feature 通信通过 Router**: Feature 之间不直接通信，所有跨 Feature 操作通过 Central Message Router 中转
2. **Feature 与子 Feature 直接通信**: 父 Feature 通过嵌套 Msg 直接调度子 Feature，不经过 Router
3. **Update 是纯函数**: Feature 的 `update()` 不直接执行副作用（不发请求、不读写文件、不跨 Feature 调用），只返回新 State + Intent（跨 Feature 请求）+ Effect（副作用描述）
4. **单向数据流**: Msg → State → View，禁止 View 直接修改 State
5. **Intent 和 Effect 都通过 Router 处理**: Intent 被 IntentRouter 转换为目标 Feature 的 Msg；Effect 被 EffectExecutor 异步执行后产生 Msg 回传

#### 嵌套通信 vs 跨 Feature 通信

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     嵌套通信（直接传递，不经 Router）                           │
└─────────────────────────────────────────────────────────────────────────────┘

Explorer.update(ExplorerMsg::InstancesMsg(InstancesMsg::ExpandInstance))
    ↓ 直接调用
instances::update(InstancesMsg::ExpandInstance, state.instances)
    ↓ 返回
(InstancesState, Vec<ExplorerIntent>, Vec<ExplorerEffect>)
    ↓ 冒泡到父级
Explorer::update() 收集 Intent 和 Effect

特点：
- 父 Feature 直接 import 子 Feature 的 Msg 类型
- 子 Feature 的 Intent/Effect 冒泡到父 Feature
- 父 Feature 决定如何处理（可能冒泡到 app_shell）

┌─────────────────────────────────────────────────────────────────────────────┐
│                     跨 Feature 通信（通过 Router 中转）                        │
└─────────────────────────────────────────────────────────────────────────────┘

Explorer.update() → 产生 ExplorerIntent::InstanceSelected
    ↓ 提交给 app_shell
app_shell IntentRouter 处理：
    ExplorerIntent::InstanceSelected { idx }
        → AppMsg::IwMsg(IwMsg::LoadInstance { idx })
    ↓ 路由到目标 Feature
InstanceWorkspace::update(IwMsg::LoadInstance { idx })

特点：
- Feature 不 import 其他 Feature 的 Msg/State 类型
- 通过 Intent 枚举解耦
- 由 app_shell 统一路由
```

***

## 八、Nested TEA 迁移执行顺序

每个 Sprint 的目标都是在不破坏现有功能的前提下，逐步将代码迁移到 Nested TEA + Central Message Router 架构。

```
Sprint 1: Nested TEA Feature 骨架搭建
├── 创建 features/ 目录及 7 个 feature 子目录:
│   ├── features/header/
│   ├── features/perf_monitor/
│   ├── features/global_footer/
│   ├── features/discover/
│   ├── features/explorer/
│   ├── features/instance_workspace/
│   └── features/sql_workspace/
├── 创建 app_shell/ 目录 (Central Message Router)
├── 为每个 feature 创建 mod.rs + msg.rs / state.rs / update.rs / view.rs 空骨架
├── 创建 AppMsg 枚举 (app_shell/msg.rs)
├── 确定 pub use 显式导入（移除 pub use *）
└── 验证 cargo check 通过 (空骨架可编译)

Sprint 2: Header + Perf Monitor + Global Footer TEA 化（最小 features，风险最低）
├── 将 Header 重构为 Nested TEA 结构
│   ├── HeaderMsg 枚举 → header/msg.rs
│   ├── HeaderState → header/state.rs (从 components/header.rs 提取)
│   ├── update() 纯函数 → header/update.rs
│   └── view() 渲染 → header/view.rs
├── 将 Perf Monitor 重构为 Nested TEA 结构
│   ├── PerfState → perf_monitor/state.rs (fps, redundancy_rate)
│   ├── update() 纯函数 → perf_monitor/update.rs (fps 计算逻辑)
│   └── view() 渲染 → perf_monitor/view.rs (从 ui.rs fps 渲染提取)
├── 将 Global Footer 重构为 Nested TEA 结构
│   ├── FooterState → global_footer/state.rs
│   ├── update() 纯函数 → global_footer/update.rs
│   └── view() 渲染 → global_footer/view.rs (从 hints.rs 提取)
├── 迁移 components/header.rs → header/state.rs
├── 迁移 hints.rs → global_footer/view.rs
├── 迁移 fps/redundancy → perf_monitor/
├── 拆分 ui.rs 中 Header/Footer 渲染 → 对应 feature
└── 验证 cargo check + cargo test 通过

Sprint 3: Discover Feature TEA 化
├── 将 discover_modal 重构为 Nested TEA 结构
│   ├── DiscoverMsg 枚举 → discover/msg.rs
│   ├── DiscoverState → discover/state.rs
│   ├── update() 纯函数 → discover/update.rs
│   └── view() 渲染 → discover/view.rs (拆分为 view_engine/targets/results)
├── 迁移 discover_modal/draw.rs → discover/view.rs
├── 迁移 discover_modal/interact.rs → discover/update.rs + msg.rs
├── 拆分 hints.rs 中 discover footer → discover/view.rs
├── 删除 discover_modal/ 目录
└── 验证 cargo check + cargo test 通过

Sprint 4: Explorer Feature TEA 化
├── 将连接树重构为 Nested TEA 结构
│   ├── ExplorerMsg 枚举 → explorer/msg.rs
│   ├── ExplorerState → explorer/state.rs
│   ├── update() 纯函数 → explorer/update.rs
│   └── view() 渲染 → explorer/view.rs (拆分为 view_instances + view_objects)
├── 迁移 tree/ + components/instances.rs + components/objects.rs
├── 拆分 ui.rs 中 explorer 渲染 → explorer/view.rs
├── 删除 tree/ 目录
└── 验证 cargo check + cargo test 通过

Sprint 5: Instance Workspace Feature TEA 化
├── 将实例工作台重构为 Nested TEA 结构
│   ├── IwMsg 枚举 → instance_workspace/msg.rs
│   ├── IwState → instance_workspace/state.rs
│   ├── update() 纯函数 → instance_workspace/update.rs
│   └── view() 渲染 → instance_workspace/view.rs (拆分为 view_overview + view_connections)
├── 迁移 instance_workspace/draw.rs + interact.rs
├── 拆分 components/manager.rs + overview.rs
├── 删除 instance_workspace/ 旧文件
└── 验证 cargo check + cargo test 通过

Sprint 6: SQL Workspace Feature TEA 化（最大最复杂）
├── 将 SQL 工作台重构为 Nested TEA 结构
│   ├── SqlMsg 枚举 → sql_workspace/msg.rs
│   ├── SqlWorkspaceState → sql_workspace/state.rs (含 SqlTab 定义)
│   ├── update() 纯函数 → sql_workspace/update.rs
│   └── view() 渲染 → sql_workspace/view.rs (拆分为 view_editor/results/history/detail)
├── 迁移 SqlTab → sql_workspace/state.rs
├── 迁移 results/ + history/ + context_picker/ + sql_completion/
├── 迁移 sql_search.rs + sql_editability.rs
├── 拆分 common/format.rs Results 专属部分
├── 拆分 common/row_change_kind.rs + line_numbers.rs
└── 验证 cargo check + cargo test 通过

Sprint 7: Central Message Router 实现 + App Shell 迁移
├── 实现 app_shell 作为 Central Message Router
│   ├── 消息路由逻辑 → app_shell/update.rs
│   ├── Effect 执行引擎 → app_shell/update.rs
│   ├── 焦点管理 → app_shell/focus.rs
│   ├── 模态管理 → app_shell/state.rs
│   └── key_echo 调试覆盖层 → app_shell/debug_overlay.rs
└── 迁移剩余壳层逻辑
    ├── 迁移 zone_nav.rs → app_shell/focus.rs
    ├── 迁移 session.rs → app_shell/session.rs
    ├── 迁移 key_echo 相关逻辑 → app_shell/
    ├── 迁移 components/focus.rs → app_shell/state.rs
    ├── 清理 app.rs + app_model.rs (最终 Deref 移除)
    └── 验证 cargo check + cargo test 通过

Sprint 8: Shared 抽离 + 最终清理
├── 审计 common/ 真正通用的内容 (跨 feature 且 TTY 无关)
├── 保留: editor.rs, theme.rs, shortcuts.rs, text_width.rs, splitter.rs, clipboard.rs,
│        pane_scrollbar.rs, scrollable_list.rs, overlay_clear.rs
├── 移出: line_numbers.rs → sql_workspace, row_change_kind.rs → sql_workspace,
│         format.rs Results 部分 → sql_workspace
├── 移除 Deref/DerefMut for App
├── 确保所有 view/update 函数接收 feature state (而非 &App)
├── 清理所有 pub use 重导出
├── 最终验证 cargo check + cargo test + clippy
└── 产出 Nested TEA 最终架构文档
```

***

## 九、需确认的决策

| 编号 | 决策项                                                         | 当前建议                 | 需确认 |
| -- | ----------------------------------------------------------- | -------------------- | --- |
| D1 | `components/` 目录是否保留                                        | 不保留，按 feature 拆分     | ☐   |
| D2 | `AppView` 的 `render_epoch` 机制是否保留                           | 保留，作为 view 状态的统一变化检测 | ☐   |
| D3 | `components/shared/search.rs` (PaneSearch) 归属               | → `common/`（通用搜索组件）  | ☐   |
| D4 | `components/shared/scroll.rs` 归属                            | → `common/`（通用滚动组件）  | ☐   |
| D5 | `InstanceWorkspaceState` 是否放入 `instance_workspace/state.rs` | 是                    | ☐   |
| D6 | `MetaCache` 是否属于 `sql_workspace`                            | 是（为 SQL 补全服务）        | ☐   |
| D7 | `driver` + `pool_manager` 是否属于 `app_shell`                  | 是（全局基础设施）            | ☐   |
| D8 | 每个 Sprint 完成后是否合并独立 PR                                      | 是（与项目规范一致）           | ☐   |

***

*文档版本: 2.0 | 创建日期: 2026-08-01 | 基线 commit: a31f366*
*架构模式: Nested TEA (The Elm Architecture) + Central Message Router*
