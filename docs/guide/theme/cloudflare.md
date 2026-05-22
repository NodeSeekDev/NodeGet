# Cloudflare部署主题

NodeGet 推荐有折腾能力的用户使用cloudflare pages来部署主题，这有非常多的好处，包括：

- cloudflare pages具有网络加速和攻击防御能力
- cloudflare pages可以多开，因此可以同时部署多个主题
- cloudflare pages部署的主题，天然可以提供给其他人作为远程主题分发服务

## 部署步骤

首先需要说明的是，推荐的是利用 cloudflare pages而非cloudflare workers

cloudflare pages的入口如下图所示

![6I8e9rwpwUpGyYPsWtXJGDea9PyXAZA5.webp](https://cdn.nodeimage.com/i/6I8e9rwpwUpGyYPsWtXJGDea9PyXAZA5.webp)

点击后会进入到一个选择界面，下面将分别讲述利用github导入和利用文件上传导入的办法
![rl4GJMZRMNZtgXmxvJbQ9EIQhkZa47Wv.webp](https://cdn.nodeimage.com/i/rl4GJMZRMNZtgXmxvJbQ9EIQhkZa47Wv.webp)

## 利用文件上传

标准的步骤是，先下载主题作者提供的编译好的静态文件，比如[官方的这个](https://nodeget.pages.dev/NodeGet-StatusShow.zip)，然后修改压缩文件里 config.json 中的配置，最后上传。这对一些未适配 新的 NodeGet 主题规范的历史主题仍然有效。 

但如果你的主题适配了 NodeGet 主题规范，这里有个更加完美的部署方式，先利用[一键部署](./quick-install.md)部署到控制面板，然后利用下图所示的下载压缩包功能直接下载完善的配置好了token的压缩包，将其上传到 pages里面即可，非常简单。

![qxkhl06ImkWg0Jz1aRiuU5YhFYYF7J7q.webp](https://cdn.nodeimage.com/i/qxkhl06ImkWg0Jz1aRiuU5YhFYYF7J7q.webp)

补充说明：如果你有定制css/js的需求，可以在控制面板定制完后，再导出压缩包。


## 利用Github上传

这种方式更加适合有折腾能力的用户，以及主题开发者

点击通过git部署后，选择你的GitHub项目（可以fork主题开发者提供的GitHub项目）

![qvLFPg2gLAAUMtodjEFZdqqSf1x42Xjt.webp](https://cdn.nodeimage.com/i/qvLFPg2gLAAUMtodjEFZdqqSf1x42Xjt.webp)

注意上面红框框起来的3个地方即可，这种方法也方便更新：

- 如果你是主题开发者，会自动更新
- 如果你fork了其他开发者的主题，到GitHub上点击同步上游代码即可自动更新