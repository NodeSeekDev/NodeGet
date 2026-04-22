# NodeGet Worker

如果你用过cloudflare worker，那么对这个功能肯定很熟悉，NodeGet 的js worker可以粗略理解是其本地部署版本。

js worker是 NodeGet 实现很多自定义行为的基础， NodeGet 将所有api操作映射为内部函数，并提供了定时执行，绑定http路由，api调用执行等功能。

NodeGet js worker功能的前后端都经过了多次打磨与优化，可以提供畅快的开发及使用体验。