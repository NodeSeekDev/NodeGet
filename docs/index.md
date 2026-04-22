---
# https://vitepress.dev/reference/default-theme-home-page
layout: home

hero:
  name: "NodeGet"
  text: 下一代服务器监控管理工具
  tagline: 极致的自由度，限制你的玩法的只有想象力
  actions:
    - theme: brand
      text: 快速开始
      link: /guide/quick-start
    - theme: alt
      text: API文档
      link: /api
  image:
    src: /logo.png
    alt: NodeGet

features:
  - title: 多主控模型
    details: agent原生支持多主控并存，dashboard原生支持多主控切换，以此为基础驱动负载均衡与数据安全（冗余备份）
  - title: 前后端完全分离，headless模式
    details: 彻底的前后端分离，所有操作都有api接口，允许前端开发者自由构建展示页面和控制面板，并部署到 cloudflare pages / github page / eo等静态储存
  - title: 细粒度权限模型
    details: 支持基于token的细粒度权限模型，大大增强安全性和灵活性，方便与其三方系统对接
  - title: 基于 js worker 的 serverless 方案
    details: 内嵌 js worker（体积极小）与 kv，允许通过自由组合NodeGet提供的基础能力来实现各种神奇的操作
  - title: 支持加载自定义插件
    details: 允许用户安装自定义插件，直接集成到dashboard，此功能与细粒度权限模型相辅相成
  - title: 卓越的性能
    details: rust语言内存模型更加安全，在关键的性能瓶颈做了巧妙的性能优化，大大提高了工程吞吐量上限
---

<style>
:root {
    --vp-home-hero-name-color: transparent;
    --vp-home-hero-name-background: linear-gradient(120deg, #b710b0, #fbc614);
    --vp-home-hero-image-background-image: linear-gradient(300deg, #b710af6c, #fbc5147d);
    --vp-home-hero-image-filter: blur(65px);
}
.main .heading .text {
  font-size:32px;
}
.main .tagline {
  font-size:22px;
  padding-top:0;
}
</style>