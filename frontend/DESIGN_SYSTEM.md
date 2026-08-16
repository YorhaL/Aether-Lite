# Aether 设计系统 v2.3

> 基于 shadcn/ui 和书本纸张主题的完整前端设计规范

**版本**: 2.3.0
**最后更新**: 2025-11-18

---

## 概述

本文档描述了 Aether 前端项目的设计系统，基于 shadcn/ui 和自定义主题构建。所有组件均已实现并在生产环境中使用。

### 核心理念

1. **一致性优先** - 所有组件遵循统一的视觉语言和交互模式
2. **响应式设计** - 组件自适应不同屏幕尺寸（移动端、平板、桌面）
3. **可访问性** - 遵循 WCAG 2.1 标准，支持键盘导航和屏幕阅读器
4. **性能优化** - 轻量级组件，按需加载，优化渲染性能
5. **开发体验** - TypeScript 类型安全，清晰的 API 设计，完善的文档

### 色彩体系

项目使用书本纸张主题色：

- **book-cloth** - 书籍封面布料色 (#cc785c / #d4a27f)
- **kraft** - 牛皮纸色 (#b97847 / #c9a26f)
- **manilla** - 马尼拉纸色 (#e8ddc5 / #d4c5a9)
- **cloud** - 云白色 (#f5f3ed / #2a2723)

详细配置见 [src/config/theme.ts](src/config/theme.ts)

---

## 技术栈

- **Vue 3** - Composition API
- **TypeScript** - 类型安全
- **Tailwind CSS** - 原子化 CSS
- **shadcn/ui** - 基础组件库
- **lucide-vue-next** - 图标库
- **Vite** - 构建工具

---

## 主题系统

### 主题配置

主题配置位于 [src/config/theme.ts](src/config/theme.ts)，包含：

```ts
export const theme = {
  colors: themeColors,      // 颜色系统
  spacing,                  // 间距系统（基于 8px 网格）
  radius,                   // 圆角系统
  shadows,                  // 阴影系统
  typography,               // 字体系统
  animations,               // 动画系统
  breakpoints,              // 响应式断点
  zIndex,                   // 层级管理
  components: componentDefaults  // 组件默认配置
}
```

### CSS 变量

全局 CSS 变量定义在 `src/assets/index.css`，使用 HSL 色彩空间：

```css
:root {
  --background: 0 0% 100%;
  --foreground: 20 14.3% 4.1%;
  --primary: 15 55% 58%;
  --border: 20 5.9% 90%;
  --muted: 60 4.8% 95.9%;
  --muted-foreground: 25 5.3% 44.7%;
  /* ... 更多变量 */
}

.dark {
  --background: 20 14.3% 4.1%;
  --foreground: 0 0% 95%;
  --primary: 15 45% 68%;
  /* ... 暗色模式变量 */
}
```

---

## 组件库

### 基础组件 (shadcn/ui)

所有基础组件位于 [src/components/ui/](src/components/ui/)：

#### 布局组件
- **Card** - 卡片容器
  - 变体：`default`、`outline`、`ghost`、`interactive`
- **Separator** - 分隔线（水平/垂直）
- **Tabs** - 选项卡容器

#### 表单组件
- **Button** - 按钮
  - 变体：`default`、`destructive`、`outline`、`secondary`、`ghost`、`link`
  - 大小：`sm`、`md`、`lg`、`icon`
- **Input** - 输入框
- **Textarea** - 多行文本框
- **Select** - 下拉选择框
- **Checkbox** - 复选框
- **Switch** - 开关
- **Label** - 表单标签

#### 反馈组件
- **Badge** - 徽章标签
- **Skeleton** - 骨架屏
- **Toast** - 消息提示
- **Dialog** - 对话框/模态框
- **Alert** - 警告提示

#### 数据展示
- **Table** 系列 - 表格组件
  - Table、TableHeader、TableBody、TableRow、TableHead、TableCell
- **Avatar** - 头像
- **Progress** - 进度条

---

### 布局组件 (Layout Components)

位于 [src/components/layout/](src/components/layout/)，所有组件支持从 `@/components/layout` 统一导入：

```ts
import { PageHeader, PageContainer, Section, CardSection, Grid, StatCard } from '@/components/layout'
```

#### PageHeader

页面头部组件，支持标题、描述、图标和操作按钮。

**使用示例：**

```vue
<script setup lang="ts">
import { PageHeader } from '@/components/layout'
import { Settings } from 'lucide-vue-next'
</script>

<template>
  <PageHeader
    title="系统设置"
    description="管理系统级别的配置和参数"
    :icon="Settings"
  >
    <template #actions>
      <Button @click="save">保存配置</Button>
    </template>
  </PageHeader>
</template>
```

**Props:**
- `title: string` - 页面标题(必填)
- `description?: string` - 页面描述
- `icon?: Component` - 图标组件

**Slots:**
- `icon` - 自定义图标区域
- `actions` - 右侧操作按钮

---

#### PageContainer

页面容器，提供响应式的最大宽度和内边距。

**使用示例：**

```vue
<template>
  <PageContainer maxWidth="2xl" padding="md">
    <!-- 页面内容 -->
  </PageContainer>
</template>
```

**Props:**
- `maxWidth?: 'sm' | 'md' | 'lg' | 'xl' | '2xl' | 'full'` - 最大宽度(默认: '2xl')
- `padding?: 'none' | 'sm' | 'md' | 'lg'` - 内边距(默认: 'md')

---

#### Section

区块容器，用于分隔页面不同区域。

**使用示例：**

```vue
<template>
  <Section
    title="用户信息"
    description="管理用户基本资料"
    spacing="md"
  >
    <template #actions>
      <Button size="sm">编辑</Button>
    </template>

    <!-- 区块内容 -->
  </Section>
</template>
```

**Props:**
- `title?: string` - 区块标题
- `description?: string` - 区块描述
- `spacing?: 'none' | 'sm' | 'md' | 'lg'` - 底部间距(默认: 'md')

**Slots:**
- `header` - 自定义头部
- `actions` - 右侧操作按钮
- `default` - 主内容

---

#### CardSection

卡片区块，基于 Card 组件的增强版。

**使用示例：**

```vue
<template>
  <CardSection
    title="系统配置"
    description="配置系统默认参数"
    variant="elevated"
    padding="lg"
  >
    <template #actions>
      <Button size="sm" variant="ghost">重置</Button>
    </template>

    <template #footer>
      <Button>保存</Button>
    </template>

    <!-- 卡片内容 -->
  </CardSection>
</template>
```

**Props:**
- `title?: string` - 卡片标题
- `description?: string` - 卡片描述
- `variant?: 'default' | 'elevated' | 'glass'` - 卡片样式(默认: 'default')
- `padding?: 'none' | 'sm' | 'md' | 'lg'` - 内边距(默认: 'md')

**Slots:**
- `header` - 自定义头部
- `actions` - 头部右侧操作
- `default` - 主内容
- `footer` - 底部内容

---

#### Grid

响应式网格布局。

**使用示例：**

```vue
<template>
  <Grid :cols="{ sm: 1, md: 2, lg: 3 }" gap="md">
    <Card>项目 1</Card>
    <Card>项目 2</Card>
    <Card>项目 3</Card>
  </Grid>
</template>
```

---

#### StatCard

统计卡片，用于展示关键指标。

**使用示例：**

```vue
<script setup lang="ts">
import { StatCard } from '@/components/layout'
import { Users } from 'lucide-vue-next'
</script>

<template>
  <StatCard
    title="总用户数"
    :value="1234"
    :icon="Users"
    trend="up"
    :trendValue="12.5"
    trendLabel="较上月"
  />
</template>
```

---

#### ShellHeader (待废弃)

旧版页面头部组件，建议迁移到 `PageHeader`。

---

### 业务组件 (Common Components)

位于 [src/components/common/](src/components/common/)：

#### 1. PageLayout

页面布局容器，集成标题、筛选、分页等功能。

**使用示例：**

```vue
<script setup lang="ts">
import PageLayout from '@/components/common/PageLayout.vue'
import DataTable from '@/components/common/DataTable.vue'

const searchQuery = ref('')
const currentPage = ref(1)
const pageSize = ref(20)
const roleFilter = ref('')
</script>

<template>
  <PageLayout
    title="用户管理"
    subtitle="管理系统用户和权限"
    :showFilters="true"
    :showPagination="true"
    v-model:searchQuery="searchQuery"
    v-model:currentPage="currentPage"
    v-model:pageSize="pageSize"
    :total="totalUsers"
    spacing="normal"
    maxWidth="full"
  >
    <template #toolbar>
      <Button @click="openAddDialog">添加用户</Button>
    </template>

    <template #filters>
      <Select v-model="roleFilter">
        <SelectTrigger><SelectValue placeholder="角色筛选" /></SelectTrigger>
        <SelectContent>
          <SelectItem value="admin">管理员</SelectItem>
          <SelectItem value="user">普通用户</SelectItem>
        </SelectContent>
      </Select>
    </template>

    <DataTable :columns="columns" :data="users" />
  </PageLayout>
</template>
```

**主要 Props：**

| 属性 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `title` | `string` | - | 页面标题（必填） |
| `subtitle` | `string` | - | 页面副标题 |
| `showHeader` | `boolean` | `true` | 是否显示页面头部 |
| `showBackButton` | `boolean` | `false` | 是否显示返回按钮 |
| `maxWidth` | `'sm' \| 'md' \| 'lg' \| 'xl' \| 'full'` | `'full'` | 内容最大宽度 |
| `spacing` | `'tight' \| 'normal' \| 'relaxed'` | `'normal'` | 内容间距 |
| `showFilters` | `boolean` | `false` | 是否显示筛选栏 |
| `showPagination` | `boolean` | `false` | 是否显示分页 |

**主要 Slots：**

- `toolbar` - 页面右上角工具栏
- `headerExtra` - 头部额外内容
- `filters` - 筛选条件
- `filterLeft` / `filterRight` - 筛选栏左右插槽
- `default` - 主内容区
- `footer` - 页面底部

---

#### 2. DataTable

响应式数据表格，桌面端显示表格，移动端自动切换为卡片视图。

**使用示例：**

```vue
<script setup lang="ts">
import DataTable, { type DataTableColumn } from '@/components/common/DataTable.vue'
import StatusBadge from '@/components/common/StatusBadge.vue'

const columns: DataTableColumn[] = [
  {
    key: 'name',
    label: '名称',
    sortable: true,
    width: '200px',
    showOnMobile: true
  },
  {
    key: 'email',
    label: '邮箱',
    align: 'left',
    showOnMobile: true
  },
  {
    key: 'status',
    label: '状态',
    align: 'center',
    showOnMobile: true
  },
  {
    key: 'created_at',
    label: '创建时间',
    formatter: (value) => new Date(value).toLocaleDateString(),
    showOnMobile: false
  },
  {
    key: 'actions',
    label: '操作',
    align: 'right',
    showOnMobile: true
  }
]

const handleRowClick = (row, index) => {
  console.log('点击行:', row)
}

const handleSort = (sortBy, sortOrder) => {
  // 处理排序
}
</script>

<template>
  <DataTable
    :columns="columns"
    :data="tableData"
    :loading="loading"
    :clickable="true"
    rowKey="id"
    @rowClick="handleRowClick"
    @sort="handleSort"
  >
    <template #cell-status="{ value }">
      <StatusBadge :status="value" />
    </template>

    <template #cell-actions="{ row }">
      <div class="flex gap-2">
        <Button size="sm" variant="outline" @click.stop="editRow(row)">
          编辑
        </Button>
        <Button size="sm" variant="destructive" @click.stop="deleteRow(row)">
          删除
        </Button>
      </div>
    </template>

    <template #mobile-card="{ row }">
      <!-- 自定义移动端卡片布局 -->
      <div class="p-4 space-y-2">
        <div class="font-semibold">{{ row.name }}</div>
        <div class="text-sm text-muted-foreground">{{ row.email }}</div>
        <StatusBadge :status="row.status" />
      </div>
    </template>

    <template #empty>
      <EmptyState
        type="empty"
        title="暂无用户"
        description="点击右上角按钮添加第一个用户"
      />
    </template>
  </DataTable>
</template>
```

**主要 Props：**

| 属性 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `columns` | `DataTableColumn[]` | - | 列配置（必填） |
| `data` | `T[]` | - | 数据源（必填） |
| `rowKey` | `string` | `'id'` | 行唯一标识字段 |
| `loading` | `boolean` | `false` | 是否加载中 |
| `clickable` | `boolean` | `false` | 是否可点击行 |
| `emptyTitle` | `string` | `'暂无数据'` | 空状态标题 |

**列配置（DataTableColumn）：**

```ts
interface DataTableColumn<T = any> {
  key: string                     // 列标识（对应数据字段）
  label: string                   // 列标题
  width?: string                  // 列宽度
  align?: 'left' | 'center' | 'right'  // 对齐方式
  sortable?: boolean              // 是否可排序
  formatter?: (value: any, row: T, index: number) => string  // 值格式化
  headerClass?: string            // 表头样式类
  cellClass?: string              // 单元格样式类
  showOnMobile?: boolean          // 是否在移动端显示（默认 true）
}
```

**主要 Events：**

- `rowClick(row, index)` - 行点击事件
- `sort(sortBy, sortOrder)` - 排序事件

**主要 Slots：**

- `cell-{key}` - 自定义单元格内容（接收 `{ row, column, index, value }` 参数）
- `mobile-card` - 自定义移动端卡片布局（接收 `{ row, index }` 参数）
- `empty` - 自定义空状态
- `footer` - 表格底部内容

---

#### 3. SearchInput

智能搜索输入框，支持防抖、清除、建议列表。

**使用示例：**

```vue
<script setup lang="ts">
const searchQuery = ref('')
const searchSuggestions = ['用户名', '邮箱', 'ID']
const searching = ref(false)

const handleSearch = async (value: string) => {
  searching.value = true
  try {
    await performSearch(value)
  } finally {
    searching.value = false
  }
}
</script>

<template>
  <SearchInput
    v-model="searchQuery"
    placeholder="搜索用户名、邮箱..."
    :suggestions="searchSuggestions"
    :loading="searching"
    :debounce="500"
    size="md"
    @search="handleSearch"
    @clear="searchQuery = ''"
  />
</template>
```

**主要 Props：**

| 属性 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `modelValue` | `string` | - | 输入值（必填） |
| `placeholder` | `string` | `'搜索...'` | 占位符 |
| `clearable` | `boolean` | `true` | 是否显示清除按钮 |
| `loading` | `boolean` | `false` | 是否显示加载图标 |
| `size` | `'sm' \| 'md' \| 'lg'` | `'md'` | 大小 |
| `suggestions` | `string[]` | `[]` | 搜索建议列表 |
| `debounce` | `number` | `300` | 防抖延迟（毫秒） |

---

#### 4. FilterBar

筛选栏容器，集成搜索和筛选条件。

**使用示例：**

```vue
<template>
  <FilterBar
    v-model:searchQuery="searchQuery"
    :showSearch="true"
    :hasActiveFilters="hasFilters"
    searchPlaceholder="搜索..."
    @searchChange="handleSearch"
    @reset="resetFilters"
  >
    <template #filters>
      <Select v-model="statusFilter">
        <SelectTrigger class="w-36">
          <SelectValue placeholder="状态" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">全部</SelectItem>
          <SelectItem value="active">活跃</SelectItem>
          <SelectItem value="inactive">禁用</SelectItem>
        </SelectContent>
      </Select>

      <Select v-model="roleFilter">
        <SelectTrigger class="w-36">
          <SelectValue placeholder="角色" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">全部</SelectItem>
          <SelectItem value="admin">管理员</SelectItem>
          <SelectItem value="user">用户</SelectItem>
        </SelectContent>
      </Select>
    </template>
  </FilterBar>
</template>
```

---

#### 5. Pagination

分页组件，支持页码导航和每页数量选择。

**使用示例：**

```vue
<script setup lang="ts">
const currentPage = ref(1)
const pageSize = ref(20)
const totalRecords = ref(1000)

const loadData = () => {
  // 重新加载数据
}
</script>

<template>
  <Pagination
    v-model:currentPage="currentPage"
    v-model:pageSize="pageSize"
    :total="totalRecords"
    :showPageSizeSelector="true"
    :pageSizeOptions="[10, 20, 50, 100]"
    @pageChange="loadData"
    @pageSizeChange="loadData"
  />
</template>
```

**主要 Props：**

| 属性 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `currentPage` | `number` | `1` | 当前页码 |
| `pageSize` | `number` | `20` | 每页显示数量 |
| `total` | `number` | `0` | 总记录数 |
| `showPageSizeSelector` | `boolean` | `true` | 是否显示页面大小选择器 |
| `pageSizeOptions` | `number[]` | `[10, 20, 50, 100]` | 每页数量选项 |

---

#### 6. EmptyState

空状态组件，支持多种类型和自定义内容。

**使用示例：**

```vue
<script setup lang="ts">
import { RefreshCw, Plus } from 'lucide-vue-next'
</script>

<template>
  <!-- 搜索无结果 -->
  <EmptyState
    type="search"
    title="未找到结果"
    description="尝试使用不同的关键词搜索"
    actionText="清空筛选"
    :actionIcon="RefreshCw"
    @action="resetSearch"
  />

  <!-- 筛选无结果 -->
  <EmptyState
    type="filter"
    title="无匹配结果"
    description="没有符合当前筛选条件的数据"
    actionText="重置筛选"
    @action="resetFilters"
  />

  <!-- 空数据 -->
  <EmptyState
    type="empty"
    size="lg"
    title="暂无用户"
    description="点击下方按钮添加第一个用户"
    actionText="添加用户"
    :actionIcon="Plus"
    actionVariant="default"
    @action="openAddDialog"
  />

  <!-- 加载错误 -->
  <EmptyState
    type="error"
    title="加载失败"
    description="数据加载过程中出现错误，请稍后重试"
    actionText="重新加载"
    @action="retry"
  />
</template>
```

**类型（type）：**

- `default` - 默认空状态
- `search` - 搜索无结果
- `filter` - 筛选无结果
- `error` - 加载错误
- `empty` - 空空如也
- `notFound` - 未找到资源

**主要 Props：**

| 属性 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `type` | `EmptyStateType` | `'default'` | 空状态类型 |
| `title` | `string` | - | 标题（自动根据类型设置） |
| `description` | `string` | - | 描述（自动根据类型设置） |
| `size` | `'sm' \| 'md' \| 'lg'` | `'md'` | 大小 |
| `actionText` | `string` | - | 操作按钮文本 |
| `actionIcon` | `Component` | - | 操作按钮图标 |
| `actionVariant` | `ButtonVariant` | `'default'` | 按钮变体 |

---

#### 7. StatusBadge

状态徽章组件，用于显示不同状态。

**使用示例：**

```vue
<template>
  <!-- 成功状态 -->
  <StatusBadge status="success" label="已激活" :showIcon="true" />

  <!-- 错误状态 -->
  <StatusBadge status="error" label="已禁用" variant="solid" />

  <!-- 警告状态 -->
  <StatusBadge status="warning" label="待审核" variant="soft" />

  <!-- 信息状态 -->
  <StatusBadge status="info" label="处理中" />

  <!-- 待处理 -->
  <StatusBadge status="pending" label="排队中" />

  <!-- 活跃 -->
  <StatusBadge status="active" label="在线" />

  <!-- 未激活 -->
  <StatusBadge status="inactive" label="离线" />
</template>
```

**状态类型（status）：**

| 状态 | 颜色 | 图标 | 用途 |
|------|------|------|------|
| `success` | 绿色 | CheckCircle2 | 成功、完成、已激活 |
| `error` | 红色 | XCircle | 错误、失败、已禁用 |
| `warning` | 黄色 | AlertCircle | 警告、待审核、需注意 |
| `info` | 蓝色 | Info | 信息、提示 |
| `pending` | 灰色 | Clock | 待处理、排队中 |
| `neutral` | 灰色 | Minus | 中性状态 |
| `active` | 主题色 | CheckCircle2 | 活跃、在线 |
| `inactive` | 灰色 | Minus | 未激活、离线 |

**变体（variant）：**

- `solid` - 实心背景
- `soft` - 柔和背景（默认）
- `outline` - 描边样式

---

#### 8. LoadingState

加载状态组件，支持多种加载样式。

**使用示例：**

```vue
<template>
  <!-- 旋转加载器 -->
  <LoadingState
    variant="spinner"
    message="加载中，请稍候..."
    size="md"
  />

  <!-- 骨架屏 -->
  <LoadingState
    variant="skeleton"
    size="lg"
    :fullHeight="true"
  />

  <!-- 脉冲点 -->
  <LoadingState
    variant="pulse"
    message="正在加载数据..."
  />
</template>
```

**变体（variant）：**

- `spinner` - 旋转加载器（默认）
- `skeleton` - 骨架屏
- `pulse` - 脉冲点动画

---

#### 9. ConfirmButton

带确认对话框的按钮组件,简化危险操作的确认流程。

**使用示例:**

```vue
<script setup lang="ts">
import { ConfirmButton } from '@/components/common'
import { Trash2 } from 'lucide-vue-next'

const handleDelete = async () => {
  await deleteItem()
  console.log('删除成功')
}
</script>

<template>
  <!-- 危险操作确认 -->
  <ConfirmButton
    variant="destructive"
    :icon="Trash2"
    confirm-type="danger"
    confirm-title="确认删除"
    confirm-message="此操作不可撤销，确定要删除吗?"
    @confirmed="handleDelete"
  >
    删除
  </ConfirmButton>

  <!-- 警告确认 -->
  <ConfirmButton
    confirm-type="warning"
    confirm-title="重置配置"
    confirm-message="将重置所有配置到默认值"
    @confirmed="resetConfig"
  >
    重置
  </ConfirmButton>

  <!-- 无需确认直接执行 -->
  <ConfirmButton
    :require-confirm="false"
    @click="handleClick"
  >
    普通按钮
  </ConfirmButton>
</template>
```

**主要 Props:**

| 属性 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `text` | `string` | - | 按钮文本 |
| `variant` | `ButtonVariant` | `'default'` | 按钮样式 |
| `size` | `ButtonSize` | `'md'` | 按钮大小 |
| `icon` | `Component` | - | 图标组件 |
| `disabled` | `boolean` | `false` | 是否禁用 |
| `confirmTitle` | `string` | `'确认操作'` | 确认对话框标题 |
| `confirmMessage` | `string` | `'确定要执行此操作吗?'` | 确认消息 |
| `confirmType` | `'default' \| 'danger' \| 'warning'` | `'default'` | 确认类型 |
| `requireConfirm` | `boolean` | `true` | 是否需要确认 |

**Events:**
- `click` - 不需要确认时触发
- `confirmed` - 确认后触发
- `cancelled` - 取消确认时触发

---

#### 10. ActionMenu

操作菜单下拉组件,用于集中展示多个操作选项。

**使用示例:**

```vue
<script setup lang="ts">
import { ActionMenu, type ActionMenuItem } from '@/components/common'
import { Edit, Copy, Trash, MoreVertical } from 'lucide-vue-next'

const menuItems: ActionMenuItem[] = [
  {
    label: '编辑',
    icon: Edit,
    onClick: () => handleEdit()
  },
  {
    label: '复制',
    icon: Copy,
    onClick: () => handleCopy()
  },
  { separator: true },
  {
    label: '删除',
    icon: Trash,
    variant: 'destructive',
    onClick: async () => {
      await handleDelete()
    }
  }
]
</script>

<template>
  <ActionMenu
    :items="menuItems"
    :trigger-icon="MoreVertical"
    trigger-variant="ghost"
    placement="bottom-end"
  />
</template>
```

**ActionMenuItem 接口:**

```ts
interface ActionMenuItem {
  label?: string          // 菜单项标签
  icon?: Component        // 图标
  badge?: string | number // 徽章
  variant?: 'default' | 'destructive'  // 样式变体
  disabled?: boolean      // 是否禁用
  separator?: boolean     // 是否为分隔线
  onClick?: () => void | Promise<void>  // 点击回调
}
```

**主要 Props:**

| 属性 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `items` | `ActionMenuItem[]` | - | 菜单项列表(必填) |
| `triggerText` | `string` | - | 触发按钮文本 |
| `triggerIcon` | `Component` | - | 触发按钮图标 |
| `triggerVariant` | `ButtonVariant` | `'outline'` | 触发按钮样式 |
| `triggerSize` | `ButtonSize` | `'sm'` | 触发按钮大小 |
| `showChevron` | `boolean` | `true` | 是否显示下拉箭头 |
| `placement` | `'bottom-start' \| 'bottom-end' \| 'top-start' \| 'top-end'` | `'bottom-end'` | 菜单位置 |

---

## 工具函数 (Composables)

位于 [src/composables/](src/composables/)

### useBreakpoints

响应式断点检测，用于实现响应式布局。

```ts
import { useBreakpoints } from '@/composables/useBreakpoints'

const {
  windowWidth,    // 窗口宽度
  isSm,          // >= 640px
  isMd,          // >= 768px
  isLg,          // >= 1024px
  isXl,          // >= 1280px
  is2Xl,         // >= 1536px
  current,       // 当前断点 'xs' | 'sm' | 'md' | 'lg' | 'xl' | '2xl'
  isMobile,      // < 768px
  isTablet,      // 768px ~ 1024px
  isDesktop      // >= 1024px
} = useBreakpoints()

// 示例：根据屏幕大小显示不同内容
<div v-if="isMobile">移动端视图</div>
<div v-else-if="isTablet">平板视图</div>
<div v-else>桌面视图</div>
```

---

### useToast

消息提示管理，统一的 Toast 通知接口。

```ts
import { useToast } from '@/composables/useToast'

const { success, error, warning, info } = useToast()

// 成功消息（5秒后自动消失）
success('操作成功')
success('数据保存成功', '提示')

// 错误消息（8秒后自动消失）
error('操作失败')
error('保存失败，请检查网络连接', '错误')

// 警告消息（8秒后自动消失）
warning('该操作可能影响其他数据', '警告')

// 信息消息（5秒后自动消失）
info('系统将在 5 分钟后进行维护', '系统通知')
```

**接口定义：**

```ts
interface UseToast {
  toasts: Ref<Toast[]>
  success(message: string, title?: string): string
  error(message: string, title?: string): string
  warning(message: string, title?: string): string
  info(message: string, title?: string): string
  showToast(options: Omit<Toast, 'id'>): string
  removeToast(id: string): void
  clearAll(): void
}

interface Toast {
  id: string
  title?: string
  message?: string
  variant?: 'success' | 'error' | 'warning' | 'info'
  duration?: number
}
```

---

### useConfirm

确认对话框，用于危险操作确认。

```ts
import { useConfirm } from '@/composables/useConfirm'

const { confirm, confirmDanger, confirmWarning } = useConfirm()

// 普通确认
const ok = await confirm('确定要删除吗？', '确认删除')
if (ok) {
  await deleteItem()
}

// 危险操作确认（红色按钮）
const ok = await confirmDanger(
  '此操作不可撤销，确定继续吗？',
  '删除确认'
)

// 警告确认（黄色主题）
const ok = await confirmWarning(
  '该操作可能影响其他用户，是否继续？',
  '警告'
)
```

---

### useClasses

类名工具函数，简化条件类名的生成。

```ts
import { useClasses } from '@/composables/useClasses'

const { cn, conditional, fromObject } = useClasses()

// 合并类名（过滤 falsy 值）
const className = cn(
  'base-class',
  isActive && 'active',
  error && 'error',
  'another-class'
)
// 结果: 'base-class active another-class' (假设 isActive=true, error=false)

// 条件类名
const className = conditional(isActive, 'bg-primary', 'bg-muted')
// 结果: isActive ? 'bg-primary' : 'bg-muted'

// 从对象生成类名
const className = fromObject({
  'text-red-500': hasError,
  'font-bold': isImportant,
  'underline': isLink
})
// 结果: 只包含值为 true 的键
```

---

## 最佳实践

### 1. 组件开发规范

#### 使用 TypeScript

```vue
<script setup lang="ts">
interface Props {
  title: string
  count?: number
  items?: string[]
}

interface Emits {
  (e: 'update', value: string): void
  (e: 'delete', id: string): void
}

const props = withDefaults(defineProps<Props>(), {
  count: 0,
  items: () => []
})

const emit = defineEmits<Emits>()
</script>
```

#### 遵循命名规范

- **组件文件**: PascalCase （如 `UserCard.vue`、`DataTable.vue`）
- **Composables**: camelCase + `use` 前缀 （如 `useAuth.ts`、`useBreakpoints.ts`）
- **工具函数**: camelCase （如 `formatDate.ts`、`validateEmail.ts`）
- **常量**: SCREAMING_SNAKE_CASE （如 `API_BASE_URL`、`MAX_FILE_SIZE`）

#### 合理使用插槽

```vue
<template>
  <Card>
    <!-- 具名插槽 + 默认内容 -->
    <template #header>
      <slot name="header">
        <h3 class="text-lg font-semibold">默认标题</h3>
      </slot>
    </template>

    <!-- 默认插槽 -->
    <slot>默认内容</slot>

    <!-- 作用域插槽 -->
    <template #footer>
      <slot name="footer" :count="items.length" :total="total" />
    </template>
  </Card>
</template>
```

#### 统一错误处理

```ts
import { useToast } from '@/composables/useToast'
import { apiClient } from '@/api/client'

const { error: showError, success: showSuccess } = useToast()

async function saveData() {
  try {
    await apiClient.post('/users', userData)
    showSuccess('用户创建成功')
  } catch (err: any) {
    const message = err.response?.data?.detail || err.message || '操作失败'
    showError(message, '错误')
    console.error('Failed to create user:', err)
  }
}
```

---

### 2. 样式规范

#### 优先使用 Tailwind 类

```vue
<template>
  <div class="flex items-center gap-4 p-6 rounded-lg bg-card border border-border hover:shadow-md transition-shadow">
    <Avatar :src="user.avatar" />
    <div class="flex-1 min-w-0">
      <h3 class="text-lg font-semibold truncate">{{ user.name }}</h3>
      <p class="text-sm text-muted-foreground">{{ user.role }}</p>
    </div>
    <Badge :variant="user.active ? 'success' : 'neutral'">
      {{ user.active ? '在线' : '离线' }}
    </Badge>
  </div>
</template>
```

#### 使用主题 CSS 变量

```vue
<template>
  <div class="custom-card">
    <!-- 内容 -->
  </div>
</template>

<style scoped>
.custom-card {
  background-color: hsl(var(--card));
  color: hsl(var(--card-foreground));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius);
}

.custom-card:hover {
  background-color: hsl(var(--muted));
}
</style>
```

#### 响应式设计

```vue
<template>
  <!-- 移动端 1 列，平板 2 列，桌面 3 列 -->
  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
    <Card v-for="item in items" :key="item.id">
      {{ item.name }}
    </Card>
  </div>

  <!-- 使用 composable 实现条件渲染 -->
  <div v-if="isMobile">移动端布局</div>
  <div v-else>桌面端布局</div>
</template>

<script setup lang="ts">
import { useBreakpoints } from '@/composables/useBreakpoints'

const { isMobile } = useBreakpoints()
</script>
```

---

### 3. 性能优化

#### 按需导入组件

```ts
import { defineAsyncComponent } from 'vue'

// 异步加载重型组件
const HeavyChart = defineAsyncComponent(() =>
  import('./components/HeavyChart.vue')
)

// 带加载状态的异步组件
const HeavyTable = defineAsyncComponent({
  loader: () => import('./components/HeavyTable.vue'),
  loadingComponent: LoadingState,
  delay: 200,
  errorComponent: ErrorState,
  timeout: 3000
})
```

#### 使用 v-memo 优化列表

```vue
<template>
  <div
    v-for="item in largeList"
    :key="item.id"
    v-memo="[item.updated_at, item.status]"
  >
    <!-- 仅当 updated_at 或 status 变化时重新渲染 -->
    <ItemCard :item="item" />
  </div>
</template>
```

#### 虚拟滚动

对于超过 100 条的列表，使用虚拟滚动：

```bash
npm install vue-virtual-scroller
```

```vue
<script setup lang="ts">
import { RecycleScroller } from 'vue-virtual-scroller'
import 'vue-virtual-scroller/dist/vue-virtual-scroller.css'
</script>

<template>
  <RecycleScroller
    :items="items"
    :item-size="64"
    key-field="id"
    v-slot="{ item }"
  >
    <UserCard :user="item" />
  </RecycleScroller>
</template>
```

---

### 4. 可访问性 (a11y)

#### 语义化 HTML

```vue
<template>
  <nav aria-label="主导航">
    <ul role="list">
      <li><RouterLink to="/dashboard">仪表盘</RouterLink></li>
      <li><RouterLink to="/users">用户管理</RouterLink></li>
    </ul>
  </nav>

  <main>
    <h1>页面标题</h1>
    <section aria-labelledby="section-title">
      <h2 id="section-title">区块标题</h2>
      <!-- 内容 -->
    </section>
  </main>
</template>
```

#### 键盘导航

```vue
<template>
  <button
    @click="handleClick"
    @keydown.enter="handleClick"
    @keydown.space.prevent="handleClick"
    :aria-label="buttonLabel"
  >
    <Icon :name="iconName" aria-hidden="true" />
    {{ text }}
  </button>

  <!-- 可聚焦的非按钮元素 -->
  <div
    role="button"
    tabindex="0"
    @click="handleAction"
    @keydown.enter="handleAction"
    @keydown.space.prevent="handleAction"
  >
    自定义按钮
  </div>
</template>
```

#### ARIA 属性

```vue
<template>
  <!-- 对话框 -->
  <div
    role="dialog"
    aria-labelledby="dialog-title"
    aria-describedby="dialog-description"
    aria-modal="true"
  >
    <h2 id="dialog-title">对话框标题</h2>
    <p id="dialog-description">对话框描述</p>
  </div>

  <!-- 表单 -->
  <form>
    <label for="username">用户名</label>
    <input
      id="username"
      type="text"
      aria-required="true"
      aria-invalid="false"
      aria-describedby="username-error"
    />
    <span id="username-error" role="alert" aria-live="polite">
      <!-- 错误信息 -->
    </span>
  </form>
</template>
```

---

## 组件迁移检查清单

### 已完全迁移到 shadcn 的页面

- [x] Dashboard.vue
- [x] Users.vue
- [x] Settings.vue
- [x] SystemSettings.vue
- [x] Profile.vue
- [x] ActivityLogs.vue
- [x] Announcements.vue
- [x] ApiKeys.vue
- [x] MyApiKeys.vue
- [x] Usage.vue
- [x] ProviderList.vue
- [x] MyProviders.vue
- [x] CacheMonitoring.vue
- [x] ProviderDetailNew.vue

### 部分迁移或自定义样式

- [ ] Home.vue - 使用大量自定义动画和样式（不建议迁移）

---

## 更新日志

### v2.3.0 (2025-11-18)

**新增组件:**
- `ConfirmButton` - 带确认对话框的按钮组件
- `ActionMenu` - 操作菜单下拉组件

**优化导入系统:**
- 创建 `@/components/ui/index.ts` 统一导出所有 shadcn UI 组件
- 完善 `@/components/layout/index.ts` 和 `@/components/common/index.ts`
- 支持更简洁的组件导入方式

**导入方式优化:**

```ts
// 旧版导入 (繁琐)
import Button from '@/components/ui/button.vue'
import Input from '@/components/ui/input.vue'
import Card from '@/components/ui/card.vue'

// 新版导入 (推荐)
import { Button, Input, Card } from '@/components/ui'
import { PageHeader, Section, CardSection } from '@/components/layout'
import { DataTable, ConfirmButton, ActionMenu } from '@/components/common'
```

**文档:**
- 添加 `ConfirmButton` 和 `ActionMenu` 组件完整文档
- 更新组件导入最佳实践

### v2.2.0 (2025-11-18)

**重构:**
- 统一布局组件目录: 将 `layout-v2` 合并到 `layout`
- 所有布局组件现在从 `@/components/layout` 统一导入
- 删除冗余的 `layout-v2` 目录

**文档:**
- 添加完整的布局组件文档和使用示例
- 标记 `ShellHeader` 为待废弃组件

**迁移指南:**
```ts
// 旧版导入 (已废弃)
import { PageHeader } from '@/components/layout-v2'

// 新版导入 (推荐)
import { PageHeader } from '@/components/layout'
```

### v2.1.0 (2025-11-18)

**新增：**
- 完善所有组件文档和使用示例
- 添加完整的 TypeScript 类型定义
- 统一 Toast 工具函数接口（useToast）
- 完善响应式支持（useBreakpoints）

**修复：**
- 修复 CacheMonitoring.vue 的 toast 调用
- 统一所有页面的组件使用

**文档：**
- 更新所有组件的使用示例
- 添加最佳实践章节
- 添加性能优化指南
- 添加可访问性指南

### v2.0.0 (2025-11-17)

- 基础设计系统搭建
- 实现所有核心业务组件
- 建立主题系统

---

## 参考资源

- [shadcn/ui 官方文档](https://ui.shadcn.com/)
- [Tailwind CSS 文档](https://tailwindcss.com/docs)
- [Vue 3 文档](https://vuejs.org/)
- [Lucide Icons](https://lucide.dev/)
- [WCAG 2.1 标准](https://www.w3.org/WAI/WCAG21/quickref/)
- [主题配置文件](src/config/theme.ts)
