# 用户终端使用

若要使用 WebShell，前端应该会提供对应界面，下面是由 AI 生成的示例代码，输入 Url 即可连接到终端:

```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <title>NodeGet Web Terminal</title>
    <!-- 引入 Xterm.js CSS -->
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/xterm@5.3.0/css/xterm.min.css">
    <style>
        body { margin: 0; background: #000; color: #fff; font-family: monospace; overflow: hidden; }
        #controls { padding: 10px; background: #222; display: flex; gap: 10px; }
        input { flex: 1; padding: 5px; background: #333; color: #fff; border: 1px solid #555; }
        button { padding: 5px 15px; cursor: pointer; background: #007bff; color: #fff; border: none; }
        button:hover { background: #0056b3; }
        #terminal-container { height: calc(100vh - 50px); width: 100vw; }
    </style>
</head>
<body>

<div id="controls">
    <input type="text" id="wsurl" placeholder="ws://host:port/terminal?agent_uuid=xxx&terminal_id=xxx&token=xxx" value="">
    <button id="connect-btn">连接终端</button>
</div>
<div id="terminal-container"></div>

<!-- 引入 Xterm.js 核心库及插件 -->
<script src="https://cdn.jsdelivr.net/npm/xterm@5.3.0/lib/xterm.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/xterm-addon-fit@0.8.0/lib/xterm-addon-fit.min.js"></script>

<script>
    const termContainer = document.getElementById('terminal-container');
    const wsUrlInput = document.getElementById('wsurl');
    const connectBtn = document.getElementById('connect-btn');

    let socket;
    let term;
    let fitAddon;

    // 1. 初始化终端
    function initTerminal() {
        term = new Terminal({
            cursorBlink: true,
            fontFamily: '"Cascadia Code", Menlo, monospace',
            fontSize: 14,
            theme: { background: '#000000' }
        });

        fitAddon = new FitAddon.FitAddon();
        term.loadAddon(fitAddon);
        term.open(termContainer);
        fitAddon.fit();

        window.addEventListener('resize', () => fitAddon.fit());
    }

    // 2. 连接 WebSocket
    function connect() {
        const url = wsUrlInput.value;
        if (!url) return alert("请输入 WebSocket URL");

        if (socket) socket.close();
        if (term) term.dispose();

        initTerminal();
        term.write('正在连接到服务器...\r\n');

        socket = new WebSocket(url);
        // 重要：PTY 数据通常是二进制流
        socket.binaryType = 'arraybuffer';

        socket.onopen = () => {
            term.write('\x1b[1;32m连接成功！\x1b[0m\r\n');
            
            // 监听键盘输入并发送给 Server -> Agent
            term.onData(data => {
                if (socket.readyState === WebSocket.OPEN) {
                    socket.send(data);
                }
            });
        };

        socket.onmessage = (event) => {
            // 将 Agent 返回的数据写入终端
            if (event.data instanceof ArrayBuffer) {
                term.write(new Uint8Array(event.data));
            } else {
                term.write(event.data);
            }
        };

        socket.onclose = (e) => {
            term.write(`\r\n\x1b[1;31m连接已断开 [代码: ${e.code}]\x1b[0m\r\n`);
        };

        socket.onerror = (err) => {
            term.write('\r\n\x1b[1;31mWebSocket 错误发生\x1b[0m\r\n');
            console.error(err);
        };
    }

    connectBtn.addEventListener('click', connect);

    // 默认展示提示信息
    window.onload = () => {
        initTerminal();
        term.write('请输入 WebSocket URL 并点击连接。\r\n');
    };
</script>
</body>
</html>
```
