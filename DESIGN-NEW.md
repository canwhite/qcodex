---
version: alpha
name: GEO Blue-Purple Dashboard
description: GEO operations dashboard system extracted from GEO_Blue_Purple_Preview.html. Cool white and blue-grey surfaces carry dense work; indigo selections and blue-to-purple gradients identify active work, key actions, and brand elements.

colors:
  primary: "#4F46E5"
  on-primary: "#FFFFFF"
  ink: "#202840"
  body: "#525D73"
  mute: "#737D91"
  faint: "#9AA3B5"
  canvas: "#F6F7FC"
  canvas-elevated: "#FFFFFF"
  hairline: "#E5E9F2"
  hairline-soft: "#F0F2F8"
  link: "#4F46E5"
  link-deep: "#245DDB"
  link-soft: "#EEEDFF"
  success: "#15803D"
  error: "#C53B4C"
  error-deep: "#A62D3D"
  warning: "#B7791F"
  warning-soft: "#FFF7E6"
  blue: "#3068E8"
  blue-mid: "#5268ED"
  violet: "#8656E8"
  purple: "#7040D9"
  active-rail: "#7266EF"
  focus: "#BBB0F5"
  brand-gradient: "linear-gradient(110deg, #3068E8 0%, #5268ED 55%, #8656E8 100%)"
  action-gradient: "linear-gradient(110deg, #245DDB, #7040D9)"
  metric-gradient: "linear-gradient(115deg, #EDF4FF, #F3EDFF)"

typography:
  font-sans: Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif
  font-mono: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace
  display-xl: { fontFamily: "{typography.font-sans}", fontSize: 32px, fontWeight: 650, lineHeight: 40px, letterSpacing: -0.6px }
  heading-lg: { fontFamily: "{typography.font-sans}", fontSize: 26px, fontWeight: 650, lineHeight: 34px, letterSpacing: -0.6px }
  heading-md: { fontFamily: "{typography.font-sans}", fontSize: 20px, fontWeight: 650, lineHeight: 28px, letterSpacing: -0.4px }
  body-lg: { fontFamily: "{typography.font-sans}", fontSize: 16px, fontWeight: 400, lineHeight: 24px, letterSpacing: 0 }
  body-md: { fontFamily: "{typography.font-sans}", fontSize: 14px, fontWeight: 400, lineHeight: 20px, letterSpacing: 0 }
  body-sm: { fontFamily: "{typography.font-sans}", fontSize: 12px, fontWeight: 400, lineHeight: 16px, letterSpacing: 0 }
  label: { fontFamily: "{typography.font-sans}", fontSize: 14px, fontWeight: 600, lineHeight: 20px, letterSpacing: 0 }
  mono: { fontFamily: "{typography.font-mono}", fontSize: 12px, fontWeight: 500, lineHeight: 16px, letterSpacing: 0 }

rounded:
  sm: 8px
  md: 10px
  card: 14px
  brand: 9px
  pill: 9999px

spacing:
  xxs: 4px
  xs: 8px
  sm: 10px
  md: 12px
  lg: 16px
  xl: 21px
  2xl: 26px
  3xl: 28px

shadows:
  control: "0 1px 3px rgb(40 55 110 / 0.04)"
  card: "0 2px 10px rgb(40 55 110 / 0.035)"
  primary: "0 3px 8px #5954df1c"
  primary-hover: "0 4px 12px #5954df30"

components:
  app-shell: { backgroundColor: "{colors.canvas}", textColor: "{colors.ink}", typography: "{typography.body-md}" }
  sidebar: { backgroundColor: "{colors.canvas-elevated}", borderColor: "{colors.hairline}", padding: "26px 12px" }
  topbar: { backgroundColor: "rgb(255 255 255 / 0.94)", borderColor: "{colors.hairline}", padding: "{spacing.md} {spacing.xl}" }
  nav-item: { textColor: "{colors.body}", typography: "{typography.body-md}", rounded: "{rounded.sm}", padding: "11px 12px" }
  nav-item-active: { backgroundColor: "{colors.link-soft}", textColor: "{colors.primary}", activeRailColor: "{colors.active-rail}", typography: "{typography.label}" }
  card: { backgroundColor: "{colors.canvas-elevated}", textColor: "{colors.ink}", borderColor: "{colors.hairline}", rounded: "{rounded.card}", padding: "{spacing.xl}", shadow: "{shadows.card}" }
  primary-button: { background: "{colors.action-gradient}", textColor: "{colors.on-primary}", rounded: "{rounded.sm}", padding: "10px 14px", shadow: "{shadows.primary}" }
  secondary-button: { backgroundColor: "{colors.canvas-elevated}", textColor: "{colors.ink}", borderColor: "{colors.hairline}", rounded: "{rounded.sm}", padding: "10px 14px" }
  input: { backgroundColor: "{colors.canvas-elevated}", textColor: "{colors.ink}", borderColor: "{colors.hairline}", focusOutline: "2px solid {colors.focus}", rounded: "{rounded.sm}", padding: "10px 12px" }
  metric-card-primary: { background: "{colors.metric-gradient}", borderColor: "#DEDFFA", valueColor: "#4F46C8" }
  blue-badge: { backgroundColor: "{colors.link-soft}", borderColor: "#DDD8FC", textColor: "#5546C4" }
  blue-notice: { backgroundColor: "#F4F2FF", borderColor: "#DFDAFC", textColor: "#5142A0" }
---

## Overview

GEO 蓝紫主题面向日常运营，不是营销落地页。冷白与蓝灰承载高密度信息；靛蓝表示当前选择、链接和关键操作；蓝紫渐变只用于品牌、主操作和进度。界面应安静、可信、易扫描：通过 `#F6F7FC` 画布、白色卡片、细边框建立层次，避免大面积彩色表面。

- 冷白画布 `#F6F7FC`，白卡片，深蓝灰文字。
- 主色 `#4F46E5` 用于当前导航、激活标签、链接、关键数值。
- 主按钮、品牌标志、进度条使用蓝→紫渐变。
- 左侧导航当前项具备淡紫底、主色文字、3px 靛蓝导轨。
- 卡片 14px 圆角，细边框，轻蓝灰阴影。

## Colors

### Foundation

| Token | Value | Use |
|---|---|---|
| `{colors.canvas}` | `#F6F7FC` | 页面工作区 |
| `{colors.canvas-elevated}` | `#FFFFFF` | 侧栏、卡片、输入框、弹窗 |
| `{colors.hairline}` | `#E5E9F2` | 卡片、控件、分割线 |
| `{colors.hairline-soft}` | `#F0F2F8` | 次级分隔、静态底色 |
| `{colors.ink}` | `#202840` | 标题、数字、主内容 |
| `{colors.body}` | `#525D73` | 正文、普通交互文本 |
| `{colors.mute}` | `#737D91` | 元信息、表头、辅助说明 |

### Brand and states

| Token | Value | Use |
|---|---|---|
| `{colors.primary}` | `#4F46E5` | 激活项、链接、主要状态 |
| `{colors.link-soft}` | `#EEEDFF` | 激活项、标签、蓝色徽标底 |
| `{colors.active-rail}` | `#7266EF` | 左导航导轨、标签底边 |
| `{colors.focus}` | `#BBB0F5` | 控件焦点描边 |
| `{colors.brand-gradient}` | `#3068E8 → #5268ED → #8656E8` | 品牌、进度、区块强调 |
| `{colors.action-gradient}` | `#245DDB → #7040D9` | 主按钮 |
| `{colors.metric-gradient}` | `#EDF4FF → #F3EDFF` | 单张核心指标卡 |

`{colors.success}` 用于完成或健康状态；`{colors.error}` 用于失败和危险操作；`{colors.warning}` 用于待确认和风险。状态色不可代替清晰文案。

## Typography

使用 `Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif`。代码、序号与紧凑技术标签使用系统等宽字体。

| Token | Size / line height | Weight | Use |
|---|---:|---:|---|
| `{typography.display-xl}` | 32 / 40px | 650 | 页面核心指标、大标题 |
| `{typography.heading-lg}` | 26 / 34px | 650 | 页面、向导标题 |
| `{typography.heading-md}` | 20 / 28px | 650 | 卡片、模块标题 |
| `{typography.body-md}` | 14 / 20px | 400 | 默认 UI 文本、表格内容 |
| `{typography.body-sm}` | 12 / 16px | 400 | 注释、元信息 |
| `{typography.label}` | 14 / 20px | 600 | 导航当前项、按钮、表单标签 |
| `{typography.mono}` | 12 / 16px | 500 | 序号、数据标签、日志标识 |

仅大标题使用负字距：`display-xl`、`heading-lg` 为 `-0.6px`；`heading-md` 为 `-0.4px`。正文保持默认字距，避免中文拥挤。

## Layout and elevation

- 应用外壳：左侧固定导航 + 顶部栏 + 内容工作区。侧栏为白色，工作区为 `{colors.canvas}`。
- 内容模块间保持 16–28px 间距；标准卡片内边距为 21px。
- 桌面卡片网格为 2–4 列；小屏降为单列。表格允许横向滚动，不能压缩数据列。
- 行 hover 使用 `#FAFAFF`，不使用强色块。
- 常规组件使用 1px `{colors.hairline}`；卡片使用 `{shadows.card}`；主按钮使用 `{shadows.primary}`。不使用重投影或发光。

## Components

### Navigation

**Sidebar**：白色表面、`#E5E9F2` 边界、顶部内边距 26px。品牌标志为 30px 方形、9px 圆角，使用 `brand-gradient`。普通导航项 `11px 12px`、8px 圆角；当前项用 `#EEEDFF`、`#4F46E5` 和左侧 3px `#7266EF` 导轨。hover 只用 `#F4F3FC`。

**Tabs**：当前项用主色文字和 `#7266EF` 底边；内容区标签可使用淡紫底与 `#DCD7FB` 边框。

### Cards and metrics

**Standard card**：白底、1px `{colors.hairline}`、14px 圆角、21px 内边距、`{shadows.card}`。标题 650 字重；正文 `{colors.body}`；表头/元信息 `{colors.mute}`。

**Primary metric card**：仅首页或模块首项使用 `metric-gradient`，边框 `#DEDFFA`，数值 `#4F46C8`。同组最多一张。

**Section entry**：标准卡片顶部增加 30×4px `brand-gradient` 短条。hover 改为 `#C7C2FA` 边框、`#FFFFFF → #FAF9FF` 背景，阴影轻微加深。

### Buttons and controls

**Primary button**：`action-gradient`、白字、8px 圆角、`10px 14px`。hover 使用 `#1D50C5 → #6032C1` 与 `{shadows.primary-hover}`。每个视图只保留一个主路径。

**Secondary / icon button**：白底、`{colors.hairline}` 边框、深蓝灰字、8px 圆角；hover 用 `#F5F3FF` 与 `#D9D3FA`。

**Inputs**：白底、1px `#E5E9F2`、8px 圆角、`10px 12px`。焦点为 2px `#BBB0F5` outline，offset 2px。

### Status

**Blue badge / run state**：`#EEEDFF` 背景、`#DDD8FC` 边框、`#5546C4` 文字。

**Blue notice**：`#F4F2FF` 背景、`#DFDAFC` 边框、`#5142A0` 文字。用于可恢复流程、异步任务提示。

**Error action**：使用 `#C53B4C` 作为边线、图标或文字；危险操作必须保留明确文字。

## Do's and don'ts

### Do

- 使用冷白底、白卡片、蓝灰文本组织运营信息。
- 让主色集中表达当前选择、链接、焦点和主路径。
- 只让主按钮、品牌、进度条使用强渐变。
- 激活导航同时提供背景、文字色和导轨，不能仅图标变色。
- 数据密集区依靠行高、间距、细边框，而非大型色块。

### Don't

- 不要把蓝紫渐变铺满页面、所有卡片或所有指标。
- 不要用成功/错误色替代主操作色，或用颜色替代状态文字。
- 不要给白卡片使用重阴影、深色描边。
- 不要给正文添加负字距。
- 不要在单个视图放置多个同等强调的主按钮。
