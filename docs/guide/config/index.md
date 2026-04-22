# 配置文件

Nodeget 的主控和被控均使用 Toml 作为配置文件格式，请事先了解 Toml 语法规范: <https://toml.io/cn/>

`nodeget-server` 使用子命令启动：

```bash
# 正常启动服务
nodeget-server serve -c ./config.toml

# 仅初始化数据库与 supertoken（若已存在则跳过），然后退出
nodeget-server init -c ./config.toml

# 删除旧 supertoken 并重新生成 id=1 supertoken（会交互确认 y/n）
nodeget-server roll-super-token -c ./config.toml
```

`-c/--config` 为必填参数；若配置文件无法读取将会 Panic 退出。

## 详细的配置参考
- [Agent](./agent.md)
- [Server](./server.md)
