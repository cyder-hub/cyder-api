---
name: front-design
description: Design and implement Vue admin dashboard UI in this repository with the established minimal light black-white-gray visual system, shared Tailwind/reka-ui components, and mobile-first responsive behavior. Use when creating or updating dashboard pages, CRUD screens, tables, forms, dialogs, navigation, layout shells, or responsive adaptations in `front`, especially for Vue 3 + Tailwind + reka-ui based management interfaces.
---

# Frontend Design Skill

为本仓库 `front` 下的 Vue 管理后台提供统一的视觉和交互规范。目标不是做通用网页，而是做克制、清晰、可维护、适合桌面和移动端的后台界面。

## 1. 设计方向

- 风格：极简、亮色、黑白灰为主，避免花哨装饰。
- 场景：管理后台、CRUD 页面、统计页、配置页、弹窗表单。
- 原则：先保证信息层级、操作可达性、响应式可用性，再谈视觉细节。
- 响应式立场：移动端不是附加补丁，默认按 mobile-first 思考，再向上增强桌面布局。

## 2. 技术与组件约束

### 2.1 基础技术栈

- 框架：`vue` 3、`vue-router`、`pinia`
- 样式：`tailwindcss` v4、`tailwind-merge`、`clsx`
- UI primitive：`reka-ui`
- 图标：`lucide-vue-next`
- 图表：`echarts`、`vue-echarts`

### 2.2 组件目录

- 业务页面：`front/src/pages`
- 布局：`front/src/layouts`
- 全局组件：`front/src/components`
- UI 原子组件：`front/src/components/ui`

不要绕过现有 UI 组件重复造轮子。优先复用：

- `Button`
- `Input`
- `Select`
- `Dialog`
- `Table`
- `Checkbox`
- `Card`
- `Badge`
- `Pagination`
- `Popover`
- `Toast`

## 3. 视觉系统

### 3.1 色彩

默认只使用灰阶，强调色只用于交互态和选中态。

```txt
页面背景: bg-gray-50
卡片/表格/弹窗: bg-white
主要文字: text-gray-900
正文文字: text-gray-600
辅助文字: text-gray-500 / text-gray-400
边框: border-gray-200
弱分隔线: border-gray-100
表头背景: bg-gray-50/80
```

规则：

- 不要做大面积彩色背景。
- 不要引入渐变、发光、玻璃拟态、重阴影。
- 删除/危险态仅在按钮 hover、提示和错误态中使用红色。

### 3.2 阴影与边框

- 卡片、表格、表单区、弹窗优先用边框，不用阴影。
- 常用外框：`border border-gray-200 rounded-lg`
- 弱分隔：`border-t border-gray-100`

### 3.3 排版

- 页面标题：`text-lg font-semibold text-gray-900 tracking-tight sm:text-xl`
- 页面描述：`mt-1 text-sm text-gray-500`
- 区块标题：`text-base font-semibold text-gray-900`
- 表头：`text-xs font-medium text-gray-500 uppercase tracking-wider`
- 正文：`text-sm`
- 技术值：`font-mono text-xs text-gray-600`
- 必填星号：`<span class="text-red-500 ml-0.5">*</span>`

## 4. 移动端与响应式规范

这是当前技能必须覆盖的重点。已落地的全局基线在 `front/src/style.css`，新增页面或重构页面时应优先复用，不要自行发明另一套骨架。

### 4.1 全局基线

已存在的通用类：

- `.app-page`
- `.app-page-shell`
- `.app-page-shell--narrow`
- `.app-section`
- `.app-stack-sm`
- `.app-stack-md`
- `.app-scroll-x`

含义：

- `app-page`：页面外层，负责统一 padding 和 safe area。
- `app-page-shell`：页面内容壳层，负责统一最大宽度和纵向节奏。
- `app-page-shell--narrow`：编辑页等窄内容页面。
- `app-scroll-x`：需要横向滚动的局部容器。

规则：

- 页面不要再直接从 `p-6` 起手，优先使用 `app-page` 和 `app-page-shell`。
- 不要假定 `100vh` 稳定，优先使用现有 `min-h-dvh` / `100dvh` 基线。
- 不要让 `body` 或页面主容器出现无意义横向滚动。

### 4.2 页面头部

统一模式：

```vue
<div class="app-page">
  <div class="app-page-shell">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div class="min-w-0">
        <h1 class="text-lg font-semibold text-gray-900 tracking-tight sm:text-xl">
          标题
        </h1>
        <p class="mt-1 text-sm text-gray-500">描述</p>
      </div>
      <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:items-center">
        <!-- actions -->
      </div>
    </div>
  </div>
</div>
```

规则：

- 小屏下标题区和操作区默认纵向堆叠。
- 主操作按钮在小屏下允许独占一行，不要强行横排。
- 长标题或描述必须允许截断或换行，不要写死单行布局。

### 4.3 导航

当前布局基线：

- 桌面端：侧栏，可折叠。
- 移动端：顶部栏 + 抽屉导航 + 遮罩。

规则：

- 修改主布局时必须同时考虑桌面和移动端两套表现。
- 移动端顶部栏要保证页面标题、菜单按钮、语言切换可达。
- 抽屉打开时要考虑遮罩、路由切换自动关闭、`body` 滚动锁定。

### 4.4 表单响应式

- 单列优先，桌面再增强双列。
- 已有 `grid grid-cols-2 gap-4` 的表单，响应式改造时优先改成 `grid grid-cols-1 sm:grid-cols-2 gap-4`。
- Select 触发器在表单中显式加 `class="w-full"`。
- Checkbox / 开关项放入带边框容器：
  `flex items-center justify-between p-3.5 border border-gray-200 rounded-lg`

### 4.5 表格与列表

- 桌面端可以保留表格。
- 移动端如果列很多，不要只依赖横向滚动，应优先考虑卡片化。
- 仍需保留表格时，用带边框容器包裹，并确保滚动只发生在局部。

基础样式：

- 外层：`border border-gray-200 rounded-lg overflow-hidden`
- 表头行：`bg-gray-50/80 hover:bg-gray-50/80`
- 操作列：`text-right`

### 4.6 弹窗

- 简单弹窗：`max-w-lg`
- 复杂弹窗：`max-w-4xl`
- 移动端弹窗必须考虑：
  - 全宽边距
  - 最大高度
  - 内容区独立滚动
  - 底部操作按钮稳定可见

### 4.7 安全区

项目已经引入 safe area 变量与 `viewport-fit=cover`。在移动端顶部栏、底部按钮区、全屏弹层中，优先复用现有基线，不要硬编码设备特例。

## 5. 组件使用规范

### 5.1 Button

- 页面主操作：`variant="outline"`，通常配 Lucide 图标
- 保存：`variant="default"`
- 取消：`variant="ghost" class="text-gray-600"`
- 表格编辑：`variant="ghost" size="sm"`
- 表格删除：`variant="ghost" size="sm" class="text-gray-400 hover:text-red-600"`

图标尺寸：

- 常规按钮：`h-4 w-4 mr-1.5`
- 表格操作：`h-3.5 w-3.5 mr-1`

### 5.2 Badge

- 状态/枚举：`variant="secondary"` + `font-mono text-xs`
- 次级标签：`variant="outline"` + `text-xs`

### 5.3 Checkbox

- 禁止原生 `<input type="checkbox">`
- 统一使用 `@/components/ui/checkbox`

### 5.4 空状态

- 外层：`flex flex-col items-center justify-center py-20`
- 图标：`h-10 w-10 stroke-1 text-gray-400`
- 文案：`text-sm font-medium text-gray-500`

### 5.5 加载状态

- 外层：`flex items-center justify-center py-16`
- 图标：`<Loader2 class="h-5 w-5 animate-spin" />`

## 6. 推荐页面模板

### 6.1 CRUD 页面

优先使用 `CrudPageLayout.vue`，不要重复实现页面外层、标题区和状态区。

### 6.2 自定义页面

当页面不适合 `CrudPageLayout` 时，至少复用：

- `app-page`
- `app-page-shell`
- 移动端优先的标题区布局

## 7. 当前仓库中的已知方向

在本仓库里设计或改页面时，优先遵守这几个方向：

- 已完成：
  - 全局移动端 spacing / safe area / `dvh` 基线
  - 默认布局的移动端顶部栏与抽屉导航
  - 页面级统一骨架与标题区模式
- 后续仍会继续加强：
  - UI 原子组件的移动端触控尺寸
  - 弹窗滚动与底部操作区
  - 多列表格页的移动端卡片化
  - 复杂编辑页的单列化与区块重排

这意味着：

- 新增页面应直接站在“移动端已是一级约束”的基础上设计。
- 不要再新增只适合桌面端的大宽度、固定高度、横向挤压布局。

## 8. 禁止事项

1. 不要使用阴影作为主要层次手段。
2. 不要硬编码中文文案，必须走 i18n。
3. 不要引入新的高饱和品牌色体系。
4. 不要绕过现有 UI primitive 直接写原生替代品。
5. 不要默认写死 `p-6`、`max-w-4xl mx-auto` 作为页面骨架，除非与 `app-page-shell--narrow` 等基线兼容。
6. 不要把移动端适配理解为“桌面完成后补几个 `sm:` / `md:`”。
7. 不要在复杂多列表格上只加 `overflow-x-auto` 就算完成移动端支持；先评估是否应改成卡片列表。
