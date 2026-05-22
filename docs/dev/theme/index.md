# 主题开发

NodeGet 主题本质上为纯静态构建的网站，可以部署到 Cloudflare Pages / GitHub Pages / 腾讯 EdgeOne 等静态文件托管服务。

你可以自由地选择自己熟悉的前端技术来构建自己的主题，也可以在官方主题的基础上进行修改

为了与大部分 NodeGet 公开探针页面兼容，我们推荐使用下面的规范主题结构框架

## NodeGet 规范主题结构

如果只是为了实现公开信息的展示，那么只要利用受限权限的token获取监控数据并作出展示UI即可，这是一个最小的可行的主题方案。

然而，为了让主题开发者能够相互协作，使开发流程更加规范化，NodeGet设计了一个较为通用的主题结构框架。为了方便起见，称其为NodeGet规范主题。

一个典型的 NodeGet 规范主题包含有：

- 一个 **nodeget-theme.json** 文件，储存主题的基本元信息
- 一个 **nodeget-files.json** 文件，储存文件列表
- config.json，储存用户定义配置
- custom.css/custom.js 用户自定义的样式和脚本
- 大量的静态资源

比如，官方的[演示主题](https://github.com/NodeSeekDev/NodeGet-StatusShow)编译结果的整体结构如下：

```
├── assets
│   ├── WorldMap-Bce51mDJ.js
│   ├── index-D0NSy5wV.js
│   └── index-DbsPOOS5.css
├── index.html
├── logo.png
├── world.geo.json
├── linux-logo-icon
│   ├── alpinelinux-icon.svg
│   ├── archlinux.svg
│   ...
│   
├── download.html // 辅助用
│
│   // 下面的5个文件非常重要
├── nodeget-theme.json
├── nodeget-theme-files.json
├── config.json
├── custom.css
└── custom.js

```

下面，将介绍下主题所涉及的重要的5个文件

### nodeget-theme.json

储存主题的元信息，是一个 NodeGet 主题里面相对固定的部分

```jsonc
{
  "name": "NodeGet Basic Theme",
  "short": "NodeGetBasic",  // 这里的名字是唯一的，可以理解为ID
  "description": "NodeGet Basic Theme - a boilerplate for custom themes", // 描述
  "author": "NodeSeekDev",
  "repository": "https://github.com/NodeSeekDev/NodeGet-StatusShow",
  "dist_page": "https://nodeget.pages.dev",
  // 后台面板生成界面的地方，根据界面表单的结果来覆盖config.json中的user_preferences
  "user_preferences_form": {
    "version": "0.0.1",
    "items": [
      {
        "key": "site_name",
        "name": "站点标题",
        "type": "string",
        "default": "NodeGet Status",
        "help": "输入站点标题"
      },
      {
        "key": "site_logo",
        "name": "站点图标",
        "type": "string",
        "default": "",
        "help": "输入站点图标链接，留空则使用默认"
      },
      {
        "key": "footer",
        "name": "页脚文本",
        "type": "string",
        "default": "Powered by NodeGet",
        "help": "输入页脚文本"
      }
    ]
  },
  "version": "1.4.1",
  "license": ""
}
```

### config.json

主题网页打开后，会自动从该文件里面获取连接后端的关键信息和用户配置偏好

当用户拿到主题编译的结果后，只要修改这个文件就可以上传到任意的静态托管服务

在 cloudflare pages 等静态文件托管服务中，可以关联GitHub repo并利用环境变量自动覆盖生成的结果

```json
{
  // user_preferences_form对应的结果
  "user_preferences": {
    "site_name": "NodeGet Status",
    "site_logo": "",
    "footer": "Powered by NodeGet"
  },
  // token信息
  "site_tokens": [
    {
      "name": "master server node 1",
      "backend_url": "wss://your-backend.example.com",
      "token": "YOUR_TOKEN_HERE"
    }
  ]
}
```

### nodeget-theme-files.json

这个文件是主题编译结果的所有文件的文件名，主要用于控制面板自动加载主题文件

一般来说不需要用户手动记录，会在编译时自动生成，例如官方主题演示在postbuild阶段[自动生成](https://github.com/NodeSeekDev/NodeGet-StatusShow/blob/main/scripts/build-filelist.mjs)

### 自定义样式和脚本

如果 NodeGet 主题从官方主题改版而来，那么会遵守开发习惯，引入下面的两个自定义文件，方便用户进行快捷的修改。

```
├── custom.css
└── custom.js
```

## 环境变量

NodeGet 公共展示页面使用环境变量 `NODEGET_CONFIG` 来生成 config.json

如果没有检测到这个环境变量，那么我们会使用 nodeget-theme.json 中的 user_preferences_form 的默认value来生成 config.json

如果都没有，那么会生成一个较为简易的 config.json 模板

合理利用环境变量可以做到很多事情，主要有两种用途

在从自己的GitHub自动化部署到 cloudflare pages等静态托管过程中，不需要硬编码配置信息，而是利用cloudflare pages的编译过程自动产生
config.json

在本地修改、开发主题的时候，创建 .env.local 来登记自己后端的信息，便于调试和预览，.env.local不会进入GitHub repo和编译结果

## user_preferences_form

nodeget-theme.json 的 user_preferences_form 属性主要用于控制面板上调整用户设置，生成对应的表单界面

这个属性借鉴自 komari 项目，在此表示感谢

注意，即使没有user_preferences_form字段（甚至是nodeget-theme.json文件），config.json
文件仍然会起作用，user_preferences_form只是方便通过UI来生成config.json，降低用户实用的门槛。

er_preferences_form的作用只有在dashboard辅助用户覆写user_preferences

以下信息节选自 komari 主题章节，基本保持兼容。

```json
{
  "user_preferences_form": {
    "items":  [
      { "key": "switch_A", "name": "测试开关", "type": "switch", "default": true, "help": "这是一个测试开关" },
      { "key": "select_A", "name": "测试选择", "type": "select", "options": "选项1,选项2,选项3", "default": "选项1", "help": "这是一个测试选择" },
      { "key": "number_A", "name": "测试输入框(数字)", "type": "number", "default": 10, "help": "这是一个测试输入框" },
      { "key": "string_A", "name": "测试输入框2", "type": "string", "required": true, "help": "这是一个测试输入框" }
    ]
  }
}

```

### 配置项 (user_preferences_form.items)

| 字段         | 适用类型      | 必需 | 描述                                                  |
|------------|-----------|----|-----------------------------------------------------|
| `type`     | 全部        | 是  | `string` / `number` / `select` / `switch` / `title` |
| `name`     | 全部        | 是  | 显示名称；`title` 类型用于分组标题，不需要 `key`                     |
| `key`      | 除 `title` | 是  | 唯一键                                                 |
| `required` | `string`  | 否  | 是否必填（默认 `false`）                                    |
| `options`  | `select`  | 是  | 逗号分隔的选项：`"A,B,C"`                                   |
| `default`  | 除 `title` | 否  | 默认值                                                 |
| `help`     | 除 `title` | 否  | 帮助提示文本                                              |

### 类型含义

- `string`: 文本输入。
- `number`: 数字输入，前端需自行校验范围。
- `select`: 下拉选择，`options` 为必填。
- `switch`: 布尔开关，值为 `true/false`。

## NodeGet 主题的前端工程化

如果没有特别需求，建议在 NodeGet 官方[演示主题](https://github.com/NodeSeekDev/NodeGet-StatusShow)的基础上定制开发

这里记录下官方演示主题的一些工程化设计，方便开发者理解并参与开发

### vite.config.ts

```ts
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { readFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const pkg = JSON.parse(
  readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), 'package.json'), 'utf8'),
)

export default defineConfig({
  plugins: [react()],
  base: './',
  envPrefix: ['NODEGET_'],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
})

```

要点：

- `NODEGET_` 开头的环境变量会被注入，扩展了默认只注入 VITE_*的设定
- `__APP_VERSION__` 作为全局变量注入，与 package.json 中的 version 同步
- 除了这里，项目所有的版本字段都与 package.json 中的 version 同步，避免碎片化问题

### package.json

```jsonc
{
  "name": "nodeget-statusshow",
  "private": true,
  "version": "1.4.3",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    // 重要，NodeGet会在编译结束后，自动进行一系列的后处理
    "postbuild": "node scripts/build-template-config.mjs && node scripts/build-filelist.mjs && node scripts/build-zip.mjs && node scripts/build-config.mjs",
    "preview": "vite preview",
    "typecheck": "tsc -p tsconfig.json"
  }
}
```

涉及到的 postbuild 后处理有

```sh
ls scripts/
# 结果有
build-template-config.mjs # 填充version字段，并在dist目录生成简单的 nodeget-theme.json + config.json
build-filelist.mjs # 扫描dist目录，生成文件列表 nodeget-theme-files.json
build-zip.mjs      # 生成一个dist目录的打包zip文件，方便分发用
build-config.mjs   # 如果检测到环境变量，重新生成并覆盖 config.json
```

### public/download.html

这是个辅助下载用的网页，当看到任何从官方主题派生的主题时，可以打开其 /download.html 页面，即可抽提出主题文件

原理是加载 nodeget-theme-files.json ，下载并打包为zip

### .env.example

本地开发时，复制为 .env.local