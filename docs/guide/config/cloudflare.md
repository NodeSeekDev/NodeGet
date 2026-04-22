# cloudflare配置

比较标准的配置方法是在本地通过http网关程序（如nginx）配置服务器的ssl证书，可以是自签名证书，然后在cloudflare上将 SSL/TLS encryption mode 设为 strict。但很多用户想跳过ssl证书的搭建，这里也给出相关的办法

可以直接将SSL/TLS encryption mode 设为 Flexible；也可以在Rules / Configuration Rules 这里增加一个主控websocket域名的匹配规则，设置SSL为Flexible，这种影响范围更加准确。

需要增加一条Origin Rule，匹配主控websocket域名，将端口重写为你设置的nodeget-server监听端口（默认端口2211）