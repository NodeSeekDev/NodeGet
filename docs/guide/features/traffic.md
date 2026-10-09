# 流量统计

按任意时间段查询设备用了多少流量，比如查本月流量、或按 VPS 商家的计费周期对账。

## 原理

只统计**出口网卡**（真正连接外网的网卡），容器、WireGuard/Tailscale 这类隧道网卡默认不计入，避免重复统计。服务端在每次收到 Agent 上报时，
在内存里累加每块网卡的总流量（定时写入数据库）。网卡计数器因重启等原因被重置时会自动识别、正确累加，不会漏算也不会重复算；重置后如果中断超过
10 分钟，会额外记一段"这段时间统计可能不准"的提示。

"某段时间用了多少流量"靠**总流量快照**算出来：结束时刻的快照 − 开始时刻的快照。快照由一个 **JS Worker**（NodeGet-Bootstrap 里的
`traffic-snapshot-worker`）定时写入，服务端本身只提供总流量计算和读写快照的接口，不自己生成快照。Worker 默认每 15 分钟（UTC 整点对齐）存一次，
同时负责清理过期快照。

出口网卡可以自动识别，也可以在 Agent 配置里通过 [`dynamic_summary_select_network_interface`](/guide/config/agent)
手动指定，例如只统计公网网卡、排除内网专线。

## 怎么查

通过 [`agent_query_traffic`](/api/monitoring/query#query-traffic) 接口按 UUID、时间段查询，返回每块网卡的流量合计，或者逐条快照明细，
也可以用 `range` 查出某台设备从什么时候起有快照数据。

## 配置

Worker 的配置放在 Kv 的 `global` 命名空间里，不配置时使用默认值，不合法的值会回退为默认值（并在 Server 日志里记一条警告）。详见
[特殊 Kv 与特殊键](/api/kv/special#server-namespace)。

| 键 | 含义 | 默认值 |
|---|---|---|
| `traffic_snapshot_interval` | 快照间隔，毫秒，必须是 60000（1 分钟）的整数倍 | 900000（15 分钟） |
| `database_limit_traffic_snapshot` | 快照保留时长，毫秒，至少 1 小时 | 31536000000（365 天） |

例如把间隔改成 5 分钟，保留 90 天：

```json
{ "namespace": "global", "key": "traffic_snapshot_interval", "value": 300000 }
{ "namespace": "global", "key": "database_limit_traffic_snapshot", "value": 7776000000 }
```

快照间隔越短，同一个时间点算出来的流量越精确，占用的空间也越大。间隔改动只影响之后新存的快照，已经存下的不会变。

## 注意事项

- **必须安装并运行流量统计 Worker**。没有 Worker 就没有快照，查不到任何时间段的流量。用 NodeGet-Bootstrap 安装的服务端会自动装上它，
  没有用 Bootstrap 的需要自行安装 `traffic-snapshot-worker`
- 装 Agent 之前的流量算不进来：第一次收到某块网卡的上报时只记下计数器，不计入之前的流量，从那一刻起才开始统计。查询的开始时间早于设备开始统计的时间时，
  会返回"找不到起点快照"的错误，可以先用 `range` 查出能查的最早时间
- 快照是定时存的，查询的时间点会对齐到最近的一条快照；要最新的总流量用 [`agent_query_traffic_current`](/api/monitoring/query#query-traffic-current)
- 快照会由 Worker 按保留时长清理，不用手动处理
