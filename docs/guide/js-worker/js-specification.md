# js语法规范支持力度

NodeGet使用了rquickjs的rust库，这是一个[QuickJS-NG](https://quickjs-ng.github.io/quickjs/)的高级封装库

QuickJS 的目标之一是要求高度符合 ECMAScript 标准，目前的进展为：

- 基本完整实现 ES2020
- 覆盖大量 ES2021 / ES2022 特性
- 通过大部分 test262

支持的典型特性包括：

- async/await
- Promise
- Proxy
- BigInt
- 模块（ESM）
- generator / iterator
- 正则增强（named groups 等）
