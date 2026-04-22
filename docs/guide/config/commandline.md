# 命令行参数

nodeget-server/agent 的命令行参数较为简单，大部分参数都在配置文件中

## nodeget-server参数

```
./nodeget-server -h
Usage: nodeget-server <COMMAND>

Commands:
    serve             Start server normally.
    init              Initialize database and super token, then exit.
    roll-super-token  Rotate the super token (id = 1) after interactive confirmation, then exit.
    get-uuid          Print server UUID from config and exit.

./nodeget-server serve -h
Usage: nodeget-server serve --config <CONFIG>

Options:
  -c, --config <CONFIG>
```

## nodeget-agent参数

```
./nodeget-agent -h
Usage: nodeget-agent --config <CONFIG>

Options:
  -c, --config <CONFIG>
```