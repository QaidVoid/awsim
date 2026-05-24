import DefaultTheme from 'vitepress/theme'
import type { Theme } from 'vitepress'
import HomeLayout from './HomeLayout.vue'
import './style.css'

export default {
  extends: DefaultTheme,
  Layout: HomeLayout,
} satisfies Theme
