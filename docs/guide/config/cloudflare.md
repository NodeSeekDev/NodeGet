# Cloudflare 配置

比较标准的配置方法是在本地通过 HTTP 网关程序（如 Nginx）配置服务器的 SSL 证书，可以是自签名证书，然后在 Cloudflare 上将
SSL/TLS encryption mode 设为 `full`。但很多用户想跳过 SSL 证书的搭建，这里也给出相关的办法

可以直接将 `SSL/TLS encryption mode` 设为 `Flexible`；也可以在 `Rules / Configuration Rules` 这里增加一个主控 WebSocket
域名的匹配规则，设置 SSL 为 `Flexible`，这种影响范围更加准确。

需要增加一条 `Origin Rule`，匹配主控 WebSocket 域名，将端口重写为你设置的 `nodeget-server` 监听端口（默认端口 `2211`）
