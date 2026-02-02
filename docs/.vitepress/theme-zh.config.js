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
	],

	sidebar: {
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
				}]
			}]
		}]
	},
	socialLinks: [{
		icon: 'github',
		link: 'https://github.com/NodeSeekDev/NodeGet'
	}]
}