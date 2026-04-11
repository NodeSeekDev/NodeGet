import { defineConfig } from 'vitepress'
import { themeZhConfig } from './theme-zh.config.js'
import { themeEnConfig } from './theme-en.config.js'

// https://vitepress.dev/reference/site-config
export default defineConfig({
    title: "NodeGet",
    description: "Next-generation server monitoring and management tools",
    head: [
        ['link', { rel: 'icon', href: '/logo.png' }]
    ],
    themeConfig: themeZhConfig,
    locales: {
        root: {
            label: '中文',
            lang: 'zh'
        },
        en: {
            label: 'English',
            lang: 'en',
            themeConfig: themeEnConfig
        }
    }
})
