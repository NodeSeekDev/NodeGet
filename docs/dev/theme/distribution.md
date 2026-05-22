# 主题分发服务

一个主题分发服务不只是静态文件下载，而且需要满足：

- 能够支持 CORS 跨域访问
- 满足 NodeGet 规范主题的文件结构要求
- 最好能够支持 IPv4/IPv6双栈访问

满足要求的主题分发域名可以利用下面这种快捷地址，直接部署到控制面板

<a href="https://dash.nodeget.com/#/dashboard/theme-management?add=https://nodeget.pages.dev">
  <img src="https://dash.nodeget.com/deploy-button.png" alt="deploy button" width="230px" />
</a>

```html
<a href="https://dash.nodeget.com/#/dashboard/theme-management?add=https://nodeget.pages.dev">
  <img src="https://dash.nodeget.com/deploy-button.png" alt="deploy button" width="230px" />
</a>
```

## cloudflare pages （最推荐）

如果你是 NodeGet 主题的创建者，想要把自己创建的主题分享给其他用户，最为推荐的部署方式是通过 cloudflare pages 分发主题

这样做有多个好处：

- cf pages提供了 pages.dev 的免费域名，因此没有域名续费压力
- cf pages 默认是允许CORS的，因此，可以直接被控制面板导入
- cf pages 默认完全托管在cloudflare上，服务稳定性高
- cf pages 可以与GitHub项目保持同步更新，自动编译最新版的镜像
- 支持 IPv4/IPv6 双栈，默认几乎无IP地址限制

需要注意的是，推荐使用 pages而非 worker，这在细节上有一些区别

创建 cf pages 的入口被刻意弱化，但是仍然能够找到：

![6I8e9rwpwUpGyYPsWtXJGDea9PyXAZA5.webp](https://cdn.nodeimage.com/i/6I8e9rwpwUpGyYPsWtXJGDea9PyXAZA5.webp)

## 后端域名传递

当通过控制面板的主题管理把其他主题导入到面板上时，自己也会成为一个主题分发服务，这在你通过custom.css/js自定义后提供给其他人时比较有用

## 其他静态托管
也可以放到 nginx 里面等等，需要开启CORS访问。