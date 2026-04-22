# 主题开发

主题文件为纯静态页面，可以部署到 cloudflare pages / github page / 腾讯 eo 等静态文件托管服务

建议将受限token直接写入到某个配置文件（如 config.json)

参考配置

```json
{
    "site_name":"",
    "site_log":"",
    "theme_name":"",
    "theme_repo":"",
    "theme_config":{

    },
    "site_tokens":[
        {
            "name":"master server node 1",
            "token":"Your Token"
        },
        {
            "name":"master server node 2",
            "token":"Your Token"
        }
    ]
}
```

建议为每个静态主题的GitHub增加`部署到Cloudflare`的便捷部署按钮

主题的推荐使用方式是：

fork到自己的GitHub，修改config.json然后点击部署到cf

或者下载GitHub，修改config.json，手动上传


## 优秀主题汇总

占位下，优秀的主题会被整理一份到这里