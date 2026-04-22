# NodeGet 插件机制

NodeGet 是一个具有细粒度 token 模型的软件，对于不同的操作定义了详细的权限限制

为了方便基于 NodeGet 的 api 和 token 设计的辅助程序，官方提供一个将程序嵌入到 dashboard 上的方法

这个工作其实并不复杂，原则上相当于在 server 上实现了一个静态文件储存器

前端通过iframe加载对应的储存器，然后支持注册到 dashboard 的路由上即可


## 先说场景

在介绍具体的插件工作原理前，先说一个具体的插件的目录结构和文件内容，有一个大体的印象

### 文件结构

```
demo-extension
├── app.json
├── readme.md
├── resources
│   ├── assets
│   │   ├── icon.svg
│   │   ├── main.js
│   │   ├── route-icon.svg
│   │   └── style.css
│   └── index.html
└── worker.js
```

### app.json

```json
{
    "name":"my-extenstion",
    "icon":"assets/icon.svg",
    "routes":[
        {
            "type":"node",
            "name":"extension-test-1",
            "icon":"assets/route-icon.svg",
            "entry":"index.html"
        },
        {
            "type":"global",
            "name":"extension-test-2",
            "icon":"assets/route-icon.svg",
            "entry":"index.html"
        }
    ],
    "limits":[
        // 这里由用户输入，token所需要的权限列表
    ],
    // 非关键字段，仅记录用
    "version": "1.0",
    "description": "Just an extension demo.",
    "extension_version": 1,
    "author":"",
    "repository":"",
    "homepage":"",
    "license":"",
}
```

##  插件安装流程介绍

插件的安装流程很简单，简述如下
- 用户在前端的插件安装界面打开本地文件夹或者zip文件
- 获取到所有文件，读取app.json，弹出确认创建指定权限token的提示框
- 用户确认后**创建对应的token**
- 生成一个随机的uuid代表扩展的id，这里暂定为 Extension_UUID
- 把resource目录下的静态文件利用后文提到的**静态文件服务上传**到 `https://主控域名/worker-route/static-worker-route/Extension_UUID/` 路由下
- 动态注册前端路由
- 把 app.json 和 Extension_UUID， token等信息**储存到kv内**，namaspace为extension-infomation，key是Extension_UUID

## 静态文件托管worker
为了实现静态文件托管到kv，实现了一个简易的js扩展worker，名称为static-worker，这个worker会在添加server时预装

下面是这个worker暴露的接口

```js
const resourceURL = 'https://WS_HOST/worker-route/static-worker-route/{Extension_UUID}/hello';

// 储存静态资源
fetch(resourceURL,{
    method:'POST',
    body:'你好世界',
    headers:{
        // Bearer + nodeget token
        'Authorization':'Bearer Token'
    }
}).then(r => r.text()).then(console.log)

// 获取静态资源(无需token)
fetch(resourceURL).then(r => r.text()).then(console.log)
```

因此，在文件上传完成后，其实只是在网页上开一个iframe，类似于：
```html
<iframe src="https://WS_HOST/worker-route/static-worker-route/Extension_UUID/{entry}#?token={}&node=${AGENT_UUID}&theme={theme}"></iframe>
```


## 插件开发及分发

插件开发时应该尽量使用相对路径，比如在vite开发时将base设为"./"

这里提供一个 demo-extension 的vite项目

分发时可以提供打包好的文件的zip包