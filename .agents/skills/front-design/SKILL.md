---
name: front-design
description: Frontend design guidelines — minimal, light, black-and-white design language for the Vue admin dashboard.
---

# Frontend Design Skill

本项目前端采用 **极简主义 · 亮色 · 黑白灰** 的统一设计语言，适用于所有管理后台页面。

---

## 1. 设计理念

| 原则         | 说明                                                          |
| ------------ | ------------------------------------------------------------- |
| **极简**     | 去掉一切不必要的装饰：无阴影、无渐变、无彩色背景              |
| **黑白为主** | 色阶限制在 gray-50 ~ gray-900，仅在少量交互态使用主题色       |
| **亮色风格** | 页面背景 `bg-gray-50`，卡片/表格背景 `bg-white`，永远保持明亮 |
| **留白充足** | 组件间用 `space-y-6`，表单项用 `space-y-4`，避免拥挤          |
| **层级清晰** | 用字重（`font-semibold` / `font-medium`）和字号区分信息层级   |

## 2. 项目依赖与组件库

### 2.1 基础依赖项

本项目前端基于 Vue 3 生态构建，主要依赖项如下：

- **核心框架**: `vue` (v3.5.x), `vue-router`, `pinia`
- **图标库**: `lucide-vue-next` (所有图标均来自此库，禁止内联 SVG)
- **UI 基础**: `radix-vue`, `reka-ui`, `class-variance-authority` (CVA)
- **样式**: `tailwindcss` (v4), `tailwind-merge`, `clsx`
- **表格**: `@tanstack/vue-table` (用于复杂表格逻辑)
- **图表**: `echarts`, `vue-echarts`

### 2.2 已安装的 UI 组件 (shadcn-vue)

所有 UI 组件均位于 `vue/src/components/ui` 目录下。目前已安装以下组件：

- **Badge**: 标签/徽章 (`@/components/ui/badge`)
- **Button**: 按钮 (`@/components/ui/button`)
- **Card**: 卡片容器 (`@/components/ui/card`)
- **Checkbox**: 复选框 (`@/components/ui/checkbox`)
- **Dialog**: 弹窗/对话框 (`@/components/ui/dialog`)
- **Input**: 输入框 (`@/components/ui/input`)
- **Label**: 表单标签 (`@/components/ui/label`)
- **Pagination**: 分页组件 (`@/components/ui/pagination`)
- **Popover**: 气泡卡片 (`@/components/ui/popover`)
- **Select**: 选择器 (`@/components/ui/select`)
- **Table**: 表格基础组件 (`@/components/ui/table`)
- **Toast**: 通知提示系统 (`@/components/ui/toast`)

## 3. 颜色规范

```
背景:        bg-gray-50 (页面) / bg-white (卡片/表格/弹窗)
表头背景:     bg-gray-50/80
主要文字:     text-gray-900
次要文字:     text-gray-600
辅助文字:     text-gray-500 / text-gray-400
边框:        border-gray-200
分隔线:      border-gray-100
```

> [!IMPORTANT]
> 不要使用 `blue-600`、`indigo-500` 等彩色做大面积背景。主题色仅出现在：主按钮 (`variant="default"`)、Checkbox 激活态、侧边栏选中态。

## 4. 排版规范

| 元素        | 样式                                                          |
| ----------- | ------------------------------------------------------------- |
| 页面标题 h1 | `text-lg font-semibold text-gray-900 tracking-tight`          |
| 页面描述    | `text-sm text-gray-500 mt-1`                                  |
| 表头文字    | `text-xs font-medium text-gray-500 uppercase tracking-wider`  |
| 表格文字    | `text-sm`（默认），主列加 `font-medium text-gray-900`         |
| 等宽文字    | `font-mono text-xs text-gray-600`（字段名、API Key 等技术值） |
| 弹窗标题    | `text-lg font-semibold text-gray-900`                         |
| 表单 Label  | 使用 `<Label>` 组件，加 `text-gray-700`                       |
| 必填项标识  | 在 Label 内侧加 `<span class="text-red-500 ml-0.5">*</span>`  |
| 等宽 Input  | `font-mono text-sm`（用于字段名、JSON、数值等技术输入框）     |

## 5. 组件使用规范

### 按钮

| 场景       | 变体                                                                        |
| ---------- | --------------------------------------------------------------------------- |
| 页面主操作 | `variant="outline"`，搭配 Lucide 图标                                       |
| 弹窗保存   | `variant="default"`                                                         |
| 弹窗取消   | `variant="ghost" class="text-gray-600"`                                     |
| 表格编辑   | `variant="ghost" size="sm"` + 图标                                          |
| 表格删除   | `variant="ghost" size="sm" class="text-gray-400 hover:text-red-600"` + 图标 |

### 表格

- 外层容器: `border border-gray-200 rounded-lg overflow-hidden`
- **不要使用 `shadow-md` 或任何阴影**
- 表头行: 加 `class="bg-gray-50/80 hover:bg-gray-50/80"`（覆盖默认 hover）
- 操作列: 右对齐 `text-right`

### Badge

- 枚举值（类型、状态）: `variant="secondary"` + `font-mono text-xs`
- 位置/类别: `variant="outline"` + `text-xs`

### Checkbox

- **禁止使用原生 `<input type="checkbox">`**
- 统一使用 `@/components/ui/checkbox` 的 `<Checkbox>` 组件
- 绑定方式: `:checked` + `@update:checked`

### 表单

- Label 与 Input 间距: `space-y-1.5`
- 表单项间距: `space-y-4`
- 并排字段用 `grid grid-cols-2 gap-4`
- 下拉选择 (Select): 业务侧使用时需显式添加 `class="w-full"` 以便在表单网格中铺满对齐。
- 开关/Checkbox 选项用带边框容器: `flex items-center justify-between p-3.5 border border-gray-200 rounded-lg`，Label 加 `cursor-pointer`
- footer 上边框: `border-t border-gray-100 pt-4 mt-2`

### 空状态

- 居中布局 `flex flex-col items-center justify-center py-20`
- Lucide 图标 `h-10 w-10 stroke-1 text-gray-400`
- 描述文字 `text-sm font-medium text-gray-500`

### 加载状态

- 居中布局 `flex items-center justify-center py-16`
- 使用 `<Loader2 class="h-5 w-5 animate-spin" />` + 文字

## 6. 图标

- 统一使用 `lucide-vue-next` (项目已安装此依赖，可直接 import 使用)
- 按钮内图标: `h-4 w-4 mr-1.5`
- 表格操作图标: `h-3.5 w-3.5 mr-1`
- 空状态图标: `h-10 w-10 stroke-1`

## 7. 页面布局模板

```vue
<template>
  <div class="p-6 space-y-6">
    <!-- 页面头部 -->
    <div class="flex justify-between items-start">
      <div>
        <h1 class="text-lg font-semibold text-gray-900 tracking-tight">标题</h1>
        <p class="mt-1 text-sm text-gray-500">描述文字</p>
      </div>
      <Button variant="outline">
        <Plus class="h-4 w-4 mr-1.5" />
        添加
      </Button>
    </div>

    <!-- 表格区域 -->
    <div class="border border-gray-200 rounded-lg overflow-hidden">
      <Table>
        <!-- ... -->
      </Table>
    </div>
  </div>
</template>
```

## 8. 注意事项

1. **禁止使用阴影** (`shadow-*`) 在表格和卡片上
2. **禁止使用原生 checkbox/radio**，统一使用 UI 组件
3. **枚举值必须用 Badge** 展示，不要直接输出纯文本
4. **所有文案必须走 i18n**，不要硬编码中文
5. **弹窗宽度** 统一 `max-w-lg`（简单表单）或 `max-w-4xl`（复杂表单）
6. **颜色** 不要随意引入新的彩色，保持黑白灰色阶
7. **操作列右对齐**，操作按钮之间不需要额外间距（ghost 按钮自带 padding）
8. **页面头部** 始终包含标题 + 描述文字 + 主操作按钮的三栏布局
9. **全局样式清理**：初始化 Vite/Vue 时，务必清理 `src/style.css` 等文件中自带的全局样式（如 `body { display: flex }` 和 `h1 { font-size: 3.2em }`），避免覆盖 Tailwind 的预设并导致布局和字体大小异常。
