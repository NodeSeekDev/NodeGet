# 命令行参数

`nodeget-server`/`nodeget-agent` 的命令行参数较为简单，大部分参数都在配置文件中

## `nodeget-server` 参数

```
./nodeget-server -h
Usage: nodeget-server <COMMAND>

Commands:
    serve             Start server normally.
    init              Initialize database and super token, then exit.
    roll-super-token  Rotate the super token (id = 1) after interactive confirmation, then exit.
    get-uuid          Print server UUID from config and exit.
    version           Print version and exit.

./nodeget-server serve -h
Usage: nodeget-server serve --config <CONFIG>

Options:
  -c, --config <CONFIG>
```

## `nodeget-agent` 参数

```
./nodeget-agent -h
Usage: nodeget-agent [OPTIONS]

Options:
  -c, --config <CONFIG>
          [default: config.toml]

  -v, --version
          [default: false]

  -d, --dry-run
          [default: false]

  使用示例:

  ```shell
  # 验证 Agent 的本地监控功能
  ./nodeget-agent -c config.toml --dry-run
  ```

`--dry-run` 模式会执行一次静态和动态监控数据采集并打印输出结果，用于在连接 Server 前验证 Agent 的监控功能是否正常工作，输出完成后进程退出。
