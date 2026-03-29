export const themeZhConfig = {
	logo: '/logo.png',
	nav: [{
		text: '主页',
		link: '/'
	},
	{
		text: '快速上手',
		link: '/guide/quick-start'
	},
	{
		text: '配置指南',
		link: '/guide/config/index.md'
	},
	{
		text: 'API',
		link: '/api/index.md'
	},
	],

	sidebar: {
		'/guide/config/': [{
			text: '配置指南',
			items: [{
				text: '概览',
				link: '/guide/config/index.md'
			},
			{
				text: 'Server 配置',
				link: '/guide/config/server.md'
			},
			{
				text: 'Agent 配置',
				link: '/guide/config/agent.md'
			}]
		}],
		'/api/': [{
			text: 'API 文档',
			items: [{
				text: '概览',
				link: '/api/index.md'
			},
			{
				text: '项目框架',
				link: '/api/framework.md'
			},
			{
				text: '错误处理',
				link: '/api/errors.md'
			},
			// Nodeget
			{
				text: 'Nodeget',
				collapsed: false,
				items: [{
					text: '介绍',
					link: '/api/nodeget/index.md'
				},
				{
					text: 'Hello',
					link: '/api/nodeget/hello.md'
				},
				{
					text: 'Version',
					link: '/api/nodeget/version.md'
				},
				{
					text: 'Server UUID',
					link: '/api/nodeget/uuid.md'
				},
				{
					text: '列出所有 Agent UUID',
					link: '/api/nodeget/list_all_agent_uuid.md'
				},
				{
					text: '读取 Server 配置',
					link: '/api/nodeget/read_config.md'
				},
				{
					text: '编辑 Server 配置',
					link: '/api/nodeget/edit_config.md'
				}]
			},

			// Monitoring
			{
				text: 'Monitoring',
				collapsed: false,
				items: [{
					text: '介绍',
					link: '/api/monitoring/index.md'
				},
				{
					text: 'Agent API',
					link: '/api/monitoring/agent.md'
				},
				{
					text: '查询 Monitoring API',
					link: '/api/monitoring/query.md'
				}]
			},
			// Task
			{
				text: 'Task',
				collapsed: false,
				items: [{
					text: '介绍',
					link: '/api/task/index.md'
				},
				{
					text: 'Agent API',
					link: '/api/task/agent.md'
				},
				{
					text: '创建 Task',
					link: '/api/task/create.md'
				},
				{
					text: '查询 Task API',
					link: '/api/task/query.md'
				}]
			},
			// Terminal
			{
				text: 'Terminal',
				collapsed: false,
				items: [{
					text: '介绍',
					link: '/api/terminal/index.md'
				},
				{
					text: 'Agent API',
					link: '/api/terminal/agent.md'
				},
				{
					text: '用户调用 Demo',
					link: '/api/terminal/user.md'
				}]
			},
			// Token
			{
				text: 'Token',
				collapsed: false,
				items: [{
					text: '介绍',
					link: '/api/token/index.md'
				},
				{
					text: '创建 Token',
					link: '/api/token/create.md'
				},
				{
					text: '获取 Token 信息',
					link: '/api/token/get.md'
				},
				{
					text: '列出所有 Token',
					link: '/api/token/list_all_tokens.md'
				},
				{
					text: '编辑 Token 权限',
					link: '/api/token/edit.md'
				},
				{
					text: '删除 Token',
					link: '/api/token/delete.md'
				}]
			},
			// Crontab
			{
				text: 'Crontab',
				collapsed: false,
				items: [{
					text: '介绍',
					link: '/api/crontab/index.md'
				},
				{
					text: '创建 Crontab',
					link: '/api/crontab/create.md'
				},
				{
					text: '编辑 Crontab',
					link: '/api/crontab/edit.md'
				},
				{
					text: '读取 Crontab',
					link: '/api/crontab/get.md'
				},
				{
					text: '删除 Crontab',
					link: '/api/crontab/delete.md'
				},
				{
					text: '切换启用状态',
					link: '/api/crontab/toggle_enable.md'
				},
				{
					text: '设置启用状态',
					link: '/api/crontab/set_enable.md'
				}]
			},
			// CrontabResult
			{
				text: 'CrontabResult',
				collapsed: false,
				items: [{
					text: '介绍',
					link: '/api/crontab_result/index.md'
				},
				{
					text: '查询 CrontabResult',
					link: '/api/crontab_result/query.md'
				},
				{
					text: '删除 CrontabResult',
					link: '/api/crontab_result/delete.md'
				}]
			},
			// JsWorker
			{
				text: 'JsWorker',
				collapsed: false,
				items: [{
					text: '介绍',
					link: '/api/js_worker/index.md'
				},
				{
					text: '创建脚本',
					link: '/api/js_worker/create.md'
				},
				{
					text: '读取脚本',
					link: '/api/js_worker/read.md'
				},
				{
					text: '更新脚本',
					link: '/api/js_worker/update.md'
				},
				{
					text: '删除脚本',
					link: '/api/js_worker/delete.md'
				},
				{
					text: '运行脚本',
					link: '/api/js_worker/run.md'
				},
				{
					text: 'HTTP 路由绑定',
					link: '/api/js_worker/route.md'
				},
				{
					text: '列出可见脚本',
					link: '/api/js_worker/list_all_js_worker.md'
				},
				{
					text: 'Runtime 池信息',
					link: '/api/js_worker/get_rt_pool.md'
				},
				{
					text: '脚本编写规范',
					link: '/api/js_worker/script.md'
				},
				{
					text: '外部注入能力',
					link: '/api/js_worker/injected.md'
				}]
			},
			// JsResult
			{
				text: 'JsResult',
				collapsed: false,
				items: [{
					text: '介绍',
					link: '/api/js_result/index.md'
				},
				{
					text: '查询执行结果',
					link: '/api/js_result/query.md'
				},
				{
					text: '删除执行结果',
					link: '/api/js_result/delete.md'
				}]
			},
			// KV
			{
				text: 'KV',
				collapsed: false,
				items: [{
					text: '介绍',
					link: '/api/kv/index.md'
				},
				{
					text: '创建命名空间',
					link: '/api/kv/create_namespace.md'
				},
				{
					text: '列出所有命名空间',
					link: '/api/kv/list_all_namespace.md'
				},
				{
					text: '批量读取键值',
					link: '/api/kv/get_multi_value.md'
				},
				{
					text: '增删改查',
					link: '/api/kv/crud.md'
				},
				{
					text: '特殊 Kv',
					link: '/api/kv/special.md'
				}]
			}]
		}]
	},
	socialLinks: [{
		icon: 'github',
		link: 'https://github.com/NodeSeekDev/NodeGet'
	}]
}
