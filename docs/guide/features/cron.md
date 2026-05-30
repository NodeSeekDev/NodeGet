# 定时任务

定时任务含义就是字面意思，定时触发某些任务。

## 定时任务类型

定时任务的类型包括：

- Agent 任务，展开包括：
    - ICMP Ping / TCP Ping / HTTP Ping
    - HTTP Request
    - DNS 查询
    - WebShell
    - Exec（执行命令）
    - 读取配置
    - 写入配置
    - 获取 IP
    - 获取版本信息
    - 自更新
- Server 任务：仅支持 Js Worker 形式（`JsWorker(String, Value)`），即执行某个 Js Worker 的 `onCron` 函数。如需数据清理等预设行为，应通过 Js Worker 的 `onCron` 实现。

如果你需要某个复杂的预设行为定期执行，比如定期查看域名是否过期，如果过期发送 Telegram 通知，或者定期总结服务器的异常，定期检查
IP 质量并报告等等。
那么可以利用 Js Worker 将一系列行为写到代码预设中，并利用定时任务定期触发。

## 定时任务与延迟曲线

NodeGet 并没有专门的全局统一的延迟探针功能，延迟曲线的实现是利用了 Cron 任务来达成。

具体来说，延迟曲线功能的工作方式是，获取所有由定时任务触发的 Ping/TCP Ping 任务结果，绘制到曲线图上。

所以当你添加一个定期执行的 Ping/TCP Ping 时，就是创建了一个延迟曲线信源。