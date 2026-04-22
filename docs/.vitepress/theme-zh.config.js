import guideSideBarZh from './guide-sidebar-zh.config.js'
import devSideBarZh from './dev-sidebar-zh.config.js'

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
		text: 'API',
		link: '/api/index.md'
	},
	{
		text: 'Dev',
		link: '/dev/'
	},
	],

	sidebar: {
		'/guide/':guideSideBarZh,
		'/dev/':devSideBarZh,
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
					text: 'CRUD 操作',
					link: '/api/nodeget/crud.md'
				}]
			},

			// Monitoring
			{
				text: 'Monitoring',
				collapsed: false,
				items: [{
					text: '总览',
					link: '/api/monitoring/index.md'
				},
				{
					text: 'Agent 上报',
					link: '/api/monitoring/agent.md'
				},
				{
					text: '查询与删除',
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
					text: 'CRUD 操作',
					link: '/api/task/crud.md'
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
					text: 'CRUD 操作',
					link: '/api/token/crud.md'
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
					text: 'CRUD 操作',
					link: '/api/crontab/crud.md'
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
					text: 'CRUD 操作',
					link: '/api/crontab_result/crud.md'
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
					text: 'CRUD 操作',
					link: '/api/js_worker/crud.md'
				},
				{
					text: 'HTTP 路由绑定',
					link: '/api/js_worker/route.md'
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
					text: 'CRUD 操作',
					link: '/api/js_result/crud.md'
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
					text: 'CRUD 操作',
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
