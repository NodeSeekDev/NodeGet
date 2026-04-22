# NodeGet 手动二进制部署指南

对于想要折腾一遍手动安装过程的用户，这里提供一些简要的安装参考意见

NodeGet 的 [GitHub releases](https://github.com/NodeSeekDev/NodeGet/releases) 提供了是可直接运行的静态编译的二进制程序

你可以下载和你系统相匹配的版本，根据 [参数和配置](/guide/config/)一章提供的配置模板编写配置文件，然后启动程序指定配置文件


```shell
./nodeget serve -c path-to-config.toml
```

屏幕打印出的下面的Token和密码都只显示一次，请及时保存

为了保持程序异常退出和开机后能够自动重启，需要守护进程。可以用系统自带的 systemd 等进程管理器，也可以用 supervisor 等方案


如果你遇到了问题，日志是最好的朋友