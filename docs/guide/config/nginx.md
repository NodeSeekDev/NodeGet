---
outline: deep
---

# nginx配置

待补充

```
location / {
    proxy_pass http://127.0.0.1:2211;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "Upgrade";
    proxy_set_header Host $host;
}

```